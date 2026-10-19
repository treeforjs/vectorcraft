//! Headless frames of symbol libraries in the library panel: clicking a symbol adds it, the Window
//! menu and the Symbols panel open libraries, thumbnails are rendered once, and Save Symbol
//! Library writes `.vcsymbols` files that open as libraries.

use egui::{Event, Modifiers, PointerButton, Pos2, Rect, vec2};
use serde_json::{Value, json};
use vectorcraft_doc::symbol_libs::SYMBOL_LIBRARIES;
use vectorcraft_engine::Session;

use super::library_panel::{self, LibraryKind, OpenLibrary};
use super::symbols::SymbolLibraries;
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

/// One frame of `draw` with `events` and `modifiers` held; the texts it painted and the number of
/// textures it uploaded.
fn frame(ctx: &egui::Context, events: Vec<Event>, modifiers: Modifiers, time: f64, mut draw: impl FnMut(&mut egui::Ui)) -> (Vec<String>, usize) {
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
    let uploads = out.textures_delta.set.len();
    out.textures_delta.clear();
    let mut v = vec![];
    out.shapes.iter().for_each(|c| texts(&c.shape, &mut v));
    (v, uploads)
}

fn panel(app: &mut VectorcraftApp, ctx: &egui::Context, events: Vec<Event>, modifiers: Modifiers, time: f64) -> usize {
    frame(ctx, events, modifiers, time, |ui| library_panel::show_window(app, ui.ctx())).1
}

/// Click the library panel's tile of symbol `name` with `modifiers` (a new window takes a sizing
/// frame first).
fn click(app: &mut VectorcraftApp, ctx: &egui::Context, name: &str, modifiers: Modifiers, time: f64) {
    for t in [time, time + 0.25] {
        panel(app, ctx, vec![], Modifiers::NONE, t);
    }
    let id = library_panel::tile_id::<SymbolLibraries>(name);
    let at = ctx.read_response(id).unwrap_or_else(|| panic!("no tile for {name}")).rect.center();
    let button = |pressed| Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers };
    panel(app, ctx, vec![Event::PointerMoved(at), button(true), button(false)], modifiers, time + 0.5);
}

fn symbols(app: &VectorcraftApp) -> Vec<String> {
    app.session.doc().unwrap().doc.symbols.iter().map(|s| s.name.clone()).collect()
}

#[test]
fn the_library_panel_shows_thumbnails_and_clicking_a_symbol_adds_it() {
    let mut app = app();
    let ctx = context();
    let r = app.run("window.symbolLibrary", json!({"library": "Map Markers"})).unwrap();
    assert_eq!((r["open"].as_str(), r["name"].as_str()), (Some("map-markers"), Some("Map Markers")));
    assert_eq!(app.ui.library_panel, Some(OpenLibrary { kind: "symbols".into(), id: "map-markers".into() }));
    let undo = app.session.doc().unwrap().history.undo.len();
    click(&mut app, &ctx, "Pin", Modifiers::NONE, 0.0);
    assert_eq!(symbols(&app), ["Pin"]);
    assert_eq!(app.session.doc().unwrap().history.undo.len(), undo + 1, "one undo step");
    assert_eq!(app.run("symbol.list", json!({})).unwrap()["current"], "Pin");
    let tile = ctx.read_response(library_panel::tile_id::<SymbolLibraries>("Pin")).unwrap().rect;
    assert_eq!(tile.width(), 30.0, "symbols open in large thumbnails");
    // Thumbnails are rendered once, not every frame.
    let uploads = panel(&mut app, &ctx, vec![], Modifiers::NONE, 2.0);
    assert_eq!(uploads, 0, "no new textures on a frame that shows the same thumbnails");
    // Modifier clicks only select; Add to Symbols adds the selection.
    click(&mut app, &ctx, "Flag", Modifiers::COMMAND, 4.0);
    assert_eq!(symbols(&app), ["Pin"]);
    SymbolLibraries::add(&mut app, "map-markers", vec!["Flag".into(), "Compass".into()]);
    assert_eq!(symbols(&app), ["Pin", "Flag", "Compass"]);
    // Find filters by name; list rows give the natural size.
    let lib = SymbolLibraries::get(&app, "map-markers").unwrap().1;
    let rows = SymbolLibraries::rows(&lib, "comp");
    assert_eq!(rows.iter().map(|r| r.name).collect::<Vec<_>>(), ["Compass"]);
    assert_eq!(SymbolLibraries::describe(rows[0].item.unwrap()), "40 × 40 pt");
    assert!(app.run("window.symbolLibrary", json!({"library": "nope"})).is_err());
    app.run("window.symbolLibrary", json!({"library": null})).unwrap();
    assert!(app.ui.library_panel.is_none());
}

