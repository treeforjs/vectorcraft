//! First steps (UX5): Home's examples, the Learn the Basics tour, the empty-canvas hint, the Undo
//! toast after destructive commands and the status bar without developer timings.

use serde_json::json;
use vectorcraft_tools::{PointerEvent, PointerKind};

use crate::{VectorcraftApp, onboarding};

fn app() -> VectorcraftApp {
    VectorcraftApp::new(vectorcraft_engine::Session::new(), crate::Services::default())
}

/// Press, drag and release the active tool over the document, as the canvas does.
fn drag(app: &mut VectorcraftApp, from: (f64, f64), to: (f64, f64)) {
    let view = app.view_info();
    for (kind, (x, y)) in [
        (PointerKind::Down, from),
        (PointerKind::Drag, ((from.0 + to.0) / 2.0, (from.1 + to.1) / 2.0)),
        (PointerKind::Drag, to),
        (PointerKind::Up, to),
    ] {
        crate::canvas::dispatch(app, &PointerEvent::new(kind, x, y), view);
    }
}

fn click(app: &mut VectorcraftApp, at: (f64, f64)) {
    let view = app.view_info();
    for kind in [PointerKind::Down, PointerKind::Up] {
        crate::canvas::dispatch(app, &PointerEvent::new(kind, at.0, at.1), view);
    }
}

/// Every text one frame of `f` drew.
fn texts(f: impl FnMut(&mut egui::Ui)) -> String {
    fn walk(s: &egui::Shape, out: &mut String) {
        match s {
            egui::Shape::Text(t) => {
                out.push_str(t.galley.text());
                out.push('\n');
            }
            egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
            _ => {}
        }
    }
    let ctx = egui::Context::default();
    crate::theme::install_fonts(&ctx);
    let raw = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 800.0))), ..Default::default() };
    let mut f = f;
    // Twice: areas (the tour card, toasts) show from their second frame.
    let mut out = ctx.run_ui(raw.clone(), &mut f);
    out.textures_delta.clear();
    let mut out = ctx.run_ui(raw, &mut f);
    out.textures_delta.clear();
    let mut s = String::new();
    out.shapes.iter().for_each(|c| walk(&c.shape, &mut s));
    s
}

fn step(app: &VectorcraftApp) -> Option<usize> {
    app.ui.tour.step
}

#[test]
fn each_tour_step_advances_when_the_user_does_it() {
    let mut app = app();
    assert_eq!(onboarding::step_titles().len(), 6);
    // Learn the Basics with no document open: a page to draw on, then the first step.
    app.run("help.tour", json!({})).unwrap();
    assert!(app.session.active().is_some(), "the tour opens a page");
    assert_eq!(step(&app), Some(0));
    // Choosing a tool alone does nothing.
    app.select_tool("rectangle");
    onboarding::watch(&mut app);
    assert_eq!(step(&app), Some(0), "not done until the rectangle is drawn");
    // 1. Draw a rectangle with the Rectangle tool.
    drag(&mut app, (100.0, 100.0), (220.0, 180.0));
    onboarding::watch(&mut app);
    assert_eq!(step(&app), Some(1), "drawing the rectangle finishes step 1");
    // 2. Change its fill colour.
    app.run("paint.setFill", json!({"color": "#ff8000"})).unwrap();
    onboarding::watch(&mut app);
    assert_eq!(step(&app), Some(2));
    // 3. Move it.
    app.run("object.move", json!({"dx": 20, "dy": 10})).unwrap();
    onboarding::watch(&mut app);
    assert_eq!(step(&app), Some(3));
    // 4. Add text with the Type tool.
    app.select_tool("type");
    click(&mut app, (300.0, 300.0));
    let view = app.view_info();
    let r = app.session.tool_text("Hello", view);
    crate::canvas::apply_requests(&mut app, r);
    onboarding::watch(&mut app);
    assert_eq!(step(&app), Some(4), "{:?}", app.session.active().map(|d| d.selection.objects.clone()));
    app.run("tool.select", json!({"tool": "selection"})).unwrap();
    onboarding::watch(&mut app);
    assert_eq!(step(&app), Some(4));
    // 5. Undo.
    app.run("edit.undo", json!({})).unwrap();
    onboarding::watch(&mut app);
    assert_eq!(step(&app), Some(5));
    // 6. Find anything: the command search, with its Cmd/Ctrl+K.
    let card = texts(|ui| {
        crate::canvas::show(&mut app, ui);
        onboarding::tour_card(&mut app, ui.ctx());
    });
    assert!(card.contains("Find anything") && card.contains(&crate::menus::pretty_shortcut("Cmd+K")), "{card}");
    app.run("help.commandPalette", json!({})).unwrap();
    onboarding::watch(&mut app);
    assert_eq!(step(&app), Some(6), "the closing card");
    assert!(!app.ui.tour.completed);
    app.run("help.tour", json!({"action": "end"})).unwrap();
    assert_eq!(step(&app), None);
    assert!(app.ui.tour.completed, "finishing is remembered");
    // In the saved UI state.
    let saved = serde_json::to_value(&app.ui).unwrap();
    let back: crate::state::UiState = serde_json::from_value(saved).unwrap();
    assert!(back.tour.completed);
}

