//! Symbol libraries (Window → Symbol Libraries, the Symbols panel's library button): read-only sets
//! of symbols the library panel shows. The built-in libraries are drawn in
//! [`vectorcraft_doc::symbol_libs`]; User Defined ones are the `.vcsymbols` files in the user
//! library folder ([`Libraries`]), and Other Library… loads more from files (or the symbols of any
//! document). `symbol.addFromLibrary` copies symbols into the document with the resources their
//! art uses, as Paste does; `symbol.saveLibrary` writes the document's.
//!
//! A `.vcsymbols` file is JSON: `{format: "vcsymbols", version: 1, name, symbols, sizes, patterns,
//! swatches, brushes, charStyles, paraStyles, images: {key: {mime, data: base64}}}`.

use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use vectorcraft_color::Swatch;
use vectorcraft_doc::symbol_libs::{self, SYMBOL_LIBRARIES, SYMBOLS_EXT, builtin_symbol_library};
use vectorcraft_doc::{Document, ImageBlob, Node, NodeId, NodeKind, PatternDef, Symbol, SymbolLibrary, TextStyleDef};
use vectorcraft_geom::Affine;

use super::clipboard::SwatchChoices;
use super::menucmds::squash;
use super::swatch::str_list;
use super::swatchlib::{FileData, LibraryFile, LibraryInfo, stem};
use super::*;
use crate::Clipboard;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            query "symbol.libraries",
            "Symbol Libraries",
            [],
            None,
            "{} → {libraries: [{id, name, category: \"builtIn\"|\"user\" (User Defined: the .vcsymbols files in the user library folder, rescanned now)|\"loaded\" (symbol.loadLibrary), count, path?}], userFolder} every symbol library the library panel opens, in menu order",
            always,
            list
        ),
        cmd!(
            query "symbol.library",
            "Symbol Library",
            [],
            None,
            "{library: id or name} → {id, name, category, symbols: [{name, size: [w, h] (natural size in points)}]}",
            always,
            get
        ),
        cmd!(
            "symbol.addFromLibrary",
            "Add to Symbols",
            ["Window", "Symbol Libraries"],
            None,
            "{library: id or name, name? | names?: [symbol names in the library] (default: all of it), place?: false (also place an instance of the first one, as symbol.place), x?, y? (where to place it; default: the artboard's centre), swatchConflict?: \"add\" (default: a library swatch whose name the document gives another colour comes in renamed, so the art keeps its look) | \"merge\"} copy library symbols into the document's Symbols as one undo step (with the placing), with the symbols, patterns, swatches, images and brushes their art uses. A symbol the document already has (same name and art) isn't added again; a name taken by other art gets a number (\"Pin 2\"). The first one becomes the current symbol → {library, added: [names in the document], existing: [names already there], symbols: [the document's name for each one asked for], id?: the placed instance}",
            has_doc,
            add
        ),
        cmd!(
            "symbol.saveLibrary",
            "Save Symbol Library…",
            ["Window", "Symbols"],
            None,
            "{path?, names?: [symbol names] (default: all), name?: library name (default: the document's), user?: false (save into the user library folder, listed under User Defined)} save the document's symbols as a .vcsymbols library (JSON: the symbols with their natural sizes, and the symbols, patterns, swatches, images and brushes their art uses) → {path, count, library?: id when saved to the user folder}; without path or user → {data: the file's text, count}",
            has_doc,
            save
        ),
        cmd!(
            "symbol.loadLibrary",
            "Other Library…",
            ["Window", "Symbol Libraries"],
            None,
            "{path? | data?: file text | dataBase64?, name?: file name (default: the path's)} load a .vcsymbols library, or the symbols of any document VectorCraft opens (see document.formats), for the library panel (Window → Symbol Libraries lists it until the app quits; a file of the user library folder, or a file with the same extension and bytes as one there, opens as that User Defined library) → {library: id, name, count}",
            always,
            load
        ),
    ]
}

impl LibraryFile for SymbolLibrary {
    const EXTS: &'static [&'static str] = &[SYMBOLS_EXT];
    fn read(bytes: &[u8], stem: &str) -> std::result::Result<Self, String> {
        read(&String::from_utf8_lossy(bytes), stem)
    }
    fn name(&self) -> &str {
        &self.name
    }
}

/// User Defined and loaded symbol libraries.
pub type Libraries = super::swatchlib::Libraries<SymbolLibrary>;

