//! Blur › Radial Blur and Smart Blur.

use vectorcraft_geom::{Affine, Point, Vec2};

use super::{PixelSpace, Px16, box_blur, to8, to16};

/// Zoom's total spread, in log scale: `amount` 100 scales the content between 1/√2 and √2.
pub(super) fn zoom_extent(amount: f64) -> f64 {
    (1.0 + amount / 100.0).ln()
}

/// `buf` (`w` × `h`) sampled bilinearly at pixel index position (`x`, `y`) (pixel centres at
/// whole coordinates); transparent outside.
pub(super) fn bilinear(buf: &[Px16], w: usize, h: usize, x: f64, y: f64) -> [f32; 4] {
    if !(x > -1.0 && y > -1.0 && x < w as f64 && y < h as f64) {
        return [0.0; 4];
    }
    let (x0, y0) = (x.floor(), y.floor());
    let (fx, fy) = ((x - x0) as f32, (y - y0) as f32);
    let (xi, yi) = (x0 as isize, y0 as isize);
    let at = |cx: isize, cy: isize| -> [f32; 4] {
        if cx < 0 || cy < 0 || cx as usize >= w || cy as usize >= h {
            return [0.0; 4];
        }
        buf.get(cy as usize * w + cx as usize).map_or([0.0; 4], |p| p.map(f32::from))
    };
    let (a, b, c, d) = (at(xi, yi), at(xi + 1, yi), at(xi, yi + 1), at(xi + 1, yi + 1));
    let (wa, wb, wc, wd) = ((1.0 - fx) * (1.0 - fy), fx * (1.0 - fy), (1.0 - fx) * fy, fx * fy);
    std::array::from_fn(|i| a[i] * wa + b[i] * wb + c[i] * wc + d[i] * wd)
}

/// Radial Blur: the average of the content turned (spin) or scaled (zoom) about the centre over
/// evenly spaced steps spanning `amount`. Each pass averages the previous result with itself
/// turned both ways by half the last step, so `passes` passes average 2^`passes` samples at two
/// lookups a pixel each.
pub(super) fn radial(px: &mut [[u8; 4]], w: usize, h: usize, space: &PixelSpace, amount: f64, zoom: bool, passes: u32) {
    // The centre in pixel index coordinates (pixel centres at whole numbers).
    let c = space.to_doc.inverse() * space.center - Vec2::new(0.5, 0.5);
    if !(c.x.is_finite() && c.y.is_finite()) {
        return;
    }
    let total = if zoom { zoom_extent(amount) } else { amount.to_radians() };
    let about = |m: Affine| Affine::translate(c.to_vec2()) * m * Affine::translate(-c.to_vec2());
    // A pixel's reflection in the view (one axis flipped) only turns the spin the other way,
    // which a symmetric blur doesn't notice.
    let step = |d: f64| if zoom { about(Affine::scale(d.exp())) } else { about(Affine::rotate(d)) };
    let mut cur = to16(px);
    let mut next = vec![[0u16; 4]; cur.len()];
    for j in 1..=passes.min(12) {
        let d = total / f64::powi(2.0, j as i32 + 1);
        let (fwd, back) = (step(d), step(-d));
        // Where a pixel's two samples lie moves by a fixed step along a row.
        let (df, db) = (fwd * Point::new(1.0, 0.0) - fwd * Point::ZERO, back * Point::new(1.0, 0.0) - back * Point::ZERO);
        for (y, row) in next.chunks_exact_mut(w).enumerate().take(h) {
            let (mut f, mut b) = (fwd * Point::new(0.0, y as f64), back * Point::new(0.0, y as f64));
            for out in row.iter_mut() {
                let (s, t) = (bilinear(&cur, w, h, f.x, f.y), bilinear(&cur, w, h, b.x, b.y));
                *out = std::array::from_fn(|i| ((s[i] + t[i]) * 0.5).round().clamp(0.0, 65535.0) as u16);
                (f, b) = (f + df, b + db);
            }
        }
        std::mem::swap(&mut cur, &mut next);
    }
    for (p, q) in px.iter_mut().zip(&cur) {
        *p = q.map(to8);
    }
}

/// The largest radius (pixels) Smart Blur looks over: farther neighbours are as good as gone.
const MAX_SMART: f64 = 4096.0;

/// What Smart Blur draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SmartMode {
    /// The blurred image.
    Normal,
    /// White edges on black: the pixels where the colour steps by more than the threshold.
    EdgeOnly,
    /// The blurred image with those edges drawn over it in white.
    OverlayEdge,
}

impl SmartMode {
    /// (id, label, mode).
    pub const KINDS: [(&'static str, &'static str, SmartMode); 3] = [
        ("normal", "Normal", SmartMode::Normal),
        ("edgeOnly", "Edge Only", SmartMode::EdgeOnly),
        ("overlayEdge", "Overlay Edge", SmartMode::OverlayEdge),
    ];
}

