//! The 3D and Materials panel drawn headless: applying an object type, its three tabs, a slider
//! drag as one undo step, the material presets and the light sphere, and the Effect menu items.

use serde_json::{Value, json};

use super::tests_appearance::{app_with_rect, click, frame_events, run, text_rect};
use super::three_d::{ID, Tab, light_geometry, show};
use super::*;
use crate::tests_labels::painted_text;

fn params(app: &mut VectorcraftApp) -> Value {
    app.session.execute("threeD.settings", &json!({})).unwrap()["params"].clone()
}

fn ctx() -> egui::Context {
    let ctx = egui::Context::default();
    crate::theme::install_fonts(&ctx);
    ctx
}

/// Drag from `from` to `to` in the panel, over a few frames.
fn drag(ctx: &egui::Context, app: &mut VectorcraftApp, from: egui::Pos2, to: egui::Pos2) {
    let button = |pos, pressed| egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
    let mid = from + (to - from) * 0.5;
    for e in [egui::Event::PointerMoved(from), button(from, true), egui::Event::PointerMoved(mid), egui::Event::PointerMoved(to), button(to, false)] {
        frame_events(ctx, app, vec![e], show);
    }
    frame_events(ctx, app, vec![], show);
}

#[test]
fn the_panel_is_listed_and_the_effect_menu_opens_it() {
    assert!(crate::state::ICON_PANELS.iter().any(|(id, ..)| *id == ID));
    let tree = crate::menus::menu_tree();
    let (_, window) = tree.iter().find(|(t, _)| *t == "Window").unwrap();
    assert!(window.iter().any(|i| matches!(i, crate::menus::Item::Cmd("3D and Materials", "window.panel", p) if p["panel"] == ID)));
    let (_, effect) = tree.iter().find(|(t, _)| *t == "Effect").unwrap();
    let Some(crate::menus::Item::Sub(_, three_d)) = effect.iter().find(|i| matches!(i, crate::menus::Item::Sub("3D and Materials", _))) else {
        panic!("Effect › 3D and Materials")
    };
    let labels: Vec<&str> = three_d
        .iter()
        .filter_map(|i| match i {
            crate::menus::Item::Cmd(l, "ui.threeD", _) | crate::menus::Item::Sub(l, _) => Some(*l),
            _ => None,
        })
        .collect();
    assert_eq!(labels, ["Extrude & Bevel…", "Revolve…", "Inflate…", "Rotate…", "Materials…", "3D (Classic)"]);
    let Some(crate::menus::Item::Sub(_, classic)) = three_d.last() else { panic!() };
    assert_eq!(classic.len(), 3, "Extrude & Bevel, Revolve and Rotate (Classic)");
    // Inflate… applies the effect as Inflate and shows the panel on its Object tab.
    let mut app = app_with_rect();
    app.ui.three_d.tab = Tab::Lighting;
    app.run("ui.threeD", json!({"object": "inflate"})).unwrap();
    assert_eq!((params(&mut app)["object"].clone(), app.ui.open_panel.as_deref(), app.ui.three_d.tab), (json!("inflate"), Some(ID), Tab::Object));
    // Materials… on art without the effect: a Plane, on the Materials tab.
    let mut app = app_with_rect();
    app.run("ui.threeD", json!({"tab": "materials"})).unwrap();
    assert_eq!((params(&mut app)["object"].clone(), app.ui.three_d.tab), (json!("plane"), Tab::Materials));
    // The Appearance panel's effect name opens the panel too.
    app.ui.open_panel = None;
    app.run("effect.dialog", json!({"effect": "threeD.materials", "index": 0})).unwrap();
    assert!(app.ui.dialog.is_none() && app.ui.open_panel.as_deref() == Some(ID));
}

#[test]
fn without_the_effect_the_object_types_apply_it() {
    let mut app = app_with_rect();
    let text = painted_text(&mut app, show);
    assert!(text.contains("Turn the selection into a 3D object:") && text.contains("Inflate"), "{text}");
    let ctx = ctx();
    let texts = frame_events(&ctx, &mut app, vec![], show);
    click(&ctx, &mut app, text_rect(&texts, "Revolve").center(), show);
    assert_eq!(params(&mut app)["object"], json!("revolve"));
    run(&mut app, "select.none", json!({}));
    assert!(painted_text(&mut app, show).contains("No Selection"));
}

