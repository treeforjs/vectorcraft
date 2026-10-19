//! Brush libraries: named, read-only sets of brushes (Window → Brush Libraries, the Brushes
//! panel's library button). The built-in ones are generated here in code ([`BRUSH_LIBRARIES`]);
//! all their art is original. User libraries are `.vcbrushes` files ([`write`], [`read`]): JSON
//! holding the brush definitions.

use std::f64::consts::{PI, TAU};
use std::sync::{Arc, OnceLock};

use serde::{Deserialize, Serialize};
use vectorcraft_color::Color;
use vectorcraft_doc::{Document, Node};
use vectorcraft_geom::{Anchor, PathData, Point, Rect, SubPath, shapes};

use crate::defaults::{calli, compound, group, path, smooth_closed};
use crate::{
    ArtBrush, ArtScale, AutoCorner, Bristle, BristleShape, Brush, BrushKind, Calligraphic, Colorization, PatternBrush, PatternFit, Scatter, Variation,
};

/// A brush library: its name and its brushes.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct BrushLibrary {
    pub name: String,
    #[serde(default)]
    pub brushes: Vec<Brush>,
}

impl BrushLibrary {
    pub fn len(&self) -> usize {
        self.brushes.len()
    }
    pub fn is_empty(&self) -> bool {
        self.brushes.is_empty()
    }
    pub fn brush(&self, name: &str) -> Option<&Brush> {
        self.brushes.iter().find(|b| b.name == name)
    }

    /// The document's brushes `names` (empty: all of them, in panel order) as a library named
    /// `name`.
    pub fn from_document(d: &Document, names: &[String], name: String) -> Result<Self, String> {
        let lib = crate::library(d);
        if let Some(n) = names.iter().find(|n| !lib.iter().any(|b| &b.name == *n)) {
            return Err(format!("no brush `{n}`"));
        }
        let brushes = lib.into_iter().filter(|b| names.is_empty() || names.contains(&b.name)).collect();
        Ok(Self { name, brushes })
    }
}

// ---------- .vcbrushes files ----------

/// The extension (and `format` header) of brush library files.
pub const BRUSHES_EXT: &str = "vcbrushes";

/// The file: a header around the library.
#[derive(Serialize, Deserialize)]
struct NativeFile {
    format: String,
    version: u32,
    #[serde(flatten)]
    library: BrushLibrary,
}

/// `lib` as a `.vcbrushes` file.
pub fn write(lib: &BrushLibrary) -> String {
    let file = NativeFile { format: BRUSHES_EXT.into(), version: 1, library: lib.clone() };
    serde_json::to_string_pretty(&file).unwrap_or_default()
}

/// Read a `.vcbrushes` file; an unnamed library is called `name`. Brushes this version can't
/// read are left out, as a document's are.
pub fn read(text: &str, name: &str) -> Result<BrushLibrary, String> {
    #[derive(Deserialize)]
    struct Loose {
        format: String,
        #[serde(default)]
        name: String,
        #[serde(default)]
        brushes: serde_json::Value,
    }
    let f: Loose = serde_json::from_str(text.trim_start_matches('\u{feff}')).map_err(|e| format!("not a brush library: {e}"))?;
    if f.format != BRUSHES_EXT {
        return Err(format!("not a brush library (format `{}`)", f.format));
    }
    let mut brushes: Vec<Brush> = vec![];
    for b in crate::parse_library(&f.brushes) {
        if !brushes.iter().any(|x| x.name == b.name) {
            brushes.push(b);
        }
    }
    let name = if f.name.trim().is_empty() { name.to_string() } else { f.name };
    Ok(BrushLibrary { name, brushes })
}

/// Does `text` look like a `.vcbrushes` file (its header names the format)?
pub fn sniff(text: &str) -> bool {
    let t = text.trim_start_matches('\u{feff}').trim_start();
    let head = t.char_indices().nth(256).map_or(t, |(i, _)| &t[..i]);
    t.starts_with('{') && head.contains(&format!("\"{BRUSHES_EXT}\""))
}

// ---------- built-in libraries ----------

/// A built-in library: a stable id, its menu name and the function that makes its brushes.
pub struct BuiltinBrushLibrary {
    pub id: &'static str,
    pub name: &'static str,
    make: fn() -> Vec<Brush>,
}

