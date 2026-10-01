//! Document I/O and transactions: open/save/export by path or bytes (base64), and `command.batch`.
//!
//! These live in the engine so every frontend (UI, CLI, MCP headless) shares them.

use drawcraft_doc::Document;
use serde_json::{Value, json};

use super::*;
use crate::EngineError;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "document.open",
            "Open Document",
            [],
            None,
            "{path} or {name, dataBase64} → {index}; .drawcraft, .svg, .pdf, .ai (PDF-compatible)",
            always,
            open
        ),
        cmd!("document.save", "Save Document", [], None, "{path?} native .drawcraft (default: the document's path) → {path}", has_doc, save),
        cmd!(query "document.serialize", "Serialize Document", [], None, "{format: drawcraft|svg|pdf|png} → {dataBase64 | text}", has_doc, serialize),
        cmd!("document.export", "Export Document", [], None, "{format: svg|png|pdf|drawcraft, path, scale?: 1, artboard?: 0}", has_doc, export),
        cmd!(
            "document.exportForScreens",
            "Export for Screens",
            ["File", "Export"],
            None,
            "{folder, artboards?: [index…] (default all), formats?: [{format: png|jpg|webp|svg|pdf, scale?: 1, suffix?: \"@2x\"}], prefix?} → {files: [...]}",
            has_doc,
            export_for_screens
        ),
        cmd!(query "command.batch", "Batch", [], None, "{label?, commands: [{command, params}]} run several commands as ONE undo step; stops at the first error and rolls back", has_doc, batch),
    ]
}

fn load(name: &str, bytes: &[u8]) -> Result<Document> {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".drawcraft") || drawcraft_format::sniff(bytes) {
        return drawcraft_format::load(bytes).map_err(|e| EngineError::Other(e.to_string()));
    }
    if lower.ends_with(".svg") || bytes.starts_with(b"<?xml") || bytes.starts_with(b"<svg") {
        let s = std::str::from_utf8(bytes).map_err(|_| EngineError::Other("SVG is not UTF-8".into()))?;
        let mut d = drawcraft_svg::import(s).map_err(|e| EngineError::Other(e.to_string()))?;
        d.title = std::path::Path::new(name).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| name.to_string());
        return Ok(d);
    }
    if lower.ends_with(".pdf") || lower.ends_with(".ai") || bytes.starts_with(b"%PDF") {
        let r = drawcraft_pdf::import_with_report(bytes, &drawcraft_pdf::ImportOptions::default()).map_err(|e| EngineError::Other(e.to_string()))?;
        let mut d = r.document;
        d.title = std::path::Path::new(name).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| name.to_string());
        return Ok(d);
    }
    Err(EngineError::Other(format!("unsupported file `{name}`")))
}

fn open(s: &mut Session, p: &Value) -> Result<Value> {
    let (name, bytes, path) = match (str_param(p, "path"), str_param(p, "dataBase64")) {
        (Some(path), _) => (path.to_string(), std::fs::read(path).map_err(|e| EngineError::Other(format!("{path}: {e}")))?, Some(path.to_string())),
        (None, Some(b64)) => (
            str_param(p, "name").unwrap_or("untitled.drawcraft").to_string(),
            drawcraft_format::base64_decode(b64).ok_or_else(|| bad("document.open", "bad base64"))?,
            None,
        ),
        _ => return Err(bad("document.open", "give path or dataBase64")),
    };
    let doc = load(&name, &bytes)?;
    let native = name.to_ascii_lowercase().ends_with(".drawcraft");
    let i = s.add_document(doc, if native { path } else { None });
    Ok(json!({ "index": i }))
}

fn save(s: &mut Session, p: &Value) -> Result<Value> {
    let path = str_param(p, "path")
        .map(str::to_string)
        .or_else(|| s.active().and_then(|d| d.path.clone()))
        .ok_or_else(|| bad("document.save", "no path"))?;
    let bytes = drawcraft_format::save_file(&s.doc()?.doc);
    std::fs::write(&path, bytes).map_err(|e| EngineError::Other(format!("{path}: {e}")))?;
    let st = s.doc_mut()?;
    st.path = Some(path.clone());
    st.saved_revision = st.revision;
    Ok(json!({ "path": path }))
}

pub(crate) fn encode(s: &Session, format: &str, scale: f64, artboard: usize) -> Result<Vec<u8>> {
    let doc = &s.doc()?.doc;
    Ok(match format {
        "drawcraft" => drawcraft_format::save_file(doc),
        "svg" => drawcraft_svg::export(doc, &drawcraft_svg::ExportOptions { artboard: Some(artboard), ..Default::default() }).into_bytes(),
        "pdf" => drawcraft_pdf::export(doc, &drawcraft_pdf::PdfOptions::default()).map_err(|e| EngineError::Other(e.to_string()))?,
        "png" | "jpg" | "jpeg" | "webp" => {
            let r = doc.artboards.get(artboard).map(|a| a.rect).ok_or_else(|| EngineError::Other("no such artboard".into()))?;
            let img = drawcraft_render::Renderer::new().render_region(doc, r, scale.clamp(0.01, 64.0), format != "png" && format != "webp");
            match format {
                "png" => img.to_png(),
                "webp" => img.to_webp(),
                _ => img.to_jpeg(90),
            }
        }
        other => return Err(EngineError::Other(format!("unknown format `{other}`"))),
    })
}

