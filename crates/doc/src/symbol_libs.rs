//! Symbol libraries: named, read-only sets of symbols (Window → Symbol Libraries). The built-in
//! ones are drawn here in code ([`SYMBOL_LIBRARIES`]), all original artwork made of plain paths;
//! user libraries are `.vcsymbols` files, which the engine reads and writes.
//!
//! A library keeps its symbols in a small [`Document`] of their own ([`SymbolLibrary::doc`]):
//! `symbols` in panel order with their natural sizes (`symbolSizes`, as the app records them), and
//! every resource their art uses (nested symbols, patterns, global and spot swatches, images,
//! brushes). Adding a symbol to a document copies it with those resources, as Paste does.

use std::sync::{Arc, OnceLock};

use serde_json::json;
use vectorcraft_color::{Color, Paint};
use vectorcraft_geom::{Affine, BezPath, PathData, Point, Rect, shapes};

use crate::{Appearance, Document, Node, SYMBOL_HALF, SYMBOL_SIZES, Symbol};

/// The extension (and `format` header) of symbol library files.
pub const SYMBOLS_EXT: &str = "vcsymbols";

/// A symbol library: its name and the document holding its symbols and their resources.
#[derive(Clone, Debug, PartialEq)]
pub struct SymbolLibrary {
    pub name: String,
    pub doc: Document,
}

impl Default for SymbolLibrary {
    fn default() -> Self {
        Self::new("")
    }
}

impl SymbolLibrary {
    /// An empty library named `name`: a document with no swatches, styles or art of its own.
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), doc: empty_document() }
    }
    pub fn len(&self) -> usize {
        self.doc.symbols.len()
    }
    pub fn is_empty(&self) -> bool {
        self.doc.symbols.is_empty()
    }
    pub fn symbol(&self, name: &str) -> Option<&Symbol> {
        self.doc.symbols.iter().find(|s| s.name == name)
    }
    /// The symbols' names, in panel order.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.doc.symbols.iter().map(|s| s.name.as_str())
    }
    /// The natural size of symbol `name` in points, as recorded (`None` for a symbol whose art is
    /// kept as drawn).
    pub fn size(&self, name: &str) -> Option<(f64, f64)> {
        natural_size(&self.doc, name)
    }

    /// Add `art` as symbol `name` the way the app makes symbols: centred on the origin and scaled
    /// into the ±10 pt instance box (strokes untouched), its size as drawn recorded. Art without
    /// bounds is skipped.
    pub fn push(&mut self, name: &str, mut art: Node) {
        let Some(b) = art.geometric_bounds() else { return };
        let (w, h) = (b.width().max(1.0), b.height().max(1.0));
        let n = Affine::scale_non_uniform(2.0 * SYMBOL_HALF / w, 2.0 * SYMBOL_HALF / h) * Affine::translate(-b.center().to_vec2());
        art.transform(n, false);
        self.doc.symbols.push(Symbol { name: name.into(), art: Arc::new(art), slice: None });
        set_natural_size(&mut self.doc, name, (w, h));
    }
}

/// The document a library starts from: no swatches, colour groups or graphic styles.
pub fn empty_document() -> Document {
    let mut d = Document::new(100.0, 100.0);
    d.swatches.clear();
    d.swatch_groups.clear();
    d.graphic_styles.clear();
    d
}

/// Symbol `name`'s natural size recorded in `d` (finite and positive), if any.
pub fn natural_size(d: &Document, name: &str) -> Option<(f64, f64)> {
    let v = d.unknown.get(SYMBOL_SIZES)?.get(name)?;
    let (w, h) = (v.get(0)?.as_f64()?, v.get(1)?.as_f64()?);
    (w.is_finite() && h.is_finite() && w > 0.0 && h > 0.0).then_some((w, h))
}

/// Record symbol `name`'s natural size in `d`.
pub fn set_natural_size(d: &mut Document, name: &str, (w, h): (f64, f64)) {
    let m = d.unknown.entry(SYMBOL_SIZES.to_string()).or_insert_with(|| json!({}));
    if !m.is_object() {
        *m = json!({});
    }
    if let Some(o) = m.as_object_mut() {
        o.insert(name.into(), json!([w, h]));
    }
}

