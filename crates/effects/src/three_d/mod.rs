//! Effect › 3D and Materials ([`materials`]) and 3D (Classic): Extrude & Bevel, Revolve and
//! Rotate as live effects.
//!
//! The object stays editable art; the renderer, the exporters ([`crate::bake_document`]) and
//! Expand Appearance draw it as [`three_d_art`]: a group of flat, shaded faces of the solid seen
//! from the effect's rotation (and perspective), painted back to front.
//!
//! - **Extrude**: every filled shape pushed back by the extrude depth, with its side walls and
//!   (with Cap on) its front and back faces.
//! - **Revolve**: every shape's outline (an open path's line) swept around a vertical axis at the
//!   art's left (or right) edge.
//! - **Rotate**: the flat art turned in space.
//!
//! Faces take the colour of the shape's fill (a stroke-only shape: its stroke's), shaded by one
//! light: none ("No Shading"), diffuse, or diffuse with a highlight ("Plastic"); "Wireframe" draws
//! the faces' edges only. Members of a group become solids of their own, sorted together.
//!
//! 3D and Materials ([`MATERIALS`]) shares the face pipeline (the view, the shapes, the faces and
//! their painting order) and adds its own object types, materials, lights and shadows.

mod materials;
#[cfg(test)]
mod tests_materials;

pub use materials::{
    BEVEL_PROFILES, LIGHTING_PRESETS, MATERIAL_PRESETS, MATERIALS, MAX_LIGHTS, OBJECT_TYPES, PARAMS_DOC as MATERIALS_DOC, ROTATION_PRESETS,
    defaults as materials_defaults, lighting_preset, material_preset, preview_shade, rotation_preset,
};

use std::sync::{Arc, Mutex, PoisonError};

use kurbo::{BezPath, PathEl, Point};
use serde_json::Value;
use vectorcraft_doc::appearance::{FillLayer, LineJoin};
use vectorcraft_doc::color::{Color, Paint};
use vectorcraft_doc::{Appearance, AppearanceItem, Node, NodeKind, StrokeLayer};
use vectorcraft_geom::{FillRule, PathData, Rect};

use crate::util::{flag, num, text};
use crate::{GeomContext, apply_geometry_bez, is_geometry, is_raster};

/// Effect › 3D (Classic) › Extrude & Bevel.
pub const EXTRUDE: &str = "threeD.extrude";
/// Effect › 3D (Classic) › Revolve.
pub const REVOLVE: &str = "threeD.revolve";
/// Effect › 3D (Classic) › Rotate.
pub const ROTATE: &str = "threeD.rotate";

/// The most faces one object's solid is built from: finer art is flattened more coarsely.
const MAX_FACES: usize = 20_000;
/// The most points the art's outlines are flattened to.
const MAX_POINTS: usize = 4_000;
/// Steps around a full revolution.
const REVOLVE_STEPS: f64 = 48.0;
/// Width of the stroke each face gets in its own colour, which hides the seams between faces.
const SEAM: f64 = 0.25;
/// Width of a wireframe's lines.
const WIRE: f64 = 0.5;

/// Is `id` one of the 3D effects?
pub fn is_three_d(id: &str) -> bool {
    matches!(id, EXTRUDE | REVOLVE | ROTATE | MATERIALS)
}

/// Does `n` carry a visible 3D effect?
pub fn has_three_d(n: &Node) -> bool {
    n.appearance.effects.iter().any(|e| e.visible && is_three_d(&e.id))
}

/// How the solid is built.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Solid {
    Extrude { depth: f64, cap: bool },
    Revolve { angle: f64, offset: f64, right: bool, cap: bool },
    Rotate,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Surface {
    Wireframe,
    NoShading,
    Diffuse,
    Plastic,
}

/// A 3D effect's options, read and clamped to their ranges.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Params {
    solid: Solid,
    /// Rotation about the X, Y and Z axes, in degrees.
    rotate: [f64; 3],
    /// The camera's field of view in degrees (0: no perspective).
    perspective: f64,
    surface: Surface,
    /// Light intensity, ambient light and highlight intensity (0..1), highlight size (0..1).
    light: f64,
    ambient: f64,
    highlight: f64,
    highlight_size: f64,
}

