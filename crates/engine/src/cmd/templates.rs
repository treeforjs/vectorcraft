//! Starter templates for everyday graphics (`file.newFromTemplate {id}`, `file.templates`): a
//! social post, a story, a slide graphic, a banner, a flyer, a logo, a flowchart and blank pages
//! in common sizes. Each one opens as a new document at the right page size with a starter layout
//! of real, editable objects (shapes, text boxes, type inside shapes, arrows) in the quick styles
//! and a colour theme (see [`super::quick`]), so `quick.theme` recolours the whole layout.
//!
//! The layouts are static descriptions ([`TEMPLATES`]), in fractions of the page: the document is
//! built from them only when one is opened, and Home paints its thumbnails from the same
//! description without building anything.

use std::collections::BTreeMap;

use serde_json::{Value, json};
use vectorcraft_color::Paint;
use vectorcraft_doc::{Document, Justify, NodeId, NodeKind};
use vectorcraft_geom::{Affine, Point, Rect};

use super::quick::{self, Theme};
use super::*;

/// The colour of a text box, by the theme's role.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ink {
    Main,
    Accent,
    Dark,
    Light,
}

impl Ink {
    /// Index in [`quick::theme_colors`].
    pub fn index(self) -> usize {
        match self {
            Ink::Main => 0,
            Ink::Accent => 2,
            Ink::Dark => 3,
            Ink::Light => 4,
        }
    }
}

/// One object of a starter layout. Positions are fractions of the page (0 = left/top edge,
/// 1 = right/bottom edge); font sizes are fractions of the page's shorter side.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Part {
    /// A quick shape (`insert.shape` id) filling `rect` (x0, y0, x1, y1) in quick style `style`
    /// (or `paper`: the theme's light colour, no stroke), with `text` inside it at `size`.
    Shape { shape: &'static str, rect: [f64; 4], style: &'static str, text: Option<&'static str>, size: f64, name: &'static str },
    /// A text box: its left, top and width, font size, colour, bold, and centred or left-aligned.
    Text { x: f64, y: f64, w: f64, size: f64, ink: Ink, bold: bool, center: bool, text: &'static str },
    /// An arrow from one point to another, in the theme's dark colour.
    Arrow { from: [f64; 2], to: [f64; 2] },
}

/// A starter template.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Template {
    /// Its id for `file.newFromTemplate`.
    pub id: &'static str,
    /// Its name (English; the UI translates it), also the new document's title.
    pub label: &'static str,
    /// The page size in points, and the units the document shows.
    pub width: f64,
    pub height: f64,
    pub units: &'static str,
    /// A blank page: a New Document preset of that name, no layout.
    pub preset: Option<&'static str>,
    /// The colour theme ([`quick::THEMES`] id).
    pub theme: &'static str,
    pub parts: &'static [Part],
}

const fn shape(shape: &'static str, rect: [f64; 4], style: &'static str, name: &'static str) -> Part {
    Part::Shape { shape, rect, style, text: None, size: 0.0, name }
}

const fn labelled(shape: &'static str, rect: [f64; 4], style: &'static str, text: &'static str, size: f64) -> Part {
    Part::Shape { shape, rect, style, text: Some(text), size, name: "" }
}

const fn text(x: f64, y: f64, w: f64, size: f64, ink: Ink, bold: bool, text: &'static str) -> Part {
    Part::Text { x, y, w, size, ink, bold, center: false, text }
}

const fn centred(x: f64, y: f64, w: f64, size: f64, ink: Ink, bold: bool, text: &'static str) -> Part {
    Part::Text { x, y, w, size, ink, bold, center: true, text }
}

const fn arrow(from: [f64; 2], to: [f64; 2]) -> Part {
    Part::Arrow { from, to }
}

const PAGE: [f64; 4] = [0.0, 0.0, 1.0, 1.0];

/// A blank page: New Document's preset `preset` (also its label).
const fn blank(id: &'static str, preset: &'static str, width: f64, height: f64, units: &'static str) -> Template {
    Template { id, label: preset, width, height, units, preset: Some(preset), theme: "ocean", parts: &[] }
}

