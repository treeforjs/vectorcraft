//! Shared widgets in Illustrator's panel style. Colours come from theme tokens.

use drawcraft_color::Paint;
use drawcraft_doc::Unit;
use egui::{Color32, CornerRadius, Pos2, Rect, Response, Sense, Stroke, StrokeKind, Ui, Vec2, pos2, vec2};

use crate::icons;
use crate::theme::{self, Tokens};

/// Square icon button; `selected` draws the pressed well.
pub fn icon_button(ui: &mut Ui, icon: &str, tip: &str, selected: bool, size: f32) -> Response {
    let t = Tokens::get(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    let bg = if selected {
        t.tool_active
    } else if resp.hovered() {
        t.hover
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, CornerRadius::same(3), bg);
    let pad = (size * 0.2).round();
    icons::paint(ui, icon, rect.shrink(pad), if selected { t.text } else { t.icon });
    if !tip.is_empty() { resp.on_hover_text(tip) } else { resp }
}

/// Small flat text button (Quick Actions style).
pub fn flat_button(ui: &mut Ui, text: &str, width: f32) -> Response {
    let t = Tokens::get(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 24.0), Sense::click());
    let rect = rect.shrink2(vec2(0.0, 0.5));
    if resp.is_pointer_button_down_on() {
        ui.painter().rect_filled(rect, CornerRadius::same(2), t.tool_active);
    } else if resp.hovered() {
        ui.painter().rect_filled(rect, CornerRadius::same(2), t.hover);
    }
    ui.painter().rect_stroke(
        rect,
        CornerRadius::same(2),
        Stroke::new(1.0, if resp.hovered() { t.text } else { t.button_border }),
        StrokeKind::Inside,
    );
    ui.painter().with_clip_rect(rect).text(rect.center(), egui::Align2::CENTER_CENTER, text, egui::FontId::proportional(12.5), t.text_strong);
    resp
}

/// Blue call-to-action pill (dialog OK/Create).
pub fn primary_button(ui: &mut Ui, text: &str) -> Response {
    let t = Tokens::get(ui.ctx());
    let galley = ui.painter().layout_no_wrap(text.to_string(), theme::semibold(12.5), Color32::WHITE);
    let w = galley.size().x + 32.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(w.max(72.0), 28.0), Sense::click());
    let bg = if resp.hovered() { t.accent } else { t.accent_strong };
    ui.painter().rect_filled(rect, CornerRadius::same(14), bg);
    ui.painter().galley(rect.center() - galley.size() / 2.0, galley, Color32::WHITE);
    resp
}

/// Outlined secondary pill (dialog Cancel).
pub fn secondary_button(ui: &mut Ui, text: &str) -> Response {
    let t = Tokens::get(ui.ctx());
    let galley = ui.painter().layout_no_wrap(text.to_string(), theme::semibold(12.5), t.text);
    let w = galley.size().x + 32.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(w.max(72.0), 28.0), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(rect, CornerRadius::same(14), t.hover);
    }
    ui.painter().rect_stroke(rect, CornerRadius::same(14), Stroke::new(1.5, t.text_dim), StrokeKind::Inside);
    ui.painter().galley(rect.center() - galley.size() / 2.0, galley, t.text);
    resp
}

/// Bold section header (Properties panel).
pub fn section_header(ui: &mut Ui, text: &str) {
    let t = Tokens::get(ui.ctx());
    ui.add_space(2.0);
    ui.label(egui::RichText::new(text).size(13.0).color(t.text));
    ui.add_space(2.0);
}

/// Full-width 1 px divider with vertical margins.
pub fn divider(ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    ui.add_space(6.0);
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().rect_filled(rect, 0.0, t.divider);
    ui.add_space(6.0);
}

pub fn dim_label(ui: &mut Ui, text: &str) -> Response {
    let t = Tokens::get(ui.ctx());
    ui.label(egui::RichText::new(text).color(t.text).size(12.5))
}

