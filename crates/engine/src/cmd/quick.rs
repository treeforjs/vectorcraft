//! Quick graphics, for everyday work art (slide pictures, social posts, diagrams, callouts) made in
//! seconds: ready-made shapes, text boxes and icons dropped at a page's centre (`insert.*`), type
//! inside a shape, one-click looks made from the document's colour theme (`quick.style`), a theme
//! picker that recolours that art consistently (`quick.theme`), and a PNG of the selection or an
//! artboard for slides and chats (`quick.png`).
//!
//! The theme and the look new inserts get are kept in the document (`Document::unknown`, keys
//! [`THEME_KEY`] and [`STYLE_KEY`]), so they travel with the file and undo with the edits.

use std::sync::Arc;

use serde_json::{Value, json};
use vectorcraft_color::{Color, Paint};
use vectorcraft_doc::{Appearance, Document, Effect, LiveShape, Node, NodeId, NodeKind, TextKind, TextObject};
use vectorcraft_geom::{Affine, BezPath, FillRule, PathData, Point, Rect, shapes};

use super::newart::NewArt;
use super::*;

/// `Document::unknown` key of the document's colour theme (a [`THEMES`] id).
pub const THEME_KEY: &str = "quickTheme";
/// `Document::unknown` key of the look new inserts get (a [`STYLES`] id).
pub const STYLE_KEY: &str = "quickStyle";

/// A colour theme: five colours by role.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Theme {
    pub id: &'static str,
    pub label: &'static str,
    /// Main, second, accent, dark (ink) and light (paper), as `#rrggbb`.
    pub colors: [&'static str; 5],
}

const OCEAN: Theme = Theme { id: "ocean", label: "Ocean", colors: ["#1f6feb", "#13a8a0", "#f2a20c", "#13213a", "#f3f7fc"] };

/// The built-in themes (original palettes), the default first.
pub const THEMES: &[Theme] = &[
    OCEAN,
    Theme { id: "sunset", label: "Sunset", colors: ["#e4572e", "#f3a712", "#7d3c98", "#2b1d16", "#fff6ec"] },
    Theme { id: "forest", label: "Forest", colors: ["#2f7d4f", "#8fb996", "#d9a228", "#1b2c21", "#f3f6f0"] },
    Theme { id: "berry", label: "Berry", colors: ["#b5306f", "#6a48c2", "#ef8a3c", "#2a1830", "#fcf1f6"] },
    Theme { id: "slate", label: "Slate", colors: ["#4a5a70", "#97a6b9", "#1597d4", "#141a24", "#f6f7f9"] },
    Theme { id: "citrus", label: "Citrus", colors: ["#e3a90b", "#2aa772", "#ee5d5a", "#23233a", "#fffbef"] },
];

/// A one-click look.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct QuickStyle {
    pub id: &'static str,
    pub label: &'static str,
}

/// The quick styles, the default first.
pub const STYLES: &[QuickStyle] = &[
    QuickStyle { id: "solid", label: "Solid" },
    QuickStyle { id: "outline", label: "Outline" },
    QuickStyle { id: "soft", label: "Soft" },
    QuickStyle { id: "accent", label: "Accent" },
    QuickStyle { id: "dashed", label: "Dashed" },
    QuickStyle { id: "shadow", label: "Shadow" },
    QuickStyle { id: "dark", label: "Dark" },
    QuickStyle { id: "alternate", label: "Alternate" },
];

/// A ready-made shape: id, label, width and height relative to the insert size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct QuickShape {
    pub id: &'static str,
    pub label: &'static str,
    pub w: f64,
    pub h: f64,
}

pub const SHAPES: &[QuickShape] = &[
    QuickShape { id: "rectangle", label: "Rectangle", w: 1.5, h: 1.0 },
    QuickShape { id: "roundedRectangle", label: "Rounded Rectangle", w: 1.5, h: 1.0 },
    QuickShape { id: "ellipse", label: "Ellipse", w: 1.0, h: 1.0 },
    QuickShape { id: "triangle", label: "Triangle", w: 1.1, h: 1.0 },
    QuickShape { id: "diamond", label: "Diamond", w: 1.0, h: 1.0 },
    QuickShape { id: "star", label: "Star", w: 1.0, h: 1.0 },
    QuickShape { id: "arrowRight", label: "Arrow", w: 1.6, h: 0.8 },
    QuickShape { id: "arrowDouble", label: "Double Arrow", w: 1.8, h: 0.7 },
    QuickShape { id: "chevron", label: "Chevron", w: 1.4, h: 0.7 },
    QuickShape { id: "line", label: "Line", w: 1.6, h: 0.0 },
    QuickShape { id: "arrowLine", label: "Line Arrow", w: 1.6, h: 0.0 },
    QuickShape { id: "speechBubble", label: "Speech Bubble", w: 1.5, h: 1.0 },
    QuickShape { id: "callout", label: "Callout", w: 1.5, h: 1.0 },
];

