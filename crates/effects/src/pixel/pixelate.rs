//! Original Pixelate kernels based on the public qualitative filter descriptions, not on another
//! application's output. The lattice is in document coordinates relative to the object's centre:
//! clipping a viewport, panning, or changing raster resolution cannot reset its phase.

use std::collections::HashMap;

use vectorcraft_geom::{Affine, Point, Rect};

use super::{
    PixelSpace,
    groups::{self, Cell},
};

type Key = (i64, i64);

fn grid(space: &PixelSpace, size: f64, angle: f64) -> Option<Affine> {
    if !size.is_finite() || size <= 0.0 || !angle.is_finite() || !space.px.is_finite() || space.px <= 0.0 {
        return None;
    }
    let transform = Affine::translate((0.5, 0.5))
        * Affine::scale(1.0 / size)
        * Affine::rotate(-angle.to_radians())
        * Affine::translate(-space.center.to_vec2())
        * space.to_doc;
    transform.as_coeffs().iter().all(|v| v.is_finite()).then_some(transform)
}

fn key(p: Point) -> Key {
    // Float-to-integer conversion saturates even for documents close to the numeric limits.
    let floor = |v: f64| {
        let rounded = v.round();
        // Equivalent view matrices (notably quarter turns) can put an exact grid boundary a
        // handful of ULPs on either side. Cancellation near zero needs an absolute floor as
        // well. Snap a subpixel numerical neighbourhood; cap it so large coordinates cannot
        // widen that neighbourhood into a visible strip of the cell.
        let tolerance = (32.0 * f64::EPSILON * v.abs().max(1.0)).clamp(1e-10, 1e-7);
        if (v - rounded).abs() <= tolerance { rounded } else { v.floor() }
    };
    (floor(p.x) as i64, floor(p.y) as i64)
}

/// A sufficient (never approximate) test that no two raster sample centres can share a cell.
/// For the usual axis-aligned grid, steps of one cell suffice. Otherwise |det|/Frobenius-norm
/// bounds the smallest singular value from below; exceeding a cell's diameter separates every
/// pair of samples. Downsampled previews then need no per-pixel hash-map entries.
fn separated(transform: Affine, diameter: f64, width: usize, height: usize) -> bool {
    // Saturating integer keys at extreme coordinates can merge otherwise distant samples.
    if [
        Point::new(0.5, 0.5),
        Point::new(width as f64 - 0.5, 0.5),
        Point::new(0.5, height as f64 - 0.5),
        Point::new(width as f64 - 0.5, height as f64 - 0.5),
    ]
    .iter()
    .map(|p| transform * *p)
    .any(|q| !q.x.is_finite() || !q.y.is_finite() || q.x.abs() > 1e9 || q.y.abs() > 1e9)
    {
        return false;
    }

    let [a, b, c, d, _, _] = transform.as_coeffs();
    if diameter == std::f64::consts::SQRT_2 && b == 0.0 && c == 0.0 && a.abs() > 1.0 + 2e-7 && d.abs() > 1.0 + 2e-7 {
        return true;
    }
    // Camera rotations have orthogonal columns. Their exact minimum stretch is the shorter
    // column, avoiding the conservative √2 loss of the general Frobenius bound below.
    if a * c + b * d == 0.0 && a.hypot(b).min(c.hypot(d)) > diameter * (1.0 + 1e-7) {
        return true;
    }
    let bound = (a * d - b * c).abs() / a.hypot(b).hypot(c.hypot(d));
    bound.is_finite() && bound > diameter * (1.0 + 1e-7)
}

/// Same-colour square cells. Retaining each pixel's original alpha makes this a colour effect:
/// it cannot reveal hidden RGB values, fill holes, or paint outside the artwork's coverage.
pub(super) fn mosaic(px: &mut [[u8; 4]], w: usize, space: &PixelSpace, size: f64) {
    let Some(transform) = grid(space, size, 0.0) else { return };
    if separated(transform, std::f64::consts::SQRT_2, w, px.len() / w) {
        return;
    }
    groups::render(
        px,
        w,
        transform,
        std::f64::consts::SQRT_2,
        |q, _| key(q),
        |_| {},
        |_, p, cell| {
            let Some(cell) = cell else {
                *p = [0; 4];
                return;
            };
            let alpha = p[3];
            for (out, total) in p.iter_mut().take(3).zip(cell.sum) {
                *out = (total as f64 / cell.sum[3] as f64 * f64::from(alpha)).round().clamp(0.0, f64::from(alpha)) as u8;
            }
        },
    );
}

