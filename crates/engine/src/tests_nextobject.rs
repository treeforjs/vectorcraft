//! Select > Next Object Above / Below.

use serde_json::json;

use super::*;

/// Four circles stacked bottom to top; the ids in stacking order.
fn stack() -> (Session, Vec<u64>) {
    let mut s = Session::new();
    s.execute("file.new", &json!({"width": 400, "height": 300})).unwrap();
    let ids = (0..4)
        .map(|i| s.execute("shape.ellipse", &json!({"x": 20 + i * 30, "y": 20, "width": 40, "height": 40})).unwrap()["id"].as_u64().unwrap())
        .collect();
    (s, ids)
}

fn selected(s: &Session) -> Vec<u64> {
    s.doc().unwrap().selection.objects.iter().map(|id| id.0).collect()
}

#[test]
fn next_object_above_and_below_step_through_the_stack() {
    let (mut s, ids) = stack();
    s.execute("select.set", &json!({"ids": [ids[1]]})).unwrap();
    s.execute("select.nextAbove", &json!({})).unwrap();
    assert_eq!(selected(&s), [ids[2]]);
    s.execute("select.nextBelow", &json!({})).unwrap();
    s.execute("select.nextBelow", &json!({})).unwrap();
    assert_eq!(selected(&s), [ids[0]]);
    // At the bottom (and the top) the selection stays.
    s.execute("select.nextBelow", &json!({})).unwrap();
    assert_eq!(selected(&s), [ids[0]]);
    s.execute("select.set", &json!({"ids": [ids[3]]})).unwrap();
    s.execute("select.nextAbove", &json!({})).unwrap();
    assert_eq!(selected(&s), [ids[3]]);
    // Bring to Front puts the object on top: nothing above it.
    s.execute("select.set", &json!({"ids": [ids[0]]})).unwrap();
    s.execute("object.arrange.bringToFront", &json!({})).unwrap();
    s.execute("select.nextAbove", &json!({})).unwrap();
    assert_eq!(selected(&s), [ids[0]]);
    s.execute("select.nextBelow", &json!({})).unwrap();
    assert_eq!(selected(&s), [ids[3]]);
}

#[test]
fn next_object_skips_hidden_and_locked_objects_and_starts_from_the_selections_edge() {
    let (mut s, ids) = stack();
    s.execute("select.set", &json!({"ids": [ids[1]]})).unwrap();
    s.execute("object.hide", &json!({})).unwrap();
    s.execute("select.set", &json!({"ids": [ids[2]]})).unwrap();
    s.execute("object.lock", &json!({})).unwrap();
    s.execute("select.set", &json!({"ids": [ids[0]]})).unwrap();
    s.execute("select.nextAbove", &json!({})).unwrap();
    assert_eq!(selected(&s), [ids[3]], "past the hidden and the locked one");
    s.execute("select.nextBelow", &json!({})).unwrap();
    assert_eq!(selected(&s), [ids[0]]);
    // Several selected: above the topmost, below the bottommost.
    s.execute("object.unlockAll", &json!({})).unwrap();
    s.execute("object.showAll", &json!({})).unwrap();
    s.execute("select.set", &json!({"ids": [ids[2], ids[1]]})).unwrap();
    s.execute("select.nextAbove", &json!({})).unwrap();
    assert_eq!(selected(&s), [ids[3]]);
    s.execute("select.set", &json!({"ids": [ids[2], ids[1]]})).unwrap();
    s.execute("select.nextBelow", &json!({})).unwrap();
    assert_eq!(selected(&s), [ids[0]]);
}
