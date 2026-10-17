//! Video › De-Interlace and NTSC Colors.
//!
//! De-Interlace's scan lines are one point tall in document space, so they stay on the art
//! whatever the raster's origin, resolution or zoom (at the default 72 ppi a line is a pixel row).

use vectorcraft_geom::Point;

use super::blur::bilinear;
use super::{PixelSpace, to8, to16};

/// The height of a scan line, in points.
const LINE: f64 = 1.0;

/// De-Interlace: the `odd` (else even) scan lines replaced by a copy of the line above them
/// (`interpolate` false) or by the average of the lines either side (`interpolate` true).
pub(super) fn de_interlace(px: &mut [[u8; 4]], w: usize, h: usize, space: &PixelSpace, odd: bool, interpolate: bool) {
    let from_doc = space.to_doc.inverse();
    if !from_doc.is_finite() {
        return;
    }
    let src = to16(px);
    // The source at document point (`x`, the middle of line `line`).
    let sample = |x: f64, line: f64| {
        let q = from_doc * Point::new(x, (line + 0.5) * LINE);
        // Pixel centres sit at whole numbers in `bilinear`'s coordinates.
        bilinear(&src, w, h, q.x - 0.5, q.y - 0.5)
    };
    for y in 0..h {
        for x in 0..w {
            let p = space.to_doc * Point::new(x as f64 + 0.5, y as f64 + 0.5);
            let line = (p.y / LINE).floor();
            if !line.is_finite() || (line.rem_euclid(2.0) == 1.0) != odd {
                continue;
            }
            let s = if interpolate {
                let (a, b) = (sample(p.x, line - 1.0), sample(p.x, line + 1.0));
                std::array::from_fn(|i| 0.5 * (a[i] + b[i]))
            } else {
                sample(p.x, line - 1.0)
            };
            if let Some(out) = px.get_mut(y * w + x) {
                *out = s.map(|v| to8(v.round().clamp(0.0, 65535.0) as u16));
            }
        }
    }
}

/// The brightest a colour's composite video signal may swing (luma plus chroma amplitude, 1 =
/// white) and the darkest, as a share of white.
const PEAK: f64 = 1.0;
const FLOOR: f64 = -0.2;

/// NTSC Colors: colours too saturated for a broadcast signal lose saturation (keeping their hue
/// and luma) until luma ± chroma stays within the legal range.
pub(super) fn ntsc_colors(px: &mut [[u8; 4]]) {
    for p in px.iter_mut() {
        let a = f64::from(p[3]);
        if a <= 0.0 {
            continue;
        }
        let [r, g, b] = [p[0], p[1], p[2]].map(|v| f64::from(v) / a);
        let y = 0.299 * r + 0.587 * g + 0.114 * b;
        let i = 0.595_716 * r - 0.274_453 * g - 0.321_263 * b;
        let q = 0.211_456 * r - 0.522_591 * g + 0.311_135 * b;
        let c = i.hypot(q);
        if c <= 1e-9 {
            continue;
        }
        let room = (PEAK - y).min(y - FLOOR).max(0.0);
        if c <= room {
            continue;
        }
        let s = room / c;
        let (i, q) = (i * s, q * s);
        let rgb = [y + 0.956_3 * i + 0.621_0 * q, y - 0.272_1 * i - 0.647_4 * q, y - 1.107_0 * i + 1.704_6 * q];
        for (out, v) in p.iter_mut().zip(rgb) {
            *out = (v.clamp(0.0, 1.0) * a).round().clamp(0.0, 255.0) as u8;
        }
    }
}
