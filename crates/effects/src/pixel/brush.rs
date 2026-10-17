//! Brush Strokes › Accented Edges, Angled Strokes, Crosshatch, Dark Strokes, Ink Outlines,
//! Spatter, Sprayed Strokes and Sumi-e.
//!
//! The strokes are segments of lanes laid across the art in document space: each lane is
//! staggered by a hash of its index, and every pixel takes the average colour along its
//! segment. So the strokes stay on the art whatever the raster's origin, resolution or zoom.

use vectorcraft_geom::{Affine, Point, Vec2};

use super::blur::bilinear;
use super::noise::{cell, hash, unit, value};
use super::pixelate::centre;
use super::{PixelSpace, Px16, blur_plane, to8, to16};

/// Sprayed Strokes' directions, in the dialog's order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    RightDiagonal,
    Horizontal,
    LeftDiagonal,
    Vertical,
}

impl Direction {
    /// (id, label, direction) in the dialog's order.
    pub const KINDS: [(&'static str, &'static str, Direction); 4] = [
        ("rightDiagonal", "Right Diagonal", Direction::RightDiagonal),
        ("horizontal", "Horizontal", Direction::Horizontal),
        ("leftDiagonal", "Left Diagonal", Direction::LeftDiagonal),
        ("vertical", "Vertical", Direction::Vertical),
    ];

    fn vector(self) -> Vec2 {
        match self {
            Direction::RightDiagonal => RISING,
            Direction::Horizontal => Vec2::new(1.0, 0.0),
            Direction::LeftDiagonal => FALLING,
            Direction::Vertical => Vec2::new(0.0, 1.0),
        }
    }
}

/// Unit vectors along the two diagonals (document y runs down): rising to the right, and falling.
const RISING: Vec2 = Vec2::new(std::f64::consts::FRAC_1_SQRT_2, -std::f64::consts::FRAC_1_SQRT_2);
const FALLING: Vec2 = Vec2::new(std::f64::consts::FRAC_1_SQRT_2, std::f64::consts::FRAC_1_SQRT_2);

/// The most samples a stroke averages.
const MAX_SAMPLES: usize = 16;

/// The source raster, sampled at document points.
struct Source {
    buf: Vec<Px16>,
    w: usize,
    h: usize,
    from_doc: Affine,
    px: f64,
}

impl Source {
    fn new(px: &[[u8; 4]], w: usize, h: usize, space: &PixelSpace) -> Option<Source> {
        let from_doc = space.to_doc.inverse();
        (w > 0 && h > 0 && from_doc.is_finite() && space.px.is_finite() && space.px > 0.0).then(|| Source {
            buf: to16(px),
            w,
            h,
            from_doc,
            px: space.px,
        })
    }

    /// The colour at document point `p` (premultiplied, 0..65535; transparent beyond the edges).
    fn at(&self, p: Point) -> [f32; 4] {
        let q = self.from_doc * p;
        bilinear(&self.buf, self.w, self.h, q.x - 0.5, q.y - 0.5)
    }

    /// The average colour along the stroke through `p`: lanes `width` apart run along `dir`, cut
    /// into segments `len` long and staggered per lane by `salt`.
    fn stroke(&self, p: Point, dir: Vec2, len: f64, width: f64, salt: u64) -> [f32; 4] {
        if !(len.is_finite() && len > 0.0 && width.is_finite() && width > 0.0) {
            return self.at(p);
        }
        let across = Vec2::new(-dir.y, dir.x);
        let (along, side) = (p.to_vec2().dot(dir), p.to_vec2().dot(across));
        let lane = cell(side, width);
        let shift = unit(hash(lane, 0, salt)) * len;
        let mid = ((along + shift) / len).floor().mul_add(len, 0.5 * len) - shift;
        let base = p + dir * (mid - along);
        let n = ((len / self.px).ceil() as usize).clamp(2, MAX_SAMPLES);
        let mut sum = [0.0f32; 4];
        for k in 0..n {
            let t = (k as f64 + 0.5) / n as f64 - 0.5;
            let c = self.at(base + dir * (t * len));
            for (s, v) in sum.iter_mut().zip(c) {
                *s += v;
            }
        }
        sum.map(|s| s / n as f32)
    }
}

/// A premultiplied 16-bit colour's straight luma in [0, 1] (0 where transparent).
fn luma(c: [f32; 4]) -> f32 {
    if c[3] > 0.0 { ((0.299 * c[0] + 0.587 * c[1] + 0.114 * c[2]) / c[3]).clamp(0.0, 1.0) } else { 0.0 }
}

