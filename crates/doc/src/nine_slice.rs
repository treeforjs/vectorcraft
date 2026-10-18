//! 9-slice scaling for symbols: four guides cut a symbol's art into a 3×3 grid. When an instance
//! is scaled, the corners keep their size, the edges stretch along one axis and the centre
//! stretches along both (the corners shrink together only when the instance is smaller than
//! they are).

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use vectorcraft_geom::{Affine, Anchor, AnchorKind, CubicBez, Line, ParamCurve, PathData, PathSeg, Point, Rect, SubPath};

use crate::{Document, Node, NodeKind, Symbol};

/// Where the 9-slice guides sit: their distance in points, at the symbol's natural size, from
/// the left, top, right and bottom edges of its art.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct NineSlice {
    pub left: f64,
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
}

impl NineSlice {
    /// Guides a quarter of the way in from each edge of art `w` × `h` points (where new guides
    /// start).
    pub fn quarter(w: f64, h: f64) -> Self {
        let (w, h) = (w.max(0.0) / 4.0, h.max(0.0) / 4.0);
        Self { left: w, top: h, right: w, bottom: h }
    }
    /// Are all four distances finite and not negative?
    pub fn is_valid(&self) -> bool {
        [self.left, self.top, self.right, self.bottom].iter().all(|v| v.is_finite() && *v >= 0.0)
    }
}

/// How one axis maps: the art spans `b0..b1` in symbol space and its guides sit at `g0` and
/// `g1`. A coordinate in a band (before, between or after the guides) maps as `x * k + o`.
#[derive(Clone, Copy, Debug)]
struct Axis {
    g0: f64,
    g1: f64,
    bands: [(f64, f64); 3],
}

impl Axis {
    /// The axis of art spanning `b0..b1` symbol units, with guides `lo` and `hi` points in from
    /// its ends at `n` points per unit (its natural size), drawn at `s` points per unit. `None`
    /// when nothing would change (the instance is at its natural size along this axis) or the
    /// numbers don't make a grid.
    fn new(b0: f64, b1: f64, lo: f64, hi: f64, n: f64, s: f64) -> Option<Self> {
        let span = b1 - b0;
        if !(span > 1e-9 && n > 1e-12 && s > 1e-12) || ((s - n) / n).abs() < 1e-9 {
            return None;
        }
        let natural = span * n;
        if !(lo >= 0.0 && hi >= 0.0 && lo + hi <= natural * (1.0 + 1e-9)) {
            return None;
        }
        let (g0, g1) = (b0 + lo / n, (b1 - hi / n).max(b0 + lo / n));
        let drawn = span * s;
        // The corners keep their size while they fit, and shrink together when they don't.
        let (k, mid) = if lo + hi <= drawn { (1.0, drawn - lo - hi) } else { (drawn / (lo + hi), 0.0) };
        // Positions in points from the left edge as drawn, then back to symbol units (the
        // instance's own scale `s` still applies after this map).
        let start = b0 * s;
        let first = (n * k, start - b0 * n * k);
        let at_g0 = start + lo * k;
        let mid_k = if g1 - g0 > 1e-12 { mid / (g1 - g0) } else { 0.0 };
        let middle = (mid_k, at_g0 - g0 * mid_k);
        let at_g1 = at_g0 + mid;
        let last = (n * k, at_g1 - g1 * n * k);
        let back = |(k, o): (f64, f64)| (k / s, o / s);
        Some(Self { g0, g1, bands: [back(first), back(middle), back(last)] })
    }
    fn band(&self, x: f64) -> (f64, f64) {
        let [a, b, c] = self.bands;
        if x < self.g0 {
            a
        } else if x <= self.g1 {
            b
        } else {
            c
        }
    }
    fn identity() -> Self {
        Self { g0: f64::NEG_INFINITY, g1: f64::INFINITY, bands: [(1.0, 0.0); 3] }
    }
}

/// The 9-slice map of one instance, in the symbol's own space.
#[derive(Clone, Copy, Debug)]
struct Grid {
    x: Axis,
    y: Axis,
}

