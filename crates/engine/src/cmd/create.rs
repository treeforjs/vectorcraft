//! Creating art: shapes, paths, text.

use drawcraft_doc::{Appearance, CharStyle, LiveShape, Node, NodeKind, TextObject};
use drawcraft_geom::{Affine, Anchor, AnchorKind, FillRule, PathData, Point, Rect, SubPath, shapes};
use serde_json::{Value, json};

use super::*;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!("shape.rectangle", "Rectangle", [], None, "{x, y, width, height, radius?: pt} → {id}", has_doc, rectangle),
        cmd!("shape.ellipse", "Ellipse", [], None, "{x, y, width, height} → {id}", has_doc, ellipse),
        cmd!("shape.polygon", "Polygon", [], None, "{cx, cy, radius, sides=6, rotation?: deg} → {id}", has_doc, polygon),
        cmd!("shape.star", "Star", [], None, "{cx, cy, radius1, radius2, points=5, rotation?: deg} → {id}", has_doc, star),
        cmd!("shape.line", "Line Segment", [], None, "{x1, y1, x2, y2} → {id}", has_doc, line),
        cmd!("shape.spiral", "Spiral", [], None, "{cx, cy, radius, decay=80 (%), segments=10, clockwise?} → {id}", has_doc, spiral),
        cmd!("shape.arc", "Arc", [], None, "{x1, y1, x2, y2, closed?} → {id}", has_doc, arc),
        cmd!("shape.rectangularGrid", "Rectangular Grid", [], None, "{x, y, width, height, rows=5, columns=5} → {id}", has_doc, rect_grid),
        cmd!("shape.polarGrid", "Polar Grid", [], None, "{x, y, width, height, concentric=5, radial=5} → {id}", has_doc, polar_grid),
        cmd!(
            "path.create",
            "Create Path",
            [],
            None,
            "{anchors: [{x, y, in?: [x,y], out?: [x,y]}], closed?: bool, d?: SVG path data} → {id}",
            has_doc,
            path_create
        ),
        cmd!(
            "text.create",
            "Create Text",
            [],
            None,
            "{x, y, text, size?: pt, font?: family, style?, color?, area?: {width, height}} → {id}",
            has_doc,
            text_create
        ),
    ]
}

/// Insert a new object at the top of the insertion parent with the current paint; select it.
pub(crate) fn add_art(s: &mut Session, label: &str, kind: NodeKind, name: Option<String>) -> Result<Value> {
    let appearance = Appearance::basic(s.paint.fill.clone(), s.paint.stroke.clone(), s.paint.stroke_width);
    add_node(s, label, kind, appearance, name)
}

pub(crate) fn add_node(s: &mut Session, label: &str, kind: NodeKind, appearance: Appearance, name: Option<String>) -> Result<Value> {
    let parent = s.doc()?.insertion_parent();
    let id = s.edit(label, |d, sel| {
        let id = d.alloc_id();
        let mut n = Node::new(id, kind);
        n.appearance = appearance;
        n.name = name;
        d.insert(parent, usize::MAX, n)?;
        sel.set([id]);
        Ok(id)
    })?;
    Ok(json!({ "id": id.0 }))
}

fn path_kind(path: PathData, live: Option<LiveShape>) -> NodeKind {
    NodeKind::Path { path, rule: FillRule::NonZero, live, clipping: false, guide: false }
}

fn rect_of(p: &Value, cmd: &str) -> Result<Rect> {
    let x = f64_req(p, "x", cmd)?;
    let y = f64_req(p, "y", cmd)?;
    let w = f64_req(p, "width", cmd)?;
    let h = f64_req(p, "height", cmd)?;
    if !(w.is_finite() && h.is_finite()) {
        return Err(bad(cmd, "invalid size"));
    }
    Ok(Rect::new(x, y, x + w, y + h).abs())
}

fn rectangle(s: &mut Session, p: &Value) -> Result<Value> {
    let r = rect_of(p, "shape.rectangle")?;
    let radius = f64_or(p, "radius", 0.0).max(0.0);
    let live = LiveShape::Rectangle { w: r.width(), h: r.height(), radii: [radius; 4], xf: Affine::translate(r.origin().to_vec2()) };
    let label = if radius > 0.0 { "Rounded Rectangle" } else { "Rectangle" };
    add_art(s, label, path_kind(live.to_path(), Some(live)), None)
}

