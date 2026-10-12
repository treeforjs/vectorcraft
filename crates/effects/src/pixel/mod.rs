//! Photoshop-style Blur, Sharpen, Stylize and Pixelate effects over premultiplied RGBA8 pixels.
//!
//! - Every distance is in document units (points) and becomes pixels through the raster's
//!   [`PixelSpace::px`], so an effect looks the same at any zoom and at any Document Raster Effects
//!   Settings resolution (as Gaussian Blur's radius does).
//! - Blurs use box approximations or bounded sample grids. Pixelate averages cap their cell maps
//!   and use exact tiled/sorted fallbacks for sparse or extreme affine grids.
//! - Beyond the raster's edges is transparency.

mod blur;
mod edges;
mod groups;
mod pixelate;
mod sharpen;

use serde_json::Value;
use vectorcraft_geom::{Affine, Point, Rect};

use crate::util::{num, text};

/// The Photoshop-style effect ids (all raster effects, see [`crate::is_raster`]).
pub const PIXEL_EFFECTS: [&str; 11] = [
    "blur.radial",
    "blur.smart",
    "sharpen.unsharpMask",
    "stylize.glowingEdges",
    "pixelate.mosaic",
    "pixelate.colorHalftone",
    "pixelate.crystallize",
    "pixelate.pointillize",
    "pixelate.fragment",
    "pixelate.mezzotint",
    "pixelate.facet",
];

/// Smart Blur's output: smoothed colour, white transitions on black, or white transitions over
/// the original colour. Both edge modes preserve the coverage of the artwork.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum SmartMode {
    #[default]
    Normal,
    EdgeOnly,
    OverlayEdge,
}

/// A Photoshop-style raster effect with its parameters read and clamped to their ranges. Lengths
/// are in document units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PixelFx {
    /// Blur › Radial Blur around the object's centre: along arcs (spin, an arc of `amount`°) or
    /// along rays (zoom, the content scaled by up to about ±`amount`/2 %), averaging 2^`passes` samples.
    RadialBlur { amount: f64, zoom: bool, passes: u32 },
    /// Blur › Smart Blur: each pixel averaged with the neighbours within `radius`
    /// whose colour differs from its own by no more than `threshold` levels, over a grid of
    /// `samples` × `samples` neighbours at most.
    SmartBlur { radius: f64, threshold: f64, samples: u32, mode: SmartMode },
    /// Sharpen › Unsharp Mask: colours pushed away from a Gaussian blur of σ = `radius` by `amount`
    /// (1 = 100 %), where they differ from it by at least `threshold` levels.
    UnsharpMask { amount: f64, radius: f64, threshold: f64 },
    /// Stylize › Glowing Edges: bright coloured Sobel outlines on black.
    GlowingEdges { width: f64, brightness: f64, smoothness: f64 },
    /// Square cells of alpha-weighted mean colour, preserving the original coverage.
    Mosaic { cell_size: f64 },
    /// Three independently rotated RGB channel screens. Dark ink coverage follows each cell's
    /// alpha-weighted mean channel value. This is an RGB effect, not CMYK plate generation.
    ColorHalftone { radius: f64, angles: [f64; 3] },
    /// Deterministic jittered Voronoi cells with alpha-weighted mean colour.
    Crystallize { cell_size: f64, seed: u64 },
    /// Coloured dots at the Voronoi sites, with an explicit RGB background between them.
    Pointillize { cell_size: f64, seed: u64, background: [u8; 3] },
    /// Four diagonally displaced copies averaged in premultiplied colour.
    Fragment { offset: f64 },
    /// Deterministic binary channel texture with document-space grain dimensions.
    Mezzotint { width: f64, height: f64, seed: u64 },
    /// Mean colours of similar-colour clusters within small square cells.
    Facet { cell_size: f64, levels: u8 },
}

/// Where a raster's pixels lie in the document.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PixelSpace {
    /// Pixel coordinates (x right, y down, (0, 0) the top-left corner of the first pixel) →
    /// document coordinates.
    pub to_doc: Affine,
    /// The size of a pixel in document units.
    pub px: f64,
    /// The object's centre (document coordinates): where Radial Blur blurs around.
    pub center: Point,
}

