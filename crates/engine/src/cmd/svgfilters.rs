//! Effect › SVG Filters: the filters a document offers (VectorCraft's own and those imported from
//! SVG files with Import SVG Filter…). Apply SVG Filter… is the `svg.filter` effect, applied with
//! `effect.apply`; it keeps its own copy of the filter, so editing or deleting the document's list
//! changes no art.

use serde_json::{Value, json};
use vectorcraft_doc::SvgFilterDef;
use vectorcraft_render::effects::svgfilter;

use super::*;

/// Longest filter name kept, in characters.
const MAX_NAME: usize = 64;
/// Largest SVG file read for its filters.
const MAX_FILE: usize = 64 * 1024 * 1024;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            query "effect.svgFilters",
            "SVG Filters",
            [],
            None,
            "{} → [{name, filter: \"<filter>…</filter>\", builtIn: bool}] VectorCraft's filters, then the document's own (apply one with effect.apply {effect: \"svg.filter\", params: {name, filter}})",
            always,
            list
        ),
        cmd!(
            "effect.svgFilters.import",
            "Import SVG Filter…",
            [],
            None,
            "{path? | data?: SVG text | dataBase64?, replace?: false (a filter named like one the document has replaces it; else it comes in as \"name 2\")} add the <filter> elements of an SVG file to the document's SVG filters (at most 500 in all) → {imported: [name…]}",
            has_doc,
            import
        ),
        cmd!(
            "effect.svgFilters.remove",
            "Delete SVG Filter",
            [],
            None,
            "{names: [name…]} take filters off the document's list (art that runs them keeps them) → {removed: n}",
            has_doc,
            remove
        ),
    ]
}

fn list(s: &mut Session, _: &Value) -> Result<Value> {
    let builtin = svgfilter::PRESETS.iter().map(|(name, xml)| json!({"name": name, "filter": xml, "builtIn": true}));
    let own: Vec<Value> = s
        .active()
        .map(|st| st.doc.svg_filters.iter().map(|f| json!({"name": f.name, "filter": f.filter, "builtIn": false})).collect())
        .unwrap_or_default();
    Ok(Value::Array(builtin.chain(own).collect()))
}

/// `name` trimmed to [`MAX_NAME`] characters, else "SVG Filter".
fn clean_name(name: &str) -> String {
    let n: String = name.trim().chars().filter(|c| !c.is_control()).take(MAX_NAME).collect();
    if n.is_empty() { "SVG Filter".into() } else { n }
}

/// `base`, or `base 2`, `base 3`… the first not in `taken`.
fn unique(base: &str, taken: &[&str]) -> String {
    if !taken.contains(&base) {
        return base.to_string();
    }
    (2..).map(|k| format!("{base} {k}")).find(|n| !taken.contains(&n.as_str())).unwrap_or_default()
}

fn import(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "effect.svgFilters.import";
    let bytes = match (str_param(p, "path"), str_param(p, "data"), str_param(p, "dataBase64")) {
        (Some(path), ..) => super::fileio::read_file(path)?,
        (None, Some(text), _) => text.as_bytes().to_vec(),
        (None, None, Some(b64)) => vectorcraft_format::base64_decode(b64).ok_or_else(|| bad(C, "bad dataBase64"))?,
        _ => return Err(bad(C, "give `path`, `data` or `dataBase64`")),
    };
    if bytes.len() > MAX_FILE {
        return Err(bad(C, "the file is too large"));
    }
    let text = std::str::from_utf8(&bytes).map_err(|_| bad(C, "not an SVG file (not UTF-8 text)"))?;
    let found = svgfilter::filters_in_svg(text, SvgFilterDef::MAX);
    if found.is_empty() {
        return Err(bad(C, "the file has no SVG filters"));
    }
    let replace = bool_or(p, "replace", false);
    let mut list = s.doc()?.doc.svg_filters.clone();
    let mut imported = vec![];
    for (name, filter) in found {
        let base = clean_name(&name);
        let at = list.iter().position(|f| f.name == base).filter(|_| replace);
        let name = match at {
            Some(_) => base,
            None => unique(&base, &list.iter().map(|f| f.name.as_str()).collect::<Vec<_>>()),
        };
        let def = SvgFilterDef { name: name.clone(), filter };
        if let Some(slot) = at.and_then(|i| list.get_mut(i)) {
            *slot = def;
        } else if list.len() < SvgFilterDef::MAX {
            list.push(def);
        } else {
            break;
        }
        imported.push(name);
    }
    if imported.is_empty() {
        return Err(bad(C, format!("a document keeps at most {} SVG filters", SvgFilterDef::MAX)));
    }
    s.edit("Import SVG Filter", |d, _| {
        d.svg_filters = list;
        Ok(())
    })?;
    Ok(json!({ "imported": imported }))
}

fn remove(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "effect.svgFilters.remove";
    let names: Vec<String> =
        p.get("names").and_then(Value::as_array).map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect()).unwrap_or_default();
    if names.is_empty() {
        return Err(bad(C, "give `names`"));
    }
    let before = s.doc()?.doc.svg_filters.len();
    let kept: Vec<SvgFilterDef> = s.doc()?.doc.svg_filters.iter().filter(|f| !names.contains(&f.name)).cloned().collect();
    let removed = before - kept.len();
    if removed > 0 {
        s.edit("Delete SVG Filter", |d, _| {
            d.svg_filters = kept;
            Ok(())
        })?;
    }
    Ok(json!({ "removed": removed }))
}

/// An `svg.filter` effect's `filter` (given in `params`, else its default) must read as an SVG
/// filter, so a bad one is refused rather than drawing nothing.
pub(crate) fn check(c: &str, params: &Value) -> Result<()> {
    let Some(v) = params.get("filter") else { return Ok(()) };
    let xml = v.as_str().ok_or_else(|| bad(c, "`filter` must be the text of an SVG <filter> element"))?;
    if xml.len() > svgfilter::MAX_XML {
        return Err(bad(c, format!("`filter` is longer than {} KB", svgfilter::MAX_XML / 1024)));
    }
    svgfilter::SvgFilter::new(xml).map(|_| ()).ok_or_else(|| bad(c, "`filter` isn't an SVG <filter> element"))
}
