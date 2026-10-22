//! The insert bar and quick styles (UX5): the bar's items and their automation ids, a click on
//! it inserting at the page's centre in view (one undo step), type inside a shape from the task
//! bar and a double-click, the quick style pickers, and Copy as PNG onto the system clipboard.

use std::cell::RefCell;
use std::rc::Rc;

use egui::{Event, PointerButton, Pos2, Rect, pos2, vec2};
use serde_json::{Value, json};
use vectorcraft_doc::{NodeId, NodeKind};
use vectorcraft_engine::Session;
use vectorcraft_engine::cmd::clipboard::{Flavour, PNG};

use crate::{Services, SystemClipboard, VectorcraftApp};

#[derive(Clone, Default)]
struct Board(Rc<RefCell<Vec<Flavour>>>);

impl SystemClipboard for Board {
    fn write(&mut self, flavours: &[Flavour]) -> Result<(), String> {
        *self.0.borrow_mut() = flavours.to_vec();
        Ok(())
    }
    fn holds_ours(&mut self) -> bool {
        true
    }
    fn read(&mut self, _: &[&'static str]) -> Option<Flavour> {
        None
    }
    fn has(&mut self, _: &[&'static str]) -> bool {
        false
    }
}

fn app() -> (VectorcraftApp, Board) {
    let board = Board::default();
    let services = Services { system_clipboard: Some(Box::new(board.clone())), ..Default::default() };
    let mut app = VectorcraftApp::new(Session::new(), services);
    app.run("file.new", json!({"width": 800, "height": 600})).unwrap();
    (app, board)
}

/// One frame of the canvas area (tabs, insert bar, canvas) with `events`.
fn frame(app: &mut VectorcraftApp, ctx: &egui::Context, events: Vec<Event>) {
    let raw = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1200.0, 800.0))), events, ..Default::default() };
    let mut out = ctx.run_ui(raw, |ui| {
        crate::chrome::doc_tabs(app, ui);
        crate::insert_bar::show(app, ui);
        crate::canvas::show(app, ui);
    });
    out.textures_delta.clear();
}

fn context() -> egui::Context {
    let ctx = egui::Context::default();
    crate::theme::install_fonts(&ctx);
    ctx
}

