//! Character panel: font family / style, size, leading, kerning, tracking, vertical and horizontal
//! scale, baseline shift, character rotation and the caps / underline / strikethrough toggles.

use drawcraft_doc::{CharStyle, NodeKind, Unit};
use egui::{Ui, vec2};
use serde_json::{Value, json};

use super::{first_selected, pstate, set_pstate};
use crate::DrawcraftApp;
use crate::theme::Tokens;
use crate::widgets::{self, menu_item};

pub const SIZE_PRESETS: [f64; 16] = [6.0, 7.0, 8.0, 9.0, 10.0, 11.0, 12.0, 14.0, 18.0, 21.0, 24.0, 36.0, 48.0, 60.0, 72.0, 96.0];
pub const TRACKING_PRESETS: [f64; 13] = [-100.0, -75.0, -50.0, -25.0, -10.0, -5.0, 0.0, 5.0, 10.0, 25.0, 50.0, 75.0, 100.0];
pub const SCALE_PRESETS: [f64; 8] = [25.0, 50.0, 75.0, 90.0, 100.0, 110.0, 125.0, 150.0];

pub(crate) fn text_style(app: &DrawcraftApp) -> Option<(CharStyle, drawcraft_doc::ParaStyle)> {
    let n = first_selected(app)?;
    match &n.kind {
        NodeKind::Text(t) => Some((t.first_style(), t.para.clone())),
        _ => None,
    }
}

fn style(app: &mut DrawcraftApp, p: Value) {
    app.run("text.setStyle", p).ok();
}
fn format(app: &mut DrawcraftApp, p: Value) {
    app.run("text.setFormat", p).ok();
}

/// A compact labelled cell: short glyph label + spinner field.
fn cell(ui: &mut Ui, label: &str, tip: &str, add: impl FnOnce(&mut Ui)) {
    let t = Tokens::get(ui.ctx());
    ui.horizontal(|ui| {
        ui.add_sized(vec2(22.0, 24.0), egui::Label::new(egui::RichText::new(label).size(11.5).strong().color(t.text))).on_hover_text(tip);
        add(ui);
    });
}

pub fn show(app: &mut DrawcraftApp, ui: &mut Ui) {
    let Some((s, _)) = text_style(app) else {
        super::empty_state(ui, "type", "No text selected", "Select a text object to edit its character attributes.");
        return;
    };
    let fams = drawcraft_text::FontDb::global().families();
    let names: Vec<&str> = fams.iter().map(String::as_str).collect();
    let w = ui.available_width();
    if let Some(i) = widgets::dropdown(ui, "ch-font", &s.font_family, &names, w - 4.0) {
        style(app, json!({"font": names[i]}));
    }
    let styles = drawcraft_text::FontDb::global().styles(&s.font_family);
    let snames: Vec<&str> = styles.iter().map(String::as_str).collect();
    if let Some(i) = widgets::dropdown(ui, "ch-style", &s.font_style, &snames, w - 4.0) {
        style(app, json!({"style": snames[i]}));
    }
    ui.add_space(2.0);
    let fw = ((w - 66.0) / 2.0).clamp(60.0, 100.0);
    egui::Grid::new("ch-grid").num_columns(2).spacing([6.0, 4.0]).show(ui, |ui| {
        cell(ui, "T", "Font Size", |ui| {
            if let Some(v) = widgets::spin_field(ui, "ch-size", Some(s.size), Unit::Points, fw, 1.0, 0.1, &SIZE_PRESETS) {
                style(app, json!({"size": v}));
            }
        });
        cell(ui, "A↕", "Leading", |ui| {
            let presets: Vec<f64> = SIZE_PRESETS.iter().map(|v| v * 1.2).collect();
            if let Some(v) = widgets::spin_field(ui, "ch-lead", Some(s.effective_leading()), Unit::Points, fw, 1.0, 0.1, &presets) {
                style(app, json!({"leading": v}));
            }
        });
        ui.end_row();
        cell(ui, "VA", "Kerning (0 = Auto)", |ui| {
            if let Some(v) = widgets::spin_plain(ui, "ch-kern", s.kerning.unwrap_or(0.0), "", 0, fw, 10.0, -1000.0, &TRACKING_PRESETS) {
                format(app, if v == 0.0 { json!({"kerning": "auto"}) } else { json!({"kerning": v}) });
            }
        });
        cell(ui, "VA↔", "Tracking", |ui| {
            if let Some(v) = widgets::spin_plain(ui, "ch-track", s.tracking, "", 0, fw, 10.0, -1000.0, &TRACKING_PRESETS) {
                style(app, json!({"tracking": v}));
            }
        });
        ui.end_row();
        if !pstate::<bool>(ui.ctx(), "ch-hide-options") {
            cell(ui, "IT", "Vertical Scale %", |ui| {
                if let Some(v) = widgets::spin_plain(ui, "ch-vs", s.v_scale, "%", 1, fw, 1.0, 1.0, &SCALE_PRESETS) {
                    format(app, json!({"vScale": v}));
                }
            });
            cell(ui, "T↔", "Horizontal Scale %", |ui| {
                if let Some(v) = widgets::spin_plain(ui, "ch-hs", s.h_scale, "%", 1, fw, 1.0, 1.0, &SCALE_PRESETS) {
                    format(app, json!({"hScale": v}));
                }
            });
            ui.end_row();
            cell(ui, "Aª", "Baseline Shift", |ui| {
                if let Some(v) = widgets::spin_field(ui, "ch-bs", Some(s.baseline_shift), Unit::Points, fw, 1.0, -1296.0, &[]) {
                    format(app, json!({"baselineShift": v}));
                }
            });
            cell(ui, "⟲T", "Character Rotation", |ui| {
                if let Some(v) = widgets::spin_plain(ui, "ch-rot", s.rotation, "°", 1, fw, 15.0, -360.0, &super::transform::ANGLE_PRESETS) {
                    format(app, json!({"rotation": v}));
                }
            });
            ui.end_row();
        }
    });
    if pstate::<bool>(ui.ctx(), "ch-hide-options") {
        return;
    }
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        for (kind, tip, key, on, enabled) in [
            (Glyph::AllCaps, "All Caps", "allCaps", s.all_caps, true),
            (Glyph::SmallCaps, "Small Caps (on the roadmap)", "", false, false),
            (Glyph::Super, "Superscript (on the roadmap)", "", false, false),
            (Glyph::Sub, "Subscript (on the roadmap)", "", false, false),
            (Glyph::Underline, "Underline", "underline", s.underline, true),
            (Glyph::Strike, "Strikethrough", "strikethrough", s.strikethrough, true),
        ] {
            if style_toggle(ui, kind, tip, on, enabled) && !key.is_empty() {
                format(app, json!({key: !on}));
            }
        }
    });
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Glyph {
    AllCaps,
    SmallCaps,
    Super,
    Sub,
    Underline,
    Strike,
}

