//! Exact cell averages with a bounded hash map. Dense images usually fit the first map. Sparse
//! or adversarial affine grids fall back to output tiles, retaining only keys actually needed
//! by that tile; source halos affect scan cost, never the map's memory bound.

use std::{collections::HashMap, hash::Hash};

use vectorcraft_geom::{Affine, Point};

const MAX_CELLS: usize = 262_144;
const TILE: usize = 256;

#[derive(Default)]
pub(super) struct Cell {
    pub sum: [u64; 4],
    pub radius: f64,
}

fn add(cell: &mut Cell, pixel: &[u8; 4]) {
    for (total, channel) in cell.sum.iter_mut().zip(pixel) {
        *total += u64::from(*channel);
    }
}

pub(super) fn render<K: Ord + Hash>(
    pixels: &mut [[u8; 4]],
    width: usize,
    transform: Affine,
    diameter: f64,
    key: impl Fn(Point, &[u8; 4]) -> K,
    prepare: impl Fn(&mut Cell),
    paint: impl Fn(Point, &mut [u8; 4], Option<&Cell>),
) {
    render_with_limit(pixels, width, transform, diameter, key, prepare, paint, MAX_CELLS);
}

#[allow(clippy::too_many_arguments)]
fn render_with_limit<K: Ord + Hash>(
    pixels: &mut [[u8; 4]],
    width: usize,
    transform: Affine,
    diameter: f64,
    key: impl Fn(Point, &[u8; 4]) -> K,
    prepare: impl Fn(&mut Cell),
    paint: impl Fn(Point, &mut [u8; 4], Option<&Cell>),
    limit: usize,
) {
    let point = |x: usize, y: usize| transform * Point::new(x as f64 + 0.5, y as f64 + 0.5);
    let mut cells: HashMap<K, Cell> = HashMap::new();
    let mut complete = true;
    'collect: for (y, row) in pixels.chunks_exact(width).enumerate() {
        for (x, pixel) in row.iter().enumerate().filter(|(_, p)| p[3] != 0) {
            let k = key(point(x, y), pixel);
            if cells.len() == limit && !cells.contains_key(&k) {
                complete = false;
                break 'collect;
            }
            add(cells.entry(k).or_default(), pixel);
        }
    }
    if complete {
        for cell in cells.values_mut() {
            prepare(cell);
        }
        for (y, row) in pixels.chunks_exact_mut(width).enumerate() {
            for (x, pixel) in row.iter_mut().enumerate() {
                let q = point(x, y);
                let cell = cells.get(&key(q, pixel));
                paint(q, pixel, cell);
            }
        }
        return;
    }
    // Release the initial map before allocating the source snapshot. Every tile map has at most
    // TILE² entries, even when its source reach covers the entire image (singular/skewed grids).
    drop(cells);
    let height = pixels.len() / width;
    let inverse = transform.inverse();
    let [a, b, c, d, _, _] = inverse.as_coeffs();
    let padding = |extent: f64, cap: usize| {
        let padding = (diameter + 2e-7) * extent;
        if padding.is_finite() { (padding.ceil() + 2.0).clamp(0.0, cap as f64) as usize } else { cap }
    };
    let (pad_x, pad_y) = (padding(a.hypot(c), width), padding(b.hypot(d), height));
    let tiles = width.div_ceil(TILE).saturating_mul(height.div_ceil(TILE));
    let scans = (TILE + 2 * pad_x).min(width).saturating_mul((TILE + 2 * pad_y).min(height)).saturating_mul(tiles);
    let safe_grid = [point(0, 0), point(width - 1, 0), point(0, height - 1), point(width - 1, height - 1)]
        .iter()
        .all(|q| q.x.is_finite() && q.y.is_finite() && q.x.abs() <= 1e9 && q.y.abs() <= 1e9);
    if !safe_grid || inverse.as_coeffs().iter().any(|v| !v.is_finite()) || scans > pixels.len().saturating_mul(16) {
        // Near-singular transforms can make a tile's halo span the whole image. Sorting the
        // existing sample keys avoids tile-count × image-size work without any quality change.
        render_sorted(pixels, width, transform, key, prepare, paint);
        return;
    }
    let source = pixels.to_vec();
    for y0 in (0..height).step_by(TILE) {
        for x0 in (0..width).step_by(TILE) {
            let (x1, y1) = ((x0 + TILE).min(width), (y0 + TILE).min(height));
            let mut cells: HashMap<K, Cell> = HashMap::new();
            for (y, row) in source.chunks_exact(width).enumerate().take(y1).skip(y0) {
                for (x, pixel) in row.iter().enumerate().take(x1).skip(x0).filter(|(_, p)| p[3] != 0) {
                    cells.entry(key(point(x, y), pixel)).or_default();
                }
            }
            let (read_x0, read_y0) = (x0.saturating_sub(pad_x), y0.saturating_sub(pad_y));
            let (read_x1, read_y1) = (x1.saturating_add(pad_x).min(width), y1.saturating_add(pad_y).min(height));
            for (y, row) in source.chunks_exact(width).enumerate().take(read_y1).skip(read_y0) {
                for (x, pixel) in row.iter().enumerate().take(read_x1).skip(read_x0).filter(|(_, p)| p[3] != 0) {
                    if let Some(cell) = cells.get_mut(&key(point(x, y), pixel)) {
                        add(cell, pixel);
                    }
                }
            }
            for cell in cells.values_mut() {
                prepare(cell);
            }
            for (y, row) in pixels.chunks_exact_mut(width).enumerate().take(y1).skip(y0) {
                for (x, pixel) in row.iter_mut().enumerate().take(x1).skip(x0) {
                    let q = point(x, y);
                    paint(q, pixel, cells.get(&key(q, pixel)));
                }
            }
        }
    }
}