// ---------- .vcsymbols files ----------

/// An embedded image of a `.vcsymbols` file.
#[derive(Serialize, Deserialize)]
struct FileImage {
    mime: String,
    data: String,
}

/// The file: a header, the library's symbols and the resources they use.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct NativeFile {
    format: String,
    version: u32,
    #[serde(default)]
    name: String,
    #[serde(default)]
    symbols: Vec<Symbol>,
    /// Natural sizes in points, by symbol name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    sizes: BTreeMap<String, Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    patterns: Vec<PatternDef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    swatches: Vec<Swatch>,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    brushes: Value,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    char_styles: Vec<TextStyleDef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    para_styles: Vec<TextStyleDef>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    images: BTreeMap<String, FileImage>,
}

/// `lib` as a `.vcsymbols` file.
pub fn write(lib: &SymbolLibrary) -> String {
    let d = &lib.doc;
    let sizes = d.symbols.iter().filter_map(|s| Some((s.name.clone(), json!(lib.size(&s.name).map(|(w, h)| [w, h])?)))).collect();
    let brushes = vectorcraft_brush::library(d);
    let file = NativeFile {
        format: SYMBOLS_EXT.into(),
        version: 1,
        name: lib.name.clone(),
        symbols: d.symbols.clone(),
        sizes,
        patterns: d.patterns.clone(),
        swatches: d.swatches.clone(),
        brushes: if brushes.is_empty() { Value::Null } else { serde_json::to_value(&brushes).unwrap_or_default() },
        char_styles: d.char_styles.clone(),
        para_styles: d.para_styles.clone(),
        images: d
            .images
            .iter()
            .map(|(k, b)| (k.clone(), FileImage { mime: b.mime.clone(), data: vectorcraft_format::base64_encode(&b.bytes) }))
            .collect(),
    };
    serde_json::to_string_pretty(&file).unwrap_or_default()
}

/// Read a `.vcsymbols` file; an unnamed library is called `name`. Sizes that aren't finite and
/// positive are left out, and so are images whose data isn't base64.
pub fn read(text: &str, name: &str) -> std::result::Result<SymbolLibrary, String> {
    let f: NativeFile = serde_json::from_str(text.trim_start_matches('\u{feff}')).map_err(|e| format!("not a symbol library: {e}"))?;
    if f.format != SYMBOLS_EXT {
        return Err(format!("not a symbol library (format `{}`)", f.format));
    }
    let mut lib = SymbolLibrary::new(if f.name.trim().is_empty() { name.to_string() } else { f.name });
    let d = &mut lib.doc;
    // Each name once.
    let mut seen = HashSet::new();
    d.symbols = f.symbols.into_iter().filter(|s| seen.insert(s.name.clone())).collect();
    for (n, v) in &f.sizes {
        let size = v.get(0).and_then(Value::as_f64).zip(v.get(1).and_then(Value::as_f64));
        if let Some((w, h)) = size.filter(|(w, h)| w.is_finite() && h.is_finite() && *w > 0.0 && *h > 0.0)
            && seen.contains(n)
        {
            symbol_libs::set_natural_size(d, n, (w, h));
        }
    }
    d.patterns = f.patterns;
    d.swatches = f.swatches;
    vectorcraft_brush::store(d, &vectorcraft_brush::parse_library(&f.brushes));
    d.char_styles = f.char_styles;
    d.para_styles = f.para_styles;
    for (k, img) in f.images {
        if let Some(bytes) = vectorcraft_format::base64_decode(&img.data) {
            d.images.insert(k, ImageBlob::new(img.mime, bytes));
        }
    }
    Ok(lib)
}

/// Does `text` look like a `.vcsymbols` file (its header names the format)?
pub fn sniff(text: &str) -> bool {
    let t = text.trim_start_matches('\u{feff}').trim_start();
    let head = t.char_indices().nth(256).map_or(t, |(i, _)| &t[..i]);
    t.starts_with('{') && head.contains(&format!("\"{SYMBOLS_EXT}\""))
}

/// An instance of each symbol `names` (what the library's resources are gathered from).
fn instances(names: &[&str]) -> Vec<Node> {
    names.iter().map(|n| Node::new(NodeId(0), NodeKind::SymbolInstance { symbol: n.to_string(), xf: Affine::IDENTITY })).collect()
}

