//! Quick styles and colour themes: one-click looks made from the document's theme
//! (`quick.style`), offered in the selection's task bar and in Properties, and the theme picker
//! (`quick.theme`) that recolours quick-styled art. The previews are a few painter shapes, drawn
//! only while a picker is open or a selection shows them.

use egui::{Color32, CornerRadius, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use serde_json::json;
use vectorcraft_engine::cmd::quick::{self, STYLES, THEMES, Theme};

use crate::VectorcraftApp;
use crate::theme::Tokens;
use crate::widgets;

/// A quick style's name in the UI language.
pub fn style_label(id: &str) -> &'static str {
    match id {
        "solid" => tl!("Solid"),
        "outline" => tl!("Outline"),
        "soft" => tl!("Soft"),
        "accent" => tl!("Accent"),
        "dashed" => tl!("Dashed"),
        "shadow" => tl!("Shadow"),
        "dark" => tl!("Dark"),
        "alternate" => tl!("Alternate"),
        _ => "",
    }
}

/// A theme's name in the UI language.
pub fn theme_label(id: &str) -> &'static str {
    match id {
        "ocean" => tl!("Ocean"),
        "sunset" => tl!("Sunset"),
        "forest" => tl!("Forest"),
        "berry" => tl!("Berry"),
        "slate" => tl!("Slate"),
        "citrus" => tl!("Citrus"),
        _ => "",
    }
}

fn c32(c: vectorcraft_color::Color) -> Color32 {
    let [r, g, b, _] = c.to_rgba8(1.0);
    Color32::from_rgb(r, g, b)
}

/// The active document's theme and the look new inserts get.
pub fn current(app: &VectorcraftApp) -> (&'static Theme, &'static str) {
    match app.session.active() {
        Some(st) => (quick::theme_of(&st.doc), quick::style_of(&st.doc)),
        None => (THEMES.first().unwrap_or(&FALLBACK), "solid"),
    }
}

const FALLBACK: Theme = Theme { id: "ocean", label: "Ocean", colors: ["#1f6feb", "#13a8a0", "#f2a20c", "#13213a", "#f3f7fc"] };

/// Paint style `style`'s preview (a small rounded card in its look) into `r`.
pub fn paint_preview(ui: &Ui, r: Rect, style: &str, theme: &Theme) {
    let Some((fill, stroke, dash, shadow)) = quick::look_preview(style, theme) else { return };
    let p = ui.painter();
    let card = r.shrink2(vec2(r.width() * 0.16, r.height() * 0.22));
    let radius = CornerRadius::same(3);
    if shadow {
        p.rect_filled(card.translate(vec2(0.0, 2.0)).expand(1.0), radius, Color32::from_black_alpha(60));
    }
    if let Some(f) = fill {
        p.rect_filled(card, radius, c32(f));
    }
    if let Some(s) = stroke {
        let st = Stroke::new(if dash { 1.2 } else { 1.6 }, c32(s));
        if dash {
            let pts = [card.left_top(), card.right_top(), card.right_bottom(), card.left_bottom(), card.left_top()];
            p.extend(egui::Shape::dashed_line(&pts, st, 3.0, 2.0));
        } else {
            p.rect_stroke(card, radius, st, StrokeKind::Inside);
        }
    }
}

/// A clickable preview tile of `style` → clicked.
fn style_tile(ui: &mut Ui, style: &str, theme: &Theme, current: bool, size: egui::Vec2) -> bool {
    let t = Tokens::get(ui.ctx());
    let (r, resp) = ui.allocate_exact_size(size, Sense::click());
    // On a white page, as the art will be.
    ui.painter().rect_filled(r, CornerRadius::same(4), if resp.hovered() { Color32::from_gray(232) } else { Color32::WHITE });
    paint_preview(ui, r, style, theme);
    let border = if current { Stroke::new(2.0, t.accent) } else { Stroke::new(1.0, t.button_border) };
    ui.painter().rect_stroke(r, CornerRadius::same(4), border, StrokeKind::Inside);
    resp.on_hover_text(style_label(style)).clicked()
}

/// Apply quick style `style` to the selection (and make it the look of new inserts); with
/// nothing selected, only the latter.
pub fn apply_style(app: &mut VectorcraftApp, style: &str) {
    let has_selection = app.session.active().is_some_and(|st| !st.selection.is_empty());
    let p = if has_selection { json!({ "style": style }) } else { json!({ "style": style, "ids": [] }) };
    crate::menus::invoke(app, "quick.style", p);
}

/// The style picker: a grid of the eight looks → the one clicked.
pub fn style_grid(app: &VectorcraftApp, ui: &mut Ui, columns: usize, tile: egui::Vec2) -> Option<&'static str> {
    let (theme, now) = current(app);
    let mut chosen = None;
    for row in STYLES.chunks(columns.max(1)) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            for s in row {
                if style_tile(ui, s.id, theme, s.id == now, tile) {
                    chosen = Some(s.id);
                }
            }
        });
    }
    chosen
}

