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

#[test]
fn draw_behind_and_inside() {
    let mut s = session();
    let a = s.execute("shape.rectangle", &json!({"x": 0, "y": 0, "width": 100, "height": 100})).unwrap()["id"].as_u64().unwrap();
    s.execute("view.drawMode", &json!({"mode": "behind"})).unwrap();
    let b = s.execute("shape.ellipse", &json!({"x": 10, "y": 10, "width": 50, "height": 50})).unwrap()["id"].as_u64().unwrap();
    let order: Vec<u64> = s.doc().unwrap().doc.layers[0].children().unwrap().iter().map(|n| n.id.0).collect();
    assert_eq!(order, vec![b, a]);
    s.execute("select.set", &json!({"ids": [a]})).unwrap();
    s.execute("view.drawMode", &json!({"mode": "inside"})).unwrap();
    let c = s.execute("shape.ellipse", &json!({"x": 50, "y": 50, "width": 100, "height": 100})).unwrap()["id"].as_u64().unwrap();
    let d = &s.doc().unwrap().doc;
    let g = d.parent_of(NodeId(c)).unwrap();
    assert_eq!(d.node(g).unwrap().kind_label(), "Clip Group");
    assert_eq!(d.parent_of(NodeId(a)), Some(g));
    // A second shape goes into the same clip group.
    s.execute("shape.ellipse", &json!({"x": 0, "y": 0, "width": 10, "height": 10})).unwrap();
    assert_eq!(s.doc().unwrap().doc.node(g).unwrap().children().unwrap().len(), 4);
    s.execute("view.drawMode", &json!({"mode": "normal"})).unwrap();
    assert!(s.draw_inside.is_none());
}

#[test]
fn export_for_screens_writes_every_combination() {
    let mut s = Session::new();
    s.execute("file.new", &json!({"width": 100, "height": 80, "artboards": 2})).unwrap();
    s.execute("shape.rectangle", &json!({"x": 10, "y": 10, "width": 50, "height": 40})).unwrap();
    let dir = std::env::temp_dir().join(format!("dc-efs-{}", std::process::id()));
    let r = s
        .execute("document.exportForScreens", &json!({"folder": dir.to_string_lossy(), "formats": [{"format": "png", "scale": 1}, {"format": "png", "scale": 2}, {"format": "jpg"}, {"format": "svg"}, {"format": "webp"}]}))
        .unwrap();
    let files = r["files"].as_array().unwrap();
    assert_eq!(files.len(), 10);
    for f in files {
        assert!(std::fs::metadata(f.as_str().unwrap()).unwrap().len() > 50, "{f}");
    }
    assert!(files.iter().any(|f| f.as_str().unwrap().ends_with("Artboard-2@2x.png")));
    let _ = std::fs::remove_dir_all(dir);
}
