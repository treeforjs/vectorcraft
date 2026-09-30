use serde_json::json;

use super::*;

fn session() -> Session {
    let mut s = Session::new();
    s.execute("file.new", &json!({"width": 400, "height": 300})).unwrap();
    s
}

#[test]
fn batch_is_one_undo_step() {
    let mut s = session();
    let r = s
        .execute(
            "command.batch",
            &json!({"label": "Logo", "commands": [
                {"command": "paint.setFill", "params": {"color": "#ff0000"}},
                {"command": "shape.rectangle", "params": {"x": 0, "y": 0, "width": 50, "height": 50}},
                {"command": "shape.ellipse", "params": {"x": 60, "y": 0, "width": 50, "height": 50}},
            ]}),
        )
        .unwrap();
    assert_eq!(r["results"].as_array().unwrap().len(), 3);
    let st = s.doc().unwrap();
    assert_eq!(st.history.undo.len(), 1);
    assert_eq!(st.history.undo[0].label, "Logo");
    assert_eq!(st.doc.layers[0].children().unwrap().len(), 2);
    assert_eq!(s.journal.iter().filter(|(c, _)| c == "command.batch").count(), 1);
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().doc.layers[0].children().unwrap().len(), 0);
}

#[test]
fn batch_rolls_back_on_error() {
    let mut s = session();
    let r = s.execute(
        "command.batch",
        &json!({"commands": [
            {"command": "shape.rectangle", "params": {"x": 0, "y": 0, "width": 50, "height": 50}},
            {"command": "no.such.command", "params": {}},
        ]}),
    );
    assert!(r.is_err());
    assert_eq!(s.doc().unwrap().doc.layers[0].children().unwrap().len(), 0);
    assert!(s.doc().unwrap().history.undo.is_empty());
    assert!(!s.in_interaction());
}

#[test]
fn serialize_and_reopen() {
    let mut s = session();
    s.execute("shape.ellipse", &json!({"x": 10, "y": 10, "width": 80, "height": 40})).unwrap();
    let b64 = s.execute("document.serialize", &json!({"format": "drawcraft"})).unwrap()["dataBase64"].as_str().unwrap().to_string();
    let svg = s.execute("document.serialize", &json!({"format": "svg"})).unwrap()["text"].as_str().unwrap().to_string();
    assert!(svg.contains("<svg"));
    s.execute("document.open", &json!({"name": "copy.drawcraft", "dataBase64": b64})).unwrap();
    assert_eq!(s.documents().len(), 2);
    assert_eq!(s.doc().unwrap().doc.node_count(), 2);
    let png = s.execute("document.serialize", &json!({"format": "png", "scale": 0.5})).unwrap()["dataBase64"].as_str().unwrap().to_string();
    assert!(png.len() > 100);
}

#[test]
fn save_and_open_path() {
    let mut s = session();
    s.execute("shape.rectangle", &json!({"x": 10, "y": 10, "width": 80, "height": 40})).unwrap();
    let dir = std::env::temp_dir().join(format!("dc-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("t.drawcraft");
    s.execute("document.save", &json!({"path": path.to_string_lossy()})).unwrap();
    assert!(!s.doc().unwrap().is_dirty());
    s.execute("document.open", &json!({"path": path.to_string_lossy()})).unwrap();
    assert_eq!(s.doc().unwrap().path.as_deref(), Some(path.to_string_lossy().as_ref()));
    let svg = dir.join("t.svg");
    s.execute("document.export", &json!({"path": svg.to_string_lossy()})).unwrap_or_default();
    s.execute("document.export", &json!({"format": "svg", "path": svg.to_string_lossy()})).unwrap();
    assert!(std::fs::read_to_string(&svg).unwrap().contains("<svg"));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn pdf_roundtrip_through_engine() {
    let mut s = session();
    s.execute("shape.rectangle", &json!({"x": 10, "y": 10, "width": 80, "height": 40})).unwrap();
    let b64 = s.execute("document.serialize", &json!({"format": "pdf"})).unwrap()["dataBase64"].as_str().unwrap().to_string();
    s.execute("document.open", &json!({"name": "x.pdf", "dataBase64": b64})).unwrap();
    assert!(s.doc().unwrap().doc.node_count() >= 2);
}