fn ellipse(s: &mut Session, p: &Value) -> Result<Value> {
    let r = rect_of(p, "shape.ellipse")?;
    let live = LiveShape::Ellipse { w: r.width(), h: r.height(), pie: (0.0, 360.0), xf: Affine::translate(r.origin().to_vec2()) };
    add_art(s, "Ellipse", path_kind(live.to_path(), Some(live)), None)
}

fn polygon(s: &mut Session, p: &Value) -> Result<Value> {
    let c = Point::new(f64_req(p, "cx", "shape.polygon")?, f64_req(p, "cy", "shape.polygon")?);
    let r = f64_req(p, "radius", "shape.polygon")?.abs();
    let sides = p.get("sides").and_then(Value::as_u64).unwrap_or(6).clamp(3, 1000) as u32;
    let rot = f64_or(p, "rotation", 0.0);
    let live = LiveShape::Polygon { radius: r, sides, xf: Affine::translate(c.to_vec2()) * Affine::rotate(rot.to_radians()) };
    add_art(s, "Polygon", path_kind(live.to_path(), Some(live)), None)
}

fn star(s: &mut Session, p: &Value) -> Result<Value> {
    let c = Point::new(f64_req(p, "cx", "shape.star")?, f64_req(p, "cy", "shape.star")?);
    let r1 = f64_req(p, "radius1", "shape.star")?.abs();
    let r2 = f64_or(p, "radius2", r1 / 2.0).abs();
    let n = p.get("points").and_then(Value::as_u64).unwrap_or(5).clamp(2, 1000) as u32;
    let path = shapes::star(c, r1, r2, n, f64_or(p, "rotation", 0.0));
    add_art(s, "Star", path_kind(path, None), None)
}

fn line(s: &mut Session, p: &Value) -> Result<Value> {
    let a = Point::new(f64_req(p, "x1", "shape.line")?, f64_req(p, "y1", "shape.line")?);
    let b = Point::new(f64_req(p, "x2", "shape.line")?, f64_req(p, "y2", "shape.line")?);
    let live = LiveShape::Line { a, b };
    // Lines are stroked, never filled (Illustrator ignores the fill for new open lines).
    let appearance = Appearance::basic(
        drawcraft_color::Paint::None,
        if s.paint.stroke.is_none() { drawcraft_color::Paint::solid(drawcraft_color::Color::BLACK) } else { s.paint.stroke.clone() },
        s.paint.stroke_width.max(0.1),
    );
    add_node(s, "Line", path_kind(live.to_path(), Some(live)), appearance, None)
}

fn spiral(s: &mut Session, p: &Value) -> Result<Value> {
    let c = Point::new(f64_req(p, "cx", "shape.spiral")?, f64_req(p, "cy", "shape.spiral")?);
    let path = shapes::spiral(
        c,
        f64_req(p, "radius", "shape.spiral")?,
        f64_or(p, "decay", 80.0),
        p.get("segments").and_then(Value::as_u64).unwrap_or(10).clamp(2, 1000) as u32,
        bool_or(p, "clockwise", true),
    );
    add_art(s, "Spiral", path_kind(path, None), None)
}

fn arc(s: &mut Session, p: &Value) -> Result<Value> {
    let a = Point::new(f64_req(p, "x1", "shape.arc")?, f64_req(p, "y1", "shape.arc")?);
    let b = Point::new(f64_req(p, "x2", "shape.arc")?, f64_req(p, "y2", "shape.arc")?);
    add_art(s, "Arc", path_kind(shapes::arc(a, b, 0.0, bool_or(p, "closed", false)), None), None)
}

fn grid_group(s: &mut Session, label: &str, paths: Vec<PathData>) -> Result<Value> {
    let parent = s.doc()?.insertion_parent();
    let ap = Appearance::basic(drawcraft_color::Paint::None, s.paint.stroke.clone(), s.paint.stroke_width);
    let id = s.edit(label, |d, sel| {
        let children = paths
            .into_iter()
            .map(|pd| {
                let id = d.alloc_id();
                std::sync::Arc::new(Node::path(id, pd, ap.clone()))
            })
            .collect();
        let gid = d.alloc_id();
        d.insert(parent, usize::MAX, Node::group(gid, children))?;
        sel.set([gid]);
        Ok(gid)
    })?;
    Ok(json!({ "id": id.0 }))
}

