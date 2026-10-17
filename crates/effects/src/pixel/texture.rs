//! Texture › Craquelure, Grain, Mosaic Tiles, Patchwork, Stained Glass and Texturizer.
//!
//! Cracks, tiles, cells, grain and textures lie on grids in document space, so they stay put on
//! the art whatever the raster's origin, resolution or zoom. The relief filters shade a height
//! field (0 = low, 1 = high, defined per document point) as if lit from one side.

use std::collections::HashMap;

use vectorcraft_geom::{Point, Vec2};

use super::PixelSpace;
use super::noise::{cell, hash, unit, value};
use super::pixelate::{centre, seed, usable};

/// Where the light comes from, in the dialog's order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Light {
    Bottom,
    BottomLeft,
    Left,
    TopLeft,
    Top,
    TopRight,
    Right,
    BottomRight,
}

impl Light {
    /// (id, label, light) in the dialog's order.
    pub const KINDS: [(&'static str, &'static str, Light); 8] = [
        ("bottom", "Bottom", Light::Bottom),
        ("bottomLeft", "Bottom Left", Light::BottomLeft),
        ("left", "Left", Light::Left),
        ("topLeft", "Top Left", Light::TopLeft),
        ("top", "Top", Light::Top),
        ("topRight", "Top Right", Light::TopRight),
        ("right", "Right", Light::Right),
        ("bottomRight", "Bottom Right", Light::BottomRight),
    ];

    /// The unit vector towards the light, in raster pixel axes (y down).
    pub(super) fn towards(self) -> Vec2 {
        let d = std::f64::consts::FRAC_1_SQRT_2;
        match self {
            Light::Bottom => Vec2::new(0.0, 1.0),
            Light::BottomLeft => Vec2::new(-d, d),
            Light::Left => Vec2::new(-1.0, 0.0),
            Light::TopLeft => Vec2::new(-d, -d),
            Light::Top => Vec2::new(0.0, -1.0),
            Light::TopRight => Vec2::new(d, -d),
            Light::Right => Vec2::new(1.0, 0.0),
            Light::BottomRight => Vec2::new(d, d),
        }
    }
}

/// Grain's kinds, in the dialog's order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Grain {
    Regular,
    Soft,
    Sprinkles,
    Clumped,
    Contrasty,
    Enlarged,
    Stippled,
    Horizontal,
    Vertical,
    Speckle,
}

impl Grain {
    /// (id, label, kind) in the dialog's order.
    pub const KINDS: [(&'static str, &'static str, Grain); 10] = [
        ("regular", "Regular", Grain::Regular),
        ("soft", "Soft", Grain::Soft),
        ("sprinkles", "Sprinkles", Grain::Sprinkles),
        ("clumped", "Clumped", Grain::Clumped),
        ("contrasty", "Contrasty", Grain::Contrasty),
        ("enlarged", "Enlarged", Grain::Enlarged),
        ("stippled", "Stippled", Grain::Stippled),
        ("horizontal", "Horizontal", Grain::Horizontal),
        ("vertical", "Vertical", Grain::Vertical),
        ("speckle", "Speckle", Grain::Speckle),
    ];

    /// The grain's cell in points: (across, down).
    fn size(self) -> (f64, f64) {
        match self {
            Grain::Soft => (1.0, 1.0),
            Grain::Clumped => (2.0, 2.0),
            Grain::Enlarged => (1.5, 1.5),
            Grain::Horizontal => (6.0, 0.5),
            Grain::Vertical => (0.5, 6.0),
            _ => (0.5, 0.5),
        }
    }
}

/// Texturizer's surfaces, in the dialog's order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Surface {
    Brick,
    Burlap,
    Canvas,
    Sandstone,
}

impl Surface {
    /// (id, label, surface) in the dialog's order.
    pub const KINDS: [(&'static str, &'static str, Surface); 4] = [
        ("brick", "Brick", Surface::Brick),
        ("burlap", "Burlap", Surface::Burlap),
        ("canvas", "Canvas", Surface::Canvas),
        ("sandstone", "Sandstone", Surface::Sandstone),
    ];

