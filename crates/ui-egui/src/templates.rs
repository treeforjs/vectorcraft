//! Home's "Start from a template" row: a card per starter template (`file.newFromTemplate {id}`)
//! and the blank pages in common sizes. The thumbnails are painted from the templates' static
//! description (a few rectangles, ellipses and bars per card), so Home builds no document to show
//! them, and a click opens the template with its name and placeholder texts in the UI language.

use egui::{Color32, CornerRadius, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Ui, pos2, vec2};
use serde_json::{Map, Value, json};
use vectorcraft_engine::cmd::quick;
use vectorcraft_engine::cmd::templates::{Part, TEMPLATES, Template};

use crate::VectorcraftApp;
use crate::theme::{self, Tokens};

/// A template's name in the UI language (also the new document's title).
pub fn label(id: &str) -> &'static str {
    match id {
        "social-post" => tl!("Social post"),
        "story" => tl!("Story"),
        "slide" => tl!("Slide graphic"),
        "banner" => tl!("Banner"),
        "flyer" => tl!("Flyer"),
        "logo" => tl!("Logo / icon"),
        "diagram" => tl!("Diagram"),
        _ => "",
    }
}

/// A placeholder text of the templates in the UI language (None: not one of theirs).
pub fn placeholder(en: &str) -> Option<&'static str> {
    Some(match en {
        "Your headline" => tl!("Your headline"),
        "Add a short line that says why it matters." => tl!("Add a short line that says why it matters."),
        "Learn more" => tl!("Learn more"),
        "Your picture here" => tl!("Your picture here"),
        "A line or two about it." => tl!("A line or two about it."),
        "Swipe up" => tl!("Swipe up"),
        "Your main point, in one short sentence" => tl!("Your main point, in one short sentence"),
        "First point" => tl!("First point"),
        "Second point" => tl!("Second point"),
        "Third point" => tl!("Third point"),
        "A short tagline goes here" => tl!("A short tagline goes here"),
        "yourwebsite.com" => tl!("yourwebsite.com"),
        "Date · Time · Place" => tl!("Date · Time · Place"),
        "Say what is happening and why people should come. Replace this text with your own." => {
            tl!("Say what is happening and why people should come. Replace this text with your own.")
        }
        "Your call to action" => tl!("Your call to action"),
        "your@email.com · yourwebsite.com" => tl!("your@email.com · yourwebsite.com"),
        "Your name" => tl!("Your name"),
        "Your process" => tl!("Your process"),
        "Start" => tl!("Start"),
        "First step" => tl!("First step"),
        "Yes or no?" => tl!("Yes or no?"),
        "Next step" => tl!("Next step"),
        "Try again" => tl!("Try again"),
        "Yes" => tl!("Yes"),
        "No" => tl!("No"),
        _ => return None,
    })
}

/// `file.newFromTemplate`'s params for template `t` in the UI language: its name as the title
/// (designed templates; a blank page is untitled) and the translated placeholder texts.
pub fn open_params(t: &Template) -> Value {
    if t.preset.is_some() {
        return json!({ "id": t.id });
    }
    let mut texts = Map::new();
    for en in vectorcraft_engine::cmd::templates::texts() {
        if let Some(shown) = placeholder(en).filter(|s| *s != en) {
            texts.insert(en.to_string(), json!(shown));
        }
    }
    json!({ "id": t.id, "title": label(t.id), "texts": texts })
}

/// Open template `id` from Home (Home closes on success, an error goes to the status bar).
fn open(app: &mut VectorcraftApp, t: &Template) {
    match app.run("file.newFromTemplate", open_params(t)) {
        Ok(_) => app.ui.home = None,
        Err(e) => app.status(e),
    }
}

