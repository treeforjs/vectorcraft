//! The Tools panel: tool groups with flyouts, fill/stroke proxy, colour/gradient/none, drawing
//! modes, screen mode and the Edit Toolbar drawer.

use drawcraft_tools::{TOOL_GROUPS, ToolInfo};
use egui::{CornerRadius, Sense, Stroke, Ui, pos2, vec2};
use serde_json::json;

use crate::theme::Tokens;
use crate::{DrawcraftApp, icons, menus, widgets};

const CELL: f32 = 32.0;

fn tool_tip(t: &ToolInfo) -> String {
    match t.shortcut {
        Some(s) => format!("{} ({})", t.label, s),
        None => t.label.to_string(),
    }
}

pub fn show(app: &mut DrawcraftApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let avail = ui.available_height();
    let need = TOOL_GROUPS.len() as f32 * (CELL - 1.0) + 190.0;
    let cols = if app.ui.toolbar_double || avail < need { 2 } else { 1 };
    let w = CELL * cols as f32 + 10.0;
    egui::Panel::left("toolbar").resizable(false).exact_size(w).frame(egui::Frame::NONE.fill(t.panel).inner_margin(egui::Margin::symmetric(5, 4)).stroke(Stroke::new(1.0, t.border))).show(ui, |ui| {
        // Header: gripper with «/» toggle.
        let (hdr, hresp) = ui.allocate_exact_size(vec2(ui.available_width(), 12.0), Sense::click());
        let chev = if app.ui.toolbar_double { "chevrons-left" } else { "chevrons-right" };
        icons::paint(ui, chev, egui::Rect::from_center_size(hdr.center(), vec2(11.0, 11.0)), if hresp.hovered() { t.text } else { t.text_dim });
        if hresp.on_hover_text("Toggle single/double column").clicked() {
            app.ui.toolbar_double = !app.ui.toolbar_double;
        }
        ui.add_space(2.0);
        let active = app.session.tool_id();
        let mut open_flyout: Option<(usize, egui::Rect)> = None;
        let groups: Vec<(usize, &'static [ToolInfo])> = TOOL_GROUPS.iter().copied().enumerate().collect();
        for row in groups.chunks(cols) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                for (gi, group) in row {
                    let shown_id = app.ui.group_tool.get(*gi).cloned().unwrap_or_else(|| group[0].id.to_string());
                    let shown = group.iter().find(|x| x.id == shown_id).unwrap_or(&group[0]);
                    let is_active = group.iter().any(|x| x.id == active);
                    let (rect, resp) = ui.allocate_exact_size(vec2(CELL, CELL - 2.0), Sense::click_and_drag());
                    let bg = if is_active {
                        t.tool_active
                    } else if resp.hovered() {
                        t.hover
                    } else {
                        egui::Color32::TRANSPARENT
                    };
                    ui.painter().rect_filled(rect.shrink(1.0), CornerRadius::same(4), bg);
                    let icon = icons::tool_icon(shown.icon);
                    icons::paint(ui, icon, rect.shrink(7.0), if is_active { t.text } else { t.icon });
                    if group.len() > 1 {
                        let c = rect.right_bottom() + vec2(-4.0, -4.0);
                        ui.painter().add(egui::Shape::convex_polygon(vec![c, c + vec2(-4.0, 0.0), c + vec2(0.0, -4.0)], t.text_dim, Stroke::NONE));
                    }
                    let long_press = resp.is_pointer_button_down_on() && ui.input(|i| i.pointer.press_start_time().is_some_and(|s| i.time - s > 0.35));
                    if resp.secondary_clicked() || long_press || (resp.clicked() && ui.input(|i| i.modifiers.alt) && group.len() > 1) {
                        if ui.input(|i| i.modifiers.alt) && resp.clicked() {
                            // Option-click cycles through the group.
                            let idx = group.iter().position(|x| x.id == shown.id).unwrap_or(0);
                            let next = group[(idx + 1) % group.len()].id;
                            app.select_tool(next);
                        } else if group.len() > 1 {
                            open_flyout = Some((*gi, rect));
                        }
                    } else if resp.clicked() {
                        app.select_tool(shown.id);
                    }
                    resp.on_hover_text(tool_tip(shown));
                }
            });
            ui.add_space(1.0);
        }
        if let Some(f) = open_flyout {
            app.ui.flyout = Some(f.0);
            ui.data_mut(|d| d.insert_temp(egui::Id::new("flyout-anchor"), f.1));
        }
        ui.add_space(6.0);
        // Fill / stroke proxy.
        let (fill, stroke) = (app.session.paint.fill.clone(), app.session.paint.stroke.clone());
        let (fill, stroke) = match app.session.active().and_then(|d| d.selection.objects.first().and_then(|id| d.doc.node(*id))) {
            Some(n) if !n.is_container() => (n.appearance.fill_paint(), n.appearance.stroke_paint()),
            _ => (fill, stroke),
        };
        ui.vertical_centered(|ui| {
            let (f, s, swap, def) = widgets::fill_stroke_proxy(ui, &fill, &stroke, app.session.fill_active, 40.0);
            if f {
                app.session.fill_active = true;
            }
            if s {
                app.session.fill_active = false;
            }
            if swap {
                app.run("paint.swap", json!({})).ok();
            }
            if def {
                app.run("paint.default", json!({})).ok();
            }
        });
        ui.add_space(4.0);
        // Color / Gradient / None.
        let target = if app.session.fill_active { "paint.setFill" } else { "paint.setStroke" };
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 1.0;
            let s = 10.0;
            let (r, resp) = ui.allocate_exact_size(vec2(s, s), Sense::click());
            let last = app.session.paint.fill.color().unwrap_or(drawcraft_color::Color::BLACK);
            widgets::paint_chip(ui, r, &drawcraft_color::Paint::solid(last));
            if resp.on_hover_text("Color (,)").clicked() {
                app.run(target, json!({"color": last.to_hex()})).ok();
            }
            let (r, resp) = ui.allocate_exact_size(vec2(s, s), Sense::click());
            widgets::paint_chip(ui, r, &drawcraft_color::Paint::Gradient(Box::new(drawcraft_color::GradientPaint::new(Default::default()))));
            if resp.on_hover_text("Gradient (.)").clicked() {
                app.run(target, json!({"gradient": {"kind": "linear"}})).ok();
            }
            let (r, resp) = ui.allocate_exact_size(vec2(s, s), Sense::click());
            widgets::paint_chip(ui, r, &drawcraft_color::Paint::None);
            if resp.on_hover_text("None (/)").clicked() {
                app.run(target, json!({"none": true})).ok();
            }
        });
        ui.add_space(6.0);
        ui.vertical_centered(|ui| {
            let modes = ["dc-draw-normal", "dc-draw-behind", "dc-draw-inside"];
            let names = ["Draw Normal", "Draw Behind", "Draw Inside"];
            let m = app.ui.draw_mode as usize % 3;
            if widgets::icon_button(ui, modes[m], &format!("{} (Shift+D)", names[m]), false, 26.0).clicked() {
                app.ui.draw_mode = (app.ui.draw_mode + 1) % 3;
            }
            if widgets::icon_button(ui, "dc-screen-mode", "Change Screen Mode (F)", false, 26.0).clicked() {
                app.run("view.screenMode", json!({})).ok();
            }
            if widgets::icon_button(ui, "ellipsis", "Edit Toolbar", false, 26.0).clicked() {
                app.ui.dialog = Some(crate::state::Dialog::new("allTools", json!({})));
            }
        });
    });
    flyout(app, ui.ctx());
}