fn rect_grid(s: &mut Session, p: &Value) -> Result<Value> {
    let r = rect_of(p, "shape.rectangularGrid")?;
    let rows = p.get("rows").and_then(Value::as_u64).unwrap_or(5).min(999) as u32;
    let cols = p.get("columns").and_then(Value::as_u64).unwrap_or(5).min(999) as u32;
    grid_group(s, "Rectangular Grid", shapes::rectangular_grid(r, rows, cols, true))
}

fn polar_grid(s: &mut Session, p: &Value) -> Result<Value> {
    let r = rect_of(p, "shape.polarGrid")?;
    let c = p.get("concentric").and_then(Value::as_u64).unwrap_or(5).min(999) as u32;
    let rad = p.get("radial").and_then(Value::as_u64).unwrap_or(5).min(999) as u32;
    grid_group(s, "Polar Grid", shapes::polar_grid(r, c, rad))
}

pub(crate) fn anchor_from_json(v: &Value) -> Option<Anchor> {
    let p = Point::new(v.get("x")?.as_f64()?, v.get("y")?.as_f64()?);
    let h_in = point_param(v, "in").unwrap_or(p);
    let h_out = point_param(v, "out").unwrap_or(p);
    let mut a = Anchor::with_handles(p, h_in, h_out);
    if v.get("smooth").and_then(Value::as_bool) == Some(true) {
        a.kind = AnchorKind::Smooth;
    }
    Some(a)
}

fn path_create(s: &mut Session, p: &Value) -> Result<Value> {
    let path = if let Some(d) = str_param(p, "d") {
        let bp = drawcraft_geom::BezPath::from_svg(d).map_err(|e| bad("path.create", format!("bad path data: {e}")))?;
        PathData::from_bezpath(&bp)
    } else {
        let anchors: Vec<Anchor> = p
            .get("anchors")
            .and_then(Value::as_array)
            .ok_or_else(|| bad("path.create", "missing anchors"))?
            .iter()
            .filter_map(anchor_from_json)
            .collect();
        if anchors.is_empty() {
            return Err(bad("path.create", "need at least one anchor"));
        }
        PathData::single(SubPath::new(anchors, bool_or(p, "closed", false)))
    };
    let mut ap = Appearance::basic(s.paint.fill.clone(), s.paint.stroke.clone(), s.paint.stroke_width);
    // Open paths drawn with a stroke of None would be invisible: mimic Illustrator and keep what the user set.
    if !path.is_closed() && ap.stroke_paint().is_none() && ap.fill_paint().is_none() {
        ap.set_stroke(drawcraft_color::Paint::solid(drawcraft_color::Color::BLACK));
    }
    add_node(s, "Pen", path_kind(path, None), ap, None)
}

fn text_create(s: &mut Session, p: &Value) -> Result<Value> {
    let x = f64_req(p, "x", "text.create")?;
    let y = f64_req(p, "y", "text.create")?;
    let text = str_param(p, "text").unwrap_or("");
    let mut style = CharStyle::default();
    if let Some(sz) = p.get("size").and_then(Value::as_f64) {
        style.size = sz.clamp(0.1, 1296.0);
    }
    if let Some(f) = str_param(p, "font") {
        style.font_family = f.to_string();
    }
    if let Some(f) = str_param(p, "style") {
        style.font_style = f.to_string();
    }
    style.fill = match p.get("color").and_then(color_value) {
        Some(c) => drawcraft_color::Paint::solid(c),
        None if !s.paint.fill.is_none() && s.paint.fill != drawcraft_color::Paint::solid(drawcraft_color::Color::WHITE) => s.paint.fill.clone(),
        None => drawcraft_color::Paint::solid(drawcraft_color::Color::BLACK),
    };
    let mut t = TextObject::point(Point::new(x, y), text, style);
    if let Some(a) = p.get("area") {
        let w = f64_or(a, "width", 200.0);
        let h = f64_or(a, "height", 100.0);
        t.kind = drawcraft_doc::TextKind::Area { frame: shapes::rectangle(Rect::new(0.0, 0.0, w, h)) };
    }
    let lay = drawcraft_text::layout(drawcraft_text::FontDb::global(), &t);
    t.cached_bounds = Some(lay.bounds);
    add_node(s, "Type", NodeKind::Text(Box::new(t)), Appearance::default(), None)
}