/// Smart Blur in `mode`: Normal blurs ([`smart_normal`]), Edge Only draws the edges white on
/// black, Overlay Edge draws them white over the blurred image (transparency kept).
pub(super) fn smart(px: &mut [[u8; 4]], w: usize, h: usize, radius: f64, threshold: f64, samples: u32, mode: SmartMode) {
    if w == 0 || h == 0 || px.len() < w.saturating_mul(h) {
        return;
    }
    let edges = (mode != SmartMode::Normal).then(|| smart_edges(px, w, h, threshold));
    if mode != SmartMode::EdgeOnly {
        smart_normal(px, w, h, radius, threshold, samples);
    }
    let Some(edges) = edges else { return };
    for (p, &e) in px.iter_mut().zip(&edges) {
        let a = p[3];
        // Premultiplied: white is the alpha in every channel, black is none.
        *p = match (mode, e) {
            (_, true) => [a, a, a, a],
            (SmartMode::EdgeOnly, false) => [0, 0, 0, a],
            _ => *p,
        };
    }
}

/// The edges Smart Blur's edge modes draw: pixels whose colour differs from their right, lower
/// or diagonal neighbour by more than `threshold` levels in some channel (a line one pixel
/// wide on one side of each step).
fn smart_edges(px: &[[u8; 4]], w: usize, h: usize, threshold: f64) -> Vec<bool> {
    let t = threshold.clamp(0.0, 255.0);
    let differs = |a: &[u8; 4], b: &[u8; 4]| a.iter().zip(b).any(|(x, y)| f64::from(x.abs_diff(*y)) > t);
    (0..px.len())
        .map(|i| {
            let (x, y) = (i % w.max(1), i / w.max(1));
            let Some(c) = px.get(i) else { return false };
            [(1, 0), (0, 1), (1, 1), (-1, 1)].iter().any(|&(dx, dy): &(isize, usize)| {
                let (Some(nx), ny) = (x.checked_add_signed(dx), y + dy) else { return false };
                nx < w && ny < h && px.get(ny * w + nx).is_some_and(|n| differs(c, n))
            })
        })
        .collect()
}

/// Smart Blur's Normal mode: each pixel averaged with itself and the neighbours within `radius`
/// pixels whose premultiplied channels all lie within `threshold` levels of its own. Wide radii
/// look at `samples` × `samples` neighbours spread over the disc, read from a copy box-blurred
/// over the gaps between them so the blur stays smooth (a neighbour counts when both it and its
/// blurred surroundings are within the threshold, so at most a fringe under the threshold crosses
/// an edge).
fn smart_normal(px: &mut [[u8; 4]], w: usize, h: usize, radius: f64, threshold: f64, samples: u32) {
    if radius.is_nan() || radius < 0.5 {
        return;
    }
    let r = radius.min(MAX_SMART);
    let n = samples.clamp(3, 15) as f64;
    let k = r.round();
    let (offsets, prefilter): (Vec<Vec2>, usize) = if 2.0 * k < n {
        let k = k as i32;
        ((-k..=k).flat_map(|dy| (-k..=k).map(move |dx| Vec2::new(dx as f64, dy as f64))).collect(), 0)
    } else {
        let gap = 2.0 * r / (n - 1.0);
        let grid = (0..n as i32).map(|i| -r + gap * i as f64);
        (grid.clone().flat_map(|dy| grid.clone().map(move |dx| Vec2::new(dx.round(), dy.round()))).collect(), (gap / 2.0).floor() as usize)
    };
    let offsets: Vec<(isize, isize)> =
        offsets.into_iter().filter(|o| o.hypot() <= r + 0.5 && *o != Vec2::ZERO).map(|o| (o.x as isize, o.y as isize)).collect();
    let src = to16(px);
    let blurred;
    let near: &[Px16] = if prefilter > 0 {
        let mut b = src.clone();
        box_blur(&mut b, w, prefilter);
        blurred = b;
        &blurred
    } else {
        &src
    };
    let t = (threshold.clamp(0.0, 255.0) * 257.0) as i32;
    for (y, (row, out)) in src.chunks_exact(w).zip(px.chunks_exact_mut(w)).enumerate().take(h) {
        for (x, (c, o)) in row.iter().zip(out.iter_mut()).enumerate() {
            let mut acc = c.map(u64::from);
            let mut count = 1u64;
            for (dx, dy) in &offsets {
                let (sx, sy) = (x as isize + dx, y as isize + dy);
                if sx < 0 || sy < 0 || sx as usize >= w || sy as usize >= h {
                    continue;
                }
                let i = sy as usize * w + sx as usize;
                let (Some(s), Some(own)) = (near.get(i), src.get(i)) else { continue };
                // The neighbour itself and its surroundings both close enough: edges stay sharp.
                let close = |p: &Px16| p.iter().zip(c).all(|(a, b)| (*a as i32 - *b as i32).abs() <= t);
                if close(own) && close(s) {
                    for (a, v) in acc.iter_mut().zip(s) {
                        *a += *v as u64;
                    }
                    count += 1;
                }
            }
            *o = acc.map(|a| to8(((a + count / 2) / count).min(65535) as u16));
        }
    }
}