fn flyout(app: &mut DrawcraftApp, ctx: &egui::Context) {
    let Some(gi) = app.ui.flyout else { return };
    let Some(group) = TOOL_GROUPS.get(gi) else { return };
    let anchor: egui::Rect = ctx.data(|d| d.get_temp(egui::Id::new("flyout-anchor"))).unwrap_or(egui::Rect::from_min_size(pos2(40.0, 100.0), vec2(32.0, 32.0)));
    let t = Tokens::get(ctx);
    let mut chosen = None;
    let resp = egui::Area::new(egui::Id::new("tool-flyout")).order(egui::Order::Foreground).fixed_pos(anchor.right_top() + vec2(4.0, 0.0)).show(ctx, |ui| {
        egui::Frame::popup(ui.style()).fill(t.panel).inner_margin(egui::Margin::same(4)).show(ui, |ui| {
            ui.set_min_width(230.0);
            for tool in group.iter() {
                let active = app.session.tool_id() == tool.id;
                let (r, resp) = ui.allocate_exact_size(vec2(230.0, 28.0), Sense::click());
                if resp.hovered() || active {
                    ui.painter().rect_filled(r, CornerRadius::same(4), if resp.hovered() { t.accent_strong } else { t.hover });
                }
                if active {
                    ui.painter().rect_filled(egui::Rect::from_min_size(r.min + vec2(3.0, 11.0), vec2(4.0, 6.0)), 0.0, t.text);
                }
                let icon = icons::tool_icon(tool.icon);
                icons::paint(ui, icon, egui::Rect::from_min_size(r.min + vec2(12.0, 5.0), vec2(18.0, 18.0)), t.text);
                ui.painter().text(r.left_center() + vec2(38.0, 0.0), egui::Align2::LEFT_CENTER, tool.label, egui::FontId::proportional(12.5), t.text);
                if let Some(sc) = tool.shortcut {
                    ui.painter().text(r.right_center() - vec2(10.0, 0.0), egui::Align2::RIGHT_CENTER, sc, egui::FontId::proportional(12.0), t.text_dim);
                }
                if resp.clicked() {
                    chosen = Some(tool.id);
                }
            }
        });
    });
    if let Some(id) = chosen {
        app.select_tool(id);
    } else if resp.response.clicked_elsewhere() {
        app.ui.flyout = None;
    }
    let _ = menus::pretty_shortcut("");
}
