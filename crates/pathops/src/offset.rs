//! Offset Path and Outline Stroke.

use drawcraft_geom::{FillRule, PathData, SubPath};
use kurbo::{BezPath, PathEl, Stroke, StrokeOpts};

pub use kurbo::{Cap, Join};

use crate::boolean::{Arrangement, DEFAULT_PRECISION, Tidy, all_contours_to_path, fill_bezpath, normalize_bez};

/// Flattening/fit tolerance handed to kurbo's stroker.
const STROKE_TOL: f64 = 1e-3;

fn stroke_bez(bp: &BezPath, width: f64, cap: Cap, join: Join, miter_limit: f64) -> BezPath {
    let style = Stroke::new(width).with_join(join).with_caps(cap).with_miter_limit(miter_limit.max(1.0));
    close_all(&kurbo::stroke(bp.iter(), &style, &StrokeOpts::default(), STROKE_TOL))
}

/// kurbo's stroker may leave the outline of an open path without a final `ClosePath`.
fn close_all(bp: &BezPath) -> BezPath {
    let mut out = BezPath::new();
    let mut open = false;
    for el in bp.iter() {
        match el {
            PathEl::MoveTo(_) => {
                if open {
                    out.close_path();
                }
                open = true;
            }
            PathEl::ClosePath => open = false,
            _ => {}
        }
        out.push(el);
    }
    if open {
        out.close_path();
    }
    out
}

/// Object → Path → Outline Stroke: the filled area painted by stroking `path` with `width`.
/// Overlaps produced by the stroker are removed, so the result is a clean compound path.
pub fn outline_stroke(path: &PathData, width: f64, cap: Cap, join: Join, miter_limit: f64) -> PathData {
    if width <= 0.0 || !width.is_finite() || path.is_empty() {
        return PathData::default();
    }
    let s = stroke_bez(&path.to_bezpath(), width, cap, join, miter_limit);
    if s.elements().is_empty() {
        return PathData::default();
    }
    normalize_bez(&s, FillRule::NonZero).map(|c| all_contours_to_path(&c, &Tidy::free(DEFAULT_PRECISION))).unwrap_or_default()
}

/// Object → Path → Offset Path. Positive `delta` grows the filled area, negative insets it.
/// Closed subpaths are treated as a non-zero filled region; open subpaths are outlined with a
/// stroke of width `2|delta|` (butt caps).
pub fn offset_path(path: &PathData, delta: f64, join: Join, miter_limit: f64) -> PathData {
    if !delta.is_finite() || path.is_empty() {
        return PathData::default();
    }
    let closed = PathData::new(path.subpaths.iter().filter(|s| s.closed && s.anchors.len() > 1).cloned().collect());
    let open: Vec<SubPath> = path.subpaths.iter().filter(|s| !s.closed && s.anchors.len() > 1).cloned().collect();
    let d = delta.abs();
    let fill = fill_bezpath(&closed);
    let ring = if d > 0.0 { stroke_bez(&closed.to_bezpath(), 2.0 * d, Cap::Butt, join, miter_limit) } else { BezPath::new() };
    let open_bp = if d > 0.0 && !open.is_empty() {
        stroke_bez(&PathData::new(open).to_bezpath(), 2.0 * d, Cap::Butt, join, miter_limit)
    } else {
        BezPath::new()
    };
    let Ok(arr) = Arrangement::new(&[(fill, FillRule::NonZero), (ring, FillRule::NonZero), (open_bp, FillRule::NonZero)]) else {
        return PathData::default();
    };
    let c = if delta >= 0.0 { arr.contours(|m| m[0] || m[1] || m[2]) } else { arr.contours(|m| m[0] && !m[1]) };
    all_contours_to_path(&c, &Tidy::free(DEFAULT_PRECISION))
}