/// Where the cards were last drawn, for `ui.inspect` (`homeTemplates`).
type Cards = Vec<(&'static str, Rect)>;

fn cards_id() -> egui::Id {
    egui::Id::new("home-template-cards")
}

/// The cards Home showed last ({id, rect}), for agents; empty while Home isn't showing.
pub fn inspect(app: &VectorcraftApp, ctx: &egui::Context) -> Value {
    if !crate::menus::home_showing(app) {
        return json!([]);
    }
    let cards: Option<std::sync::Arc<Cards>> = ctx.data(|d| d.get_temp(cards_id()));
    Value::Array(cards.iter().flat_map(|v| v.iter()).map(|(id, r)| json!({ "id": id, "rect": [r.left(), r.top(), r.width(), r.height()] })).collect())
}

const CARD: egui::Vec2 = egui::vec2(146.0, 150.0);

/// Home's "Start from a template" section: the designed templates as cards, then the blank pages.
pub fn home_section(app: &mut VectorcraftApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    ui.add_space(28.0);
    ui.label(egui::RichText::new(tl!("Start from a template")).font(theme::semibold(14.0)).color(t.text));
    ui.add_space(2.0);
    ui.label(
        egui::RichText::new(tl!("A page at the right size with a layout to edit: replace the words, then change colors with the theme."))
            .size(12.0)
            .color(t.text_dim),
    );
    ui.add_space(10.0);
    let mut picked = None;
    let mut cards: Cards = vec![];
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(14.0, 14.0);
        for tpl in TEMPLATES.iter().filter(|t| t.preset.is_none()) {
            let resp = card(ui, tpl);
            cards.push((tpl.id, resp.rect));
            if resp.clicked() {
                picked = Some(tpl);
            }
        }
    });
    ui.add_space(12.0);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(6.0, 8.0);
        ui.label(egui::RichText::new(tl!("Or a blank page:")).size(12.5).color(t.text_dim));
        ui.add_space(4.0);
        for tpl in TEMPLATES.iter().filter(|t| t.preset.is_some()) {
            let name = tpl.preset.map(crate::dialogs::preset_name).unwrap_or_default();
            let resp = crate::widgets::secondary_button(ui, name).on_hover_text(tl!("A blank page in this size"));
            cards.push((tpl.id, resp.rect));
            if resp.clicked() {
                picked = Some(tpl);
            }
        }
    });
    ui.ctx().data_mut(|d| d.insert_temp(cards_id(), std::sync::Arc::new(cards)));
    if let Some(tpl) = picked {
        open(app, tpl);
    }
}

/// A template's card: its thumbnail, name and size.
fn card(ui: &mut Ui, tpl: &Template) -> egui::Response {
    let t = Tokens::get(ui.ctx());
    let (r, resp) = ui.allocate_exact_size(CARD, Sense::click());
    let p = ui.painter();
    p.rect_filled(r, CornerRadius::same(6), if resp.hovered() { t.hover } else { t.panel_darker });
    let frame = Rect::from_min_size(r.min + vec2(8.0, 8.0), vec2(r.width() - 16.0, 100.0));
    let k = ((frame.width() - 8.0) / tpl.width.max(1.0) as f32).min((frame.height() - 8.0) / tpl.height.max(1.0) as f32);
    let page = Rect::from_center_size(frame.center(), vec2(tpl.width as f32 * k, tpl.height as f32 * k));
    p.rect_filled(page.translate(vec2(2.0, 2.0)), 0.0, Color32::from_black_alpha(80));
    p.rect_filled(page, 0.0, Color32::WHITE);
    p.with_clip_rect(page).extend(thumbnail_shapes(ui.ctx(), tpl, page));
    p.text(pos2(r.center().x, r.top() + 116.0), egui::Align2::CENTER_TOP, label(tpl.id), theme::semibold(11.5), t.text_strong);
    p.text(pos2(r.center().x, r.top() + 133.0), egui::Align2::CENTER_TOP, size_label(tpl), egui::FontId::proportional(11.0), t.text_dim);
    resp.on_hover_text(tl!("Start a new document from this template"))
}

/// The page size in the template's units (`1080 × 1080 px`, `8.5 × 11 in`).
fn size_label(tpl: &Template) -> String {
    let (k, unit) = match tpl.units {
        "in" => (1.0 / 72.0, "in"),
        "mm" => (25.4 / 72.0, "mm"),
        _ => (1.0, "px"),
    };
    let n = |v: f64| {
        let v = (v * k * 100.0).round() / 100.0;
        if v.fract() == 0.0 { format!("{v:.0}") } else { format!("{v}") }
    };
    format!("{} × {} {unit}", n(tpl.width), n(tpl.height))
}

/// Shapes collected for a thumbnail (the painter calls it uses, as shapes).
#[derive(Default)]
struct Out(Vec<Shape>);

impl Out {
    fn rect_filled(&mut self, r: Rect, radius: impl Into<CornerRadius>, c: Color32) {
        self.0.push(Shape::rect_filled(r, radius, c));
    }
    fn rect_stroke(&mut self, r: Rect, radius: impl Into<CornerRadius>, st: Stroke, kind: StrokeKind) {
        self.0.push(Shape::rect_stroke(r, radius, st, kind));
    }
    fn line_segment(&mut self, pts: [Pos2; 2], st: Stroke) {
        self.0.push(Shape::line_segment(pts, st));
    }
    fn add(&mut self, sh: Shape) {
        self.0.push(sh);
    }
}

