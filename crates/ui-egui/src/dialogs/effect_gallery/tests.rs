use super::*;
use vectorcraft_doc::NodeId;
use vectorcraft_engine::Session;

fn app() -> VectorcraftApp {
    let mut app = VectorcraftApp::new(Session::new(), Default::default());
    app.run("file.new", json!({"width": 200, "height": 200})).unwrap();
    app.run("shape.rectangle", json!({"x": 40, "y": 40, "width": 100, "height": 100})).unwrap();
    app
}
fn stack(app: &VectorcraftApp) -> Vec<Effect> {
    let st = app.session.active().unwrap();
    st.doc.node(st.selection.subjects()[0]).unwrap().appearance.effects.clone()
}
fn set(app: &mut VectorcraftApp, field: &str, value: Value) {
    app.ui.dialog.as_mut().unwrap().fields.insert(field.into(), value);
}
fn frame(app: &mut VectorcraftApp) -> String {
    crate::tests_labels::painted_text(app, |app, ui| crate::dialogs::show(app, ui.ctx()))
}
fn effects() -> Value {
    json!([
        {"id": "blur.gaussian", "params": {"radius": 3}, "visible": true},
        {"id": "sharpen.unsharpMask", "params": {"amount": 150}, "visible": false}
    ])
}

#[test]
fn gallery_preview_cancel_and_unchanged_ok_keep_history_and_redo() {
    let mut app = app();
    app.run("effect.apply", json!({"effect": "blur.gaussian"})).unwrap();
    app.run("edit.undo", json!({})).unwrap();
    let before = app.session.doc().unwrap().history.undo.len();
    let redo = app.session.doc().unwrap().history.redo.len();
    app.run("effect.gallery", json!({})).unwrap();
    let text = frame(&mut app);
    for label in ["Effect Gallery", "Raster Effects", "Effect Stack", "Add Effect", "Preview"] {
        assert!(text.contains(label), "{label}: {text}");
    }
    crate::dialogs::confirm(&mut app).unwrap();
    assert_eq!(app.session.doc().unwrap().history.undo.len(), before);
    assert_eq!(app.session.doc().unwrap().history.redo.len(), redo);
    app.run("effect.gallery", json!({})).unwrap();
    set(&mut app, "effects", effects());
    frame(&mut app);
    assert_eq!(stack(&app).len(), 2);
    assert_eq!(app.session.doc().unwrap().history.undo.len(), before);
    crate::dialogs::cancel(&mut app);
    assert!(stack(&app).is_empty());
    assert_eq!(app.session.doc().unwrap().history.redo.len(), redo);
}

#[test]
fn gallery_preview_off_commit_undo_redo_and_native_roundtrip() {
    let mut app = app();
    let before = app.session.doc().unwrap().history.undo.len();
    app.run("effect.gallery", json!({})).unwrap();
    set(&mut app, "effects", effects());
    frame(&mut app);
    set(&mut app, "preview", json!(false));
    frame(&mut app);
    assert!(stack(&app).is_empty());
    crate::dialogs::confirm(&mut app).unwrap();
    assert_eq!(app.session.doc().unwrap().history.undo.len(), before + 1);
    let kept = stack(&app);
    assert_eq!(kept[0].id, "blur.gaussian");
    assert!(!kept[1].visible);
    app.run("edit.undo", json!({})).unwrap();
    assert!(stack(&app).is_empty());
    app.run("edit.redo", json!({})).unwrap();
    assert_eq!(stack(&app), kept);
    let st = app.session.doc().unwrap();
    let bytes = vectorcraft_format::save(&st.doc, false);
    let back = vectorcraft_format::load(&bytes).unwrap();
    assert_eq!(back.node(st.selection.subjects()[0]).unwrap().appearance.effects, kept);
}