    /// The surface's height in [0, 1] at document point `p`, its features `scale` × their usual
    /// size.
    pub(super) fn height(self, p: Point, scale: f64) -> f64 {
        let (x, y) = (p.x / scale, p.y / scale);
        match self {
            Surface::Brick => {
                let (bw, bh, mortar) = (12.0, 5.0, 0.8);
                let row = (y / bh).floor();
                let shift = if row.rem_euclid(2.0) == 1.0 { bw * 0.5 } else { 0.0 };
                let (u, v) = ((x + shift).rem_euclid(bw), y.rem_euclid(bh));
                let edge = u.min(bw - u).min(v).min(bh - v);
                smooth((edge - mortar * 0.5) / 0.6) * (0.85 + 0.15 * value(x / 2.0, y / 2.0, 61))
            }
            Surface::Burlap => weave(x, y, 2.5) * 0.8 + 0.2 * value(x / 1.5, y / 1.5, 62),
            Surface::Canvas => weave(x, y, 1.2) * 0.9 + 0.1 * value(x, y, 63),
            Surface::Sandstone => (value(x / 1.5, y / 1.5, 64) + 0.5 * value(x / 0.75, y / 0.75, 65)) / 1.5,
        }
    }
}

/// Texturizer's parameters.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Texturizer {
    pub surface: Surface,
    /// The surface's size, 1 = 100 %.
    pub scaling: f64,
    pub relief: f64,
    pub light: Light,
    pub invert: bool,
}