/// A built-in icon: id, label and its SVG (24 × 24, stroked). Lucide icons come from the repo's
/// already-attributed `assets/icons` (ISC); `check`, `heart` and `user` are VectorCraft's own.
pub const ICONS: &[(&str, &str, &str)] = &[
    (
        "check",
        "Check",
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 24 24\" width=\"24\" height=\"24\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"2\" stroke-linecap=\"round\" stroke-linejoin=\"round\"><path d=\"M4.5 12.5l5 5L19.5 7\"/></svg>",
    ),
    (
        "heart",
        "Heart",
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 24 24\" width=\"24\" height=\"24\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"2\" stroke-linecap=\"round\" stroke-linejoin=\"round\"><path d=\"M12 20C12 20 3.5 14.6 3.5 8.9C3.5 6.2 5.5 4.2 8 4.2C9.7 4.2 11.2 5.1 12 6.6C12.8 5.1 14.3 4.2 16 4.2C18.5 4.2 20.5 6.2 20.5 8.9C20.5 14.6 12 20 12 20Z\"/></svg>",
    ),
    (
        "user",
        "Person",
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 24 24\" width=\"24\" height=\"24\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"2\" stroke-linecap=\"round\" stroke-linejoin=\"round\"><circle cx=\"12\" cy=\"8\" r=\"4\"/><path d=\"M4.5 20.5C4.5 16.6 7.9 14 12 14C16.1 14 19.5 16.6 19.5 20.5\"/></svg>",
    ),
    ("star", "Star", include_str!("../../../../assets/icons/star.svg")),
    ("sparkles", "Sparkles", include_str!("../../../../assets/icons/sparkles.svg")),
    ("sun", "Sun", include_str!("../../../../assets/icons/sun.svg")),
    ("cloud", "Cloud", include_str!("../../../../assets/icons/cloud.svg")),
    ("globe", "Globe", include_str!("../../../../assets/icons/globe.svg")),
    ("house", "House", include_str!("../../../../assets/icons/house.svg")),
    ("info", "Info", include_str!("../../../../assets/icons/info.svg")),
    ("lock", "Lock", include_str!("../../../../assets/icons/lock.svg")),
    ("map", "Map", include_str!("../../../../assets/icons/map.svg")),
    ("message-circle", "Message", include_str!("../../../../assets/icons/message-circle.svg")),
    ("monitor", "Monitor", include_str!("../../../../assets/icons/monitor.svg")),
    ("search", "Search", include_str!("../../../../assets/icons/search.svg")),
    ("settings", "Settings", include_str!("../../../../assets/icons/settings.svg")),
    ("chart-column", "Chart", include_str!("../../../../assets/icons/chart-column.svg")),
    ("pin", "Pin", include_str!("../../../../assets/icons/pin.svg")),
    ("share-2", "Share", include_str!("../../../../assets/icons/share-2.svg")),
    ("image", "Picture", include_str!("../../../../assets/icons/image.svg")),
];

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            query "quick.list",
            "Quick Graphics",
            [],
            None,
            "{} what the insert bar offers → {shapes: [{id, label}], icons: [{id, label}], styles: [{id, label}], themes: [{id, label, colors}], theme: the document's theme id, style: the look new inserts get}",
            always,
            list
        ),
        cmd!(
            "insert.shape",
            "Insert Shape",
            [],
            None,
            "{shape: rectangle|roundedRectangle|ellipse|triangle|diamond|star|arrowRight|arrowDouble|chevron|line|arrowLine|speechBubble|callout, center?: [x, y] (default: the artboard's centre), artboard?: index (default: the one under center, else the first), size?: [w, h] (default: in proportion to the artboard)} add the shape with the document's quick style, selected, as one undo step → {id}",
            has_doc,
            insert_shape
        ),
        cmd!(
            "insert.textBox",
            "Insert Text Box",
            [],
            None,
            "{text?: (default \"Your text here\"), center?: [x, y], artboard?, width?: pt, size?: font size pt} add centred area type in the theme's colours, selected, as one undo step → {id}",
            has_doc,
            insert_text_box
        ),
        cmd!(
            "insert.icon",
            "Insert Icon",
            [],
            None,
            "{icon: an id from quick.list (star, check, heart, user, globe…), center?: [x, y], artboard?, size?: pt} add the icon as a group of stroked paths in the theme's colour, selected, as one undo step → {id}",
            has_doc,
            insert_icon
        ),
        cmd!(
            "insert.textInShape",
            "Add Text to Shape",
            [],
            None,
            "{id?: a closed path (default: the selection's first object), text?: \"\", size?: pt} put centred area type inside the shape, grouped with it so they move and resize together, as one undo step; a shape that already has text answers with that text → {id: the text, group, created}",
            has_doc,
            text_in_shape
        ),
        cmd!(
            "quick.style",
            "Quick Style",
            [],
            None,
            "{style: solid|outline|soft|accent|dashed|shadow|dark|alternate, ids?} give the objects (groups: their contents) that look in the document's theme colours, and make it the look new inserts get, as one undo step → {changed}",
            has_doc,
            quick_style
        ),
        cmd!(
            "quick.theme",
            "Color Theme",
            [],
            None,
            "{theme: ocean|sunset|forest|berry|slate|citrus} make it the document's colour theme and recolour the art in the old theme's colours to the new one's, as one undo step → {theme, changed}",
            has_doc,
            quick_theme
        ),
        cmd!(
            query "quick.png",
            "Picture as PNG",
            [],
            None,
            "{target?: selection|artboard (default: the selection, else the artboard), artboard?: index, scale?: pixels per point (default 2), background?: transparent|white|#rrggbb (default: transparent for the selection, white for an artboard)} → {dataBase64, width, height}",
            has_doc,
            png
        ),
    ]
}

