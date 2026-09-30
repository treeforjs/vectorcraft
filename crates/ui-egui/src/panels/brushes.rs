//! Brushes panel: the brushes referenced by this document's strokes (list or thumbnails), with an
//! honest empty state until brush definitions land.

use std::collections::BTreeSet;

use drawcraft_doc::{AppearanceItem, Node};
use egui::{Sense, Ui, vec2};
use serde_json::json;

use super::{first_selected, pstate, set_pstate};
use crate::DrawcraftApp;
use crate::theme::Tokens;
use crate::widgets::{self, menu_item};

fn collect(n: &Node, out: &mut BTreeSet<String>) {
    n.walk(&mut |c| {
        for it in &c.appearance.items {
            if let AppearanceItem::Stroke(s) = it
                && let Some(b) = &s.brush
            {
                out.insert(b.clone());
            }
        }
    });
}

/// Brush names used anywhere in the document.
pub fn used_brushes(app: &DrawcraftApp) -> Vec<String> {
    let mut set = BTreeSet::new();
    if let Some(st) = app.session.active() {
        for l in &st.doc.layers {
            collect(l, &mut set);
        }
    }
    set.into_iter().collect()
}

pub fn show(app: &mut DrawcraftApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let brushes = used_brushes(app);
    let sel_brush = first_selected(app).and_then(|n| n.appearance.stroke().and_then(|s| s.brush.clone()));
    widgets::list_box(ui, |ui| {
        ui.set_min_height(110.0);
        ui.set_width(ui.available_width());
        if brushes.is_empty() {
            super::empty_state(
                ui,
                "paintbrush",
                "No brushes in this document",
                "Brush definitions (calligraphic, art, scatter, pattern) are on the roadmap.",
            );
            return;
        }
        for b in &brushes {
            let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::click());
            if sel_brush.as_deref() == Some(b.as_str()) {
                ui.painter().rect_filled(r, 0.0, t.row_selected);
            } else if resp.hovered() {
                ui.painter().rect_filled(r, 0.0, t.hover);
            }
            let stroke_r = egui::Rect::from_min_size(r.left_center() + vec2(6.0, -5.0), vec2(60.0, 10.0));
            super::stroke::paint_profile(ui, stroke_r, Some(&drawcraft_doc::WidthProfile::lens()), t.text_strong);
            ui.painter().text(r.left_center() + vec2(76.0, 0.0), egui::Align2::LEFT_CENTER, b, egui::FontId::proportional(12.5), t.text);
            if resp.clicked() {
                app.run("stroke.setAdvanced", json!({"brush": b})).ok();
            }
        }
    });
    let has_brush = sel_brush.is_some();
    widgets::bottom_bar(ui, |ui| {
        widgets::icon_button_enabled(ui, "library", "Brush Libraries (on the roadmap)", false, false, 24.0);
        if widgets::icon_button_enabled(ui, "dc-remove-brush", "Remove Brush Stroke", false, has_brush, 24.0).clicked() {
            app.run("stroke.setAdvanced", json!({"brush": null})).ok();
        }
        widgets::icon_button_enabled(ui, "dc-options", "Options of Selected Object (on the roadmap)", false, false, 24.0);
        ui.add_space((ui.available_width() - 2.0 * 28.0).max(0.0));
        widgets::icon_button_enabled(ui, "dc-new-item", "New Brush (on the roadmap)", false, false, 24.0);
        widgets::icon_button_enabled(ui, "trash-2", "Delete Brush (on the roadmap)", false, false, 24.0);
    });
}

pub fn menu(app: &mut DrawcraftApp, ui: &mut Ui) {
    let has_brush = first_selected(app).and_then(|n| n.appearance.stroke().and_then(|s| s.brush.clone())).is_some();
    for l in ["New Brush…", "Duplicate Brush", "Delete Brush"] {
        menu_item(ui, l, false, false);
    }
    if menu_item(ui, "Remove Brush Stroke", has_brush, false) {
        app.run("stroke.setAdvanced", json!({"brush": null})).ok();
    }
    menu_item(ui, "Select All Unused", false, false);
    ui.separator();
    for l in ["Show Calligraphic Brushes", "Show Scatter Brushes", "Show Art Brushes", "Show Bristle Brushes", "Show Pattern Brushes"] {
        menu_item(ui, l, false, true);
    }
    ui.separator();
    let list: bool = pstate(ui.ctx(), "br-list");
    if menu_item(ui, "Thumbnail View", true, !list) {
        set_pstate(ui.ctx(), "br-list", false);
    }
    if menu_item(ui, "List View", true, list) {
        set_pstate(ui.ctx(), "br-list", true);
    }
    ui.separator();
    menu_item(ui, "Brush Options…", false, false);
    menu_item(ui, "Open Brush Library", false, false);
}
