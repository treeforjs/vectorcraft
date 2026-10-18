//! Live compound shapes: operands stay editable, the result follows them, Release and Expand.
use super::*;
use serde_json::json;
use vectorcraft_doc::compound::{OPERANDS, RESULT};
use vectorcraft_doc::{Node, NodeKind};
use vectorcraft_geom::{Point, Shape as _};

fn session() -> Session {
    let mut s = Session::new();
    s.execute("file.new", &json!({"width": 500, "height": 500})).unwrap();
    s
}
fn rectangle(s: &mut Session, x: f64, fill: &str) -> NodeId {
    s.execute("paint.setFill", &json!({"color": fill, "ids": []})).unwrap();
    let id = s.execute("shape.rectangle", &json!({"x":x,"y":0,"width":100,"height":100})).unwrap()["id"].as_u64().unwrap();
    NodeId(id)
}
fn node(s: &Session, id: NodeId) -> &Node {
    s.doc().unwrap().doc.node(id).unwrap()
}
fn result(s: &Session, id: NodeId) -> Node {
    let r = node(s, id).children().unwrap().get(1).unwrap();
    assert_eq!(r.name.as_deref(), Some(RESULT));
    (**r).clone()
}
fn inside(n: &Node, x: f64, y: f64) -> bool {
    let p = Point::new(x, y);
    match &n.kind {
        NodeKind::Compound { children, .. } => children.iter().map(|c| c.path_data().unwrap().to_bezpath().winding(p)).sum::<i32>() != 0,
        _ => n.path_data().unwrap().to_bezpath().winding(p) != 0,
    }
}
fn make(s: &mut Session, ids: &[NodeId], mode: &str) -> NodeId {
    s.execute("select.set", &json!({"ids": ids.iter().map(|i| i.0).collect::<Vec<_>>()})).unwrap();
    NodeId(s.execute("object.compoundShape.make", &json!({"mode": mode})).unwrap()["id"].as_u64().unwrap())
}

#[test]
fn each_mode_combines_the_operands_and_keeps_them_editable() {
    // Two 100×100 squares overlapping on x 50–100.
    for (mode, expect) in
        [("add", [true, true, true]), ("subtract", [true, false, false]), ("intersect", [false, true, false]), ("exclude", [true, false, true])]
    {
        let mut s = session();
        let a = rectangle(&mut s, 0.0, "#ff0000");
        let b = rectangle(&mut s, 50.0, "#0000ff");
        let undo = s.doc().unwrap().history.undo.len();
        let g = make(&mut s, &[a, b], mode);
        assert_eq!(s.doc().unwrap().history.undo.len(), undo + 1, "{mode}: one undo step");
        let r = result(&s, g);
        assert_eq!([inside(&r, 25.0, 50.0), inside(&r, 75.0, 50.0), inside(&r, 125.0, 50.0)], expect, "{mode}");
        let ops = node(&s, g).children().unwrap().first().unwrap();
        assert_eq!(ops.name.as_deref(), Some(OPERANDS));
        assert_eq!(ops.opacity, 0.0);
        assert_eq!(ops.children().unwrap().len(), 2, "{mode}: the operands are kept");
        // The result takes the top operand's paint, unless the top one only subtracts.
        let paint = r.appearance.fill_paint().label();
        assert_eq!(paint, if mode == "subtract" { "#ff0000" } else { "#0000ff" }, "{mode}");
        assert_eq!(node(&s, g).geometric_bounds(), r.geometric_bounds(), "{mode}: bounds are the result's");
    }
}

