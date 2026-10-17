//! Variable font axes set freely (`fontVariations` on `text.setStyle` and `text.setRangeStyle`):
//! the type is laid out at those settings, a named style returns to its own, and a save keeps them.

use serde_json::json;
use vectorcraft_doc::{NodeKind, TextObject};
use vectorcraft_text::test_fonts::{VARIABLE_CHARS, VARIABLE_FAMILY, variable_font};

use super::*;

fn session() -> (Session, NodeId) {
    vectorcraft_text::FontDb::global().add_font(variable_font().unwrap());
    let mut s = Session::new();
    s.execute("file.new", &json!({"width": 800, "height": 600})).unwrap();
    let id = s.execute("text.create", &json!({"x": 40, "y": 100, "size": 48, "text": VARIABLE_CHARS})).unwrap()["id"].as_u64().unwrap();
    s.execute("select.set", &json!({"ids": [id]})).unwrap();
    s.execute("text.setStyle", &json!({"font": VARIABLE_FAMILY, "style": "Regular"})).unwrap();
    (s, NodeId(id))
}

fn text(s: &Session, id: NodeId) -> TextObject {
    match &s.doc().unwrap().doc.node(id).unwrap().kind {
        NodeKind::Text(t) => (**t).clone(),
        _ => panic!("not text"),
    }
}

fn width(s: &Session, id: NodeId) -> f64 {
    text(s, id).cached_bounds.unwrap().width()
}

#[test]
fn font_variations_set_the_axes_of_the_selected_type() {
    let (mut s, id) = session();
    let regular = width(&s, id);
    s.execute("text.setStyle", &json!({"fontVariations": {"wght": 625}})).unwrap();
    assert_eq!(text(&s, id).runs[0].style.font_variations.get("wght"), Some(&625.0));
    let at = width(&s, id);
    assert!(at > regular * 1.1, "wider at 625: {regular} -> {at}");
    // A range: only its characters change.
    s.execute("text.setRangeStyle", &json!({"id": id.0, "start": 0, "end": 1, "fontVariations": {"wght": null}})).unwrap();
    let t = text(&s, id);
    assert!(t.runs[0].style.font_variations.is_empty() && t.runs[1].style.font_variations.get("wght") == Some(&625.0), "{:?}", t.runs);
    // Undo steps back one change at a time.
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(text(&s, id).runs.len(), 1);
    // A named style returns to its own settings.
    s.execute("text.setStyle", &json!({"style": "Bold"})).unwrap();
    assert!(text(&s, id).runs[0].style.font_variations.is_empty());
    // Saved and opened again: the settings stay.
    s.execute("text.setStyle", &json!({"fontVariations": {"wght": 500.5}})).unwrap();
    let saved = serde_json::to_value(&s.doc().unwrap().doc).unwrap();
    let back: vectorcraft_doc::Document = serde_json::from_value(saved).unwrap();
    let NodeKind::Text(t) = &back.node(id).unwrap().kind else { panic!() };
    assert_eq!(t.runs[0].style.font_variations.get("wght"), Some(&500.5));
}

#[test]
fn bad_font_variations_are_refused() {
    let (mut s, _) = session();
    for bad in [json!([1]), json!({"wghtx": 1}), json!({"wght": "heavy"}), json!({"wght": true})] {
        assert!(s.execute("text.setStyle", &json!({"fontVariations": bad})).is_err(), "{bad}");
        assert!(s.execute("text.setRangeStyle", &json!({"fontVariations": bad})).is_err(), "{bad}");
    }
}
