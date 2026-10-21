use serde_json::json;
use vectorcraft_engine::Session;

use crate::{ControlRequest, VectorcraftApp};

#[test]
fn a_timed_out_queued_edit_never_mutates_the_document_when_the_ui_recovers() {
    let (tx, rx) = std::sync::mpsc::channel();
    let mut app = VectorcraftApp::new(Session::new(), Default::default()).with_control(rx);
    let ctx = egui::Context::default();
    app.run("file.new", json!({})).unwrap();
    let before = app.session.execute("document.inspect", &json!({})).unwrap();
    let journal = app.session.journal.clone();
    let (expired, reply) = ControlRequest::new("engine.execute", json!({"command": "file.new", "params": {}}));
    let ticket = expired.cancellation();
    tx.send(expired).unwrap();
    assert!(ticket.cancel_before_start());
    app.drain_control(&ctx);
    assert_eq!(app.session.execute("document.inspect", &json!({})).unwrap(), before);
    assert_eq!(app.session.journal, journal);
    assert_eq!(app.session.documents().len(), 1);
    assert!(reply.try_recv().is_err());

    let (fresh, reply) = ControlRequest::new("engine.execute", json!({"command": "file.new", "params": {}}));
    tx.send(fresh).unwrap();
    app.drain_control(&ctx);
    assert_eq!(reply.try_recv().unwrap()["ok"], true);
    assert_eq!(app.session.documents().len(), 2);
}
