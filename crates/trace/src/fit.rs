//! Loop → polygon → Bézier curves.

use drawcraft_geom::{AnchorKind, PathData, Point, SubPath};
use drawcraft_pathops::{SimplifyOptions, simplify_with};

use crate::contour::Loop;

#[derive(Clone, Copy, Debug)]
pub(crate) struct FitOptions {
    pub polygon_tol: f64,
    pub fit_tol: f64,
    pub corner_angle: f64,
    pub snap_lines: bool,
}

fn seg_dist(p: Point, a: Point, b: Point) -> f64 {
    let ab = b - a;
    let l2 = ab.hypot2();
    if l2 < 1e-18 {
        return p.distance(a);
    }
    let t = ((p - a).dot(ab) / l2).clamp(0.0, 1.0);
    p.distance(a + ab * t)
}

/// Douglas–Peucker on `pts[a..=b]`, marking kept indices.
fn rdp(pts: &[Point], a: usize, b: usize, tol: f64, keep: &mut [bool]) {
    let mut stack = vec![(a, b)];
    while let Some((a, b)) = stack.pop() {
        if b <= a + 1 {
            continue;
        }
        let (pa, pb) = (pts[a], pts[b % pts.len()]);
        let mut best = (0.0, a);
        for i in a + 1..b {
            let d = seg_dist(pts[i], pa, pb);
            if d > best.0 {
                best = (d, i);
            }
        }
        if best.0 > tol {
            keep[best.1] = true;
            stack.push((a, best.1));
            stack.push((best.1, b));
        }
    }
}

/// The polygon approximating a closed loop of pixel-corner vertices within `tol`.
pub(crate) fn polygon(loop_pts: &[(i32, i32)], tol: f64) -> Vec<Point> {
    let pts: Vec<Point> = loop_pts.iter().map(|&(x, y)| Point::new(x as f64, y as f64)).collect();
    let n = pts.len();
    if n < 4 {
        return pts;
    }
    // Split the closed loop at vertex 0 and the vertex farthest from it.
    let far = (1..n).max_by(|&i, &j| pts[i].distance_squared(pts[0]).total_cmp(&pts[j].distance_squared(pts[0]))).unwrap_or(n / 2);
    let mut ext = pts.clone();
    ext.push(pts[0]);
    let mut keep = vec![false; n + 1];
    keep[0] = true;
    keep[far] = true;
    rdp(&ext, 0, far, tol, &mut keep);
    rdp(&ext, far, n, tol, &mut keep);
    let mut out: Vec<Point> = (0..n).filter(|&i| keep[i]).map(|i| pts[i]).collect();
    // Vertex 0 was forced; drop it when it's not needed.
    if out.len() > 3 {
        let (p, a, b) = (out[0], out[out.len() - 1], out[1]);
        if seg_dist(p, a, b) <= tol * 0.5 {
            out.remove(0);
        }
    }
    out
}

/// Fit one loop into a closed Bézier subpath (pixel coordinates).
pub(crate) fn fit_loop(l: &Loop, o: &FitOptions) -> Option<SubPath> {
    let poly = polygon(&l.pts, o.polygon_tol);
    if poly.len() < 3 {
        return None;
    }
    let sp = SubPath::polyline(&poly, true);
    let fitted =
        simplify_with(&PathData::single(sp), &SimplifyOptions { tolerance: o.fit_tol, corner_angle_deg: o.corner_angle, straight_lines: false });
    let mut sp = fitted.subpaths.into_iter().next()?;
    if sp.anchors.len() < 2 {
        return None;
    }
    if o.snap_lines {
        snap_to_lines(&mut sp, o.fit_tol.max(0.75));
    }
    Some(sp)
}

/// Retract the handles of curves whose control points lie within `tol` of their chord.
fn snap_to_lines(sp: &mut SubPath, tol: f64) {
    let n = sp.anchors.len();
    for i in 0..sp.segment_count() {
        let j = (i + 1) % n;
        let (a, b) = (sp.anchors[i], sp.anchors[j]);
        if !a.has_out() && !b.has_in() {
            continue;
        }
        if seg_dist(a.h_out, a.p, b.p) <= tol && seg_dist(b.h_in, a.p, b.p) <= tol {
            sp.anchors[i].h_out = a.p;
            sp.anchors[j].h_in = b.p;
        }
    }
    for a in &mut sp.anchors {
        if !a.has_in() || !a.has_out() {
            a.kind = AnchorKind::Corner;
        }
    }
}