#[test]
fn the_tabs_show_the_settings_of_each_part() {
    let mut app = app_with_rect();
    app.run("threeD.apply", json!({"params": {"bevel": true, "shadow": true}})).unwrap();
    let text = painted_text(&mut app, show);
    for s in ["Object", "Materials", "Lighting", "Depth", "Twist", "Taper", "Bevel", "Profile", "Rotation", "Off-Axis Front", "Perspective"] {
        assert!(text.contains(s), "{s} in {text}");
    }
    app.ui.three_d.tab = Tab::Materials;
    let text = painted_text(&mut app, show);
    for s in ["Default", "Gold", "Rubber", "Roughness", "Metallic", "Material Color"] {
        assert!(text.contains(s), "{s} in {text}");
    }
    app.ui.three_d.tab = Tab::Lighting;
    let text = painted_text(&mut app, show);
    for s in ["Standard", "Top Left", "Light 1", "Intensity", "Softness", "Ambient Light", "Shadows", "Below Object", "Shadow Bounds"] {
        assert!(text.contains(s), "{s} in {text}");
    }
    run(&mut app, "threeD.set", json!({"params": {"object": "inflate"}}));
    app.ui.three_d.tab = Tab::Object;
    let text = painted_text(&mut app, show);
    assert!(text.contains("Volume") && text.contains("Inflate Both Sides") && !text.contains("Twist"), "{text}");
}

#[test]
fn a_slider_drag_is_one_undo_step() {
    let mut app = app_with_rect();
    app.run("threeD.apply", json!({})).unwrap();
    let ctx = ctx();
    frame_events(&ctx, &mut app, vec![], show);
    let texts = frame_events(&ctx, &mut app, vec![], show);
    let label = text_rect(&texts, "Twist");
    // The slider runs from just right of the label to the number field.
    let y = label.center().y;
    drag(&ctx, &mut app, egui::pos2(label.left() + 100.0, y), egui::pos2(label.left() + 160.0, y));
    let twist = params(&mut app)["twist"].as_f64().unwrap();
    assert!(twist.abs() > 1.0, "the drag twisted it: {twist}");
    app.run("edit.undo", json!({})).unwrap();
    assert_eq!(params(&mut app)["twist"], json!(0.0), "one undo step");
}

#[test]
fn material_presets_and_lights_are_set_from_the_panel() {
    let mut app = app_with_rect();
    app.run("threeD.apply", json!({})).unwrap();
    app.ui.three_d.tab = Tab::Materials;
    let ctx = ctx();
    frame_events(&ctx, &mut app, vec![], show);
    let texts = frame_events(&ctx, &mut app, vec![], show);
    click(&ctx, &mut app, text_rect(&texts, "Gold").center(), show);
    let p = params(&mut app);
    assert_eq!((p["material"].clone(), p["metallic"].clone()), (json!("gold"), json!(1.0)));
    // Lighting: a preset, then a light added and moved on the sphere.
    app.ui.three_d.tab = Tab::Lighting;
    frame_events(&ctx, &mut app, vec![], show);
    let texts = frame_events(&ctx, &mut app, vec![], show);
    click(&ctx, &mut app, text_rect(&texts, "Diffuse").center(), show);
    assert_eq!(params(&mut app)["lights"].as_array().unwrap().len(), 2);
    app.run("threeD.set", json!({"light": {"index": 2}})).unwrap();
    assert_eq!(params(&mut app)["lights"].as_array().unwrap().len(), 3);
    // The sphere's light positions map back to their angles.
    let c = egui::pos2(100.0, 100.0);
    for (rotation, height) in [(-45.0, 45.0), (90.0, 10.0), (0.0, 80.0), (180.0, 30.0)] {
        assert_eq!(light_geometry(c, 50.0, rotation, height), (rotation, height));
    }
}