/// The templates: the designed ones first, then the blank pages.
pub const TEMPLATES: &[Template] = &[
    Template {
        id: "social-post",
        label: "Social post",
        width: 1080.0,
        height: 1080.0,
        units: "px",
        preset: None,
        theme: "sunset",
        parts: &[
            shape("rectangle", PAGE, "solid", "Background"),
            shape("ellipse", [0.62, 0.08, 0.92, 0.38], "accent", "Circle"),
            text(0.08, 0.44, 0.84, 0.11, Ink::Light, true, "Your headline"),
            text(0.08, 0.62, 0.72, 0.042, Ink::Light, false, "Add a short line that says why it matters."),
            labelled("roundedRectangle", [0.08, 0.80, 0.42, 0.90], "accent", "Learn more", 0.036),
        ],
    },
    Template {
        id: "story",
        label: "Story",
        width: 1080.0,
        height: 1920.0,
        units: "px",
        preset: None,
        theme: "berry",
        parts: &[
            shape("rectangle", PAGE, "paper", "Background"),
            labelled("rectangle", [0.0, 0.0, 1.0, 0.52], "solid", "Your picture here", 0.05),
            text(0.08, 0.57, 0.84, 0.12, Ink::Dark, true, "Your headline"),
            text(0.08, 0.69, 0.84, 0.05, Ink::Dark, false, "A line or two about it."),
            labelled("roundedRectangle", [0.08, 0.85, 0.92, 0.92], "accent", "Swipe up", 0.05),
        ],
    },
    Template {
        id: "slide",
        label: "Slide graphic",
        width: 1920.0,
        height: 1080.0,
        units: "px",
        preset: None,
        theme: "ocean",
        parts: &[
            shape("rectangle", PAGE, "paper", "Background"),
            shape("rectangle", [0.0, 0.0, 0.025, 1.0], "solid", "Accent bar"),
            text(0.08, 0.11, 0.84, 0.09, Ink::Dark, true, "Your headline"),
            text(0.08, 0.27, 0.6, 0.045, Ink::Main, false, "Your main point, in one short sentence"),
            labelled("roundedRectangle", [0.08, 0.46, 0.34, 0.84], "soft", "First point", 0.042),
            labelled("roundedRectangle", [0.37, 0.46, 0.63, 0.84], "soft", "Second point", 0.042),
            labelled("roundedRectangle", [0.66, 0.46, 0.92, 0.84], "soft", "Third point", 0.042),
        ],
    },
    Template {
        id: "banner",
        label: "Banner",
        width: 1500.0,
        height: 500.0,
        units: "px",
        preset: None,
        theme: "slate",
        parts: &[
            shape("rectangle", PAGE, "dark", "Background"),
            shape("ellipse", [0.74, 0.2, 0.94, 0.8], "accent", "Circle"),
            text(0.06, 0.16, 0.62, 0.2, Ink::Light, true, "Your headline"),
            text(0.06, 0.46, 0.62, 0.08, Ink::Light, false, "A short tagline goes here"),
            labelled("roundedRectangle", [0.06, 0.66, 0.3, 0.84], "solid", "yourwebsite.com", 0.06),
        ],
    },
    Template {
        id: "flyer",
        label: "Flyer",
        width: 612.0,
        height: 792.0,
        units: "in",
        preset: None,
        theme: "forest",
        parts: &[
            shape("rectangle", PAGE, "paper", "Background"),
            shape("rectangle", [0.0, 0.0, 1.0, 0.42], "solid", "Header"),
            centred(0.08, 0.11, 0.84, 0.12, Ink::Light, true, "Your headline"),
            centred(0.08, 0.29, 0.84, 0.045, Ink::Light, false, "Date · Time · Place"),
            centred(0.12, 0.50, 0.76, 0.034, Ink::Dark, false, "Say what is happening and why people should come. Replace this text with your own."),
            labelled("roundedRectangle", [0.28, 0.66, 0.72, 0.74], "accent", "Your call to action", 0.034),
            centred(0.08, 0.91, 0.84, 0.024, Ink::Dark, false, "your@email.com · yourwebsite.com"),
        ],
    },
    Template {
        id: "logo",
        label: "Logo / icon",
        width: 512.0,
        height: 512.0,
        units: "px",
        preset: None,
        theme: "citrus",
        parts: &[
            shape("roundedRectangle", [0.08, 0.08, 0.92, 0.92], "dark", "Badge"),
            shape("star", [0.31, 0.16, 0.69, 0.54], "solid", "Mark"),
            centred(0.1, 0.61, 0.8, 0.12, Ink::Light, true, "Your name"),
        ],
    },
    Template {
        id: "diagram",
        label: "Diagram",
        width: 1600.0,
        height: 900.0,
        units: "px",
        preset: None,
        theme: "ocean",
        parts: &[
            text(0.06, 0.07, 0.88, 0.07, Ink::Dark, true, "Your process"),
            labelled("roundedRectangle", [0.04, 0.41, 0.18, 0.61], "solid", "Start", 0.04),
            labelled("rectangle", [0.26, 0.40, 0.44, 0.62], "soft", "First step", 0.04),
            labelled("diamond", [0.52, 0.31, 0.70, 0.71], "accent", "Yes or no?", 0.035),
            labelled("rectangle", [0.78, 0.40, 0.96, 0.62], "soft", "Next step", 0.04),
            labelled("roundedRectangle", [0.52, 0.80, 0.70, 0.95], "outline", "Try again", 0.035),
            arrow([0.18, 0.51], [0.26, 0.51]),
            arrow([0.44, 0.51], [0.52, 0.51]),
            arrow([0.70, 0.51], [0.78, 0.51]),
            arrow([0.61, 0.71], [0.61, 0.80]),
            text(0.715, 0.44, 0.06, 0.03, Ink::Dark, false, "Yes"),
            text(0.625, 0.735, 0.06, 0.03, Ink::Dark, false, "No"),
        ],
    },
    blank("blank-letter", "Letter", 612.0, 792.0, "in"),
    blank("blank-a4", "A4", 595.28, 841.89, "mm"),
    blank("blank-widescreen", "Web 1920×1080", 1920.0, 1080.0, "px"),
    blank("blank-square", "Social Square Post 1080×1080", 1080.0, 1080.0, "px"),
    blank("blank-phone", "Phone 390×844", 390.0, 844.0, "px"),
    blank("blank-postcard", "Postcard", 288.0, 432.0, "in"),
];

