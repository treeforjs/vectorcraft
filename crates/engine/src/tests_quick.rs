//! Quick graphics (UX5): inserts land at the page's centre in the quick style and undo in one
//! step; quick styles and themes apply and undo; type goes inside a shape, grouped with it; PNGs
//! of the selection and the page.

use serde_json::{Value, json};
use vectorcraft_color::{Color, Paint};
use vectorcraft_doc::{NodeKind, TextKind};

use super::*;
use crate::cmd::quick::{ICONS, SHAPES, STYLES, THEMES};

fn session() -> Session {
    let mut s = Session::new();
    s.execute("file.new", &json!({"width": 800, "height": 600})).unwrap();
    s
}

fn run(s: &mut Session, cmd: &str, p: Value) -> Value {
    s.execute(cmd, &p).unwrap_or_else(|e| panic!("{cmd} {p}: {e}"))
}

fn id_of(v: &Value) -> NodeId {
    NodeId(v["id"].as_u64().unwrap())
}

fn bounds(s: &Session, id: NodeId) -> vectorcraft_geom::Rect {
    s.doc().unwrap().doc.node(id).unwrap().geometric_bounds().unwrap()
}

fn undo_len(s: &Session) -> usize {
    s.doc().unwrap().history.undo.len()
}

fn board(s: &Session) -> vectorcraft_geom::Rect {
    s.doc().unwrap().doc.artboards[0].rect
}

fn fill_of(s: &Session, id: NodeId) -> Paint {
    s.doc().unwrap().doc.node(id).unwrap().appearance.fill_paint()
}

fn hex(c: &str) -> Paint {
    Paint::solid(Color::from_hex(c).unwrap())
}

#[test]
fn the_themes_and_looks_are_well_formed() {
    for t in THEMES {
        for c in t.colors {
            assert!(Color::from_hex(c).is_some(), "{} {c}", t.id);
        }
    }
    for st in STYLES {
        assert!(quick::look_preview(st.id, &THEMES[0]).is_some(), "{}", st.id);
    }
    let list = run(&mut session(), "quick.list", json!({}));
    assert_eq!(list["shapes"].as_array().unwrap().len(), SHAPES.len());
    assert_eq!(list["icons"].as_array().unwrap().len(), ICONS.len());
    assert_eq!((list["theme"].as_str(), list["style"].as_str()), (Some("ocean"), Some("solid")));
}

use crate::cmd::quick;

#[test]
fn every_shape_lands_at_the_page_centre_selected_and_undoes_in_one_step() {
    let mut s = session();
    let c = board(&s).center();
    for shape in SHAPES {
        let undo = undo_len(&s);
        let id = id_of(&run(&mut s, "insert.shape", json!({ "shape": shape.id })));
        let b = bounds(&s, id);
        assert!((b.center().x - c.x).abs() < 0.5 && (b.center().y - c.y).abs() < 0.5, "{}: {b:?} not centred on {c:?}", shape.id);
        assert!(b.width() > 50.0, "{}: a sensible size, {b:?}", shape.id);
        assert_eq!(s.doc().unwrap().selection.objects, vec![id], "{}", shape.id);
        assert_eq!(undo_len(&s), undo + 1, "{}: one undo step", shape.id);
        let NodeKind::Path { path, .. } = &s.doc().unwrap().doc.node(id).unwrap().kind else { panic!("{} is a path", shape.id) };
        let open = matches!(shape.id, "line" | "arrowLine");
        assert_eq!(path.is_closed(), !open, "{}", shape.id);
        if !open {
            // The default quick style: filled in the theme's main colour.
            assert_eq!(fill_of(&s, id), hex(THEMES[0].colors[0]), "{}", shape.id);
        }
        run(&mut s, "edit.undo", json!({}));
        assert!(s.doc().unwrap().doc.node(id).is_none(), "{}: undone", shape.id);
        run(&mut s, "edit.redo", json!({}));
    }
    // A centre and a size given are used; the arrow line has an arrowhead.
    let id = id_of(&run(&mut s, "insert.shape", json!({ "shape": "arrowLine", "center": [100, 120], "size": [80, 0] })));
    let b = bounds(&s, id);
    assert!((b.center().x - 100.0).abs() < 0.01 && (b.width() - 80.0).abs() < 0.01, "{b:?}");
    let ap = &s.doc().unwrap().doc.node(id).unwrap().appearance;
    assert!(ap.stroke().unwrap().end_arrow.is_some() && ap.fill_paint().is_none());
    // Bad params fail without changing the document.
    let undo = undo_len(&s);
    for p in [json!({"shape": "blob"}), json!({}), json!({"shape": "star", "size": [-1, 4]}), json!({"shape": "star", "center": [1e300, 0]})] {
        assert!(s.execute("insert.shape", &p).is_err(), "{p}");
    }
    assert_eq!(undo_len(&s), undo);
}