/// The built-in brush libraries, in menu order.
pub const BRUSH_LIBRARIES: &[BuiltinBrushLibrary] = &[
    BuiltinBrushLibrary { id: "arrows", name: "Arrows", make: arrows },
    BuiltinBrushLibrary { id: "borders", name: "Borders", make: borders },
    BuiltinBrushLibrary { id: "decorative-scatter", name: "Decorative Scatter", make: decorative_scatter },
    BuiltinBrushLibrary { id: "ink-calligraphy", name: "Ink and Calligraphy", make: ink_calligraphy },
    BuiltinBrushLibrary { id: "paint-pencil", name: "Paint and Pencil", make: paint_pencil },
];

/// Built-in library `id`, made once.
pub fn builtin_brush_library(id: &str) -> Option<Arc<BrushLibrary>> {
    static LIBS: OnceLock<Vec<Arc<BrushLibrary>>> = OnceLock::new();
    let libs = LIBS.get_or_init(|| BRUSH_LIBRARIES.iter().map(|b| Arc::new(BrushLibrary { name: b.name.into(), brushes: (b.make)() })).collect());
    BRUSH_LIBRARIES.iter().position(|b| b.id == id).and_then(|i| libs.get(i).cloned())
}

fn brush(name: &str, kind: BrushKind) -> Brush {
    Brush { name: name.into(), kind }
}

fn art(name: &str, art: Node, scale: ArtScale) -> Brush {
    brush(name, BrushKind::Art(ArtBrush { art, scale, colorization: Colorization::Tints, ..Default::default() }))
}

fn polygon(pts: &[(f64, f64)]) -> PathData {
    let pts: Vec<Point> = pts.iter().map(|&(x, y)| Point::new(x, y)).collect();
    PathData::single(SubPath::polyline(&pts, true))
}

fn black(p: PathData) -> Node {
    path(p, Color::BLACK)
}

// Art brush art runs left to right along y = 0, about 100 pt long.

fn arrows() -> Vec<Brush> {
    let head = |x: f64, len: f64, half: f64| polygon(&[(x - len, -half), (x, 0.0), (x - len, half)]);
    let tail_head = |x: f64, len: f64, half: f64| polygon(&[(x + len, -half), (x, 0.0), (x + len, half)]);
    let shaft = |x0: f64, x1: f64, half: f64| shapes::rectangle(Rect::new(x0, -half, x1, half));
    // An open chevron head: two bars meeting at the tip.
    let chevron = polygon(&[(86.0, -5.0), (100.0, 0.0), (86.0, 5.0), (84.5, 3.6), (95.0, 0.0), (84.5, -3.6)]);
    let fletch = |x: f64| {
        let vane = |s: f64| polygon(&[(x, 0.0), (x + 6.0, 0.0), (x + 2.0, 4.0 * s), (x - 4.0, 4.0 * s)]);
        [vane(-1.0), vane(1.0)]
    };
    let [f1, f2] = fletch(2.0);
    let [f3, f4] = fletch(10.0);
    let guides = |start: f64, end: f64| ArtScale::BetweenGuides { start, end };
    vec![
        art("Open Arrow", group(vec![black(shaft(0.0, 97.0, 0.75)), black(chevron)]), guides(0.0, 0.84)),
        art(
            "Double Arrow",
            group(vec![black(shaft(14.0, 86.0, 1.0)), black(head(100.0, 18.0, 4.5)), black(tail_head(0.0, 18.0, 4.5))]),
            guides(0.18, 0.82),
        ),
        art("Tapered Arrow", group(vec![black(polygon(&[(0.0, 0.0), (80.0, -1.8), (80.0, 1.8)])), black(head(100.0, 22.0, 5.0))]), guides(0.0, 0.78)),
        art(
            "Fletched Arrow",
            group(vec![black(shaft(0.0, 84.0, 0.8)), black(head(100.0, 16.0, 4.0)), black(f1), black(f2), black(f3), black(f4)]),
            guides(0.18, 0.84),
        ),
        art(
            "Dot to Arrow",
            group(vec![black(shapes::ellipse(Rect::new(0.0, -3.5, 7.0, 3.5))), black(shaft(5.0, 84.0, 0.9)), black(head(100.0, 16.0, 4.5))]),
            guides(0.07, 0.84),
        ),
        art(
            "Curved Head Arrow",
            group(vec![
                black(shaft(0.0, 86.0, 0.9)),
                black(PathData::single(SubPath::new(
                    vec![
                        Anchor::corner(Point::new(80.0, -6.0)),
                        Anchor::with_handles(Point::new(100.0, 0.0), Point::new(92.0, -1.0), Point::new(92.0, 1.0)),
                        Anchor::corner(Point::new(80.0, 6.0)),
                        Anchor::with_handles(Point::new(86.0, 0.0), Point::new(85.0, 3.0), Point::new(85.0, -3.0)),
                    ],
                    true,
                ))),
            ]),
            guides(0.0, 0.8),
        ),
    ]
}

