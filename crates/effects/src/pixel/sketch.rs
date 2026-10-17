//! Sketch › Bas Relief, Chalk & Charcoal, Charcoal, Chrome, Conté Crayon, Graphic Pen, Halftone
//! Pattern, Note Paper, Photocopy, Plaster, Reticulation, Stamp, Torn Edges and Water Paper.
//!
//! All but Water Paper redraw the art in two tones, black ink on white paper, keeping its
//! coverage. Strokes, grain and screens lie in document space, so they stay on the art whatever
//! the raster's origin, resolution or zoom.

use vectorcraft_geom::{Point, Vec2};

use super::brush::{Direction, FALLING, RISING, Source, each, luma, put};
use super::noise::{cell, hash, unit, value};
use super::pixelate::centre;
use super::texture::{Light, Texturizer, heights, relief, smooth};
use super::{PixelSpace, blur_plane, to16};

/// Halftone Pattern's screens, in the dialog's order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    Circle,
    Dot,
    Line,
}

impl Screen {
    /// (id, label, screen) in the dialog's order.
    pub const KINDS: [(&'static str, &'static str, Screen); 3] =
        [("circle", "Circle", Screen::Circle), ("dot", "Dot", Screen::Dot), ("line", "Line", Screen::Line)];
}

/// A Sketch filter with its parameters read and clamped to their ranges.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Sketch {
    BasRelief { detail: f64, smoothness: f64, light: Light },
    ChalkCharcoal { charcoal: f64, chalk: f64, pressure: f64 },
    Charcoal { thickness: f64, detail: f64, balance: f64 },
    Chrome { detail: f64, smoothness: f64 },
    ConteCrayon { foreground: f64, background: f64, paper: Texturizer },
    GraphicPen { length: f64, balance: f64, direction: Direction },
    HalftonePattern { size: f64, contrast: f64, screen: Screen },
    NotePaper { balance: f64, grain: f64, relief: f64 },
    Photocopy { detail: f64, darkness: f64 },
    Plaster { balance: f64, smoothness: f64, light: Light },
    Reticulation { density: f64, foreground: f64, background: f64 },
    Stamp { balance: f64, smoothness: f64 },
    TornEdges { balance: f64, smoothness: f64, contrast: f64 },
    WaterPaper { fiber: f64, brightness: f64, contrast: f64 },
}

/// Every pixel's straight luma in [0, 1] (transparent pixels count as white paper).
fn lumas(px: &[[u8; 4]]) -> Vec<f32> {
    to16(px).iter().map(|p| if p[3] > 0 { luma(p.map(f32::from)) } else { 1.0 }).collect()
}

/// `lumas` blurred by `sigma` points.
pub(super) fn soft_lumas(px: &[[u8; 4]], w: usize, h: usize, space: &PixelSpace, sigma: f64) -> Vec<f32> {
    let mut l = lumas(px);
    if space.px.is_finite() && space.px > 0.0 {
        // Blur the ink rather than the paper so the edges don't fade into black.
        for v in l.iter_mut() {
            *v = 1.0 - *v;
        }
        blur_plane(&mut l, w, h, sigma / space.px);
        for v in l.iter_mut() {
            *v = 1.0 - *v;
        }
    }
    l
}

/// Paints every pixel the grey `tone(index)` (0 = ink, 1 = paper) under its own coverage.
fn paint(px: &mut [[u8; 4]], tone: impl Fn(usize) -> f32) {
    for (i, p) in px.iter_mut().enumerate() {
        let a = f32::from(p[3]) * 257.0;
        let v = tone(i).clamp(0.0, 1.0) * a;
        put(p, [v, v, v, a]);
    }
}

/// The slope (per point, raster axes) of plane `l` at pixel (`x`, `y`).
pub(super) fn slope(l: &[f32], w: usize, h: usize, x: usize, y: usize, pt: f64) -> Vec2 {
    let at = |x: usize, y: usize| f64::from(l.get(y * w + x).copied().unwrap_or(0.0));
    let (xl, xr) = (x.saturating_sub(1), (x + 1).min(w.saturating_sub(1)));
    let (yu, yd) = (y.saturating_sub(1), (y + 1).min(h.saturating_sub(1)));
    let run = |a: usize, b: usize| (b - a).max(1) as f64 * pt;
    Vec2::new((at(xr, y) - at(xl, y)) / run(xl, xr), (at(x, yd) - at(x, yu)) / run(yu, yd))
}