/// The bar's items as `ui.inspect` lists them: (automation id, centre).
fn items(app: &VectorcraftApp, ctx: &egui::Context) -> Vec<(String, Pos2)> {
    let v = crate::insert_bar::inspect(app, ctx);
    v["items"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|i| {
                    let r: Vec<f32> = i["rect"].as_array().unwrap().iter().map(|x| x.as_f64().unwrap() as f32).collect();
                    (i["id"].as_str().unwrap().to_string(), pos2(r[0] + r[2] / 2.0, r[1] + r[3] / 2.0))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn click(app: &mut VectorcraftApp, ctx: &egui::Context, at: Pos2) {
    let m = egui::Modifiers::NONE;
    frame(app, ctx, vec![Event::PointerMoved(at)]);
    frame(app, ctx, vec![Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: true, modifiers: m }]);
    frame(app, ctx, vec![Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: false, modifiers: m }]);
    frame(app, ctx, vec![]);
}

fn selected(app: &VectorcraftApp) -> Vec<NodeId> {
    app.session.active().unwrap().selection.objects.clone()
}

#[test]
fn the_insert_bar_has_its_items_with_automation_ids_and_layouts_can_hide_it() {
    let (mut app, _) = app();
    let ctx = context();
    frame(&mut app, &ctx, vec![]);
    let ids: Vec<String> = items(&app, &ctx).into_iter().map(|i| i.0).collect();
    for want in ["insert.shapes", "insert.text", "insert.icons", "insert.image", "insert.style", "insert.theme", "insert.copyPng", "insert.exportPng"]
    {
        assert!(ids.iter().any(|i| i == want), "{want} in {ids:?}");
    }
    // The control channel's inspect has them.
    assert!(crate::control::inspect(&app, &ctx)["insertBar"]["items"].as_array().is_some_and(|a| a.len() == ids.len()));
    // Shown for everyone by default; a layout (or the command) can hide it.
    assert!(crate::state::UiState::default().insert_bar);
    assert_eq!(app.run("window.insertBar", json!({})).unwrap(), json!(false));
    frame(&mut app, &ctx, vec![]);
    assert!(crate::insert_bar::inspect(&app, &ctx).is_null());
    app.run("window.insertBar", json!({"show": true})).unwrap();
    // The classic layout hides it; the default one shows it.
    let layouts = crate::workspaces::builtins();
    let classic = layouts.iter().find(|w| w.name == "Classic").unwrap();
    crate::workspaces::apply(&mut app.ui, classic);
    assert!(!app.ui.insert_bar);
    crate::workspaces::apply(&mut app.ui, &layouts[0]);
    assert!(app.ui.insert_bar);
    // Not on Home.
    app.run("app.home", json!({})).unwrap();
    assert!(!crate::insert_bar::visible(&app));
}

#[test]
fn clicking_text_inserts_a_text_box_at_the_page_centre_in_one_undo_step() {
    let (mut app, _) = app();
    let ctx = context();
    frame(&mut app, &ctx, vec![]);
    frame(&mut app, &ctx, vec![]);
    let text = items(&app, &ctx).into_iter().find(|i| i.0 == "insert.text").unwrap().1;
    let undo = app.session.active().unwrap().history.undo.len();
    click(&mut app, &ctx, text);
    let st = app.session.active().unwrap();
    assert_eq!(st.history.undo.len(), undo + 1, "one undo step");
    let [id] = selected(&app)[..] else { panic!("the new text box is selected") };
    let st = app.session.active().unwrap();
    let n = st.doc.node(id).unwrap();
    assert!(matches!(n.kind, NodeKind::Text(_)));
    let b = n.geometric_bounds().unwrap();
    let page = st.doc.artboards[0].rect;
    assert!((b.center().x - page.center().x).abs() < 1.0 && b.contains(page.center()), "{b:?} centred on {page:?}");
    assert_eq!(app.session.tool_id(), "selection", "ready to move and resize");
    app.run("edit.undo", json!({})).unwrap();
    assert!(app.session.active().unwrap().doc.node(id).is_none());
}

#[test]
fn inserts_go_to_the_centre_of_the_page_in_view() {
    let (mut app, _) = app();
    let ctx = context();
    frame(&mut app, &ctx, vec![]);
    let v = app.view_mut().unwrap();
    v.center = vectorcraft_geom::Point::new(150.0, 100.0);
    let r = app.run(
        "insert.shape",
        crate::insert_bar::target(&app)
            .as_object()
            .map(|o| {
                let mut o = o.clone();
                o.insert("shape".into(), json!("star"));
                Value::Object(o)
            })
            .unwrap(),
    );
    let id = NodeId(r.unwrap()["id"].as_u64().unwrap());
    let b = app.session.active().unwrap().doc.node(id).unwrap().geometric_bounds().unwrap();
    assert!((b.center().x - 150.0).abs() < 1.0, "{b:?}");
    // Scrolled off the page: its centre.
    app.view_mut().unwrap().center = vectorcraft_geom::Point::new(-5000.0, -5000.0);
    assert_eq!(crate::insert_bar::target(&app)["center"], json!([400.0, 300.0]));
}

#[test]
fn add_text_puts_type_inside_the_shape_and_starts_typing() {
    let (mut app, _) = app();
    let ctx = context();
    frame(&mut app, &ctx, vec![]);
    let shape = NodeId(app.run("insert.shape", json!({"shape": "ellipse"})).unwrap()["id"].as_u64().unwrap());
    let r = app.run("quick.typeInShape", json!({})).unwrap();
    let text = NodeId(r["id"].as_u64().unwrap());
    assert_eq!(app.session.tool_id(), "type");
    assert_eq!(app.session.tool_options()["editing"], json!(text.0), "editing the new type");
    let st = app.session.active().unwrap();
    assert_eq!(st.doc.parent_of(shape), st.doc.parent_of(text), "grouped with the shape");
    // Typing goes into it.
    let view = app.view_info();
    app.session.tool_text("Hi", view).unwrap();
    let NodeKind::Text(t) = &app.session.active().unwrap().doc.node(text).unwrap().kind else { panic!() };
    assert_eq!(t.plain_text(), "Hi");
    // After a double-click picked it (the select tool selects it and switches to Type), it is
    // edited again.
    app.select_tool("selection");
    app.run("select.set", json!({"ids": [text.0]})).unwrap();
    app.select_tool("type");
    assert!(crate::insert_bar::edit_shape_text(&mut app));
    assert_eq!(app.session.tool_options()["editing"], json!(text.0));
}

#[test]
fn quick_style_pickers_apply_to_the_selection_or_set_the_next_look() {
    let (mut app, _) = app();
    let id = NodeId(app.run("insert.shape", json!({"shape": "rectangle"})).unwrap()["id"].as_u64().unwrap());
    crate::quick_styles::apply_style(&mut app, "outline");
    let st = app.session.active().unwrap();
    assert!(st.doc.node(id).unwrap().appearance.fill_paint().is_none());
    assert_eq!(crate::quick_styles::current(&app).1, "outline");
    app.run("select.none", json!({})).unwrap();
    crate::quick_styles::apply_style(&mut app, "dark");
    let st = app.session.active().unwrap();
    assert!(st.doc.node(id).unwrap().appearance.fill_paint().is_none(), "nothing selected: the art is left alone");
    assert_eq!(crate::quick_styles::current(&app).1, "dark");
    // Every style and theme has a translated name.
    for s in vectorcraft_engine::cmd::quick::STYLES {
        assert!(!crate::quick_styles::style_label(s.id).is_empty(), "{}", s.id);
    }
    for t in vectorcraft_engine::cmd::quick::THEMES {
        assert!(!crate::quick_styles::theme_label(t.id).is_empty(), "{}", t.id);
    }
    for s in vectorcraft_engine::cmd::quick::SHAPES {
        assert!(!crate::insert_bar::shape_label(s.id).is_empty(), "{}", s.id);
    }
    for (i, _, _) in vectorcraft_engine::cmd::quick::ICONS {
        assert!(!crate::insert_bar::icon_label(i).is_empty(), "{i}");
    }
}

#[test]
fn copy_as_png_puts_a_picture_on_the_system_clipboard() {
    let (mut app, board) = app();
    app.run("insert.shape", json!({"shape": "rectangle", "size": [100, 50]})).unwrap();
    let r = app.run("quick.copyPng", json!({})).unwrap();
    assert_eq!((r["width"].as_u64(), r["height"].as_u64()), (Some(200), Some(100)));
    let got = board.0.borrow().clone();
    assert_eq!(got.len(), 1, "a picture only, so slides and chats paste an image");
    assert_eq!(got[0].mime, PNG);
    assert_eq!(&got[0].data[..4], b"\x89PNG");
    // The objects stay on the app's own clipboard.
    assert!(!app.session.clipboard.is_empty());
    // Nothing selected: the page.
    app.run("select.none", json!({})).unwrap();
    let r = app.run("quick.copyPng", json!({"scale": 1})).unwrap();
    assert_eq!((r["width"].as_u64(), r["height"].as_u64()), (Some(800), Some(600)));
}

/// Every text shape `f` paints, one per line.
fn painted_text(f: impl FnMut(&mut egui::Ui)) -> String {
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
    let ctx = context();
    let raw = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1200.0, 800.0))), ..Default::default() };
    let mut f = f;
    let mut out = ctx.run_ui(raw, |ui| f(ui));
    out.textures_delta.clear();
    let mut s = String::new();
    out.shapes.iter().for_each(|c| walk(&c.shape, &mut s));
    s
}