/// A recessed numeric field showing `value` (points) in `unit`. Returns the new value (points)
/// when the user commits (Enter / focus loss). Supports unit suffixes and arithmetic.
pub fn num_field(ui: &mut Ui, id: impl std::hash::Hash + std::fmt::Debug, value: Option<f64>, unit: Unit, width: f32) -> Option<f64> {
    let t = Tokens::get(ui.ctx());
    let id = ui.id().with(id);
    let shown = value.map(|v| unit.format(v)).unwrap_or_default();
    let mut buf: String = ui.data_mut(|d| d.get_temp::<String>(id)).unwrap_or_else(|| shown.clone());
    let editing = ui.memory(|m| m.has_focus(id));
    if !editing {
        buf = shown.clone();
    }
    let resp = egui::Frame::NONE
        .fill(t.input)
        .stroke(Stroke::new(1.0, if editing { t.accent } else { t.input_border }))
        .corner_radius(CornerRadius::same(2))
        .inner_margin(egui::Margin::symmetric(6, 4))
        .show(ui, |ui| {
            ui.add(
                egui::TextEdit::singleline(&mut buf)
                    .id(id)
                    .frame(egui::Frame::NONE)
                    .desired_width(width - 14.0)
                    .font(egui::FontId::proportional(12.5))
                    .text_color(t.text_strong),
            )
        })
        .inner;
    let commit = resp.lost_focus() && buf != shown;
    ui.data_mut(|d| d.insert_temp(id, buf.clone()));
    if commit { unit.parse(&buf) } else { None }
}

/// A plain number field (percent, degrees, counts) with optional suffix.
pub fn plain_field(ui: &mut Ui, id: impl std::hash::Hash + std::fmt::Debug, value: f64, suffix: &str, decimals: usize, width: f32) -> Option<f64> {
    let t = Tokens::get(ui.ctx());
    let id = ui.id().with(id);
    let shown = {
        let s = format!("{:.*}", decimals, value);
        let s = if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_string() } else { s };
        format!("{s}{suffix}")
    };
    let editing = ui.memory(|m| m.has_focus(id));
    let mut buf: String = if editing { ui.data_mut(|d| d.get_temp::<String>(id)).unwrap_or_else(|| shown.clone()) } else { shown.clone() };
    let resp = egui::Frame::NONE
        .fill(t.input)
        .stroke(Stroke::new(1.0, if editing { t.accent } else { t.input_border }))
        .corner_radius(CornerRadius::same(2))
        .inner_margin(egui::Margin::symmetric(6, 4))
        .show(ui, |ui| {
            ui.add(
                egui::TextEdit::singleline(&mut buf)
                    .id(id)
                    .frame(egui::Frame::NONE)
                    .desired_width(width - 14.0)
                    .font(egui::FontId::proportional(12.5))
                    .text_color(t.text_strong),
            )
        })
        .inner;
    ui.data_mut(|d| d.insert_temp(id, buf.clone()));
    if resp.lost_focus() && buf != shown {
        buf.trim().trim_end_matches(suffix.trim()).trim().trim_end_matches(['%', '°']).parse::<f64>().ok()
    } else {
        None
    }
}