#[test]
fn text_boxes_and_icons_land_at_the_centre_and_undo() {
    let mut s = session();
    let c = board(&s).center();
    let id = id_of(&run(&mut s, "insert.textBox", json!({ "text": "Hello" })));
    let node = s.doc().unwrap().doc.node(id).unwrap().clone();
    let NodeKind::Text(t) = &node.kind else { panic!("text") };
    assert!(matches!(t.kind, TextKind::Area { .. }));
    assert_eq!(t.plain_text(), "Hello");
    let b = bounds(&s, id);
    assert!((b.center().x - c.x).abs() < 1.0, "{b:?}");
    run(&mut s, "edit.undo", json!({}));
    assert!(s.doc().unwrap().doc.node(id).is_none());
    for (icon, _, _) in ICONS {
        let undo = undo_len(&s);
        let id = id_of(&run(&mut s, "insert.icon", json!({ "icon": icon })));
        let n = s.doc().unwrap().doc.node(id).unwrap().clone();
        assert!(matches!(n.kind, NodeKind::Group { .. }), "{icon}");
        let b = n.geometric_bounds().unwrap();
        assert!((b.center().x - c.x).abs() < 40.0 && (b.center().y - c.y).abs() < 40.0, "{icon}: {b:?}");
        assert!(b.width() > 20.0, "{icon}: {b:?}");
        assert_eq!(undo_len(&s), undo + 1, "{icon}");
        // Stroked in the theme's main colour.
        let mut strokes = vec![];
        n.walk(&mut |c| {
            if !c.is_container() {
                strokes.push(c.appearance.stroke_paint());
            }
        });
        assert!(strokes.iter().any(|p| *p == hex(THEMES[0].colors[0])), "{icon}: {strokes:?}");
        run(&mut s, "edit.undo", json!({}));
        assert!(s.doc().unwrap().doc.node(id).is_none(), "{icon}");
    }
    assert!(s.execute("insert.icon", &json!({"icon": "nope"})).is_err());
}

#[test]
fn a_quick_style_applies_undoes_and_becomes_the_look_of_new_inserts() {
    let mut s = session();
    let id = id_of(&run(&mut s, "insert.shape", json!({ "shape": "rectangle" })));
    let before = s.doc().unwrap().doc.node(id).unwrap().appearance.clone();
    let undo = undo_len(&s);
    assert_eq!(run(&mut s, "quick.style", json!({ "style": "outline" }))["changed"], 1);
    assert_eq!(undo_len(&s), undo + 1);
    let ap = s.doc().unwrap().doc.node(id).unwrap().appearance.clone();
    assert!(ap.fill_paint().is_none());
    assert_eq!(ap.stroke_paint(), hex(THEMES[0].colors[0]));
    assert_eq!(ap.stroke().unwrap().width, 3.0);
    // Dashed and shadow.
    run(&mut s, "quick.style", json!({ "style": "dashed" }));
    assert!(s.doc().unwrap().doc.node(id).unwrap().appearance.stroke().unwrap().dash.is_some());
    run(&mut s, "quick.style", json!({ "style": "shadow" }));
    let ap2 = s.doc().unwrap().doc.node(id).unwrap().appearance.clone();
    assert!(ap2.stroke().is_none_or(|st| st.dash.is_none()));
    assert!(ap2.effects.iter().any(|e| e.id == "stylize.dropShadow"));
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.doc().unwrap().doc.node(id).unwrap().appearance, before);
    // Picked with nothing selected: the next insert gets it.
    run(&mut s, "quick.style", json!({ "style": "dark", "ids": [] }));
    let next = id_of(&run(&mut s, "insert.shape", json!({ "shape": "ellipse" })));
    assert_eq!(fill_of(&s, next), hex(THEMES[0].colors[3]));
    assert!(s.execute("quick.style", &json!({"style": "glitter"})).is_err());
}