/// The template called `id`.
pub fn find(id: &str) -> Option<&'static Template> {
    TEMPLATES.iter().find(|t| t.id == id)
}

/// Every placeholder text the templates use (English), for translators and the UI.
pub fn texts() -> Vec<&'static str> {
    let mut out = vec![];
    for t in TEMPLATES {
        for p in t.parts {
            let s = match p {
                Part::Shape { text: Some(s), .. } | Part::Text { text: s, .. } => *s,
                _ => continue,
            };
            if !out.contains(&s) {
                out.push(s);
            }
        }
    }
    out
}

pub fn specs() -> Vec<CommandSpec> {
    vec![cmd!(
        query "file.templates",
        "Templates",
        [],
        None,
        "{} the starter templates `file.newFromTemplate {id}` opens → {templates: [{id, label, width, height (pt), units, blank: bool, theme, texts: [the placeholder texts it uses]}]}",
        always,
        list
    )]
}

fn list(_: &mut Session, _: &Value) -> Result<Value> {
    let templates: Vec<Value> = TEMPLATES
        .iter()
        .map(|t| {
            let texts: Vec<&str> = texts().into_iter().filter(|s| t.parts.iter().any(|p| part_text(p) == Some(*s))).collect();
            json!({"id": t.id, "label": t.label, "width": t.width, "height": t.height, "units": t.units, "blank": t.preset.is_some(), "theme": t.theme, "texts": texts})
        })
        .collect();
    Ok(json!({ "templates": templates }))
}

fn part_text(p: &Part) -> Option<&'static str> {
    match p {
        Part::Shape { text, .. } => *text,
        Part::Text { text, .. } => Some(text),
        Part::Arrow { .. } => None,
    }
}

const C: &str = "file.newFromTemplate";

