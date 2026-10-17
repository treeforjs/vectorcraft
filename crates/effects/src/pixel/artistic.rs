//! Artistic › Colored Pencil, Cutout, Dry Brush, Film Grain, Fresco, Neon Glow, Paint Daubs,
//! Palette Knife, Plastic Wrap, Poster Edges, Rough Pastels, Smudge Stick, Sponge, Underpainting
//! and Watercolor.
//!
//! Each repaints the art inside its own coverage. Strokes, daubs, grain and sponge holes lie in
//! document space, so they stay on the art whatever the raster's origin, resolution or zoom.

use vectorcraft_geom::{Point, Vec2};

use super::brush::{FALLING, RISING, Source, each, edges, put};
use super::noise::{cell, hash, unit, value};
use super::pixelate::{centre, seed, usable};
use super::sketch::{slope, soft_lumas};
use super::texture::{Texturizer, texturize};
use super::{PixelSpace, Px16, gaussian, to16};

/// Paint Daubs' brushes, in the dialog's order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Daub {
    Simple,
    LightRough,
    DarkRough,
    WideSharp,
    WideBlurry,
    Sparkle,
}

impl Daub {
    /// (id, label, brush) in the dialog's order.
    pub const KINDS: [(&'static str, &'static str, Daub); 6] = [
        ("simple", "Simple", Daub::Simple),
        ("lightRough", "Light Rough", Daub::LightRough),
        ("darkRough", "Dark Rough", Daub::DarkRough),
        ("wideSharp", "Wide Sharp", Daub::WideSharp),
        ("wideBlurry", "Wide Blurry", Daub::WideBlurry),
        ("sparkle", "Sparkle", Daub::Sparkle),
    ];
}

/// An Artistic filter with its parameters read and clamped to their ranges.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Artistic {
    ColoredPencil { width: f64, pressure: f64, paper: f64 },
    Cutout { levels: f64, simplicity: f64, fidelity: f64 },
    DryBrush { size: f64, detail: f64, texture: f64 },
    FilmGrain { grain: f64, highlight: f64, intensity: f64 },
    Fresco { size: f64, detail: f64, texture: f64 },
    NeonGlow { size: f64, brightness: f64, color: [f32; 3] },
    PaintDaubs { size: f64, sharpness: f64, brush: Daub },
    PaletteKnife { size: f64, detail: f64, softness: f64 },
    PlasticWrap { strength: f64, detail: f64, smoothness: f64 },
    PosterEdges { thickness: f64, intensity: f64, posterization: f64 },
    RoughPastels { length: f64, detail: f64, paper: Texturizer },
    SmudgeStick { length: f64, highlight: f64, intensity: f64 },
    Sponge { size: f64, definition: f64, smoothness: f64 },
    Underpainting { size: f64, coverage: f64, paper: Texturizer },
    Watercolor { detail: f64, shadow: f64, texture: f64 },
}

/// A premultiplied 16-bit colour's straight RGB in [0, 1] (black where transparent).
fn straight(c: [f32; 4]) -> [f32; 3] {
    if c[3] > 0.0 { [c[0], c[1], c[2]].map(|v| (v / c[3]).clamp(0.0, 1.0)) } else { [0.0; 3] }
}

/// Straight RGB in [0, 1] under coverage `a` (0..65535), premultiplied.
fn under(rgb: [f32; 3], a: f32) -> [f32; 4] {
    [rgb[0].clamp(0.0, 1.0) * a, rgb[1].clamp(0.0, 1.0) * a, rgb[2].clamp(0.0, 1.0) * a, a]
}

/// `v` rounded to one of `n` (≥ 2) evenly spaced levels.
fn level(v: f32, n: f32) -> f32 {
    let steps = (n - 1.0).max(1.0);
    (v.clamp(0.0, 1.0) * steps).round() / steps
}

/// A luma in [0, 1] of straight RGB.
fn grey(c: [f32; 3]) -> f32 {
    0.299 * c[0] + 0.587 * c[1] + 0.114 * c[2]
}

