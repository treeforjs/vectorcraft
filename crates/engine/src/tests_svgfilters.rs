//! Effect › SVG Filters: Apply SVG Filter (the `svg.filter` effect) applied, edited and checked as
//! commands, drawn on the canvas and in PDF, written to SVG as the filter itself and read back;
//! Import SVG Filter adding a file's filters to the document's.

use serde_json::json;
use vectorcraft_geom::Rect;
use vectorcraft_render::{Rendered, Renderer};

use super::*;

const SHADOW: &str = r#"<filter x="-20%" y="-20%" width="150%" height="150%"><feGaussianBlur in="SourceAlpha" stdDeviation="3"/><feOffset dx="6" dy="6" result="s"/><feMerge><feMergeNode in="s"/><feMergeNode in="SourceGraphic"/></feMerge></filter>"#;

/// A red square 50..150; returns its id.
fn square() -> (Session, u64) {
    let mut s = Session::new();
    s.execute("file.new", &json!({"width": 200, "height": 200})).unwrap();
    let id = s.execute("shape.rectangle", &json!({"x": 50, "y": 50, "width": 100, "height": 100})).unwrap()["id"].as_u64().unwrap();
    s.execute("paint.setFill", &json!({"color": "#e03020", "ids": [id]})).unwrap();
    s.execute("paint.setStroke", &json!({"none": true, "ids": [id]})).unwrap();
    (s, id)
}

fn render(s: &Session) -> Rendered {
    Renderer::new().render_region(&s.doc().unwrap().doc, Rect::new(0.0, 0.0, 200.0, 200.0), 1.0, true)
}

/// The rendered pixel at (x, y), RGBA.
fn px(r: &Rendered, x: usize, y: usize) -> [u8; 4] {
    let i = (y * r.width as usize + x) * 4;
    [r.pixels[i], r.pixels[i + 1], r.pixels[i + 2], r.pixels[i + 3]]
}

#[test]
fn applied_drawn_edited_and_undone() {
    let (mut s, id) = square();
    let plain = render(&s);
    // Below the square's bottom right corner: white before, the shadow after.
    assert_eq!(px(&plain, 153, 153), [255, 255, 255, 255]);
    s.execute("effect.apply", &json!({"effect": "svg.filter", "ids": [id], "params": {"name": "Shadow", "filter": SHADOW}})).unwrap();
    let shadowed = render(&s);
    let p = px(&shadowed, 153, 153);
    assert!(p[0] < 200 && p[0] == p[1], "a grey shadow: {p:?}");
    // The square itself is unchanged (but for rounding: filters work in linear RGB by default).
    let (a, b) = (px(&shadowed, 100, 100), px(&plain, 100, 100));
    assert!(a.iter().zip(b).all(|(x, y)| x.abs_diff(y) <= 3), "{a:?} {b:?}");
    // Edited: no offset, no shadow below the corner.
    s.execute("effect.setParams", &json!({"index": 0, "ids": [id], "params": {"filter": SHADOW.replace("dx=\"6\" dy=\"6\"", "dx=\"0\" dy=\"0\"")}}))
        .unwrap();
    assert!(px(&render(&s), 156, 156)[0] > 250);
    s.execute("edit.undo", &json!({})).unwrap();
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(s.doc().unwrap().doc.node(NodeId(id)).unwrap().appearance.effects.is_empty());
}

#[test]
fn what_isnt_a_filter_is_refused() {
    let (mut s, id) = square();
    for bad in [json!("<g/>"), json!("<filter"), json!(5), json!("x".repeat(300 * 1024))] {
        let r = s.execute("effect.apply", &json!({"effect": "svg.filter", "ids": [id], "params": {"filter": bad}}));
        assert!(r.is_err(), "{bad}");
    }
    assert!(s.doc().unwrap().doc.node(NodeId(id)).unwrap().appearance.effects.is_empty());
    // The default filter applies with no parameters.
    s.execute("effect.apply", &json!({"effect": "svg.filter", "ids": [id]})).unwrap();
    assert!(s.execute("effect.setParams", &json!({"index": 0, "ids": [id], "params": {"filter": "<svg/>"}})).is_err());
    let _ = render(&s);
}