#[test]
fn the_empty_page_hint_points_at_the_insert_bar_and_the_tour_counts_its_inserts() {
    let (mut app, _) = app();
    // The empty page's hint names the bar (and not File › Place, one of its items); hidden, it
    // names File › Place again.
    let shown = painted_text(|ui| crate::canvas::show(&mut app, ui));
    assert!(shown.contains("Insert bar above") && !shown.contains("File › Place"), "{shown}");
    app.run("window.insertBar", json!({"show": false})).unwrap();
    let shown = painted_text(|ui| crate::canvas::show(&mut app, ui));
    assert!(shown.contains("File › Place"), "{shown}");
    app.run("window.insertBar", json!({"show": true})).unwrap();

    // Learn the Basics: a rectangle from the bar draws the rectangle, its Text the text.
    app.run("help.tour", json!({})).unwrap();
    assert_eq!(app.ui.tour.step, Some(0));
    app.run("insert.shape", json!({"shape": "rectangle"})).unwrap();
    crate::onboarding::watch(&mut app);
    assert_eq!(app.ui.tour.step, Some(1), "a rectangle from the bar finishes step 1");
    app.run("help.tour", json!({"action": "skip"})).unwrap();
    app.run("help.tour", json!({"action": "skip"})).unwrap();
    assert_eq!(app.ui.tour.step, Some(3), "Add some text");
    crate::onboarding::watch(&mut app);
    app.run("insert.textBox", json!({})).unwrap();
    crate::onboarding::watch(&mut app);
    assert_eq!(app.ui.tour.step, Some(4), "a text box from the bar adds the text");
}
