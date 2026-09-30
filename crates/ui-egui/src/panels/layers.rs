//! The Layers panel: tree with visibility/lock columns, layer colour bars, thumbnails, target
//! circles and selection squares; bottom bar with new/delete.

use std::collections::HashSet;

use drawcraft_doc::{Node, NodeId, NodeKind};
use egui::{Color32, Sense, Stroke, StrokeKind, Ui, vec2};
use serde_json::json;

use crate::theme::Tokens;
use crate::{DrawcraftApp, icons, widgets};

const ROW: f32 = 26.0;

fn expanded_id() -> egui::Id {
    egui::Id::new("layers-expanded")
}

pub fn show(app: &mut DrawcraftApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let Some(st) = app.session.active() else { return };
    let doc = st.doc.clone();
    let sel: HashSet<NodeId> = st.selection.objects.iter().copied().collect();
    let current = st.active_layer;
    let mut expanded: HashSet<u64> = ui.data(|d| d.get_temp(expanded_id())).unwrap_or_else(|| doc.layers.iter().map(|l| l.id.0).collect());
    let mut actions: Vec<(String, serde_json::Value)> = vec![];
    let h = ui.available_height() - 34.0;
    egui::ScrollArea::vertical().max_height(h).auto_shrink([false, false]).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        for l in doc.layers.iter().rev() {
            row(ui, &doc, l, 0, &sel, current, &mut expanded, &mut actions, &t);
        }
    });
    ui.data_mut(|d| d.insert_temp(expanded_id(), expanded));
    // Bottom bar.
    let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::hover());
    ui.painter().line_segment([bar.left_top(), bar.right_top()], Stroke::new(1.0, t.divider));
    let n = doc.layers.len();
    ui.painter().text(bar.left_center() + vec2(4.0, 0.0), egui::Align2::LEFT_CENTER, format!("{n} Layer{}", if n == 1 { "" } else { "s" }), egui::FontId::proportional(11.5), t.text_dim);
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(bar).layout(egui::Layout::right_to_left(egui::Align::Center)));
    if widgets::icon_button(&mut child, "trash-2", "Delete Selection", false, 24.0).clicked() {
        if app.session.active().is_some_and(|d| !d.selection.is_empty()) {
            actions.push(("edit.clear".into(), json!({})));
        } else {
            actions.push(("layer.delete".into(), json!({})));
        }
    }
    if widgets::icon_button(&mut child, "file-plus", "Create New Layer", false, 24.0).clicked() {
        actions.push(("layer.new".into(), json!({})));
    }
    if widgets::icon_button(&mut child, "plus", "Create New Sublayer", false, 24.0).clicked() {
        actions.push(("layer.newSublayer".into(), json!({})));
    }
    if widgets::icon_button(&mut child, "frame", "Make/Release Clipping Mask", false, 24.0).clicked() {
        actions.push(("object.clippingMask.make".into(), json!({})));
    }
    if widgets::icon_button(&mut child, "search", "Locate Object", false, 24.0).clicked() {
        // Expand ancestors of the selection.
        if let Some(st) = app.session.active() {
            let mut ex: HashSet<u64> = ui.data(|d| d.get_temp(expanded_id())).unwrap_or_default();
            for id in &st.selection.objects {
                for a in st.doc.ancestry(*id).unwrap_or_default() {
                    ex.insert(a.0);
                }
            }
            ui.data_mut(|d| d.insert_temp(expanded_id(), ex));
        }
    }
    for (c, p) in actions {
        app.run(&c, p).ok();
    }
}