/// A pattern brush of side tile `side`.
fn pattern(name: &str, side: Node, fit: PatternFit, auto_corner: AutoCorner) -> Brush {
    brush(name, BrushKind::Pattern(PatternBrush { side, fit, auto_corner, ..Default::default() }))
}

/// `shape` over an invisible spacer `w` wide that fixes the tile's width (and gap).
fn spaced(w: f64, h: f64, shapes: Vec<Node>) -> Node {
    let mut spacer = black(shapes::rectangle(Rect::new(0.0, -h / 2.0, w, h / 2.0)));
    spacer.appearance = Default::default();
    let mut all = vec![spacer];
    all.extend(shapes);
    group(all)
}

fn borders() -> Vec<Brush> {
    // One period of a sine band, 16 × 6 pt.
    let wave = {
        let n = 16;
        let y = |i: usize, off: f64| Point::new(16.0 * i as f64 / n as f64, 2.0 * (TAU * i as f64 / n as f64).sin() + off);
        let mut pts: Vec<Point> = (0..=n).map(|i| y(i, -0.8)).collect();
        pts.extend((0..=n).rev().map(|i| y(i, 0.8)));
        black(PathData::single(SubPath::polyline(&pts, true)))
    };
    let zigzag = black(polygon(&[(0.0, 3.0), (5.0, -3.0), (10.0, 3.0), (10.0, 4.4), (5.0, -1.6), (0.0, 4.4)]));
    let scallop = black(PathData::single(SubPath::new(
        vec![
            Anchor::corner(Point::new(0.0, 2.0)),
            Anchor::with_handles(Point::new(5.0, -3.0), Point::new(1.0, -3.0), Point::new(9.0, -3.0)),
            Anchor::corner(Point::new(10.0, 2.0)),
        ],
        true,
    )));
    // A twisted strand: slanted lenses side by side.
    let rope = {
        let lens = smooth_closed(&[Point::new(0.0, 3.0), Point::new(5.0, 1.4), Point::new(8.0, -3.0), Point::new(3.0, -1.4)]);
        group(vec![black(lens)])
    };
    let checker = group(vec![
        black(shapes::rectangle(Rect::new(0.0, -3.0, 4.0, 0.0))),
        black(shapes::rectangle(Rect::new(4.0, 0.0, 8.0, 3.0))),
        path(shapes::rectangle(Rect::new(4.0, -3.0, 8.0, 0.0)), Color::gray(0.55)),
        path(shapes::rectangle(Rect::new(0.0, 0.0, 4.0, 3.0)), Color::gray(0.55)),
    ]);
    vec![
        pattern(
            "Dashes",
            spaced(12.0, 2.0, vec![black(shapes::rounded_rectangle(Rect::new(1.0, -1.0, 9.0, 1.0), 1.0))]),
            PatternFit::Stretch,
            AutoCorner::Between,
        ),
        pattern(
            "Dotted Line",
            spaced(7.0, 3.0, vec![black(shapes::ellipse(Rect::new(2.0, -1.5, 5.0, 1.5)))]),
            PatternFit::Stretch,
            AutoCorner::Centered,
        ),
        pattern("Zigzag", zigzag, PatternFit::Stretch, AutoCorner::Between),
        pattern("Waves", wave, PatternFit::Stretch, AutoCorner::Between),
        pattern("Scallops", scallop, PatternFit::Stretch, AutoCorner::Centered),
        pattern("Rope", spaced(5.0, 6.0, vec![rope]), PatternFit::Stretch, AutoCorner::Between),
        pattern("Checker", checker, PatternFit::Stretch, AutoCorner::Sliced),
    ]
}

/// A scatter brush of `art`, sizes, spacing, scatter and rotation ranges in percent and degrees.
fn scatter(
    name: &str,
    art: Node,
    size: (f64, f64),
    spacing: (f64, f64),
    spread: (f64, f64),
    rotation: (f64, f64),
    colorization: Colorization,
) -> Brush {
    brush(
        name,
        BrushKind::Scatter(Scatter { art, size, spacing, scatter: spread, rotation, rotation_relative_to_path: false, colorization, modes: None }),
    )
}