// ---------- built-in libraries ----------

/// A built-in library: a stable id, its menu name and the function that draws its symbols.
pub struct BuiltinSymbolLibrary {
    pub id: &'static str,
    pub name: &'static str,
    make: fn(&mut Art),
}

/// The built-in symbol libraries, in menu order.
pub const SYMBOL_LIBRARIES: &[BuiltinSymbolLibrary] = &[
    BuiltinSymbolLibrary { id: "basic-shapes", name: "Basic Shapes", make: basic_shapes },
    BuiltinSymbolLibrary { id: "direction-signs", name: "Direction Signs", make: direction_signs },
    BuiltinSymbolLibrary { id: "interface-glyphs", name: "Interface Glyphs", make: interface_glyphs },
    BuiltinSymbolLibrary { id: "outdoors", name: "Outdoors", make: outdoors },
    BuiltinSymbolLibrary { id: "map-markers", name: "Map Markers", make: map_markers },
    BuiltinSymbolLibrary { id: "badges-labels", name: "Badges and Labels", make: badges_labels },
];

/// Built-in library `id`, drawn once.
pub fn builtin_symbol_library(id: &str) -> Option<Arc<SymbolLibrary>> {
    static LIBS: OnceLock<Vec<Arc<SymbolLibrary>>> = OnceLock::new();
    let libs = LIBS.get_or_init(|| {
        SYMBOL_LIBRARIES
            .iter()
            .map(|b| {
                let mut art = Art { lib: SymbolLibrary::new(b.name) };
                (b.make)(&mut art);
                Arc::new(art.lib)
            })
            .collect()
    });
    let i = SYMBOL_LIBRARIES.iter().position(|b| b.id == id)?;
    libs.get(i).cloned()
}

/// A library being drawn. Coordinates are points, y down; each symbol is drawn in a 40 × 40 box.
struct Art {
    lib: SymbolLibrary,
}

/// One shape of a symbol: its outline, fill and stroke.
struct Shape {
    path: PathData,
    fill: Paint,
    stroke: Paint,
    width: f64,
}

impl Art {
    /// Symbol `name` made of `shapes`, painted first to last (one shape: a path, else a group).
    fn symbol(&mut self, name: &str, shapes: Vec<Shape>) {
        let doc = &mut self.lib.doc;
        let mut nodes: Vec<Node> =
            shapes.into_iter().map(|s| Node::path(doc.alloc_id(), s.path, Appearance::basic(s.fill, s.stroke, s.width))).collect();
        let art = match nodes.len() {
            1 => nodes.pop(),
            _ => Some(Node::group(doc.alloc_id(), nodes.into_iter().map(Arc::new).collect())),
        };
        if let Some(art) = art {
            self.lib.push(name, art);
        }
    }
}

fn hex(h: &str) -> Paint {
    Paint::solid(Color::from_hex(h).unwrap_or(Color::BLACK))
}

/// SVG path data `d` (an empty path if it doesn't parse).
fn svg(d: &str) -> PathData {
    PathData::from_bezpath(&BezPath::from_svg(d).unwrap_or_default())
}

/// `path` filled with `fill`.
fn filled(path: PathData, fill: &str) -> Shape {
    Shape { path, fill: hex(fill), stroke: Paint::None, width: 0.0 }
}

/// `path` filled and stroked.
fn outlined(path: PathData, fill: &str, stroke: &str, width: f64) -> Shape {
    Shape { path, fill: hex(fill), stroke: hex(stroke), width }
}

/// `path` stroked only.
fn stroked(path: PathData, stroke: &str, width: f64) -> Shape {
    Shape { path, fill: Paint::None, stroke: hex(stroke), width }
}

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> PathData {
    shapes::rectangle(Rect::new(x0, y0, x1, y1))
}

