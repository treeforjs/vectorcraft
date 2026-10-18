//! 9-slice scaling for symbols: Symbol Options guides, and instances whose corners keep their
//! size when scaled.

use serde_json::json;
use vectorcraft_doc::{Node, NodeKind};
use vectorcraft_geom::{Point, Rect};

use super::*;

fn session() -> Session {
    let mut s = Session::new();
    s.execute("file.new", &json!({"width": 600, "height": 400})).unwrap();
    s
}

fn id(v: &serde_json::Value) -> NodeId {
    NodeId(v["id"].as_u64().unwrap())
}

/// A 100 × 50 pt panel with a 10 pt square in its top-left corner, made into symbol "Panel";
/// returns the instance.
fn panel(s: &mut Session) -> NodeId {
    let a = id(&s.execute("shape.rectangle", &json!({"x": 100, "y": 100, "width": 100, "height": 50})).unwrap());
    let b = id(&s.execute("shape.rectangle", &json!({"x": 100, "y": 100, "width": 10, "height": 10})).unwrap());
    s.select(|_, sel| sel.set([a, b])).unwrap();
    id(&s.execute("symbol.new", &json!({"name": "Panel"})).unwrap())
}

/// Instance `inst` broken into plain art: the bounds of each of its paths, largest first.
fn expanded(s: &mut Session, inst: NodeId) -> Vec<Rect> {
    let r = s.execute("symbol.breakLink", &json!({"ids": [inst.0]})).unwrap();
    let mut out = vec![];
    for i in r["ids"].as_array().unwrap() {
        let n = s.doc().unwrap().doc.node(NodeId(i.as_u64().unwrap())).cloned().unwrap();
        n.walk(&mut |c: &Node| {
            if matches!(c.kind, NodeKind::Path { .. }) {
                out.extend(c.geometric_bounds());
            }
        });
    }
    out.sort_by(|a, b| b.area().total_cmp(&a.area()));
    out
}

fn near(a: Rect, b: Rect) -> bool {
    [(a.x0, b.x0), (a.y0, b.y0), (a.x1, b.x1), (a.y1, b.y1)].iter().all(|(p, q)| (p - q).abs() < 1e-6)
}

fn scale(s: &mut Session, inst: NodeId, sx: f64, sy: f64) {
    s.select(|_, sel| sel.set([inst])).unwrap();
    s.execute("object.scale", &json!({"sx": sx, "sy": sy, "origin": [100, 100]})).unwrap();
}

#[test]
fn options_turn_guides_on_a_quarter_in_and_validate_them() {
    let mut s = session();
    panel(&mut s);
    let r = s.execute("symbol.options", &json!({"name": "Panel", "nineSlice": true})).unwrap();
    assert_eq!(r["nineSlice"], true);
    assert_eq!(r["guides"], json!({"left": 25.0, "top": 12.5, "right": 25.0, "bottom": 12.5}));
    let r = s.execute("symbol.options", &json!({"name": "Panel", "guides": {"left": 10, "top": 10}})).unwrap();
    assert_eq!(r["guides"]["left"], 10.0);
    assert_eq!(r["guides"]["right"], 25.0, "unnamed guides stay where they were");
    assert!(s.execute("symbol.options", &json!({"name": "Panel", "guides": {"left": 90}})).is_err(), "guides crossing");
    assert!(s.execute("symbol.options", &json!({"name": "Panel", "guides": {"top": -1}})).is_err());
    assert!(s.execute("symbol.options", &json!({"name": "Nope", "nineSlice": true})).is_err());
    let l = s.execute("symbol.list", &json!({})).unwrap();
    assert_eq!(l["symbols"][0]["nineSlice"], true);
    // Off, and undoable.
    s.execute("symbol.options", &json!({"name": "Panel", "nineSlice": false})).unwrap();
    assert!(s.doc().unwrap().doc.symbols[0].slice.is_none());
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(s.doc().unwrap().doc.symbols[0].slice.is_some());
}

#[test]
fn corners_keep_their_size_when_an_instance_is_stretched() {
    let mut s = session();
    let inst = panel(&mut s);
    s.execute("symbol.options", &json!({"name": "Panel", "guides": {"left": 10, "top": 10, "right": 10, "bottom": 10}})).unwrap();
    scale(&mut s, inst, 300.0, 200.0);
    let parts = expanded(&mut s, inst);
    assert!(near(parts[0], Rect::new(100.0, 100.0, 400.0, 200.0)), "the panel fills the scaled box: {:?}", parts[0]);
    assert!(near(parts[1], Rect::new(100.0, 100.0, 110.0, 110.0)), "the corner square keeps its size: {:?}", parts[1]);
}

#[test]
fn without_guides_the_art_scales_as_a_whole() {
    let mut s = session();
    let inst = panel(&mut s);
    scale(&mut s, inst, 300.0, 200.0);
    let parts = expanded(&mut s, inst);
    assert!(near(parts[1], Rect::new(100.0, 100.0, 130.0, 120.0)), "{:?}", parts[1]);
}

#[test]
fn corners_shrink_together_below_their_size() {
    let mut s = session();
    let inst = panel(&mut s);
    s.execute("symbol.options", &json!({"name": "Panel", "guides": {"left": 20, "top": 10, "right": 20, "bottom": 10}})).unwrap();
    // 100 → 20 pt wide: the 20 + 20 pt corners share 20 pt.
    scale(&mut s, inst, 20.0, 100.0);
    let parts = expanded(&mut s, inst);
    assert!(near(parts[0], Rect::new(100.0, 100.0, 120.0, 150.0)), "{:?}", parts[0]);
    assert!(near(parts[1], Rect::new(100.0, 100.0, 105.0, 110.0)), "{:?}", parts[1]);
}

