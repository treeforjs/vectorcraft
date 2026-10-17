//! Pixelate › Color Halftone, Crystallize, Mezzotint and Pointillize.
//!
//! Their cells, dots and grains lie on grids in document space, so the pattern stays put on the
//! art whatever the raster's origin, resolution or the view's zoom. Random placement comes from a
//! hash of the cell, so every render of the same art is the same.

use std::collections::HashMap;

use vectorcraft_geom::{Affine, Point};

use super::PixelSpace;
use super::noise::{cell, hash, unit};

/// Mezzotint's grain kinds, in the dialog's order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mezzotint {
    FineDots,
    MediumDots,
    GrainyDots,
    CoarseDots,
    ShortLines,
    MediumLines,
    LongLines,
    ShortStrokes,
    MediumStrokes,
    LongStrokes,
}

impl Mezzotint {
    /// (id, label) in the dialog's order.
    pub const KINDS: [(&'static str, &'static str, Mezzotint); 10] = [
        ("fineDots", "Fine Dots", Mezzotint::FineDots),
        ("mediumDots", "Medium Dots", Mezzotint::MediumDots),
        ("grainyDots", "Grainy Dots", Mezzotint::GrainyDots),
        ("coarseDots", "Coarse Dots", Mezzotint::CoarseDots),
        ("shortLines", "Short Lines", Mezzotint::ShortLines),
        ("mediumLines", "Medium Lines", Mezzotint::MediumLines),
        ("longLines", "Long Lines", Mezzotint::LongLines),
        ("shortStrokes", "Short Strokes", Mezzotint::ShortStrokes),
        ("mediumStrokes", "Medium Strokes", Mezzotint::MediumStrokes),
        ("longStrokes", "Long Strokes", Mezzotint::LongStrokes),
    ];

    /// The grain's size in points: (across, along), and whether it runs diagonally.
    fn grain(self) -> (f64, f64, bool) {
        match self {
            Mezzotint::FineDots => (0.5, 0.5, false),
            Mezzotint::MediumDots => (1.0, 1.0, false),
            Mezzotint::GrainyDots => (0.75, 0.75, false),
            Mezzotint::CoarseDots => (2.0, 2.0, false),
            Mezzotint::ShortLines => (0.5, 3.0, false),
            Mezzotint::MediumLines => (0.5, 6.0, false),
            Mezzotint::LongLines => (0.5, 12.0, false),
            Mezzotint::ShortStrokes => (0.5, 3.0, true),
            Mezzotint::MediumStrokes => (0.5, 6.0, true),
            Mezzotint::LongStrokes => (0.5, 12.0, true),
        }
    }
}

/// The jittered seed point of grid cell (`i`, `j`), document coordinates.
fn seed(i: i64, j: i64, size: f64, salt: u64) -> Point {
    let h = hash(i, j, salt);
    let (u, v) = (unit(h), unit(hash(i, j, h)));
    Point::new((i as f64 + 0.1 + 0.8 * u) * size, (j as f64 + 0.1 + 0.8 * v) * size)
}

/// The document point at the centre of pixel (`x`, `y`).
fn centre(to_doc: Affine, x: usize, y: usize) -> Point {
    to_doc * Point::new(x as f64 + 0.5, y as f64 + 0.5)
}

/// The pixel holding document point `p`, if it lies on the raster.
fn pixel_at(from_doc: Affine, p: Point, w: usize, h: usize) -> Option<usize> {
    let q = from_doc * p;
    let (x, y) = (q.x.floor(), q.y.floor());
    (x >= 0.0 && y >= 0.0 && x < w as f64 && y < h as f64).then(|| y as usize * w + x as usize)
}

/// The raster's pixel nearest document point `p` (`None` for an empty raster or a point at no
/// finite place).
fn nearest_pixel(from_doc: Affine, p: Point, w: usize, h: usize) -> Option<usize> {
    let q = from_doc * p;
    if w == 0 || h == 0 || !(q.x.is_finite() && q.y.is_finite()) {
        return None;
    }
    let (x, y) = (q.x.floor().clamp(0.0, (w - 1) as f64), q.y.floor().clamp(0.0, (h - 1) as f64));
    Some(y as usize * w + x as usize)
}

