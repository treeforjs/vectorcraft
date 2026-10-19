//! Brush libraries (Window → Brush Libraries, the Brushes panel's library button): read-only sets
//! of brushes the library panel shows. The built-in libraries are generated in
//! [`vectorcraft_brush::libs`]; User Defined ones are the `.vcbrushes` files in the user library
//! folder ([`Libraries`]), and Other Library… loads more from files. `brush.addFromLibrary` copies
//! brushes into the document; `brush.saveLibrary` writes the document's.

use std::collections::HashSet;
use std::sync::Arc;

use serde_json::{Value, json};
use vectorcraft_brush::libs::{self, BRUSH_LIBRARIES, BRUSHES_EXT, BrushLibrary, builtin_brush_library};
use vectorcraft_brush::{self as brush, Brush};

use super::menucmds::squash;
use super::swatch::str_list;
use super::swatchlib::{FileData, LibraryFile, LibraryInfo, stem};
use super::*;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            query "brush.libraries",
            "Brush Libraries",
            [],
            None,
            "{} → {libraries: [{id, name, category: \"builtIn\"|\"user\" (User Defined: the .vcbrushes files in the user library folder, rescanned now)|\"loaded\" (brush.loadLibrary), count, path?}], userFolder} every brush library the library panel opens, in menu order",
            always,
            list
        ),
        cmd!(
            query "brush.library",
            "Brush Library",
            [],
            None,
            "{library: id or name} → {id, name, category, brushes: [{name, type}], definitions?: [brush definitions as brush.get] (with definitions: true)}",
            always,
            get
        ),
        cmd!(
            "brush.addFromLibrary",
            "Add to Brushes",
            ["Window", "Brush Libraries"],
            None,
            "{library: id or name, name? | names?: [brush names in the library] (default: all of it), apply?: false (also apply the first one to the selected paths as brush.apply, or make it the current brush when nothing is selected), ids?} copy library brushes into the document's Brushes as one undo step (with the apply). A brush the document already has (same name and definition) isn't added again; a name taken by another brush gets a number (\"Stars 2\") → {library, added: [names in the document], existing: [names already there], applied?: name, current?: name}",
            has_doc,
            add
        ),
        cmd!(
            "brush.saveLibrary",
            "Save Brush Library…",
            ["Window", "Brush Libraries"],
            None,
            "{path?, names?: [brush names] (default: all), name?: library name (default: the document's), user?: false (save into the user library folder, listed under User Defined)} save the document's brushes as a .vcbrushes library (JSON brush definitions) → {path, count, library?: id when saved to the user folder}; without path or user → {data: the file's text, count}",
            has_doc,
            save
        ),
        cmd!(
            "brush.loadLibrary",
            "Other Library…",
            ["Window", "Brush Libraries"],
            None,
            "{path? | data?: file text | dataBase64?, name?: file name (default: the path's)} load a .vcbrushes library, or the brushes of any document VectorCraft opens (see document.formats), for the library panel (Window → Brush Libraries lists it until the app quits; a file of the user library folder, or a file with the same extension and bytes as one there, opens as that User Defined library) → {library: id, name, count}",
            always,
            load
        ),
    ]
}

impl LibraryFile for BrushLibrary {
    const EXTS: &'static [&'static str] = &[BRUSHES_EXT];
    fn read(bytes: &[u8], stem: &str) -> std::result::Result<Self, String> {
        libs::read(&String::from_utf8_lossy(bytes), stem)
    }
    fn name(&self) -> &str {
        &self.name
    }
}

/// User Defined and loaded brush libraries.
pub type Libraries = super::swatchlib::Libraries<BrushLibrary>;

fn builtin_info(b: &libs::BuiltinBrushLibrary) -> LibraryInfo {
    LibraryInfo { id: b.id.into(), name: b.name.into(), category: "builtIn" }
}

/// Every brush library, in menu order: built-in, User Defined, loaded.
pub fn libraries(s: &Session) -> Vec<LibraryInfo> {
    BRUSH_LIBRARIES.iter().map(builtin_info).chain(s.brush_libraries.infos().cloned()).collect()
}

/// Library `key` (an id, or a name in any case) with its info.
pub fn library(s: &Session, key: &str) -> Option<(LibraryInfo, Arc<BrushLibrary>)> {
    if let Some(found) = s.brush_libraries.get(key) {
        return Some(found);
    }
    if let Some(b) = BRUSH_LIBRARIES.iter().find(|b| b.id == key) {
        return Some((builtin_info(b), builtin_brush_library(key)?));
    }
    let id = libraries(s).into_iter().find(|l| l.name.eq_ignore_ascii_case(key))?.id;
    if id == key {
        return None;
    }
    library(s, &id)
}

fn library_param(s: &Session, p: &Value, cmd: &str) -> Result<(LibraryInfo, Arc<BrushLibrary>)> {
    let key = str_param(p, "library").ok_or_else(|| bad(cmd, "missing `library` (see brush.libraries)"))?;
    library(s, key).ok_or_else(|| bad(cmd, format!("no brush library `{key}` (see brush.libraries)")))
}