/// 0 well below `edge`, 1 well above it, across a ramp `width` wide.
fn step(v: f32, edge: f32, width: f32) -> f32 {
    smooth(f64::from((v - edge) / width.max(1e-4) + 0.5)) as f32
}

impl Sketch {
    /// How far (points) the content a pixel takes can lie from it.
    pub(super) fn reach(&self) -> f64 {
        match *self {
            Sketch::BasRelief { detail, smoothness, .. } => 3.0 * soft(detail, smoothness) + 1.0,
            Sketch::ChalkCharcoal { .. } => 8.0,
            Sketch::Charcoal { .. } => 6.0,
            Sketch::Chrome { detail, smoothness } => 3.0 * (0.2 * (11.0 - detail) + 0.2 * smoothness) + 1.0,
            Sketch::ConteCrayon { .. } | Sketch::Reticulation { .. } | Sketch::HalftonePattern { .. } => 1.0,
            Sketch::GraphicPen { length, .. } => length,
            Sketch::NotePaper { .. } => 3.0,
            Sketch::Photocopy { detail, .. } => 0.75 * detail + 1.0,
            Sketch::Plaster { smoothness, .. } => 0.9 * smoothness + 1.0,
            Sketch::Stamp { smoothness, .. } => 0.6 * smoothness + 1.0,
            Sketch::TornEdges { smoothness, .. } => 0.75 * smoothness + 1.0,
            Sketch::WaterPaper { fiber, .. } => fiber * 0.3,
        }
    }