#[allow(clippy::too_many_arguments)]
fn row(ui: &mut Ui, doc: &drawcraft_doc::Document, n: &Node, depth: usize, sel: &HashSet<NodeId>, current: Option<NodeId>, expanded: &mut HashSet<u64>, actions: &mut Vec<(String, serde_json::Value)>, t: &Tokens) {
    let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click());
    let is_sel = sel.contains(&n.id);
    let child_sel = !is_sel && n.children().is_some_and(|_| {
        let mut any = false;
        n.walk(&mut |c| any |= c.id != n.id && sel.contains(&c.id));
        any
    });
    let color = {
        let c = doc.layer_color(n.id);
        Color32::from_rgb(c[0], c[1], c[2])
    };
    if n.is_layer() && Some(n.id) == current {
        ui.painter().rect_filled(r, 0.0, t.row_selected);
    } else if resp.hovered() {
        ui.painter().rect_filled(r, 0.0, t.hover.gamma_multiply(0.6));
    }
    ui.painter().line_segment([r.left_bottom(), r.right_bottom()], Stroke::new(1.0, t.border));
    // Eye and lock columns.
    let eye = egui::Rect::from_min_size(r.min, vec2(24.0, ROW));
    let lock = egui::Rect::from_min_size(r.min + vec2(24.0, 0.0), vec2(22.0, ROW));
    ui.painter().line_segment([eye.right_top(), eye.right_bottom()], Stroke::new(1.0, t.border));
    ui.painter().line_segment([lock.right_top(), lock.right_bottom()], Stroke::new(1.0, t.border));
    let er = ui.interact(eye, ui.id().with(("eye", n.id.0)), Sense::click());
    if n.visible {
        icons::paint(ui, "eye", egui::Rect::from_center_size(eye.center(), vec2(14.0, 14.0)), t.icon);
    }
    if er.clicked() {
        let cmd = if n.is_layer() { "layer.setProps" } else { "object.setProps" };
        let key = if n.is_layer() { "id" } else { "ids" };
        let idv = if n.is_layer() { json!(n.id.0) } else { json!([n.id.0]) };
        actions.push((cmd.into(), json!({key: idv, "visible": !n.visible})));
    }
    let lr = ui.interact(lock, ui.id().with(("lock", n.id.0)), Sense::click());
    if n.locked {
        icons::paint(ui, "lock", egui::Rect::from_center_size(lock.center(), vec2(12.0, 12.0)), t.icon);
    } else if lr.hovered() {
        icons::paint(ui, "lock", egui::Rect::from_center_size(lock.center(), vec2(12.0, 12.0)), t.text_disabled);
    }
    if lr.clicked() {
        let cmd = if n.is_layer() { "layer.setProps" } else { "object.setProps" };
        let key = if n.is_layer() { "id" } else { "ids" };
        let idv = if n.is_layer() { json!(n.id.0) } else { json!([n.id.0]) };
        actions.push((cmd.into(), json!({key: idv, "locked": !n.locked})));
    }
    // Layer colour bar.
    let mut x = lock.right() + 2.0;
    if n.is_layer() {
        ui.painter().rect_filled(egui::Rect::from_min_size(egui::pos2(x, r.top() + 2.0), vec2(3.0, ROW - 4.0)), 0.0, color);
    }
    x += 6.0 + depth as f32 * 14.0;
    // Disclosure.
    let has_children = n.children().is_some_and(|c| !c.is_empty()) && !matches!(n.kind, NodeKind::Compound { .. });
    if has_children {
        let open = expanded.contains(&n.id.0);
        let dr = egui::Rect::from_min_size(egui::pos2(x, r.top() + 5.0), vec2(14.0, 16.0));
        let dresp = ui.interact(dr, ui.id().with(("disc", n.id.0)), Sense::click());
        icons::paint(ui, if open { "chevron-down" } else { "chevron-right" }, dr, t.text_dim);
        if dresp.clicked() {
            if open {
                expanded.remove(&n.id.0);
            } else {
                expanded.insert(n.id.0);
            }
        }
    }
    x += 16.0;
    // Thumbnail.
    let th = egui::Rect::from_min_size(egui::pos2(x, r.top() + 3.0), vec2(20.0, 20.0));
    ui.painter().rect_filled(th, 0.0, Color32::WHITE);
    ui.painter().rect_stroke(th, 0.0, Stroke::new(1.0, t.border), StrokeKind::Outside);
    thumb(ui, n, th);
    x += 26.0;
    // Name.
    let name = n.display_name();
    let font = if n.is_layer() { egui::FontId::proportional(12.5) } else { egui::FontId::proportional(12.0) };
    ui.painter().with_clip_rect(egui::Rect::from_min_max(egui::pos2(x, r.top()), egui::pos2(r.right() - 44.0, r.bottom()))).text(egui::pos2(x, r.center().y), egui::Align2::LEFT_CENTER, name, font, t.text);
    // Target circle and selection square.
    let tc = egui::pos2(r.right() - 30.0, r.center().y);
    let styled = !n.appearance.is_basic() || n.opacity < 1.0;
    ui.painter().circle_stroke(tc, 5.0, Stroke::new(1.0, t.text_dim));
    if is_sel {
        ui.painter().circle_stroke(tc, 3.0, Stroke::new(1.0, t.text_dim));
    }
    if styled {
        ui.painter().circle_filled(tc, 3.5, t.text_dim);
    }
    let sq = egui::pos2(r.right() - 12.0, r.center().y);
    if is_sel {
        ui.painter().rect_filled(egui::Rect::from_center_size(sq, vec2(8.0, 8.0)), 0.0, color);
    } else if child_sel {
        ui.painter().rect_filled(egui::Rect::from_center_size(sq, vec2(4.0, 4.0)), 0.0, color);
    }
    let sq_resp = ui.interact(egui::Rect::from_center_size(sq, vec2(18.0, ROW)), ui.id().with(("selsq", n.id.0)), Sense::click());
    if sq_resp.clicked() {
        if n.is_layer() {
            actions.push(("layer.selectAll".into(), json!({"id": n.id.0})));
        } else if ui.input(|i| i.modifiers.shift) {
            actions.push(("select.toggle".into(), json!({"id": n.id.0})));
        } else {
            actions.push(("select.set".into(), json!({"ids": [n.id.0]})));
        }
    } else if resp.clicked() {
        if n.is_layer() {
            actions.push(("layer.setCurrent".into(), json!({"id": n.id.0})));
        } else {
            actions.push(("select.set".into(), json!({"ids": [n.id.0]})));
        }
    }
    if resp.double_clicked() && n.is_layer() {
        // Rename via Layer Options (later: inline edit).
    }
    if has_children && expanded.contains(&n.id.0) {
        for c in n.children().unwrap().iter().rev() {
            row(ui, doc, c, depth + 1, sel, current, expanded, actions, t);
        }
    }
}

/// Tiny vector thumbnail painted with egui (fast, no raster).
fn thumb(ui: &Ui, n: &Node, r: egui::Rect) {
    let Some(b) = n.visual_bounds() else { return };
    let s = ((r.width() - 3.0) as f64 / b.width().max(b.height()).max(1e-6)) as f32;
    let off = r.center() - vec2(b.center().x as f32 * s, b.center().y as f32 * s);
    let p = ui.painter().with_clip_rect(r);
    let mut count = 0;
    n.walk(&mut |c| {
        if count > 40 {
            return;
        }
        if let Some(pd) = c.path_data() {
            count += 1;
            let col = c.appearance.fill_paint().color().or(c.appearance.stroke_paint().color()).map(|cc| {
                let [a, b2, d, _] = cc.to_rgba8(1.0);
                Color32::from_rgb(a, b2, d)
            });
            if let Some(bb) = pd.bounds() {
                let rr = egui::Rect::from_min_max(off + vec2(bb.x0 as f32 * s, bb.y0 as f32 * s), off + vec2(bb.x1 as f32 * s, bb.y1 as f32 * s));
                p.rect_filled(rr, 0.0, col.unwrap_or(Color32::from_gray(120)));
            }
        }
    });
}
