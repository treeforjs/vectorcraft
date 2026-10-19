//! 3D and Materials panel (Window › 3D and Materials; Effect › 3D and Materials): the selected
//! object's 3D and Materials effect in three tabs.
//!
//! - **Object**: the object type (Plane, Extrude, Revolve, Inflate) and its options (depth, cap,
//!   twist, taper and the bevel; the revolve's angle, offset and edge; the inflate's volume and
//!   sides), and the rotation (a named view, X/Y/Z and perspective).
//! - **Materials**: the material presets, drawn as lit spheres, and roughness, metallic and an
//!   optional material colour.
//! - **Lighting**: the lighting presets, a sphere showing the lights (drag on it to move the
//!   chosen light), the lights (add, remove, colour, intensity, rotation, height, softness),
//!   ambient light and the shadow.
//!
//! Every control acts through `threeD.set` (sliders preview while dragged and land as one undo
//! step); with nothing 3D selected, the object types apply the effect (`threeD.apply`). Widget ids
//! start with `3d-` followed by the setting they edit (`3d-depth`, `3d-light-sphere`…).

use std::ops::RangeInclusive;

use egui::{Color32, Mesh, Pos2, Sense, Stroke, Ui, pos2, vec2};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use vectorcraft_effects::{BEVEL_PROFILES, LIGHTING_PRESETS, MATERIAL_PRESETS, MAX_LIGHTS, OBJECT_TYPES, ROTATION_PRESETS, preview_shade};

use super::live_run;
use crate::VectorcraftApp;
use crate::theme::Tokens;
use crate::widgets::{self, Live};

pub const ID: &str = "threeDMaterials";

/// The undo label of the panel's edits.
const LABEL: &str = "3D and Materials";
const LABEL_W: f32 = 78.0;
const FIELD_W: f32 = 58.0;

/// The panel's tabs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Tab {
    #[default]
    Object,
    Materials,
    Lighting,
}

impl Tab {
    const ALL: [Tab; 3] = [Tab::Object, Tab::Materials, Tab::Lighting];

    fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.id().eq_ignore_ascii_case(s))
    }

    fn id(self) -> &'static str {
        match self {
            Tab::Object => "object",
            Tab::Materials => "materials",
            Tab::Lighting => "lighting",
        }
    }
}

/// What the panel shows (UI state, saved with the workspace): its tab and the light being edited.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PanelState {
    pub tab: Tab,
    pub light: usize,
}

/// `ui.threeD {object?, tab?}`: apply the effect as `object` (when given), then show the panel on
/// `tab`. Materials… gives a selection without the effect a Plane to show its material on.
pub fn open(app: &mut VectorcraftApp, p: &Value) -> Result<Value, String> {
    let tab = p.get("tab").and_then(Value::as_str).and_then(Tab::parse);
    let mut out = Value::Null;
    if let Some(object) = p.get("object").and_then(Value::as_str) {
        out = app.run("threeD.apply", json!({ "object": object }))?;
    } else if tab == Some(Tab::Materials) && settings_now(app)["params"].is_null() && super::selection_len(app) > 0 {
        out = app.run("threeD.apply", json!({"object": "plane"}))?;
    }
    app.ui.three_d.tab = tab.unwrap_or(Tab::Object);
    if app.ui.open_panel.as_deref() != Some(ID) || crate::floating::group_of(&app.ui, ID).is_some() {
        app.run("window.panel", json!({ "panel": ID }))?;
    }
    Ok(out)
}

fn settings_now(app: &mut VectorcraftApp) -> Value {
    app.session.execute("threeD.settings", &json!({})).unwrap_or(Value::Null)
}

/// `threeD.settings`, asked again only when the document or the selection changes.
fn settings(app: &mut VectorcraftApp, ctx: &egui::Context) -> Option<Value> {
    type Key = (u64, u64, Vec<u64>);
    let st = app.session.active()?;
    let key: Key = (st.uid, st.revision, st.selection.objects.iter().map(|i| i.0).collect());
    let id = egui::Id::new("3d-settings");
    if let Some((k, v)) = ctx.data(|d| d.get_temp::<(Key, Value)>(id))
        && k == key
    {
        return Some(v);
    }
    let v = settings_now(app);
    ctx.data_mut(|d| d.insert_temp(id, (key, v.clone())));
    Some(v)
}

/// Edit the settings (one undo step).
fn set(app: &mut VectorcraftApp, p: Value) {
    if let Err(e) = app.run("threeD.set", p) {
        app.ui.status = e;
    }
}