impl Grid {
    /// The affine map of the cell holding `p`.
    fn at(&self, p: Point) -> Affine {
        let ((kx, ox), (ky, oy)) = (self.x.band(p.x), self.y.band(p.y));
        Affine::new([kx, 0.0, 0.0, ky, ox, oy])
    }
    fn guides_x(&self) -> [f64; 2] {
        [self.x.g0, self.x.g1]
    }
    fn guides_y(&self) -> [f64; 2] {
        [self.y.g0, self.y.g1]
    }
}

/// The scale an instance transform applies along the symbol's own x and y axes (after which it
/// only rotates, flips and moves). `None` when it skews or collapses the art.
fn axis_scales(xf: Affine) -> Option<(f64, f64)> {
    let [a, b, c, d, _, _] = xf.as_coeffs();
    let sx = a.hypot(b);
    let sy = c.hypot(d);
    if !(sx > 1e-12 && sy > 1e-12) || !sx.is_finite() || !sy.is_finite() {
        return None;
    }
    // Perpendicular axes only: a skewed instance draws its art as it is.
    ((a * c + b * d).abs() <= 1e-6 * sx * sy).then_some((sx, sy))
}

impl Symbol {
    /// The art an instance placed by `xf` shows, in symbol space (the caller still moves it by
    /// `xf`): with 9-slice scaling on, its corners keep the size they have at the symbol's
    /// natural scale `natural` (points per symbol unit along x and y) however `xf` scales it.
    pub fn art_for(&self, xf: Affine, natural: (f64, f64)) -> Arc<Node> {
        self.sliced_art(xf, natural).map(Arc::new).unwrap_or_else(|| self.art.clone())
    }

    fn sliced_art(&self, xf: Affine, (nx, ny): (f64, f64)) -> Option<Node> {
        let slice = self.slice.filter(NineSlice::is_valid)?;
        let (sx, sy) = axis_scales(xf)?;
        let b = self.art.geometric_bounds()?;
        let x = Axis::new(b.x0, b.x1, slice.left, slice.right, nx, sx);
        let y = Axis::new(b.y0, b.y1, slice.top, slice.bottom, ny, sy);
        if x.is_none() && y.is_none() {
            return None;
        }
        let grid = Grid { x: x.unwrap_or_else(Axis::identity), y: y.unwrap_or_else(Axis::identity) };
        Some(map_node(&self.art, &grid, 0))
    }
}

impl Document {
    /// The art symbol instance `inst` shows in symbol space ([`Symbol::art_for`] at the
    /// symbol's natural size). `None` for other objects and missing symbols.
    pub fn symbol_art_for(&self, inst: &Node) -> Option<Arc<Node>> {
        let NodeKind::SymbolInstance { symbol, xf } = &inst.kind else { return None };
        let s = self.symbols.iter().find(|s| s.name == *symbol)?;
        let n = self.symbol_natural_xf(symbol).as_coeffs();
        Some(s.art_for(*xf, (n[0].abs(), n[3].abs())))
    }
}

/// How deep groups may nest inside a symbol's art before the rest moves as one piece.
const MAX_DEPTH: u32 = 64;

fn map_node(n: &Node, g: &Grid, depth: u32) -> Node {
    let mut out = n.clone();
    let centre = n.geometric_bounds().map(|b| b.center());
    if let Some(m) = &mut out.mask {
        m.art = Arc::new(map_node(&m.art, g, depth + 1));
    }
    match &mut out.kind {
        NodeKind::Path { path, live, .. } => {
            *path = map_path(path, g);
            *live = None;
        }
        NodeKind::Layer { children, .. } | NodeKind::Group { children, .. } | NodeKind::Compound { children, .. } if depth < MAX_DEPTH => {
            for c in children.iter_mut() {
                *c = Arc::new(map_node(c, g, depth + 1));
            }
        }
        _ => {
            // Type, images, symbols and live art move with the cell their middle is in.
            if let Some(c) = centre {
                out.transform(g.at(c), false);
            }
            return out;
        }
    }
    out.appearance.warp_gradients(&|p| g.at(p));
    out
}