// ---------- themes and looks ----------

/// The theme called `id`.
pub fn theme(id: &str) -> Option<&'static Theme> {
    THEMES.iter().find(|t| t.id == id)
}

/// The document's theme (the default when it has none or an unknown one).
pub fn theme_of(d: &Document) -> &'static Theme {
    d.unknown.get(THEME_KEY).and_then(Value::as_str).and_then(theme).unwrap_or(&OCEAN)
}

/// The look new inserts get in `d`.
pub fn style_of(d: &Document) -> &'static str {
    d.unknown.get(STYLE_KEY).and_then(Value::as_str).and_then(|s| STYLES.iter().find(|q| q.id == s)).map_or("solid", |q| q.id)
}

/// A theme's colours by role.
#[derive(Clone, Copy, Debug)]
struct Palette {
    main: Color,
    second: Color,
    accent: Color,
    dark: Color,
    light: Color,
}

impl Palette {
    fn of(t: &Theme) -> Self {
        let c = |i: usize| t.colors.get(i).and_then(|h| Color::from_hex(h)).unwrap_or(Color::BLACK);
        Self { main: c(0), second: c(1), accent: c(2), dark: c(3), light: c(4) }
    }
    /// The soft tint of the main colour.
    fn tint(&self) -> Color {
        self.main.lerp(&self.light, 0.82)
    }
    /// Every colour the looks use, in a fixed order (theme changes map them one to one).
    fn all(&self) -> [Color; 6] {
        [self.main, self.second, self.accent, self.dark, self.light, self.tint()]
    }
}

/// What a quick style gives an object.
#[derive(Clone, Copy, Debug)]
struct Look {
    fill: Option<Color>,
    stroke: Option<(Color, f64)>,
    dash: bool,
    shadow: bool,
    /// Type set on its own (a text box).
    ink: Color,
}

fn look(style: &str, p: &Palette) -> Option<Look> {
    let l = |fill, stroke, ink| Look { fill, stroke, dash: false, shadow: false, ink };
    Some(match style {
        "solid" => l(Some(p.main), None, p.main),
        "outline" => l(None, Some((p.main, 3.0)), p.main),
        "soft" => l(Some(p.tint()), Some((p.main, 1.5)), p.dark),
        "accent" => l(Some(p.accent), None, p.accent),
        "dashed" => Look { dash: true, ..l(None, Some((p.dark, 2.0)), p.dark) },
        "shadow" => Look { shadow: true, ..l(Some(p.light), None, p.dark) },
        "dark" => l(Some(p.dark), None, p.dark),
        "alternate" => l(Some(p.second), None, p.second),
        _ => return None,
    })
}