pub fn show(app: &mut VectorcraftApp, ui: &mut Ui) {
    let Some(st) = settings(app, ui.ctx()) else {
        widgets::dim_label(ui, tl!("No document"));
        return;
    };
    ui.push_id("3d", |ui| {
        let params = &st["params"];
        if params.is_null() {
            if super::selection_len(app) == 0 {
                widgets::dim_label(ui, tl!("No Selection"));
                return;
            }
            widgets::dim_label(ui, tl!("Turn the selection into a 3D object:"));
            object_buttons(app, ui, None);
            return;
        }
        let tab = app.ui.three_d.tab;
        let names = [tl!("Object"), tl!("Materials"), tl!("Lighting")];
        let current = Tab::ALL.iter().position(|t| *t == tab).unwrap_or(0);
        if let Some(i) = widgets::tab_bar(ui, &names, current) {
            app.ui.three_d.tab = Tab::ALL.get(i).copied().unwrap_or_default();
        }
        ui.add_space(4.0);
        match app.ui.three_d.tab {
            Tab::Object => object_tab(app, ui, params),
            Tab::Materials => materials_tab(app, ui, params),
            Tab::Lighting => lighting_tab(app, ui, params),
        }
    });
}

pub fn menu(app: &mut VectorcraftApp, ui: &mut Ui) {
    let has = app.session.active().is_some_and(|d| !d.selection.is_empty());
    for (label, tab) in [(tl!("Object"), Tab::Object), (tl!("Materials"), Tab::Materials), (tl!("Lighting"), Tab::Lighting)] {
        if widgets::menu_item(ui, label, true, app.ui.three_d.tab == tab) {
            app.ui.three_d.tab = tab;
        }
    }
    ui.separator();
    if widgets::menu_item(ui, tl!("Expand Appearance"), has, false) {
        app.run("effect.expandAppearance", json!({})).ok();
    }
}

/// The Effect › 3D and Materials submenu: the object types and Materials… (the panel), then the
/// 3D (Classic) effects (`classic`).
pub fn effect_menu(classic: Vec<crate::menus::Item>) -> Vec<crate::menus::Item> {
    use crate::menus::Item;
    let mut items: Vec<Item> = [("Extrude & Bevel…", "extrude"), ("Revolve…", "revolve"), ("Inflate…", "inflate"), ("Rotate…", "plane")]
        .into_iter()
        .map(|(label, object)| Item::Cmd(label, "ui.threeD", json!({ "object": object })))
        .collect();
    items.push(Item::Cmd("Materials…", "ui.threeD", json!({"tab": "materials"})));
    if !classic.is_empty() {
        items.push(Item::Sep);
        items.push(Item::Sub("3D (Classic)", classic));
    }
    items
}

/// The object-type buttons; `current` is highlighted. Without a 3D effect they apply it.
fn object_buttons(app: &mut VectorcraftApp, ui: &mut Ui, current: Option<&str>) {
    ui.horizontal_wrapped(|ui| {
        for (id, label) in OBJECT_TYPES {
            let sel = current == Some(id);
            let r = ui.push_id(("3d-object", id), |ui| ui.add(egui::Button::selectable(sel, tl!(label)).min_size(vec2(56.0, 24.0)))).inner;
            if r.clicked() && !sel {
                if current.is_some() {
                    set(app, json!({"params": {"object": id}}));
                } else if let Err(e) = app.run("threeD.apply", json!({ "object": id })) {
                    app.ui.status = e;
                }
            }
        }
    });
}

fn num(p: &Value, key: &str, default: f64) -> f64 {
    p.get(key).and_then(Value::as_f64).filter(|v| v.is_finite()).unwrap_or(default)
}

fn flag(p: &Value, key: &str) -> bool {
    p.get(key).and_then(Value::as_bool).unwrap_or(false)
}

