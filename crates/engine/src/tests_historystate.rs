//! History panel commands: list the states, jump to one, clear them.
use super::*;
use serde_json::json;

fn session_with_steps() -> Session {
    let mut s = Session::new();
    s.execute("file.new", &json!({"width": 500, "height": 500})).unwrap();
    for x in [0, 100, 200] {
        s.execute("shape.rectangle", &json!({"x": x, "y": 0, "width": 50, "height": 50})).unwrap();
    }
    s
}
fn objects(s: &Session) -> usize {
    let mut n = 0;
    s.doc().unwrap().doc.walk(|node| n += usize::from(matches!(node.kind, vectorcraft_doc::NodeKind::Path { .. })));
    n
}

#[test]
fn history_lists_states_and_go_to_state_jumps_both_ways() {
    let mut s = session_with_steps();
    let h = s.execute("edit.history", &json!({})).unwrap();
    let states = h["states"].as_array().unwrap().len();
    let current = h["current"].as_u64().unwrap() as usize;
    assert_eq!(h["states"][0], json!("Open"));
    assert_eq!(states, current + 1);
    assert_eq!(objects(&s), 3);
    // Back to before the rectangles, then forward to the second one.
    let first = (1..states).find(|i| {
        s.execute("edit.goToState", &json!({"state": i})).unwrap();
        objects(&s) == 1
    });
    let first = first.expect("a state with one rectangle");
    s.execute("edit.goToState", &json!({"state": 0})).unwrap();
    assert_eq!(objects(&s), 0);
    let h = s.execute("edit.history", &json!({})).unwrap();
    assert_eq!(h["current"], json!(0));
    assert_eq!(h["states"].as_array().unwrap().len(), states, "later states stay as redo steps");
    s.execute("edit.goToState", &json!({"state": first})).unwrap();
    assert_eq!(objects(&s), 1);
    s.execute("edit.goToState", &json!({"state": states - 1})).unwrap();
    assert_eq!(objects(&s), 3);
    // Out of range and missing states are errors that change nothing.
    assert!(s.execute("edit.goToState", &json!({"state": states})).is_err());
    assert!(s.execute("edit.goToState", &json!({"state": -1})).is_err());
    assert!(s.execute("edit.goToState", &json!({})).is_err());
    assert_eq!(objects(&s), 3);
}

#[test]
fn clear_history_keeps_the_document() {
    let mut s = session_with_steps();
    s.execute("edit.undo", &json!({})).unwrap();
    s.execute("edit.clearHistory", &json!({})).unwrap();
    assert_eq!(objects(&s), 2);
    let h = s.execute("edit.history", &json!({})).unwrap();
    assert_eq!(h, json!({"states": ["Open"], "current": 0}));
    assert!(s.execute("edit.undo", &json!({})).is_err());
}