#[test]
fn a_theme_recolours_quick_styled_art_and_undoes() {
    let mut s = session();
    let a = id_of(&run(&mut s, "insert.shape", json!({ "shape": "rectangle" })));
    run(&mut s, "quick.style", json!({ "style": "soft" }));
    let b = id_of(&run(&mut s, "insert.shape", json!({ "shape": "star" })));
    run(&mut s, "quick.style", json!({ "style": "accent" }));
    // Art in other colours is left alone.
    let other = id_of(&run(&mut s, "shape.rectangle", json!({"x": 10, "y": 10, "width": 20, "height": 20})));
    run(&mut s, "paint.setFill", json!({ "ids": [other.0], "color": "#123456", "keepModel": true }));
    let other_fill = fill_of(&s, other);
    let undo = undo_len(&s);
    let sunset = THEMES.iter().find(|t| t.id == "sunset").unwrap();
    run(&mut s, "quick.theme", json!({ "theme": "sunset" }));
    assert_eq!(undo_len(&s), undo + 1);
    assert_eq!(s.doc().unwrap().doc.node(a).unwrap().appearance.stroke_paint(), hex(sunset.colors[0]));
    assert_eq!(fill_of(&s, b), hex(sunset.colors[2]));
    assert_eq!(fill_of(&s, other), other_fill);
    assert_eq!(run(&mut s, "quick.list", json!({}))["theme"], "sunset");
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(fill_of(&s, b), hex(THEMES[0].colors[2]));
    assert_eq!(run(&mut s, "quick.list", json!({}))["theme"], "ocean");
    assert!(s.execute("quick.theme", &json!({"theme": "plaid"})).is_err());
}

#[test]
fn type_inside_a_shape_is_grouped_with_it_and_flows_in_its_outline() {
    let mut s = session();
    let shape = id_of(&run(&mut s, "insert.shape", json!({ "shape": "ellipse" })));
    let sb = bounds(&s, shape);
    let undo = undo_len(&s);
    let r = run(&mut s, "insert.textInShape", json!({ "text": "Launch" }));
    assert_eq!(r["created"], true);
    assert_eq!(undo_len(&s), undo + 1);
    let (text, group) = (id_of(&r), NodeId(r["group"].as_u64().unwrap()));
    let d = &s.doc().unwrap().doc;
    assert_eq!(d.parent_of(text), Some(group));
    assert_eq!(d.parent_of(shape), Some(group));
    let NodeKind::Text(t) = &d.node(text).unwrap().kind else { panic!("text") };
    let TextKind::Area { frame } = &t.kind else { panic!("area type") };
    assert_eq!(frame.bounds().unwrap(), sb, "the type flows in the shape's outline");
    assert_eq!(t.para.justify, vectorcraft_doc::Justify::Center);
    assert_eq!(t.area.vertical_align, vectorcraft_doc::VerticalAlign::Center);
    // Light type on the filled shape.
    assert_eq!(t.runs[0].style.fill, hex(THEMES[0].colors[4]));
    assert_eq!(vectorcraft_tools::select::shape_text_parts(d, group), Some((group, text)));
    // Again (on the group or the shape): the same type.
    let again = run(&mut s, "insert.textInShape", json!({ "id": group.0 }));
    assert_eq!((again["created"].clone(), id_of(&again)), (json!(false), text));
    // Moving the group moves both.
    run(&mut s, "select.set", json!({ "ids": [group.0] }));
    run(&mut s, "object.move", json!({ "dx": 30, "dy": 0 }));
    assert!((bounds(&s, shape).x0 - sb.x0 - 30.0).abs() < 0.01);
    // A quick style keeps the type readable on the shape.
    run(&mut s, "quick.style", json!({ "style": "outline" }));
    let NodeKind::Text(t) = &s.doc().unwrap().doc.node(text).unwrap().kind else { panic!() };
    assert_eq!(t.runs[0].style.fill, hex(THEMES[0].colors[3]));
    // Undo takes it all back.
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "edit.undo", json!({}));
    assert!(s.doc().unwrap().doc.node(text).is_none());
    assert!(s.doc().unwrap().doc.node(group).is_none() && s.doc().unwrap().doc.node(shape).is_some());
    // An open line can't take type.
    let line = id_of(&run(&mut s, "insert.shape", json!({ "shape": "line" })));
    assert!(s.execute("insert.textInShape", &json!({ "id": line.0 })).is_err());
}

#[test]
fn pngs_of_the_selection_and_the_page() {
    let mut s = session();
    run(&mut s, "insert.shape", json!({ "shape": "rectangle", "size": [100, 50] }));
    let sel = run(&mut s, "quick.png", json!({}));
    assert_eq!((sel["width"].as_u64(), sel["height"].as_u64()), (Some(200), Some(100)));
    let png = vectorcraft_format::base64_decode(sel["dataBase64"].as_str().unwrap()).unwrap();
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    let page = run(&mut s, "quick.png", json!({ "target": "artboard", "scale": 1 }));
    assert_eq!((page["width"].as_u64(), page["height"].as_u64()), (Some(800), Some(600)));
    run(&mut s, "select.none", json!({}));
    assert!(s.execute("quick.png", &json!({ "target": "selection" })).is_err());
    assert!(s.execute("quick.png", &json!({ "scale": 0 })).is_err());
    assert!(s.execute("quick.png", &json!({ "target": "artboard", "artboard": 9 })).is_err());
}