/// Draw a paint preview (swatch chip) into `rect`.
pub fn paint_chip(ui: &Ui, rect: Rect, paint: &Paint) {
    let p = ui.painter();
    match paint {
        Paint::None => {
            p.rect_filled(rect, 0.0, Color32::WHITE);
            p.line_segment([rect.left_bottom(), rect.right_top()], Stroke::new(1.6, Color32::from_rgb(0xe0, 0x20, 0x20)));
        }
        Paint::Solid { color, .. } => {
            let [r, g, b, _] = color.to_rgba8(1.0);
            p.rect_filled(rect, 0.0, Color32::from_rgb(r, g, b));
        }
        Paint::Gradient(gp) => {
            let n = 24;
            let w = rect.width() / n as f32;
            for i in 0..n {
                let tt = (i as f32 + 0.5) / n as f32;
                let (c, _) = gp.gradient.sample(tt);
                let [r, g, b, _] = c.to_rgba8(1.0);
                let rr = Rect::from_min_size(pos2(rect.left() + i as f32 * w, rect.top()), vec2(w + 0.5, rect.height()));
                if gp.gradient.kind == drawcraft_color::GradientKind::Radial {
                    let (c, _) = gp.gradient.sample(1.0 - tt);
                    let [r, g, b, _] = c.to_rgba8(1.0);
                    let s = rect.width().min(rect.height()) * tt / 2.0;
                    p.circle_filled(rect.center(), s.max(0.5), Color32::from_rgb(r, g, b));
                } else {
                    p.rect_filled(rr, 0.0, Color32::from_rgb(r, g, b));
                }
            }
        }
        Paint::Pattern { .. } => {
            p.rect_filled(rect, 0.0, Color32::from_gray(200));
            for i in 0..4 {
                let x = rect.left() + rect.width() * i as f32 / 4.0;
                p.line_segment([pos2(x, rect.bottom()), pos2(x + rect.width() / 4.0, rect.top())], Stroke::new(1.0, Color32::from_gray(90)));
            }
        }
    }
}