/// Radius of a centred circle whose intersection with the unit square has area `coverage`.
/// Solving the clipped-circle area also represents dark tones correctly: a black cell fills all
/// four corners instead of leaving white pinholes when the nominal circle reaches the sides.
fn dot_radius(coverage: f64) -> f64 {
    if coverage <= std::f64::consts::FRAC_PI_4 {
        return (coverage.max(0.0) / std::f64::consts::PI).sqrt();
    }
    let (mut lo, mut hi) = (0.5, std::f64::consts::FRAC_1_SQRT_2);
    for _ in 0..16 {
        let r = (lo + hi) * 0.5;
        let area = std::f64::consts::PI * r * r - 4.0 * (r * r * (0.5 / r).acos() - 0.5 * (r * r - 0.25).sqrt());
        if area < coverage { lo = r } else { hi = r }
    }
    (lo + hi) * 0.5
}

/// RGB screen simulation. Each channel has an independent angle and cell average; a white channel
/// has no ink dots and a black channel fills its cell. Original alpha is preserved. This effect
/// operates before output colour conversion and does not claim to create four CMYK separations.
pub(super) fn halftone(px: &mut [[u8; 4]], w: usize, space: &PixelSpace, radius: f64, angles: [f64; 3]) {
    let size = std::f64::consts::SQRT_2 * radius;
    for (channel, angle) in angles.into_iter().enumerate() {
        let Some(transform) = grid(space, size, angle) else { continue };
        // Earlier passes only changed other channels, so this channel and alpha are original.
        let aa = (space.px / size).clamp(1e-9, 1.0);
        let paint = |q: Point, p: &mut [u8; 4], cell: Option<&Cell>| {
            let Some(cell) = cell else {
                p[channel] = 0;
                return;
            };
            let tone = (cell.sum[channel] as f64 / cell.sum[3] as f64).clamp(0.0, 1.0);
            let distance = (q.x - q.x.floor() - 0.5).hypot(q.y - q.y.floor() - 0.5);
            let value = if tone == 0.0 || tone == 1.0 { tone } else { ((distance - cell.radius) / aa + 0.5).clamp(0.0, 1.0) };
            p[channel] = (value * f64::from(p[3])).round() as u8;
        };
        if separated(transform, std::f64::consts::SQRT_2, w, px.len() / w) {
            // Exact memoization: isolated samples have only 256² possible channel/alpha pairs.
            let mut radii = vec![f64::NAN; 65_536];
            for (y, row) in px.chunks_exact_mut(w).enumerate() {
                for (x, p) in row.iter_mut().enumerate() {
                    if p[3] == 0 {
                        p[channel] = 0;
                        continue;
                    }
                    let index = usize::from(p[3]) * 256 + usize::from(p[channel]);
                    let Some(radius) = radii.get_mut(index) else { continue };
                    if radius.is_nan() {
                        *radius = dot_radius((1.0 - f64::from(p[channel]) / f64::from(p[3])).clamp(0.0, 1.0));
                    }
                    let cell = Cell { sum: p.map(u64::from), radius: *radius };
                    paint(transform * Point::new(x as f64 + 0.5, y as f64 + 0.5), p, Some(&cell));
                }
            }
        } else {
            groups::render(
                px,
                w,
                transform,
                std::f64::consts::SQRT_2,
                |q, _| key(q),
                |cell| {
                    cell.radius = dot_radius((1.0 - cell.sum[channel] as f64 / cell.sum[3] as f64).clamp(0.0, 1.0));
                },
                paint,
            );
        }
    }
}

/// One site in the central half of each unit cell. A point's own site is at most 0.75√2 away;
/// sites beyond its eight neighbours are at least 1.25 away, so nine candidates suffice.
fn nearest(p: Point, seed: u64) -> (Key, f64) {
    let base = key(p);
    let mut best = (base, f64::INFINITY);
    for dy in -1i64..=1 {
        for dx in -1i64..=1 {
            let (Some(x), Some(y)) = (base.0.checked_add(dx), base.1.checked_add(dy)) else { continue };
            let sx = x as f64 + 0.5 + 0.25 * crate::util::noise(seed, x as u64, y as u64, 0);
            let sy = y as f64 + 0.5 + 0.25 * crate::util::noise(seed, x as u64, y as u64, 1);
            let distance = (p.x - sx).hypot(p.y - sy);
            if distance < best.1 {
                best = ((x, y), distance);
            }
        }
    }
    best
}