/// A labelled slider and number field for setting `key`; a drag previews and lands as one undo
/// step. `edit` turns the new value into `threeD.set` params.
#[allow(clippy::too_many_arguments)]
fn slider_with(
    app: &mut VectorcraftApp,
    ui: &mut Ui,
    key: &str,
    label: &str,
    value: f64,
    range: RangeInclusive<f64>,
    suffix: &str,
    decimals: usize,
    edit: &dyn Fn(f64) -> Value,
) {
    ui.push_id(("3d", key), |ui| {
        ui.horizontal(|ui| {
            let t = Tokens::get(ui.ctx());
            ui.add_sized([LABEL_W, 20.0], egui::Label::new(egui::RichText::new(label).size(12.0).color(t.text)).truncate());
            // The theme's widget fill matches the panel, which would hide the rail.
            ui.visuals_mut().widgets.inactive.bg_fill = t.input_border;
            ui.visuals_mut().selection.bg_fill = t.accent;
            let mut v = value.clamp(*range.start(), *range.end());
            let w = (ui.available_width() - FIELD_W - 8.0).max(40.0);
            let r = ui.add_sized([w, 20.0], egui::Slider::new(&mut v, range.clone()).show_value(false).trailing_fill(true));
            let phase = if r.dragged() && !r.drag_stopped() {
                Live::Dragging
            } else if r.drag_stopped() || (r.changed() && !r.dragged()) {
                Live::Released
            } else {
                Live::Idle
            };
            if phase != Live::Idle {
                live_run(app, LABEL, "threeD.set", edit(widgets::round_to(v, decimals as i32)), phase);
            }
            if let Some(x) = widgets::range_field(ui, ("field", key), value, range, suffix, decimals, FIELD_W) {
                set(app, edit(x));
            }
        });
    });
}

/// [`slider_with`] for top-level setting `key` of `params`.
#[allow(clippy::too_many_arguments)]
fn slider(app: &mut VectorcraftApp, ui: &mut Ui, params: &Value, key: &str, label: &str, range: RangeInclusive<f64>, suffix: &str, decimals: usize) {
    let value = num(params, key, *range.start());
    slider_with(app, ui, key, label, value, range, suffix, decimals, &|v| json!({"params": { key: v }}));
}

/// A checkbox for boolean setting `key`.
fn check(app: &mut VectorcraftApp, ui: &mut Ui, params: &Value, key: &str, label: &str) {
    let on = flag(params, key);
    if ui.push_id(("3d", key), |ui| widgets::check(ui, label, on, true)).inner {
        set(app, json!({"params": { key: !on }}));
    }
}

/// A labelled dropdown of `(id, label)` options for setting `key`; `extra` is shown for a value
/// that isn't one of them (Custom).
fn choice(app: &mut VectorcraftApp, ui: &mut Ui, key: &str, label: &str, current: &str, options: &[(&str, &str)], edit: &dyn Fn(&str) -> Value) {
    ui.push_id(("3d", key), |ui| {
        ui.horizontal(|ui| {
            let t = Tokens::get(ui.ctx());
            ui.add_sized([LABEL_W, 20.0], egui::Label::new(egui::RichText::new(label).size(12.0).color(t.text)).truncate());
            let shown = options.iter().find(|o| o.0 == current).map_or("Custom", |o| o.1);
            let labels: Vec<&str> = options.iter().map(|o| o.1).collect();
            let w = ui.available_width().max(80.0);
            if let Some(i) = widgets::dropdown(ui, "dropdown", shown, &labels, w)
                && let Some((id, _)) = options.get(i)
            {
                set(app, edit(id));
            }
        });
    });
}