impl Params {
    fn parse(id: &str, p: &Value) -> Option<Self> {
        let solid = match id {
            EXTRUDE => Solid::Extrude { depth: num(p, "depth", 50.0).clamp(0.0, 2000.0), cap: flag(p, "cap", true) },
            REVOLVE => Solid::Revolve {
                angle: num(p, "angle", 360.0).clamp(1.0, 360.0),
                offset: num(p, "offset", 0.0).clamp(0.0, 1000.0),
                right: text(p, "from", "leftEdge").eq_ignore_ascii_case("rightEdge"),
                cap: flag(p, "cap", true),
            },
            ROTATE => Solid::Rotate,
            _ => return None,
        };
        let surface = match text(p, "surface", "plastic").to_ascii_lowercase().as_str() {
            "wireframe" => Surface::Wireframe,
            "noshading" | "none" => Surface::NoShading,
            "diffuse" => Surface::Diffuse,
            _ => Surface::Plastic,
        };
        let pct = |key, default| num(p, key, default).clamp(0.0, 100.0) / 100.0;
        Some(Self {
            solid,
            rotate: [num(p, "rotateX", -18.0), num(p, "rotateY", -26.0), num(p, "rotateZ", 8.0)].map(|a| a.clamp(-180.0, 180.0)),
            perspective: num(p, "perspective", 0.0).clamp(0.0, 160.0),
            surface,
            light: pct("lightIntensity", 100.0),
            ambient: pct("ambientLight", 50.0),
            highlight: pct("highlightIntensity", 60.0),
            highlight_size: pct("highlightSize", 90.0),
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct V3 {
    x: f64,
    y: f64,
    z: f64,
}

impl V3 {
    const fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }
    fn sub(self, o: Self) -> Self {
        Self::new(self.x - o.x, self.y - o.y, self.z - o.z)
    }
    fn add(self, o: Self) -> Self {
        Self::new(self.x + o.x, self.y + o.y, self.z + o.z)
    }
    fn scale(self, s: f64) -> Self {
        Self::new(self.x * s, self.y * s, self.z * s)
    }
    fn dot(self, o: Self) -> f64 {
        self.x * o.x + self.y * o.y + self.z * o.z
    }
    fn cross(self, o: Self) -> Self {
        Self::new(self.y * o.z - self.z * o.y, self.z * o.x - self.x * o.z, self.x * o.y - self.y * o.x)
    }
    /// The unit vector along `self` (`None` when it has no length).
    fn unit(self) -> Option<Self> {
        let l = self.dot(self).sqrt();
        (l.is_finite() && l > 1e-12).then(|| self.scale(1.0 / l))
    }
}

/// Where the solid is seen from: its rotation about `centre` and the camera.
struct View {
    rot: [[f64; 3]; 3],
    centre: V3,
    /// The camera's distance in front of the centre (`None`: parallel projection).
    focal: Option<f64>,
}

impl View {
    fn new(rotate: [f64; 3], perspective: f64, centre: V3, size: f64) -> Self {
        let [ax, ay, az] = rotate.map(f64::to_radians);
        let (sx, cx) = ax.sin_cos();
        let (sy, cy) = ay.sin_cos();
        let (sz, cz) = az.sin_cos();
        let rx = [[1.0, 0.0, 0.0], [0.0, cx, -sx], [0.0, sx, cx]];
        let ry = [[cy, 0.0, sy], [0.0, 1.0, 0.0], [-sy, 0.0, cy]];
        let rz = [[cz, -sz, 0.0], [sz, cz, 0.0], [0.0, 0.0, 1.0]];
        let rot = mul(rz, mul(ry, rx));
        let focal = (perspective >= 0.5).then(|| size.max(1.0) / (2.0 * (perspective.to_radians() / 2.0).tan()));
        Self { rot, centre, focal }
    }

    /// `p` rotated about the centre (relative to it).
    fn turn(&self, p: V3) -> V3 {
        let d = p.sub(self.centre);
        let r = |row: [f64; 3]| row[0] * d.x + row[1] * d.y + row[2] * d.z;
        V3::new(r(self.rot[0]), r(self.rot[1]), r(self.rot[2]))
    }

    /// A direction seen from the viewer (turned, not moved) back in the art's own space.
    fn unturn_dir(&self, d: V3) -> V3 {
        let c = |j: usize| self.rot[0][j] * d.x + self.rot[1][j] * d.y + self.rot[2][j] * d.z;
        V3::new(c(0), c(1), c(2))
    }

    /// A direction in the art's space as the viewer sees it.
    fn turn_dir(&self, d: V3) -> V3 {
        self.turn(d.add(self.centre))
    }

    /// A turned point on the page.
    fn project(&self, q: V3) -> Point {
        let s = match self.focal {
            Some(f) => f / (f + q.z).max(f * 0.05),
            None => 1.0,
        };
        Point::new(self.centre.x + q.x * s, self.centre.y + q.y * s)
    }
}

fn mul(a: [[f64; 3]; 3], b: [[f64; 3]; 3]) -> [[f64; 3]; 3] {
    let mut m = [[0.0; 3]; 3];
    for (i, row) in m.iter_mut().enumerate() {
        for (j, v) in row.iter_mut().enumerate() {
            *v = (0..3).map(|k| a[i][k] * b[k][j]).sum();
        }
    }
    m
}

/// One shape of the art: its outline in document space, how it fills and its colour.
struct Shape {
    path: BezPath,
    rule: FillRule,
    /// Filled (a closed region) rather than a line.
    filled: bool,
    color: [f64; 3],
}

/// A face ready to paint.
struct Face {
    path: BezPath,
    rule: FillRule,
    /// Distance from the viewer (larger: farther, painted first).
    depth: f64,
    color: [f64; 3],
}

/// The art of an object with a 3D effect (the last visible one): a group with the object's id,
/// name, transparency, mask and its raster effects, of the solid's faces (with 3D and Materials'
/// shadow first). `None` without a visible 3D effect or without art to build a solid from.
///
/// The renderer asks for it more than once a frame (culling, then drawing) and on every frame
/// the object hasn't changed: the last few solids are kept and reused while their object stays
/// the same.
pub fn three_d_art(n: &Node) -> Option<Node> {
    if !has_three_d(n) {
        return None;
    }
    if let Some(hit) = cache().lock().unwrap_or_else(PoisonError::into_inner).iter().find(|(k, _)| k == n).map(|(_, v)| v.clone()) {
        return hit;
    }
    let art = build_art(n);
    let mut c = cache().lock().unwrap_or_else(PoisonError::into_inner);
    c.insert(0, (n.clone(), art.clone()));
    c.truncate(CACHED);
    art
}

/// How many solids [`three_d_art`] keeps.
const CACHED: usize = 4;

type Cache = Mutex<Vec<(Node, Option<Node>)>>;

fn cache() -> &'static Cache {
    static C: std::sync::OnceLock<Cache> = std::sync::OnceLock::new();
    C.get_or_init(Mutex::default)
}

fn build_art(n: &Node) -> Option<Node> {
    let fx = n.appearance.effects.iter().rev().find(|e| e.visible && is_three_d(&e.id))?;
    let merged = crate::merged_params(&fx.id, &fx.params);
    let shapes = shapes_of(n);
    let bounds = shapes.iter().filter_map(|s| rect_of(&s.path)).reduce(|a, b| a.union(b))?;
    let (faces, wire, shadow) = if fx.id == MATERIALS {
        let (faces, shadow) = materials::build(&materials::Params::parse(&merged), &shapes, bounds);
        (faces, false, shadow)
    } else {
        let params = Params::parse(&fx.id, &merged)?;
        (build(&params, &shapes, bounds), params.surface == Surface::Wireframe, None)
    };
    let shadow = shadow.map(|s| {
        let mut node = Node::path(
            n.id,
            PathData::from_bezpath(&s.path),
            Appearance { items: vec![AppearanceItem::Fill(FillLayer::new(Paint::solid(Color::rgb(0.0, 0.0, 0.0))))], ..Default::default() },
        );
        node.opacity = s.opacity as f32;
        if s.softness > 0.0 {
            node.appearance.effects.push(vectorcraft_doc::Effect {
                id: "blur.gaussian".into(),
                params: serde_json::json!({"radius": s.softness}),
                visible: true,
            });
        }
        Arc::new(node)
    });
    let children = shadow
        .into_iter()
        .chain(faces.into_iter().map(|f| {
            let paint = Paint::solid(Color::rgb(f.color[0] as f32, f.color[1] as f32, f.color[2] as f32));
            let mut stroke = StrokeLayer::new(paint.clone(), if wire { WIRE } else { SEAM });
            stroke.join = LineJoin::Round;
            let mut items = vec![AppearanceItem::Stroke(stroke)];
            if !wire {
                items.insert(0, AppearanceItem::Fill(FillLayer::new(paint)));
            }
            let mut face = Node::path(n.id, PathData::from_bezpath(&f.path), Appearance { items, ..Default::default() });
            if let NodeKind::Path { rule, .. } = &mut face.kind {
                *rule = f.rule;
            }
            Arc::new(face)
        }))
        .collect();
    let mut g = Node::group(n.id, children);
    (g.visible, g.locked) = (n.visible, n.locked);
    g.name = n.name.clone();
    g.opacity = n.opacity;
    g.blend = n.blend;
    g.isolate = n.isolate;
    g.mask = n.mask.clone();
    g.attrs = n.attrs.clone();
    // The shadows and glows go around the solid.
    g.appearance.effects = n.appearance.effects.iter().filter(|e| is_raster(&e.id)).cloned().collect();
    Some(g)
}

fn rect_of(bp: &BezPath) -> Option<Rect> {
    let r = kurbo::Shape::bounding_box(bp);
    (!bp.elements().is_empty() && r.is_finite()).then_some(r)
}

/// The shapes a solid is built from: the object's paths with its geometry effects applied (type
/// as its glyphs, live objects and groups through their members).
fn shapes_of(n: &Node) -> Vec<Shape> {
    let art = match &n.kind {
        NodeKind::Path { .. } | NodeKind::Compound { .. } => n.clone(),
        NodeKind::Group { .. } | NodeKind::Layer { .. } => crate::reshape::outlined_members(n),
        _ => crate::reshape(n, None).or_else(|| crate::outline_art(n, None)).unwrap_or_else(|| n.clone()),
    };
    let mut out = vec![];
    art.walk(&mut |leaf| {
        let Some((path, rule)) = leaf_path(leaf) else { return };
        if path.elements().is_empty() {
            return;
        }
        let fill = paint_color(&leaf.appearance.fill_paint());
        let stroke = leaf.appearance.stroke().filter(|s| s.width > 0.0).and_then(|s| paint_color(&s.paint));
        let (filled, color) = match (fill, stroke) {
            (Some(c), _) => (true, c),
            (None, Some(c)) => (false, c),
            // Unpainted art still shows as a grey solid.
            (None, None) => (true, [0.6; 3]),
        };
        out.push(Shape { path, rule, filled, color });
    });
    out
}

/// A leaf's outline, with the leaf's own geometry effects applied.
fn leaf_path(leaf: &Node) -> Option<(BezPath, FillRule)> {
    let (bp, rule) = match &leaf.kind {
        NodeKind::Path { path, rule, guide: false, clipping: false, .. } => (path.to_bezpath(), *rule),
        NodeKind::Compound { children, rule } => {
            let mut bp = BezPath::new();
            for c in children {
                if let Some(p) = c.path_data() {
                    bp.extend(p.to_bezpath());
                }
            }
            (bp, *rule)
        }
        _ => return None,
    };
    let geometry: Vec<_> = leaf.appearance.effects.iter().filter(|e| e.visible && is_geometry(&e.id)).cloned().collect();
    if geometry.is_empty() {
        return Some((bp, rule));
    }
    let b = rect_of(&bp)?;
    Some((apply_geometry_bez(&geometry, &bp, b, &GeomContext::of(leaf)), rule))
}

/// A paint's colour as linear-ish RGB 0..1 (a gradient: its stops' average).
fn paint_color(p: &Paint) -> Option<[f64; 3]> {
    let rgb = |c: &Color| c.to_rgb().map(f64::from);
    match p {
        Paint::Solid { color, .. } => Some(rgb(color)),
        Paint::Gradient(g) => {
            let stops = &g.gradient.stops;
            let n = stops.len().max(1) as f64;
            (!stops.is_empty())
                .then(|| stops.iter().map(|s| rgb(&s.color)).fold([0.0; 3], |a, c| [a[0] + c[0] / n, a[1] + c[1] / n, a[2] + c[2] / n]))
        }
        _ => None,
    }
}

/// Every subpath of `bp` as a polyline (with whether it is closed), flattened at `tol`.
fn polylines(bp: &BezPath, tol: f64) -> Vec<(Vec<Point>, bool)> {
    let mut out: Vec<(Vec<Point>, bool)> = vec![];
    kurbo::flatten(bp.iter(), tol, |el| match el {
        PathEl::MoveTo(p) => out.push((vec![p], false)),
        PathEl::LineTo(p) => {
            if let Some((l, _)) = out.last_mut()
                && l.last().is_none_or(|q| (*q - p).hypot() > 1e-9)
            {
                l.push(p);
            }
        }
        PathEl::ClosePath => {
            if let Some((l, closed)) = out.last_mut() {
                *closed = true;
                if l.len() > 1 && l.first().zip(l.last()).is_some_and(|(a, b)| (*a - *b).hypot() < 1e-9) {
                    l.pop();
                }
            }
        }
        _ => {}
    });
    out.retain(|(l, _)| l.len() > 1);
    out
}

/// The shapes' polylines at a tolerance that keeps them within [`MAX_POINTS`] (and `budget`).
fn flattened(shapes: &[Shape], size: f64, budget: usize) -> Vec<Vec<(Vec<Point>, bool)>> {
    let mut tol = (size / 400.0).max(0.05);
    loop {
        let lines: Vec<_> = shapes.iter().map(|s| polylines(&s.path, tol)).collect();
        let count: usize = lines.iter().flatten().map(|(l, _)| l.len()).sum();
        if count <= budget.min(MAX_POINTS) || tol > size {
            return lines;
        }
        tol *= 2.0;
    }
}

/// The faces of the solid, back to front.
fn build(p: &Params, shapes: &[Shape], bounds: Rect) -> Vec<Face> {
    let size = bounds.width().hypot(bounds.height());
    let light = Light::new(p);
    let centre = |x: f64| V3::new(x, bounds.center().y, 0.0);
    match p.solid {
        Solid::Extrude { depth, cap } => {
            let view = View::new(p.rotate, p.perspective, centre(bounds.center().x), size + depth);
            let mut sink = Sink::new(&view, &light, false);
            let lines = flattened(shapes, size, MAX_FACES);
            for (s, lines) in shapes.iter().zip(&lines) {
                extrude(&mut sink, s, lines, depth, cap);
            }
            sink.sorted()
        }
        Solid::Rotate => {
            let view = View::new(p.rotate, p.perspective, centre(bounds.center().x), size);
            let mut sink = Sink::new(&view, &light, false);
            for s in shapes {
                sink.cap(s, 0.0, None);
            }
            sink.sorted()
        }
        Solid::Revolve { angle, offset, right, cap } => {
            let axis = revolve_axis(bounds, offset, right);
            let view = View::new(p.rotate, p.perspective, centre(axis), revolve_size(bounds, axis));
            let mut sink = Sink::new(&view, &light, false);
            let steps = revolve_steps(angle);
            let lines = flattened(shapes, size, MAX_FACES / steps.max(1));
            for (s, lines) in shapes.iter().zip(&lines) {
                revolve(&mut sink, s, lines, axis, angle.to_radians(), steps, cap);
            }
            sink.sorted()
        }
    }
}

/// Where a revolve's axis is: at the art's left (or right) edge, `offset` away from it.
fn revolve_axis(bounds: Rect, offset: f64, right: bool) -> f64 {
    if right { bounds.x1 + offset } else { bounds.x0 - offset }
}

/// The size of a revolved solid (its view's reference for perspective).
fn revolve_size(bounds: Rect, axis: f64) -> f64 {
    let reach = (bounds.x0 - axis).abs().max((bounds.x1 - axis).abs());
    (2.0 * reach).hypot(bounds.height())
}

/// Steps around a revolution of `angle` degrees.
fn revolve_steps(angle: f64) -> usize {
    (angle / 360.0 * REVOLVE_STEPS).ceil().clamp(2.0, REVOLVE_STEPS) as usize
}

/// How a face's colour is lit: by the Classic light, or by 3D and Materials' lights and material.
trait Shader {
    /// `base` lit on a face with (turned) normal `n`.
    fn shade(&self, base: [f64; 3], n: V3) -> [f64; 3];
}

/// Where a solid's faces go as they are built: shaded and projected into `faces`, and with a
/// shadow to cast, their corners (in the art's own space) into `shadow`.
struct Sink<'a> {
    view: &'a View,
    shader: &'a dyn Shader,
    faces: Vec<Face>,
    shadow: Option<Vec<Vec<V3>>>,
}

impl<'a> Sink<'a> {
    fn new(view: &'a View, shader: &'a dyn Shader, shadow: bool) -> Self {
        Self { view, shader, faces: vec![], shadow: shadow.then(Vec::new) }
    }