#[test]
fn rotated_instances_slice_along_their_own_axes() {
    let mut s = session();
    let inst = panel(&mut s);
    s.execute("symbol.options", &json!({"name": "Panel", "guides": {"left": 10, "top": 10, "right": 10, "bottom": 10}})).unwrap();
    scale(&mut s, inst, 300.0, 100.0);
    s.select(|_, sel| sel.set([inst])).unwrap();
    s.execute("object.rotate", &json!({"angle": 90, "origin": [100, 100]})).unwrap();
    let parts = expanded(&mut s, inst);
    // 300 × 50 turned a quarter: 50 wide, 300 tall; the corner square is still 10 × 10.
    assert!((parts[0].width() - 50.0).abs() < 1e-6 && (parts[0].height() - 300.0).abs() < 1e-6, "{:?}", parts[0]);
    assert!((parts[1].width() - 10.0).abs() < 1e-6 && (parts[1].height() - 10.0).abs() < 1e-6, "{:?}", parts[1]);
}

#[test]
fn curves_crossing_guides_are_cut_and_stay_continuous() {
    let mut s = session();
    let e = id(&s.execute("shape.ellipse", &json!({"x": 100, "y": 100, "width": 100, "height": 100})).unwrap());
    s.select(|_, sel| sel.set([e])).unwrap();
    let inst = id(&s.execute("symbol.new", &json!({"name": "Round"})).unwrap());
    s.execute("symbol.options", &json!({"name": "Round", "nineSlice": true})).unwrap();
    scale(&mut s, inst, 400.0, 100.0);
    let r = s.execute("symbol.breakLink", &json!({"ids": [inst.0]})).unwrap();
    let n = s.doc().unwrap().doc.node(NodeId(r["ids"][0].as_u64().unwrap())).cloned().unwrap();
    let NodeKind::Path { path, .. } = &n.kind else { panic!("{:?}", n.kind) };
    let sp = &path.subpaths[0];
    // Cut at both vertical guides on the top and bottom halves.
    assert_eq!(sp.anchors.len(), 8, "{:?}", sp.anchors);
    assert!(near(path.bounds().unwrap(), Rect::new(100.0, 100.0, 500.0, 200.0)), "{:?}", path.bounds());
    // The ends keep their size: everything left of the first guide stays within 25 pt of the
    // left edge…
    for a in &sp.anchors {
        assert!(a.p.x >= 100.0 - 1e-6 && a.p.x <= 500.0 + 1e-6);
    }
    let cuts: Vec<Point> = sp.anchors.iter().map(|a| a.p).filter(|p| (p.x - 125.0).abs() < 1e-6 || (p.x - 475.0).abs() < 1e-6).collect();
    assert_eq!(cuts.len(), 4, "the guides land 25 pt in from each end: {:?}", sp.anchors);
    // …and the stretched top edge stays at the top.
    assert!(sp.anchors.iter().any(|a| (a.p - Point::new(300.0, 100.0)).hypot() < 1e-6), "{:?}", sp.anchors);
}

#[test]
fn skewed_instances_draw_their_art_unsliced() {
    let mut s = session();
    let inst = panel(&mut s);
    s.execute("symbol.options", &json!({"name": "Panel", "nineSlice": true})).unwrap();
    let doc = s.doc().unwrap().doc.clone();
    let mut n = doc.node(inst).cloned().unwrap();
    let NodeKind::SymbolInstance { xf, .. } = &mut n.kind else { panic!() };
    *xf = vectorcraft_geom::Affine::skew(0.5, 0.0) * *xf;
    let art = doc.symbol_art_for(&n).unwrap();
    assert!(std::sync::Arc::ptr_eq(&art, &doc.symbols[0].art));
}

#[test]
fn guides_survive_save_and_duplicate() {
    let mut s = session();
    panel(&mut s);
    s.execute("symbol.options", &json!({"name": "Panel", "guides": {"left": 5, "top": 6, "right": 7, "bottom": 8}})).unwrap();
    s.execute("symbol.duplicate", &json!({"name": "Panel"})).unwrap();
    let d = &s.doc().unwrap().doc;
    assert_eq!(d.symbols[1].slice, d.symbols[0].slice);
    let text = serde_json::to_string(d).unwrap();
    let back: vectorcraft_doc::Document = serde_json::from_str(&text).unwrap();
    assert_eq!(back.symbols[0].slice, d.symbols[0].slice);
}

#[test]
fn hostile_guides_from_a_file_never_panic() {
    let mut s = session();
    let inst = panel(&mut s);
    let doc = s.doc().unwrap().doc.clone();
    let n = doc.node(inst).cloned().unwrap();
    for g in [[f64::NAN, 0.0, 0.0, 0.0], [1e300, 1e300, 1e300, 1e300], [-5.0, 0.0, 0.0, 0.0], [0.0, 0.0, 0.0, 0.0]] {
        let mut d = (*doc).clone();
        d.symbols[0].slice = Some(vectorcraft_doc::nine_slice::NineSlice { left: g[0], top: g[1], right: g[2], bottom: g[3] });
        for xf in [vectorcraft_geom::Affine::scale(0.0), vectorcraft_geom::Affine::scale(1e-300), vectorcraft_geom::Affine::scale(1e300)] {
            let mut n = n.clone();
            if let NodeKind::SymbolInstance { xf: x, .. } = &mut n.kind {
                *x = xf;
            }
            let _ = d.symbol_art_for(&n);
        }
    }
}