fn object_tab(app: &mut VectorcraftApp, ui: &mut Ui, p: &Value) {
    let object = p.get("object").and_then(Value::as_str).unwrap_or("extrude").to_ascii_lowercase();
    object_buttons(app, ui, Some(object.as_str()));
    ui.add_space(4.0);
    match object.as_str() {
        "plane" => {}
        "revolve" => {
            slider(app, ui, p, "angle", tl!("Angle"), 1.0..=360.0, "°", 0);
            slider(app, ui, p, "offset", tl!("Offset"), 0.0..=500.0, " pt", 1);
            let from = p.get("from").and_then(Value::as_str).unwrap_or("leftEdge");
            choice(
                app,
                ui,
                "from",
                tl!("From"),
                from,
                &[("leftEdge", "Left Edge"), ("rightEdge", "Right Edge")],
                &|v| json!({"params": {"from": v}}),
            );
            check(app, ui, p, "cap", tl!("Caps"));
        }
        "inflate" => {
            slider(app, ui, p, "depth", tl!("Depth"), 0.0..=500.0, " pt", 1);
            slider(app, ui, p, "volume", tl!("Volume"), 0.0..=100.0, "%", 0);
            check(app, ui, p, "bothSides", tl!("Inflate Both Sides"));
        }
        _ => {
            slider(app, ui, p, "depth", tl!("Depth"), 0.0..=1000.0, " pt", 1);
            slider(app, ui, p, "twist", tl!("Twist"), -360.0..=360.0, "°", 0);
            slider(app, ui, p, "taper", tl!("Taper"), 1.0..=200.0, "%", 0);
            check(app, ui, p, "cap", tl!("Caps"));
            widgets::divider(ui);
            check(app, ui, p, "bevel", tl!("Bevel"));
            if flag(p, "bevel") {
                let shape = p.get("bevelShape").and_then(Value::as_str).unwrap_or("straight");
                choice(app, ui, "bevelShape", tl!("Profile"), shape, &BEVEL_PROFILES, &|v| json!({"params": {"bevelShape": v}}));
                slider(app, ui, p, "bevelWidth", tl!("Width"), 0.0..=100.0, " pt", 1);
                slider(app, ui, p, "bevelHeight", tl!("Height"), 0.0..=100.0, " pt", 1);
                slider(app, ui, p, "bevelRepeat", tl!("Repeat"), 1.0..=10.0, "", 0);
                check(app, ui, p, "bevelOutside", tl!("Bevel Outside"));
                check(app, ui, p, "bevelBothSides", tl!("Bevel Both Sides"));
            }
        }
    }
    widgets::divider(ui);
    widgets::subheader(ui, tl!("Rotation"));
    let rotation: Vec<(&str, &str)> = ROTATION_PRESETS.iter().map(|(id, label, _)| (*id, *label)).collect();
    let current = p.get("rotation").and_then(Value::as_str).unwrap_or("custom");
    choice(app, ui, "rotation", tl!("Preset"), current, &rotation, &|v| json!({ "rotation": v }));
    for (key, label) in [("rotateX", "X"), ("rotateY", "Y"), ("rotateZ", "Z")] {
        slider(app, ui, p, key, label, -180.0..=180.0, "°", 0);
    }
    slider(app, ui, p, "perspective", tl!("Perspective"), 0.0..=160.0, "°", 0);
}

/// A lit sphere of radius `r` at `c`, shaded as `params` would shade a surface of colour `base`.
fn sphere(ui: &Ui, c: Pos2, r: f32, params: &Value, base: [f64; 3]) {
    const RINGS: usize = 7;
    const SEGMENTS: usize = 28;
    let color = |x: f64, y: f64| {
        let z = (1.0 - x * x - y * y).max(0.0).sqrt();
        let [r, g, b] = preview_shade(params, base, [x, y, -z]);
        Color32::from_rgb((r * 255.0).round() as u8, (g * 255.0).round() as u8, (b * 255.0).round() as u8)
    };
    let mut mesh = Mesh::default();
    mesh.colored_vertex(c, color(0.0, 0.0));
    for i in 1..=RINGS {
        let f = i as f64 / RINGS as f64;
        for j in 0..SEGMENTS {
            let a = j as f64 / SEGMENTS as f64 * std::f64::consts::TAU;
            let (x, y) = (f * a.cos() * 0.999, f * a.sin() * 0.999);
            mesh.colored_vertex(c + vec2(x as f32 * r, y as f32 * r), color(x, y));
        }
    }
    let at = |ring: usize, j: usize| if ring == 0 { 0 } else { (1 + (ring - 1) * SEGMENTS + j % SEGMENTS) as u32 };
    for ring in 0..RINGS {
        for j in 0..SEGMENTS {
            if ring == 0 {
                mesh.add_triangle(0, at(1, j), at(1, j + 1));
            } else {
                mesh.add_triangle(at(ring, j), at(ring + 1, j), at(ring + 1, j + 1));
                mesh.add_triangle(at(ring, j), at(ring + 1, j + 1), at(ring, j + 1));
            }
        }
    }
    ui.painter().add(egui::Shape::mesh(mesh));
}