/// A usable grid size: finite, positive and not so small that a pixel spans millions of cells.
fn usable(size: f64, space: &PixelSpace) -> Option<f64> {
    (size.is_finite() && size > 0.0 && space.px.is_finite() && space.px > 0.0).then(|| size.max(space.px * 0.25))
}

/// Crystallize: each pixel takes the average colour of its cell of a Voronoi diagram over
/// jittered seeds `size` points apart.
pub(super) fn crystallize(px: &mut [[u8; 4]], w: usize, h: usize, space: &PixelSpace, size: f64) {
    let Some(size) = usable(size, space) else { return };
    let owner = |p: Point| {
        let (ci, cj) = (cell(p.x, size), cell(p.y, size));
        let mut best = ((ci, cj), f64::INFINITY);
        for dj in -1..=1 {
            for di in -1..=1 {
                let (i, j) = (ci.saturating_add(di), cj.saturating_add(dj));
                let d = (seed(i, j, size, 1) - p).hypot2();
                if d < best.1 {
                    best = ((i, j), d);
                }
            }
        }
        best.0
    };
    let owners: Vec<(i64, i64)> = (0..h).flat_map(|y| (0..w).map(move |x| (x, y))).map(|(x, y)| owner(centre(space.to_doc, x, y))).collect();
    let mut sums: HashMap<(i64, i64), ([u64; 4], u64)> = HashMap::new();
    for (p, o) in px.iter().zip(&owners) {
        let e = sums.entry(*o).or_insert(([0; 4], 0));
        for (a, v) in e.0.iter_mut().zip(p) {
            *a += u64::from(*v);
        }
        e.1 += 1;
    }
    for (p, o) in px.iter_mut().zip(&owners) {
        if let Some((sum, n)) = sums.get(o) {
            let n = (*n).max(1);
            *p = sum.map(|s| ((s + n / 2) / n).min(255) as u8);
        }
    }
}

/// How much of a pixel `px` points wide a disc's edge `d - r` points away covers (a one-pixel
/// ramp across the edge).
fn coverage(d: f64, r: f64, px: f64) -> f64 {
    ((r - d) / px.max(1e-12) + 0.5).clamp(0.0, 1.0)
}

/// Pointillize: dots of the colour under their centres, `size` points apart and jittered, on a
/// white background where the art is.
pub(super) fn pointillize(px: &mut [[u8; 4]], w: usize, h: usize, space: &PixelSpace, size: f64) {
    let Some(size) = usable(size, space) else { return };
    let from_doc = space.to_doc.inverse();
    let src = px.to_vec();
    let radius = 0.6 * size;
    let mut colours: HashMap<(i64, i64), Option<[u8; 4]>> = HashMap::new();
    for y in 0..h {
        for x in 0..w {
            let p = centre(space.to_doc, x, y);
            let (ci, cj) = (cell(p.x, size), cell(p.y, size));
            // The nearest dot covering the pixel draws it.
            let mut best: Option<(f64, f64, [u8; 4])> = None;
            for dj in -1..=1 {
                for di in -1..=1 {
                    let (i, j) = (ci.saturating_add(di), cj.saturating_add(dj));
                    let s = seed(i, j, size, 2);
                    let d = (s - p).hypot();
                    let c = coverage(d, radius, space.px);
                    if c <= 0.0 || best.is_some_and(|b| b.0 <= d) {
                        continue;
                    }
                    let colour = *colours.entry((i, j)).or_insert_with(|| pixel_at(from_doc, s, w, h).and_then(|k| src.get(k).copied()));
                    if let Some(colour) = colour.filter(|c| c[3] > 0) {
                        best = Some((d, c, colour));
                    }
                }
            }
            let (Some(out), Some(orig)) = (px.get_mut(y * w + x), src.get(y * w + x)) else { continue };
            // White where the art is, premultiplied by its coverage, under the dot.
            let bg = [orig[3]; 4];
            *out = match best {
                Some((_, c, dot)) => {
                    let keep = 1.0 - c * f64::from(dot[3]) / 255.0;
                    std::array::from_fn(|k| (f64::from(bg[k]) * keep + f64::from(dot[k]) * c).round().clamp(0.0, 255.0) as u8)
                }
                None => bg,
            };
        }
    }
}