fn list(s: &mut Session, _: &Value) -> Result<Value> {
    s.brush_libraries.rescan();
    let libs: Vec<Value> = libraries(s)
        .into_iter()
        .map(|l| {
            let count = library(s, &l.id).map_or(0, |(_, lib)| lib.len());
            json!({"id": l.id, "name": l.name, "category": l.category, "count": count, "path": s.brush_libraries.path(&l.id)})
        })
        .collect();
    Ok(json!({ "libraries": libs, "userFolder": s.brush_libraries.user_dir() }))
}

fn get(s: &mut Session, p: &Value) -> Result<Value> {
    let (info, lib) = library_param(s, p, "brush.library")?;
    let brushes: Vec<Value> = lib.brushes.iter().map(|b| json!({"name": b.name, "type": b.kind.type_id()})).collect();
    let mut out = json!({"id": info.id, "name": info.name, "category": info.category, "brushes": brushes});
    if bool_or(p, "definitions", false) {
        out["definitions"] = serde_json::to_value(&lib.brushes).unwrap_or_default();
    }
    Ok(out)
}

fn add(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "brush.addFromLibrary";
    let (info, lib) = library_param(s, p, C)?;
    let mut names = str_list(p, "names");
    names.extend(str_param(p, "name").map(str::to_string));
    if let Some(n) = names.iter().find(|n| lib.brush(n).is_none()) {
        return Err(bad(C, format!("no brush `{n}` in `{}`", lib.name)));
    }
    let mut picked: Vec<&Brush> = if names.is_empty() { lib.brushes.iter().collect() } else { names.iter().filter_map(|n| lib.brush(n)).collect() };
    // Each once, in the order asked for.
    let mut seen = HashSet::new();
    picked.retain(|b| seen.insert(b.name.as_str()));
    // Brushes the document already has (same name and definition) are reused, not added again.
    let mut doc_lib = brush::library(&s.doc()?.doc);
    let (existing, fresh): (Vec<&Brush>, Vec<&Brush>) = picked.iter().partition(|b| doc_lib.iter().any(|x| x == **b));
    let undo_before = s.doc()?.history.undo.len();
    // Library name → the name it got in the document.
    let mut added: Vec<(String, String)> = vec![];
    if !fresh.is_empty() {
        for b in fresh {
            let name = brush::unique_name(&doc_lib, &b.name);
            added.push((b.name.clone(), name.clone()));
            doc_lib.push(Brush { name, ..b.clone() });
        }
        s.edit("Add to Brushes", |d, _| {
            brush::store(d, &doc_lib);
            Ok(())
        })?;
    }
    let doc_name = |lib_name: &str| added.iter().find(|(l, _)| l == lib_name).map_or_else(|| lib_name.to_string(), |(_, n)| n.clone());
    let mut out = json!({
        "library": info.id,
        "added": added.iter().map(|(_, n)| n).collect::<Vec<_>>(),
        "existing": existing.iter().map(|b| &b.name).collect::<Vec<_>>(),
    });
    if let Some(name) = picked.first().map(|b| doc_name(&b.name)).filter(|_| bool_or(p, "apply", false)) {
        let has_targets = p.get("ids").is_some() || s.doc().is_ok_and(|st| !st.selection.is_empty());
        if has_targets {
            let mut params = json!({ "name": name });
            if let Some(ids) = p.get("ids") {
                params["ids"] = ids.clone();
            }
            s.execute("brush.apply", &params)?;
            // Adding and applying undo together.
            squash(s, undo_before, "Add to Brushes");
            out["applied"] = json!(name);
        } else {
            s.execute("brush.setCurrent", &json!({ "name": name }))?;
            out["current"] = json!(name);
        }
    }
    Ok(out)
}

fn save(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "brush.saveLibrary";
    let st = s.doc()?;
    let name = str_param(p, "name").map(str::trim).filter(|n| !n.is_empty()).map_or_else(|| stem(&st.title()).to_string(), str::to_string);
    let lib = BrushLibrary::from_document(&st.doc, &str_list(p, "names"), name).map_err(|e| bad(C, e))?;
    if lib.is_empty() {
        return Err(bad(C, "the document has no brushes to save"));
    }
    let mut out = json!({ "count": lib.len() });
    s.brush_libraries.write(p, &lib.name, BRUSHES_EXT, FileData::Text(libs::write(&lib)), &mut out, C)?;
    Ok(out)
}

fn load(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "brush.loadLibrary";
    let (info, lib) = s.brush_libraries.load(p, C, |bytes, file| {
        // A library file, or a document whose brushes become the library.
        match std::str::from_utf8(bytes).ok().filter(|t| libs::sniff(t)) {
            Some(t) => libs::read(t, stem(file)).map_err(|e| bad(C, e)),
            None => {
                let loaded = super::fileio::load(file, bytes).map_err(|e| bad(C, e.to_string()))?;
                if loaded.preview_only {
                    return Err(bad(C, "only this Affinity file's embedded preview could be read; use File › Open to see it with its warning"));
                }
                BrushLibrary::from_document(&loaded.doc, &[], stem(file).to_string()).map_err(|e| bad(C, e))
            }
        }
    })?;
    Ok(json!({"library": info.id, "name": info.name, "count": lib.len()}))
}