#[test]
fn invalid_confirm_never_commits_a_previous_good_preview() {
    let mut app = app();
    let before = app.session.doc().unwrap().history.undo.len();
    app.run("effect.gallery", json!({})).unwrap();
    set(&mut app, "effects", effects());
    frame(&mut app);
    set(&mut app, "effects", json!([{"id": "missing.effect"}]));
    assert!(crate::dialogs::confirm(&mut app).is_err());
    assert!(app.ui.dialog.is_some());
    assert!(!app.session.in_interaction());
    assert!(stack(&app).is_empty());
    assert_eq!(app.session.doc().unwrap().history.undo.len(), before);
    crate::dialogs::cancel(&mut app);
    assert!(stack(&app).is_empty());
}

#[test]
fn gallery_preserves_geometry_rows_and_targets_the_selected_fill_only() {
    let mut app = app();
    app.run("effect.apply", json!({"effect": "distort.roughen", "params": {"size": 3, "seed": 7}})).unwrap();
    let original = stack(&app);
    app.run("effect.gallery", json!({})).unwrap();
    frame(&mut app);
    crate::dialogs::confirm(&mut app).unwrap();
    assert_eq!(stack(&app), original);
    app.run("appearance.setActiveItem", json!({"index": 0})).unwrap();
    app.run("effect.gallery", json!({})).unwrap();
    set(&mut app, "effects", effects());
    frame(&mut app);
    crate::dialogs::confirm(&mut app).unwrap();
    assert_eq!(stack(&app), original);
    let st = app.session.doc().unwrap();
    assert_eq!(st.doc.node(st.selection.subjects()[0]).unwrap().appearance.effects_at(Some(0)).unwrap().len(), 2);
}

#[test]
fn mixed_stacks_rejected_and_selection_changes_do_not_retarget() {
    let mut app = app();
    let first = app.session.doc().unwrap().selection.subjects()[0];
    app.run("shape.rectangle", json!({"x": 10, "y": 10, "width": 20, "height": 20})).unwrap();
    let second = app.session.doc().unwrap().selection.subjects()[0];
    app.run("effect.apply", json!({"effect": "blur.gaussian"})).unwrap();
    app.run("select.set", json!({"ids": [first.0, second.0]})).unwrap();
    assert!(app.run("effect.gallery", json!({})).unwrap_err().contains("different effect stacks"));
    app.run("select.set", json!({"ids": [first.0]})).unwrap();
    app.run("effect.gallery", json!({})).unwrap();
    app.run("select.set", json!({"ids": [second.0]})).unwrap();
    set(&mut app, "effects", effects());
    crate::dialogs::confirm(&mut app).unwrap();
    let doc = &app.session.doc().unwrap().doc;
    assert_eq!(doc.node(NodeId(first.0)).unwrap().appearance.effects.len(), 2);
    assert_eq!(doc.node(NodeId(second.0)).unwrap().appearance.effects.len(), 1);
}

#[test]
fn sparse_native_stack_is_unchanged_by_opening_the_gallery() {
    let mut app = app();
    app.run("effect.setStack", json!({"effects": [{"id": "blur.smart", "params": {"radius": 9}}]})).unwrap();
    let original = stack(&app);
    assert_eq!(original[0].params, json!({"radius": 9}));
    let history = app.session.doc().unwrap().history.undo.len();
    app.run("effect.gallery", json!({})).unwrap();
    frame(&mut app);
    crate::dialogs::confirm(&mut app).unwrap();
    assert_eq!(stack(&app), original);
    assert_eq!(app.session.doc().unwrap().history.undo.len(), history);
}