/// Moves straight RGB away from its grey by `k` (1 = unchanged).
fn saturate(c: [f32; 3], k: f32) -> [f32; 3] {
    let g = grey(c);
    c.map(|v| g + (v - g) * k)
}

/// Stretches straight RGB away from mid grey by `k`.
fn stretch(c: [f32; 3], k: f32) -> [f32; 3] {
    c.map(|v| (v - 0.5) * k + 0.5)
}

/// Hash noise in [-0.5, 0.5) on a grid of `size` points.
fn speck(p: Point, size: f64, salt: u64) -> f32 {
    unit(hash(cell(p.x, size), cell(p.y, size), salt)) as f32 - 0.5
}

/// A random stroke direction for the patch of `size` points around `p`.
fn heading(p: Point, size: f64, salt: u64) -> Vec2 {
    let angle = unit(hash(cell(p.x, size), cell(p.y, size), salt)) * std::f64::consts::PI;
    Vec2::new(angle.cos(), angle.sin())
}

/// The art blurred by `sigma` points (premultiplied, 16-bit).
fn blurred(px: &[[u8; 4]], w: usize, space: &PixelSpace, sigma: f64) -> Vec<Px16> {
    let mut b = to16(px);
    gaussian(&mut b, w, sigma / space.px);
    b
}

/// Repaints every pixel `f(index, straight colour of the blurred art)` under its own coverage.
fn repaint(px: &mut [[u8; 4]], b: &[Px16], f: impl Fn(usize, [f32; 3]) -> [f32; 3]) {
    for (i, (p, q)) in px.iter_mut().zip(b).enumerate() {
        let a = f32::from(p[3]) * 257.0;
        put(p, under(f(i, straight(q.map(f32::from))), a));
    }
}

/// Lightens the brightest tones: `area` (0..20) widens the band, `intensity` (0..10) its strength.
fn highlights(c: [f32; 3], area: f64, intensity: f64) -> [f32; 3] {
    if area <= 0.0 {
        return c;
    }
    let edge = 1.0 - (area / 20.0) as f32 * 0.7;
    let t = ((grey(c) - edge) / (1.0 - edge).max(1e-3)).clamp(0.0, 1.0) * (intensity / 10.0) as f32;
    c.map(|v| v + (1.0 - v) * t)
}

/// The colour at `p` of the stroke through it, straight, falling back to the colour right there
/// where the stroke runs off the art.
fn stroked(src: &Source, p: Point, dir: Vec2, len: f64, width: f64, salt: u64) -> [f32; 3] {
    let c = src.stroke(p, dir, len, width, salt);
    if c[3] > 1.0 { straight(c) } else { straight(src.at(p)) }
}

impl Artistic {
    /// How far (points) the content a pixel takes can lie from it.
    pub(super) fn reach(&self) -> f64 {
        match *self {
            Artistic::ColoredPencil { .. } => 6.0,
            Artistic::Cutout { simplicity, fidelity, .. } => 3.0 * cut_blur(simplicity, fidelity) + 1.0,
            Artistic::DryBrush { size, .. } | Artistic::Fresco { size, .. } => dab(size) + 1.0,
            Artistic::FilmGrain { .. } => 0.0,
            Artistic::NeonGlow { size, .. } => 0.9 * size.abs() + 1.0,
            Artistic::PaintDaubs { size, brush, .. } => 2.0 * daub(size, brush) + 1.0,
            Artistic::PaletteKnife { size, detail, softness } => 3.0 * knife(size, detail) + 0.45 * softness,
            Artistic::PlasticWrap { smoothness, .. } => 0.75 * smoothness + 1.0,
            Artistic::PosterEdges { thickness, .. } => 3.0 * (0.25 + 0.15 * thickness) + 1.0,
            Artistic::RoughPastels { length, .. } => 0.5 * length + 1.5,
            Artistic::SmudgeStick { length, .. } => 0.8 * length + 1.0,
            Artistic::Sponge { size, .. } => 0.75 * size + 1.0,
            Artistic::Underpainting { size, .. } => 0.9 * size + 1.0,
            Artistic::Watercolor { detail, .. } => 3.0 * wash(detail) + 1.0,
        }
    }

