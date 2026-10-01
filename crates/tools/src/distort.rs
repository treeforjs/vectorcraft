//! Distortion tools: Width (Shift+W), the Liquify family (Warp Shift+R, Twirl, Pucker, Bloat,
//! Scallop, Crystallize, Wrinkle), Puppet Warp, Perspective Grid (Shift+P) and Perspective
//! Selection (Shift+V).
//!
//! Like every tool they only emit actions. The geometry kernels the engine commands run live here
//! too (so tools, engine and tests share one implementation):
//! - [`liquify`]: deterministic brush deformation of anchors and handles with adaptive subdivision;
//! - [`arap`]: As-Rigid-As-Possible mesh deformation (Igarashi, Moscovich & Hughes 2005) with a
//!   small banded Cholesky solver;
//! - [`perspective`]: the perspective grid model (one/two/three-point) and plane homographies;
//! - [`pathutil`]: arc-length helpers for width points.

pub mod arap;
pub mod liquify;
pub mod pathutil;
pub mod perspective;
mod puppet;
mod width;

use std::sync::Arc;

use vectorcraft_color::Paint;
use vectorcraft_doc::{Appearance, AppearanceItem, Node, NodeKind};
use vectorcraft_geom::{Affine, Point, Vec2};

use crate::Tool;

pub use liquify::LiquifyTool;
pub use perspective::{PerspectiveGridTool, PerspectiveSelectionTool};
pub use puppet::{PuppetWarpTool, mesh_for, mesh_input};
pub use width::WidthTool;

/// Create a distortion tool by id (None = not one of ours).
pub fn create(id: &str) -> Option<Box<dyn Tool>> {
    Some(match id {
        "width" => Box::new(WidthTool::default()),
        "warp" | "twirl" | "pucker" | "bloat" | "scallop" | "crystallize" | "wrinkle" => Box::new(LiquifyTool::new(id)),
        "puppetWarp" => Box::new(PuppetWarpTool::default()),
        "perspectiveGrid" => Box::new(PerspectiveGridTool::default()),
        "perspectiveSelection" => Box::new(PerspectiveSelectionTool::default()),
        _ => return None,
    })
}

/// Apply a point map to every anchor and handle of `n` and its descendants. Gradient vectors are
/// mapped too; objects without editable points (text, images, symbols, meshes, live objects) get
/// the map's affine approximation at their centre. Live shapes become plain paths.
pub fn warp_node_with(n: &mut Node, f: &dyn Fn(Point) -> Point) {
    warp_paints(&mut n.appearance, f);
    match &mut n.kind {
        NodeKind::Path { path, live, .. } => {
            *live = None;
            for sp in &mut path.subpaths {
                for a in &mut sp.anchors {
                    a.p = f(a.p);
                    a.h_in = f(a.h_in);
                    a.h_out = f(a.h_out);
                }
            }
        }
        NodeKind::Layer { children, .. }
        | NodeKind::Group { children, .. }
        | NodeKind::Compound { children, .. }
        | NodeKind::Blend { children, .. } => {
            for c in children.iter_mut() {
                warp_node_with(Arc::make_mut(c), f);
            }
        }
        _ => {
            if let Some(b) = n.geometric_bounds() {
                let a = affine_near(f, b.center(), (b.width().max(b.height()) * 0.05).max(0.5));
                n.transform(a, false);
            }
        }
    }
}

fn warp_paints(ap: &mut Appearance, f: &dyn Fn(Point) -> Point) {
    for it in &mut ap.items {
        let p = match it {
            AppearanceItem::Fill(fl) => &mut fl.paint,
            AppearanceItem::Stroke(s) => &mut s.paint,
        };
        if let Paint::Gradient(g) = p
            && let Some(geom) = &mut g.geom
        {
            geom.start = f(geom.start);
            geom.end = f(geom.end);
        }
    }
}

