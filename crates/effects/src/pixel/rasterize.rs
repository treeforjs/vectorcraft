//! Effect › Rasterize: the object shown as an image of a given resolution, colour model and
//! background, while it stays editable art.

use std::collections::HashMap;

use vectorcraft_doc::{RasterColorModel, RasterEffectsSettings};

use super::pixelate::centre;
use super::{PixelSpace, to8};

/// Effect › Rasterize's options, read and clamped to their ranges.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rasterize {
    /// The image's resolution in pixels per inch; `None` keeps the document's.
    pub ppi: Option<f64>,
    pub color_model: RasterColorModel,
    /// A white background over the object's bounds (and the room around them).
    pub white: bool,
    /// Off: the art's edges become hard.
    pub anti_alias: bool,
    /// With a white background, the white only under the art.
    pub clipping_mask: bool,
    /// Room around the object, in points.
    pub add_around: f64,
}

/// The most image cells a pixel's colour is averaged into: beyond it the cells are smaller than
/// the raster's pixels, so there's nothing to average.
const MAX_CELLS: usize = 1 << 24;

impl Rasterize {
    /// The side of one image pixel in points (`None` at the document's resolution).
    pub(super) fn cell(&self) -> Option<f64> {
        self.ppi.filter(|p| p.is_finite() && *p > 0.0).map(|p| 72.0 / p)
    }

    /// How far beyond the object it paints: the room around it, plus a cell where the image's
    /// edge pixels straddle the art's.
    pub(super) fn outset(&self) -> f64 {
        let room = if self.white { self.add_around } else { 0.0 };
        room + self.cell().unwrap_or(0.0)
    }

    pub(super) fn apply(&self, px: &mut [[u8; 4]], w: usize, h: usize, space: &PixelSpace) {
        if w == 0 || h == 0 || !(space.px.is_finite() && space.px > 0.0) {
            return;
        }
        if let Some(cell) = self.cell().filter(|c| *c > space.px * 1.01) {
            resample(px, w, h, space, cell);
        }
        if !self.anti_alias {
            for p in px.iter_mut() {
                *p = if p[3] >= 128 { unpremultiplied(*p) } else { [0; 4] };
            }
        }
        let look = RasterEffectsSettings { color_model: self.color_model, ..Default::default() };
        look.finish_pixels(px.as_flattened_mut());
        if self.white {
            let room = space.bounds.inflate(self.add_around, self.add_around);
            for y in 0..h {
                for x in 0..w {
                    if !room.contains(centre(space.to_doc, x, y)) {
                        continue;
                    }
                    if let Some(p) = px.get_mut(y * w + x)
                        && !(self.clipping_mask && p[3] == 0)
                    {
                        let under = 255 - p[3];
                        *p = [p[0].saturating_add(under), p[1].saturating_add(under), p[2].saturating_add(under), 255];
                    }
                }
            }
        }
    }
}

/// A premultiplied pixel made opaque with its straight colour.
fn unpremultiplied(p: [u8; 4]) -> [u8; 4] {
    let a = u32::from(p[3]).max(1);
    let f = |v: u8| ((u32::from(v) * 255 + a / 2) / a).min(255) as u8;
    [f(p[0]), f(p[1]), f(p[2]), 255]
}

/// Every pixel takes the average colour of its cell of a grid `cell` points square, laid from
/// the object's bounds.
fn resample(px: &mut [[u8; 4]], w: usize, h: usize, space: &PixelSpace, cell: f64) {
    let origin = space.bounds.origin();
    let key = |x: usize, y: usize| {
        let p = centre(space.to_doc, x, y);
        (((p.x - origin.x) / cell).floor() as i64, ((p.y - origin.y) / cell).floor() as i64)
    };
    let mut sums: HashMap<(i64, i64), [u64; 5]> = HashMap::new();
    for y in 0..h {
        for x in 0..w {
            let Some(p) = px.get(y * w + x) else { continue };
            if sums.len() >= MAX_CELLS && !sums.contains_key(&key(x, y)) {
                return;
            }
            let s = sums.entry(key(x, y)).or_insert([0; 5]);
            for (acc, v) in s.iter_mut().zip(p) {
                *acc += u64::from(*v) * 257;
            }
            s[4] += 1;
        }
    }
    for y in 0..h {
        for x in 0..w {
            let (Some(s), Some(p)) = (sums.get(&key(x, y)), px.get_mut(y * w + x)) else { continue };
            let n = s[4].max(1);
            *p = [0, 1, 2, 3].map(|i| to8(((s[i] + n / 2) / n).min(65535) as u16));
        }
    }
}