    pub(super) fn apply(&self, px: &mut [[u8; 4]], w: usize, h: usize, space: &PixelSpace) {
        if w == 0 || h == 0 || !(space.px.is_finite() && space.px > 0.0) {
            return;
        }
        match *self {
            Artistic::ColoredPencil { width, pressure, paper } => {
                let Some(src) = Source::new(px, w, h, space) else { return };
                let e = edges(px, w, h, 0.5 / space.px);
                let paper = (paper / 50.0) as f32;
                let press = 0.3 + (pressure / 15.0) as f32;
                each(px, w, h, space, |p, i| {
                    let a = src.at(p)[3];
                    // Pencil hatching runs along one diagonal or the other in 4 pt patches.
                    let dir = if unit(hash(cell(p.x, 4.0), cell(p.y, 4.0), 111)) < 0.5 { RISING } else { FALLING };
                    let c = stroked(&src, p, dir, 6.0, width * 0.25, 112);
                    let tooth = 0.7 + 0.6 * speck(p, 0.5, 113);
                    let cover = (((1.0 - grey(c)) * press + 2.0 * e.get(i).copied().unwrap_or(0.0)) * tooth).clamp(0.0, 1.0);
                    under(c.map(|v| paper + (v - paper) * cover), a)
                });
            }
            Artistic::Cutout { levels, simplicity, fidelity } => {
                let b = blurred(px, w, space, cut_blur(simplicity, fidelity));
                let n = levels.round() as f32;
                repaint(px, &b, |_, c| c.map(|v| level(v, n)));
            }
            Artistic::DryBrush { size, detail, texture } | Artistic::Fresco { size, detail, texture } => {
                let fresco = matches!(self, Artistic::Fresco { .. });
                let Some(src) = Source::new(px, w, h, space) else { return };
                let e = if fresco { edges(px, w, h, 0.5 / space.px) } else { Vec::new() };
                let n = 3.0 + detail.round() as f32;
                let grit = texture.round() as f32 * 0.05;
                each(px, w, h, space, |p, i| {
                    let a = src.at(p)[3];
                    let mut c = stroked(&src, p, heading(p, 3.0, 114), dab(size), 1.0, 115);
                    if fresco {
                        // Fresco's dabs are darker, contrastier and outlined.
                        c = stretch(c, 1.4).map(|v| v * (1.0 - 0.5 * e.get(i).copied().unwrap_or(0.0)));
                    }
                    let g = speck(p, 0.5, 116) * grit;
                    under(c.map(|v| level(v, n) + g), a)
                });
            }
            Artistic::FilmGrain { grain, highlight, intensity } => {
                let amount = (grain / 20.0) as f32 * 0.5;
                let b = to16(px);
                let tones: Vec<f32> = (0..w * h).map(|i| speck(centre(space.to_doc, i % w, i / w), 0.35, 117)).collect();
                repaint(px, &b, |i, c| {
                    // Grain shows in the shadows and midtones, fading out in the highlights.
                    let g = tones.get(i).copied().unwrap_or(0.0) * amount * (1.0 - 0.7 * grey(c));
                    highlights(c.map(|v| v + g), highlight, intensity)
                });
            }
            Artistic::NeonGlow { size, brightness, color } => {
                let soft = soft_lumas(px, w, h, space, 0.3 * size.abs());
                let lift = (brightness / 50.0) as f32 * 1.5;
                let b = to16(px);
                repaint(px, &b, |i, c| {
                    let l = soft.get(i).copied().unwrap_or(1.0);
                    // A positive size glows in from the light tones, a negative one from the dark.
                    let g = if size >= 0.0 { l } else { 1.0 - l };
                    let glow = (g * g * lift).min(1.0);
                    let base = grey(c) * 0.5;
                    [0, 1, 2].map(|k| base + (color.get(k).copied().unwrap_or(0.0) - base) * glow)
                });
            }
            Artistic::PaintDaubs { size, sharpness, brush } => {
                let Some(src) = Source::new(px, w, h, space) else { return };
                let len = daub(size, brush);
                let k = if brush == Daub::WideBlurry { 0.0 } else { (sharpness / 40.0) as f32 * 1.5 };
                each(px, w, h, space, |p, _| {
                    let a = src.at(p)[3];
                    let dir = heading(p, len * 1.5, 118);
                    let c = stroked(&src, p, dir, len, (len * 0.5).max(0.5), 119);
                    let wide = stroked(&src, p, dir, len * 2.0, len.max(0.5), 120);
                    let n = speck(p, 0.5, 121);
                    let mut out = [0, 1, 2].map(|j| {
                        let (v, u) = (c.get(j).copied().unwrap_or(0.0), wide.get(j).copied().unwrap_or(0.0));
                        v + (v - u) * k
                    });
                    match brush {
                        Daub::LightRough => out = out.map(|v| v + n.max(0.0) * 0.3),
                        Daub::DarkRough => out = out.map(|v| v + n.min(0.0) * 0.3),
                        Daub::Sparkle if n > 0.47 => out = out.map(|v| v + (1.0 - v) * 0.7),
                        _ => {}
                    }
                    under(out, a)
                });
            }
            Artistic::PaletteKnife { size, detail, softness } => {
                let Some(src) = Source::new(px, w, h, space) else { return };
                let Some(cellsize) = usable(knife(size, detail), space) else { return };
                each(px, w, h, space, |p, _| {
                    let a = src.at(p)[3];
                    // Each knife stroke lays the colour at its seed flat across its cell; a cell
                    // whose seed lies off the art takes the nearest seed on it.
                    let (ci, cj) = (cell(p.x, cellsize), cell(p.y, cellsize));
                    let mut seeds = [(f64::INFINITY, p); 9];
                    for (k, s) in seeds.iter_mut().enumerate() {
                        let (di, dj) = (k as i64 % 3 - 1, k as i64 / 3 - 1);
                        let q = seed(ci.saturating_add(di), cj.saturating_add(dj), cellsize, 122);
                        *s = ((q - p).hypot(), q);
                    }
                    seeds.sort_by(|a, b| a.0.total_cmp(&b.0));
                    let c = seeds.iter().map(|s| src.at(s.1)).find(|c| c[3] > 1.0).unwrap_or_else(|| src.at(p));
                    let c = straight(c);
                    under(c, a)
                });
                if softness > 0.0 {
                    let b = blurred(px, w, space, 0.15 * softness);
                    repaint(px, &b, |_, c| c);
                }
            }
            Artistic::PlasticWrap { strength, detail, smoothness } => {
                let l = soft_lumas(px, w, h, space, 0.25 * smoothness);
                let bands = detail as f32 * 0.5 + 1.0;
                let k = (strength / 20.0) as f32;
                let b = to16(px);
                repaint(px, &b, |i, c| {
                    let v = l.get(i).copied().unwrap_or(1.0);
                    let s = slope(&l, w, h, i % w, i / w, space.px).hypot() as f32;
                    // The film catches the light along contours where the art's tone changes.
                    let shine = (0.5 + 0.5 * (v * std::f32::consts::PI * bands).cos()).powi(8) * (s * 4.0).min(1.0) * k;
                    c.map(|x| x + (1.0 - x) * shine)
                });
            }
            Artistic::PosterEdges { thickness, intensity, posterization } => {
                let e = edges(px, w, h, (0.25 + 0.15 * thickness) / space.px);
                let n = 2.0 + posterization.round() as f32;
                let ink = 1.0 + intensity as f32;
                let b = to16(px);
                repaint(px, &b, |i, c| {
                    let dark = (e.get(i).copied().unwrap_or(0.0) * ink * 1.5).min(1.0);
                    c.map(|v| level(v, n) * (1.0 - dark))
                });
            }
            Artistic::RoughPastels { length, detail, paper } => {
                let Some(src) = Source::new(px, w, h, space) else { return };
                let k = 0.8 + (detail / 20.0) as f32 * 0.6;
                each(px, w, h, space, |p, _| {
                    let a = src.at(p)[3];
                    under(stretch(stroked(&src, p, RISING, 0.5 * length + 0.5, 0.75, 123), k), a)
                });
                texturize(px, w, h, space, paper);
            }
            Artistic::SmudgeStick { length, highlight, intensity } => {
                let Some(src) = Source::new(px, w, h, space) else { return };
                each(px, w, h, space, |p, _| {
                    let a = src.at(p)[3];
                    let c = stroked(&src, p, FALLING, 0.8 * length + 0.5, 0.75, 124);
                    // The smudging deepens the darker tones.
                    let c = c.map(|v| v * (0.85 + 0.15 * v));
                    under(highlights(c, highlight, intensity), a)
                });
            }
            Artistic::Sponge { size, definition, smoothness } => {
                let b = blurred(px, w, space, 0.25 * size);
                let scale = 1.0 + 0.2 * smoothness;
                let width = 0.05 + (smoothness / 15.0) as f32 * 0.3;
                let k = (definition / 25.0) as f32 * 0.7;
                repaint(px, &b, |i, c| {
                    let p = centre(space.to_doc, i % w, i / w);
                    let n = (0.7 * value(p.x / scale, p.y / scale, 125) + 0.3 * value(p.x / (0.4 * scale), p.y / (0.4 * scale), 126)) as f32;
                    let hole = ((n - 0.5) / width + 0.5).clamp(0.0, 1.0);
                    c.map(|v| v * (1.0 + (hole - 0.5) * k))
                });
            }
            Artistic::Underpainting { size, coverage, paper } => {
                let b = blurred(px, w, space, 0.3 * size);
                repaint(px, &b, |_, c| saturate(c, 1.1));
                texturize(px, w, h, space, Texturizer { relief: paper.relief * (coverage / 16.0), ..paper });
            }
            Artistic::Watercolor { detail, shadow, texture } => {
                let sigma = wash(detail);
                let b = blurred(px, w, space, sigma);
                let e = edges(px, w, h, (sigma + 0.3) / space.px);
                let deep = (shadow / 10.0) as f32 * 0.6;
                let grit = texture.round() as f32 * 0.03;
                repaint(px, &b, |i, c| {
                    let p = centre(space.to_doc, i % w, i / w);
                    // Pigment pools along the edges and in the shadows.
                    let pool = 1.0 - 0.6 * e.get(i).copied().unwrap_or(0.0);
                    let shade = 1.0 - deep * (1.0 - grey(c));
                    saturate(c, 1.25).map(|v| v * pool * shade + speck(p, 0.5, 127) * grit)
                });
            }
        }
    }
}

/// Cutout's blur in points: simpler edges and lower fidelity smooth the shapes more.
fn cut_blur(simplicity: f64, fidelity: f64) -> f64 {
    0.4 * simplicity + 0.3 * (3.0 - fidelity) + 0.2
}

/// Dry Brush and Fresco's stroke length in points.
fn dab(size: f64) -> f64 {
    0.8 * size + 0.5
}

/// Paint Daubs' daub length in points.
fn daub(size: f64, brush: Daub) -> f64 {
    let wide = matches!(brush, Daub::WideSharp | Daub::WideBlurry);
    size * if wide { 0.6 } else { 0.4 }
}

/// Palette Knife's stroke size in points: more detail cuts smaller strokes.
fn knife(size: f64, detail: f64) -> f64 {
    (size * 0.3 * (4.0 - detail) / 3.0).max(0.5)
}

/// Watercolor's wash blur in points: less detail spreads the paint further.
fn wash(detail: f64) -> f64 {
    (15.0 - detail) * 0.15
}