/// The affine map that best matches `f` near `p` (finite differences with step `eps`).
pub fn affine_near(f: &dyn Fn(Point) -> Point, p: Point, eps: f64) -> Affine {
    let o = f(p);
    let ex = (f(p + Vec2::new(eps, 0.0)) - f(p - Vec2::new(eps, 0.0))) / (2.0 * eps);
    let ey = (f(p + Vec2::new(0.0, eps)) - f(p - Vec2::new(0.0, eps))) / (2.0 * eps);
    let lin = Affine::new([ex.x, ex.y, ey.x, ey.y, 0.0, 0.0]);
    Affine::translate(o.to_vec2()) * lin * Affine::translate(-p.to_vec2())
}

/// Every anchor and handle position in `n` (for meshes and bounds).
pub fn collect_points(n: &Node, out: &mut Vec<Point>) {
    n.walk(&mut |c| match &c.kind {
        NodeKind::Path { path, .. } => {
            for sp in &path.subpaths {
                for a in &sp.anchors {
                    out.extend([a.p, a.h_in, a.h_out]);
                }
            }
        }
        NodeKind::Layer { .. } | NodeKind::Group { .. } | NodeKind::Compound { .. } | NodeKind::Blend { .. } => {}
        _ => {
            if let Some(b) = c.geometric_bounds() {
                out.extend([Point::new(b.x0, b.y0), Point::new(b.x1, b.y1)]);
            }
        }
    });
}

/// Selection blue used by the distortion tool overlays.
pub(crate) const BLUE: [u8; 3] = [0x4a, 0x7c, 0xff];

/// A small diamond polygon at `p` (half-diagonal `r`).
pub(crate) fn diamond(p: Point, r: f64) -> vectorcraft_geom::BezPath {
    crate::xform::polygon(&[Point::new(p.x, p.y - r), Point::new(p.x + r, p.y), Point::new(p.x, p.y + r), Point::new(p.x - r, p.y)], true)
}

/// A circle / ellipse outline for brush feedback.
pub(crate) fn ellipse_path(c: Point, rx: f64, ry: f64, angle_deg: f64) -> vectorcraft_geom::BezPath {
    use vectorcraft_geom::Shape;
    let e = vectorcraft_geom::kurbo::Ellipse::new(c, (rx.max(0.01), ry.max(0.01)), angle_deg.to_radians());
    e.to_path(0.05)
}

#[cfg(test)]
mod tests {
    use super::*;
    use vectorcraft_geom::{PathData, Rect, shapes};

    #[test]
    fn create_covers_all_distort_tools() {
        for id in [
            "width",
            "warp",
            "twirl",
            "pucker",
            "bloat",
            "scallop",
            "crystallize",
            "wrinkle",
            "puppetWarp",
            "perspectiveGrid",
            "perspectiveSelection",
        ] {
            assert_eq!(create(id).map(|t| t.id()), Some(id));
        }
        assert!(create("pen").is_none());
    }

    #[test]
    fn warp_node_maps_all_points_and_drops_live_shape() {
        let mut n = Node::path(vectorcraft_doc::NodeId(1), shapes::rectangle(Rect::new(0.0, 0.0, 10.0, 10.0)), Appearance::default_art());
        warp_node_with(&mut n, &|p| Point::new(p.x * 2.0, p.y + 1.0));
        let b = n.geometric_bounds().unwrap();
        assert_eq!((b.x0, b.y0, b.x1, b.y1), (0.0, 1.0, 20.0, 11.0));
        if let NodeKind::Path { live, .. } = &n.kind {
            assert!(live.is_none());
        }
        let _ = PathData::default();
    }

    #[test]
    fn affine_near_recovers_affine_maps() {
        let a = Affine::new([2.0, 0.5, -0.3, 1.5, 10.0, -4.0]);
        let got = affine_near(&|p| a * p, Point::new(3.0, 7.0), 0.5);
        for (x, y) in a.as_coeffs().iter().zip(got.as_coeffs()) {
            assert!((x - y).abs() < 1e-9);
        }
    }
}
