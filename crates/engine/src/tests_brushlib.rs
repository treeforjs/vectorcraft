//! Brush libraries: the built-in libraries, adding to the document (deduped, one undo step,
//! applied or made current), saving and loading `.vcbrushes` and user libraries.

use serde_json::{Value, json};
use vectorcraft_doc::{AppearanceItem, NodeId};

use super::*;

fn session() -> Session {
    let mut s = Session::new();
    s.execute("file.new", &json!({"width": 200, "height": 200})).unwrap();
    s
}

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn undo_len(s: &Session) -> usize {
    s.doc().unwrap().history.undo.len()
}

fn names(v: &Value) -> Vec<&str> {
    v.as_array().unwrap().iter().map(|n| n.as_str().unwrap()).collect()
}

fn doc_brushes(s: &mut Session) -> Vec<String> {
    run(s, "brush.list", json!({}))["brushes"].as_array().unwrap().iter().map(|b| b["name"].as_str().unwrap().to_string()).collect()
}

#[test]
fn list_and_get_every_builtin_library() {
    let mut s = Session::new();
    let list = run(&mut s, "brush.libraries", json!({}));
    let libs = list["libraries"].as_array().unwrap();
    assert!(libs.len() >= 5 && libs.iter().all(|l| l["category"] == "builtIn"), "{libs:?}");
    let mut kinds = std::collections::HashSet::new();
    for l in libs {
        let id = l["id"].as_str().unwrap();
        let g = run(&mut s, "brush.library", json!({"library": id, "definitions": true}));
        let brushes = g["brushes"].as_array().unwrap();
        assert!(brushes.len() >= 6 && l["count"].as_u64() == Some(brushes.len() as u64), "{id}");
        assert_eq!(g["definitions"].as_array().unwrap().len(), brushes.len());
        kinds.extend(brushes.iter().map(|b| b["type"].as_str().unwrap().to_string()));
    }
    assert_eq!(kinds.len(), 5, "every kind of brush is in a library: {kinds:?}");
    // By name in any case.
    assert_eq!(run(&mut s, "brush.library", json!({"library": "decorative scatter"}))["id"], "decorative-scatter");
    assert!(s.execute("brush.library", &json!({"library": "nope"})).is_err());
}

#[test]
fn add_from_library_is_one_undo_step_and_deduplicates() {
    let mut s = session();
    let before = doc_brushes(&mut s);
    let n = undo_len(&s);
    let r = run(&mut s, "brush.addFromLibrary", json!({"library": "arrows", "names": ["Open Arrow", "Double Arrow"]}));
    assert_eq!(names(&r["added"]), ["Open Arrow", "Double Arrow"]);
    assert_eq!(undo_len(&s), n + 1);
    assert_eq!(s.doc().unwrap().history.undo.last().unwrap().label, "Add to Brushes");
    let after = doc_brushes(&mut s);
    assert_eq!(after.len(), before.len() + 2);
    // Again: the same brushes are already there.
    let r = run(&mut s, "brush.addFromLibrary", json!({"library": "arrows", "name": "Open Arrow"}));
    assert!(r["added"].as_array().unwrap().is_empty());
    assert_eq!(names(&r["existing"]), ["Open Arrow"]);
    assert_eq!(undo_len(&s), n + 1, "nothing to add is no undo step");
    // A name taken by another brush gets a number.
    run(&mut s, "brush.new", json!({"type": "calligraphic", "name": "Stars"}));
    let r = run(&mut s, "brush.addFromLibrary", json!({"library": "decorative-scatter", "name": "Stars"}));
    assert_eq!(names(&r["added"]), ["Stars 2"]);
    assert!(s.execute("brush.addFromLibrary", &json!({"library": "arrows", "name": "Nope"})).is_err());
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(doc_brushes(&mut s), before);
}

#[test]
fn add_and_apply_undo_together_or_make_the_brush_current() {
    let mut s = session();
    // Nothing selected: the brush becomes the Paintbrush's.
    let r = run(&mut s, "brush.addFromLibrary", json!({"library": "ink-calligraphy", "name": "Brush Pen", "apply": true}));
    assert_eq!(r["current"], "Brush Pen");
    assert_eq!(run(&mut s, "brush.list", json!({}))["current"], "Brush Pen");
    let id = NodeId(run(&mut s, "shape.ellipse", json!({"x": 10, "y": 10, "width": 80, "height": 50}))["id"].as_u64().unwrap());
    let n = undo_len(&s);
    let r = run(&mut s, "brush.addFromLibrary", json!({"library": "borders", "name": "Waves", "apply": true}));
    assert_eq!(r["applied"], "Waves");
    assert_eq!(undo_len(&s), n + 1, "adding and applying is one step");
    let node = s.doc().unwrap().doc.node(id).unwrap().clone();
    assert!(node.appearance.items.iter().any(|i| matches!(i, AppearanceItem::Stroke(st) if st.brush.as_deref() == Some("Waves"))));
    run(&mut s, "edit.undo", json!({}));
    assert!(!doc_brushes(&mut s).contains(&"Waves".to_string()));
}

#[test]
fn save_and_load_round_trip_and_user_libraries() {
    let mut s = session();
    run(&mut s, "brush.addFromLibrary", json!({"library": "paint-pencil", "names": ["Pencil", "Fan Brush"]}));
    let saved = run(&mut s, "brush.saveLibrary", json!({"names": ["Pencil", "Fan Brush"], "name": "Sketchy"}));
    assert_eq!(saved["count"], 2);
    let data = saved["data"].as_str().unwrap().to_string();
    let r = run(&mut s, "brush.loadLibrary", json!({"data": data, "name": "Sketchy.vcbrushes"}));
    assert_eq!((r["name"].as_str(), r["count"].as_u64()), (Some("Sketchy"), Some(2)));
    let id = r["library"].as_str().unwrap().to_string();
    assert!(run(&mut s, "brush.libraries", json!({}))["libraries"].as_array().unwrap().iter().any(|l| l["id"] == id.as_str()));
    let lib = run(&mut s, "brush.library", json!({"library": id, "definitions": true}));
    let pencil = run(&mut s, "brush.get", json!({"name": "Pencil"}));
    assert_eq!(lib["definitions"][0], pencil, "definitions survive the round trip");
    assert!(s.execute("brush.saveLibrary", &json!({"names": ["Nope"]})).is_err());
    assert!(s.execute("brush.loadLibrary", &json!({"data": "{\"format\": \"vcstyles\"}", "name": "x.vcbrushes"})).is_err());

    // The user library folder: saved with user: true, listed as User Defined.
    let dir = std::env::temp_dir().join(format!("vc-brushlib-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    s.brush_libraries.set_user_dir(Some(dir.to_string_lossy().to_string()));
    let r = run(&mut s, "brush.saveLibrary", json!({"user": true, "name": "Mine"}));
    assert_eq!(r["library"], "user/Mine.vcbrushes");
    let listed = run(&mut s, "brush.libraries", json!({}));
    let user: Vec<&Value> = listed["libraries"].as_array().unwrap().iter().filter(|l| l["category"] == "user").collect();
    assert_eq!(user.len(), 1);
    assert_eq!(user[0]["name"], "Mine");
    // A document's brushes load as a library too.
    let file = dir.join("brushes.vectorcraft");
    run(&mut s, "file.saveAs", json!({"path": file.to_string_lossy()}));
    let r = run(&mut s, "brush.loadLibrary", json!({"path": file.to_string_lossy()}));
    assert_eq!(r["count"].as_u64(), Some(doc_brushes(&mut s).len() as u64));
    let _ = std::fs::remove_dir_all(&dir);
}