fn render_sorted<K: Ord>(
    pixels: &mut [[u8; 4]],
    width: usize,
    transform: Affine,
    key: impl Fn(Point, &[u8; 4]) -> K,
    prepare: impl Fn(&mut Cell),
    paint: impl Fn(Point, &mut [u8; 4], Option<&Cell>),
) {
    let point = |index: usize| transform * Point::new((index % width) as f64 + 0.5, (index / width) as f64 + 0.5);
    // Allocate exactly N slots: filter/collect growth could otherwise reserve close to 2N.
    // Current keys use at most 32 bytes including the index, with no source snapshot or map.
    let mut sorted = Vec::with_capacity(pixels.len());
    for (index, pixel) in pixels.iter().enumerate().filter(|(_, p)| p[3] != 0) {
        sorted.push((key(point(index), pixel), index));
    }
    sorted.sort_unstable_by(|a, b| a.0.cmp(&b.0));
    let mut start = 0;
    while let Some(first) = sorted.get(start) {
        let mut end = start + 1;
        while sorted.get(end).is_some_and(|next| next.0 == first.0) {
            end += 1;
        }
        let Some(group) = sorted.get(start..end) else { break };
        let mut cell = Cell::default();
        for (_, index) in group {
            if let Some(pixel) = pixels.get(*index) {
                add(&mut cell, pixel);
            }
        }
        prepare(&mut cell);
        for (_, index) in group {
            if let Some(pixel) = pixels.get_mut(*index) {
                paint(point(*index), pixel, Some(&cell));
            }
        }
        start = end;
    }
    // Valid premultiplied transparent samples are already zero. Run the callback consistently
    // for callers which explicitly clear them, without including them in color averages.
    for (index, pixel) in pixels.iter_mut().enumerate().filter(|(_, p)| p[3] == 0) {
        paint(point(index), pixel, None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capped_tiles_match_source_only_averages_for_all_affine_shapes() {
        let width = 281;
        let source: Vec<[u8; 4]> = (0..width * 267)
            .map(|i| {
                let alpha = [0, 73, 173, 255][i % 4];
                let rgb = [i * 19 % 256, i * 43 % 256, i * 67 % 256].map(|v| (v * alpha / 255) as u8);
                [rgb[0], rgb[1], rgb[2], alpha as u8]
            })
            .collect();
        for transform in [
            Affine::scale(0.2),
            Affine::rotate(std::f64::consts::FRAC_PI_2) * Affine::scale(0.3),
            Affine::rotate(0.43) * Affine::scale(0.2),
            Affine::new([-0.3, 0.11, 0.17, 0.2, 10.5, -9.2]),
            Affine::new([0.001, 0.0, 0.0, 1000.0, 0.1, -0.2]),
            Affine::new([0.0, 0.0, 0.0, 1.0, 0.0, 0.0]),
        ] {
            let key = |q: Point, p: &[u8; 4]| ((q.x.floor() as i64, q.y.floor() as i64), p[0] / 64);
            let mut sums: HashMap<_, [u64; 4]> = HashMap::new();
            for (i, p) in source.iter().enumerate().filter(|(_, p)| p[3] != 0) {
                let q = transform * Point::new((i % width) as f64 + 0.5, (i / width) as f64 + 0.5);
                let sum = sums.entry(key(q, p)).or_default();
                for channel in 0..4 {
                    sum[channel] += u64::from(p[channel]);
                }
            }
            let paint = |_: Point, p: &mut [u8; 4], cell: Option<&Cell>| {
                if let Some(cell) = cell {
                    let alpha = p[3];
                    for (channel, out) in p.iter_mut().take(3).enumerate() {
                        *out = (cell.sum[channel] as f64 / cell.sum[3] as f64 * f64::from(alpha)).round().clamp(0.0, f64::from(alpha)) as u8;
                    }
                } else {
                    *p = [0; 4];
                }
            };
            let mut expected = source.clone();
            for (i, p) in expected.iter_mut().enumerate() {
                let q = transform * Point::new((i % width) as f64 + 0.5, (i / width) as f64 + 0.5);
                let cell = sums.get(&key(q, p)).map(|sum| Cell { sum: *sum, radius: 0.0 });
                paint(q, p, cell.as_ref());
            }
            let mut actual = source.clone();
            render_with_limit(&mut actual, width, transform, std::f64::consts::SQRT_2, key, |_| {}, paint, 7);
            let difference = actual.iter().zip(&expected).enumerate().find(|(_, (a, b))| a != b);
            assert!(difference.is_none(), "transform={transform:?}, first difference={difference:?}");
            let mut sorted = source.clone();
            render_sorted(&mut sorted, width, transform, key, |_| {}, paint);
            let difference = sorted.iter().zip(&expected).enumerate().find(|(_, (a, b))| a != b);
            assert!(difference.is_none(), "sorted transform={transform:?}, first difference={difference:?}");
        }
    }
}
