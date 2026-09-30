//! Parameter parsing, deterministic noise and path-mapping helpers.

use drawcraft_geom::{Anchor, CubicBez, ParamCurve, PathData, Point, Rect, SubPath, Vec2};
use serde_json::Value;

/// Number param (also accepts numeric strings such as `"10 pt"`); non-finite → default.
pub fn num(p: &Value, key: &str, default: f64) -> f64 {
    match p.get(key) {
        Some(Value::Number(n)) => n.as_f64(),
        Some(Value::String(s)) => {
            let t: String = s.trim().chars().take_while(|c| c.is_ascii_digit() || matches!(c, '.' | '-' | '+' | 'e' | 'E')).collect();
            t.parse().ok()
        }
        Some(Value::Bool(b)) => Some(if *b { 1.0 } else { 0.0 }),
        _ => None,
    }
    .filter(|v| v.is_finite())
    .unwrap_or(default)
}

pub fn flag(p: &Value, key: &str, default: bool) -> bool {
    match p.get(key) {
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|v| v != 0.0),
        _ => default,
    }
}

pub fn text<'a>(p: &'a Value, key: &str, default: &'a str) -> &'a str {
    p.get(key).and_then(Value::as_str).unwrap_or(default)
}

pub fn join(p: &Value, key: &str) -> drawcraft_pathops::Join {
    match text(p, key, "miter").to_ascii_lowercase().as_str() {
        "round" => drawcraft_pathops::Join::Round,
        "bevel" => drawcraft_pathops::Join::Bevel,
        _ => drawcraft_pathops::Join::Miter,
    }
}

/// "smooth" (default) vs "corner" points.
pub fn smooth_points(p: &Value) -> bool {
    !text(p, "points", "smooth").eq_ignore_ascii_case("corner")
}

/// Deterministic hash noise in [-1, 1] for (seed, a, b, c).
pub fn noise(seed: u64, a: u64, b: u64, c: u64) -> f64 {
    let mut x = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ a.wrapping_mul(0xBF58_476D_1CE4_E5B9) ^ b.wrapping_mul(0x94D0_49BB_1331_11EB) ^ c;
    // splitmix64 finaliser
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^= x >> 31;
    (x >> 11) as f64 / (1u64 << 53) as f64 * 2.0 - 1.0
}

pub fn seed(p: &Value) -> u64 {
    num(p, "seed", 0.0).abs() as u64
}

/// Mean side of a box (the reference for "relative" sizes).
pub fn mean_size(b: Rect) -> f64 {
    (b.width().abs() + b.height().abs()) / 2.0
}

/// Segment as a cubic; straight lines get handles at 1/3 and 2/3 so a non-linear map bends them.
pub fn seg_cubic(sp: &SubPath, i: usize) -> CubicBez {
    let c = sp.segment(i);
    if sp.segment_is_line(i) { CubicBez::new(c.p0, c.p0.lerp(c.p3, 1.0 / 3.0), c.p0.lerp(c.p3, 2.0 / 3.0), c.p3) } else { c }
}

fn poly_len(c: &CubicBez) -> f64 {
    c.p0.distance(c.p1) + c.p1.distance(c.p2) + c.p2.distance(c.p3)
}

/// Map every point of `path` through the non-linear function `f`. Segments are split into pieces
/// no longer than about `max_piece` (up to 64 per segment) and their control points mapped, which
/// approximates the image curve to O(h²). Closedness and subpath structure are preserved.
pub fn map_nonlinear(path: &PathData, max_piece: f64, f: impl Fn(Point) -> Point) -> PathData {
    let max_piece = max_piece.max(1e-3);
    let mut subs = Vec::with_capacity(path.subpaths.len());
    for sp in &path.subpaths {
        let n = sp.anchors.len();
        if n == 0 {
            continue;
        }
        let segs = sp.segment_count();
        let mut res: Vec<Anchor> = Vec::new();
        let mut pending_in: Option<Point> = None;
        for i in 0..n {
            let a = &sp.anchors[i];
            let h_in = pending_in.take().unwrap_or_else(|| f(a.h_in));
            res.push(Anchor { p: f(a.p), h_in, h_out: f(a.h_out), kind: a.kind });
            if i < segs {
                let c = seg_cubic(sp, i);
                let k = ((poly_len(&c) / max_piece).ceil() as usize).clamp(1, 64);
                for j in 0..k {
                    let sub = c.subsegment((j as f64 / k as f64)..((j + 1) as f64 / k as f64));
                    res.last_mut().unwrap().h_out = f(sub.p1);
                    if j + 1 < k {
                        res.push(Anchor { p: f(sub.p3), h_in: f(sub.p2), h_out: f(sub.p3), kind: Default::default() });
                    } else {
                        pending_in = Some(f(sub.p2));
                    }
                }
            }
        }
        if let Some(h) = pending_in
            && sp.closed
        {
            res[0].h_in = h;
        }
        let anchors = res.into_iter().map(|a| Anchor::with_handles(a.p, a.h_in, a.h_out)).collect();
        subs.push(SubPath::new(anchors, sp.closed));
    }
    PathData::new(subs)
}

/// Smooth subpath through `pts` (Catmull-Rom tangents).
pub fn catmull_rom(pts: &[Point], closed: bool, tension: f64) -> SubPath {
    let n = pts.len();
    if n < 3 {
        return SubPath::polyline(pts, closed);
    }
    let anchors = (0..n)
        .map(|i| {
            let prev = if i == 0 { if closed { pts[n - 1] } else { pts[0] } } else { pts[i - 1] };
            let next = if i == n - 1 { if closed { pts[0] } else { pts[n - 1] } } else { pts[i + 1] };
            let t: Vec2 = (next - prev) * (tension / 6.0);
            Anchor::with_handles(pts[i], pts[i] - t, pts[i] + t)
        })
        .collect();
    SubPath::new(anchors, closed)
}

/// Unit normal (left of travel direction in y-down coordinates) of cubic `c` at `t`.
pub fn normal_at(c: &CubicBez, t: f64) -> Vec2 {
    let eps = 1e-4;
    let a = c.eval((t - eps).max(0.0));
    let b = c.eval((t + eps).min(1.0));
    let mut d = b - a;
    if d.hypot() < 1e-12 {
        d = c.p3 - c.p0;
    }
    let len = d.hypot();
    if len < 1e-12 { Vec2::ZERO } else { Vec2::new(d.y / len, -d.x / len) }
}