    /// Room for another face: the solid stops at [`MAX_FACES`] however fine its art.
    fn full(&self) -> bool {
        self.faces.len() >= MAX_FACES
    }

    /// A flat polygon (document-space corners `pts`).
    fn polygon(&mut self, pts: &[V3], color: [f64; 3]) {
        if self.full() {
            return;
        }
        self.faces.extend(polygon(self.view, self.shader, pts, color, 0.0));
        if let Some(s) = &mut self.shadow
            && s.len() < MAX_FACES
        {
            s.push(pts.to_vec());
        }
    }

    /// The flat shape `s` at depth `z` ([`cap_face`]).
    fn cap(&mut self, s: &Shape, z: f64, facing: Option<V3>) {
        if self.full() {
            return;
        }
        self.faces.extend(cap_face(self.view, self.shader, s, z, facing));
    }

    /// The outline `lines` at depth `z` cast a shadow (a cap drawn with [`Sink::cap`]).
    fn cap_shadow(&mut self, lines: &[(Vec<Point>, bool)], z: f64) {
        if let Some(s) = &mut self.shadow
            && s.len() < MAX_FACES
        {
            s.extend(lines.iter().map(|(l, _)| l.iter().map(|p| V3::new(p.x, p.y, z)).collect()));
        }
    }

