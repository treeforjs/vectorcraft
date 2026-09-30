//! Graphic Styles panel: thumbnail grid or list of the document's styles.

use drawcraft_doc::Appearance;
use egui::{Rect, Sense, Stroke, StrokeKind, Ui, vec2};
use serde_json::json;

use super::{pstate, selection_len, set_pstate};
use crate::DrawcraftApp;
use crate::theme::Tokens;
use crate::widgets::{self, menu_item};

fn preview(ui: &Ui, r: Rect, ap: &Appearance) {
    ui.painter().rect_filled(r, 0.0, egui::Color32::WHITE);
    let inner = r.shrink(r.width() * 0.18);
    widgets::paint_chip(ui, inner, &ap.fill_paint());
    if let Some(s) = ap.stroke()
        && let Some(c) = s.paint.color()
    {
        ui.painter().rect_stroke(inner, 0.0, Stroke::new((s.width as f32).clamp(1.0, 4.0), super::c32(&c)), StrokeKind::Middle);
    }
    if !ap.effects.is_empty() {
        ui.painter().text(
            r.right_bottom() - vec2(3.0, 2.0),
            egui::Align2::RIGHT_BOTTOM,
            "fx",
            egui::FontId::proportional(9.0),
            egui::Color32::DARK_GRAY,
        );
    }
}

pub fn show(app: &mut DrawcraftApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let styles: Vec<(String, Appearance)> =
        app.session.active().map(|d| d.doc.graphic_styles.iter().map(|g| (g.name.clone(), g.appearance.clone())).collect()).unwrap_or_default();
    let sel: Option<String> = pstate(ui.ctx(), "gs-sel");
    let list: bool = pstate(ui.ctx(), "gs-list");
    let mut clicked = None;
    widgets::list_box(ui, |ui| {
        ui.set_min_height(120.0);
        ui.set_width(ui.available_width());
        if styles.is_empty() {
            super::empty_state(ui, "dc-graphic-styles", "No graphic styles", "Select styled art and click New Graphic Style.");
            return;
        }
        if list {
            for (name, ap) in &styles {
                let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::click());
                if sel.as_deref() == Some(name) {
                    ui.painter().rect_filled(r, 0.0, t.row_selected);
                } else if resp.hovered() {
                    ui.painter().rect_filled(r, 0.0, t.hover);
                }
                preview(ui, Rect::from_min_size(r.left_center() + vec2(4.0, -9.0), vec2(18.0, 18.0)), ap);
                ui.painter().text(r.left_center() + vec2(30.0, 0.0), egui::Align2::LEFT_CENTER, name, egui::FontId::proportional(12.0), t.text);
                if resp.clicked() {
                    clicked = Some(name.clone());
                }
            }
        } else {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(3.0, 3.0);
                for (name, ap) in &styles {
                    let (r, resp) = ui.allocate_exact_size(vec2(40.0, 40.0), Sense::click());
                    preview(ui, r, ap);
                    let on = sel.as_deref() == Some(name.as_str());
                    ui.painter().rect_stroke(
                        r,
                        0.0,
                        Stroke::new(
                            if on { 2.0 } else { 1.0 },
                            if on {
                                t.accent
                            } else if resp.hovered() {
                                t.text
                            } else {
                                t.border
                            },
                        ),
                        StrokeKind::Inside,
                    );
                    if resp.on_hover_text(name).clicked() {
                        clicked = Some(name.clone());
                    }
                }
            });
        }
    });
    if let Some(name) = clicked {
        set_pstate(ui.ctx(), "gs-sel", Some(name.clone()));
        if selection_len(app) > 0 {
            app.run("graphicStyle.apply", json!({"name": name})).ok();
        }
    }
    let sel: Option<String> = pstate(ui.ctx(), "gs-sel");
    let has_sel = selection_len(app) > 0;
    widgets::bottom_bar(ui, |ui| {
        widgets::icon_button_enabled(ui, "library", "Graphic Style Libraries (on the roadmap)", false, false, 24.0);
        widgets::icon_button_enabled(ui, "link-2-off", "Break Link to Graphic Style (on the roadmap)", false, false, 24.0);
        ui.add_space((ui.available_width() - 2.0 * 28.0).max(0.0));
        if widgets::icon_button_enabled(ui, "dc-new-item", "New Graphic Style", false, has_sel, 24.0).clicked() {
            app.run("graphicStyle.new", json!({})).ok();
        }
        if widgets::icon_button_enabled(ui, "trash-2", "Delete Graphic Style", false, sel.is_some(), 24.0).clicked()
            && let Some(n) = &sel
            && app.run("graphicStyle.delete", json!({"name": n})).is_ok()
        {
            set_pstate::<Option<String>>(ui.ctx(), "gs-sel", None);
        }
    });
}

pub fn menu(app: &mut DrawcraftApp, ui: &mut Ui) {
    let sel: Option<String> = pstate(ui.ctx(), "gs-sel");
    let list: bool = pstate(ui.ctx(), "gs-list");
    if menu_item(ui, "New Graphic Style…", selection_len(app) > 0, false) {
        app.run("graphicStyle.new", json!({})).ok();
    }
    if menu_item(ui, "Duplicate Graphic Style", sel.is_some(), false)
        && let Some(n) = &sel
    {
        app.run("graphicStyle.duplicate", json!({"name": n})).ok();
    }
    menu_item(ui, "Merge Graphic Styles", false, false);
    if menu_item(ui, "Delete Graphic Style", sel.is_some(), false)
        && let Some(n) = &sel
    {
        app.run("graphicStyle.delete", json!({"name": n})).ok();
    }
    menu_item(ui, "Break Link to Graphic Style", false, false);
    ui.separator();
    menu_item(ui, "Select All Unused", false, false);
    menu_item(ui, "Sort by Name", false, false);
    ui.separator();
    if menu_item(ui, "Thumbnail View", true, !list) {
        set_pstate(ui.ctx(), "gs-list", false);
    }
    if menu_item(ui, "Large List View", true, list) {
        set_pstate(ui.ctx(), "gs-list", true);
    }
    ui.separator();
    menu_item(ui, "Graphic Style Options…", false, false);
    menu_item(ui, "Open Graphic Style Library", false, false);
}