fn materials_tab(app: &mut VectorcraftApp, ui: &mut Ui, p: &Value) {
    let t = Tokens::get(ui.ctx());
    let current = p.get("material").and_then(Value::as_str).unwrap_or("custom");
    widgets::subheader(ui, tl!("Presets"));
    let size = vec2(58.0, 62.0);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
        for (id, label, ..) in MATERIAL_PRESETS {
            let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
            let resp = ui.interact(rect, egui::Id::new(("3d-material", id)), Sense::click());
            let mut look = p.clone();
            if let (Some(m), Some(Value::Object(preset))) = (look.as_object_mut(), vectorcraft_effects::material_preset(id)) {
                m.extend(preset);
            }
            let sel = current == id;
            if sel || resp.hovered() {
                ui.painter().rect_filled(rect, 4.0, if sel { t.row_selected } else { t.hover });
            }
            sphere(ui, rect.center_top() + vec2(0.0, 22.0), 17.0, &look, [0.72, 0.72, 0.74]);
            let text = egui::RichText::new(tl!(label)).size(10.5).color(if sel { t.text_strong } else { t.text_dim });
            let galley = ui.painter().layout(text.text().to_string(), egui::FontId::proportional(10.5), t.text, size.x);
            ui.painter().galley(
                pos2(rect.center().x - galley.size().x / 2.0, rect.top() + 44.0),
                galley,
                if sel { t.text_strong } else { t.text_dim },
            );
            if resp.on_hover_text(tl!(label)).clicked() {
                set(app, json!({ "material": id }));
            }
        }
    });
    widgets::divider(ui);
    widgets::subheader(ui, tl!("Base Properties"));
    slider(app, ui, p, "roughness", tl!("Roughness"), 0.0..=1.0, "", 2);
    slider(app, ui, p, "metallic", tl!("Metallic"), 0.0..=1.0, "", 2);
    let color = p.get("materialColor").and_then(Value::as_str);
    ui.push_id("3d-materialColor", |ui| {
        ui.horizontal(|ui| {
            if widgets::check(ui, tl!("Material Color"), color.is_some(), true) {
                let v = if color.is_some() { Value::Null } else { json!("#b0b0b0") };
                set(app, json!({"params": {"materialColor": v}}));
            }
            if let Some(c) = color
                && let Some(hex) = widgets::hex_field(ui, "hex", c.trim_start_matches('#'))
                && let Some(v) = normalize_hex(&hex)
            {
                set(app, json!({"params": {"materialColor": v}}));
            }
        });
    });
}

/// `"#rrggbb"` from what was typed in a hex field, `None` when it isn't a colour.
fn normalize_hex(s: &str) -> Option<String> {
    let s = format!("#{}", s.trim().trim_start_matches('#'));
    vectorcraft_color::Color::from_hex(&s).map(|c| c.to_hex())
}