/// Relative luminance (0 black … 1 white).
fn luminance(c: Color) -> f32 {
    let [r, g, b] = c.to_rgb();
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

/// Type that reads on `fill` (none: the page).
fn text_on(fill: Option<Color>, p: &Palette) -> Color {
    match fill {
        Some(f) if luminance(f) < 0.55 => p.light,
        _ => p.dark,
    }
}

fn shadow_effect(p: &Palette) -> Effect {
    Effect {
        id: "stylize.dropShadow".into(),
        params: json!({"mode": "multiply", "opacity": 35.0, "x": 0.0, "y": 4.0, "blur": 8.0, "color": p.dark.to_hex()}),
        visible: true,
    }
}

/// Give `n` the look: a closed path or compound path its fill and stroke, an open path the stroke
/// alone (in the look's main colour), type its colour.
fn apply_look(n: &mut Node, lk: &Look, p: &Palette, in_shape: bool) -> bool {
    let paint = |c: Option<Color>| c.map_or(Paint::None, Paint::solid);
    let open = match &n.kind {
        NodeKind::Path { path, guide: false, clipping: false, .. } => !path.is_closed(),
        NodeKind::Compound { .. } => false,
        NodeKind::Text(_) => {
            let ink = if in_shape { text_on(lk.fill, p) } else { lk.ink };
            return set_text_color(n, ink);
        }
        _ => return false,
    };
    let ap = &mut n.appearance;
    if open {
        let color = lk.stroke.map(|s| s.0).or(lk.fill).unwrap_or(p.main);
        let width = ap.stroke().map_or(3.0, |s| s.width).max(lk.stroke.map_or(3.0, |s| s.1));
        ap.set_fill(Paint::None);
        ap.set_stroke(Paint::solid(color));
        if let Some(st) = ap.stroke_mut() {
            st.width = width;
        }
    } else {
        ap.set_fill(paint(lk.fill));
        ap.set_stroke(paint(lk.stroke.map(|s| s.0)));
        if let Some((_, w)) = lk.stroke
            && let Some(st) = ap.stroke_mut()
        {
            st.width = w;
        }
    }
    if let Some(st) = ap.stroke_mut() {
        st.dash = lk.dash.then(|| vectorcraft_doc::Dash { pattern: vec![6.0, 4.0], offset: 0.0, align_corners: true });
    }
    ap.effects.retain(|e| e.id != "stylize.dropShadow");
    if lk.shadow {
        ap.effects.push(shadow_effect(p));
    }
    n.graphic_style = None;
    true
}

fn set_text_color(n: &mut Node, c: Color) -> bool {
    let NodeKind::Text(t) = &mut n.kind else { return false };
    for r in &mut t.runs {
        r.style.fill = Paint::solid(c);
    }
    true
}

/// The appearance a new closed (or open) shape gets from look `lk`.
fn new_appearance(lk: &Look, p: &Palette, closed: bool) -> Appearance {
    let mut n =
        Node::new(NodeId(0), NodeKind::Path { path: PathData::default(), rule: FillRule::NonZero, live: None, clipping: false, guide: false });
    n.appearance = Appearance::basic(Paint::None, Paint::None, 1.0);
    if !closed {
        // An open path's look (see apply_look) needs a path that isn't closed.
        n.kind =
            NodeKind::Path { path: line_path(Rect::new(0.0, 0.0, 1.0, 0.0)), rule: FillRule::NonZero, live: None, clipping: false, guide: false };
    } else {
        n.kind = NodeKind::Path {
            path: shapes::rectangle(Rect::new(0.0, 0.0, 1.0, 1.0)),
            rule: FillRule::NonZero,
            live: None,
            clipping: false,
            guide: false,
        };
    }
    apply_look(&mut n, lk, p, false);
    n.appearance
}

// ---------- where inserts go ----------

/// The artboard and centre an insert goes to (`center`, `artboard` params).
fn place(s: &Session, p: &Value, cmd: &str) -> Result<(Rect, Point)> {
    let d = &s.doc()?.doc;
    let center = point_param(p, "center");
    if let Some(c) = center
        && !(c.x.is_finite() && c.y.is_finite() && c.x.abs() < 1e7 && c.y.abs() < 1e7)
    {
        return Err(bad(cmd, "center must be two finite numbers"));
    }
    let index = match p.get("artboard").filter(|v| !v.is_null()) {
        Some(v) => Some(v.as_u64().ok_or_else(|| bad(cmd, "artboard must be an index"))? as usize),
        None => center.and_then(|c| d.artboard_at(c)),
    };
    let board = match index {
        Some(i) => d.artboards.get(i).map(|a| a.rect).ok_or_else(|| bad(cmd, format!("no artboard {i}")))?,
        None => d.artboards.first().map_or(Rect::new(0.0, 0.0, 612.0, 792.0), |a| a.rect),
    };
    Ok((board, center.unwrap_or(board.center())))
}

/// The insert size for `board`: about a quarter of its shorter side.
fn base_size(board: Rect) -> f64 {
    let side = board.width().min(board.height());
    if side.is_finite() { (side * 0.28).clamp(24.0, 480.0) } else { 120.0 }
}

/// `size: [w, h]` from `p`, checked.
fn size_param(p: &Value, cmd: &str) -> Result<Option<(f64, f64)>> {
    let Some(v) = p.get("size").filter(|v| !v.is_null()) else { return Ok(None) };
    let a = v.as_array().ok_or_else(|| bad(cmd, "size must be [width, height]"))?;
    let w = a.first().and_then(Value::as_f64).ok_or_else(|| bad(cmd, "size must be [width, height]"))?;
    let h = a.get(1).and_then(Value::as_f64).ok_or_else(|| bad(cmd, "size must be [width, height]"))?;
    let ok = |x: f64| x.is_finite() && (0.0..=100_000.0).contains(&x);
    if !ok(w) || !ok(h) || w < 0.5 {
        return Err(bad(cmd, "size must be finite, 0.5 to 100000 pt wide"));
    }
    Ok(Some((w, h)))
}

// ---------- shapes ----------

fn svg_path(d: &str) -> Result<PathData> {
    let bp = BezPath::from_svg(d).map_err(|e| bad("insert.shape", format!("shape path: {e}")))?;
    Ok(PathData::from_bezpath(&bp))
}

fn line_path(r: Rect) -> PathData {
    let mut bp = BezPath::new();
    bp.move_to(Point::new(r.x0, r.y0));
    bp.line_to(Point::new(r.x1, r.y0));
    PathData::from_bezpath(&bp)
}

fn polygon(pts: &[(f64, f64)]) -> PathData {
    let mut bp = BezPath::new();
    for (i, (x, y)) in pts.iter().enumerate() {
        if i == 0 {
            bp.move_to(Point::new(*x, *y));
        } else {
            bp.line_to(Point::new(*x, *y));
        }
    }
    bp.close_path();
    PathData::from_bezpath(&bp)
}

/// A rounded rectangle body `body` of corner radius `rad` with a tail along its bottom edge from
/// `tail.0` to `tail.1` (x) pointing at `tip`: a speech bubble.
fn bubble(body: Rect, rad: f64, tail: (f64, f64), tip: (f64, f64)) -> Result<PathData> {
    let (x0, y0, x1, yb) = (body.x0, body.y0, body.x1, body.y1);
    let (t0, t1) = tail;
    let k = rad * (1.0 - 0.5523);
    let d = format!(
        "M{a},{y0} H{b} C{c},{y0} {x1},{e} {x1},{f} V{g} C{x1},{h} {c},{yb} {b},{yb} H{t1} L{tx},{ty} L{t0},{yb} H{a} C{k0},{yb} {x0},{h} {x0},{g} V{f} C{x0},{e} {k0},{y0} {a},{y0} Z",
        a = x0 + rad,
        b = x1 - rad,
        c = x1 - k,
        e = y0 + k,
        f = y0 + rad,
        g = yb - rad,
        h = yb - k,
        k0 = x0 + k,
        tx = tip.0,
        ty = tip.1,
    );
    svg_path(&d)
}

/// The shape's path in `r` and the live shape it is, if one.
pub fn shape_path(id: &str, r: Rect) -> Result<(PathData, Option<LiveShape>)> {
    let (x0, y0, x1, y1) = (r.x0, r.y0, r.x1, r.y1);
    let (w, h) = (r.width(), r.height());
    let (cx, cy) = (r.center().x, r.center().y);
    let at = Affine::translate(r.origin().to_vec2());
    Ok(match id {
        "rectangle" | "roundedRectangle" => {
            let radius = if id == "rectangle" { 0.0 } else { w.min(h) * 0.18 };
            let live = LiveShape::Rectangle { w, h, radii: [radius; 4], kinds: Default::default(), xf: at };
            (live.to_path(), Some(live))
        }
        "ellipse" => {
            let live = LiveShape::Ellipse { w, h, pie: (0.0, 360.0), xf: at };
            (live.to_path(), Some(live))
        }
        "triangle" => (polygon(&[(cx, y0), (x1, y1), (x0, y1)]), None),
        "diamond" => (polygon(&[(cx, y0), (x1, cy), (cx, y1), (x0, cy)]), None),
        "star" => {
            let r1 = w.min(h) / 2.0;
            (shapes::star(r.center(), r1, r1 * 0.48, 5, 0.0), None)
        }
        "arrowRight" => {
            let head = (w * 0.4).min(h * 0.9);
            let half = h * 0.22;
            (
                polygon(&[
                    (x0, cy - half),
                    (x1 - head, cy - half),
                    (x1 - head, y0),
                    (x1, cy),
                    (x1 - head, y1),
                    (x1 - head, cy + half),
                    (x0, cy + half),
                ]),
                None,
            )
        }
        "arrowDouble" => {
            let head = (w * 0.28).min(h * 0.9);
            let half = h * 0.22;
            (
                polygon(&[
                    (x0, cy),
                    (x0 + head, y0),
                    (x0 + head, cy - half),
                    (x1 - head, cy - half),
                    (x1 - head, y0),
                    (x1, cy),
                    (x1 - head, y1),
                    (x1 - head, cy + half),
                    (x0 + head, cy + half),
                    (x0 + head, y1),
                ]),
                None,
            )
        }
        "chevron" => {
            let d = (h * 0.5).min(w * 0.4);
            (polygon(&[(x0, y0), (x1 - d, y0), (x1, cy), (x1 - d, y1), (x0, y1), (x0 + d, cy)]), None)
        }
        "line" | "arrowLine" => (line_path(Rect::new(x0, cy, x1, cy)), None),
        "speechBubble" => {
            let yb = y0 + h * 0.78;
            let rad = (w.min(yb - y0) * 0.22).max(0.0);
            (bubble(Rect::new(x0, y0, x1, yb), rad, (x0 + w * 0.2, x0 + w * 0.38), (x0 + w * 0.16, y1))?, None)
        }
        "callout" => {
            let yb = y0 + h * 0.78;
            (polygon(&[(x0, y0), (x1, y0), (x1, yb), (x0 + w * 0.4, yb), (x0 + w * 0.16, y1), (x0 + w * 0.22, yb), (x0, yb)]), None)
        }
        o => {
            let ids: Vec<&str> = SHAPES.iter().map(|s| s.id).collect();
            return Err(bad("insert.shape", format!("unknown shape `{o}` (one of {})", ids.join(", "))));
        }
    })
}

fn insert_shape(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "insert.shape";
    let id = str_param(p, "shape").ok_or_else(|| bad(C, "missing `shape` (see quick.list)"))?;
    let spec = SHAPES.iter().find(|q| q.id == id).ok_or_else(|| bad(C, format!("unknown shape `{id}` (see quick.list)")))?;
    let (board, c) = place(s, p, C)?;
    let base = base_size(board);
    let (w, h) = size_param(p, C)?.unwrap_or((base * spec.w, base * spec.h));
    let r = Rect::from_center_size(c, (w, h));
    let (mut path, live) = shape_path(id, r)?;
    // Shapes that don't fill their box (a star) are centred by their outline.
    if live.is_none()
        && let Some(b) = path.bounds()
    {
        path = path.transformed(Affine::translate(c - b.center()));
    }
    let d = &s.doc()?.doc;
    let pal = Palette::of(theme_of(d));
    let lk = look(style_of(d), &pal).ok_or_else(|| bad(C, "unknown quick style"))?;
    let closed = path.is_closed();
    let mut appearance = new_appearance(&lk, &pal, closed);
    if id == "arrowLine"
        && let Some(st) = appearance.stroke_mut()
    {
        st.end_arrow = Some(vectorcraft_doc::Arrowhead::Triangle);
    }
    let kind = NodeKind::Path { path, rule: FillRule::NonZero, live, clipping: false, guide: false };
    super::create::add_look(s, spec.label, kind, NewArt::plain(appearance), Some(spec.label.to_string()))
}

fn insert_text_box(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "insert.textBox";
    let (board, c) = place(s, p, C)?;
    let base = base_size(board);
    let size = match p.get("size").filter(|v| !v.is_null()) {
        Some(v) => v.as_f64().filter(|x| x.is_finite() && (1.0..=1296.0).contains(x)).ok_or_else(|| bad(C, "size is a font size, 1 to 1296 pt"))?,
        None => (base / 5.0).clamp(12.0, 96.0).round(),
    };
    let width = match p.get("width").filter(|v| !v.is_null()) {
        Some(v) => v.as_f64().filter(|x| x.is_finite() && (1.0..=100_000.0).contains(x)).ok_or_else(|| bad(C, "width must be 1 to 100000 pt"))?,
        None => (base * 3.0).max(size * 8.0),
    };
    let height = (size * 1.6).max(12.0);
    let text = str_param(p, "text").unwrap_or("Your text here");
    let pal = Palette::of(theme_of(&s.doc()?.doc));
    let mut style = super::create::new_type_style(s, &json!({}));
    style.size = size;
    style.fill = Paint::solid(pal.dark);
    let origin = Point::new(c.x - width / 2.0, c.y - height / 2.0);
    let mut t = TextObject::point(origin, text, style);
    t.para = super::create::new_type_para();
    t.para.justify = vectorcraft_doc::Justify::Center;
    super::create::new_type_alignment(s, p, C, &mut t)?;
    t.kind = TextKind::Area { frame: shapes::rectangle(Rect::new(0.0, 0.0, width, height)) };
    t.area.fit = vectorcraft_doc::AreaFit::AutoHeight;
    t.area.vertical_align = vectorcraft_doc::VerticalAlign::Center;
    super::typecmd::refresh_bounds(&mut t);
    super::create::add_node(s, "Text Box", NodeKind::Text(Box::new(t)), Appearance::default(), None)
}

/// Paint every stroke (and fill) of `n` and its contents `c`.
fn paint_icon(n: &mut Node, c: Color, depth: usize) {
    if depth > 32 {
        return;
    }
    if let Some(children) = n.children_mut() {
        for ch in children {
            paint_icon(Arc::make_mut(ch), c, depth + 1);
        }
        return;
    }
    if !n.appearance.fill_paint().is_none() {
        n.appearance.set_fill(Paint::solid(c));
    }
    if !n.appearance.stroke_paint().is_none() {
        n.appearance.set_stroke(Paint::solid(c));
    }
}

/// The SVG of icon `id`.
pub fn icon_svg(id: &str) -> Option<&'static str> {
    ICONS.iter().find(|i| i.0 == id).map(|i| i.2)
}

