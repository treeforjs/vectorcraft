//! Headless frames of brush libraries in the library panel: clicking a brush adds and applies it,
//! the Window menu and the Brushes panel open libraries, and Save Brush Library writes
//! `.vcbrushes` files that open as libraries.

use egui::{Event, Modifiers, PointerButton, Pos2, Rect, vec2};
use serde_json::{Value, json};
use vectorcraft_brush::libs::BRUSH_LIBRARIES;
use vectorcraft_engine::Session;

use super::brushes::BrushLibraries;
use super::library_panel::{self, LibraryKind, OpenLibrary};
use crate::VectorcraftApp;

fn app_with(services: crate::Services) -> VectorcraftApp {
    let mut app = VectorcraftApp::new(Session::new(), services);
    app.run("file.new", json!({"width": 200, "height": 200})).unwrap();
    app
}

fn app() -> VectorcraftApp {
    app_with(Default::default())
}

fn context() -> egui::Context {
    let ctx = egui::Context::default();
    crate::theme::install_fonts(&ctx);
    ctx
}

/// One frame of `draw` with `events` and `modifiers` held; the texts it painted.
fn frame(ctx: &egui::Context, events: Vec<Event>, modifiers: Modifiers, time: f64, mut draw: impl FnMut(&mut egui::Ui)) -> Vec<String> {
    fn texts(s: &egui::Shape, out: &mut Vec<String>) {
        match s {
            egui::Shape::Text(t) => out.push(t.galley.text().to_string()),
            egui::Shape::Vec(v) => v.iter().for_each(|s| texts(s, out)),
            _ => {}
        }
    }
    let screen_rect = Some(Rect::from_min_size(Pos2::ZERO, vec2(1200.0, 800.0)));
    let events = std::iter::once(Event::ModifiersChanged(modifiers)).chain(events).collect();
    let input = egui::RawInput { events, time: Some(time), screen_rect, ..Default::default() };
    let mut out = ctx.run_ui(input, &mut draw);
    out.textures_delta.clear();
    let mut v = vec![];
    out.shapes.iter().for_each(|c| texts(&c.shape, &mut v));
    v
}

/// Click the library panel's tile of brush `name` with `modifiers` (a new window takes a sizing
/// frame first).
fn click(app: &mut VectorcraftApp, ctx: &egui::Context, name: &str, modifiers: Modifiers, time: f64) {
    for t in [time, time + 0.25] {
        frame(ctx, vec![], Modifiers::NONE, t, |ui| library_panel::show_window(app, ui.ctx()));
    }
    let id = library_panel::tile_id::<BrushLibraries>(name);
    let at = ctx.read_response(id).unwrap_or_else(|| panic!("no tile for {name}")).rect.center();
    let button = |pressed| Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers };
    frame(ctx, vec![Event::PointerMoved(at), button(true), button(false)], modifiers, time + 0.5, |ui| library_panel::show_window(app, ui.ctx()));
}

fn undo_len(app: &VectorcraftApp) -> usize {
    app.session.doc().unwrap().history.undo.len()
}

fn brush_names(app: &mut VectorcraftApp) -> Vec<String> {
    let v = app.run("brush.list", json!({})).unwrap();
    v["brushes"].as_array().unwrap().iter().map(|b| b["name"].as_str().unwrap().to_string()).collect()
}

