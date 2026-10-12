//! Tab leaders survive normal editing, native persistence and conversion/export without becoming
//! repeated characters in the user's text. Inputs are original, made through public commands.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::json;
use vectorcraft_doc::{Document, NodeId, NodeKind, TextObject};
use vectorcraft_engine::Session;
use vectorcraft_geom::Rect;
use vectorcraft_testkit::raster::{assert_similar, render_artboard, render_region};

fn session() -> (Session, NodeId) {
    let mut s = Session::new();
    s.execute("file.new", &json!({"width": 360, "height": 140})).unwrap();
    let id = s.execute("text.create", &json!({"x": 20, "y": 80, "size": 20, "text": "Item\t12.5"})).unwrap()["id"].as_u64().unwrap();
    s.execute("text.setRangeStyle", &json!({"id": id, "start": 4, "end": 5, "fill": "#cc0000"})).unwrap();
    s.execute("text.tabs.set", &json!({"stops": [{"position": 240, "align": "decimal", "leader": ". "}]})).unwrap();
    (s, NodeId(id))
}

fn text(doc: &Document, id: NodeId) -> &TextObject {
    let NodeKind::Text(t) = &doc.node(id).unwrap().kind else { panic!("not text") };
    t
}

#[test]
fn native_save_and_undo_keep_the_pattern_and_the_original_text() {
    let (mut s, id) = session();
    let doc = s.doc().unwrap().doc.clone();
    let t = text(&doc, id);
    assert_eq!(t.plain_text(), "Item\t12.5");
    assert_eq!(t.para.tabs[0].leader, ". ");
    let image = render_artboard(&doc);
    let reopened = vectorcraft_format::load(&vectorcraft_format::save(&doc, false)).unwrap();
    assert_eq!(text(&reopened, id).runs, t.runs);
    assert_eq!(text(&reopened, id).para.tabs, t.para.tabs);
    assert_eq!(render_artboard(&reopened), image);
    s.execute("text.tabs.clear", &json!({})).unwrap();
    let without = render_artboard(&s.doc().unwrap().doc);
    assert_ne!(without, image, "removing a leader changes paint");
    assert_eq!(text(&s.doc().unwrap().doc, id).plain_text(), "Item\t12.5");
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(render_artboard(&s.doc().unwrap().doc), image);
    s.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(render_artboard(&s.doc().unwrap().doc), without);
}

#[test]
fn outlines_and_pdf_keep_the_leaders_and_their_tab_character_paint() {
    let (mut s, id) = session();
    let doc = s.doc().unwrap().doc.clone();
    let gap = Rect::new(80.0, 66.0, 210.0, 84.0);
    let leader_image = render_region(&doc, gap, 3.0);
    assert!(leader_image.ink() > 60, "the gap contains painted leaders");
    assert!(leader_image.count(|p| p[0] > p[1].saturating_add(20) && p[0] > p[2].saturating_add(20)) > 60, "the tab's red fill is used");

    s.execute("type.createOutlines", &json!({})).unwrap();
    assert!(s.doc().unwrap().doc.node(id).is_none());
    assert_similar(&leader_image, &render_region(&s.doc().unwrap().doc, gap, 3.0), 20.0, 0.001);
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(text(&s.doc().unwrap().doc, id).plain_text(), "Item\t12.5");

    for outline_text in [false, true] {
        let mut opts = vectorcraft_pdf::PdfOptions::default();
        opts.settings.advanced.outline_text = outline_text;
        let pdf = vectorcraft_pdf::export(&doc, &opts).unwrap();
        let imported = vectorcraft_pdf::import(&pdf).unwrap();
        assert_similar(&leader_image, &render_region(&imported, gap, 3.0), 25.0, 0.005);
    }
}

#[test]
fn paragraph_scoped_patterns_and_multibyte_text_round_trip() {
    let (mut s, id) = session();
    s.execute("text.editRange", &json!({"id": id.0, "start": 0, "end": 9, "insert": "Café\t8\nCrème\t12"})).unwrap();
    let second = "Café\t8\n".len();
    s.execute("text.tabs.set", &json!({"ids": [id.0], "start": second, "end": second, "stops": [{"position": 180, "leader": "· "}]})).unwrap();
    let doc = s.doc().unwrap().doc.clone();
    let t = text(&doc, id);
    assert_eq!(t.plain_text(), "Café\t8\nCrème\t12");
    assert_eq!(t.para_at(0).tabs[0].leader, ". ");
    assert_eq!(t.para_at(1).tabs[0].leader, "· ");
    let layout = vectorcraft_text::layout(vectorcraft_text::FontDb::global(), t);
    assert_eq!(layout.tab_leaders.len(), 2);
    let back = vectorcraft_format::load(&vectorcraft_format::save(&doc, false)).unwrap();
    assert_eq!(render_artboard(&back), render_artboard(&doc));
    assert_eq!(text(&back, id).plain_text(), t.plain_text());
}