fn decorative_scatter() -> Vec<Brush> {
    let o = Point::ORIGIN;
    let heart = black(PathData::single(SubPath::new(
        vec![
            Anchor::corner(Point::new(0.0, 4.0)),
            Anchor::with_handles(Point::new(-4.0, -1.0), Point::new(-3.0, 1.5), Point::new(-5.0, -3.0)),
            Anchor::with_handles(Point::new(0.0, -2.0), Point::new(-2.0, -4.5), Point::new(2.0, -4.5)),
            Anchor::with_handles(Point::new(4.0, -1.0), Point::new(5.0, -3.0), Point::new(3.0, 1.5)),
        ],
        true,
    )));
    let ring = compound(vec![shapes::ellipse(Rect::new(-4.0, -4.0, 4.0, 4.0)), shapes::ellipse(Rect::new(-3.0, -3.0, 3.0, 3.0))], Color::BLACK);
    let snowflake = {
        let arm = |k: usize| {
            let a = PI / 3.0 * k as f64;
            let (c, s) = (a.cos(), a.sin());
            let p = |x: f64, y: f64| (x * c - y * s, x * s + y * c);
            black(polygon(&[p(0.0, -0.35), p(4.5, -0.35), p(4.5, 0.35), p(0.0, 0.35)]))
        };
        group((0..6).map(arm).chain([black(shapes::polygon(o, 1.2, 6, 0.0))]).collect())
    };
    let petal = path(
        smooth_closed(&[Point::new(-3.5, 0.0), Point::new(0.0, -1.6), Point::new(3.5, 0.0), Point::new(0.0, 1.6)]),
        Color::rgb8(0xe8, 0x7a, 0x9a),
    );
    vec![
        scatter(
            "Stars",
            black(shapes::star(o, 4.0, 1.8, 5, 0.0)),
            (70.0, 130.0),
            (110.0, 200.0),
            (-120.0, 120.0),
            (-180.0, 180.0),
            Colorization::Tints,
        ),
        scatter("Hearts", heart, (60.0, 120.0), (120.0, 200.0), (-100.0, 100.0), (-30.0, 30.0), Colorization::Tints),
        scatter("Bubbles", ring, (40.0, 140.0), (90.0, 180.0), (-150.0, 150.0), (0.0, 0.0), Colorization::Tints),
        scatter("Snowflakes", snowflake, (60.0, 120.0), (130.0, 220.0), (-140.0, 140.0), (-180.0, 180.0), Colorization::Tints),
        scatter(
            "Sparkles",
            black(shapes::star(o, 4.0, 0.9, 4, 0.0)),
            (40.0, 120.0),
            (120.0, 240.0),
            (-160.0, 160.0),
            (-20.0, 20.0),
            Colorization::Tints,
        ),
        scatter("Petals", petal, (70.0, 110.0), (90.0, 140.0), (-80.0, 80.0), (-180.0, 180.0), Colorization::None),
    ]
}

/// A calligraphic brush whose size follows pen pressure (or varies at random) by `variation` pt.
fn calli_varied(name: &str, angle: f64, roundness: f64, size: f64, variation: f64, mode: Variation) -> Brush {
    brush(
        name,
        BrushKind::Calligraphic(Calligraphic {
            angle,
            roundness,
            size,
            variation: [0.0, 0.0, variation],
            modes: Some([Variation::Fixed, Variation::Fixed, mode]),
        }),
    )
}

fn ink_calligraphy() -> Vec<Brush> {
    vec![
        calli("Fine Nib", 0.0, 100.0, 1.0),
        calli("Chisel", 30.0, 20.0, 5.0),
        calli("Broad Edge", 45.0, 8.0, 12.0),
        calli("Italic Nib", -40.0, 15.0, 7.0),
        calli_varied("Brush Pen", 0.0, 60.0, 4.0, 3.0, Variation::Pressure),
        calli_varied("Wobbly Marker", 20.0, 70.0, 5.0, 1.5, Variation::Random),
        calli("Soft Oval", -20.0, 55.0, 9.0),
    ]
}

/// A rough outline of `jitter` around y = 0, `half` pt either side, tapered at both ends.
fn rough_band(half: f64, jitter: &[f64], taper: f64) -> PathData {
    let n = 30;
    let edge = |i: usize, side: f64, k: usize| {
        let x = 100.0 * i as f64 / n as f64;
        let env = (PI * x / 100.0).sin().powf(taper);
        let j = jitter.get(k % jitter.len().max(1)).copied().unwrap_or(1.0);
        Point::new(x, side * half * env * j)
    };
    let mut pts: Vec<Point> = (0..=n).map(|i| edge(i, -1.0, i)).collect();
    pts.extend((1..n).rev().map(|i| edge(i, 1.0, i * 5 + 2)));
    PathData::single(SubPath::polyline(&pts, true))
}

