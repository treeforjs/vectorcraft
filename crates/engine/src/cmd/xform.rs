//! Commands backing transform/utility tools: free distort, eyedropper, gradient vector, artboard move.

use std::sync::Arc;

use serde_json::{Value, json};
use vectorcraft_color::{Gradient, GradientGeom, GradientPaint, Paint};
use vectorcraft_doc::{Appearance, Node, NodeId, NodeKind};
use vectorcraft_geom::{Affine, Point, Rect, Vec2};

use super::edit::selected_roots;
use super::*;
use crate::EngineError;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "object.distort",
            "Free Distort",
            [],
            None,
            "{corners: [[x,y]×4] (TL, TR, BR, BL), from?: [x0,y0,x1,y1] (default: selection bounds), ids?} projective warp of anchors and handles",
            has_doc,
            distort
        ),
        cmd!(
            "appearance.copyFrom",
            "Eyedropper",
            [],
            None,
            "{source: id, ids?} copy fill, stroke, weight, opacity and blend from `source` to ids (default: selection) and to the paint defaults",
            has_doc,
            copy_from
        ),
        cmd!(
            "paint.sampleColor",
            "Sample Color",
            [],
            None,
            "{color, stroke?: bool (default: whichever proxy is active), ids?} sample a colour into the active fill/stroke (and the selection)",
            has_doc,
            sample_color
        ),
        cmd!(
            "paint.setGradientGeom",
            "Gradient Vector",
            [],
            None,
            "{start: [x,y], end: [x,y], ids?, stroke?: bool} set the gradient vector (solid paints become the default gradient)",
            has_doc,
            set_gradient_geom
        ),
        cmd!(
            "artboard.move",
            "Move Artboard",
            [],
            None,
            "{index, dx, dy, moveArt?: bool} move an artboard (and the unlocked art fully inside it)",
            has_doc,
            artboard_move
        ),
    ]
}

// ---------- projective distort ----------

/// A 2D projective transform (homography) mapping the unit square to a quad.
#[derive(Clone, Copy, Debug)]
pub struct Projective {
    m: [f64; 8],
    src: Rect,
}

impl Projective {
    /// Map `src`'s corners (TL, TR, BR, BL) onto `q`.
    pub fn from_rect(src: Rect, q: [Point; 4]) -> Self {
        let [p0, p1, p2, p3] = q;
        let (dx1, dx2, dx3) = (p1.x - p2.x, p3.x - p2.x, p0.x - p1.x + p2.x - p3.x);
        let (dy1, dy2, dy3) = (p1.y - p2.y, p3.y - p2.y, p0.y - p1.y + p2.y - p3.y);
        let (g, h) = if dx3.abs() < 1e-12 && dy3.abs() < 1e-12 {
            (0.0, 0.0)
        } else {
            let den = dx1 * dy2 - dx2 * dy1;
            if den.abs() < 1e-12 { (0.0, 0.0) } else { ((dx3 * dy2 - dx2 * dy3) / den, (dx1 * dy3 - dx3 * dy1) / den) }
        };
        let a = p1.x - p0.x + g * p1.x;
        let b = p3.x - p0.x + h * p3.x;
        let d = p1.y - p0.y + g * p1.y;
        let e = p3.y - p0.y + h * p3.y;
        Self { m: [a, b, p0.x, d, e, p0.y, g, h], src }
    }

    pub fn apply(&self, p: Point) -> Point {
        let [a, b, c, d, e, f, g, h] = self.m;
        let u = if self.src.width().abs() > 1e-12 { (p.x - self.src.x0) / self.src.width() } else { 0.0 };
        let v = if self.src.height().abs() > 1e-12 { (p.y - self.src.y0) / self.src.height() } else { 0.0 };
        let w = g * u + h * v + 1.0;
        let w = if w.abs() < 1e-9 { 1e-9_f64.copysign(w) } else { w };
        Point::new((a * u + b * v + c) / w, (d * u + e * v + f) / w)
    }

    /// Affine approximation at a point (for objects that can't be warped: text, images, symbols).
    fn affine_near(&self, p: Point) -> Affine {
        let eps = 1.0;
        let o = self.apply(p);
        let ex = self.apply(p + Vec2::new(eps, 0.0)) - o;
        let ey = self.apply(p + Vec2::new(0.0, eps)) - o;
        let lin = Affine::new([ex.x / eps, ex.y / eps, ey.x / eps, ey.y / eps, 0.0, 0.0]);
        Affine::translate(o.to_vec2()) * lin * Affine::translate(-p.to_vec2())
    }
}

fn warp_paints(ap: &mut Appearance, pr: &Projective) {
    for it in &mut ap.items {
        let p = match it {
            vectorcraft_doc::AppearanceItem::Fill(f) => &mut f.paint,
            vectorcraft_doc::AppearanceItem::Stroke(s) => &mut s.paint,
        };
        if let Paint::Gradient(g) = p
            && let Some(geom) = &mut g.geom
        {
            geom.start = pr.apply(geom.start);
            geom.end = pr.apply(geom.end);
        }
    }
}