/// Effect `id`'s parameters `p` (defaults merged in) as a [`PixelFx`]; `None` for other effects.
pub(crate) fn parse(id: &str, p: &Value) -> Option<PixelFx> {
    let quality = |levels: [(&str, u32); 3], default: u32| {
        let q = text(p, "quality", "");
        levels.iter().find(|(name, _)| name.eq_ignore_ascii_case(q)).map_or(default, |(_, v)| *v)
    };
    Some(match id {
        "blur.radial" => PixelFx::RadialBlur {
            amount: num(p, "amount", 10.0).clamp(1.0, 100.0),
            zoom: text(p, "method", "spin").eq_ignore_ascii_case("zoom"),
            passes: quality([("draft", 4), ("good", 6), ("best", 8)], 6),
        },
        "blur.smart" => PixelFx::SmartBlur {
            radius: num(p, "radius", 3.0).clamp(0.1, 100.0),
            threshold: num(p, "threshold", 25.0).clamp(0.1, 100.0),
            samples: quality([("low", 5), ("medium", 7), ("high", 9)], 7),
            mode: match text(p, "mode", "normal").to_ascii_lowercase().as_str() {
                "edgeonly" => SmartMode::EdgeOnly,
                "overlayedge" => SmartMode::OverlayEdge,
                _ => SmartMode::Normal,
            },
        },
        "sharpen.unsharpMask" => PixelFx::UnsharpMask {
            amount: num(p, "amount", 50.0).clamp(1.0, 500.0) / 100.0,
            radius: num(p, "radius", 1.0).clamp(0.1, 250.0),
            threshold: num(p, "threshold", 0.0).clamp(0.0, 255.0),
        },
        "stylize.glowingEdges" => PixelFx::GlowingEdges {
            width: num(p, "edgeWidth", 2.0).clamp(1.0, 14.0),
            brightness: num(p, "edgeBrightness", 6.0).clamp(0.0, 20.0),
            smoothness: num(p, "smoothness", 5.0).clamp(1.0, 15.0),
        },
        "pixelate.mosaic" => PixelFx::Mosaic { cell_size: num(p, "cellSize", 10.0).clamp(2.0, 200.0) },
        "pixelate.colorHalftone" => PixelFx::ColorHalftone {
            radius: num(p, "radius", 8.0).clamp(2.0, 127.0),
            angles: [num(p, "angle1", 108.0), num(p, "angle2", 162.0), num(p, "angle3", 90.0)].map(|a| a.rem_euclid(360.0)),
        },
        "pixelate.crystallize" => PixelFx::Crystallize { cell_size: num(p, "cellSize", 10.0).clamp(3.0, 300.0), seed: crate::util::seed(p) },
        "pixelate.pointillize" => {
            let color = vectorcraft_doc::color::Color::from_hex(text(p, "backgroundColor", "#ffffff"))
                .unwrap_or(vectorcraft_doc::color::Color::WHITE)
                .to_rgba8(1.0);
            PixelFx::Pointillize {
                cell_size: num(p, "cellSize", 10.0).clamp(3.0, 300.0),
                seed: crate::util::seed(p),
                background: [color[0], color[1], color[2]],
            }
        }
        "pixelate.fragment" => PixelFx::Fragment { offset: num(p, "offset", 2.0).clamp(0.1, 100.0) },
        "pixelate.mezzotint" => {
            let (width, height) = match text(p, "type", "fineDots") {
                "mediumDots" => (2.0, 2.0),
                "grainyDots" => (1.5, 2.5),
                "coarseDots" => (4.0, 4.0),
                "shortStrokes" => (2.0, 5.0),
                "mediumStrokes" => (2.0, 10.0),
                "longStrokes" => (2.0, 20.0),
                "shortLines" => (10.0, 1.0),
                "mediumLines" => (20.0, 1.0),
                "longLines" => (40.0, 1.0),
                _ => (1.0, 1.0),
            };
            let grain = num(p, "grainSize", 1.0).clamp(0.1, 20.0);
            PixelFx::Mezzotint { width: width * grain, height: height * grain, seed: crate::util::seed(p) }
        }
        "pixelate.facet" => {
            PixelFx::Facet { cell_size: num(p, "cellSize", 3.0).clamp(2.0, 100.0), levels: num(p, "levels", 8.0).round().clamp(2.0, 64.0) as u8 }
        }
        _ => return None,
    })
}