#[test]
fn clicking_a_library_brush_adds_and_applies_it_in_one_step() {
    let mut app = app();
    let ctx = context();
    let id = app.run("shape.ellipse", json!({"x": 10, "y": 10, "width": 80, "height": 40})).unwrap()["id"].as_u64().unwrap();
    let r = app.run("window.brushLibrary", json!({"library": "Borders"})).unwrap();
    assert_eq!((r["open"].as_str(), r["name"].as_str()), (Some("borders"), Some("Borders")));
    assert_eq!(app.ui.library_panel, Some(OpenLibrary { kind: "brushes".into(), id: "borders".into() }));
    let undo = undo_len(&app);
    click(&mut app, &ctx, "Waves", Modifiers::NONE, 0.0);
    let node = app.session.doc().unwrap().doc.node(vectorcraft_doc::NodeId(id)).unwrap().clone();
    assert_eq!(node.appearance.stroke().and_then(|s| s.brush.as_deref()), Some("Waves"));
    assert_eq!(undo_len(&app), undo + 1, "added and applied as one undo step");
    let tile = ctx.read_response(library_panel::tile_id::<BrushLibraries>("Waves")).unwrap().rect;
    assert_eq!(tile.width(), 30.0, "brushes open in large thumbnails");
    // Modifier clicks only select; Add to Brushes adds the selection.
    click(&mut app, &ctx, "Rope", Modifiers::COMMAND, 2.0);
    assert!(!brush_names(&mut app).contains(&"Rope".to_string()));
    BrushLibraries::add(&mut app, "borders", vec!["Rope".into(), "Checker".into()]);
    let names = brush_names(&mut app);
    assert!(names.contains(&"Rope".to_string()) && names.contains(&"Checker".to_string()));
    // Find filters by name; list rows describe the brushes; definitions are made once per library.
    let lib = BrushLibraries::get(&app, "borders").unwrap().1;
    let rows = BrushLibraries::rows(&lib, "zig");
    assert_eq!(rows.iter().map(|r| r.name).collect::<Vec<_>>(), ["Zigzag"]);
    assert_eq!(BrushLibraries::describe(rows[0].item.unwrap()), "Pattern Brush");
    assert!(std::sync::Arc::ptr_eq(&lib, &BrushLibraries::get(&app, "borders").unwrap().1));
    assert!(app.run("window.brushLibrary", json!({"library": "nope"})).is_err());
    app.run("window.brushLibrary", json!({"library": null})).unwrap();
    assert!(app.ui.library_panel.is_none());
}

#[test]
fn the_window_menu_and_the_panel_open_every_library() {
    let mut app = app();
    let entries = crate::menus::menu_entries(&app);
    let opened: Vec<Value> = entries
        .iter()
        .filter(|e| e.path == ["Window", "Brush Libraries"] && e.command.as_deref() == Some("window.brushLibrary"))
        .map(|e| e.params["library"].clone())
        .collect();
    assert_eq!(opened, BRUSH_LIBRARIES.iter().map(|b| json!(b.id)).collect::<Vec<_>>());
    let has = |cmd: &str| entries.iter().any(|e| e.path == ["Window", "Brush Libraries"] && e.command.as_deref() == Some(cmd) && e.enabled);
    assert!(has("window.brushLibrary.other") && has("ui.saveBrushLibrary"));
    assert!(!entries.iter().any(|e| e.path == ["Window", "Brush Libraries", "User Defined"]), "empty slots are hidden");
    // The Brushes panel's menu.
    let ctx = context();
    let texts = frame(&ctx, vec![], Modifiers::NONE, 0.0, |ui| {
        super::brushes::show(&mut app, ui);
        super::brushes::menu(&mut app, ui);
    });
    for t in ["Open Brush Library", "Save Brush Library…"] {
        assert!(texts.iter().any(|x| x.ends_with(t)), "{t}: {texts:?}");
    }
    let ids: Vec<String> = BrushLibraries::list(&app).into_iter().map(|l| l.id).collect();
    assert_eq!(ids, BRUSH_LIBRARIES.iter().map(|b| b.id).collect::<Vec<_>>());
}