fn rounded(x0: f64, y0: f64, x1: f64, y1: f64, radius: f64) -> PathData {
    shapes::rounded_rectangle(Rect::new(x0, y0, x1, y1), radius)
}

fn ellipse(x0: f64, y0: f64, x1: f64, y1: f64) -> PathData {
    shapes::ellipse(Rect::new(x0, y0, x1, y1))
}

fn star(cx: f64, cy: f64, r1: f64, r2: f64, points: u32) -> PathData {
    shapes::star(Point::new(cx, cy), r1, r2, points, 0.0)
}

/// A gear of `teeth` around (20, 20): flat-topped teeth between radii 15 and 20.
fn gear(teeth: u32) -> PathData {
    let n = teeth.max(3);
    let step = std::f64::consts::TAU / f64::from(n);
    let mut pts = vec![];
    for i in 0..n {
        let a = step * f64::from(i);
        for (da, r) in [(0.0, 15.0), (0.18, 20.0), (0.32, 20.0), (0.5, 15.0)] {
            let t = a + da * step;
            pts.push(Point::new(20.0 + r * t.cos(), 20.0 + r * t.sin()));
        }
    }
    PathData::single(vectorcraft_geom::SubPath::polyline(&pts, true))
}

fn basic_shapes(a: &mut Art) {
    a.symbol("Circle", vec![filled(ellipse(0.0, 0.0, 40.0, 40.0), "#3a86ff")]);
    a.symbol("Square", vec![filled(rect(0.0, 0.0, 40.0, 40.0), "#ff006e")]);
    a.symbol("Rounded Square", vec![filled(rounded(0.0, 0.0, 40.0, 40.0, 8.0), "#8338ec")]);
    a.symbol("Triangle", vec![filled(svg("M20 2 L38 36 L2 36 Z"), "#fb5607")]);
    a.symbol("Diamond", vec![filled(svg("M20 0 L40 20 L20 40 L0 20 Z"), "#ffbe0b")]);
    a.symbol("Hexagon", vec![filled(shapes::polygon(Point::new(20.0, 20.0), 20.0, 6, 0.0), "#06d6a0")]);
    a.symbol("Star", vec![outlined(star(20.0, 21.0, 20.0, 8.0, 5), "#ffd166", "#e09f3e", 1.0)]);
    a.symbol(
        "Heart",
        vec![filled(svg("M20 36 C8 27 0 20 0 11 C0 5 5 0 11 0 C15 0 18 2 20 6 C22 2 25 0 29 0 C35 0 40 5 40 11 C40 20 32 27 20 36 Z"), "#ef476f")],
    );
    a.symbol("Ring", vec![stroked(ellipse(4.0, 4.0, 36.0, 36.0), "#118ab2", 8.0)]);
}

fn direction_signs(a: &mut Art) {
    const INK: &str = "#1d3557";
    a.symbol("Arrow Right", vec![filled(svg("M0 14 H24 V4 L40 20 L24 36 V26 H0 Z"), INK)]);
    a.symbol("Arrow Up", vec![filled(svg("M14 40 V16 H4 L20 0 L36 16 H26 V40 Z"), INK)]);
    a.symbol("Arrow Down", vec![filled(svg("M20 40 L4 22 H13 V0 H27 V22 H36 Z"), INK)]);
    a.symbol("Two-Way Arrow", vec![filled(svg("M0 20 L12 8 V15 H28 V8 L40 20 L28 32 V25 H12 V32 Z"), "#457b9d")]);
    a.symbol("Chevron", vec![filled(svg("M8 0 H20 L36 20 L20 40 H8 L24 20 Z"), "#e63946")]);
    a.symbol("Turn Arrow", vec![filled(svg("M4 40 V18 C4 11 9 6 16 6 H26 V0 L38 11 L26 22 V16 H16 C15 16 14 17 14 18 V40 Z"), INK)]);
    a.symbol(
        "Arrow in Circle",
        vec![filled(ellipse(0.0, 0.0, 40.0, 40.0), "#457b9d"), filled(svg("M10 17 H22 V11 L31 20 L22 29 V23 H10 Z"), "#ffffff")],
    );
    a.symbol(
        "Round Trip",
        vec![
            stroked(svg("M33 15 C31 9 26 6 20 6 C12 6 6 12 6 20 C6 28 12 34 20 34 C26 34 31 30 33 25"), INK, 4.0),
            filled(svg("M26 7 L38 9 L34 20 Z"), INK),
        ],
    );
}