#[test]
fn cancel_after_tab_switch_never_cancels_another_documents_interaction() {
    let mut app = app();
    app.run("effect.gallery", json!({})).unwrap();
    set(&mut app, "effects", effects());
    frame(&mut app);
    // Document switching already rolls back the departing document's interaction.
    app.session.execute("file.new", &json!({"width": 200, "height": 200})).unwrap();
    app.session.execute("shape.rectangle", &json!({"x": 10, "y": 10, "width": 20, "height": 20})).unwrap();
    app.session.begin_interaction("Other document").unwrap();
    app.session.preview("effect.apply", &json!({"effect": "blur.gaussian"})).unwrap();
    assert!(crate::dialogs::confirm(&mut app).is_err());
    crate::dialogs::cancel(&mut app);
    assert!(app.session.in_interaction());
    assert_eq!(stack(&app).len(), 1);
    app.session.cancel_interaction().unwrap();
    assert!(app.session.set_active(0));
    assert!(stack(&app).is_empty());
}

#[test]
fn gallery_buttons_add_reorder_hide_and_delete_using_pointer_events() {
    use egui::{Event, Pos2, Rect};
    let mut app = app();
    app.run("effect.gallery", json!({})).unwrap();
    let ctx = egui::Context::default();
    crate::theme::install_fonts(&ctx);
    crate::theme::apply(&ctx, Default::default());
    let draw = |app: &mut VectorcraftApp, events| {
        let raw = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1280.0, 1000.0))), events, ..Default::default() };
        let mut output = ctx.run_ui(raw, |ui| crate::dialogs::show(app, ui.ctx()));
        output.textures_delta.clear();
        fn collect(shape: &egui::Shape, text: &mut Vec<(String, Rect)>) {
            match shape {
                egui::Shape::Text(t) => text.push((t.galley.text().to_string(), t.visual_bounding_rect())),
                egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| collect(s, text)),
                _ => {}
            }
        }
        let mut text = vec![];
        output.shapes.iter().for_each(|c| collect(&c.shape, &mut text));
        text
    };
    for _ in 0..4 {
        draw(&mut app, vec![]);
    }
    let click = |app: &mut VectorcraftApp, label: &str| {
        let text = draw(app, vec![]);
        let pos = text.iter().find(|(s, _)| s == label).unwrap_or_else(|| panic!("{label}: {text:?}")).1.center();
        let event = |pressed| Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
        draw(app, vec![Event::PointerMoved(pos), event(true)]);
        draw(app, vec![event(false)]);
        draw(app, vec![]);
    };
    let revision = app.session.doc().unwrap().revision;
    click(&mut app, "+");
    assert_eq!(preview_view(app.ui.dialog.as_ref().unwrap()).0, 2.0);
    click(&mut app, "Fit");
    assert_eq!(preview_view(app.ui.dialog.as_ref().unwrap()), (1.0, egui::pos2(0.5, 0.5)));
    assert_eq!(app.session.doc().unwrap().revision, revision, "preview navigation must not edit the document");
    click(&mut app, "Add Effect");
    assert_eq!(stack(&app)[0].id, "blur.gaussian");
    set(&mut app, "__catalog", json!("sharpen.unsharpMask"));
    click(&mut app, "Add Effect");
    assert_eq!(stack(&app)[1].id, "sharpen.unsharpMask");
    click(&mut app, "Move Up");
    assert_eq!(stack(&app)[0].id, "sharpen.unsharpMask");
    // The visibility checkbox is directly before the selected stack row's label.
    let text = draw(&mut app, vec![]);
    let row = text.iter().find(|(s, _)| s == "Unsharp Mask").unwrap().1;
    let pos = egui::pos2(row.left() - 20.0, row.center().y);
    let event = |pressed| Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
    draw(&mut app, vec![Event::PointerMoved(pos), event(true)]);
    draw(&mut app, vec![event(false)]);
    draw(&mut app, vec![]);
    assert!(!stack(&app)[0].visible);
    click(&mut app, "Delete");
    assert_eq!(stack(&app).len(), 1);
    assert_eq!(stack(&app)[0].id, "blur.gaussian");
    click(&mut app, "Cancel");
    assert!(stack(&app).is_empty());
}