fn serialize(s: &mut Session, p: &Value) -> Result<Value> {
    let f = str_param(p, "format").unwrap_or("drawcraft");
    let bytes = encode(s, f, f64_or(p, "scale", 1.0), p.get("artboard").and_then(Value::as_u64).unwrap_or(0) as usize)?;
    if f == "svg" {
        return Ok(json!({ "text": String::from_utf8_lossy(&bytes) }));
    }
    Ok(json!({ "dataBase64": drawcraft_format::base64_encode(&bytes) }))
}

fn export(s: &mut Session, p: &Value) -> Result<Value> {
    let path = str_param(p, "path").ok_or_else(|| bad("document.export", "missing path"))?;
    let f = str_param(p, "format")
        .map(str::to_string)
        .unwrap_or_else(|| std::path::Path::new(path).extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_else(|| "png".into()));
    let bytes = encode(s, &f, f64_or(p, "scale", 1.0), p.get("artboard").and_then(Value::as_u64).unwrap_or(0) as usize)?;
    std::fs::write(path, &bytes).map_err(|e| EngineError::Other(format!("{path}: {e}")))?;
    Ok(json!({ "path": path, "bytes": bytes.len() }))
}

fn batch(s: &mut Session, p: &Value) -> Result<Value> {
    let cmds = p.get("commands").and_then(Value::as_array).ok_or_else(|| bad("command.batch", "missing commands"))?.clone();
    let label = str_param(p, "label").unwrap_or("Batch").to_string();
    s.begin_interaction(&label)?;
    // Session-level state a step may change (paint defaults, drawing mode) rolls back too.
    let saved = (s.paint.clone(), s.fill_active, s.draw_mode, s.draw_inside, s.clipboard.clone());
    let mut results = vec![];
    for c in &cmds {
        let id = c.get("command").and_then(Value::as_str).unwrap_or("");
        if id == "command.batch" {
            s.cancel_interaction()?;
            return Err(bad("command.batch", "batches cannot nest"));
        }
        let params = c.get("params").cloned().unwrap_or(json!({}));
        match s.execute(id, &params) {
            Ok(v) => results.push(v),
            Err(e) => {
                s.cancel_interaction()?;
                (s.paint, s.fill_active, s.draw_mode, s.draw_inside, s.clipboard) = saved;
                return Err(EngineError::Other(format!("batch step {} (`{id}`) failed: {e}", results.len())));
            }
        }
    }
    // Commit as one undo step (the interaction's "preview" is the batch itself).
    if let Some(it) = s.doc_mut()?.interaction.as_mut() {
        it.preview = Some(("command.batch".into(), p.clone()));
    }
    s.commit_interaction()?;
    Ok(json!({ "results": results }))
}

fn export_for_screens(s: &mut Session, p: &Value) -> Result<Value> {
    let folder = str_param(p, "folder").ok_or_else(|| bad("document.exportForScreens", "missing folder"))?.to_string();
    let n = s.doc()?.doc.artboards.len();
    let boards: Vec<usize> = match p.get("artboards").and_then(Value::as_array) {
        Some(a) => a.iter().filter_map(Value::as_u64).map(|v| v as usize).filter(|i| *i < n).collect(),
        None => (0..n).collect(),
    };
    let formats: Vec<(String, f64, String)> = match p.get("formats").and_then(Value::as_array) {
        Some(a) => a
            .iter()
            .map(|f| {
                let fmt = str_param(f, "format").unwrap_or("png").to_string();
                let sc = f64_or(f, "scale", 1.0);
                let suffix = str_param(f, "suffix")
                    .map(str::to_string)
                    .unwrap_or_else(|| if (sc - 1.0).abs() < 1e-9 { String::new() } else { format!("@{sc}x") });
                (fmt, sc, suffix)
            })
            .collect(),
        None => vec![("png".into(), 1.0, String::new())],
    };
    let prefix = str_param(p, "prefix").unwrap_or("").to_string();
    std::fs::create_dir_all(&folder).map_err(|e| EngineError::Other(format!("{folder}: {e}")))?;
    let mut files = vec![];
    for b in boards {
        let name: String =
            s.doc()?.doc.artboards[b].name.chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '-' }).collect();
        for (fmt, sc, suffix) in &formats {
            let bytes = encode(s, fmt, *sc, b)?;
            let ext = if fmt == "jpeg" { "jpg" } else { fmt.as_str() };
            let path = format!("{folder}/{prefix}{name}{suffix}.{ext}");
            std::fs::write(&path, &bytes).map_err(|e| EngineError::Other(format!("{path}: {e}")))?;
            files.push(path);
        }
    }
    Ok(json!({ "files": files }))
}