fn warp_node(n: &mut Node, pr: &Projective) {
    warp_paints(&mut n.appearance, pr);
    match &mut n.kind {
        NodeKind::Path { path, live, .. } => {
            *live = None;
            for sp in &mut path.subpaths {
                for a in &mut sp.anchors {
                    a.p = pr.apply(a.p);
                    a.h_in = pr.apply(a.h_in);
                    a.h_out = pr.apply(a.h_out);
                }
            }
        }
        NodeKind::Layer { children, .. } | NodeKind::Group { children, .. } | NodeKind::Compound { children, .. } => {
            for c in children.iter_mut() {
                warp_node(Arc::make_mut(c), pr);
            }
        }
        _ => {
            if let Some(b) = n.geometric_bounds() {
                n.transform(pr.affine_near(b.center()), false);
            }
        }
    }
}

fn distort(s: &mut Session, p: &Value) -> Result<Value> {
    let corners: Vec<Point> = p
        .get("corners")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|c| Some(Point::new(c.get(0)?.as_f64()?, c.get(1)?.as_f64()?))).collect())
        .unwrap_or_default();
    if corners.len() != 4 {
        return Err(bad("object.distort", "corners must be 4 [x,y] points (TL, TR, BR, BL)"));
    }
    let ids = match ids_param(p, "ids") {
        Some(v) => v,
        None => selected_roots(s)?,
    };
    let src = match p.get("from").and_then(Value::as_array) {
        Some(a) if a.len() == 4 => {
            let f: Vec<f64> = a.iter().filter_map(Value::as_f64).collect();
            if f.len() != 4 {
                return Err(bad("object.distort", "from must be [x0,y0,x1,y1]"));
            }
            Rect::new(f[0], f[1], f[2], f[3])
        }
        _ => s.doc()?.doc.bounds_of(&ids, false).ok_or_else(|| EngineError::Other("nothing to distort".into()))?,
    };
    if src.width().abs() < 1e-9 || src.height().abs() < 1e-9 {
        return Err(EngineError::Other("cannot distort a zero-size bounding box".into()));
    }
    let pr = Projective::from_rect(src, [corners[0], corners[1], corners[2], corners[3]]);
    s.edit("Free Distort", |d, _| {
        for id in &ids {
            if let Some(n) = d.node_mut(*id) {
                warp_node(n, &pr);
            }
        }
        Ok(())
    })?;
    Ok(json!({ "ids": ids.iter().map(|i| i.0).collect::<Vec<_>>() }))
}

// ---------- eyedropper ----------

/// Leaves whose appearance a paint command changes (groups expand to their contents; compound
/// paths own their children's appearance).
fn leaf_targets(s: &Session, ids: &[NodeId]) -> Result<Vec<NodeId>> {
    let d = &s.doc()?.doc;
    let mut out = vec![];
    for id in ids {
        let Some(n) = d.node(*id) else { continue };
        match &n.kind {
            NodeKind::Group { .. } | NodeKind::Layer { .. } => n.walk(&mut |c| {
                if !c.is_container() || matches!(c.kind, NodeKind::Compound { .. }) {
                    out.push(c.id)
                }
            }),
            _ => out.push(*id),
        }
    }
    let comp: Vec<NodeId> = out.iter().filter(|id| matches!(d.node(**id).map(|n| &n.kind), Some(NodeKind::Compound { .. }))).copied().collect();
    out.retain(|id| !comp.iter().any(|c| d.parent_of(*id) == Some(*c)));
    Ok(out)
}

fn copy_from(s: &mut Session, p: &Value) -> Result<Value> {
    let src_id = id_param(p, "source").ok_or_else(|| bad("appearance.copyFrom", "missing `source` id"))?;
    let src = s.doc()?.doc.node(src_id).cloned().ok_or(EngineError::NoNode(src_id))?;
    let appearance = match &src.kind {
        NodeKind::Text(t) => match t.runs.first() {
            Some(r) => Appearance::basic(r.style.fill.clone(), r.style.stroke.clone(), r.style.stroke_width),
            None => src.appearance.clone(),
        },
        _ => src.appearance.clone(),
    };
    s.paint.fill = appearance.fill_paint();
    s.paint.stroke = appearance.stroke_paint();
    if appearance.stroke().is_some() {
        s.paint.stroke_width = appearance.stroke_width();
    }
    let ids = match ids_param(p, "ids") {
        Some(v) => v,
        None => selected_roots(s)?,
    };
    let mut targets = leaf_targets(s, &ids)?;
    targets.retain(|id| *id != src_id);
    if targets.is_empty() {
        return Ok(json!({ "ids": [] }));
    }
    let (opacity, blend) = (src.opacity, src.blend);
    s.edit("Eyedropper", |d, _| {
        for id in &targets {
            let Some(n) = d.node_mut(*id) else { continue };
            n.opacity = opacity;
            n.blend = blend;
            if let NodeKind::Text(t) = &mut n.kind {
                for r in &mut t.runs {
                    r.style.fill = appearance.fill_paint();
                    r.style.stroke = appearance.stroke_paint();
                    r.style.stroke_width = appearance.stroke_width();
                }
                continue;
            }
            n.appearance = appearance.clone();
        }
        Ok(())
    })?;
    Ok(json!({ "ids": targets.iter().map(|i| i.0).collect::<Vec<_>>() }))
}