impl PixelFx {
    /// How far beyond `bounds` (the content's, document units) the effect can paint.
    pub fn outset(&self, bounds: Rect) -> f64 {
        // From the centre to the farthest corner, and to the nearest side.
        let far = 0.5 * bounds.width().hypot(bounds.height());
        let near = 0.5 * bounds.width().min(bounds.height());
        match *self {
            // A rotated corner reaches the circle through the corners.
            PixelFx::RadialBlur { zoom: false, .. } => (far - near).max(0.0),
            PixelFx::RadialBlur { amount, zoom: true, .. } => far * (blur::zoom_extent(amount).exp() - 1.0),
            PixelFx::SmartBlur { radius, .. } => radius,
            PixelFx::UnsharpMask { .. } => 0.0,
            PixelFx::GlowingEdges { width, smoothness, .. } => width.max(smoothness * 3.0),
            PixelFx::Mosaic { .. } | PixelFx::ColorHalftone { .. } | PixelFx::Crystallize { .. } | PixelFx::Pointillize { .. } => 0.0,
            PixelFx::Fragment { offset } => offset,
            PixelFx::Mezzotint { .. } | PixelFx::Facet { .. } => 0.0,
        }
    }

    /// How far (document units) from a pixel the content that decides it lies; `None` when that
    /// is anywhere in the object (Radial Blur).
    pub fn reach(&self) -> Option<f64> {
        match *self {
            PixelFx::RadialBlur { .. } => None,
            PixelFx::SmartBlur { radius, .. } => Some(radius),
            PixelFx::UnsharpMask { radius, .. } => Some(3.0 * radius),
            PixelFx::GlowingEdges { width, smoothness, .. } => Some(width.max(smoothness * 3.0)),
            PixelFx::Mosaic { cell_size } => Some(cell_size * std::f64::consts::SQRT_2),
            PixelFx::ColorHalftone { radius, .. } => Some(2.0 * radius),
            PixelFx::Crystallize { cell_size, .. } | PixelFx::Pointillize { cell_size, .. } => Some(3.0 * cell_size),
            PixelFx::Fragment { offset } => Some(offset * std::f64::consts::SQRT_2),
            PixelFx::Mezzotint { .. } => Some(0.0),
            PixelFx::Facet { cell_size, .. } => Some(cell_size * std::f64::consts::SQRT_2),
        }
    }

    /// Run the effect on `data`, `w` × `h` premultiplied RGBA8 pixels lying in the document as
    /// `space` says. Does nothing when the sizes don't match.
    pub fn apply(&self, data: &mut [u8], w: usize, h: usize, space: &PixelSpace) {
        let Some(px) = pixels(data, w, h) else { return };
        let to_px = |len: f64| len / space.px.max(1e-9);
        match *self {
            PixelFx::RadialBlur { amount, zoom, passes } => blur::radial(px, w, h, space, amount, zoom, passes),
            PixelFx::SmartBlur { radius, threshold, samples, mode } => blur::smart(px, w, h, to_px(radius), threshold, samples, mode),
            PixelFx::UnsharpMask { amount, radius, threshold } => sharpen::unsharp(px, w, h, amount, to_px(radius), threshold),
            PixelFx::GlowingEdges { width, brightness, smoothness } => edges::glow(px, w, h, to_px(width), brightness, to_px(smoothness)),
            PixelFx::Mosaic { cell_size } => pixelate::mosaic(px, w, space, cell_size),
            PixelFx::ColorHalftone { radius, angles } => pixelate::halftone(px, w, space, radius, angles),
            PixelFx::Crystallize { cell_size, seed } => pixelate::crystals(px, w, space, cell_size, seed, None),
            PixelFx::Pointillize { cell_size, seed, background } => pixelate::crystals(px, w, space, cell_size, seed, Some(background)),
            PixelFx::Fragment { offset } => pixelate::fragment(px, w, h, space, offset),
            PixelFx::Mezzotint { width, height, seed } => pixelate::mezzotint(px, w, space, width, height, seed),
            PixelFx::Facet { cell_size, levels } => pixelate::facet(px, w, space, cell_size, levels),
        }
    }
}

