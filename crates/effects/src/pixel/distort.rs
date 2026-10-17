//! Distort › Diffuse Glow, Glass and Ocean Ripple.
//!
//! Glass and Ocean Ripple move pixels by a displacement field; their textures and ripples lie in
//! document space, so they stay on the art whatever the raster's origin, resolution or zoom.

use vectorcraft_geom::{Affine, Point, Vec2};

use super::blur::bilinear;
use super::noise::{cell, hash, unit, value};
use super::{PixelSpace, blur_plane, to8, to16};

/// Glass's surface textures, in the dialog's order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GlassTexture {
    Frosted,
    Blocks,
    Canvas,
    TinyLens,
}

impl GlassTexture {
    /// (id, label, texture) in the dialog's order.
    pub const KINDS: [(&'static str, &'static str, GlassTexture); 4] = [
        ("frosted", "Frosted", GlassTexture::Frosted),
        ("blocks", "Blocks", GlassTexture::Blocks),
        ("canvas", "Canvas", GlassTexture::Canvas),
        ("tinyLens", "Tiny Lens", GlassTexture::TinyLens),
    ];

    /// The texture's height in [0, 1] at document point `p`, its features `scale` × their usual
    /// size.
    fn height(self, p: Point, scale: f64) -> f64 {
        match self {
            GlassTexture::Frosted => {
                let f = 2.0 * scale;
                (value(p.x / f, p.y / f, 21) + 0.5 * value(p.x / f * 2.0, p.y / f * 2.0, 22)) / 1.5
            }
            GlassTexture::Blocks => {
                let f = 8.0 * scale;
                let (u, v) = ((p.x / f).rem_euclid(1.0), (p.y / f).rem_euclid(1.0));
                1.0 - 2.0 * (u - 0.5).abs().max((v - 0.5).abs())
            }
            GlassTexture::Canvas => {
                let f = 3.0 * scale;
                let t = std::f64::consts::TAU;
                0.5 + 0.25 * ((t * p.x / f).sin() + (t * p.y / f).sin())
            }
            GlassTexture::TinyLens => {
                let f = 6.0 * scale;
                let (u, v) = ((p.x / f).rem_euclid(1.0) - 0.5, (p.y / f).rem_euclid(1.0) - 0.5);
                (1.0 - 4.0 * (u * u + v * v)).max(0.0).sqrt()
            }
        }
    }
}

/// The document point at the centre of pixel (`x`, `y`).
fn centre(to_doc: Affine, x: usize, y: usize) -> Point {
    to_doc * Point::new(x as f64 + 0.5, y as f64 + 0.5)
}

/// A document-space offset as a pixel-space one.
fn to_px(from_doc: Affine, v: Vec2) -> Vec2 {
    (from_doc * v.to_point()) - (from_doc * Point::ZERO)
}

/// Every pixel takes the colour `offset(x, y)` pixels away (bilinear, transparent outside).
fn displace(px: &mut [[u8; 4]], w: usize, h: usize, offset: impl Fn(usize, usize) -> Vec2) {
    let src = to16(px);
    for y in 0..h {
        for x in 0..w {
            let d = offset(x, y);
            let (sx, sy) = (x as f64 + d.x, y as f64 + d.y);
            let s = if sx.is_finite() && sy.is_finite() { bilinear(&src, w, h, sx, sy) } else { [0.0; 4] };
            if let Some(out) = px.get_mut(y * w + x) {
                *out = s.map(|v| to8(v.round().clamp(0.0, 65535.0) as u16));
            }
        }
    }
}

/// Ocean Ripple: the content wobbles by up to `magnitude` × 0.5 points under ripples about
/// `size` × 2 points across.
pub(super) fn ocean_ripple(px: &mut [[u8; 4]], w: usize, h: usize, space: &PixelSpace, size: f64, magnitude: f64) {
    let (wave, amp) = (size * 2.0, magnitude * 0.5);
    if !(wave > 0.0 && wave.is_finite() && amp.is_finite()) || amp <= 0.0 {
        return;
    }
    let from_doc = space.to_doc.inverse();
    let field = |p: Point, salt: u64| {
        let (x, y) = (p.x / wave, p.y / wave);
        // Two octaves: broad swells with finer ripples on them.
        (2.0 * value(x, y, salt) - 1.0 + (value(2.0 * x, 2.0 * y, salt + 1) - 0.5)) / 1.5
    };
    displace(px, w, h, |x, y| {
        let p = centre(space.to_doc, x, y);
        to_px(from_doc, Vec2::new(amp * field(p, 31), amp * field(p, 41)))
    });
}