/// The Fill/Stroke proxy pair (Illustrator's overlapping squares). Returns (fill clicked, stroke clicked, swap, default).
pub fn fill_stroke_proxy(ui: &mut Ui, fill: &Paint, stroke: &Paint, fill_active: bool, size: f32) -> (bool, bool, bool, bool) {
    let t = Tokens::get(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(vec2(size, size), Sense::hover());
    let s = size * 0.62;
    let fill_r = Rect::from_min_size(rect.min + vec2(0.0, 0.0), Vec2::splat(s));
    let stroke_r = Rect::from_min_size(rect.max - Vec2::splat(s), Vec2::splat(s));
    let stroke_resp = ui.interact(stroke_r, ui.id().with("stroke-proxy"), Sense::click());
    let fill_resp = ui.interact(fill_r, ui.id().with("fill-proxy"), Sense::click());
    let draw_fill = |ui: &Ui| {
        paint_chip(ui, fill_r, fill);
        ui.painter().rect_stroke(fill_r, 0.0, Stroke::new(1.0, Color32::from_gray(20)), StrokeKind::Inside);
        ui.painter().rect_stroke(fill_r.expand(1.0), 0.0, Stroke::new(1.0, Color32::from_gray(150)), StrokeKind::Outside);
    };
    let draw_stroke = |ui: &Ui| {
        let w = s * 0.28;
        paint_chip(ui, stroke_r, stroke);
        let inner = stroke_r.shrink(w);
        ui.painter().rect_filled(inner, 0.0, t.panel);
        ui.painter().rect_stroke(stroke_r, 0.0, Stroke::new(1.0, Color32::from_gray(20)), StrokeKind::Inside);
        ui.painter().rect_stroke(inner, 0.0, Stroke::new(1.0, Color32::from_gray(20)), StrokeKind::Outside);
        ui.painter().rect_stroke(stroke_r.expand(1.0), 0.0, Stroke::new(1.0, Color32::from_gray(150)), StrokeKind::Outside);
    };
    if fill_active {
        draw_stroke(ui);
        draw_fill(ui);
    } else {
        draw_fill(ui);
        draw_stroke(ui);
    }
    // Swap arrow (top-right) and default (bottom-left) mini buttons.
    let swap_r = Rect::from_min_size(pos2(rect.right() - size * 0.3, rect.top()), Vec2::splat(size * 0.3));
    let def_r = Rect::from_min_size(pos2(rect.left(), rect.bottom() - size * 0.3), Vec2::splat(size * 0.3));
    let swap = ui.interact(swap_r, ui.id().with("swap-proxy"), Sense::click());
    let def = ui.interact(def_r, ui.id().with("default-proxy"), Sense::click());
    icons::paint(ui, "dc-swap", swap_r.shrink(1.0), if swap.hovered() { t.text } else { t.icon });
    let a = Rect::from_min_size(def_r.min + vec2(1.0, 1.0), Vec2::splat(size * 0.14));
    let b = Rect::from_min_size(def_r.min + vec2(size * 0.1, size * 0.1), Vec2::splat(size * 0.14));
    ui.painter().rect_filled(b, 0.0, Color32::BLACK);
    ui.painter().rect_stroke(b, 0.0, Stroke::new(1.0, Color32::WHITE), StrokeKind::Inside);
    ui.painter().rect_filled(a, 0.0, Color32::WHITE);
    ui.painter().rect_stroke(a, 0.0, Stroke::new(1.0, Color32::BLACK), StrokeKind::Inside);
    (
        fill_resp.on_hover_text("Fill (X)").clicked(),
        stroke_resp.on_hover_text("Stroke (X)").clicked(),
        swap.on_hover_text("Swap Fill and Stroke (Shift+X)").clicked(),
        def.on_hover_text("Default Fill and Stroke (D)").clicked(),
    )
}

/// A compact dropdown. Returns the chosen index.
pub fn dropdown(ui: &mut Ui, id: impl std::hash::Hash + std::fmt::Debug, current: &str, options: &[&str], width: f32) -> Option<usize> {
    let t = Tokens::get(ui.ctx());
    let mut chosen = None;
    let resp = egui::Frame::NONE.fill(t.input).stroke(Stroke::new(1.0, t.input_border)).corner_radius(CornerRadius::same(3)).show(ui, |ui| {
        egui::ComboBox::from_id_salt(ui.id().with(id)).selected_text(egui::RichText::new(current).size(12.0)).width(width - 4.0).show_ui(ui, |ui| {
            for (i, o) in options.iter().enumerate() {
                if ui.selectable_label(*o == current, *o).clicked() {
                    chosen = Some(i);
                }
            }
        })
    });
    let _ = resp;
    chosen
}

/// The 3×3 reference point locator. Returns a new index when clicked.
pub fn reference_point(ui: &mut Ui, current: usize) -> Option<usize> {
    let t = Tokens::get(ui.ctx());
    let size = 22.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    let mut out = None;
    let step = size / 2.0 - 2.5;
    ui.painter().rect_stroke(rect.shrink(4.5), 0.0, Stroke::new(1.0, t.text_dim), StrokeKind::Middle);
    for i in 0..9 {
        let c = Pos2::new(rect.left() + 3.0 + (i % 3) as f32 * step + 1.5, rect.top() + 3.0 + (i / 3) as f32 * step + 1.5);
        let r = Rect::from_center_size(c, Vec2::splat(5.0));
        let resp = ui.interact(r.expand(1.5), ui.id().with(("refpt", i)), Sense::click());
        if resp.clicked() {
            out = Some(i);
        }
        if i == current {
            ui.painter().rect_filled(r, 0.0, t.text);
        } else {
            ui.painter().rect_filled(r, 0.0, t.panel);
            ui.painter().rect_stroke(r, 0.0, Stroke::new(1.0, t.text_dim), StrokeKind::Inside);
        }
    }
    out
}

/// A toggle icon (e.g. eye / lock columns) — returns clicked.
pub fn toggle_icon(ui: &mut Ui, on_icon: &str, on: bool, size: f32, tip: &str) -> bool {
    let t = Tokens::get(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    if on {
        icons::paint(ui, on_icon, rect.shrink(size * 0.2), if resp.hovered() { t.text } else { t.icon });
    } else if resp.hovered() {
        icons::paint(ui, on_icon, rect.shrink(size * 0.2), t.text_disabled);
    }
    resp.on_hover_text(tip).clicked()
}