fn interface_glyphs(a: &mut Art) {
    const INK: &str = "#2b2d42";
    a.symbol("Home", vec![filled(svg("M20 2 L40 20 H34 V38 H24 V26 H16 V38 H6 V20 H0 Z"), INK)]);
    a.symbol("Magnifier", vec![stroked(ellipse(3.0, 3.0, 27.0, 27.0), INK, 5.0), filled(svg("M23 27 L27 23 L39 35 L35 39 Z"), INK)]);
    a.symbol("Envelope", vec![filled(rect(0.0, 6.0, 40.0, 34.0), INK), stroked(svg("M3 9 L20 22 L37 9"), "#ffffff", 2.5)]);
    a.symbol("Person", vec![filled(ellipse(12.0, 2.0, 28.0, 18.0), INK), filled(svg("M4 40 C4 29 11 22 20 22 C29 22 36 29 36 40 Z"), INK)]);
    a.symbol(
        "Bell",
        vec![
            filled(svg("M20 2 C27 2 31 8 31 15 V25 L36 32 H4 L9 25 V15 C9 8 13 2 20 2 Z"), "#ffb703"),
            filled(ellipse(16.0, 33.0, 24.0, 40.0), "#fb8500"),
        ],
    );
    a.symbol(
        "Padlock",
        vec![stroked(svg("M11 19 V12 C11 6 15 2 20 2 C25 2 29 6 29 12 V19"), INK, 4.0), filled(rounded(4.0, 18.0, 36.0, 40.0, 3.0), INK)],
    );
    a.symbol(
        "Chat Bubble",
        vec![filled(svg("M4 4 H36 C38 4 40 6 40 8 V26 C40 28 38 30 36 30 H16 L6 38 V30 H4 C2 30 0 28 0 26 V8 C0 6 2 4 4 4 Z"), "#3a86ff")],
    );
    a.symbol("Check Mark", vec![filled(svg("M2 22 L8 16 L16 24 L32 6 L38 12 L16 36 Z"), "#2a9d8f")]);
    a.symbol("Gear", vec![filled(gear(8), "#6c757d"), filled(ellipse(13.0, 13.0, 27.0, 27.0), "#ffffff")]);
}

fn outdoors(a: &mut Art) {
    a.symbol("Leaf", vec![filled(svg("M2 38 C2 16 14 2 38 2 C38 26 24 38 2 38 Z"), "#52b788"), stroked(svg("M5 35 L30 10"), "#2d6a4f", 1.5)]);
    let mut petals: Vec<Shape> = (0..5)
        .map(|k| {
            let mut p = ellipse(15.0, 0.0, 25.0, 19.0);
            p.transform(Affine::rotate_about(f64::from(k) * 72f64.to_radians(), Point::new(20.0, 20.0)));
            filled(p, "#f4acb7")
        })
        .collect();
    petals.push(filled(ellipse(14.0, 14.0, 26.0, 26.0), "#ffd166"));
    a.symbol("Blossom", petals);
    a.symbol("Tree", vec![filled(rect(17.0, 24.0, 23.0, 40.0), "#7f4f24"), filled(ellipse(4.0, 0.0, 36.0, 28.0), "#40916c")]);
    a.symbol("Fir", vec![filled(rect(17.0, 29.0, 23.0, 40.0), "#6f4518"), filled(svg("M20 0 L32 16 H26 L36 30 H4 L14 16 H8 Z"), "#2d6a4f")]);
    a.symbol("Sun", vec![filled(star(20.0, 20.0, 20.0, 14.0, 12), "#ffb703"), filled(ellipse(9.0, 9.0, 31.0, 31.0), "#fb8500")]);
    a.symbol(
        "Cloud",
        vec![outlined(
            svg("M10 34 C4 34 0 30 0 25 C0 20 4 16 9 16 C10 9 15 4 22 4 C29 4 34 9 35 15 C38 16 40 20 40 24 C40 30 36 34 30 34 Z"),
            "#caf0f8",
            "#48cae4",
            1.5,
        )],
    );
    a.symbol(
        "Crescent",
        vec![filled(svg("M26 2 C16 4 10 12 10 21 C10 31 18 38 28 38 C32 38 36 37 38 35 C24 36 16 28 16 18 C16 11 20 5 26 2 Z"), "#ffd166")],
    );
    a.symbol("Raindrop", vec![filled(svg("M20 0 C26 10 34 18 34 26 C34 34 28 40 20 40 C12 40 6 34 6 26 C6 18 14 10 20 0 Z"), "#48cae4")]);
    a.symbol(
        "Peaks",
        vec![filled(svg("M0 36 L14 10 L22 22 L28 14 L40 36 Z"), "#6c757d"), filled(svg("M14 10 L18.5 18.4 L14 16 L9.7 18 Z"), "#ffffff")],
    );
}