/// Moves a premultiplied colour `t` of the way to white (`t` > 0) or black (`t` < 0).
fn tint(c: [f32; 4], t: f32) -> [f32; 4] {
    let target = if t >= 0.0 { c[3] } else { 0.0 };
    let t = t.abs().min(1.0);
    [c[0] + (target - c[0]) * t, c[1] + (target - c[1]) * t, c[2] + (target - c[2]) * t, c[3]]
}

/// Stretches a premultiplied colour's channels away from mid grey by `k`.
fn contrast(c: [f32; 4], k: f32) -> [f32; 4] {
    let mid = 0.5 * c[3];
    [((c[0] - mid) * k + mid).clamp(0.0, c[3]), ((c[1] - mid) * k + mid).clamp(0.0, c[3]), ((c[2] - mid) * k + mid).clamp(0.0, c[3]), c[3]]
}

/// Writes a premultiplied 16-bit colour to an 8-bit pixel.
fn put(out: &mut [u8; 4], c: [f32; 4]) {
    let a = c[3].round().clamp(0.0, 65535.0);
    *out = [c[0], c[1], c[2], c[3]].map(|v| to8(v.round().clamp(0.0, a) as u16));
}

/// Runs `f` (document point, pixel index) → colour over every pixel.
fn each(px: &mut [[u8; 4]], w: usize, h: usize, space: &PixelSpace, f: impl Fn(Point, usize) -> [f32; 4]) {
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            let c = f(centre(space.to_doc, x, y), i);
            if let Some(out) = px.get_mut(i) {
                put(out, c);
            }
        }
    }
}

/// How strongly each pixel lies on an edge, in [0, 1]: the Sobel gradient of the luma after a
/// blur of `sigma` pixels.
fn edges(px: &[[u8; 4]], w: usize, h: usize, sigma: f64) -> Vec<f32> {
    let mut l: Vec<f32> = px.iter().map(|p| (0.299 * f32::from(p[0]) + 0.587 * f32::from(p[1]) + 0.114 * f32::from(p[2])) / 255.0).collect();
    blur_plane(&mut l, w, h, sigma);
    let at = |x: usize, y: usize| l.get(y * w + x).copied().unwrap_or(0.0);
    let mut out = Vec::with_capacity(l.len());
    for y in 0..h {
        for x in 0..w {
            let (xl, xr) = (x.saturating_sub(1), (x + 1).min(w.saturating_sub(1)));
            let (yu, yd) = (y.saturating_sub(1), (y + 1).min(h.saturating_sub(1)));
            let gx = at(xr, yu) + 2.0 * at(xr, y) + at(xr, yd) - at(xl, yu) - 2.0 * at(xl, y) - at(xl, yd);
            let gy = at(xl, yd) + 2.0 * at(x, yd) + at(xr, yd) - at(xl, yu) - 2.0 * at(x, yu) - at(xr, yu);
            out.push((gx.hypot(gy) * 0.5).min(1.0));
        }
    }
    out
}

/// Accented Edges: the edges (about `width` × 0.5 points wide, after smoothing by `smooth` × 0.25
/// points) lightened (`brightness` above 25, up to 50) or darkened (below 25).
fn accented_edges(px: &mut [[u8; 4]], w: usize, h: usize, space: &PixelSpace, width: f64, brightness: f64, smooth: f64) {
    if !(space.px.is_finite() && space.px > 0.0) {
        return;
    }
    let e = edges(px, w, h, (smooth * 0.25 + width * 0.25) / space.px);
    let push = ((brightness - 25.0) / 25.0).clamp(-1.0, 1.0) as f32;
    let src = to16(px);
    for ((out, c), e) in px.iter_mut().zip(&src).zip(&e) {
        let c = c.map(f32::from);
        put(out, tint(c, push * (e * 2.0).min(1.0)));
    }
}

/// Angled Strokes: the light tones painted in strokes along one diagonal and the dark tones along
/// the other, `len` points long; `balance` (0..100) moves the split, `sharp` (0..10) the contrast.
fn angled_strokes(px: &mut [[u8; 4]], w: usize, h: usize, space: &PixelSpace, balance: f64, len: f64, sharp: f64) {
    let Some(src) = Source::new(px, w, h, space) else { return };
    let split = (balance / 100.0).clamp(0.0, 1.0) as f32;
    let k = 1.0 + sharp as f32 * 0.05;
    each(px, w, h, space, |p, _| {
        let dir = if luma(src.at(p)) >= split { RISING } else { FALLING };
        contrast(src.stroke(p, dir, len, 1.0, 21), k)
    });
}