    pub(super) fn apply(&self, px: &mut [[u8; 4]], w: usize, h: usize, space: &PixelSpace) {
        if w == 0 || h == 0 || !(space.px.is_finite() && space.px > 0.0) {
            return;
        }
        match *self {
            Sketch::BasRelief { detail, smoothness, light } => {
                let l = soft_lumas(px, w, h, space, soft(detail, smoothness));
                let to = light.towards();
                let tones: Vec<f32> = (0..w * h)
                    .map(|i| {
                        (0.5 + slope(&l, w, h, i % w, i / w, space.px).dot(to) * -1.5 + (f64::from(l.get(i).copied().unwrap_or(1.0)) - 0.5) * 0.4)
                            as f32
                    })
                    .collect();
                paint(px, |i| tones.get(i).copied().unwrap_or(1.0));
            }
            Sketch::ChalkCharcoal { charcoal, chalk, pressure } => {
                let Some(src) = Source::new(px, w, h, space) else { return };
                let ramp = 0.12 / (1.0 + pressure as f32);
                let (dark, light) = ((charcoal / 20.0) as f32 * 0.6, 1.0 - (chalk / 20.0) as f32 * 0.6);
                each(px, w, h, space, |p, _| {
                    let a = src.at(p)[3];
                    let ink = 1.0 - step(luma(src.stroke(p, FALLING, 8.0, 0.75, 91)), dark, ramp);
                    let white = step(luma(src.stroke(p, RISING, 8.0, 0.75, 92)), light, ramp);
                    let v = (0.5 * (1.0 - ink) + 0.5 * white) * a;
                    [v, v, v, a]
                });
            }
            Sketch::Charcoal { thickness, detail, balance } => {
                let Some(src) = Source::new(px, w, h, space) else { return };
                let lane = thickness * 0.5;
                let ramp = 0.3 / (1.0 + detail as f32);
                each(px, w, h, space, |p, _| {
                    let a = src.at(p)[3];
                    let v = step(luma(src.stroke(p, RISING, 6.0, lane, 93)), (balance / 100.0) as f32, ramp) * a;
                    [v, v, v, a]
                });
            }
            Sketch::Chrome { detail, smoothness } => {
                let l = soft_lumas(px, w, h, space, 0.2 * (11.0 - detail) + 0.2 * smoothness);
                // Three bands of polished highlights and reflections.
                paint(px, |i| 0.5 - 0.5 * (l.get(i).copied().unwrap_or(1.0) * std::f32::consts::PI * 3.0).cos());
            }
            Sketch::ConteCrayon { foreground, background, paper } => {
                let l = lumas(px);
                let (fg, bg) = ((foreground / 15.0) as f32, (background / 15.0) as f32);
                paint(px, |i| {
                    let v = l.get(i).copied().unwrap_or(1.0);
                    let ink = (fg * (1.0 - v) * 1.5).min(1.0);
                    let chalk = (bg * v * 1.5).min(1.0);
                    0.5 * (1.0 - ink) + 0.5 * chalk
                });
                let hf = heights(w, h, space, |p| {
                    let v = paper.surface.height(p, paper.scaling);
                    if paper.invert { 1.0 - v } else { v }
                });
                relief(px, w, h, space, &hf, paper.relief * 0.05, paper.light);
            }
            Sketch::GraphicPen { length, balance, direction } => {
                let Some(src) = Source::new(px, w, h, space) else { return };
                let dir = direction.vector();
                let across = Vec2::new(-dir.y, dir.x);
                each(px, w, h, space, |p, _| {
                    let a = src.at(p)[3];
                    let (lane, seg) = (cell(p.to_vec2().dot(across), 0.5), cell(p.to_vec2().dot(dir), length.max(0.5)));
                    // Each stroke inks where the art is darker than its own random threshold.
                    let limit = (balance / 100.0) as f32 * (0.4 + 1.2 * unit(hash(lane, seg, 94)) as f32);
                    let v = if luma(src.stroke(p, dir, length, 0.5, 94)) < limit { 0.0 } else { a };
                    [v, v, v, a]
                });
            }
            Sketch::HalftonePattern { size, contrast, screen } => {
                let l = lumas(px);
                let k = 1.0 + (contrast / 25.0) as f32;
                let period = size * 1.5;
                let edge = space.px;
                let tones: Vec<f32> = (0..w * h)
                    .map(|i| {
                        let v = ((l.get(i).copied().unwrap_or(1.0) - 0.5) * k + 0.5).clamp(0.0, 1.0);
                        let p = centre(space.to_doc, i % w, i / w);
                        let ink = f64::from(1.0 - v);
                        // How far inside the inked part of its screen cell the pixel lies.
                        let inside = match screen {
                            Screen::Dot => {
                                let c = Point::new((p.x / period).floor() * period + 0.5 * period, (p.y / period).floor() * period + 0.5 * period);
                                period * (ink / std::f64::consts::PI).sqrt() - (p - c).hypot()
                            }
                            Screen::Line => 0.5 * ink * period - ((p.y / period).fract().abs() - 0.5).abs() * period,
                            Screen::Circle => {
                                let r = (p - space.center).hypot();
                                0.5 * ink * period - ((r / period).fract() - 0.5).abs() * period
                            }
                        };
                        1.0 - ((inside / edge + 0.5).clamp(0.0, 1.0)) as f32
                    })
                    .collect();
                paint(px, |i| tones.get(i).copied().unwrap_or(1.0));
            }
            Sketch::NotePaper { balance, grain, relief: depth } => {
                let l = soft_lumas(px, w, h, space, 0.5);
                let b = (balance / 50.0) as f32;
                let raised: Vec<f64> = l.iter().map(|v| f64::from(1.0 - step(*v, b, 0.08))).collect();
                let amount = (grain / 20.0) as f32 * 0.15;
                let grains: Vec<f32> = (0..w * h)
                    .map(|i| {
                        let p = centre(space.to_doc, i % w, i / w);
                        (unit(hash(cell(p.x, 0.5), cell(p.y, 0.5), 95)) as f32 - 0.5) * 2.0 * amount
                    })
                    .collect();
                paint(px, |i| 0.92 - 0.55 * raised.get(i).copied().unwrap_or(0.0) as f32 + grains.get(i).copied().unwrap_or(0.0));
                relief(px, w, h, space, &raised, depth * 0.04, Light::TopLeft);
            }
            Sketch::Photocopy { detail, darkness } => {
                let l = lumas(px);
                let mut b = l.clone();
                blur_plane(&mut b, w, h, 0.25 * detail / space.px);
                let k = darkness as f32 * 0.5;
                paint(px, |i| {
                    let v = l.get(i).copied().unwrap_or(1.0);
                    let d = b.get(i).copied().unwrap_or(1.0) - v;
                    // Edges darker than their surroundings copy black, and so do the deepest shadows.
                    (1.0 - (d * k).clamp(0.0, 1.0)).min(step(v, 0.2, 0.15))
                });
            }
            Sketch::Plaster { balance, smoothness, light } => {
                let l = soft_lumas(px, w, h, space, 0.3 * smoothness);
                let b = (balance / 50.0) as f32;
                let raised: Vec<f64> = l.iter().map(|v| f64::from(1.0 - step(*v, b, 0.1))).collect();
                paint(px, |i| 0.75 - 0.2 * raised.get(i).copied().unwrap_or(0.0) as f32);
                relief(px, w, h, space, &raised, 1.0, light);
            }
            Sketch::Reticulation { density, foreground, background } => {
                let l = lumas(px);
                let shift = (density / 50.0) as f32 * 0.5 - 0.25;
                let (ink, paper) = (1.0 - (foreground / 50.0) as f32, 1.0 - (background / 50.0) as f32 * 0.5);
                let tones: Vec<f32> = (0..w * h)
                    .map(|i| {
                        let p = centre(space.to_doc, i % w, i / w);
                        let n = 0.6 * unit(hash(cell(p.x, 0.5), cell(p.y, 0.5), 96)) + 0.4 * value(p.x / 1.5, p.y / 1.5, 97);
                        if l.get(i).copied().unwrap_or(1.0) < n as f32 + shift { ink } else { paper }
                    })
                    .collect();
                paint(px, |i| tones.get(i).copied().unwrap_or(1.0));
            }
            Sketch::Stamp { balance, smoothness } => {
                let l = soft_lumas(px, w, h, space, 0.2 * smoothness);
                let b = (balance / 50.0) as f32;
                paint(px, |i| step(l.get(i).copied().unwrap_or(1.0), b, 0.04));
            }
            Sketch::TornEdges { balance, smoothness, contrast } => {
                let l = soft_lumas(px, w, h, space, 0.25 * smoothness);
                let b = (balance / 50.0) as f32;
                let ramp = 0.1 / (contrast as f32 / 5.0).max(0.2);
                paint(px, |i| {
                    let p = centre(space.to_doc, i % w, i / w);
                    // The paper tears along a ragged line around the threshold.
                    let rag =
                        (value(p.x / 1.5, p.y / 1.5, 98) as f32 - 0.5) * 0.3 + (unit(hash(cell(p.x, 0.5), cell(p.y, 0.5), 99)) as f32 - 0.5) * 0.1;
                    step(l.get(i).copied().unwrap_or(1.0), b + rag, ramp)
                });
            }
            Sketch::WaterPaper { fiber, brightness, contrast } => {
                let Some(src) = Source::new(px, w, h, space) else { return };
                let len = fiber * 0.3;
                let lift = ((brightness - 50.0) / 50.0) as f32 * 0.5;
                let k = (contrast / 80.0) as f32;
                each(px, w, h, space, |p, _| {
                    // Fibres run in a random direction in each 6 pt patch of paper.
                    let angle = unit(hash(cell(p.x, 6.0), cell(p.y, 6.0), 100)) * std::f64::consts::PI;
                    let c = src.stroke(p, Vec2::new(angle.cos(), angle.sin()), len, 0.5, 101);
                    let a = c[3];
                    let f = |v: f32| ((v - 0.5 * a) * k + 0.5 * a + lift * a).clamp(0.0, a);
                    [f(c[0]), f(c[1]), f(c[2]), a]
                });
            }
        }
    }
}

/// Bas Relief's blur in points: less detail and more smoothness soften the relief.
fn soft(detail: f64, smoothness: f64) -> f64 {
    0.15 * (16.0 - detail) + 0.2 * smoothness
}