/// Document `d`'s symbols `names` (empty: all of them, in panel order) as a library named `name`,
/// with the resources their art uses and their natural sizes.
pub fn library_from_document(d: &Document, names: &[String], name: String) -> std::result::Result<SymbolLibrary, String> {
    if let Some(n) = names.iter().find(|n| !d.symbols.iter().any(|s| s.name == **n)) {
        return Err(format!("no symbol `{n}`"));
    }
    let picked: Vec<&str> = d.symbols.iter().map(|s| s.name.as_str()).filter(|n| names.is_empty() || names.iter().any(|x| x == n)).collect();
    let clip = Clipboard::gather(d, instances(&picked));
    let mut lib = SymbolLibrary::new(name);
    let out = &mut lib.doc;
    // The symbols asked for first, then the ones they place.
    let mut symbols = clip.symbols;
    symbols.sort_by_key(|s| !picked.contains(&s.name.as_str()));
    for s in &symbols {
        if let Some(size) = symbol_libs::natural_size(d, &s.name) {
            symbol_libs::set_natural_size(out, &s.name, size);
        }
    }
    out.symbols = symbols;
    out.images = clip.images;
    out.patterns = clip.patterns;
    out.swatches = clip.swatches;
    out.char_styles = clip.char_styles;
    out.para_styles = clip.para_styles;
    vectorcraft_brush::store(out, &clip.brushes);
    Ok(lib)
}

// ---------- commands ----------

fn builtin_info(b: &symbol_libs::BuiltinSymbolLibrary) -> LibraryInfo {
    LibraryInfo { id: b.id.into(), name: b.name.into(), category: "builtIn" }
}

/// Every symbol library, in menu order: built-in, User Defined, loaded.
pub fn libraries(s: &Session) -> Vec<LibraryInfo> {
    SYMBOL_LIBRARIES.iter().map(builtin_info).chain(s.symbol_libraries.infos().cloned()).collect()
}

/// Library `key` (an id, or a name in any case) with its info.
pub fn library(s: &Session, key: &str) -> Option<(LibraryInfo, Arc<SymbolLibrary>)> {
    if let Some(found) = s.symbol_libraries.get(key) {
        return Some(found);
    }
    if let Some(b) = SYMBOL_LIBRARIES.iter().find(|b| b.id == key) {
        return Some((builtin_info(b), builtin_symbol_library(key)?));
    }
    let id = libraries(s).into_iter().find(|l| l.name.eq_ignore_ascii_case(key))?.id;
    library(s, &id)
}

fn library_param(s: &Session, p: &Value, cmd: &str) -> Result<(LibraryInfo, Arc<SymbolLibrary>)> {
    let key = str_param(p, "library").ok_or_else(|| bad(cmd, "missing `library` (see symbol.libraries)"))?;
    library(s, key).ok_or_else(|| bad(cmd, format!("no symbol library `{key}` (see symbol.libraries)")))
}

fn list(s: &mut Session, _: &Value) -> Result<Value> {
    s.symbol_libraries.rescan();
    let libs: Vec<Value> = libraries(s)
        .into_iter()
        .map(|l| {
            let count = library(s, &l.id).map_or(0, |(_, lib)| lib.len());
            json!({"id": l.id, "name": l.name, "category": l.category, "count": count, "path": s.symbol_libraries.path(&l.id)})
        })
        .collect();
    Ok(json!({ "libraries": libs, "userFolder": s.symbol_libraries.user_dir() }))
}

fn get(s: &mut Session, p: &Value) -> Result<Value> {
    let (info, lib) = library_param(s, p, "symbol.library")?;
    let symbols: Vec<Value> = lib.names().map(|n| json!({"name": n, "size": lib.size(n).map(|(w, h)| [w, h])})).collect();
    Ok(json!({"id": info.id, "name": info.name, "category": info.category, "symbols": symbols}))
}