    /// The faces, back to front.
    fn sorted(mut self) -> Vec<Face> {
        self.faces.sort_by(|a, b| b.depth.total_cmp(&a.depth));
        self.faces
    }
}

/// A flat polygon (document-space corners `pts`) as a face, shaded by its normal.
fn polygon(view: &View, shader: &dyn Shader, pts: &[V3], color: [f64; 3], nudge: f64) -> Option<Face> {
    let turned: Vec<V3> = pts.iter().map(|q| view.turn(*q)).collect();
    let (a, b, c) = (*turned.first()?, *turned.get(1)?, *turned.get(turned.len().checked_sub(1)?)?);
    let normal = b.sub(a).cross(c.sub(a)).unit().or_else(|| turned.get(2).and_then(|t| t.sub(a).cross(c.sub(b)).unit()))?;
    let mut path = BezPath::new();
    for (i, q) in turned.iter().enumerate() {
        let p = view.project(*q);
        if i == 0 { path.move_to(p) } else { path.line_to(p) }
    }
    path.close_path();
    let depth = turned.iter().map(|q| q.z).sum::<f64>() / turned.len() as f64 + nudge;
    Some(Face { path, rule: FillRule::NonZero, depth, color: shader.shade(color, normal) })
}

/// The flat shape `s` at depth `z`, turned and projected (curves kept). With `facing`, only when
/// its outward normal (`facing`, unturned) points at the viewer. Drawn in front of the rest of its
/// solid.
fn cap_face(view: &View, shader: &dyn Shader, s: &Shape, z: f64, facing: Option<V3>) -> Option<Face> {
    let normal = view.turn_dir(facing.unwrap_or(V3::new(0.0, 0.0, -1.0)));
    if facing.is_some() && normal.z >= 0.0 {
        return None;
    }
    let data = PathData::from_bezpath(&s.path);
    let piece = rect_of(&s.path).map_or(1.0, |b| (b.width().max(b.height()) / 64.0).max(0.25));
    let path = vectorcraft_doc::live::map_nonlinear(&data, piece, |p| view.project(view.turn(V3::new(p.x, p.y, z)))).to_bezpath();
    // In front of every other face of the shape: its nearest corner, a little nearer.
    let near = rect_of(&s.path).map_or(0.0, |b| {
        [b.origin(), Point::new(b.x1, b.y0), Point::new(b.x0, b.y1), Point::new(b.x1, b.y1)]
            .iter()
            .map(|c| view.turn(V3::new(c.x, c.y, z)).z)
            .fold(f64::INFINITY, f64::min)
    });
    Some(Face { path, rule: s.rule, depth: near - 1e-3, color: shader.shade(s.color, normal) })
}

fn extrude(sink: &mut Sink, s: &Shape, lines: &[(Vec<Point>, bool)], depth: f64, cap: bool) {
    let (front, back) = (-depth / 2.0, depth / 2.0);
    for (line, closed) in lines {
        let closed = *closed || s.filled;
        let n = line.len();
        let edges = if closed { n } else { n.saturating_sub(1) };
        for i in 0..edges {
            let (Some(a), Some(b)) = (line.get(i), line.get((i + 1) % n)) else { continue };
            let quad = [V3::new(a.x, a.y, front), V3::new(b.x, b.y, front), V3::new(b.x, b.y, back), V3::new(a.x, a.y, back)];
            sink.polygon(&quad, s.color);
        }
    }
    if s.filled && (cap || depth <= 0.0) {
        if depth <= 0.0 {
            sink.cap(s, 0.0, None);
            sink.cap_shadow(lines, 0.0);
        } else {
            sink.cap(s, front, Some(V3::new(0.0, 0.0, -1.0)));
            sink.cap(s, back, Some(V3::new(0.0, 0.0, 1.0)));
            sink.cap_shadow(lines, front);
        }
    }
}

fn revolve(sink: &mut Sink, s: &Shape, lines: &[(Vec<Point>, bool)], axis: f64, angle: f64, steps: usize, cap: bool) {
    let at = |p: &Point, t: f64| {
        let r = p.x - axis;
        V3::new(axis + r * t.cos(), p.y, r * t.sin())
    };
    let theta = |j: usize| angle * j as f64 / steps.max(1) as f64;
    for (line, closed) in lines {
        let closed = *closed && s.filled;
        let n = line.len();
        let edges = if closed { n } else { n.saturating_sub(1) };
        for i in 0..edges {
            let (Some(a), Some(b)) = (line.get(i), line.get((i + 1) % n)) else { continue };
            for j in 0..steps {
                let (t0, t1) = (theta(j), theta(j + 1));
                sink.polygon(&[at(a, t0), at(b, t0), at(b, t1), at(a, t1)], s.color);
            }
        }
        // The profile at both ends of a partial revolution.
        if closed && cap && angle < std::f64::consts::TAU - 1e-9 {
            for t in [0.0, angle] {
                let ring: Vec<V3> = line.iter().map(|p| at(p, t)).collect();
                sink.polygon(&ring, s.color);
            }
        }
    }
}

/// One light, up and to the left of the viewer.
struct Light {
    surface: Surface,
    dir: V3,
    p: Params,
}

impl Light {
    fn new(p: &Params) -> Self {
        Self { surface: p.surface, dir: V3::new(-0.45, -0.55, -0.7).unit().unwrap_or(V3::new(0.0, 0.0, -1.0)), p: *p }
    }
}

impl Shader for Light {
    /// `base` lit on a face with (turned) normal `n`, which is made to face the viewer.
    fn shade(&self, base: [f64; 3], n: V3) -> [f64; 3] {
        let n = if n.z > 0.0 { n.scale(-1.0) } else { n };
        match self.surface {
            Surface::Wireframe | Surface::NoShading => base,
            Surface::Diffuse | Surface::Plastic => {
                let diffuse = n.dot(self.dir).max(0.0);
                let i = (self.p.ambient + (1.0 - self.p.ambient) * self.p.light * diffuse).clamp(0.0, 1.0);
                let lit = base.map(|c| c * i);
                if self.surface == Surface::Diffuse {
                    return lit;
                }
                let half = self.dir.add(V3::new(0.0, 0.0, -1.0)).unit().unwrap_or(self.dir);
                let k = 5.0 + (1.0 - self.p.highlight_size) * 150.0;
                let spec = (self.p.highlight * n.dot(half).max(0.0).powf(k)).clamp(0.0, 1.0);
                lit.map(|c| c + (1.0 - c) * spec)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use vectorcraft_doc::{Effect, NodeId};
    use vectorcraft_geom::shapes;

    fn square(fx: &str, params: Value) -> Node {
        let mut n = Node::path(
            NodeId(3),
            shapes::rectangle(Rect::new(0.0, 0.0, 100.0, 100.0)),
            Appearance::basic(Paint::solid(Color::rgb(1.0, 0.0, 0.0)), Paint::None, 0.0),
        );
        n.appearance.effects.push(Effect { id: fx.into(), params, visible: true });
        n
    }

    fn faces(art: &Node) -> Vec<&Node> {
        art.children().unwrap().iter().map(|c| &**c).collect()
    }

    fn rgb(n: &Node) -> [f32; 3] {
        n.appearance.fill_paint().color().unwrap().to_rgb()
    }

    #[test]
    fn params_take_defaults_and_stay_in_range() {
        let p = Params::parse(EXTRUDE, &json!({})).unwrap();
        assert_eq!((p.solid, p.rotate, p.surface), (Solid::Extrude { depth: 50.0, cap: true }, [-18.0, -26.0, 8.0], Surface::Plastic));
        let p = Params::parse(EXTRUDE, &json!({"depth": 1e9, "rotateX": -1e9, "perspective": 1e9, "ambientLight": -5})).unwrap();
        assert_eq!((p.solid, p.rotate[0], p.perspective, p.ambient), (Solid::Extrude { depth: 2000.0, cap: true }, -180.0, 160.0, 0.0));
        let r = Params::parse(REVOLVE, &json!({"angle": 0, "from": "rightEdge"})).unwrap();
        assert_eq!(r.solid, Solid::Revolve { angle: 1.0, offset: 0.0, right: true, cap: true });
        assert!(Params::parse("nope", &json!({})).is_none());
        assert!(!has_three_d(&Node::path(NodeId(1), shapes::rectangle(Rect::new(0.0, 0.0, 1.0, 1.0)), Appearance::default())));
    }

    /// Extrude: side walls and the front face, shaded darker than the lit front, in the object's
    /// identity; facing straight on, the front face covers the square exactly.
    #[test]
    fn extrude_builds_shaded_walls_behind_the_front() {
        let n = square(EXTRUDE, json!({"depth": 40}));
        let art = three_d_art(&n).unwrap();
        assert_eq!(art.id, NodeId(3));
        let fs = faces(&art);
        assert_eq!(fs.len(), 4 + 1, "four walls and the visible cap");
        let front = fs.last().unwrap();
        assert!(fs[..4].iter().all(|w| rgb(w) != rgb(front)), "walls shaded apart from the front");
        for f in &fs {
            let [r, g, b] = rgb(f);
            assert!(r > 0.0 && g < 0.5 && b < 0.5 && r <= 1.0, "red, shaded: {:?}", [r, g, b]);
        }
        // The solid reaches beyond the square (its walls show), while the front stays its size.
        let b = art.geometric_bounds().unwrap();
        assert!(b.width() > 105.0 || b.height() > 105.0, "{b:?}");
        // Seen straight on, only the front shows and it is the square.
        let flat = three_d_art(&square(EXTRUDE, json!({"depth": 40, "rotateX": 0, "rotateY": 0, "rotateZ": 0}))).unwrap();
        let fb = flat.children().unwrap().last().unwrap().geometric_bounds().unwrap();
        assert!((fb.x0).abs() < 1e-6 && (fb.x1 - 100.0).abs() < 1e-6 && (fb.y1 - 100.0).abs() < 1e-6, "{fb:?}");
        // No cap: no front face.
        assert_eq!(faces(&three_d_art(&square(EXTRUDE, json!({"cap": false}))).unwrap()).len(), 4);
    }

    #[test]
    fn perspective_makes_the_far_side_smaller() {
        let p = |angle| {
            let n = square(EXTRUDE, json!({"depth": 100, "rotateX": 0, "rotateY": 0, "rotateZ": 0, "perspective": angle, "cap": false}));
            three_d_art(&n).unwrap().geometric_bounds().unwrap()
        };
        let (flat, deep) = (p(0), p(90));
        assert!((flat.width() - 100.0).abs() < 1e-6, "parallel: walls edge-on {flat:?}");
        // The near (front) edges spread wider than the far ones: the walls show around the square.
        assert!(deep.width() > 100.5, "{deep:?}");
    }

    #[test]
    fn revolve_sweeps_the_shape_around_its_left_edge() {
        let n = square(REVOLVE, json!({"rotateX": 0, "rotateY": 0, "rotateZ": 0}));
        let art = three_d_art(&n).unwrap();
        // Seen from the front, a cylinder of radius 100 around x = 0.
        let b = art.geometric_bounds().unwrap();
        assert!((b.x0 + 100.0).abs() < 1.0 && (b.x1 - 100.0).abs() < 1.0 && (b.height() - 100.0).abs() < 1.0, "{b:?}");
        assert!(faces(&art).len() > 48, "{}", faces(&art).len());
        // Lit from the upper left: the left of the drum is brighter than the right.
        let lum = |f: &&Node| rgb(f)[0];
        // (The front half of the drum is painted last.)
        let fs = faces(&art);
        let near = fs.get(fs.len() / 2..).unwrap();
        let side = |left: bool| {
            let lit: Vec<f32> =
                near.iter().filter(|f| f.geometric_bounds().is_some_and(|b| if left { b.x1 < -50.0 } else { b.x0 > 50.0 })).map(lum).collect();
            lit.iter().sum::<f32>() / lit.len().max(1) as f32
        };
        assert!(side(true) > side(false), "{} vs {}", side(true), side(false));
        // A half revolution with caps closes its ends.
        let half = three_d_art(&square(REVOLVE, json!({"angle": 180}))).unwrap();
        assert!(faces(&half).len() >= 3 * 24 + 2, "{}", faces(&half).len());
    }

    #[test]
    fn rotate_turns_the_flat_art_and_wireframe_draws_edges_only() {
        let art = three_d_art(&square(ROTATE, json!({"rotateX": 0, "rotateY": 60, "rotateZ": 0, "surface": "noShading"}))).unwrap();
        let [face] = faces(&art)[..] else { panic!("one face") };
        let b = face.geometric_bounds().unwrap();
        assert!((b.width() - 50.0).abs() < 0.5 && (b.height() - 100.0).abs() < 1e-6, "cos 60° foreshortens: {b:?}");
        assert_eq!(rgb(face), [1.0, 0.0, 0.0], "no shading keeps the colour");
        let wire = three_d_art(&square(EXTRUDE, json!({"surface": "wireframe"}))).unwrap();
        assert!(faces(&wire).iter().all(|f| f.appearance.fill_paint().is_none() && !f.appearance.stroke_paint().is_none()));
    }

    /// Hostile numbers and empty art never panic; a hidden effect draws nothing of its own.
    #[test]
    fn hostile_params_and_empty_art_are_safe() {
        for params in [json!({"depth": f64::MAX, "perspective": 160, "rotateX": 90}), json!({"rotateY": "x", "surface": 7}), json!(null)] {
            for id in [EXTRUDE, REVOLVE, ROTATE] {
                let _ = three_d_art(&square(id, params.clone()));
            }
        }
        let mut empty = Node::path(NodeId(1), PathData::default(), Appearance::default());
        empty.appearance.effects.push(Effect { id: EXTRUDE.into(), params: json!({}), visible: true });
        assert!(three_d_art(&empty).is_none());
        let mut hidden = square(EXTRUDE, json!({}));
        hidden.appearance.effects[0].visible = false;
        assert!(three_d_art(&hidden).is_none());
    }
}