/// Crosshatch: the art redrawn in strokes `len` points long along both diagonals, `strength`
/// (1..3) passes deep, its contrast raised by `sharp` (0..20).
fn crosshatch(px: &mut [[u8; 4]], w: usize, h: usize, space: &PixelSpace, len: f64, sharp: f64, strength: u32) {
    let Some(src) = Source::new(px, w, h, space) else { return };
    let k = 1.0 + sharp as f32 * 0.04;
    let passes = strength.clamp(1, 3);
    each(px, w, h, space, |p, _| {
        let mut sum = [0.0f32; 4];
        for pass in 0..passes {
            for (dir, salt) in [(RISING, 31), (FALLING, 32)] {
                let c = src.stroke(p, dir, len * (1.0 + 0.5 * f64::from(pass)), 1.0, salt + u64::from(pass) * 2);
                for (s, v) in sum.iter_mut().zip(c) {
                    *s += v;
                }
            }
        }
        // Hatched lines: every other lane of each diagonal is drawn a little darker.
        let lane = |d: Vec2| (cell(p.to_vec2().dot(Vec2::new(-d.y, d.x)), 1.0)).rem_euclid(2) == 0;
        let ink = 0.06 * (f32::from(u8::from(lane(RISING))) + f32::from(u8::from(lane(FALLING))));
        let n = (passes * 2) as f32;
        tint(contrast(sum.map(|s| s / n), k), -ink)
    });
}

/// Dark Strokes: dark tones painted in short dark strokes along one diagonal and light tones in
/// long light strokes along the other; `balance` (0..10) sets the split, `black` and `white`
/// (0..10) how dark and light they go.
fn dark_strokes(px: &mut [[u8; 4]], w: usize, h: usize, space: &PixelSpace, balance: f64, black: f64, white: f64) {
    let Some(src) = Source::new(px, w, h, space) else { return };
    let split = (balance / 10.0).clamp(0.0, 1.0) as f32;
    each(px, w, h, space, |p, _| {
        if luma(src.at(p)) < split {
            tint(src.stroke(p, FALLING, 6.0, 1.0, 41), -(black as f32) * 0.08)
        } else {
            tint(src.stroke(p, RISING, 12.0, 1.0, 42), white as f32 * 0.08)
        }
    });
}

/// Ink Outlines: the art redrawn in strokes `len` points long, its edges inked black by `dark`
/// (0..50) and its flat areas lightened by `light` (0..50).
fn ink_outlines(px: &mut [[u8; 4]], w: usize, h: usize, space: &PixelSpace, len: f64, dark: f64, light: f64) {
    let Some(src) = Source::new(px, w, h, space) else { return };
    let e = edges(px, w, h, 0.5 / space.px);
    let (dark, light) = ((dark / 50.0).clamp(0.0, 1.0) as f32, (light / 50.0).clamp(0.0, 1.0) as f32);
    each(px, w, h, space, |p, i| {
        let edge = (e.get(i).copied().unwrap_or(0.0) * 2.0).min(1.0);
        let c = tint(src.stroke(p, RISING, len, 1.0, 51), light * 0.4 * (1.0 - edge));
        tint(c, -dark * edge)
    });
}

/// Each pixel taken from up to `radius` points away in a random direction, the scatter smoothed
/// by `smooth` (0..15; 0 = speckled, 15 = flowing).
fn spray(src: &Source, p: Point, radius: f64, smooth: f64, salt: u64) -> [f32; 4] {
    if !(radius.is_finite() && radius > 0.0) {
        return src.at(p);
    }
    let s = (smooth / 15.0).clamp(0.0, 1.0);
    let (i, j) = (cell(p.x, 0.5), cell(p.y, 0.5));
    let speck = |k: u64| unit(hash(i, j, salt + k)) * 2.0 - 1.0;
    let flow = |k: u64| value(p.x / 3.0, p.y / 3.0, salt + 10 + k) * 2.0 - 1.0;
    let d = Vec2::new((1.0 - s) * speck(0) + s * flow(0), (1.0 - s) * speck(1) + s * flow(1));
    src.at(p + d * radius)
}

/// Spatter: the art as if sprayed through an airbrush, scattered by up to `radius` × 0.5 points.
fn spatter(px: &mut [[u8; 4]], w: usize, h: usize, space: &PixelSpace, radius: f64, smooth: f64) {
    let Some(src) = Source::new(px, w, h, space) else { return };
    each(px, w, h, space, |p, _| spray(&src, p, radius * 0.5, smooth, 61));
}