/// `file.newFromTemplate {id, title?, texts?}`: open template `id` as a new untitled document,
/// titled `title` (default: the template's name), its placeholder texts replaced by `texts`
/// ({English: shown}, how the UI shows them in its language). The new document has no undo
/// history (undo can't take the layout apart into a half-built page) and is unmodified.
pub(crate) fn open(s: &mut Session, p: &Value) -> Result<Value> {
    let id = match p.get("id") {
        Some(Value::String(id)) => id.as_str(),
        _ => return Err(bad(C, "id must be a template id (see file.templates)")),
    };
    let names = || TEMPLATES.iter().map(|t| t.id).collect::<Vec<_>>().join(", ");
    let t = find(id).ok_or_else(|| bad(C, format!("no template `{id}` (templates: {})", names())))?;
    let given = match p.get("title").filter(|v| !v.is_null()) {
        Some(v) => Some(
            v.as_str()
                .map(str::trim)
                .filter(|s| !s.is_empty() && s.chars().count() <= 256)
                .ok_or_else(|| bad(C, "title must be 1–256 characters"))?,
        ),
        None => None,
    };
    let texts = texts_param(p)?;
    if let Some(preset) = t.preset {
        // A blank page is New Document's preset, untitled unless named (it goes to Recent as
        // File › New does).
        let r = s.execute("file.new", &json!({ "preset": preset, "title": given }))?;
        let title = s.doc()?.doc.title.clone();
        return Ok(json!({ "index": r["index"], "id": t.id, "title": title }));
    }
    let title = given.unwrap_or(t.label);
    let mut doc = build(s, t, &texts)?;
    doc.title = title.to_string();
    let i = s.add_document(doc, None);
    Ok(json!({ "index": i, "id": t.id, "title": title }))
}

/// `texts: {English: shown}`, checked.
fn texts_param(p: &Value) -> Result<BTreeMap<String, String>> {
    let Some(v) = p.get("texts").filter(|v| !v.is_null()) else { return Ok(BTreeMap::new()) };
    let o = v.as_object().ok_or_else(|| bad(C, "texts must be an object {English text: replacement}"))?;
    let mut out = BTreeMap::new();
    for (k, v) in o.iter().take(256) {
        let v = v.as_str().ok_or_else(|| bad(C, "texts' values must be strings"))?;
        if v.chars().count() > 1000 {
            return Err(bad(C, "a text may be at most 1000 characters"));
        }
        out.insert(k.clone(), v.to_string());
    }
    Ok(out)
}

/// Build template `t`'s document in a scratch session (the user's session, its defaults and its
/// documents are untouched) with the quick-graphics commands, so the art is exactly what the
/// insert bar makes.
fn build(s: &Session, t: &Template, texts: &BTreeMap<String, String>) -> Result<Document> {
    let mut b = Session::new();
    // New type follows the interface language (Japanese defaults for a Japanese UI).
    b.ui_language = s.ui_language.clone();
    b.execute("file.new", &json!({ "width": t.width, "height": t.height, "units": t.units }))?;
    b.execute("quick.theme", &json!({ "theme": t.theme }))?;
    let theme = quick::theme(t.theme).ok_or_else(|| bad(C, format!("template {}: unknown theme", t.id)))?;
    let colors = quick::theme_colors(theme);
    let (w, h) = (t.width, t.height);
    let side = w.min(h);
    let at = |x: f64, y: f64| Point::new(x * w, y * h);
    let say = |en: &'static str| texts.get(en).cloned().unwrap_or_else(|| en.to_string());
    let id_of = |r: &Value| r.get("id").and_then(Value::as_u64).map(NodeId).ok_or_else(|| bad(C, "a template part made no object"));
    for part in t.parts {
        match *part {
            Part::Shape { shape, rect, style, text, size, name } => {
                let r = Rect::from_points(at(rect[0], rect[1]), at(rect[2], rect[3]));
                let made =
                    b.execute("insert.shape", &json!({ "shape": shape, "center": [r.center().x, r.center().y], "size": [r.width(), r.height()] }))?;
                let id = id_of(&made)?;
                paint(&mut b, id, style, theme)?;
                if !name.is_empty() {
                    rename(&mut b, id, name)?;
                }
                if let Some(en) = text {
                    b.execute("insert.textInShape", &json!({ "id": id.0, "text": say(en), "size": (size * side).round().max(1.0) }))?;
                }
            }
            Part::Text { x, y, w: tw, size, ink, bold, center, text } => {
                let pt = (size * side).round().max(1.0);
                let width = tw * w;
                let c = at(x, y) + vectorcraft_geom::Vec2::new(width / 2.0, pt * 0.8);
                let made = b.execute("insert.textBox", &json!({ "text": say(text), "center": [c.x, c.y], "width": width, "size": pt }))?;
                let id = id_of(&made)?;
                let color = colors.get(ink.index()).copied().unwrap_or(vectorcraft_color::Color::BLACK);
                b.edit("Text", |d, _| {
                    let Some(n) = d.node_mut(id) else { return Ok(()) };
                    let NodeKind::Text(t) = &mut n.kind else { return Ok(()) };
                    let justify = if center { Justify::Center } else { Justify::Left };
                    t.para.justify = justify;
                    for p in &mut t.paras {
                        p.justify = justify;
                    }
                    for r in &mut t.runs {
                        r.style.fill = Paint::solid(color);
                        // The bundled family has a bold face; other families keep their style.
                        if bold && r.style.font_family == "Source Sans 3" {
                            r.style.font_style = "Bold".into();
                        }
                    }
                    super::typecmd::refresh_bounds(t);
                    Ok(())
                })?;
            }
            Part::Arrow { from, to } => {
                let (a, z) = (at(from[0], from[1]), at(to[0], to[1]));
                let len = (z - a).hypot();
                if !(len.is_finite() && len > 0.5) {
                    continue;
                }
                let mid = a.midpoint(z);
                let made = b.execute("insert.shape", &json!({ "shape": "arrowLine", "center": [mid.x, mid.y], "size": [len, 0.0] }))?;
                let id = id_of(&made)?;
                paint(&mut b, id, "dark", theme)?;
                let angle = (z.y - a.y).atan2(z.x - a.x);
                if angle.abs() > 1e-9 {
                    b.edit("Rotate", |d, _| {
                        if let Some(n) = d.node_mut(id) {
                            n.transform(Affine::rotate_about(angle, mid), true);
                        }
                        Ok(())
                    })?;
                }
            }
        }
    }
    let st = b.doc()?;
    let mut doc = (*st.doc).clone();
    // New inserts take the first look, whatever the layout used last.
    doc.unknown.insert(quick::STYLE_KEY.into(), json!("solid"));
    Ok(doc)
}