#[test]
fn editing_an_operand_reshapes_the_result_and_modes_can_change() {
    let mut s = session();
    let a = rectangle(&mut s, 0.0, "#ff0000");
    let b = rectangle(&mut s, 50.0, "#0000ff");
    let g = make(&mut s, &[a, b], "subtract");
    assert!(!inside(&result(&s, g), 75.0, 50.0));
    // Moving the front operand away uncovers the back one.
    s.execute("select.set", &json!({"ids": [b.0]})).unwrap();
    s.execute("object.move", &json!({"dx": 200, "dy": 0})).unwrap();
    let r = result(&s, g);
    assert!(inside(&r, 75.0, 50.0));
    assert!(!inside(&r, 275.0, 50.0));
    // The operand takes another mode.
    s.execute("object.compoundShape.setMode", &json!({"ids": [b.0], "mode": "add"})).unwrap();
    assert!(inside(&result(&s, g), 275.0, 50.0));
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(!inside(&result(&s, g), 275.0, 50.0));
    // Alt-clicking a shape mode with the compound shape selected sets every operand above the bottom.
    s.execute("select.set", &json!({"ids": [g.0]})).unwrap();
    s.execute("object.compoundShape.make", &json!({"mode": "intersect"})).unwrap();
    assert!(node(&s, g).geometric_bounds().is_none(), "separated squares intersect to nothing");
    // Moving the whole compound shape moves its result with it.
    s.execute("object.compoundShape.setMode", &json!({"ids": [g.0], "mode": "add"})).unwrap();
    s.execute("select.set", &json!({"ids": [g.0]})).unwrap();
    s.execute("object.move", &json!({"dx": 0, "dy": 100})).unwrap();
    assert!(inside(&result(&s, g), 25.0, 150.0));
    // Repainting the compound shape keeps the new paint when its operands change later.
    s.execute("paint.setFill", &json!({"color": "#00ff00"})).unwrap();
    s.execute("select.set", &json!({"ids": [a.0]})).unwrap();
    s.execute("object.move", &json!({"dx": 1, "dy": 0})).unwrap();
    assert_eq!(result(&s, g).appearance.fill_paint().label(), "#00ff00");
}

#[test]
fn release_and_expand() {
    let mut s = session();
    let a = rectangle(&mut s, 0.0, "#ff0000");
    let b = rectangle(&mut s, 50.0, "#0000ff");
    let g = make(&mut s, &[a, b], "add");
    let out = s.execute("object.compoundShape.release", &json!({})).unwrap();
    assert_eq!(out["ids"].as_array().unwrap().len(), 2);
    assert!(s.doc().unwrap().doc.node(g).is_none());
    assert_eq!(node(&s, a).appearance.fill_paint().label(), "#ff0000");
    let g = make(&mut s, &[a, b], "add");
    let r = result(&s, g);
    let id = NodeId(s.execute("object.compoundShape.expand", &json!({})).unwrap()["id"].as_u64().unwrap());
    let n = node(&s, id);
    assert!(n.compound.is_none() && n.name.is_none());
    assert_eq!(n.geometric_bounds(), r.geometric_bounds());
    assert!(s.doc().unwrap().doc.node(a).is_none(), "the operands are gone");
}

#[test]
fn nested_compound_shapes_and_bad_params() {
    let mut s = session();
    let a = rectangle(&mut s, 0.0, "#ff0000");
    let b = rectangle(&mut s, 50.0, "#0000ff");
    let c = rectangle(&mut s, 100.0, "#00ff00");
    let inner = make(&mut s, &[a, b], "intersect");
    let outer = make(&mut s, &[inner, c], "add");
    let r = result(&s, outer);
    assert!(!inside(&r, 25.0, 50.0) && inside(&r, 75.0, 50.0) && inside(&r, 175.0, 50.0));
    // Editing the inner operand reaches the outer result.
    s.execute("select.set", &json!({"ids": [b.0]})).unwrap();
    s.execute("object.move", &json!({"dx": -50, "dy": 0})).unwrap();
    assert!(inside(&result(&s, outer), 25.0, 50.0));
    // Pathfinder sees the compound shape as what it draws.
    s.execute("select.set", &json!({"ids": [outer.0]})).unwrap();
    assert!(s.execute("object.compoundShape.make", &json!({"mode": "divide"})).is_err());
    s.execute("select.set", &json!({"ids": [c.0]})).unwrap();
    assert!(s.execute("object.compoundShape.setMode", &json!({"mode": "add", "ids": [999_999]})).is_err());
    assert!(s.execute("object.compoundShape.setMode", &json!({})).is_err());
    let lone = rectangle(&mut s, 300.0, "#000000");
    s.execute("select.set", &json!({"ids": [lone.0]})).unwrap();
    assert!(s.execute("object.compoundShape.make", &json!({})).is_err(), "one object is not enough");
    assert!(s.execute("object.compoundShape.release", &json!({})).is_err());
}

#[test]
fn deleting_every_operand_removes_the_compound_shape() {
    let mut s = session();
    let a = rectangle(&mut s, 0.0, "#ff0000");
    let b = rectangle(&mut s, 50.0, "#0000ff");
    let g = make(&mut s, &[a, b], "add");
    s.execute("select.set", &json!({"ids": [a.0, b.0]})).unwrap();
    s.execute("edit.clear", &json!({})).unwrap();
    assert!(s.doc().unwrap().doc.node(g).is_none());
}