fn insert_icon(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "insert.icon";
    let id = str_param(p, "icon").ok_or_else(|| bad(C, "missing `icon` (see quick.list)"))?;
    let (_, label, svg) = ICONS.iter().find(|i| i.0 == id).copied().ok_or_else(|| bad(C, format!("unknown icon `{id}` (see quick.list)")))?;
    let (board, c) = place(s, p, C)?;
    let size = match p.get("size").filter(|v| !v.is_null()) {
        Some(v) => v.as_f64().filter(|x| x.is_finite() && (1.0..=100_000.0).contains(x)).ok_or_else(|| bad(C, "size must be 1 to 100000 pt"))?,
        None => base_size(board) * 0.8,
    };
    let icon = vectorcraft_svg::import(&svg.replace("currentColor", "#000000")).map_err(|e| bad(C, format!("icon {id}: {e}")))?;
    let frame = icon.artboards.first().map_or(Rect::new(0.0, 0.0, 24.0, 24.0), |a| a.rect);
    let k = size / frame.width().max(frame.height()).max(1.0);
    let m = Affine::translate(c.to_vec2()) * Affine::scale(k) * Affine::translate(-frame.center().to_vec2());
    let d = &s.doc()?.doc;
    let pal = Palette::of(theme_of(d));
    let lk = look(style_of(d), &pal).ok_or_else(|| bad(C, "unknown quick style"))?;
    let color = lk.stroke.map(|s| s.0).or(lk.fill).unwrap_or(pal.main);
    let color = if color == pal.light { pal.dark } else { color };
    let mut nodes: Vec<Node> = icon.layers.iter().flat_map(|l| l.children().into_iter().flatten()).map(|n| (**n).clone()).collect();
    if nodes.is_empty() {
        return Err(bad(C, format!("icon {id} is empty")));
    }
    for n in &mut nodes {
        n.transform(m, true);
        paint_icon(n, color, 0);
    }
    let parent = s.doc()?.insertion_parent();
    let id = s.edit("Icon", |d, sel| {
        let children = nodes.iter().map(|n| Arc::new(d.reid(n))).collect();
        let gid = d.alloc_id();
        let mut g = Node::new(gid, NodeKind::Group { children, clip: false });
        g.name = Some(label.to_string());
        d.insert(parent, usize::MAX, g)?;
        sel.set([gid]);
        Ok(gid)
    })?;
    Ok(json!({ "id": id.0 }))
}