/// Voronoi mosaics or pointillist dots. Seeded document-space sites make results reproducible in
/// preview, crop, native files, and exports. Averaging never samples unrelated transparent RGB.
pub(super) fn crystals(px: &mut [[u8; 4]], w: usize, space: &PixelSpace, size: f64, seed: u64, background: Option<[u8; 3]>) {
    let Some(transform) = grid(space, size, 0.0) else { return };
    let aa = (space.px / size).clamp(1e-9, 1.0);
    let paint = |q: Point, p: &mut [u8; 4], cell: Option<&Cell>| {
        let Some(cell) = cell else {
            *p = [0; 4];
            return;
        };
        let colour = std::array::from_fn::<_, 3, _>(|i| cell.sum[i] as f64 / cell.sum[3] as f64);
        let dot = if background.is_some() { ((0.4 - nearest(q, seed).1) / aa + 0.5).clamp(0.0, 1.0) } else { 1.0 };
        let alpha = p[3];
        for (channel, out) in p.iter_mut().take(3).enumerate() {
            let colour = background.map_or(colour[channel], |bg| colour[channel] * dot + f64::from(bg[channel]) / 255.0 * (1.0 - dot));
            *out = (colour * f64::from(alpha)).round().clamp(0.0, f64::from(alpha)) as u8;
        }
    };
    if separated(transform, 3.0, w, px.len() / w) {
        for (y, row) in px.chunks_exact_mut(w).enumerate() {
            for (x, p) in row.iter_mut().enumerate() {
                let cell = Cell { sum: p.map(u64::from), radius: 0.0 };
                let covered = p[3] != 0;
                paint(transform * Point::new(x as f64 + 0.5, y as f64 + 0.5), p, covered.then_some(&cell));
            }
        }
    } else {
        groups::render(px, w, transform, 3.0, |q, _| nearest(q, seed).0, |_| {}, paint);
    }
}

/// Four translated copies of the source, averaged including alpha. The offsets are document
/// vectors rather than screen vectors, so rotating a view does not rotate the filter itself.
pub(super) fn fragment(px: &mut [[u8; 4]], w: usize, h: usize, space: &PixelSpace, offset: f64) {
    let inverse = space.to_doc.inverse();
    let vectors =
        [(-offset, -offset), (offset, -offset), (-offset, offset), (offset, offset)].map(|(x, y)| inverse * Point::new(x, y) - inverse * Point::ZERO);
    if vectors.iter().any(|v| !v.x.is_finite() || !v.y.is_finite()) {
        return;
    }
    let source = super::to16(px);
    for (y, row) in px.chunks_exact_mut(w).enumerate() {
        for (x, p) in row.iter_mut().enumerate() {
            let mut sum = [0.0f32; 4];
            for delta in vectors {
                let sampled = super::blur::bilinear(&source, w, h, x as f64 + delta.x, y as f64 + delta.y);
                for (total, value) in sum.iter_mut().zip(sampled) {
                    *total += value;
                }
            }
            *p = sum.map(|v| super::to8((v * 0.25).round().clamp(0.0, 65535.0) as u16));
        }
    }
}

/// Reproducible binary RGB texture. Dot/stroke/line types select the grain's aspect and size;
/// shared channel thresholds preserve neutral greys and generate saturated colours from colour.
pub(super) fn mezzotint(px: &mut [[u8; 4]], w: usize, space: &PixelSpace, width: f64, height: f64, seed: u64) {
    let Some(transform) = grid(space, 1.0, 0.0) else { return };
    if !width.is_finite() || width <= 0.0 || !height.is_finite() || height <= 0.0 {
        return;
    }
    for (y, row) in px.chunks_exact_mut(w).enumerate() {
        for (x, p) in row.iter_mut().enumerate() {
            let q = transform * Point::new(x as f64 + 0.5, y as f64 + 0.5);
            let (gx, gy) = key(Point::new((q.x - 0.5) / width + 0.5, (q.y - 0.5) / height + 0.5));
            let threshold = (crate::util::noise(seed, gx as u64, gy as u64, 2) + 1.0) * 0.5;
            let alpha = p[3];
            for channel in p.iter_mut().take(3) {
                *channel = if f64::from(*channel) > threshold * f64::from(alpha) { alpha } else { 0 };
            }
        }
    }
}