#[test]
fn save_dialog_writes_user_libraries_that_the_window_menu_lists() {
    let dir = std::env::temp_dir().join(format!("vc-brush-libraries-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut app = app();
    app.session.brush_libraries.set_user_dir(Some(dir.to_string_lossy().to_string()));
    let user_items = |app: &VectorcraftApp| -> Vec<(String, String)> {
        crate::menus::menu_entries(app)
            .into_iter()
            .filter(|e| e.path == ["Window", "Brush Libraries", "User Defined"])
            .map(|e| (e.label, e.command.unwrap_or_default()))
            .collect()
    };
    // The selected path's brush is what Selected Brushes Only saves.
    app.run("shape.ellipse", json!({"x": 10, "y": 10, "width": 80, "height": 40})).unwrap();
    app.run("brush.apply", json!({"name": "Chain"})).unwrap();
    app.run("ui.saveBrushLibrary", json!({})).unwrap();
    let d = app.ui.dialog.as_mut().expect("Save Brush Library opens");
    assert_eq!((d.kind.as_str(), d.bool("user")), (crate::dialogs::save_brush_library::KIND, true));
    d.fields.insert("name".into(), json!("Links"));
    d.fields.insert("selectedOnly".into(), json!(true));
    let ctx = context();
    frame(&ctx, vec![], Modifiers::NONE, 0.0, |ui| crate::dialogs::show(&mut app, ui.ctx()));
    let texts = frame(&ctx, vec![], Modifiers::NONE, 0.5, |ui| crate::dialogs::show(&mut app, ui.ctx()));
    assert!(texts.iter().any(|t| t == "Selected Brushes Only (1)"), "{texts:?}");
    crate::dialogs::confirm(&mut app).unwrap();
    assert!(app.ui.dialog.is_none());
    assert_eq!(user_items(&app), [("Links".to_string(), "window.userBrushLibrary1".to_string())]);
    app.run("window.userBrushLibrary1", json!({})).unwrap();
    assert_eq!(app.ui.library_panel.as_ref().map(|o| o.id.as_str()), Some("user/Links.vcbrushes"));
    let (name, lib) = BrushLibraries::get(&app, "user/Links.vcbrushes").unwrap();
    assert_eq!((name.as_str(), lib.len()), ("Links", 1), "only the selected brush");
    // Its tile draws in the panel.
    frame(&ctx, vec![], Modifiers::NONE, 1.0, |ui| library_panel::show_window(&mut app, ui.ctx()));
    frame(&ctx, vec![], Modifiers::NONE, 1.5, |ui| library_panel::show_window(&mut app, ui.ctx()));
    assert!(ctx.read_response(library_panel::tile_id::<BrushLibraries>("Chain")).is_some());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn saving_to_a_file_and_opening_a_vcbrushes_file() {
    let written = std::rc::Rc::new(std::cell::RefCell::new(vec![]));
    let w = written.clone();
    let services = crate::Services {
        pick_save: Some(Box::new(|p: &crate::FilePick| Some(format!("/tmp/{}", p.name)))),
        write: Some(Box::new(move |p: &str, b: &[u8]| {
            w.borrow_mut().push((p.to_string(), b.to_vec()));
            Ok(())
        })),
        ..Default::default()
    };
    let mut app = app_with(services);
    app.run("ui.saveBrushLibrary", json!({})).unwrap();
    assert!(!app.ui.dialog.as_ref().unwrap().bool("__user"), "no user folder: a file");
    crate::dialogs::confirm(&mut app).unwrap();
    let (path, bytes) = written.borrow()[0].clone();
    assert!(path.ends_with(".vcbrushes"), "{path}");
    // Opening it opens it in the library panel, not as a document.
    crate::io::open_bytes(&mut app, "Saved.vcbrushes", &bytes, None).unwrap();
    let open = app.ui.library_panel.clone().unwrap();
    assert_eq!(open.kind, BrushLibraries::KIND);
    assert_eq!(app.session.documents().len(), 1);
    let other = BrushLibraries::list(&app).into_iter().find(|l| l.id == open.id).unwrap();
    assert_eq!(other.submenu, Some("Other Libraries"));
    assert_eq!(BrushLibraries::get(&app, &open.id).unwrap().1.len(), brush_names(&mut app).len());
}