#[test]
fn the_tour_has_back_skip_and_end() {
    let mut app = app();
    app.run("help.tour", json!({})).unwrap();
    app.run("help.tour", json!({"action": "skip"})).unwrap();
    app.run("help.tour", json!({"action": "skip"})).unwrap();
    assert_eq!(step(&app), Some(2));
    app.run("help.tour", json!({"action": "back"})).unwrap();
    assert_eq!(step(&app), Some(1));
    app.run("help.tour", json!({"action": "end"})).unwrap();
    assert_eq!(step(&app), None);
    assert!(!app.ui.tour.completed, "ending early isn't finishing");
    assert!(app.run("help.tour", json!({"action": "sideways"})).is_err());
    // The card draws (and names its step) while the tour is open.
    app.run("help.tour", json!({})).unwrap();
    let shown = texts(|ui| {
        crate::canvas::show(&mut app, ui);
        onboarding::tour_card(&mut app, ui.ctx());
    });
    assert!(shown.contains("Draw a rectangle") && shown.contains("Step 1 of 6"), "{shown}");
}

#[test]
fn delete_shows_an_undo_toast_that_undoes() {
    let mut app = app();
    app.run("file.new", json!({})).unwrap();
    app.run("shape.rectangle", json!({"x": 10, "y": 10, "width": 50, "height": 50})).unwrap();
    app.run("shape.ellipse", json!({"x": 100, "y": 10, "width": 50, "height": 50})).unwrap();
    app.run("select.all", json!({})).unwrap();
    assert!(app.onboarding.toasts.items().is_empty(), "drawing isn't destructive");
    app.run("edit.clear", json!({})).unwrap();
    assert!(onboarding::is_empty(&app.session.active().unwrap().doc));
    let toasts = onboarding::toasts_json(&app);
    assert_eq!(toasts, json!([{"message": "Deleted 2 objects", "action": "edit.undo"}]));
    // Its Undo button runs edit.undo: the objects come back.
    let cmd = app.onboarding.toasts.items()[0].action.clone().unwrap().1;
    crate::menus::invoke(&mut app, &cmd, json!({}));
    assert!(!onboarding::is_empty(&app.session.active().unwrap().doc));
    // It draws (above the status bar), with its button.
    let shown = texts(|ui| onboarding::show_toasts(&mut app, ui.ctx()));
    assert!(shown.contains("Deleted 2 objects") && shown.contains("Undo"), "{shown}");
    // Nothing to delete: no toast.
    app.onboarding.toasts.clear();
    app.run("select.none", json!({})).ok();
    let _ = app.run("edit.clear", json!({}));
    assert!(app.onboarding.toasts.items().is_empty());
}

#[test]
fn status_bar_timings_are_hidden_by_default() {
    let mut app = app();
    app.run("file.new", json!({})).unwrap();
    assert!(!app.ui.status_timings);
    let shown = texts(|ui| crate::chrome::status_bar(&mut app, ui));
    assert!(!shown.contains("fps") && !shown.contains("render "), "{shown}");
    assert!(shown.contains("RGB/Preview") || shown.contains("CMYK/Preview"), "the modes moved here from the tab: {shown}");
    // Settings › Performance › Show Frame Timings in the Status Bar.
    crate::prefs_dialog::open(&mut app, Some("Performance"));
    app.ui.dialog.as_mut().unwrap().fields.insert("__statusTimings".into(), json!(true));
    crate::prefs_dialog::confirm(&mut app).unwrap();
    assert!(app.ui.status_timings);
    let shown = texts(|ui| crate::chrome::status_bar(&mut app, ui));
    assert!(shown.contains("fps"), "{shown}");
}

#[test]
fn the_empty_canvas_hint_shows_only_without_art() {
    let mut app = app();
    app.run("file.new", json!({})).unwrap();
    let shown = texts(|ui| crate::canvas::show(&mut app, ui));
    assert!(shown.contains("Your page is empty"), "{shown}");
    // How to draw is the hint bar's: the hint doesn't repeat its keys.
    assert!(shown.contains("The bar below") && !shown.contains("press M"), "{shown}");
    app.run("shape.rectangle", json!({"x": 10, "y": 10, "width": 50, "height": 50})).unwrap();
    let shown = texts(|ui| crate::canvas::show(&mut app, ui));
    assert!(!shown.contains("Your page is empty"), "gone once there is art");
}

#[test]
fn examples_open_from_home_as_current_untitled_documents() {
    let mut app = app();
    // Home lists them with the tour.
    let shown = texts(|ui| crate::canvas::show(&mut app, ui));
    assert!(shown.contains("Open an Example") && shown.contains("Learn the Basics") && shown.contains("Dusk Poster"), "{shown}");
    for ex in onboarding::EXAMPLES {
        // Saved at the current format version (not "[Converted]" when opened from disk), with the
        // preview Home shows.
        let (_, info) = vectorcraft_format::load_info(ex.bytes).unwrap();
        assert!(!info.is_old(), "{} is an old version", ex.name);
        assert!(vectorcraft_format::preview(ex.bytes).is_some(), "{} has no preview", ex.name);
        app.run("help.openExample", json!({"name": ex.name})).unwrap();
        let d = app.session.active().unwrap();
        assert!(!d.title().contains("Converted"), "{}", d.title());
        assert!(d.path.is_none(), "a copy: Save never writes the bundled file");
        assert!(!onboarding::is_empty(&d.doc));
    }
    assert!(app.run("help.openExample", json!({"name": "nope"})).is_err());
}

#[test]
fn help_has_learn_the_basics_and_a_working_help_item() {
    let app = app();
    let entries = serde_json::to_value(crate::menus::menu_entries(&app)).unwrap();
    let flat = entries.to_string();
    assert!(flat.contains("help.tour") && flat.contains("help.docs"), "Help › Learn the Basics and VectorCraft Help…");
    assert_eq!(crate::menus::shortcut_of("help.docs"), Some("F1"));
}