/// Can type go inside `n` (a closed path with an area)?
pub fn takes_text(n: &Node) -> bool {
    vectorcraft_tools::select::takes_text(n)
}

fn text_in_shape(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "insert.textInShape";
    let id = match p.get("id").filter(|v| !v.is_null()) {
        Some(v) => checked_id(s, v, C)?,
        None => s.doc()?.selection.objects.first().copied().ok_or_else(|| bad(C, "select a shape"))?,
    };
    let d = &s.doc()?.doc;
    if let Some((group, text)) = vectorcraft_tools::select::shape_text_parts(d, id) {
        return Ok(json!({ "id": text.0, "group": group.0, "created": false }));
    }
    let node = d.node(id).cloned().ok_or(EngineError::NoNode(id))?;
    let NodeKind::Path { path, .. } = &node.kind else {
        return Err(bad(C, "text goes inside a closed shape: select a rectangle, ellipse or other closed path"));
    };
    if !takes_text(&node) {
        return Err(bad(C, "text goes inside a closed shape: select a rectangle, ellipse or other closed path"));
    }
    let b = path.bounds().ok_or_else(|| bad(C, "the shape is empty"))?;
    let pal = Palette::of(theme_of(d));
    let fill = node.appearance.fill_paint().color();
    let size = match p.get("size").filter(|v| !v.is_null()) {
        Some(v) => v.as_f64().filter(|x| x.is_finite() && (1.0..=1296.0).contains(x)).ok_or_else(|| bad(C, "size is a font size, 1 to 1296 pt"))?,
        None => (b.width().min(b.height()) / 5.0).clamp(8.0, 72.0).round(),
    };
    let mut style = super::create::new_type_style(s, &json!({}));
    style.size = size;
    style.fill = Paint::solid(text_on(fill, &pal));
    let mut t = TextObject::point(Point::ZERO, str_param(p, "text").unwrap_or(""), style);
    t.xf = Affine::IDENTITY;
    // A speech bubble's or callout's type sits in its body, above the tail.
    let bubble = SHAPES.iter().any(|q| matches!(q.id, "speechBubble" | "callout") && node.name.as_deref() == Some(q.label));
    let frame = if bubble { shapes::rectangle(Rect::new(b.x0, b.y0, b.x1, b.y0 + b.height() * 0.78)) } else { path.clone() };
    t.kind = TextKind::Area { frame };
    t.para = super::create::new_type_para();
    t.para.justify = vectorcraft_doc::Justify::Center;
    super::create::new_type_alignment(s, p, C, &mut t)?;
    t.area.inset = (b.width().min(b.height()) * 0.08).clamp(2.0, 24.0);
    t.area.vertical_align = vectorcraft_doc::VerticalAlign::Center;
    super::typecmd::refresh_bounds(&mut t);
    let (group, text) = s.edit("Add Text", |d, sel| {
        let (par, idx, _) = d.position(id).ok_or(EngineError::NoNode(id))?;
        let shape = d.remove(id)?;
        let tid = d.alloc_id();
        let gid = d.alloc_id();
        let mut g = Node::new(gid, NodeKind::Group { children: vec![shape, Arc::new(Node::new(tid, NodeKind::Text(Box::new(t))))], clip: false });
        g.name = node.name.clone();
        d.insert(par, idx, g)?;
        sel.set([tid]);
        Ok((gid, tid))
    })?;
    Ok(json!({ "id": text.0, "group": group.0, "created": true }))
}