fn lighting_tab(app: &mut VectorcraftApp, ui: &mut Ui, p: &Value) {
    let t = Tokens::get(ui.ctx());
    let current = p.get("lighting").and_then(Value::as_str).unwrap_or("custom");
    widgets::subheader(ui, tl!("Presets"));
    ui.horizontal_wrapped(|ui| {
        for (id, label) in LIGHTING_PRESETS {
            let sel = current == id;
            if ui.push_id(("3d-lighting", id), |ui| ui.add(egui::Button::selectable(sel, tl!(label)))).inner.clicked() && !sel {
                set(app, json!({ "lighting": id }));
            }
        }
    });
    let lights: Vec<Value> = p.get("lights").and_then(Value::as_array).cloned().unwrap_or_default();
    let chosen = app.ui.three_d.light.min(lights.len().saturating_sub(1));
    // The lights on a sphere: drag on it to move the chosen one.
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 120.0), Sense::hover());
    let resp = ui.interact(rect, egui::Id::new("3d-light-sphere"), Sense::click_and_drag());
    let (c, r) = (rect.center(), 52.0);
    sphere(ui, c, r, p, [0.75, 0.75, 0.75]);
    ui.painter().circle_stroke(c, r, Stroke::new(1.0, t.border));
    for (i, l) in lights.iter().enumerate() {
        let at = light_pos(c, r, num(l, "rotation", -45.0), num(l, "height", 45.0));
        let sel = i == chosen;
        ui.painter().circle(
            at,
            if sel { 6.0 } else { 4.5 },
            if sel { t.accent } else { t.panel },
            Stroke::new(1.5, if sel { Color32::WHITE } else { t.text }),
        );
    }
    if !lights.is_empty()
        && let Some((pt, phase)) = widgets::pointer_phase(&resp)
    {
        let (rotation, height) = light_angles(c, r, pt);
        live_run(app, LABEL, "threeD.set", json!({"light": {"index": chosen, "rotation": rotation, "height": height}}), phase);
    }
    resp.on_hover_text(tl!("Drag to move the chosen light"));
    // The lights, and adding and removing one.
    ui.horizontal_wrapped(|ui| {
        for i in 0..lights.len() {
            let name = crate::i18n::fmt(tl!("Light {n}"), &[("n", &(i + 1).to_string())]);
            if ui.push_id(("3d-light", i), |ui| ui.add(egui::Button::selectable(i == chosen, name))).inner.clicked() {
                app.ui.three_d.light = i;
            }
        }
        let add =
            ui.push_id("3d-light-add", |ui| widgets::icon_button_enabled(ui, "plus", tl!("Add Light"), false, lights.len() < MAX_LIGHTS, 18.0)).inner;
        if add.clicked() {
            set(app, json!({"light": {"index": lights.len()}}));
            app.ui.three_d.light = lights.len();
        }
        let del =
            ui.push_id("3d-light-delete", |ui| widgets::icon_button_enabled(ui, "trash-2", tl!("Delete Light"), false, lights.len() > 1, 18.0)).inner;
        if del.clicked() {
            set(app, json!({ "removeLight": chosen }));
            app.ui.three_d.light = chosen.saturating_sub(1);
        }
    });
    if let Some(l) = lights.get(chosen) {
        let light_slider =
            |app: &mut VectorcraftApp, ui: &mut Ui, key: &'static str, label: &str, range: RangeInclusive<f64>, suffix: &str, default: f64| {
                slider_with(
                    app,
                    ui,
                    &format!("light-{key}"),
                    label,
                    num(l, key, default),
                    range,
                    suffix,
                    0,
                    &|v| json!({"light": {"index": chosen, key: v}}),
                );
            };
        ui.push_id("3d-light-color", |ui| {
            ui.horizontal(|ui| {
                ui.add_sized([LABEL_W, 20.0], egui::Label::new(egui::RichText::new(tl!("Color")).size(12.0).color(t.text)).truncate());
                let c = l.get("color").and_then(Value::as_str).unwrap_or("#ffffff");
                if let Some(hex) = widgets::hex_field(ui, "hex", c.trim_start_matches('#'))
                    && let Some(v) = normalize_hex(&hex)
                {
                    set(app, json!({"light": {"index": chosen, "color": v}}));
                }
            });
        });
        light_slider(app, ui, "intensity", tl!("Intensity"), 0.0..=400.0, "%", 100.0);
        light_slider(app, ui, "rotation", tl!("Rotation"), -180.0..=180.0, "°", -45.0);
        light_slider(app, ui, "height", tl!("Height"), 0.0..=90.0, "°", 45.0);
        light_slider(app, ui, "softness", tl!("Softness"), 0.0..=100.0, "%", 50.0);
    }
    slider(app, ui, p, "ambient", tl!("Ambient Light"), 0.0..=200.0, "%", 0);
    widgets::divider(ui);
    check(app, ui, p, "shadow", tl!("Shadows"));
    if flag(p, "shadow") {
        let position = p.get("shadowPosition").and_then(Value::as_str).unwrap_or("below");
        choice(
            app,
            ui,
            "shadowPosition",
            tl!("Position"),
            position,
            &[("behind", "Behind Object"), ("below", "Below Object")],
            &|v| json!({"params": {"shadowPosition": v}}),
        );
        slider(app, ui, p, "shadowDistance", tl!("Distance"), 0.0..=500.0, " pt", 1);
        slider(app, ui, p, "shadowBounds", tl!("Shadow Bounds"), 10.0..=400.0, "%", 0);
        slider(app, ui, p, "shadowSoftness", tl!("Blur"), 0.0..=50.0, " pt", 1);
        slider(app, ui, p, "shadowOpacity", tl!("Opacity"), 0.0..=100.0, "%", 0);
    }
}

/// Where a light `rotation`° around the view and `height`° above the art shows on a sphere of
/// radius `r` at `c` (straight above the art: the middle).
fn light_pos(c: Pos2, r: f32, rotation: f64, height: f64) -> Pos2 {
    let (ro, e) = (rotation.to_radians(), height.clamp(0.0, 90.0).to_radians());
    c + vec2((e.cos() * ro.sin()) as f32 * r, (-e.cos() * ro.cos()) as f32 * r)
}

/// The rotation and height of a light shown at `p` on the sphere of radius `r` at `c`
/// ([`light_pos`]'s inverse; points past the rim lie on it).
fn light_angles(c: Pos2, r: f32, p: Pos2) -> (f64, f64) {
    let d = (p - c) / r.max(1.0);
    let (x, y) = (f64::from(d.x), f64::from(d.y));
    let len = x.hypot(y).min(1.0);
    let rotation = if len < 1e-6 { 0.0 } else { x.atan2(-y).to_degrees() };
    (rotation.round(), len.acos().to_degrees().round())
}

#[cfg(test)]
pub(super) fn light_geometry(c: Pos2, r: f32, rotation: f64, height: f64) -> (f64, f64) {
    light_angles(c, r, light_pos(c, r, rotation, height))
}