/// Give `id` quick style `style` (`paper`: the theme's light colour alone).
fn paint(b: &mut Session, id: NodeId, style: &str, theme: &Theme) -> Result<()> {
    if style == "paper" {
        b.execute("quick.style", &json!({ "style": "solid", "ids": [id.0] }))?;
        let light = quick::theme_colors(theme)[4];
        b.edit("Paper", |d, _| {
            if let Some(n) = d.node_mut(id) {
                n.appearance.set_fill(Paint::solid(light));
            }
            Ok(())
        })?;
        return Ok(());
    }
    b.execute("quick.style", &json!({ "style": style, "ids": [id.0] }))?;
    Ok(())
}

fn rename(b: &mut Session, id: NodeId, name: &str) -> Result<()> {
    b.edit("Name", |d, _| {
        if let Some(n) = d.node_mut(id) {
            n.name = Some(name.to_string());
        }
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn objects(d: &Document) -> Vec<vectorcraft_doc::Node> {
        let mut out = vec![];
        d.walk(|n| out.push(n.clone()));
        out
    }

    #[test]
    fn every_template_opens_at_its_size_with_editable_objects() {
        for t in TEMPLATES {
            let mut s = Session::new();
            let r = s.execute("file.newFromTemplate", &json!({ "id": t.id })).unwrap();
            assert_eq!(r["id"], t.id);
            let st = s.doc().unwrap();
            let board = st.doc.artboards.first().unwrap().rect;
            assert!((board.width() - t.width).abs() < 0.01 && (board.height() - t.height).abs() < 0.01, "{}: {board:?}", t.id);
            assert_eq!(st.history.undo.len(), 0, "{}: opens with no history", t.id);
            assert!(!st.is_dirty(), "{}: opens unmodified", t.id);
            let all = objects(&st.doc);
            let texts = all.iter().filter(|n| matches!(n.kind, NodeKind::Text(_))).count();
            let paths = all.iter().filter(|n| matches!(n.kind, NodeKind::Path { .. })).count();
            if t.preset.is_some() {
                assert_eq!(texts + paths, 0, "{}: a blank page", t.id);
                assert!(st.doc.title.starts_with("Untitled"), "{}: {}", t.id, st.doc.title);
                continue;
            }
            assert_eq!(st.doc.title, t.label);
            assert!(texts >= 1 && paths >= 1, "{}: {texts} texts, {paths} paths", t.id);
            // Every object is editable: none locked or hidden, every text has its words.
            assert!(all.iter().all(|n| !n.locked && n.visible), "{}", t.id);
            for n in &all {
                if let NodeKind::Text(tx) = &n.kind {
                    assert!(!tx.runs.iter().map(|r| r.text.as_str()).collect::<String>().trim().is_empty(), "{}: empty text", t.id);
                }
            }
            // And the art sits on the page.
            let bounds = all.iter().filter_map(|n| n.geometric_bounds()).fold(None::<Rect>, |a, b| Some(a.map_or(b, |a| a.union(b))));
            let b = bounds.unwrap();
            assert!(b.x0 > -t.width && b.x1 < 2.0 * t.width && b.y0 > -t.height && b.y1 < 2.0 * t.height, "{}: {b:?}", t.id);
            // The theme recolours the layout.
            assert_eq!(quick::theme_of(&st.doc).id, t.theme);
            let changed = s.execute("quick.theme", &json!({ "theme": if t.theme == "slate" { "berry" } else { "slate" } })).unwrap();
            assert!(changed["changed"].as_u64().unwrap() > 0, "{}", t.id);
        }
    }

    #[test]
    fn a_template_is_one_new_document_and_undo_keeps_it() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"width": 100, "height": 100})).unwrap();
        s.execute("shape.rectangle", &json!({"x": 0, "y": 0, "width": 10, "height": 10})).unwrap();
        s.execute("file.newFromTemplate", &json!({ "id": "diagram" })).unwrap();
        assert_eq!(s.docs.len(), 2);
        let before = objects(&s.doc().unwrap().doc).len();
        assert!(s.execute("edit.undo", &json!({})).is_err() || objects(&s.doc().unwrap().doc).len() == before);
        assert_eq!(objects(&s.doc().unwrap().doc).len(), before);
        // The other document and the session's defaults are untouched.
        assert_eq!(s.docs[0].history.undo.len(), 1);
        assert_eq!(s.draw_mode, crate::DrawMode::Normal);
        // Editing it works and undoes as usual.
        let st = s.doc().unwrap();
        let first = objects(&st.doc).into_iter().find(|n| matches!(n.kind, NodeKind::Text(_))).unwrap().id;
        s.execute("select.set", &json!({"ids": [first.0]})).unwrap();
        s.execute("edit.clear", &json!({})).unwrap();
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(objects(&s.doc().unwrap().doc).len(), before);
    }

    #[test]
    fn title_and_texts_follow_the_params() {
        let mut s = Session::new();
        let r = s
            .execute("file.newFromTemplate", &json!({ "id": "social-post", "title": "Beitrag", "texts": {"Your headline": "Ihre Überschrift"} }))
            .unwrap();
        assert_eq!(r["title"], "Beitrag");
        let st = s.doc().unwrap();
        assert_eq!(st.doc.title, "Beitrag");
        let words: Vec<String> = objects(&st.doc)
            .iter()
            .filter_map(|n| match &n.kind {
                NodeKind::Text(t) => Some(t.runs.iter().map(|r| r.text.as_str()).collect()),
                _ => None,
            })
            .collect();
        assert!(words.iter().any(|w| w == "Ihre Überschrift"), "{words:?}");
        assert!(words.iter().any(|w| w == "Learn more"), "{words:?}");
    }

    #[test]
    fn bad_ids_and_params_are_errors_not_documents() {
        let mut s = Session::new();
        for p in [
            json!({}),
            json!({ "id": "" }),
            json!({ "id": "nope" }),
            json!({ "id": 3 }),
            json!({ "id": null }),
            json!({ "id": ["slide"] }),
            json!({ "id": "slide", "texts": "x" }),
            json!({ "id": "slide", "texts": {"Your headline": 5} }),
            json!({ "id": "slide", "title": "" }),
            json!({ "id": "slide", "title": 7 }),
        ] {
            assert!(s.execute("file.newFromTemplate", &p).is_err(), "{p}");
        }
        assert!(s.docs.is_empty());
    }

    #[test]
    fn the_list_names_every_template() {
        let mut s = Session::new();
        let l = s.execute("file.templates", &json!({})).unwrap();
        let ids: Vec<&str> = l["templates"].as_array().unwrap().iter().map(|t| t["id"].as_str().unwrap()).collect();
        assert_eq!(ids, TEMPLATES.iter().map(|t| t.id).collect::<Vec<_>>());
        assert!(l["templates"][0]["texts"].as_array().unwrap().iter().any(|t| t == "Your headline"));
        // The arrows keep their heads after their look.
        s.execute("file.newFromTemplate", &json!({ "id": "diagram" })).unwrap();
        let arrows = objects(&s.doc().unwrap().doc).into_iter().filter(|n| n.appearance.stroke().is_some_and(|st| st.end_arrow.is_some())).count();
        assert_eq!(arrows, 4);
    }
}