/// Copy `clip`'s symbols into `d` ([`Clipboard::import_into`]) with their natural sizes; `lib`
/// holds the sizes. → each library symbol's name in `d`, and the names `d` had before.
fn import(d: &mut Document, clip: &Clipboard, lib: &SymbolLibrary, choices: &SwatchChoices) -> (BTreeMap<String, String>, HashSet<String>) {
    let before: HashSet<String> = d.symbols.iter().map(|s| s.name.clone()).collect();
    let imported = clip.import_into(d, choices, false);
    let mut names: BTreeMap<String, String> = clip.symbols.iter().map(|s| (s.name.clone(), s.name.clone())).collect();
    for r in imported.renamed.iter().filter(|r| r["kind"] == "symbol") {
        if let (Some(from), Some(to)) = (r["from"].as_str(), r["to"].as_str()) {
            names.insert(from.to_string(), to.to_string());
        }
    }
    for (from, to) in &names {
        if !before.contains(to)
            && let Some(size) = lib.size(from)
        {
            symbol_libs::set_natural_size(d, to, size);
        }
    }
    (names, before)
}

fn add(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "symbol.addFromLibrary";
    let (info, lib) = library_param(s, p, C)?;
    let mut names = str_list(p, "names");
    names.extend(str_param(p, "name").map(str::to_string));
    if let Some(n) = names.iter().find(|n| lib.symbol(n).is_none()) {
        return Err(bad(C, format!("no symbol `{n}` in `{}`", lib.name)));
    }
    let mut picked: Vec<&str> = if names.is_empty() { lib.names().collect() } else { names.iter().map(String::as_str).collect() };
    // Each once, in the order asked for.
    let mut seen = HashSet::new();
    picked.retain(|n| seen.insert(*n));
    let choices = match p.get("swatchConflict") {
        None | Some(Value::Null) => SwatchChoices::parse(C, &json!({"swatchConflict": "add"}))?,
        Some(_) => SwatchChoices::parse(C, p)?,
    };
    let clip = Clipboard::gather(&lib.doc, instances(&picked));
    // A dry run tells whether anything is added (nothing: no undo step).
    let doc = s.doc()?.doc.clone();
    let mut probe = (*doc).clone();
    let (mut renamed, mut before) = import(&mut probe, &clip, &lib, &choices);
    let undo_before = s.doc()?.history.undo.len();
    if probe != *doc {
        (renamed, before) = s.edit("Add to Symbols", |d, _| Ok(import(d, &clip, &lib, &choices)))?;
    }
    let doc_names: Vec<String> = picked.iter().map(|n| renamed.get(*n).cloned().unwrap_or_else(|| n.to_string())).collect();
    let (existing, added): (Vec<&String>, Vec<&String>) = doc_names.iter().partition(|n| before.contains(*n));
    let mut out = json!({ "library": info.id, "added": added, "existing": existing, "symbols": doc_names });
    if let Some(first) = doc_names.first() {
        s.execute("symbol.setCurrent", &json!({ "name": first }))?;
        if bool_or(p, "place", false) {
            let mut params = json!({ "name": first });
            for k in ["x", "y"] {
                if let Some(v) = p.get(k) {
                    params[k] = v.clone();
                }
            }
            let r = s.execute("symbol.place", &params)?;
            out["id"] = r["id"].clone();
            // Adding and placing undo together.
            squash(s, undo_before, "Add to Symbols");
        }
    }
    Ok(out)
}

fn save(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "symbol.saveLibrary";
    let st = s.doc()?;
    let name = str_param(p, "name").map(str::trim).filter(|n| !n.is_empty()).map_or_else(|| stem(&st.title()).to_string(), str::to_string);
    let lib = library_from_document(&st.doc, &str_list(p, "names"), name).map_err(|e| bad(C, e))?;
    let mut out = json!({ "count": lib.len() });
    s.symbol_libraries.write(p, &lib.name, SYMBOLS_EXT, FileData::Text(write(&lib)), &mut out, C)?;
    Ok(out)
}

fn load(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "symbol.loadLibrary";
    let (info, lib) = s.symbol_libraries.load(p, C, |bytes, file| {
        // A library file, or a document whose symbols become the library.
        match std::str::from_utf8(bytes).ok().filter(|t| sniff(t)) {
            Some(t) => read(t, stem(file)).map_err(|e| bad(C, e)),
            None => {
                let loaded = super::fileio::load(file, bytes).map_err(|e| bad(C, e.to_string()))?;
                if loaded.preview_only {
                    return Err(bad(C, "only this Affinity file's embedded preview could be read; use File › Open to see it with its warning"));
                }
                library_from_document(&loaded.doc, &[], stem(file).to_string()).map_err(|e| bad(C, e))
            }
        }
    })?;
    Ok(json!({"library": info.id, "name": info.name, "count": lib.len()}))
}