/// Colour-aware small blocks: unlike Mosaic, dissimilar colours in the same spatial block are
/// not averaged together. The documented clumping behavior is implemented with a bounded RGB
/// partition and exact alpha-weighted averages, with no data-dependent region-growing loops.
pub(super) fn facet(px: &mut [[u8; 4]], w: usize, space: &PixelSpace, size: f64, levels: u8) {
    let Some(transform) = grid(space, size, 0.0) else { return };
    if separated(transform, std::f64::consts::SQRT_2, w, px.len() / w) {
        return;
    }
    let levels = u16::from(levels.clamp(2, 64));
    let color_key = |p: &[u8; 4]| {
        let alpha = u16::from(p[3]).max(1);
        std::array::from_fn::<_, 3, _>(|i| (u16::from(p[i]) * levels / alpha).min(levels - 1) as u8)
    };
    if facet_cells(px, w, transform, color_key) {
        return;
    }
    // The generic fallback also bounds memory and scan work for skewed/extreme PixelSpaces.
    groups::render(
        px,
        w,
        transform,
        std::f64::consts::SQRT_2,
        |q, p| (key(q), color_key(p)),
        |_| {},
        |_, p, cell| {
            let Some(cell) = cell else {
                *p = [0; 4];
                return;
            };
            let alpha = p[3];
            for (out, total) in p.iter_mut().take(3).zip(cell.sum) {
                *out = (total as f64 / cell.sum[3] as f64 * f64::from(alpha)).round().clamp(0.0, f64::from(alpha)) as u8;
            }
        },
    );
}

