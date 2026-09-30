//! Transparency panel: blend mode, opacity (field + slider popup), object/mask thumbnails with the
//! opacity-mask controls (on the roadmap), Isolate Blending and Knockout Group.

use drawcraft_color::{BlendMode, Paint};
use egui::{Sense, Stroke, StrokeKind, Ui, vec2};
use serde_json::json;

use super::{current_paints, first_selected, live_run, pstate, selection_len, set_pstate};
use crate::theme::Tokens;
use crate::widgets::{self, Live, menu_item};
use crate::{DrawcraftApp, icons};

pub fn show(app: &mut DrawcraftApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let n = first_selected(app);
    let has = n.is_some();
    let (blend, op, isolate, knockout) =
        n.as_ref().map(|n| (n.blend, n.opacity, n.isolate, n.knockout)).unwrap_or((BlendMode::Normal, 1.0, false, false));
    ui.horizontal(|ui| {
        let labels: Vec<&str> = BlendMode::ALL.iter().map(|b| b.label()).collect();
        ui.add_enabled_ui(has, |ui| {
            if let Some(i) = widgets::dropdown(ui, "tr-blend", blend.label(), &labels, 104.0) {
                app.run("transparency.set", json!({"blend": labels[i]})).ok();
            }
        });
        widgets::dim_label(ui, "Opacity:");
        ui.spacing_mut().item_spacing.x = 0.0;
        ui.add_enabled_ui(has, |ui| {
            if let Some(o) = widgets::plain_field(ui, "tr-op", op as f64 * 100.0, "%", 0, 50.0) {
                app.run("transparency.set", json!({"opacity": o.clamp(0.0, 100.0)})).ok();
            }
        });
        let (r, resp) = ui.allocate_exact_size(vec2(18.0, 26.0), if has { Sense::click() } else { Sense::hover() });
        ui.painter().rect_stroke(r, 2, Stroke::new(1.0, t.input_border), StrokeKind::Inside);
        icons::paint(ui, "chevron-right", r.shrink2(vec2(3.0, 7.0)), if has { t.icon } else { t.text_disabled });
        egui::Popup::menu(&resp).show(|ui| {
            let mut o = op * 100.0;
            let r = ui.add(egui::Slider::new(&mut o, 0.0..=100.0).show_value(false));
            let phase = if r.drag_stopped() || (r.changed() && !r.dragged()) {
                Live::Released
            } else if r.changed() {
                Live::Dragging
            } else {
                Live::Idle
            };
            live_run(app, "Opacity", "transparency.set", json!({"opacity": o.round()}), phase);
        });
    });
    widgets::divider(ui);
    // Thumbnails and the opacity-mask controls.
    let hide_thumbs: bool = pstate(ui.ctx(), "tr-hide-thumbs");
    ui.horizontal(|ui| {
        if !hide_thumbs {
            let (r, _) = ui.allocate_exact_size(vec2(60.0, 50.0), Sense::hover());
            ui.painter().rect_filled(r, 0.0, egui::Color32::WHITE);
            ui.painter().rect_stroke(r, 0.0, Stroke::new(1.5, t.border), StrokeKind::Outside);
            if has {
                let (f, s) = current_paints(app);
                let inner = r.shrink2(vec2(6.0, 10.0));
                widgets::paint_chip(ui, inner, &f);
                if let Some(c) = s.color()
                    && !matches!(s, Paint::None)
                {
                    ui.painter().rect_stroke(inner, 0.0, Stroke::new(1.0, super::c32(&c)), StrokeKind::Middle);
                }
            }
            let (m, _) = ui.allocate_exact_size(vec2(50.0, 50.0), Sense::hover());
            ui.painter().rect_stroke(m, 0.0, Stroke::new(1.0, t.input_border), StrokeKind::Inside);
            icons::paint(ui, "dc-mask-none", m.shrink(12.0), t.text_disabled);
        }
        ui.vertical(|ui| {
            ui.add_enabled_ui(false, |ui| widgets::flat_button(ui, "Make Mask", 96.0))
                .inner
                .on_disabled_hover_text("Opacity masks are on the roadmap");
            widgets::check(ui, "Clip", true, false);
            widgets::check(ui, "Invert Mask", false, false);
        });
    });
    if !pstate::<bool>(ui.ctx(), "tr-hide-options") {
        widgets::divider(ui);
        if widgets::check(ui, "Isolate Blending", isolate, has) {
            app.run("transparency.set", json!({"isolate": !isolate})).ok();
        }
        if widgets::check(ui, "Knockout Group", knockout, has) {
            app.run("transparency.set", json!({"knockout": !knockout})).ok();
        }
        widgets::check(ui, "Opacity & Mask Define Knockout Shape", false, false);
    }
    if !has {
        widgets::dim_label(ui, if selection_len(app) == 0 { "No Selection" } else { "" });
    }
}

pub fn menu(_app: &mut DrawcraftApp, ui: &mut Ui) {
    let hide_thumbs: bool = pstate(ui.ctx(), "tr-hide-thumbs");
    let hide_opts: bool = pstate(ui.ctx(), "tr-hide-options");
    if menu_item(ui, if hide_thumbs { "Show Thumbnails" } else { "Hide Thumbnails" }, true, false) {
        set_pstate(ui.ctx(), "tr-hide-thumbs", !hide_thumbs);
    }
    if menu_item(ui, if hide_opts { "Show Options" } else { "Hide Options" }, true, false) {
        set_pstate(ui.ctx(), "tr-hide-options", !hide_opts);
    }
    ui.separator();
    for l in ["Make Opacity Mask", "Release Opacity Mask", "Disable Opacity Mask", "Unlink Opacity Mask"] {
        menu_item(ui, l, false, false);
    }
    ui.separator();
    menu_item(ui, "New Opacity Masks Are Clipping", false, true);
    menu_item(ui, "New Opacity Masks Are Inverted", false, false);
    ui.separator();
    menu_item(ui, "Page Isolated Blending", false, false);
    menu_item(ui, "Page Knockout Group", false, false);
}