// ---------- quick styles and themes ----------

fn quick_style(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "quick.style";
    let style = str_param(p, "style").ok_or_else(|| bad(C, "missing `style` (see quick.list)"))?;
    let st = s.doc()?;
    let pal = Palette::of(theme_of(&st.doc));
    let lk = look(style, &pal).ok_or_else(|| bad(C, format!("unknown style `{style}` (see quick.list)")))?;
    let roots = targets(s, p)?;
    let leaves = leaf_targets(s, &roots)?;
    // Type inside a shape takes a colour that reads on the shape.
    let d = &s.doc()?.doc;
    let in_shape: Vec<NodeId> =
        leaves.iter().copied().filter(|id| vectorcraft_tools::select::shape_text_parts(d, *id).is_some_and(|(_, t)| t == *id)).collect();
    let shape_fill = |d: &Document, text: NodeId| {
        let (g, _) = vectorcraft_tools::select::shape_text_parts(d, text)?;
        let first = d.node(g)?.children()?.first()?.id;
        Some(first)
    };
    let id = style.to_string();
    let changed = s.edit("Quick Style", |d, _| {
        let mut changed = 0;
        for leaf in &leaves {
            let inside = in_shape.contains(leaf);
            let Some(n) = d.node_mut(*leaf) else { continue };
            if apply_look(n, &lk, &pal, inside) {
                changed += 1;
            }
        }
        // Then the type inside shapes reads on their new fill.
        for text in &in_shape {
            let fill = shape_fill(d, *text).and_then(|sh| d.node(sh)).and_then(|n| n.appearance.fill_paint().color());
            if let Some(n) = d.node_mut(*text) {
                set_text_color(n, text_on(fill, &pal));
            }
        }
        d.unknown.insert(STYLE_KEY.into(), json!(id));
        Ok(changed)
    })?;
    Ok(json!({ "changed": changed }))
}