/// Template `tpl`'s thumbnail shapes for `page`: worked out once per template and page size
/// (Home's cards keep theirs), then only moved to where the card is.
fn thumbnail_shapes(ctx: &egui::Context, tpl: &Template, page: Rect) -> Vec<Shape> {
    let id = egui::Id::new(("template-thumb", tpl.id, page.width().to_bits(), page.height().to_bits()));
    let cached: Option<std::sync::Arc<Vec<Shape>>> = ctx.data(|d| d.get_temp(id));
    let shapes = cached.unwrap_or_else(|| {
        let mut out = Out::default();
        thumbnail(&mut out, tpl, Rect::from_min_size(Pos2::ZERO, page.size()));
        let shapes = std::sync::Arc::new(out.0);
        ctx.data_mut(|d| d.insert_temp(id, shapes.clone()));
        shapes
    });
    let offset = page.min.to_vec2();
    shapes
        .iter()
        .cloned()
        .map(|mut sh| {
            sh.translate(offset);
            sh
        })
        .collect()
}

fn c32(c: vectorcraft_color::Color) -> Color32 {
    let [r, g, b, _] = c.to_rgba8(1.0);
    Color32::from_rgb(r, g, b)
}

/// Paint template `tpl`'s layout into `page`: its shapes as painter shapes, its words as bars.
fn thumbnail(p: &mut Out, tpl: &Template, page: Rect) {
    let Some(theme) = quick::theme(tpl.theme) else { return };
    let colors = quick::theme_colors(theme);
    let at = |x: f64, y: f64| pos2(page.left() + x as f32 * page.width(), page.top() + y as f32 * page.height());
    let side = page.width().min(page.height());
    for part in tpl.parts {
        match *part {
            Part::Shape { shape, rect, style, text, size, .. } => {
                let r = Rect::from_two_pos(at(rect[0], rect[1]), at(rect[2], rect[3]));
                let (fill, stroke, dash, shadow) = match style {
                    "paper" => (colors.get(4).copied(), None, false, false),
                    s => quick::look_preview(s, theme).unwrap_or((None, None, false, false)),
                };
                if shadow {
                    paint_shape(p, shape, r.translate(vec2(0.0, 1.0)), Some(Color32::from_black_alpha(50)), None);
                }
                let st = stroke.map(|c| Stroke::new(if dash { 0.6 } else { 1.0 }, c32(c)));
                paint_shape(p, shape, r, fill.map(c32), st);
                if let Some(words) = text {
                    let ink = quick::text_color_on(fill, theme);
                    bars(p, words, r.shrink(r.width().min(r.height()) * 0.12), size as f32 * side, c32(ink), true);
                }
            }
            Part::Text { x, y, w, size, ink, bold, center, text } => {
                let r = Rect::from_min_size(at(x, y), vec2(w as f32 * page.width(), page.height()));
                let color = colors.get(ink.index()).copied().map_or(Color32::BLACK, c32);
                let h = size as f32 * side * if bold { 1.0 } else { 0.85 };
                bars_top(p, text, r, h, color, center);
            }
            Part::Arrow { from, to } => {
                let (a, z) = (at(from[0], from[1]), at(to[0], to[1]));
                let color = colors.get(3).copied().map_or(Color32::BLACK, c32);
                p.line_segment([a, z], Stroke::new(1.0, color));
                let d = (z - a).normalized();
                let n = vec2(-d.y, d.x);
                let head = 3.0;
                p.add(Shape::convex_polygon(vec![z, z - d * head + n * head * 0.6, z - d * head - n * head * 0.6], color, Stroke::NONE));
            }
        }
    }
}

/// A quick shape's outline in `r`, filled and/or stroked.
fn paint_shape(p: &mut Out, shape: &str, r: Rect, fill: Option<Color32>, stroke: Option<Stroke>) {
    let fill_c = fill.unwrap_or(Color32::TRANSPARENT);
    let st = stroke.unwrap_or(Stroke::NONE);
    match shape {
        "roundedRectangle" | "rectangle" => {
            let radius = if shape == "rectangle" { 0.0 } else { r.width().min(r.height()) * 0.18 };
            let radius = CornerRadius::same(radius.clamp(0.0, 255.0) as u8);
            p.rect_filled(r, radius, fill_c);
            if stroke.is_some() {
                p.rect_stroke(r, radius, st, StrokeKind::Inside);
            }
        }
        "ellipse" => {
            p.add(Shape::ellipse_filled(r.center(), r.size() / 2.0, fill_c));
            if stroke.is_some() {
                p.add(Shape::ellipse_stroke(r.center(), r.size() / 2.0, st));
            }
        }
        "diamond" => {
            let pts = vec![r.center_top(), r.right_center(), r.center_bottom(), r.left_center()];
            p.add(Shape::convex_polygon(pts, fill_c, st));
        }
        "star" => {
            // A fan from the centre: a star is star-shaped from there, so the triangles cover it.
            let c = r.center();
            let outer = r.width().min(r.height()) / 2.0;
            let pts: Vec<Pos2> = (0..10)
                .map(|i| {
                    let a = std::f32::consts::PI * (i as f32 / 5.0) - std::f32::consts::FRAC_PI_2;
                    let rad = if i % 2 == 0 { outer } else { outer * 0.48 };
                    c + vec2(a.cos(), a.sin()) * rad
                })
                .collect();
            let mut mesh = egui::Mesh::default();
            mesh.colored_vertex(c, fill_c);
            for q in &pts {
                mesh.colored_vertex(*q, fill_c);
            }
            for i in 0..10u32 {
                mesh.add_triangle(0, 1 + i, 1 + (i + 1) % 10);
            }
            p.add(Shape::mesh(mesh));
            if stroke.is_some() {
                p.add(Shape::closed_line(pts, st));
            }
        }
        _ => {
            p.rect_filled(r, 0.0, fill_c);
        }
    }
}