fn paint_pencil() -> Vec<Brush> {
    const JITTER: [f64; 11] = [0.9, 1.0, 0.75, 0.95, 0.82, 1.0, 0.7, 0.88, 0.97, 0.8, 0.92];
    let streak = |y: f64, x0: f64, x1: f64, h: f64| black(shapes::ellipse(Rect::new(x0, y - h, x1, y + h)));
    let dry = group(vec![
        streak(-2.6, 0.0, 92.0, 0.45),
        streak(-1.5, 4.0, 100.0, 0.5),
        streak(-0.4, 0.0, 86.0, 0.4),
        streak(0.6, 8.0, 97.0, 0.55),
        streak(1.7, 2.0, 78.0, 0.4),
        streak(2.7, 12.0, 94.0, 0.35),
    ]);
    let marker = black(shapes::rounded_rectangle(Rect::new(0.0, -2.5, 100.0, 2.5), 2.5));
    let wash = group(vec![path(rough_band(4.0, &JITTER, 0.4), Color::gray(0.7)), path(rough_band(2.4, &JITTER[3..], 0.6), Color::gray(0.45))]);
    let ink = black(PathData::single(SubPath::new(
        vec![
            Anchor::with_handles(Point::new(0.0, 0.0), Point::new(0.0, 3.0), Point::new(0.0, -3.0)),
            Anchor::with_handles(Point::new(100.0, 0.0), Point::new(60.0, -0.4), Point::new(60.0, 0.4)),
        ],
        true,
    )));
    let bristle = |name: &str, shape: BristleShape, size: f64, density: f64, opacity: f64| {
        brush(name, BrushKind::Bristle(Bristle { shape, size, density, opacity, ..Default::default() }))
    };
    vec![
        art("Pencil", black(rough_band(0.8, &JITTER, 0.15)), ArtScale::Stretch),
        art("Dry Brush", dry, ArtScale::Stretch),
        art("Marker", marker, ArtScale::BetweenGuides { start: 0.03, end: 0.97 }),
        art("Watercolor Wash", wash, ArtScale::Stretch),
        art("Ink Taper", ink, ArtScale::Stretch),
        bristle("Fan Brush", BristleShape::FlatFan, 10.0, 70.0, 60.0),
        bristle("Round Blunt", BristleShape::RoundBlunt, 7.0, 60.0, 80.0),
        bristle("Flat Curve", BristleShape::FlatCurve, 8.0, 55.0, 70.0),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stroke_pieces;
    use vectorcraft_doc::StrokeLayer;
    use vectorcraft_geom::BezPath;

    #[test]
    fn every_builtin_library_draws_every_brush() {
        let mut bp = BezPath::new();
        bp.move_to((0.0, 0.0));
        bp.curve_to((40.0, -30.0), (80.0, 30.0), (120.0, 0.0));
        bp.line_to((120.0, 60.0));
        let stroke = StrokeLayer::new(vectorcraft_color::Paint::solid(Color::BLACK), 1.0);
        for b in BRUSH_LIBRARIES {
            let lib = builtin_brush_library(b.id).unwrap();
            assert_eq!(lib.name, b.name);
            assert!(lib.len() >= 6, "{}", b.id);
            for br in &lib.brushes {
                assert_eq!(lib.brushes.iter().filter(|x| x.name == br.name).count(), 1, "{} in {}", br.name, b.id);
                assert!(!stroke_pieces(br, &bp, &stroke).is_empty(), "{} in {} draws", br.name, b.id);
            }
        }
        assert!(builtin_brush_library("nope").is_none());
        // Made once.
        assert!(Arc::ptr_eq(&builtin_brush_library("arrows").unwrap(), &builtin_brush_library("arrows").unwrap()));
    }

    #[test]
    fn files_round_trip_and_bad_ones_are_refused() {
        let lib = builtin_brush_library("borders").unwrap();
        let text = write(&lib);
        assert!(sniff(&text));
        assert_eq!(read(&text, "x").unwrap(), *lib);
        let unnamed = text.replacen("\"name\": \"Borders\"", "\"name\": \"\"", 1);
        assert_eq!(read(&unnamed, "Mine").unwrap().name, "Mine");
        assert!(read("{\"format\": \"vcstyles\"}", "x").is_err());
        assert!(read("not json", "x").is_err() && !sniff("not json"));
        // Brushes it can't read are left out.
        let odd = r#"{"format": "vcbrushes", "version": 1, "name": "Odd", "brushes": [{"name": "A", "type": "calligraphic"}, {"name": "B", "type": "laser"}]}"#;
        assert_eq!(read(odd, "x").unwrap().brushes.iter().map(|b| b.name.as_str()).collect::<Vec<_>>(), ["A"]);
    }
}