/// Sprayed Strokes: the art in strokes `len` points long along `dir`, scattered by up to `radius`
/// × 0.5 points.
fn sprayed_strokes(px: &mut [[u8; 4]], w: usize, h: usize, space: &PixelSpace, len: f64, radius: f64, dir: Direction) {
    let Some(src) = Source::new(px, w, h, space) else { return };
    let v = dir.vector();
    each(px, w, h, space, |p, _| {
        let c = src.stroke(p, v, len.max(0.5), 1.0, 71);
        if radius > 0.0 {
            let s = spray(&src, p, radius * 0.5, 4.0, 72);
            std::array::from_fn(|k| 0.5 * (c[k] + s[k]))
        } else {
            c
        }
    });
}

/// Sumi-e: the art in wide, wet black-ink strokes `width` × 0.5 points wide along a diagonal,
/// pressed `pressure` (0..15) dark, with `contrast` (0..40).
fn sumi_e(px: &mut [[u8; 4]], w: usize, h: usize, space: &PixelSpace, width: f64, pressure: f64, contrast_: f64) {
    let Some(src) = Source::new(px, w, h, space) else { return };
    let lane = (width * 0.5).max(0.5);
    let k = 1.0 + contrast_ as f32 * 0.025;
    let ink = pressure as f32 * 0.04;
    each(px, w, h, space, |p, _| {
        let c = contrast(src.stroke(p, RISING, lane * 4.0, lane, 81), k);
        // Dark tones soak up the most ink.
        tint(c, -ink * (1.0 - luma(c)))
    });
}

/// A Brush Strokes filter with its parameters read and clamped to their ranges.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BrushStroke {
    AccentedEdges { width: f64, brightness: f64, smoothness: f64 },
    AngledStrokes { balance: f64, length: f64, sharpness: f64 },
    Crosshatch { length: f64, sharpness: f64, strength: u32 },
    DarkStrokes { balance: f64, black: f64, white: f64 },
    InkOutlines { length: f64, dark: f64, light: f64 },
    Spatter { radius: f64, smoothness: f64 },
    SprayedStrokes { length: f64, radius: f64, direction: Direction },
    SumiE { width: f64, pressure: f64, contrast: f64 },
}

impl BrushStroke {
    /// How far (points) the content a pixel takes can lie from it.
    pub(super) fn reach(&self) -> f64 {
        match *self {
            BrushStroke::AccentedEdges { width, smoothness, .. } => (smoothness * 0.25 + width * 0.25) * 3.0 + 1.0,
            // A stroke through a pixel can run its whole length to either side of it.
            BrushStroke::AngledStrokes { length, .. } | BrushStroke::InkOutlines { length, .. } => length,
            BrushStroke::Crosshatch { length, strength, .. } => length * (1.0 + 0.5 * f64::from(strength.saturating_sub(1))),
            BrushStroke::DarkStrokes { .. } => 12.0,
            BrushStroke::Spatter { radius, .. } => radius * 0.5,
            BrushStroke::SprayedStrokes { length, radius, .. } => length.max(0.5) + radius * 0.5,
            BrushStroke::SumiE { width, .. } => (width * 0.5).max(0.5) * 4.0,
        }
    }

    /// How far (points) beyond the content the filter can paint.
    pub(super) fn outset(&self) -> f64 {
        match self {
            BrushStroke::AccentedEdges { .. } => 0.0,
            _ => self.reach(),
        }
    }

    pub(super) fn apply(&self, px: &mut [[u8; 4]], w: usize, h: usize, space: &PixelSpace) {
        match *self {
            BrushStroke::AccentedEdges { width, brightness, smoothness } => accented_edges(px, w, h, space, width, brightness, smoothness),
            BrushStroke::AngledStrokes { balance, length, sharpness } => angled_strokes(px, w, h, space, balance, length, sharpness),
            BrushStroke::Crosshatch { length, sharpness, strength } => crosshatch(px, w, h, space, length, sharpness, strength),
            BrushStroke::DarkStrokes { balance, black, white } => dark_strokes(px, w, h, space, balance, black, white),
            BrushStroke::InkOutlines { length, dark, light } => ink_outlines(px, w, h, space, length, dark, light),
            BrushStroke::Spatter { radius, smoothness } => spatter(px, w, h, space, radius, smoothness),
            BrushStroke::SprayedStrokes { length, radius, direction } => sprayed_strokes(px, w, h, space, length, radius, direction),
            BrushStroke::SumiE { width, pressure, contrast } => sumi_e(px, w, h, space, width, pressure, contrast),
        }
    }
}