#[test]
fn the_window_menu_and_the_symbols_panel_open_every_library() {
    let mut app = app();
    let entries = crate::menus::menu_entries(&app);
    let opened: Vec<Value> = entries
        .iter()
        .filter(|e| e.path == ["Window", "Symbol Libraries"] && e.command.as_deref() == Some("window.symbolLibrary"))
        .map(|e| e.params["library"].clone())
        .collect();
    assert_eq!(opened, SYMBOL_LIBRARIES.iter().map(|b| json!(b.id)).collect::<Vec<_>>());
    let has = |cmd: &str| entries.iter().any(|e| e.path == ["Window", "Symbol Libraries"] && e.command.as_deref() == Some(cmd) && e.enabled);
    assert!(has("window.symbolLibrary.other") && has("ui.saveSymbolLibrary"));
    assert!(!entries.iter().any(|e| e.path == ["Window", "Symbol Libraries", "User Defined"]), "empty slots are hidden");
    // Every built-in library opens from the menu and draws its tiles.
    let ctx = context();
    for (i, b) in SYMBOL_LIBRARIES.iter().enumerate() {
        app.run("window.symbolLibrary", json!({"library": b.id})).unwrap();
        let t = i as f64;
        panel(&mut app, &ctx, vec![], Modifiers::NONE, t);
        panel(&mut app, &ctx, vec![], Modifiers::NONE, t + 0.5);
        let lib = vectorcraft_doc::symbol_libs::builtin_symbol_library(b.id).unwrap();
        for n in lib.names() {
            assert!(ctx.read_response(library_panel::tile_id::<SymbolLibraries>(n)).is_some(), "{} / {n}", b.id);
        }
    }
    // The Symbols panel's menu.
    let (texts, _) = frame(&ctx, vec![], Modifiers::NONE, 20.0, |ui| {
        super::symbols::show(&mut app, ui);
        super::symbols::menu(&mut app, ui);
    });
    for t in ["Open Symbol Library", "Save Symbol Library…"] {
        assert!(texts.iter().any(|x| x.ends_with(t)), "{t}: {texts:?}");
    }
}

#[test]
fn save_dialog_writes_user_libraries_that_the_window_menu_lists() {
    let dir = std::env::temp_dir().join(format!("vc-symbol-libraries-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut app = app();
    app.session.symbol_libraries.set_user_dir(Some(dir.to_string_lossy().to_string()));
    app.run("symbol.addFromLibrary", json!({"library": "outdoors", "names": ["Sun", "Cloud"]})).unwrap();
    let user_items = |app: &VectorcraftApp| -> Vec<(String, String)> {
        crate::menus::menu_entries(app)
            .into_iter()
            .filter(|e| e.path == ["Window", "Symbol Libraries", "User Defined"])
            .map(|e| (e.label, e.command.unwrap_or_default()))
            .collect()
    };
    app.run("ui.saveSymbolLibrary", json!({"names": ["Cloud"]})).unwrap();
    let d = app.ui.dialog.as_mut().expect("Save Symbol Library opens");
    assert_eq!((d.kind.as_str(), d.bool("user")), (crate::dialogs::save_symbol_library::KIND, true));
    d.fields.insert("name".into(), json!("Weather"));
    d.fields.insert("selectedOnly".into(), json!(true));
    let ctx = context();
    frame(&ctx, vec![], Modifiers::NONE, 0.0, |ui| crate::dialogs::show(&mut app, ui.ctx()));
    let (texts, _) = frame(&ctx, vec![], Modifiers::NONE, 0.5, |ui| crate::dialogs::show(&mut app, ui.ctx()));
    assert!(texts.iter().any(|t| t == "Selected Symbols Only (1)"), "{texts:?}");
    crate::dialogs::confirm(&mut app).unwrap();
    assert!(app.ui.dialog.is_none());
    assert_eq!(user_items(&app), [("Weather".to_string(), "window.userSymbolLibrary1".to_string())]);
    app.run("window.userSymbolLibrary1", json!({})).unwrap();
    assert_eq!(app.ui.library_panel.as_ref().map(|o| o.id.as_str()), Some("user/Weather.vcsymbols"));
    let (name, lib) = SymbolLibraries::get(&app, "user/Weather.vcsymbols").unwrap();
    assert_eq!((name.as_str(), lib.len()), ("Weather", 1), "only the selected symbol");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn saving_to_a_file_and_opening_a_vcsymbols_file() {
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
    app.run("symbol.addFromLibrary", json!({"library": "badges-labels"})).unwrap();
    app.run("ui.saveSymbolLibrary", json!({})).unwrap();
    assert!(!app.ui.dialog.as_ref().unwrap().bool("__user"), "no user folder: a file");
    crate::dialogs::confirm(&mut app).unwrap();
    let (path, bytes) = written.borrow()[0].clone();
    assert!(path.ends_with(".vcsymbols"), "{path}");
    // Opening it opens it in the library panel, not as a document.
    crate::io::open_bytes(&mut app, "Saved.vcsymbols", &bytes, None).unwrap();
    let open = app.ui.library_panel.clone().unwrap();
    assert_eq!(open.kind, SymbolLibraries::KIND);
    assert_eq!(app.session.documents().len(), 1);
    let other = SymbolLibraries::list(&app).into_iter().find(|l| l.id == open.id).unwrap();
    assert_eq!(other.submenu, Some("Other Libraries"));
    assert_eq!(SymbolLibraries::get(&app, &open.id).unwrap().1.len(), app.session.doc().unwrap().doc.symbols.len());
}