/// `path` cut where it crosses the guides, each piece moved by the map of its cell.
fn map_path(path: &PathData, g: &Grid) -> PathData {
    PathData::new(path.subpaths.iter().map(|sp| map_subpath(sp, g)).collect())
}

fn map_subpath(sp: &SubPath, g: &Grid) -> SubPath {
    let n = sp.anchors.len();
    if sp.segment_count() == 0 {
        let anchors = sp.anchors.iter().map(|a| map_anchor(a, g.at(a.p))).collect();
        return SubPath::new(anchors, sp.closed);
    }
    let mut anchors: Vec<Anchor> = Vec::with_capacity(n);
    let Some(first) = sp.anchors.first() else { return sp.clone() };
    anchors.push(Anchor { kind: first.kind, ..map_anchor(first, g.at(first.p)) });
    for i in 0..sp.segment_count() {
        let c = sp.segment(i);
        let line = sp.segment_is_line(i);
        let end = sp.anchors.get((i + 1) % n).map_or(AnchorKind::Corner, |a| a.kind);
        let cuts = cuts(c, g);
        let mut t0 = 0.0;
        for (j, &t1) in cuts.iter().chain(std::iter::once(&1.0)).enumerate() {
            let piece = c.subsegment(t0..t1);
            let a = g.at(c.eval((t0 + t1) / 2.0));
            let last = j == cuts.len();
            if let Some(prev) = anchors.last_mut() {
                prev.h_out = if line { prev.p } else { a * piece.p1 };
            }
            let p = a * piece.p3;
            let kind = if last {
                end
            } else if line {
                AnchorKind::Corner
            } else {
                AnchorKind::Smooth
            };
            anchors.push(Anchor { p, h_in: if line { p } else { a * piece.p2 }, h_out: p, kind });
            t0 = t1;
        }
    }
    if sp.closed && anchors.len() > 1 {
        // The closing segment ended back on the first anchor: its handle goes there.
        if let Some(back) = anchors.pop()
            && let Some(f) = anchors.first_mut()
        {
            f.h_in = back.h_in;
        }
    } else if let Some(last) = anchors.last_mut()
        && let Some(orig) = sp.anchors.last()
    {
        // An open path's end keeps its own outgoing handle.
        last.h_out = g.at(orig.p) * orig.h_out;
    }
    SubPath::new(anchors, sp.closed)
}

fn map_anchor(a: &Anchor, m: Affine) -> Anchor {
    Anchor { p: m * a.p, h_in: m * a.h_in, h_out: m * a.h_out, kind: a.kind }
}

/// Where along `c` (strictly inside) it crosses a guide, in order.
fn cuts(c: CubicBez, g: &Grid) -> Vec<f64> {
    let seg = PathSeg::Cubic(c);
    let bb = c.p0.x.min(c.p1.x).min(c.p2.x).min(c.p3.x)..=c.p0.x.max(c.p1.x).max(c.p2.x).max(c.p3.x);
    let bby = c.p0.y.min(c.p1.y).min(c.p2.y).min(c.p3.y)..=c.p0.y.max(c.p1.y).max(c.p2.y).max(c.p3.y);
    let reach = Rect::new(*bb.start(), *bby.start(), *bb.end(), *bby.end()).inflate(1.0, 1.0);
    let mut ts: Vec<f64> = Vec::new();
    for x in g.guides_x().into_iter().filter(|x| x.is_finite() && bb.contains(x)) {
        let l = Line::new((x, reach.y0), (x, reach.y1));
        ts.extend(seg.intersect_line(l).into_iter().map(|i| i.segment_t));
    }
    for y in g.guides_y().into_iter().filter(|y| y.is_finite() && bby.contains(y)) {
        let l = Line::new((reach.x0, y), (reach.x1, y));
        ts.extend(seg.intersect_line(l).into_iter().map(|i| i.segment_t));
    }
    ts.retain(|t| t.is_finite() && *t > 1e-9 && *t < 1.0 - 1e-9);
    ts.sort_by(f64::total_cmp);
    ts.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
    ts
}