fn quick_theme(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "quick.theme";
    let id = str_param(p, "theme").ok_or_else(|| bad(C, "missing `theme` (see quick.list)"))?;
    let new = theme(id).ok_or_else(|| bad(C, format!("unknown theme `{id}` (see quick.list)")))?;
    let old = theme_of(&s.doc()?.doc);
    let (from, to) = (Palette::of(old).all(), Palette::of(new).all());
    let map = move |c: Color| {
        let hex = c.to_hex();
        from.iter().position(|f| f.to_hex() == hex).and_then(|i| to.get(i).copied()).unwrap_or(c)
    };
    let shadow = (Palette::of(old).dark.to_hex(), Palette::of(new).dark.to_hex());
    let changed = s.edit("Color Theme", |d, _| {
        let ids: Vec<NodeId> = d.layers.iter().map(|l| l.id).collect();
        let scope = super::colorcmds::Scope { fill: true, stroke: true, images: false, patterns: true };
        let changed = super::colorcmds::Recolor::new(scope, &map).run(d, &ids);
        // Quick-style shadows are in the theme's dark colour too.
        let mut ids = vec![];
        d.walk(|n| ids.push(n.id));
        for id in ids {
            if let Some(n) = d.node_mut(id) {
                for e in n.appearance.effects.iter_mut().filter(|e| e.id == "stylize.dropShadow") {
                    if e.params.get("color").and_then(Value::as_str) == Some(shadow.0.as_str()) {
                        e.params["color"] = json!(shadow.1);
                    }
                }
            }
        }
        d.unknown.insert(THEME_KEY.into(), json!(new.id));
        Ok(changed)
    })?;
    Ok(json!({ "theme": new.id, "changed": changed }))
}

fn list(s: &mut Session, _: &Value) -> Result<Value> {
    let (theme, style) = match s.active() {
        Some(st) => (theme_of(&st.doc).id, style_of(&st.doc)),
        None => (OCEAN.id, "solid"),
    };
    Ok(json!({
        "shapes": SHAPES.iter().map(|q| json!({"id": q.id, "label": q.label})).collect::<Vec<_>>(),
        "icons": ICONS.iter().map(|i| json!({"id": i.0, "label": i.1})).collect::<Vec<_>>(),
        "styles": STYLES.iter().map(|q| json!({"id": q.id, "label": q.label})).collect::<Vec<_>>(),
        "themes": THEMES.iter().map(|t| json!({"id": t.id, "label": t.label, "colors": t.colors})).collect::<Vec<_>>(),
        "theme": theme,
        "style": style,
    }))
}

/// A theme's colours (main, second, accent, dark, light) for swatches in the UI.
pub fn theme_colors(t: &Theme) -> [Color; 5] {
    let p = Palette::of(t);
    [p.main, p.second, p.accent, p.dark, p.light]
}

/// How quick style `style` looks in theme `t` (for previews): fill, stroke, dashed, shadow.
pub fn look_preview(style: &str, t: &Theme) -> Option<(Option<Color>, Option<Color>, bool, bool)> {
    let lk = look(style, &Palette::of(t))?;
    Some((lk.fill, lk.stroke.map(|s| s.0), lk.dash, lk.shadow))
}

// ---------- pictures ----------

impl Session {
    /// A PNG of the selection (`artboard: None`; none selected: the active artboard `fallback`) or
    /// of an artboard, at `scale` pixels per point → (bytes, width, height).
    pub fn quick_png(&self, artboard: Option<usize>, fallback: usize, scale: f64, background: Option<&str>) -> Result<(Vec<u8>, u32, u32)> {
        const C: &str = "quick.png";
        if !(scale.is_finite() && scale > 0.0 && scale <= 16.0) {
            return Err(bad(C, "scale must be above 0, at most 16"));
        }
        let st = self.doc()?;
        let selection = artboard.is_none() && !st.selection.is_empty();
        let (doc, rect, bg) = if selection {
            let clip = super::clipboard::Clipboard::copy(st, &st.selection.objects);
            let b = clip.bounds().filter(|b| b.width() > 0.0 && b.height() > 0.0).ok_or_else(|| bad(C, "the selection has no area to picture"))?;
            (fileio::with_single_artboard(clip.to_document(), b, "Selection"), b, background.unwrap_or("transparent"))
        } else {
            let i = artboard.unwrap_or(fallback);
            let r = st.doc.artboards.get(i).map(|a| a.rect).ok_or_else(|| bad(C, format!("no artboard {i}")))?;
            (fileio::with_single_artboard((*st.doc).clone(), r, "Artboard"), r, background.unwrap_or("white"))
        };
        let (w, h) = vectorcraft_render::raster_size(rect, scale).map_err(|e| bad(C, e))?;
        let png = fileio::encode(&doc, "png", &json!({ "scale": scale, "background": bg, "artboard": 0 }))?;
        Ok((png, w, h))
    }
}

fn png(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "quick.png";
    let artboard = match (str_param(p, "target"), p.get("artboard").and_then(Value::as_u64)) {
        (Some("artboard"), a) => Some(a.unwrap_or(0) as usize),
        (Some("selection") | None, None) => None,
        (None, Some(a)) => Some(a as usize),
        (Some("selection"), Some(_)) => return Err(bad(C, "give target selection or an artboard, not both")),
        (Some(o), _) => return Err(bad(C, format!("target must be selection or artboard, not `{o}`"))),
    };
    if artboard.is_none() && str_param(p, "target") == Some("selection") && s.doc()?.selection.is_empty() {
        return Err(bad(C, "nothing selected"));
    }
    let (png, w, h) = s.quick_png(artboard, 0, f64_or(p, "scale", 2.0), str_param(p, "background"))?;
    Ok(json!({ "dataBase64": vectorcraft_format::base64_encode(&png), "width": w, "height": h }))
}