/// A drawn "T" style toggle (All Caps, Small Caps, Superscript, Subscript, Underline, Strikethrough).
fn style_toggle(ui: &mut Ui, g: Glyph, tip: &str, on: bool, enabled: bool) -> bool {
    let t = Tokens::get(ui.ctx());
    let (r, resp) = ui.allocate_exact_size(vec2(30.0, 26.0), if enabled { egui::Sense::click() } else { egui::Sense::hover() });
    if on {
        ui.painter().rect_filled(r, 3, t.tool_active);
    } else if resp.hovered() && enabled {
        ui.painter().rect_filled(r, 3, t.hover);
    }
    let col = if enabled { t.text_strong } else { t.text_disabled };
    let big = egui::FontId::proportional(15.0);
    let small = egui::FontId::proportional(10.5);
    let c = r.center();
    let p = ui.painter();
    match g {
        Glyph::AllCaps => {
            p.text(c - vec2(5.0, 0.0), egui::Align2::CENTER_CENTER, "T", big.clone(), col);
            p.text(c + vec2(5.0, 0.0), egui::Align2::CENTER_CENTER, "T", big, col);
        }
        Glyph::SmallCaps => {
            p.text(c - vec2(4.0, 0.0), egui::Align2::CENTER_CENTER, "T", big, col);
            p.text(c + vec2(6.0, 2.0), egui::Align2::CENTER_CENTER, "T", small, col);
        }
        Glyph::Super => {
            p.text(c - vec2(3.0, 0.0), egui::Align2::CENTER_CENTER, "T", big, col);
            p.text(c + vec2(6.0, -5.0), egui::Align2::CENTER_CENTER, "1", small, col);
        }
        Glyph::Sub => {
            p.text(c - vec2(3.0, 0.0), egui::Align2::CENTER_CENTER, "T", big, col);
            p.text(c + vec2(6.0, 5.0), egui::Align2::CENTER_CENTER, "1", small, col);
        }
        Glyph::Underline => {
            p.text(c - vec2(0.0, 1.0), egui::Align2::CENTER_CENTER, "T", big, col);
            p.line_segment([c + vec2(-5.0, 7.0), c + vec2(5.0, 7.0)], egui::Stroke::new(1.2, col));
        }
        Glyph::Strike => {
            p.text(c, egui::Align2::CENTER_CENTER, "T", big, col);
            p.line_segment([c + vec2(-6.0, 1.0), c + vec2(6.0, 1.0)], egui::Stroke::new(1.2, col));
        }
    }
    let resp = resp.on_hover_text(tip);
    enabled && resp.clicked()
}

pub fn menu(app: &mut DrawcraftApp, ui: &mut Ui) {
    let st = text_style(app);
    let has = st.is_some();
    let hidden: bool = pstate(ui.ctx(), "ch-hide-options");
    if menu_item(ui, if hidden { "Show Options" } else { "Hide Options" }, true, false) {
        set_pstate(ui.ctx(), "ch-hide-options", !hidden);
    }
    ui.separator();
    let s = st.map(|x| x.0);
    for (label, key, on) in [
        ("All Caps", "allCaps", s.as_ref().is_some_and(|s| s.all_caps)),
        ("Underline", "underline", s.as_ref().is_some_and(|s| s.underline)),
        ("Strikethrough", "strikethrough", s.as_ref().is_some_and(|s| s.strikethrough)),
    ] {
        if menu_item(ui, label, has, on) {
            format(app, json!({key: !on}));
        }
    }
    for l in ["Small Caps", "Superscript", "Subscript"] {
        menu_item(ui, l, false, false);
    }
    ui.separator();
    for l in ["Standard Vertical Roman Alignment", "Tate-chu-yoko", "Fractional Widths", "System Layout", "No Break"] {
        menu_item(ui, l, false, l == "Fractional Widths");
    }
    ui.separator();
    if menu_item(ui, "Reset Panel", has, false) {
        style(app, json!({"tracking": 0, "leading": "auto"}));
        format(
            app,
            json!({"kerning": "auto", "baselineShift": 0, "hScale": 100, "vScale": 100, "rotation": 0, "underline": false, "strikethrough": false, "allCaps": false}),
        );
    }
}