/// About how wide `words` set at `h` is, and on how many lines of width `w` (at most 3).
fn lines(words: &str, h: f32, w: f32) -> (f32, usize) {
    let total = words.chars().count() as f32 * h * 0.5;
    let n = if w > 0.0 { (total / w).ceil().clamp(1.0, 3.0) as usize } else { 1 };
    (total, n)
}

/// Words as bars centred in `r`.
fn bars(p: &mut Out, words: &str, r: Rect, h: f32, color: Color32, center: bool) {
    let (total, n) = lines(words, h, r.width());
    let block = n as f32 * h * 1.25;
    let top = Rect::from_min_size(pos2(r.left(), r.center().y - block / 2.0), vec2(r.width(), block));
    draw_bars(p, top, total, n, h, color, center);
}

/// Words as bars from the top of `r`.
fn bars_top(p: &mut Out, words: &str, r: Rect, h: f32, color: Color32, center: bool) {
    let (total, n) = lines(words, h, r.width());
    draw_bars(p, r, total, n, h, color, center);
}

fn draw_bars(p: &mut Out, r: Rect, total: f32, n: usize, h: f32, color: Color32, center: bool) {
    let mut left = total;
    for i in 0..n {
        let w = left.min(r.width()).max(1.0);
        left -= w;
        let y = r.top() + i as f32 * h * 1.25 + h * 0.15;
        let x = if center { r.center().x - w / 2.0 } else { r.left() };
        let bar = Rect::from_min_size(pos2(x, y), vec2(w, (h * 0.7).max(0.8)));
        p.rect_filled(bar, CornerRadius::same(((h * 0.2).clamp(0.0, 4.0)) as u8), color.gamma_multiply(0.92));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every template has a name and every placeholder a translatable string.
    #[test]
    fn every_template_and_placeholder_is_named() {
        for t in TEMPLATES.iter().filter(|t| t.preset.is_none()) {
            assert!(!label(t.id).is_empty(), "{}", t.id);
        }
        for s in vectorcraft_engine::cmd::templates::texts() {
            assert!(placeholder(s).is_some(), "no tl! for {s:?}");
        }
    }

    /// A card's click opens the template titled with its name and leaves Home; a blank page is
    /// untitled; the thumbnails are worked out once.
    #[test]
    fn home_cards_open_templates() {
        let mut app = crate::VectorcraftApp::new(vectorcraft_engine::Session::new(), crate::Services::default());
        for (i, tpl) in TEMPLATES.iter().enumerate() {
            app.ui.home = Some(Default::default());
            open(&mut app, tpl);
            assert!(app.ui.home.is_none(), "{}", tpl.id);
            assert_eq!(app.session.documents().len(), i + 1);
            let title = app.session.active().unwrap().doc.title.clone();
            if tpl.preset.is_some() {
                assert!(title.starts_with("Untitled"), "{}: {title}", tpl.id);
            } else {
                assert_eq!(title, label(tpl.id));
            }
        }
        let ctx = egui::Context::default();
        let tpl = TEMPLATES.first().unwrap();
        let page = Rect::from_min_size(pos2(10.0, 20.0), vec2(90.0, 90.0));
        let a = thumbnail_shapes(&ctx, tpl, page);
        let b = thumbnail_shapes(&ctx, tpl, page.translate(vec2(5.0, 0.0)));
        assert!(!a.is_empty() && a.len() == b.len());
        assert_eq!(a[0].visual_bounding_rect().translate(vec2(5.0, 0.0)), b[0].visual_bounding_rect());
    }
}