/// `data` as `w` × `h` RGBA pixels, `None` when it isn't that size (or is empty).
fn pixels(data: &mut [u8], w: usize, h: usize) -> Option<&mut [[u8; 4]]> {
    let (px, rest) = data.as_chunks_mut::<4>();
    (w > 0 && h > 0 && rest.is_empty() && w.checked_mul(h) == Some(px.len())).then_some(px)
}

/// The largest box radius the blurs use: wider boxes look the same on any raster we make.
const MAX_BOX: usize = 1 << 17;

/// Radii of three box blurs approximating a Gaussian of `sigma` pixels.
pub fn gauss_boxes(sigma: f64) -> [usize; 3] {
    let sigma = if sigma.is_finite() { sigma.clamp(0.0, MAX_BOX as f64 / 2.0) } else { 0.0 };
    let n = 3.0;
    let w_ideal = (12.0 * sigma * sigma / n + 1.0).sqrt();
    let mut wl = w_ideal.floor() as i64;
    if wl % 2 == 0 {
        wl -= 1;
    }
    let wu = wl + 2;
    let m_ideal = (12.0 * sigma * sigma - n * (wl * wl) as f64 - 4.0 * n * wl as f64 - 3.0 * n) / (-4.0 * wl as f64 - 4.0);
    let m = m_ideal.round() as i64;
    let r = |i: i64| ((((if i < m { wl } else { wu }) - 1) / 2).max(0) as usize).min(MAX_BOX);
    [r(0), r(1), r(2)]
}

/// One horizontal box blur of radius `r` over the rows of `w` values of a plane (running sum,
/// zero past the ends).
fn box_plane(src: &[f32], dst: &mut [f32], w: usize, r: usize) {
    if r == 0 {
        dst.copy_from_slice(src);
        return;
    }
    let norm = 1.0 / (2 * r + 1) as f32;
    for (row, out) in src.chunks_exact(w).zip(dst.chunks_exact_mut(w)) {
        let mut acc: f32 = row.iter().take(r + 1).sum();
        for (x, o) in out.iter_mut().enumerate() {
            *o = acc * norm;
            if let Some(v) = row.get(x + r + 1) {
                acc += v;
            }
            if let Some(v) = x.checked_sub(r).and_then(|i| row.get(i)) {
                acc -= v;
            }
        }
    }
}

fn transpose<T: Copy>(src: &[T], dst: &mut [T], w: usize, h: usize) {
    for (y, row) in src.chunks_exact(w).enumerate().take(h) {
        for (x, v) in row.iter().enumerate() {
            if let Some(d) = dst.get_mut(x * h + y) {
                *d = *v;
            }
        }
    }
}

/// Gaussian-blur a `w` × `h` plane of values in place (three box passes per axis, zero past the
/// edges). Does nothing when the sizes don't match.
pub fn blur_plane(a: &mut Vec<f32>, w: usize, h: usize, sigma: f64) {
    if sigma < 0.2 || w == 0 || h == 0 || w.checked_mul(h) != Some(a.len()) {
        return;
    }
    let boxes = gauss_boxes(sigma);
    let mut tmp = vec![0.0; a.len()];
    for (len, other) in [(w, h), (h, w)] {
        for r in boxes {
            box_plane(a, &mut tmp, len, r);
            std::mem::swap(a, &mut tmp);
        }
        transpose(a, &mut tmp, len, other);
        std::mem::swap(a, &mut tmp);
    }
}

/// Premultiplied RGBA at 16 bits (×257): blurs and averages that keep their tones.
pub(super) type Px16 = [u16; 4];

pub(super) fn to16(px: &[[u8; 4]]) -> Vec<Px16> {
    px.iter().map(|p| p.map(|v| v as u16 * 257)).collect()
}

pub(super) fn to8(v: u16) -> u8 {
    ((v as u32 + 128) / 257).min(255) as u8
}

