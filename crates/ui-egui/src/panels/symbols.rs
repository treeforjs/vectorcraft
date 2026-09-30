//! Symbols panel: the document's symbols as thumbnails or a list, with an honest empty state.

use egui::{Sense, Stroke, StrokeKind, Ui, vec2};

use super::{pstate, set_pstate};
use crate::theme::Tokens;
use crate::widgets::{self, menu_item};
use crate::{DrawcraftApp, icons};

pub fn show(app: &mut DrawcraftApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let names: Vec<String> = app.session.active().map(|d| d.doc.symbols.iter().map(|s| s.name.clone()).collect()).unwrap_or_default();
    let list: bool = pstate(ui.ctx(), "sym-list");
    let sel: Option<String> = pstate(ui.ctx(), "sym-sel");
    widgets::list_box(ui, |ui| {
        ui.set_min_height(110.0);
        ui.set_width(ui.available_width());
        if names.is_empty() {
            super::empty_state(
                ui,
                "spray-can",
                "No symbols in this document",
                "Symbols come in through SVG/PDF import; creating them is on the roadmap.",
            );
            return;
        }
        if list {
            for n in &names {
                let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::click());
                if sel.as_deref() == Some(n.as_str()) {
                    ui.painter().rect_filled(r, 0.0, t.row_selected);
                } else if resp.hovered() {
                    ui.painter().rect_filled(r, 0.0, t.hover);
                }
                icons::paint(ui, "shapes", egui::Rect::from_center_size(r.left_center() + vec2(14.0, 0.0), vec2(16.0, 16.0)), t.icon);
                ui.painter().text(r.left_center() + vec2(30.0, 0.0), egui::Align2::LEFT_CENTER, n, egui::FontId::proportional(12.5), t.text);
                if resp.clicked() {
                    set_pstate(ui.ctx(), "sym-sel", Some(n.clone()));
                }
            }
        } else {
            ui.horizontal_wrapped(|ui| {
                for n in &names {
                    let (r, resp) = ui.allocate_exact_size(vec2(40.0, 40.0), Sense::click());
                    ui.painter().rect_filled(r, 0.0, egui::Color32::WHITE);
                    icons::paint(ui, "shapes", r.shrink(8.0), egui::Color32::DARK_GRAY);
                    let on = sel.as_deref() == Some(n.as_str());
                    ui.painter().rect_stroke(
                        r,
                        0.0,
                        Stroke::new(if on { 2.0 } else { 1.0 }, if on { t.accent } else { t.border }),
                        StrokeKind::Inside,
                    );
                    if resp.on_hover_text(n).clicked() {
                        set_pstate(ui.ctx(), "sym-sel", Some(n.clone()));
                    }
                }
            });
        }
    });
    widgets::bottom_bar(ui, |ui| {
        widgets::icon_button_enabled(ui, "library", "Symbol Libraries (on the roadmap)", false, false, 24.0);
        widgets::icon_button_enabled(ui, "dc-place-symbol", "Place Symbol Instance (on the roadmap)", false, false, 24.0);
        widgets::icon_button_enabled(ui, "link-2-off", "Break Link to Symbol (on the roadmap)", false, false, 24.0);
        ui.add_space((ui.available_width() - 2.0 * 28.0).max(0.0));
        widgets::icon_button_enabled(ui, "dc-new-item", "New Symbol (on the roadmap)", false, false, 24.0);
        widgets::icon_button_enabled(ui, "trash-2", "Delete Symbol (on the roadmap)", false, false, 24.0);
    });
}

pub fn menu(_app: &mut DrawcraftApp, ui: &mut Ui) {
    for l in [
        "New Symbol…",
        "Redefine Symbol",
        "Duplicate Symbol",
        "Delete Symbol",
        "Edit Symbol",
        "Place Symbol Instance",
        "Replace Symbol",
        "Break Link to Symbol",
    ] {
        menu_item(ui, l, false, false);
    }
    ui.separator();
    menu_item(ui, "Select All Unused", false, false);
    menu_item(ui, "Select All Instances", false, false);
    menu_item(ui, "Sort by Name", false, false);
    ui.separator();
    let list: bool = pstate(ui.ctx(), "sym-list");
    if menu_item(ui, "Thumbnail View", true, !list) {
        set_pstate(ui.ctx(), "sym-list", false);
    }
    if menu_item(ui, "List View", true, list) {
        set_pstate(ui.ctx(), "sym-list", true);
    }
    ui.separator();
    menu_item(ui, "Symbol Options…", false, false);
    menu_item(ui, "Open Symbol Library", false, false);
}