fn map_markers(a: &mut Art) {
    a.symbol(
        "Pin",
        vec![
            filled(svg("M20 40 C20 40 6 24 6 14 C6 6 12 0 20 0 C28 0 34 6 34 14 C34 24 20 40 20 40 Z"), "#e63946"),
            filled(ellipse(14.0, 8.0, 26.0, 20.0), "#ffffff"),
        ],
    );
    a.symbol("Flag", vec![filled(rect(4.0, 0.0, 7.0, 40.0), "#343a40"), filled(svg("M7 2 H36 L30 10 L36 18 H7 Z"), "#e63946")]);
    a.symbol(
        "Compass",
        vec![
            outlined(ellipse(0.0, 0.0, 40.0, 40.0), "#f1faee", "#1d3557", 1.5),
            filled(star(20.0, 20.0, 18.0, 4.0, 4), "#1d3557"),
            filled(svg("M20 2 L24 20 H16 Z"), "#e63946"),
        ],
    );
    a.symbol(
        "First Aid",
        vec![filled(ellipse(0.0, 0.0, 40.0, 40.0), "#e63946"), filled(svg("M16 7 H24 V16 H33 V24 H24 V33 H16 V24 H7 V16 H16 Z"), "#ffffff")],
    );
    a.symbol(
        "Information",
        vec![
            filled(ellipse(0.0, 0.0, 40.0, 40.0), "#1d70b8"),
            filled(ellipse(17.0, 7.0, 23.0, 13.0), "#ffffff"),
            filled(rounded(17.0, 16.0, 23.0, 33.0, 1.0), "#ffffff"),
        ],
    );
    a.symbol(
        "Parking",
        vec![
            filled(rounded(0.0, 0.0, 40.0, 40.0, 6.0), "#1d70b8"),
            filled(
                svg("M13 8 H23 C28 8 31 11 31 16 C31 21 28 24 23 24 H18 V32 H13 Z M18 13 V19 H22 C24 19 25.5 18 25.5 16 C25.5 14 24 13 22 13 Z"),
                "#ffffff",
            ),
        ],
    );
    a.symbol("Summit", vec![filled(svg("M0 38 L20 4 L40 38 Z"), "#588157"), filled(svg("M20 4 L26 14 L20 12 L14 14 Z"), "#ffffff")]);
    a.symbol("Campsite", vec![filled(svg("M20 4 L38 36 H2 Z"), "#bc6c25"), filled(svg("M20 18 L26 36 H14 Z"), "#283618")]);
}