fn add(acc: &mut [u64; 4], p: &Px16) {
    for (a, v) in acc.iter_mut().zip(p) {
        *a += *v as u64;
    }
}

fn sub(acc: &mut [u64; 4], p: &Px16) {
    for (a, v) in acc.iter_mut().zip(p) {
        *a = a.saturating_sub(*v as u64);
    }
}

fn mean(acc: &[u64; 4], n: u64) -> Px16 {
    acc.map(|a| ((a + n / 2) / n.max(1)).min(65535) as u16)
}

/// One box blur of radius `r` along the rows of `w` pixels (transparent past the ends).
fn box_rows(src: &[Px16], dst: &mut [Px16], w: usize, r: usize) {
    let n = 2 * r as u64 + 1;
    for (row, out) in src.chunks_exact(w).zip(dst.chunks_exact_mut(w)) {
        let mut acc = [0u64; 4];
        row.iter().take(r + 1).for_each(|p| add(&mut acc, p));
        for (x, o) in out.iter_mut().enumerate() {
            *o = mean(&acc, n);
            if let Some(p) = row.get(x + r + 1) {
                add(&mut acc, p);
            }
            if let Some(p) = x.checked_sub(r).and_then(|i| row.get(i)) {
                sub(&mut acc, p);
            }
        }
    }
}

/// One box blur of radius `r` down the columns of `w`-wide rows (transparent past the ends), row
/// by row with a running sum per column.
fn box_cols(src: &[Px16], dst: &mut [Px16], w: usize, r: usize) {
    let n = 2 * r as u64 + 1;
    let rows: Vec<&[Px16]> = src.chunks_exact(w).collect();
    let mut acc = vec![[0u64; 4]; w];
    let add_row = |acc: &mut [[u64; 4]], row: &[Px16]| acc.iter_mut().zip(row).for_each(|(a, p)| add(a, p));
    rows.iter().take(r + 1).for_each(|row| add_row(&mut acc, row));
    for (y, out) in dst.chunks_exact_mut(w).enumerate() {
        for (o, a) in out.iter_mut().zip(&acc) {
            *o = mean(a, n);
        }
        if let Some(row) = rows.get(y + r + 1) {
            add_row(&mut acc, row);
        }
        if let Some(row) = y.checked_sub(r).and_then(|i| rows.get(i)) {
            acc.iter_mut().zip(*row).for_each(|(a, p)| sub(a, p));
        }
    }
}

/// Box-blur `w`-wide rows of pixels in place with radius `r` along both axes.
pub(super) fn box_blur(px: &mut [Px16], w: usize, r: usize) {
    if r == 0 || w == 0 {
        return;
    }
    let mut tmp = vec![[0u16; 4]; px.len()];
    box_rows(px, &mut tmp, w, r);
    box_cols(&tmp, px, w, r);
}

/// Gaussian-blur `w`-wide rows of premultiplied pixels in place (σ = `sigma` pixels, three box
/// passes per axis, transparent past the edges).
pub(super) fn gaussian(px: &mut [Px16], w: usize, sigma: f64) {
    if sigma < 0.2 || w == 0 {
        return;
    }
    let mut tmp = vec![[0u16; 4]; px.len()];
    for r in gauss_boxes(sigma) {
        box_rows(px, &mut tmp, w, r);
        px.copy_from_slice(&tmp);
    }
    for r in gauss_boxes(sigma) {
        box_cols(px, &mut tmp, w, r);
        px.copy_from_slice(&tmp);
    }
}

/// Gaussian-blur `data`, `w` × `h` premultiplied RGBA8 pixels, in place (σ = `sigma` pixels). Does
/// nothing when the sizes don't match.
pub fn gaussian_rgba(data: &mut [u8], w: usize, h: usize, sigma: f64) {
    if let Some(px) = pixels(data, w, h)
        && sigma >= 0.2
    {
        let mut wide = to16(px);
        gaussian(&mut wide, w, sigma);
        for (p, q) in px.iter_mut().zip(&wide) {
            *p = q.map(to8);
        }
    }
}

#[cfg(test)]
mod tests;