/// Glass: the content seen through a textured pane, moved by up to `distortion` × 0.5 points
/// along the texture's slopes, the texture softened by `smoothness` × 0.5 points.
pub(super) fn glass(px: &mut [[u8; 4]], w: usize, h: usize, space: &PixelSpace, g: Glass) {
    let reach = g.distortion * 0.5;
    if w == 0 || h == 0 || !(reach > 0.0 && reach.is_finite() && space.px.is_finite() && space.px > 0.0 && g.scaling.is_finite() && g.scaling > 0.0) {
        return;
    }
    let mut heights: Vec<f32> = Vec::with_capacity(px.len());
    for y in 0..h {
        for x in 0..w {
            let t = g.texture.height(centre(space.to_doc, x, y), g.scaling) as f32;
            heights.push(if g.invert { 1.0 - t } else { t });
        }
    }
    blur_plane(&mut heights, w, h, g.smoothness * 0.5 / space.px);
    let at = |x: usize, y: usize| heights.get(y * w + x).copied().unwrap_or(0.0);
    let slope = |x: usize, y: usize| {
        let (l, r) = (at(x.saturating_sub(1), y), at((x + 1).min(w - 1), y));
        let (u, d) = (at(x, y.saturating_sub(1)), at(x, (y + 1).min(h - 1)));
        Vec2::new(f64::from(r - l), f64::from(d - u))
    };
    // The steepest slope moves the content the full distance.
    let mut steepest = 0.0f64;
    for y in 0..h {
        for x in 0..w {
            steepest = steepest.max(slope(x, y).hypot());
        }
    }
    if steepest <= 1e-9 {
        return;
    }
    // Slopes are per pixel; the move is along them in pixels of the raster.
    let scale = reach / space.px / steepest;
    displace(px, w, h, |x, y| slope(x, y) * scale);
}

/// Glass's parameters.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Glass {
    pub distortion: f64,
    pub smoothness: f64,
    pub texture: GlassTexture,
    /// The texture's size, 1 = 100 %.
    pub scaling: f64,
    pub invert: bool,
}

/// Diffuse Glow: a soft white glow over the highlights, cleared from the darker tones by
/// `clear` (0..20) and as strong as `glow` (0..20), with white grain as heavy as `grain` (0..10).
pub(super) fn diffuse_glow(px: &mut [[u8; 4]], w: usize, h: usize, space: &PixelSpace, grain: f64, glow: f64, clear: f64) {
    if !(space.px.is_finite() && space.px > 0.0) {
        return;
    }
    let mut light: Vec<f32> = px
        .iter()
        .map(|p| {
            let a = f32::from(p[3]);
            if a > 0.0 { (0.2126 * f32::from(p[0]) + 0.7152 * f32::from(p[1]) + 0.0722 * f32::from(p[2])) / a } else { 0.0 }
        })
        .collect();
    blur_plane(&mut light, w, h, 1.5 / space.px);
    let low = (clear / 20.0).clamp(0.0, 1.0) * 0.9;
    let strength = (glow / 10.0).clamp(0.0, 2.0);
    let specks = (grain / 10.0).clamp(0.0, 1.0) * 0.15;
    for y in 0..h {
        for x in 0..w {
            let Some(out) = px.get_mut(y * w + x) else { continue };
            let l = f64::from(light.get(y * w + x).copied().unwrap_or(0.0));
            let t = ((l - low) / (1.0 - low).max(1e-6)).clamp(0.0, 1.0);
            let mut m = (t * t * (3.0 - 2.0 * t) * strength).min(1.0);
            // Grain: specks of white on a half-point grid fixed to the art.
            let p = centre(space.to_doc, x, y);
            if unit(hash(cell(p.x, 0.5), cell(p.y, 0.5), 51)) < specks {
                m = m.max(0.6);
            }
            let a = f64::from(out[3]);
            for c in out.iter_mut().take(3) {
                let v = f64::from(*c);
                *c = (v + (a - v) * m).round().clamp(0.0, 255.0) as u8;
            }
        }
    }
}