/// Process disjoint spatial cells separately, reusing a colour-only map (at most 64³ entries).
/// The inverse cell bounds include candidate pixels for rotated views; exact key membership
/// excludes adjacent cells. Earlier writes cannot affect a later cell's source samples.
fn facet_cells(px: &mut [[u8; 4]], w: usize, transform: Affine, color_key: impl Fn(&[u8; 4]) -> [u8; 3]) -> bool {
    let h = px.len() / w;
    let bounds = transform.transform_rect_bbox(Rect::new(0.5, 0.5, w as f64 - 0.5, h as f64 - 0.5));
    if [bounds.x0, bounds.y0, bounds.x1, bounds.y1].iter().any(|v| !v.is_finite() || v.abs() > 1e9) {
        return false;
    }
    let (start, end) = (key(Point::new(bounds.x0, bounds.y0)), key(Point::new(bounds.x1, bounds.y1)));
    let count = (end.0 - start.0 + 1) as u64 * (end.1 - start.1 + 1) as u64;
    if count > (px.len() as u64).saturating_mul(8) {
        return false;
    }
    let inverse = transform.inverse();
    if inverse.as_coeffs().iter().any(|v| !v.is_finite()) {
        return false;
    }
    let [a, b, c, d, _, _] = inverse.as_coeffs();
    // A finite near-singular inverse can make every cell candidate box span the whole image.
    // Bound total candidate visits before the first write; the generic sorted fallback handles
    // these grids in O(N log N) instead of repeatedly traversing the same source pixels.
    let samples = |span: f64, cap: usize| ((span * (1.0 + 2e-7)).ceil() + 4.0).min(cap as f64) as u64;
    let scans = count.saturating_mul(samples(a.abs() + c.abs(), w)).saturating_mul(samples(b.abs() + d.abs(), h));
    if scans > (px.len() as u64).saturating_mul(16) {
        return false;
    }
    let mut groups: HashMap<[u8; 3], Cell> = HashMap::new();
    for cy in start.1..=end.1 {
        for cx in start.0..=end.0 {
            let cell = (cx, cy);
            // key() snaps in grid units, so expand before the inverse transform. A fixed pixel
            // epsilon would miss snapped samples when a single grid cell spans many pixels.
            let candidates =
                inverse.transform_rect_bbox(Rect::new(cx as f64 - 1e-7, cy as f64 - 1e-7, cx as f64 + 1.0 + 1e-7, cy as f64 + 1.0 + 1e-7));
            // Include boundary samples and inverse-transform roundoff; exact key membership
            // assigns each sample to precisely one cell.
            let epsilon = |v: f64| (32.0 * f64::EPSILON * v.abs()).max(1e-7);
            let lower = |v: f64, cap: usize| (v - 0.5 - epsilon(v)).ceil().clamp(0.0, cap as f64) as usize;
            let upper = |v: f64, cap: usize| ((v - 0.5 + epsilon(v)).floor() + 1.0).clamp(0.0, cap as f64) as usize;
            let (x0, x1, y0, y1) = (lower(candidates.x0, w), upper(candidates.x1, w), lower(candidates.y0, h), upper(candidates.y1, h));
            let belongs = |x: usize, y: usize| key(transform * Point::new(x as f64 + 0.5, y as f64 + 0.5)) == cell;
            groups.clear();
            for (y, row) in px.chunks_exact(w).enumerate().take(y1).skip(y0) {
                for (x, p) in row.iter().enumerate().take(x1).skip(x0) {
                    if p[3] == 0 || !belongs(x, y) {
                        continue;
                    }
                    let group = groups.entry(color_key(p)).or_default();
                    for (total, channel) in group.sum.iter_mut().zip(p) {
                        *total += u64::from(*channel);
                    }
                }
            }
            for (y, row) in px.chunks_exact_mut(w).enumerate().take(y1).skip(y0) {
                for (x, p) in row.iter_mut().enumerate().take(x1).skip(x0) {
                    if !belongs(x, y) {
                        continue;
                    }
                    let Some(group) = groups.get(&color_key(p)) else { continue };
                    let alpha = p[3];
                    for (out, total) in p.iter_mut().take(3).zip(group.sum) {
                        *out = (total as f64 / group.sum[3] as f64 * f64::from(alpha)).round().clamp(0.0, f64::from(alpha)) as u8;
                    }
                }
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Unbounded, source-only grouping is an independent oracle for the bounded per-cell path:
    /// no inverse bounding boxes and no writes while gathering any of the averages.
    fn facet_reference(source: &[[u8; 4]], w: usize, transform: Affine, levels: u8) -> Vec<[u8; 4]> {
        let classify = |index: usize, pixel: &[u8; 4]| {
            let point = transform * Point::new((index % w) as f64 + 0.5, (index / w) as f64 + 0.5);
            let color = pixel[..3]
                .iter()
                .map(|c| (u16::from(*c) * u16::from(levels) / u16::from(pixel[3]).max(1)).min(u16::from(levels - 1)))
                .collect::<Vec<_>>();
            (key(point), color)
        };
        let mut all: HashMap<_, [u64; 4]> = HashMap::new();
        for (index, pixel) in source.iter().enumerate().filter(|(_, p)| p[3] != 0) {
            let sum = all.entry(classify(index, pixel)).or_default();
            for channel in 0..4 {
                sum[channel] += u64::from(pixel[channel]);
            }
        }
        source
            .iter()
            .enumerate()
            .map(|(index, pixel)| {
                let Some(sum) = all.get(&classify(index, pixel)) else { return [0; 4] };
                std::array::from_fn(|channel| {
                    if channel == 3 {
                        pixel[3]
                    } else {
                        (sum[channel] as f64 / sum[3] as f64 * f64::from(pixel[3])).round().clamp(0.0, f64::from(pixel[3])) as u8
                    }
                })
            })
            .collect()
    }

    #[test]
    fn bounded_facet_matches_reference_under_rotated_sheared_and_snapped_cells() {
        let w = 65;
        let source: Vec<_> = (0..w * 47)
            .map(|i| {
                let alpha = [0, 73, 173, 255][i % 4];
                let rgb = [(i * 19 % 256) as u8, (i * 43 % 256) as u8, (i * 67 % 256) as u8].map(|v| (u16::from(v) * alpha / 255) as u8);
                [rgb[0], rgb[1], rgb[2], alpha as u8]
            })
            .collect();
        let transforms = [
            Affine::scale(1.0 / 7.0),
            Affine::translate((0.5, -0.5)) * Affine::scale(1.0 / 3.0),
            Affine::rotate(std::f64::consts::FRAC_PI_2) * Affine::scale(1.0 / 3.0),
            Affine::rotate(0.43) * Affine::scale(1.0 / 9.0),
            Affine::new([0.12, -0.07, 0.08, 0.21, -0.4, 0.2]),
            Affine::new([-0.3, 0.11, 0.17, 0.2, 10.5, -9.2]),
            Affine::scale(2.0),
            // The first sample of cell zero lies just below its mathematical boundary. key()
            // snaps it upward; inverse bounds must include the corresponding pixel distance.
            Affine::new([1e-8, 0.0, 0.0, 0.1, -5.0505e-7, 0.0]),
        ];
        for transform in transforms {
            for levels in [2u8, 8, 64] {
                let mut actual = source.clone();
                let classify = |p: &[u8; 4]| {
                    std::array::from_fn(|i| (u16::from(p[i]) * u16::from(levels) / u16::from(p[3]).max(1)).min(u16::from(levels - 1)) as u8)
                };
                if !facet_cells(&mut actual, w, transform, classify) {
                    let space = PixelSpace { to_doc: transform, center: Point::new(0.5, 0.5), px: 1.0 };
                    facet(&mut actual, w, &space, 1.0, levels);
                }
                let expected = facet_reference(&source, w, transform, levels);
                let difference = actual.iter().zip(&expected).enumerate().find(|(_, (a, b))| a != b);
                assert!(difference.is_none(), "transform={transform:?}, levels={levels}, first difference={difference:?}");
            }
        }
    }

    #[test]
    fn facet_rejects_excessive_inverse_cell_scans_before_mutation() {
        let transform = Affine::new([0.5, 0.5, 0.5, 0.500000001, 0.0, 0.0]);
        let source: Vec<[u8; 4]> = (0..48 * 48).map(|i| [(i % 80) as u8, (i % 100) as u8, (i % 120) as u8, 255]).collect();
        let mut actual = source.clone();
        assert!(!facet_cells(&mut actual, 48, transform, |_| [0; 3]));
        assert_eq!(actual, source, "cost rejection must happen before any output write");
        let space = PixelSpace { to_doc: transform, center: Point::new(0.5, 0.5), px: 1.0 };
        facet(&mut actual, 48, &space, 1.0, 2);
        assert_eq!(actual, facet_reference(&source, 48, transform, 2));
    }

    #[test]
    fn separated_grid_fast_path_preserves_isolated_samples() {
        let source = vec![[25, 50, 75, 100], [0; 4], [17, 80, 63, 99], [50, 123, 230, 255]];
        for to_doc in [Affine::scale(30.0), Affine::rotate(0.72) * Affine::scale(30.0), Affine::new([30.0, 9.0, 7.0, 25.0, 0.0, 0.0])] {
            let space = PixelSpace { to_doc, center: Point::new(1.0, 1.0), px: 30.0 };
            let mut actual = source.clone();
            mosaic(&mut actual, 2, &space, 3.0);
            assert_eq!(actual, source);
            facet(&mut actual, 2, &space, 3.0, 64);
            assert_eq!(actual, source);
            crystals(&mut actual, 2, &space, 3.0, 7, None);
            assert_eq!(actual, source);
        }
    }

    #[test]
    fn halftone_isolated_sample_cache_matches_uncached_channel_averages() {
        let source: Vec<[u8; 4]> = (0..99)
            .map(|i| {
                let alpha = [0u16, 73, 173, 255][i % 4];
                let rgb = [(i * 19 % 256) as u8, (i * 43 % 256) as u8, (i * 67 % 256) as u8].map(|v| (u16::from(v) * alpha / 255) as u8);
                [rgb[0], rgb[1], rgb[2], alpha as u8]
            })
            .collect();
        let space = PixelSpace { to_doc: Affine::rotate(0.37) * Affine::scale(30.0), px: 30.0, center: Point::new(75.0, -19.0) };
        let angles = [108.0, 162.0, 90.0];
        let mut expected = source.clone();
        for (channel, angle) in angles.into_iter().enumerate() {
            let transform = grid(&space, 3.0 * std::f64::consts::SQRT_2, angle).unwrap();
            assert!(separated(transform, std::f64::consts::SQRT_2, 11, 9));
            let mut averages: HashMap<Key, [u64; 2]> = HashMap::new();
            for (i, p) in source.iter().enumerate().filter(|(_, p)| p[3] != 0) {
                let q = transform * Point::new((i % 11) as f64 + 0.5, (i / 11) as f64 + 0.5);
                let sum = averages.entry(key(q)).or_default();
                sum[0] += u64::from(p[channel]);
                sum[1] += u64::from(p[3]);
            }
            for (i, p) in expected.iter_mut().enumerate() {
                let q = transform * Point::new((i % 11) as f64 + 0.5, (i / 11) as f64 + 0.5);
                let Some(sum) = averages.get(&key(q)) else {
                    p[channel] = 0;
                    continue;
                };
                let tone = sum[0] as f64 / sum[1] as f64;
                let distance = (q.x - q.x.floor() - 0.5).hypot(q.y - q.y.floor() - 0.5);
                let value = if tone == 0.0 || tone == 1.0 { tone } else { (distance - dot_radius(1.0 - tone) + 0.5).clamp(0.0, 1.0) };
                p[channel] = (value * f64::from(p[3])).round() as u8;
            }
        }
        let mut actual = source;
        halftone(&mut actual, 11, &space, 3.0, angles);
        assert_eq!(actual, expected);
    }
}
