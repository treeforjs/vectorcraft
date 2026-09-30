//! Paint conversion (solid, gradients, patterns), arrowheads and image decoding.

use drawcraft_color::{GradientKind, Paint};
use drawcraft_doc::{Arrowhead, Document, StrokeLayer};
use drawcraft_geom::{Affine, BezPath, Point, Rect, Shape, Vec2};
use vello_cpu::RenderContext;
use vello_cpu::kurbo::{self, ParamCurve, ParamCurveDeriv};
use vello_cpu::peniko::{self, ColorStop};

fn color(c: &drawcraft_color::Color, alpha: f32) -> peniko::Color {
    let [r, g, b, a] = c.to_rgba8(alpha);
    peniko::Color::from_rgba8(r, g, b, a)
}

/// Set the context paint. Returns false if nothing should be drawn.
pub fn set_paint(ctx: &mut RenderContext, p: &Paint, bounds: Rect, _doc: &Document) -> bool {
    match p {
        Paint::None => false,
        Paint::Solid { color: c, .. } => {
            ctx.set_paint(color(c, 1.0));
            true
        }
        Paint::Gradient(g) => {
            let geom = g.resolve(bounds);
            let stops: Vec<ColorStop> = g.gradient.expanded_stops().iter().map(|(o, c, a)| ColorStop::from((*o, color(c, *a)))).collect();
            if stops.is_empty() {
                return false;
            }
            let grad = match g.gradient.kind {
                GradientKind::Radial => {
                    let r = geom.length().max(1e-6) as f32;
                    let angle = (geom.end - geom.start).atan2();
                    // Aspect ratio squashes the circle along the perpendicular of the gradient vector.
                    ctx.set_paint_transform(
                        Affine::translate(geom.start.to_vec2())
                            * Affine::rotate(angle)
                            * Affine::scale_non_uniform(1.0, geom.aspect.max(1e-3))
                            * Affine::rotate(-angle)
                            * Affine::translate(-geom.start.to_vec2()),
                    );
                    peniko::Gradient::new_radial(geom.start, r).with_stops(stops.as_slice())
                }
                _ => {
                    ctx.reset_paint_transform();
                    let (s, e) =
                        if geom.start.distance(geom.end) < 1e-9 { (geom.start, geom.start + Vec2::new(1.0, 0.0)) } else { (geom.start, geom.end) };
                    peniko::Gradient::new_linear(s, e).with_stops(stops.as_slice())
                }
            };
            ctx.set_paint(grad);
            true
        }
        Paint::Pattern { .. } => {
            // Patterns render as a neutral mid-grey until pattern swatches land (M10.5).
            ctx.set_paint(peniko::Color::from_rgba8(128, 128, 128, 255));
            true
        }
    }
}

/// Arrowhead geometry for a stroke: (which, filled outline) in document coordinates.
pub fn arrowheads(bp: &BezPath, st: &StrokeLayer) -> Vec<(Arrowhead, BezPath)> {
    let segs: Vec<kurbo::PathSeg> = bp.segments().collect();
    let mut out = Vec::new();
    let (Some(first), Some(last)) = (segs.first(), segs.last()) else { return out };
    if let Some(a) = st.start_arrow {
        let p = first.eval(0.0);
        let d = tangent(first, 0.0) * -1.0;
        out.push((a, head(a, p, d, st.width * st.arrow_scale.0 / 100.0)));
    }
    if let Some(a) = st.end_arrow {
        let p = last.eval(1.0);
        let d = tangent(last, 1.0);
        out.push((a, head(a, p, d, st.width * st.arrow_scale.1 / 100.0)));
    }
    out
}

fn tangent(s: &kurbo::PathSeg, t: f64) -> Vec2 {
    let d = match s {
        kurbo::PathSeg::Line(l) => l.p1 - l.p0,
        kurbo::PathSeg::Quad(q) => q.deriv().eval(t).to_vec2(),
        kurbo::PathSeg::Cubic(c) => {
            let v = c.deriv().eval(t).to_vec2();
            if v.hypot() < 1e-9 { c.p3 - c.p0 } else { v }
        }
    };
    let l = d.hypot();
    if l < 1e-12 { Vec2::new(1.0, 0.0) } else { d / l }
}

/// Arrowhead outline pointing along `dir` with its tip at `tip`, sized for stroke weight `w`.
fn head(kind: Arrowhead, tip: Point, dir: Vec2, w: f64) -> BezPath {
    let w = w.max(0.25);
    let n = Vec2::new(-dir.y, dir.x);
    let len = 4.0 * w;
    let half = 2.0 * w;
    let base = tip - dir * len;
    let mut p = BezPath::new();
    match kind {
        Arrowhead::Triangle | Arrowhead::TriangleOpen | Arrowhead::Arrow | Arrowhead::ArrowOpen => {
            p.move_to(tip);
            p.line_to(base + n * half);
            if matches!(kind, Arrowhead::Arrow | Arrowhead::ArrowOpen) {
                p.line_to(tip - dir * (len * 0.7));
            }
            p.line_to(base - n * half);
            p.close_path();
        }
        Arrowhead::Circle | Arrowhead::CircleOpen => {
            p = kurbo::Circle::new(tip - dir * half, half).to_path(0.01);
        }
        Arrowhead::Square | Arrowhead::SquareOpen => {
            let c = tip - dir * half;
            p.move_to(c + dir * half + n * half);
            p.line_to(c - dir * half + n * half);
            p.line_to(c - dir * half - n * half);
            p.line_to(c + dir * half - n * half);
            p.close_path();
        }
        Arrowhead::Diamond => {
            let c = tip - dir * half;
            p.move_to(tip);
            p.line_to(c + n * half);
            p.line_to(c - dir * half);
            p.line_to(c - n * half);
            p.close_path();
        }
        Arrowhead::Bar => {
            let t = w * 0.75;
            p.move_to(tip + n * half);
            p.line_to(tip + n * half - dir * t);
            p.line_to(tip - n * half - dir * t);
            p.line_to(tip - n * half);
            p.close_path();
        }
    }
    p
}

/// Decode encoded image bytes into a premultiplied pixmap.
pub fn decode_pixmap(bytes: &[u8]) -> Option<vello_cpu::Pixmap> {
    let img = image::load_from_memory(bytes).ok()?.to_rgba8();
    let (w, h) = img.dimensions();
    if w == 0 || h == 0 || w > u16::MAX as u32 || h > u16::MAX as u32 {
        return None;
    }
    let data: Vec<vello_cpu::color::PremulRgba8> = img
        .pixels()
        .map(|p| {
            let a = p[3] as u16;
            let m = |c: u8| ((c as u16 * a + 127) / 255) as u8;
            vello_cpu::color::PremulRgba8 { r: m(p[0]), g: m(p[1]), b: m(p[2]), a: p[3] }
        })
        .collect();
    Some(vello_cpu::Pixmap::from_parts(data, w as u16, h as u16))
}