#[test]
fn svg_export_writes_the_filter_and_import_reads_it_back() {
    let (mut s, id) = square();
    let turbulence =
        r#"<filter id="t"><feTurbulence baseFrequency="0.05" result="n"/><feDisplacementMap in="SourceGraphic" in2="n" scale="10"/></filter>"#;
    s.execute("effect.apply", &json!({"effect": "svg.filter", "ids": [id], "params": {"name": "Wobble", "filter": turbulence}})).unwrap();
    let svg = s.execute("document.serialize", &json!({"format": "svg"})).unwrap()["text"].as_str().unwrap().to_string();
    assert!(svg.contains("<feDisplacementMap") && !svg.contains("<image"), "{svg}");
    let (back, warnings) = vectorcraft_svg::import_with_report(&svg).unwrap();
    let mut fx = vec![];
    back.walk(|n| fx.extend(n.appearance.effects.clone()));
    assert_eq!(fx.len(), 1, "{warnings:?}");
    assert_eq!(fx[0].id, "svg.filter");
    assert!(fx[0].params["filter"].as_str().unwrap().contains("feDisplacementMap"));
    // PDF draws it as the canvas does, as an image.
    let doc = s.doc().unwrap().doc.clone();
    let r = crate::cmd::rasterfx::export_pdf_with_report(&doc, &Default::default()).unwrap();
    let back = vectorcraft_pdf::import(&r.bytes).unwrap();
    let a = render(&s);
    let b = Renderer::new().render_region(&back, Rect::new(0.0, 0.0, 200.0, 200.0), 1.0, true);
    let diff: u64 = a.pixels.iter().zip(&b.pixels).map(|(x, y)| x.abs_diff(*y) as u64).sum();
    assert!((diff as f64 / a.pixels.len() as f64) < 255.0 * 0.03);
}

#[test]
fn import_svg_filter_adds_the_files_filters() {
    let (mut s, _) = square();
    let list = s.execute("effect.svgFilters", &json!({})).unwrap();
    let presets = list.as_array().unwrap().len();
    assert!(presets >= 10 && list[0]["builtIn"] == json!(true));
    let file = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg"><defs>{SHADOW}<filter id="Glow"><feGaussianBlur stdDeviation="2"/></filter></defs></svg>"#
    );
    let r = s.execute("effect.svgFilters.import", &json!({"data": file})).unwrap();
    assert_eq!(r["imported"], json!(["Filter 1", "Glow"]));
    // Again: new names; with replace: the same.
    let r = s.execute("effect.svgFilters.import", &json!({"data": file})).unwrap();
    assert_eq!(r["imported"], json!(["Filter 1 2", "Glow 2"]));
    s.execute("effect.svgFilters.import", &json!({"data": file, "replace": true})).unwrap();
    let list = s.execute("effect.svgFilters", &json!({})).unwrap();
    assert_eq!(list.as_array().unwrap().len(), presets + 4);
    assert_eq!(list[presets + 1]["name"], json!("Glow"));
    // Saved with the document and undoable.
    let text = s.execute("document.serialize", &json!({"format": "vectorcraft"})).unwrap()["text"].as_str().unwrap_or_default().to_string();
    if !text.is_empty() {
        assert!(text.contains("svg_filters") || text.contains("svgFilters"));
    }
    assert_eq!(s.execute("effect.svgFilters.remove", &json!({"names": ["Glow 2", "nope"]})).unwrap()["removed"], json!(1));
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().doc.svg_filters.len(), 4);
    for bad in [json!({}), json!({"data": "<svg/>"}), json!({"data": "nope"}), json!({"dataBase64": "!!"})] {
        assert!(s.execute("effect.svgFilters.import", &bad).is_err(), "{bad}");
    }
}