/// 0 below 0, 1 above 1, an S-curve between.
pub(super) fn smooth(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Threads `s` apart woven over and under each other.
fn weave(x: f64, y: f64, s: f64) -> f64 {
    let t = std::f64::consts::TAU / s;
    let over = (cell(x, s).wrapping_add(cell(y, s))).rem_euclid(2) == 0;
    // Along a thread its height rises and falls; across it, it is round.
    if over { 0.5 + 0.5 * (t * y).sin().abs() * (0.6 + 0.4 * (t * x * 0.5).cos()) } else { 0.5 * (t * x).sin().abs() }
}

/// The height field `f` sampled at every pixel's centre.
pub(super) fn heights(w: usize, h: usize, space: &PixelSpace, f: impl Fn(Point) -> f64) -> Vec<f64> {
    let mut out = Vec::with_capacity(w.saturating_mul(h));
    for y in 0..h {
        for x in 0..w {
            let v = f(centre(space.to_doc, x, y));
            out.push(if v.is_finite() { v } else { 0.0 });
        }
    }
    out
}

/// Shades `px` by the slopes of `heights` (one per pixel) lit from `light`: a slope rising by one
/// over a point facing the light brightens by `strength`, one facing away darkens as much.
pub(super) fn relief(px: &mut [[u8; 4]], w: usize, h: usize, space: &PixelSpace, heights: &[f64], strength: f64, light: Light) {
    if w == 0 || h == 0 || !(space.px.is_finite() && space.px > 0.0 && strength.is_finite()) || strength == 0.0 {
        return;
    }
    let l = light.towards();
    let at = |x: usize, y: usize| heights.get(y * w + x).copied().unwrap_or(0.0);
    for y in 0..h {
        for x in 0..w {
            let (xl, xr) = (x.saturating_sub(1), (x + 1).min(w - 1));
            let (yu, yd) = (y.saturating_sub(1), (y + 1).min(h - 1));
            let run = |a: usize, b: usize| (b - a).max(1) as f64 * space.px;
            let slope = Vec2::new((at(xr, y) - at(xl, y)) / run(xl, xr), (at(x, yd) - at(x, yu)) / run(yu, yd));
            let shade = (1.0 - strength * slope.dot(l)).clamp(0.0, 2.0);
            if let Some(p) = px.get_mut(y * w + x) {
                scale_colour(p, shade);
            }
        }
    }
}

/// Multiplies a premultiplied colour's channels by `k`, keeping them within its alpha.
fn scale_colour(p: &mut [u8; 4], k: f64) {
    let a = f64::from(p[3]);
    for c in p.iter_mut().take(3) {
        *c = (f64::from(*c) * k).round().clamp(0.0, a) as u8;
    }
}

/// Moves a premultiplied colour's channels `t` of the way to white (or, `t` < 0, to black).
fn towards_white(p: &mut [u8; 4], t: f64) {
    let a = f64::from(p[3]);
    for c in p.iter_mut().take(3) {
        let v = f64::from(*c);
        let target = if t >= 0.0 { a } else { 0.0 };
        *c = (v + (target - v) * t.abs().min(1.0)).round().clamp(0.0, a) as u8;
    }
}

/// The two nearest jittered seeds of a grid of `size` to `p`: (distance to the nearest, its cell,
/// distance to the second nearest).
fn voronoi(p: Point, size: f64, salt: u64) -> (f64, (i64, i64), f64) {
    let (ci, cj) = (cell(p.x, size), cell(p.y, size));
    let (mut d1, mut d2, mut owner) = (f64::INFINITY, f64::INFINITY, (ci, cj));
    // Seeds stay inside the middle 80 % of their cells, so the nearest two (near enough for crack
    // and lead widths) lie in the 3 × 3 block around the point.
    for dj in -1..=1 {
        for di in -1..=1 {
            let (i, j) = (ci.saturating_add(di), cj.saturating_add(dj));
            let d = (seed(i, j, size, salt) - p).hypot();
            if d < d1 {
                (d2, d1, owner) = (d1, d, (i, j));
            } else if d < d2 {
                d2 = d;
            }
        }
    }
    (d1, owner, d2)
}

/// Craquelure: the art painted on plaster that has cracked into plates about `spacing` points
/// across, the cracks `depth` deep, the whole `brightness` (0..10) bright.
pub(super) fn craquelure(px: &mut [[u8; 4]], w: usize, h: usize, space: &PixelSpace, spacing: f64, depth: f64, brightness: f64) {
    let Some(size) = usable(spacing, space) else { return };
    let crack = (size * 0.06).max(0.6);
    let hf = heights(w, h, space, |p| {
        let (d1, _, d2) = voronoi(p, size, 11);
        // Half the gap between the two nearest seeds: the distance to the crack between them.
        let plate = smooth(((d2 - d1) * 0.5 - crack * 0.5) / crack);
        plate * (1.0 - 0.25 * (d1 / size).min(1.0))
    });
    let light = 0.55 + 0.05 * brightness.clamp(0.0, 10.0);
    let dark = depth.clamp(0.0, 10.0) / 10.0 * 0.5;
    for (p, ht) in px.iter_mut().zip(&hf) {
        scale_colour(p, light * (1.0 - dark * (1.0 - ht.min(1.0))));
    }
    relief(px, w, h, space, &hf, depth * 0.08, Light::TopLeft);
}

/// Grain: noise of `kind`, `intensity` (0..100) strong, after the contrast is set by `contrast`
/// (0..100, 50 = unchanged).
pub(super) fn grain(px: &mut [[u8; 4]], w: usize, h: usize, space: &PixelSpace, intensity: f64, contrast: f64, kind: Grain) {
    let amount = (intensity / 100.0).clamp(0.0, 1.0);
    let mut gain = (contrast / 50.0).clamp(0.0, 2.0);
    if kind == Grain::Contrasty {
        gain *= 2.0;
    }
    let (gw, gh) = kind.size();
    for y in 0..h {
        for x in 0..w {
            let Some(p) = px.get_mut(y * w + x) else { continue };
            let a = f64::from(p[3]);
            if a <= 0.0 {
                continue;
            }
            let q = centre(space.to_doc, x, y);
            let (i, j) = (cell(q.x, gw), cell(q.y, gh));
            let noise = match kind {
                Grain::Soft | Grain::Clumped => value(q.x / gw, q.y / gh, 71) * 2.0 - 1.0,
                _ => unit(hash(i, j, 72)) * 2.0 - 1.0,
            };
            let mut rgb = [p[0], p[1], p[2]].map(|c| f64::from(c) / a * 255.0);
            for c in rgb.iter_mut() {
                *c = (*c - 127.5) * gain + 127.5;
            }
            let hit = unit(hash(i, j, 73)) < amount / 3.0;
            match kind {
                Grain::Sprinkles if hit => rgb = [255.0; 3],
                Grain::Speckle if hit => rgb = [0.0; 3],
                Grain::Sprinkles | Grain::Speckle => {}
                Grain::Stippled => {
                    let l = 0.299 * rgb[0] + 0.587 * rgb[1] + 0.114 * rgb[2];
                    let dot = if l / 255.0 > unit(hash(i, j, 74)) { 255.0 } else { 0.0 };
                    rgb = rgb.map(|c| c + (dot - c) * amount);
                }
                Grain::Soft => rgb = rgb.map(|c| c + noise * amount * 64.0),
                _ => rgb = rgb.map(|c| c + noise * amount * 128.0),
            }
            for (out, c) in p.iter_mut().zip(rgb) {
                *out = (c.clamp(0.0, 255.0) * a / 255.0).round().clamp(0.0, 255.0) as u8;
            }
        }
    }
}

/// Mosaic Tiles: the art cut into raised square tiles `tile` points across, set in grout
/// `grout` × 0.3 points wide that is `lighten` (0..10) lighter than the art under it.
pub(super) fn mosaic_tiles(px: &mut [[u8; 4]], w: usize, h: usize, space: &PixelSpace, tile: f64, grout: f64, lighten: f64) {
    let Some(size) = usable(tile, space) else { return };
    let gap = (grout * 0.3).clamp(0.0, size * 0.45);
    let edge = |p: Point| {
        let (u, v) = (p.x.rem_euclid(size), p.y.rem_euclid(size));
        u.min(size - u).min(v).min(size - v)
    };
    let hf = heights(w, h, space, |p| smooth((edge(p) - gap * 0.5) / 0.75));
    for y in 0..h {
        for x in 0..w {
            if edge(centre(space.to_doc, x, y)) < gap * 0.5
                && let Some(p) = px.get_mut(y * w + x)
            {
                towards_white(p, lighten.clamp(0.0, 10.0) * 0.07);
            }
        }
    }
    relief(px, w, h, space, &hf, 0.3, Light::TopLeft);
}

/// Patchwork: squares `square` points across, each the average colour of the art under it,
/// raised by a random height up to `relief`.
pub(super) fn patchwork(px: &mut [[u8; 4]], w: usize, h: usize, space: &PixelSpace, square: f64, relief_: f64) {
    let Some(size) = usable(square, space) else { return };
    let owners: Vec<(i64, i64)> = (0..h)
        .flat_map(|y| (0..w).map(move |x| (x, y)))
        .map(|(x, y)| centre(space.to_doc, x, y))
        .map(|p| (cell(p.x, size), cell(p.y, size)))
        .collect();
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
    let bevel = (size * 0.2).min(0.75);
    let hf = heights(w, h, space, |p| {
        let (u, v) = (p.x.rem_euclid(size), p.y.rem_euclid(size));
        let e = u.min(size - u).min(v).min(size - v);
        (0.4 + 0.6 * unit(hash(cell(p.x, size), cell(p.y, size), 81))) * smooth(e / bevel)
    });
    relief(px, w, h, space, &hf, relief_ * 0.03, Light::TopLeft);
}

/// Stained Glass: the art as panes about `size` points across, each its average colour, leaded
/// with black `border` × 0.3 points wide, lit from the object's middle by `light` (0..10).
pub(super) fn stained_glass(px: &mut [[u8; 4]], w: usize, h: usize, space: &PixelSpace, size: f64, border: f64, light: f64) {
    let Some(size) = usable(size, space) else { return };
    let lead = (border * 0.3).clamp(0.0, size * 0.45);
    let cells: Vec<((i64, i64), f64)> = (0..h)
        .flat_map(|y| (0..w).map(move |x| (x, y)))
        .map(|(x, y)| {
            let (d1, owner, d2) = voronoi(centre(space.to_doc, x, y), size, 12);
            (owner, (d2 - d1) * 0.5)
        })
        .collect();
    let mut sums: HashMap<(i64, i64), ([u64; 4], u64)> = HashMap::new();
    for (p, (o, _)) in px.iter().zip(&cells) {
        let e = sums.entry(*o).or_insert(([0; 4], 0));
        for (a, v) in e.0.iter_mut().zip(p) {
            *a += u64::from(*v);
        }
        e.1 += 1;
    }
    // The light falls off from the object's middle, over about ten panes.
    let glow = light.clamp(0.0, 10.0) * 0.06;
    for ((k, p), (o, gap)) in px.iter_mut().enumerate().zip(&cells) {
        let Some((sum, n)) = sums.get(o) else { continue };
        let n = (*n).max(1);
        let a = ((sum[3] + n / 2) / n).min(255) as u8;
        if *gap < lead * 0.5 {
            *p = [0, 0, 0, a];
            continue;
        }
        *p = sum.map(|s| ((s + n / 2) / n).min(255) as u8);
        let d = (centre(space.to_doc, k % w.max(1), k / w.max(1)) - space.center).hypot() / (10.0 * size);
        let fall = 1.0 / (1.0 + d * d);
        towards_white(p, glow * fall);
    }
}

/// Texturizer: the art printed on a surface, its relief lit from one side.
pub(super) fn texturize(px: &mut [[u8; 4]], w: usize, h: usize, space: &PixelSpace, t: Texturizer) {
    if !(t.scaling.is_finite() && t.scaling > 0.0) {
        return;
    }
    let hf = heights(w, h, space, |p| {
        let v = t.surface.height(p, t.scaling);
        if t.invert { 1.0 - v } else { v }
    });
    relief(px, w, h, space, &hf, t.relief * 0.05, t.light);
}