fn badges_labels(a: &mut Art) {
    a.symbol("Banner", vec![filled(svg("M0 8 H40 L34 16 L40 24 H0 L6 16 Z"), "#e63946")]);
    a.symbol("Seal", vec![filled(star(20.0, 20.0, 20.0, 17.0, 16), "#ffb703"), stroked(ellipse(7.0, 7.0, 33.0, 33.0), "#ffffff", 1.5)]);
    a.symbol("Shield", vec![outlined(svg("M20 0 L38 6 V18 C38 29 30 36 20 40 C10 36 2 29 2 18 V6 Z"), "#457b9d", "#1d3557", 1.5)]);
    a.symbol("Tag", vec![filled(svg("M0 4 H28 L40 20 L28 36 H0 Z"), "#f4a261"), filled(ellipse(26.0, 17.0, 32.0, 23.0), "#ffffff")]);
    a.symbol(
        "Ticket",
        vec![filled(svg("M0 8 H40 V16 C37 16 36 18 36 20 C36 22 37 24 40 24 V32 H0 V24 C3 24 4 22 4 20 C4 18 3 16 0 16 Z"), "#2a9d8f")],
    );
    a.symbol(
        "Rosette",
        vec![
            filled(svg("M12 22 L6 40 L13 36 L17 40 L20 26 Z M28 22 L34 40 L27 36 L23 40 L20 26 Z"), "#1d3557"),
            filled(star(20.0, 15.0, 15.0, 13.0, 20), "#e9c46a"),
        ],
    );
    a.symbol("Pill Label", vec![filled(rounded(0.0, 10.0, 40.0, 30.0, 10.0), "#8338ec")]);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NodeKind;

    #[test]
    fn builtin_libraries_are_drawn_once_with_unique_names_and_sizes() {
        let mut ids: Vec<&str> = SYMBOL_LIBRARIES.iter().map(|b| b.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), SYMBOL_LIBRARIES.len(), "library ids are unique");
        for b in SYMBOL_LIBRARIES {
            let lib = builtin_symbol_library(b.id).unwrap();
            assert_eq!(lib.name, b.name);
            assert!((6..=12).contains(&lib.len()), "{}: {}", b.id, lib.len());
            let mut names: Vec<&str> = lib.names().collect();
            names.sort_unstable();
            names.dedup();
            assert_eq!(names.len(), lib.len(), "symbol names in {} are unique", b.id);
            for s in &lib.doc.symbols {
                let (w, h) = lib.size(&s.name).unwrap();
                assert!(w >= 20.0 && h >= 10.0 && w <= 41.0 && h <= 41.0, "{} / {}: {w} × {h}", b.id, s.name);
                let bounds = s.art.geometric_bounds().unwrap();
                assert!((bounds.width() - 20.0).abs() < 1e-6 && (bounds.height() - 20.0).abs() < 1e-6, "{}: normalised", s.name);
                assert!(bounds.center().to_vec2().hypot() < 1e-6, "{}: centred", s.name);
                let mut paths = 0;
                s.art.walk(&mut |n| paths += usize::from(matches!(&n.kind, NodeKind::Path { path, .. } if !path.is_empty())));
                assert!(paths > 0, "{}", s.name);
            }
            assert!(lib.doc.swatches.is_empty() && lib.doc.graphic_styles.is_empty());
        }
        assert!(Arc::ptr_eq(&builtin_symbol_library("outdoors").unwrap(), &builtin_symbol_library("outdoors").unwrap()), "made once");
        assert!(builtin_symbol_library("nope").is_none());
    }

    #[test]
    fn sizes_are_read_defensively() {
        let mut lib = SymbolLibrary::new("x");
        lib.doc.unknown.insert(SYMBOL_SIZES.into(), json!({"a": [10, 20], "b": [0, 5], "c": "x"}));
        assert_eq!(lib.size("a"), Some((10.0, 20.0)));
        assert_eq!((lib.size("b"), lib.size("c"), lib.size("e")), (None, None, None));
        lib.doc.unknown.insert(SYMBOL_SIZES.into(), json!(3));
        set_natural_size(&mut lib.doc, "a", (4.0, 5.0));
        assert_eq!(lib.size("a"), Some((4.0, 5.0)));
    }
}