/// Mezzotint: each colour channel of each grain (sized by `kind`, in points) becomes full or
/// none against a random threshold, so the image turns into saturated specks or streaks.
pub(super) fn mezzotint(px: &mut [[u8; 4]], w: usize, h: usize, space: &PixelSpace, kind: Mezzotint) {
    let (across, along, diagonal) = kind.grain();
    let (Some(across), Some(along)) = (usable(across, space), usable(along, space)) else { return };
    // Strokes run at 45°: grains measured along the diagonals.
    let turn = if diagonal { Affine::rotate(std::f64::consts::FRAC_PI_4) } else { Affine::IDENTITY };
    let contrast = if kind == Mezzotint::GrainyDots { 2.0 } else { 1.0 };
    for y in 0..h {
        for x in 0..w {
            let p = turn * centre(space.to_doc, x, y);
            let (i, j) = (cell(p.x, along), cell(p.y, across));
            let Some(out) = px.get_mut(y * w + x) else { continue };
            let a = f64::from(out[3]);
            if a <= 0.0 {
                continue;
            }
            for (k, c) in out.iter_mut().take(3).enumerate() {
                let v = f64::from(*c) / a;
                let t = unit(hash(i, j, 3 + k as u64));
                // Grainy dots push the tones apart before the threshold, for harder grain.
                let v = ((v - 0.5) * contrast + 0.5).clamp(0.0, 1.0);
                *c = if v > t { out_alpha(a) } else { 0 };
            }
        }
    }
}

fn out_alpha(a: f64) -> u8 {
    a.round().clamp(0.0, 255.0) as u8
}

/// The radius of a halftone dot covering `level` of its cell (2`radius` across): a disc of that
/// area while the dots stay apart, then growing to `full` (the cell's corners) as they merge.
fn dot_radius(level: f64, radius: f64, full: f64) -> f64 {
    let apart = std::f64::consts::FRAC_PI_4;
    if level <= apart {
        2.0 * radius * (level / std::f64::consts::PI).sqrt()
    } else {
        radius + (full - radius) * ((level - apart) / (1.0 - apart)).min(1.0)
    }
}

/// Color Halftone: each colour channel as dots on its own screen, turned by the channel's angle
/// (degrees), dots up to `radius` points with an area following the channel's level.
pub(super) fn halftone(px: &mut [[u8; 4]], w: usize, h: usize, space: &PixelSpace, radius: f64, angles: [f64; 3]) {
    let Some(radius) = usable(radius, space) else { return };
    let from_doc = space.to_doc.inverse();
    let src = px.to_vec();
    let cell_size = 2.0 * radius;
    // A dot as large as the cell's diagonal covers it whole.
    let full = radius * std::f64::consts::SQRT_2;
    for (k, angle) in angles.iter().enumerate() {
        let angle = if angle.is_finite() { angle.to_radians() } else { 0.0 };
        let (turn, back) = (Affine::rotate(-angle), Affine::rotate(angle));
        let mut levels: HashMap<(i64, i64), f64> = HashMap::new();
        for y in 0..h {
            for x in 0..w {
                let p = turn * centre(space.to_doc, x, y);
                let (ci, cj) = (cell(p.x, cell_size), cell(p.y, cell_size));
                let mut cover = 0.0f64;
                for dj in -1..=1 {
                    for di in -1..=1 {
                        let (i, j) = (ci.saturating_add(di), cj.saturating_add(dj));
                        let c = Point::new((i as f64 + 0.5) * cell_size, (j as f64 + 0.5) * cell_size);
                        let level = *levels.entry((i, j)).or_insert_with(|| {
                            // Screen cells past the raster's edge take the colour at the edge.
                            nearest_pixel(from_doc, back * c, w, h)
                                .and_then(|n| src.get(n))
                                .map_or(0.0, |s| if s[3] == 0 { 0.0 } else { f64::from(s[k]) / f64::from(s[3]) })
                        });
                        if level > 0.0 {
                            cover = cover.max(coverage((c - p).hypot(), dot_radius(level, radius, full + space.px), space.px));
                        }
                    }
                }
                if let Some(out) = px.get_mut(y * w + x) {
                    let a = f64::from(out[3]);
                    if let Some(c) = out.get_mut(k) {
                        *c = (cover * a).round().clamp(0.0, 255.0) as u8;
                    }
                }
            }
        }
    }
}