/// Five dots of a theme's colours in `r`.
fn theme_dots(ui: &Ui, r: Rect, theme: &Theme) {
    let colors = quick::theme_colors(theme);
    let d = (r.height() * 0.7).min(r.width() / 5.6);
    for (i, c) in colors.iter().enumerate() {
        let x = r.left() + d / 2.0 + i as f32 * d * 1.1;
        ui.painter().circle(pos2(x, r.center().y), d / 2.0, c32(*c), Stroke::new(1.0, Tokens::get(ui.ctx()).text_dim));
    }
}

/// The theme picker rows → the theme clicked.
pub fn theme_list(app: &VectorcraftApp, ui: &mut Ui) -> Option<&'static str> {
    let t = Tokens::get(ui.ctx());
    let (now, _) = current(app);
    let mut chosen = None;
    for th in THEMES {
        let (r, resp) = ui.allocate_exact_size(vec2(200.0, 28.0), Sense::click());
        if resp.hovered() || th.id == now.id {
            ui.painter().rect_filled(r, CornerRadius::same(3), if resp.hovered() { t.hover } else { t.row_selected });
        }
        theme_dots(ui, Rect::from_min_size(r.min + vec2(8.0, 6.0), vec2(80.0, 16.0)), th);
        ui.painter().text(
            pos2(r.left() + 98.0, r.center().y),
            egui::Align2::LEFT_CENTER,
            theme_label(th.id),
            egui::FontId::proportional(13.0),
            t.text_strong,
        );
        if resp.clicked() {
            chosen = Some(th.id);
        }
    }
    chosen
}

/// The body of Properties' Quick Style section: the eight looks and the document's theme.
pub fn inspector_section(app: &mut VectorcraftApp, ui: &mut Ui) {
    let w = ((ui.available_width() - 12.0) / 4.0).clamp(30.0, 60.0);
    if let Some(s) = style_grid(app, ui, 4, vec2(w, 30.0)) {
        apply_style(app, s);
    }
    let (theme, _) = current(app);
    ui.add_space(2.0);
    ui.horizontal(|ui| {
        widgets::dim_label(ui, tl!("Theme"));
        let resp = theme_button(ui, theme, 150.0);
        egui::Popup::menu(&resp).show(|ui| {
            if let Some(id) = theme_list(app, ui) {
                crate::menus::invoke(app, "quick.theme", json!({ "theme": id }));
            }
        });
    });
}

/// A button showing `theme`'s dots and name (it opens the theme picker).
pub fn theme_button(ui: &mut Ui, theme: &Theme, width: f32) -> egui::Response {
    let t = Tokens::get(ui.ctx());
    let (r, resp) = ui.allocate_exact_size(vec2(width, 26.0), Sense::click());
    ui.painter().rect_filled(r, CornerRadius::same(3), if resp.hovered() { t.hover } else { t.input });
    ui.painter().rect_stroke(r, CornerRadius::same(3), Stroke::new(1.0, t.button_border), StrokeKind::Inside);
    theme_dots(ui, Rect::from_min_size(r.min + vec2(6.0, 6.0), vec2(56.0, 14.0)), theme);
    ui.painter().with_clip_rect(r).text(
        pos2(r.left() + 68.0, r.center().y),
        egui::Align2::LEFT_CENTER,
        theme_label(theme.id),
        egui::FontId::proportional(12.5),
        t.text_strong,
    );
    resp.on_hover_text(tl!("Color theme: recolors quick-styled art"))
}

/// The task bar's Style chip: the current look's preview (beside the fill chip), opening the
/// style picker and the themes.
pub fn task_bar_button(app: &mut VectorcraftApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let (theme, now) = current(app);
    let (r, resp) = ui.allocate_exact_size(vec2(34.0, 30.0), Sense::click());
    ui.painter().rect_filled(r, CornerRadius::same(3), if resp.hovered() { t.hover } else { Color32::WHITE });
    ui.painter().rect_stroke(r, CornerRadius::same(3), Stroke::new(1.0, t.button_border), StrokeKind::Inside);
    paint_preview(ui, r.shrink(2.0), now, theme);
    let resp = resp.on_hover_text(tl!("Quick styles: one-click looks in the document's colors"));
    egui::Popup::menu(&resp).show(|ui| {
        if let Some(s) = style_grid(app, ui, 4, vec2(52.0, 38.0)) {
            apply_style(app, s);
        }
        ui.separator();
        if let Some(id) = theme_list(app, ui) {
            crate::menus::invoke(app, "quick.theme", json!({ "theme": id }));
        }
    });
}