#[test]
fn gallery_remains_on_screen_in_a_small_window() {
    let mut app = app();
    app.run("effect.gallery", json!({})).unwrap();
    set(&mut app, "effects", effects());
    let ctx = egui::Context::default();
    crate::theme::install_fonts(&ctx);
    crate::theme::apply(&ctx, Default::default());
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(620.0, 580.0));
    for _ in 0..6 {
        let raw = egui::RawInput { screen_rect: Some(screen), ..Default::default() };
        ctx.run_ui(raw, |ui| crate::dialogs::show(&mut app, ui.ctx())).textures_delta.clear();
    }
    let dialog = ctx.memory(|m| m.area_rect(egui::Id::new(("dialog", KIND)))).unwrap();
    assert!(screen.contains_rect(dialog.shrink(0.5)), "dialog extends offscreen: {dialog:?}");
    crate::dialogs::cancel(&mut app);
}

#[test]
fn unchanged_invalid_gallery_parameters_do_not_create_a_repaint_loop() {
    let mut app = app();
    app.run("effect.gallery", json!({})).unwrap();
    set(&mut app, "effects", json!([{"id": "missing.effect", "params": {}}]));
    frame(&mut app);
    let revision = app.session.doc().unwrap().revision;
    frame(&mut app);
    assert_eq!(app.session.doc().unwrap().revision, revision);
    assert!(!app.session.in_interaction());
    set(&mut app, "effects", effects());
    frame(&mut app);
    assert_eq!(stack(&app).len(), 2);
    crate::dialogs::confirm(&mut app).unwrap();
}

#[test]
fn gallery_exposes_pixelate_parameters_and_smart_modes_without_losing_history() {
    let mut app = app();
    let before = app.session.doc().unwrap().history.undo.len();
    app.run("effect.gallery", json!({})).unwrap();
    set(
        &mut app,
        "effects",
        json!([
            {"id": "pixelate.mosaic", "params": {"cellSize": 15}, "visible": true},
            {"id": "pixelate.colorHalftone", "params": {"radius": 6, "angle1": 15, "angle2": 45, "angle3": 75}, "visible": true},
            {"id": "blur.smart", "params": {"mode": "edgeOnly"}, "visible": true}
        ]),
    );
    set(&mut app, "__catalog", json!("pixelate.mosaic"));
    let text = frame(&mut app);
    assert!(text.contains("Pixelate / Mosaic") && text.contains("Cell Size"), "{text}");
    set(&mut app, "selected", json!(1));
    assert!(frame(&mut app).contains("Angle1"));
    set(&mut app, "selected", json!(2));
    assert!(frame(&mut app).contains("Edge Only"));
    let kept = stack(&app);
    crate::dialogs::confirm(&mut app).unwrap();
    assert_eq!(app.session.doc().unwrap().history.undo.len(), before + 1);
    app.run("edit.undo", json!({})).unwrap();
    assert!(stack(&app).is_empty());
    app.run("edit.redo", json!({})).unwrap();
    assert_eq!(stack(&app), kept);
}

#[test]
fn gallery_preview_navigation_is_bounded_and_never_edits_the_document() {
    let mut app = app();
    app.run("effect.gallery", json!({})).unwrap();
    let before = app.session.doc().unwrap().doc.clone();
    let revision = app.session.doc().unwrap().revision;
    let history = app.session.doc().unwrap().history.undo.len();
    set(&mut app, "previewZoom", json!(1e308));
    set(&mut app, "previewPanX", json!(-1e308));
    set(&mut app, "previewPanY", json!(1e308));
    let (zoom, center) = preview_view(app.ui.dialog.as_ref().unwrap());
    assert_eq!(zoom, 4.0);
    assert_eq!(center, egui::pos2(0.125, 0.875));
    assert!(frame(&mut app).contains("400%"));
    assert_eq!(app.session.doc().unwrap().doc, before);
    assert_eq!(app.session.doc().unwrap().revision, revision);
    crate::dialogs::confirm(&mut app).unwrap();
    assert_eq!(app.session.doc().unwrap().history.undo.len(), history);
}