fn sample_color(s: &mut Session, p: &Value) -> Result<Value> {
    let stroke = p.get("stroke").and_then(Value::as_bool).unwrap_or(!s.fill_active);
    let c = p.get("color").ok_or_else(|| bad("paint.sampleColor", "missing color"))?;
    let mut q = json!({ "color": c });
    if let Some(ids) = p.get("ids") {
        q["ids"] = ids.clone();
    }
    let id = if stroke { "paint.setStroke" } else { "paint.setFill" };
    let spec = find_command(id).ok_or_else(|| EngineError::UnknownCommand(id.into()))?;
    (spec.run)(s, &q)
}

// ---------- gradient tool ----------

fn set_gradient_geom(s: &mut Session, p: &Value) -> Result<Value> {
    let start = point_param(p, "start").ok_or_else(|| bad("paint.setGradientGeom", "missing start [x,y]"))?;
    let end = point_param(p, "end").ok_or_else(|| bad("paint.setGradientGeom", "missing end [x,y]"))?;
    let stroke = bool_or(p, "stroke", false);
    let ids = match ids_param(p, "ids") {
        Some(v) => v,
        None => selected_roots(s)?,
    };
    let targets = leaf_targets(s, &ids)?;
    if targets.is_empty() {
        return Err(EngineError::Other("nothing selected".into()));
    }
    s.edit("Gradient", |d, _| {
        for id in &targets {
            let Some(n) = d.node_mut(*id) else { continue };
            if matches!(n.kind, NodeKind::Text(_)) {
                continue;
            }
            let cur = if stroke { n.appearance.stroke_paint() } else { n.appearance.fill_paint() };
            let mut gp = match cur {
                Paint::Gradient(g) => *g,
                _ => GradientPaint::new(Gradient::default()),
            };
            let aspect = gp.geom.map(|g| g.aspect).unwrap_or(1.0);
            gp.geom = Some(GradientGeom { start, end, aspect });
            gp.angle = GradientGeom { start, end, aspect }.angle_deg();
            let paint = Paint::Gradient(Box::new(gp));
            if stroke {
                n.appearance.set_stroke(paint);
            } else {
                n.appearance.set_fill(paint);
            }
        }
        Ok(())
    })?;
    Ok(json!({ "ids": targets.iter().map(|i| i.0).collect::<Vec<_>>() }))
}

// ---------- artboards ----------

fn artboard_move(s: &mut Session, p: &Value) -> Result<Value> {
    let i = p.get("index").and_then(Value::as_u64).ok_or_else(|| bad("artboard.move", "missing index"))? as usize;
    let dv = Vec2::new(f64_or(p, "dx", 0.0), f64_or(p, "dy", 0.0));
    let move_art = bool_or(p, "moveArt", false);
    let st = s.doc()?;
    let rect = st.doc.artboards.get(i).map(|a| a.rect).ok_or_else(|| EngineError::Other("no such artboard".into()))?;
    // Top-level objects (children of layers) lying entirely inside the artboard.
    let mut art = vec![];
    if move_art {
        fn collect(n: &Node, rect: Rect, out: &mut Vec<NodeId>) {
            for c in n.children().into_iter().flatten() {
                if c.locked {
                    continue;
                }
                if c.is_layer() {
                    collect(c, rect, out);
                } else if let Some(b) = c.geometric_bounds()
                    && rect.contains(Point::new(b.x0, b.y0))
                    && rect.contains(Point::new(b.x1, b.y1))
                {
                    out.push(c.id);
                }
            }
        }
        for l in &st.doc.layers {
            if !l.locked {
                collect(l, rect, &mut art);
            }
        }
    }
    let scale_strokes = s.prefs.scale_strokes;
    s.edit("Move Artboard", |d, _| {
        let a = d.artboards.get_mut(i).ok_or_else(|| EngineError::Other("no such artboard".into()))?;
        a.rect = a.rect + dv;
        for id in &art {
            if let Some(n) = d.node_mut(*id) {
                n.transform(Affine::translate(dv), scale_strokes);
            }
        }
        Ok(())
    })?;
    Ok(json!({ "index": i, "moved": art.iter().map(|i| i.0).collect::<Vec<_>>() }))
}
