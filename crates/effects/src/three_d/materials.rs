//! Effect › 3D and Materials: one live effect ([`MATERIALS`]) that turns flat art into a lit,
//! shaded solid.
//!
//! - **Object**: Plane (the flat art turned in space), Extrude (pushed back by a depth, with a
//!   twist over the depth, a taper of the back face and a bevel along the front, or both, edges),
//!   Revolve (swept around a vertical axis at the art's left or right edge) or Inflate (a pillow
//!   puffed up from the shape, deepest where it is farthest from its edges).
//! - **Rotation**: X, Y and Z angles (with named views, [`ROTATION_PRESETS`]) and perspective.
//! - **Material**: a roughness and a metallic amount (0..1) and an optional colour that replaces
//!   the art's. Faces are shaded with a Lambert diffuse term and a GGX specular highlight: the
//!   rougher the material, the wider and dimmer the highlight; the more metallic, the darker the
//!   diffuse colour and the more the highlight (and a soft reflection of the surroundings) takes
//!   the material's colour.
//! - **Lighting**: up to [`MAX_LIGHTS`] lights (colour, intensity, rotation around the view,
//!   height above the art, softness) and ambient light; a shadow cast by the first light onto a
//!   plane behind or below the object, drawn as the solid's faces projected onto it, filled with
//!   translucent black and blurred.
//!
//! The faces share 3D (Classic)'s pipeline: built in the art's own space, turned to the view,
//! projected and painted back to front, within the same face budget.

use std::f64::consts::PI;

use kurbo::{BezPath, Point, Vec2};
use serde_json::{Value, json};
use vectorcraft_doc::color::Color;
use vectorcraft_geom::{FillRule, Rect};

use super::{Face, MAX_FACES, Shader, Shape, Sink, V3, View, flattened, rect_of, revolve, revolve_axis, revolve_size, revolve_steps};
use crate::util::{flag, num, text};

/// Effect › 3D and Materials.
pub const MATERIALS: &str = "threeD.materials";

/// The most lights a 3D and Materials effect has.
pub const MAX_LIGHTS: usize = 4;
/// The most slices a twist cuts the depth into (one per 6° of twist below that).
const MAX_TWIST_SLICES: usize = 90;
/// Grid cells along an inflated shape's longer side.
const INFLATE_GRID: usize = 40;
/// The most points an inflated shape's outline is flattened to.
const INFLATE_POINTS: usize = 600;
/// The most points 3D and Materials flattens all of an object's outlines to.
const MAX_OUTLINE_POINTS: usize = 4_000;

/// The object types: (id, label).
pub const OBJECT_TYPES: [(&str, &str); 4] = [("plane", "Plane"), ("extrude", "Extrude"), ("revolve", "Revolve"), ("inflate", "Inflate")];

/// The bevel profiles: (id, label).
pub const BEVEL_PROFILES: [(&str, &str); 4] = [("straight", "Straight"), ("rounded", "Round"), ("stepped", "Stepped"), ("cove", "Cove")];

/// Named views: (id, label, [rotateX, rotateY, rotateZ] in degrees). The isometric views turn the
/// three axes of the art equally far from the page (Top: a steeper view of the top).
pub const ROTATION_PRESETS: [(&str, &str, [f64; 3]); 15] = [
    ("front", "Front", [0.0, 0.0, 0.0]),
    ("back", "Back", [0.0, 180.0, 0.0]),
    ("left", "Left", [0.0, -90.0, 0.0]),
    ("right", "Right", [0.0, 90.0, 0.0]),
    ("top", "Top", [90.0, 0.0, 0.0]),
    ("bottom", "Bottom", [-90.0, 0.0, 0.0]),
    ("offAxisFront", "Off-Axis Front", [15.0, -25.0, 0.0]),
    ("offAxisBack", "Off-Axis Back", [15.0, 155.0, 0.0]),
    ("offAxisLeft", "Off-Axis Left", [15.0, -65.0, 0.0]),
    ("offAxisRight", "Off-Axis Right", [15.0, 65.0, 0.0]),
    ("offAxisTop", "Off-Axis Top", [65.0, -20.0, 0.0]),
    ("offAxisBottom", "Off-Axis Bottom", [-65.0, -20.0, 0.0]),
    ("isometricLeft", "Isometric Left", [45.0, -35.264, -30.0]),
    ("isometricRight", "Isometric Right", [45.0, 35.264, 30.0]),
    ("isometricTop", "Isometric Top", [63.435, 24.095, 39.232]),
];

/// Material presets: (id, label, roughness, metallic, colour or `None` for the art's own).
pub const MATERIAL_PRESETS: [(&str, &str, f64, f64, Option<&str>); 10] = [
    ("default", "Default", 0.45, 0.0, None),
    ("plastic", "Plastic", 0.25, 0.0, None),
    ("chalk", "Chalk", 0.9, 0.0, None),
    ("glossy", "Glossy", 0.08, 0.0, None),
    ("metal", "Metal", 0.3, 1.0, None),
    ("brushedMetal", "Brushed Metal", 0.6, 1.0, None),
    ("gold", "Gold", 0.3, 1.0, Some("#e2b04a")),
    ("copper", "Copper", 0.35, 1.0, Some("#c97a50")),
    ("mirror", "Mirror", 0.05, 1.0, Some("#e8e8e8")),
    ("rubber", "Rubber", 0.95, 0.0, Some("#2e2e2e")),
];

/// Lighting presets: (id, label).
pub const LIGHTING_PRESETS: [(&str, &str); 4] = [("standard", "Standard"), ("diffuse", "Diffuse"), ("topLeft", "Top Left"), ("right", "Right")];

/// A preset light: (rotation°, height°, intensity %, softness %).
type PresetLamp = (f64, f64, f64, f64);

/// A lighting preset's lights and its ambient light (%).
fn lighting_lamps(id: &str) -> Option<(&'static [PresetLamp], f64)> {
    Some(match id {
        "standard" => (&[(-45.0, 45.0, 100.0, 50.0)], 30.0),
        "diffuse" => (&[(-30.0, 70.0, 70.0, 100.0), (150.0, 30.0, 30.0, 100.0)], 55.0),
        "topLeft" => (&[(-45.0, 20.0, 115.0, 25.0)], 20.0),
        "right" => (&[(90.0, 25.0, 115.0, 25.0)], 20.0),
        _ => return None,
    })
}

/// The parameters a lighting preset sets: `{lighting, lights, ambient}`.
pub fn lighting_preset(id: &str) -> Option<Value> {
    let (lamps, ambient) = lighting_lamps(id)?;
    let lights: Vec<Value> = lamps
        .iter()
        .map(|(rotation, height, intensity, softness)| {
            json!({"color": "#ffffff", "intensity": intensity, "rotation": rotation, "height": height, "softness": softness})
        })
        .collect();
    Some(json!({"lighting": id, "lights": lights, "ambient": ambient}))
}

/// The parameters a material preset sets: `{material, roughness, metallic, materialColor}`.
pub fn material_preset(id: &str) -> Option<Value> {
    let (id, _, roughness, metallic, color) = MATERIAL_PRESETS.iter().find(|m| m.0 == id)?;
    Some(json!({"material": id, "roughness": roughness, "metallic": metallic, "materialColor": color}))
}

/// The parameters a named view sets: `{rotation, rotateX, rotateY, rotateZ}`.
pub fn rotation_preset(id: &str) -> Option<Value> {
    let (id, _, [x, y, z]) = ROTATION_PRESETS.iter().find(|r| r.0 == id)?;
    Some(json!({"rotation": id, "rotateX": x, "rotateY": y, "rotateZ": z}))
}

/// The effect's defaults (an extrusion seen off-axis from the front, the Default material and
/// the Standard lighting, no shadow).
pub fn defaults() -> Value {
    let mut d = json!({
        "object": "extrude", "depth": 50.0, "cap": true, "twist": 0.0, "taper": 100.0,
        "bevel": false, "bevelShape": "straight", "bevelWidth": 4.0, "bevelHeight": 4.0, "bevelRepeat": 1, "bevelOutside": false, "bevelBothSides": false,
        "angle": 360.0, "offset": 0.0, "from": "leftEdge",
        "volume": 50.0, "bothSides": false,
        "perspective": 0.0,
        "shadow": false, "shadowPosition": "below", "shadowDistance": 0.0, "shadowBounds": 100.0, "shadowSoftness": 4.0, "shadowOpacity": 50.0,
    });
    for preset in [rotation_preset("offAxisFront"), material_preset("default"), lighting_preset("standard")].into_iter().flatten() {
        if let (Some(d), Value::Object(p)) = (d.as_object_mut(), preset) {
            d.extend(p);
        }
    }
    d
}

/// The parameter documentation in the effect catalogue.
pub const PARAMS_DOC: &str = "{object: \"plane\"|\"extrude\"|\"revolve\"|\"inflate\" (\"extrude\"), depth: pt 0..2000 (50; extrude and inflate), cap: bool (true; extrude and revolve), twist: deg -3600..3600 (0; the back turned this far, the depth sliced to follow), taper: % 1..400 (100; the back face's size), bevel: bool (false), bevelShape: \"straight\"|\"rounded\"|\"stepped\"|\"cove\", bevelWidth: pt 0..500 (4), bevelHeight: pt 0..500 (4), bevelRepeat: 1..10 (1), bevelOutside: bool (false: cut into the shape; true: added around it), bevelBothSides: bool (false: the front edge only), angle: deg 1..360 (360; revolve), offset: pt 0..1000 (0; revolve: the axis this far from the edge), from: \"leftEdge\"|\"rightEdge\" (revolve), volume: % 0..100 (50; inflate: how puffed up), bothSides: bool (false; inflate both faces), rotation: view preset id (see threeD.settings), rotateX, rotateY, rotateZ: deg -180..180 (15, -25, 0), perspective: deg 0..160 (0), material: preset id, roughness: 0..1 (0.45), metallic: 0..1 (0), materialColor: \"#rrggbb\"|null (null: the art's colours), lighting: preset id (\"standard\"), lights: [{color: \"#rrggbb\", intensity: % 0..400 (100), rotation: deg around the view (0 from the top, 90 from the right; -45), height: deg 0..90 above the art (45), softness: % 0..100 (50)}] (at most 4), ambient: % 0..200 (30), shadow: bool (false), shadowPosition: \"behind\"|\"below\" (\"below\"), shadowDistance: pt 0..1000 (0), shadowBounds: % 10..400 (100), shadowSoftness: pt 0..50 (4; blur), shadowOpacity: % 0..100 (50)} flat art as a lit, shaded solid with a material, lights and a shadow";

/// The shape of a bevel's edge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Profile {
    Straight,
    Rounded,
    Stepped,
    Cove,
}

impl Profile {
    fn parse(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "rounded" => Self::Rounded,
            "stepped" => Self::Stepped,
            "cove" => Self::Cove,
            _ => Self::Straight,
        }
    }

    /// The profile from the wall (0, 0) to the cap (1, 1), as (inset, height) fractions,
    /// `repeat` times over.
    fn points(self, repeat: usize) -> Vec<(f64, f64)> {
        let arc = |f: fn(f64) -> (f64, f64)| -> Vec<(f64, f64)> { (0..=8).map(|i| f(i as f64 / 8.0 * PI / 2.0)).collect() };
        let one = match self {
            Self::Straight => vec![(0.0, 0.0), (1.0, 1.0)],
            // Convex: rising with the wall first, then turning flat into the cap.
            Self::Rounded => arc(|a| (1.0 - a.cos(), a.sin())),
            Self::Stepped => vec![(0.0, 0.0), (0.0, 0.5), (0.5, 0.5), (0.5, 1.0), (1.0, 1.0)],
            // Concave: cut in first, then rising into the cap.
            Self::Cove => arc(|a| (a.sin(), 1.0 - a.cos())),
        };
        let n = repeat.clamp(1, 10);
        let mut out = vec![(0.0, 0.0)];
        for k in 0..n {
            for (u, v) in one.iter().skip(1) {
                out.push(((k as f64 + u) / n as f64, (k as f64 + v) / n as f64));
            }
        }
        out
    }

    /// The points of profile `id` (tests).
    #[cfg(test)]
    pub(super) fn points_of(id: &str, repeat: usize) -> Vec<(f64, f64)> {
        Self::parse(id).points(repeat)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Bevel {
    pub(super) profile: Profile,
    pub(super) width: f64,
    pub(super) height: f64,
    pub(super) repeat: usize,
    pub(super) outside: bool,
    pub(super) both: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Extrusion {
    pub(super) depth: f64,
    pub(super) cap: bool,
    /// Degrees the back is turned from the front.
    pub(super) twist: f64,
    /// The back face's scale (1: none).
    pub(super) taper: f64,
    pub(super) bevel: Option<Bevel>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Object {
    Plane,
    Extrude(Extrusion),
    Revolve { angle: f64, offset: f64, right: bool, cap: bool },
    Inflate { depth: f64, volume: f64, both: bool },
}

/// One light: its direction (toward the light, as the viewer sees it), colour, intensity and
/// softness.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Lamp {
    pub(super) dir: V3,
    color: [f64; 3],
    intensity: f64,
    softness: f64,
}

/// The lights and the material: how 3D and Materials shades a face.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Lighting {
    pub(super) lamps: Vec<Lamp>,
    ambient: f64,
    roughness: f64,
    metallic: f64,
    color: Option<[f64; 3]>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Shadow {
    below: bool,
    distance: f64,
    bounds: f64,
    softness: f64,
    opacity: f64,
}

/// A shadow ready to paint: its outline, opacity and blur radius.
pub(super) struct ShadowArt {
    pub(super) path: BezPath,
    pub(super) opacity: f64,
    pub(super) softness: f64,
}

/// A 3D and Materials effect's options, read and clamped to their ranges.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Params {
    pub(super) object: Object,
    pub(super) rotate: [f64; 3],
    pub(super) perspective: f64,
    pub(super) lighting: Lighting,
    pub(super) shadow: Option<Shadow>,
}

/// `"#rrggbb"` as RGB 0..1.
fn hex(s: &str) -> Option<[f64; 3]> {
    Color::from_hex(s).map(|c| c.to_rgb().map(|v| f64::from(v).clamp(0.0, 1.0)))
}

/// The direction toward a light `rotation`° around the view (0: from the top, 90: from the right)
/// and `height`° above the art (90: from the viewer).
fn light_dir(rotation: f64, height: f64) -> V3 {
    let (r, e) = (rotation.to_radians(), height.to_radians());
    V3::new(e.cos() * r.sin(), -e.cos() * r.cos(), -e.sin()).unit().unwrap_or(V3::new(0.0, 0.0, -1.0))
}

fn lamp(l: &Value) -> Lamp {
    Lamp {
        dir: light_dir(num(l, "rotation", -45.0).clamp(-360.0, 360.0), num(l, "height", 45.0).clamp(0.0, 90.0)),
        color: hex(text(l, "color", "#ffffff")).unwrap_or([1.0; 3]),
        intensity: num(l, "intensity", 100.0).clamp(0.0, 400.0) / 100.0,
        softness: num(l, "softness", 50.0).clamp(0.0, 100.0) / 100.0,
    }
}

impl Lighting {
    fn parse(p: &Value) -> Self {
        let preset = lighting_preset(text(p, "lighting", "standard")).or_else(|| lighting_preset("standard")).unwrap_or(Value::Null);
        let lamps = match p.get("lights").and_then(Value::as_array) {
            Some(list) => list.iter().filter(|l| l.is_object()).take(MAX_LIGHTS).map(lamp).collect(),
            None => preset["lights"].as_array().into_iter().flatten().take(MAX_LIGHTS).map(lamp).collect(),
        };
        Self {
            lamps,
            ambient: num(p, "ambient", num(&preset, "ambient", 30.0)).clamp(0.0, 200.0) / 100.0,
            roughness: num(p, "roughness", 0.45).clamp(0.0, 1.0),
            metallic: num(p, "metallic", 0.0).clamp(0.0, 1.0),
            color: p.get("materialColor").and_then(Value::as_str).and_then(hex),
        }
    }

    /// The direction toward the light that casts the shadow (the first; from the upper left
    /// without lights).
    fn key_light(&self) -> V3 {
        self.lamps.first().map_or_else(|| light_dir(-45.0, 45.0), |l| l.dir)
    }
}

impl Shader for Lighting {
    fn shade(&self, base: [f64; 3], n: V3) -> [f64; 3] {
        let base = self.color.unwrap_or(base).map(|c| if c.is_finite() { c.clamp(0.0, 1.0) } else { 0.0 });
        // Two-sided: the face's side toward the viewer.
        let n = if n.z > 0.0 { n.scale(-1.0) } else { n };
        let v = V3::new(0.0, 0.0, -1.0);
        let (r, m) = (self.roughness.clamp(0.02, 1.0), self.metallic);
        let a2 = (r * r).powi(2);
        let k = (r + 1.0).powi(2) / 8.0;
        let ndv = n.dot(v).max(1e-3);
        // Reflectance straight on: 4% for plastics, the material's colour for metals.
        let f0 = base.map(|c| 0.04 + (c - 0.04) * m);
        // Ambient: the colour lit evenly, and on metals a reflection of surroundings that are
        // brighter above than below (without it, metals lit by a few point lights look black).
        let sky = 0.45 + 0.5 * (0.5 - 0.5 * n.y);
        let env = 0.55 + 0.45 * self.ambient.min(1.0);
        let mut out = [0.0; 3];
        for i in 0..3 {
            out[i] = base[i] * self.ambient * (1.0 - m) + f0[i] * m * sky * env;
        }
        for l in &self.lamps {
            let ndl = n.dot(l.dir);
            // Soft lights wrap further around the solid.
            let w = l.softness * 0.5;
            let wrapped = ((ndl + w) / (1.0 + w)).max(0.0);
            let diffuse = wrapped * (1.0 - m);
            // Metals spread the light they reflect more widely the rougher they are.
            let glossy = wrapped * m * 0.25 * (0.5 + r);
            let mut spec = [0.0; 3];
            if ndl > 0.0
                && let Some(h) = l.dir.add(v).unit()
            {
                let ndh = n.dot(h).max(0.0);
                let vdh = v.dot(h).max(0.0);
                let d = a2 / (PI * (ndh * ndh * (a2 - 1.0) + 1.0).powi(2)).max(1e-12);
                let g = ndl / (ndl * (1.0 - k) + k) * ndv / (ndv * (1.0 - k) + k);
                let term = d * g * PI / (4.0 * ndv);
                let fresnel = (1.0 - vdh).powi(5);
                spec = f0.map(|f| (f + (1.0 - f) * fresnel) * term);
            }
            for i in 0..3 {
                out[i] += l.color[i] * l.intensity * (base[i] * diffuse + f0[i] * glossy + spec[i]);
            }
        }
        out.map(|c| if c.is_finite() { c.clamp(0.0, 1.0) } else { 0.0 })
    }
}

impl Params {
    pub(super) fn parse(p: &Value) -> Self {
        let depth = num(p, "depth", 50.0).clamp(0.0, 2000.0);
        let cap = flag(p, "cap", true);
        let pct = |key, default| num(p, key, default).clamp(0.0, 100.0) / 100.0;
        let object = match text(p, "object", "extrude").to_ascii_lowercase().as_str() {
            "plane" => Object::Plane,
            "revolve" => Object::Revolve {
                angle: num(p, "angle", 360.0).clamp(1.0, 360.0),
                offset: num(p, "offset", 0.0).clamp(0.0, 1000.0),
                right: text(p, "from", "leftEdge").eq_ignore_ascii_case("rightEdge"),
                cap,
            },
            "inflate" => Object::Inflate { depth, volume: pct("volume", 50.0), both: flag(p, "bothSides", false) },
            _ => Object::Extrude(Extrusion {
                depth,
                cap,
                twist: num(p, "twist", 0.0).clamp(-3600.0, 3600.0),
                taper: num(p, "taper", 100.0).clamp(1.0, 400.0) / 100.0,
                bevel: flag(p, "bevel", false)
                    .then(|| Bevel {
                        profile: Profile::parse(text(p, "bevelShape", "straight")),
                        width: num(p, "bevelWidth", 4.0).clamp(0.0, 500.0),
                        height: num(p, "bevelHeight", 4.0).clamp(0.0, 500.0),
                        repeat: num(p, "bevelRepeat", 1.0).round().clamp(1.0, 10.0) as usize,
                        outside: flag(p, "bevelOutside", false),
                        both: flag(p, "bevelBothSides", false),
                    })
                    .filter(|b| b.width > 0.0 && b.height > 0.0),
            }),
        };
        Self {
            object,
            rotate: [num(p, "rotateX", 15.0), num(p, "rotateY", -25.0), num(p, "rotateZ", 0.0)].map(|a| a.clamp(-180.0, 180.0)),
            perspective: num(p, "perspective", 0.0).clamp(0.0, 160.0),
            lighting: Lighting::parse(p),
            shadow: flag(p, "shadow", false).then(|| Shadow {
                below: !text(p, "shadowPosition", "below").eq_ignore_ascii_case("behind"),
                distance: num(p, "shadowDistance", 0.0).clamp(0.0, 1000.0),
                bounds: num(p, "shadowBounds", 100.0).clamp(10.0, 400.0) / 100.0,
                softness: num(p, "shadowSoftness", 4.0).clamp(0.0, 50.0),
                opacity: pct("shadowOpacity", 50.0),
            }),
        }
    }
}

/// A colour shaded by the material and lights of 3D and Materials parameters `params` (missing
/// keys take the defaults) on a surface whose normal, as the viewer sees it, is `normal` (x right,
/// y down, z away from the viewer): the panel's material and lighting previews.
pub fn preview_shade(params: &Value, base: [f64; 3], normal: [f64; 3]) -> [f64; 3] {
    let p = crate::merged_params(MATERIALS, params);
    let n = V3::new(normal[0], normal[1], normal[2]).unit().unwrap_or(V3::new(0.0, 0.0, -1.0));
    Lighting::parse(&p).shade(base, n)
}

/// The faces of the solid, back to front, and its shadow.
pub(super) fn build(p: &Params, shapes: &[Shape], bounds: Rect) -> (Vec<Face>, Option<ShadowArt>) {
    let size = bounds.width().hypot(bounds.height());
    let c = bounds.center();
    let view = match p.object {
        Object::Revolve { offset, right, .. } => {
            let axis = revolve_axis(bounds, offset, right);
            View::new(p.rotate, p.perspective, V3::new(axis, c.y, 0.0), revolve_size(bounds, axis))
        }
        Object::Extrude(Extrusion { depth, .. }) | Object::Inflate { depth, .. } => {
            View::new(p.rotate, p.perspective, V3::new(c.x, c.y, 0.0), size + depth)
        }
        Object::Plane => View::new(p.rotate, p.perspective, V3::new(c.x, c.y, 0.0), size),
    };
    let mut sink = Sink::new(&view, &p.lighting, p.shadow.is_some());
    match p.object {
        Object::Plane => {
            let lines = flattened(shapes, size, MAX_OUTLINE_POINTS);
            for (s, lines) in shapes.iter().zip(&lines) {
                sink.cap(s, 0.0, None);
                sink.cap_shadow(lines, 0.0);
            }
        }
        Object::Extrude(ex) => {
            let lines = flattened(shapes, size, MAX_FACES / ring_budget(&ex));
            for (s, lines) in shapes.iter().zip(&lines) {
                extrude(&mut sink, s, lines, &ex, c);
            }
        }
        Object::Revolve { angle, offset, right, cap } => {
            let steps = revolve_steps(angle);
            let lines = flattened(shapes, size, MAX_FACES / steps.max(1));
            for (s, lines) in shapes.iter().zip(&lines) {
                revolve(&mut sink, s, lines, revolve_axis(bounds, offset, right), angle.to_radians(), steps, cap);
            }
        }
        Object::Inflate { depth, volume, both } => {
            let sides = if both { 2 } else { 1 };
            let cells = (MAX_FACES / 2 / sides / shapes.len().max(1)).min(INFLATE_GRID * INFLATE_GRID);
            let lines = flattened(shapes, size, (INFLATE_POINTS * shapes.len()).min(MAX_OUTLINE_POINTS));
            for (s, lines) in shapes.iter().zip(&lines) {
                inflate(&mut sink, s, lines, Pillow { depth, volume, both, cells });
            }
        }
    }
    let polys = sink.shadow.take();
    let shadow = p.shadow.zip(polys).and_then(|(sh, polys)| cast_shadow(&view, p.lighting.key_light(), &polys, &sh));
    (sink.sorted(), shadow)
}

/// About how many faces each outline point of an extrusion makes (its rings), so the outlines
/// are flattened within the face budget.
fn ring_budget(ex: &Extrusion) -> usize {
    let bevel = ex.bevel.map_or(0, |b| b.profile.points(b.repeat).len() * if b.both { 2 } else { 1 });
    (twist_slices(ex.twist) + bevel + 2).max(1)
}

/// Slices the depth is cut into to follow a twist of `twist` degrees.
fn twist_slices(twist: f64) -> usize {
    ((twist.abs() / 6.0).ceil() as usize).clamp(1, MAX_TWIST_SLICES)
}

/// One cross-section of an extrusion: at depth `z`, the outline offset by `d` (outward).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Ring {
    z: f64,
    d: f64,
}

/// The cross-sections of an extrusion from front to back: the front bevel (cap to wall), the
/// wall's slices and the back bevel (wall to cap). `width` is the bevel width this shape takes.
fn rings(ex: &Extrusion, bevel: Option<Bevel>) -> Vec<Ring> {
    let (front, back) = (-ex.depth / 2.0, ex.depth / 2.0);
    let base = bevel.map_or(0.0, |b| if b.outside { b.width } else { 0.0 });
    let (hf, hb) = match bevel {
        Some(b) if b.both => (b.height.min(ex.depth / 2.0), b.height.min(ex.depth / 2.0)),
        Some(b) => (b.height.min(ex.depth), 0.0),
        None => (0.0, 0.0),
    };
    let mut out = vec![];
    match bevel {
        Some(b) => out.extend(b.profile.points(b.repeat).iter().rev().map(|(u, v)| Ring { z: front + hf * (1.0 - v), d: base - b.width * u })),
        None => out.push(Ring { z: front, d: base }),
    }
    let (z0, z1) = (front + hf, back - hb);
    let slices = twist_slices(ex.twist);
    out.extend((1..=slices).map(|k| Ring { z: z0 + (z1 - z0) * k as f64 / slices as f64, d: base }));
    if let Some(b) = bevel.filter(|b| b.both) {
        out.extend(b.profile.points(b.repeat).iter().map(|(u, v)| Ring { z: back - hb * (1.0 - v), d: base - b.width * u }));
    }
    out.dedup_by(|a, b| (a.z - b.z).abs() < 1e-9 && (a.d - b.d).abs() < 1e-9);
    out
}

/// The outward normals (away from the shape's filled region) at the corners of each closed
/// outline of `lines`, mitred (at most about three times longer at sharp corners); zero for open
/// lines and unfilled shapes.
fn corner_normals(s: &Shape, lines: &[(Vec<Point>, bool)]) -> Vec<Vec<Vec2>> {
    let region = outline_path(lines);
    let inside = |p: Point| {
        let w = kurbo::Shape::winding(&region, p);
        match s.rule {
            FillRule::NonZero => w != 0,
            FillRule::EvenOdd => w % 2 != 0,
        }
    };
    lines
        .iter()
        .map(|(line, closed)| {
            let n = line.len();
            if !(s.filled || *closed) || n < 3 {
                return vec![Vec2::ZERO; n];
            }
            let edge = |i: usize| -> (Vec2, f64) {
                let (Some(a), Some(b)) = (line.get(i), line.get((i + 1) % n)) else { return (Vec2::ZERO, 0.0) };
                let d = *b - *a;
                let len = d.hypot();
                if len > 1e-12 { (Vec2::new(d.y, -d.x) / len, len) } else { (Vec2::ZERO, 0.0) }
            };
            let edges: Vec<(Vec2, f64)> = (0..n).map(edge).collect();
            // Which side is outside: a step off the longest edge's middle.
            let sign = edges
                .iter()
                .enumerate()
                .max_by(|a, b| a.1.1.total_cmp(&b.1.1))
                .and_then(|(i, (e, len))| {
                    let (a, b) = (line.get(i)?, line.get((i + 1) % n)?);
                    let probe = a.midpoint(*b) + *e * (len * 0.01).max(1e-6);
                    Some(if inside(probe) { -1.0 } else { 1.0 })
                })
                .unwrap_or(1.0);
            (0..n)
                .map(|i| {
                    let (prev, cur) = (edges.get((i + n - 1) % n).map_or(Vec2::ZERO, |e| e.0), edges.get(i).map_or(Vec2::ZERO, |e| e.0));
                    let sum = prev + cur;
                    let m = if sum.hypot() < 1e-9 { cur } else { sum.normalize() };
                    let cos = m.dot(cur).max(0.35);
                    m * (sign / cos)
                })
                .collect()
        })
        .collect()
}

/// `lines` as one path of closed polygons.
fn outline_path(lines: &[(Vec<Point>, bool)]) -> BezPath {
    let mut bp = BezPath::new();
    for (line, _) in lines {
        let mut pts = line.iter();
        if let Some(first) = pts.next() {
            bp.move_to(*first);
            for p in pts {
                bp.line_to(*p);
            }
            bp.close_path();
        }
    }
    bp
}

fn extrude(sink: &mut Sink, s: &Shape, lines: &[(Vec<Point>, bool)], ex: &Extrusion, centre: Point) {
    if ex.depth <= 0.0 {
        if s.filled {
            sink.cap(s, 0.0, None);
            sink.cap_shadow(lines, 0.0);
        }
        return;
    }
    // A bevel at most nearly half the shape's narrower side wide (filled shapes only).
    let bevel = ex.bevel.filter(|_| s.filled).and_then(|b| {
        let r = rect_of(&s.path)?;
        let width = b.width.min(0.45 * r.width().min(r.height()));
        (width > 1e-6).then_some(Bevel { width, ..b })
    });
    let rings = rings(ex, bevel);
    let normals = corner_normals(s, lines);
    let front = -ex.depth / 2.0;
    let twist = ex.twist.to_radians();
    let place = |p: Point, n: Vec2, r: &Ring| {
        let t = ((r.z - front) / ex.depth).clamp(0.0, 1.0);
        let q = p + n * r.d;
        let scale = 1.0 + (ex.taper - 1.0) * t;
        let (sin, cos) = (twist * t).sin_cos();
        let (x, y) = ((q.x - centre.x) * scale, (q.y - centre.y) * scale);
        V3::new(centre.x + x * cos - y * sin, centre.y + x * sin + y * cos, r.z)
    };
    // Each outline at each ring.
    let mut first_loops = vec![];
    let mut last_loops = vec![];
    for ((line, closed), normals) in lines.iter().zip(&normals) {
        let closed = *closed || s.filled;
        let n = line.len();
        let at = |r: &Ring| -> Vec<V3> { line.iter().zip(normals).map(|(p, nrm)| place(*p, *nrm, r)).collect() };
        let mut prev: Option<Vec<V3>> = None;
        for r in &rings {
            let cur = at(r);
            if let Some(a) = &prev {
                let edges = if closed { n } else { n.saturating_sub(1) };
                for i in 0..edges {
                    let j = (i + 1) % n.max(1);
                    let (Some(a0), Some(a1), Some(b1), Some(b0)) = (a.get(i), a.get(j), cur.get(j), cur.get(i)) else { continue };
                    sink.polygon(&[*a0, *a1, *b1, *b0], s.color);
                }
            } else {
                first_loops.push(cur.clone());
            }
            prev = Some(cur);
        }
        last_loops.extend(prev);
    }
    if !(s.filled && ex.cap) {
        return;
    }
    let plain = bevel.is_none() && ex.twist == 0.0 && (ex.taper - 1.0).abs() < 1e-12;
    if plain {
        // The shape itself, curves kept.
        sink.cap(s, front, Some(V3::new(0.0, 0.0, -1.0)));
        sink.cap(s, -front, Some(V3::new(0.0, 0.0, 1.0)));
        sink.cap_shadow(lines, front);
    } else {
        flat_cap(sink, &first_loops, s, V3::new(0.0, 0.0, -1.0));
        flat_cap(sink, &last_loops, s, V3::new(0.0, 0.0, 1.0));
    }
}

/// A flat face of polygons `loops` (corners in the art's space, one depth) filled with the
/// shape's rule, when its outward normal (`facing`) points at the viewer; drawn in front of the
/// rest of its solid.
fn flat_cap(sink: &mut Sink, loops: &[Vec<V3>], s: &Shape, facing: V3) {
    if let Some(sh) = &mut sink.shadow
        && sh.len() < MAX_FACES
    {
        sh.extend(loops.iter().cloned());
    }
    let normal = sink.view.turn_dir(facing);
    if normal.z >= 0.0 {
        return;
    }
    let mut path = BezPath::new();
    let mut near = f64::INFINITY;
    for l in loops {
        for (i, q) in l.iter().enumerate() {
            let t = sink.view.turn(*q);
            near = near.min(t.z);
            let p = sink.view.project(t);
            if i == 0 { path.move_to(p) } else { path.line_to(p) }
        }
        if !l.is_empty() {
            path.close_path();
        }
    }
    if near.is_finite() && !sink.full() {
        sink.faces.push(Face { path, rule: s.rule, depth: near - 1e-3, color: sink.shader.shade(s.color, normal) });
    }
}

/// How an inflated shape puffs up: its depth (at the point farthest from its edges), volume
/// (0..1: from a ridged tent to a full, flat-topped pillow), both faces or the front only, and the
/// most grid cells it is sampled with.
#[derive(Clone, Copy, Debug)]
struct Pillow {
    depth: f64,
    volume: f64,
    both: bool,
    cells: usize,
}

/// A grid point of an inflated shape: where it is, how far from the edge (0 on it) and whether
/// it is part of the surface.
#[derive(Clone, Copy)]
struct Sample {
    at: Point,
    dist: f64,
    used: bool,
    edge: bool,
}

fn inflate(sink: &mut Sink, s: &Shape, lines: &[(Vec<Point>, bool)], pillow: Pillow) {
    let flat = |sink: &mut Sink| {
        sink.cap(s, 0.0, None);
        sink.cap_shadow(lines, 0.0);
    };
    let side = (pillow.cells as f64).sqrt().floor() as usize;
    let Some(b) = rect_of(&s.path) else { return };
    let long = b.width().max(b.height());
    if !s.filled || pillow.depth <= 0.0 || side < 2 || long.is_nan() || long <= 1e-9 {
        return flat(sink);
    }
    let cell = long / side.min(INFLATE_GRID) as f64;
    let (nx, ny) = (((b.width() / cell).ceil() as usize).clamp(1, INFLATE_GRID), ((b.height() / cell).ceil() as usize).clamp(1, INFLATE_GRID));
    let region = outline_path(lines);
    let inside = |p: Point| {
        let w = kurbo::Shape::winding(&region, p);
        match s.rule {
            FillRule::NonZero => w != 0,
            FillRule::EvenOdd => w % 2 != 0,
        }
    };
    let segments: Vec<(Point, Point)> = lines
        .iter()
        .flat_map(|(l, _)| l.iter().zip(l.iter().skip(1).chain(l.first())).map(|(a, b)| (*a, *b)))
        .filter(|(a, b)| (*a - *b).hypot() > 1e-12)
        .collect();
    // The nearest point of the outline to `p`, and how far it is.
    let nearest = |p: Point| {
        segments.iter().fold((f64::INFINITY, p), |best, (a, b)| {
            let d = *b - *a;
            let t = ((p - *a).dot(d) / d.hypot2()).clamp(0.0, 1.0);
            let q = *a + d * t;
            let dist = (p - q).hypot();
            if dist < best.0 { (dist, q) } else { best }
        })
    };
    let (w, h) = (nx + 1, ny + 1);
    let mut grid: Vec<Sample> = (0..w * h)
        .map(|k| {
            let at = Point::new(b.x0 + (k % w) as f64 * cell, b.y0 + (k / w) as f64 * cell);
            let used = inside(at);
            Sample { at, dist: if used { nearest(at).0 } else { 0.0 }, used, edge: false }
        })
        .collect();
    // Points just outside an inside point move onto the outline, where the surface meets it.
    let inner: Vec<bool> = grid.iter().map(|g| g.used).collect();
    for k in 0..w * h {
        if inner.get(k).copied().unwrap_or(true) {
            continue;
        }
        let (x, y) = ((k % w) as isize, (k / w) as isize);
        let near_inside = (-1..=1).any(|dy| {
            (-1..=1).any(|dx| {
                let (i, j) = (x + dx, y + dy);
                i >= 0 && j >= 0 && (i as usize) < w && (j as usize) < h && inner.get(j as usize * w + i as usize).copied().unwrap_or(false)
            })
        });
        if near_inside && let Some(g) = grid.get_mut(k) {
            g.at = nearest(g.at).1;
            (g.used, g.edge, g.dist) = (true, true, 0.0);
        }
    }
    let far = grid.iter().filter(|g| g.used).map(|g| g.dist).fold(0.0, f64::max);
    if far.is_nan() || far <= 1e-9 {
        return flat(sink);
    }
    let k = 1.0 + 4.0 * pillow.volume;
    let height = |g: &Sample| pillow.depth * (1.0 - (1.0 - (g.dist / far).clamp(0.0, 1.0)).powf(k));
    let point = |g: &Sample, sign: f64| V3::new(g.at.x, g.at.y, sign * height(g));
    for y in 0..ny {
        for x in 0..nx {
            let corners = [y * w + x, y * w + x + 1, (y + 1) * w + x + 1, (y + 1) * w + x];
            let used: Vec<Sample> = corners.iter().filter_map(|k| grid.get(*k)).filter(|g| g.used).copied().collect();
            // A cell whose corners are all on the outline lies flat in it: nothing to draw.
            if used.len() < 3 || used.iter().all(|g| g.edge) {
                continue;
            }
            let tris: Vec<[Sample; 3]> = match used[..] {
                [a, b, c, d] => vec![[a, b, c], [a, c, d]],
                [a, b, c] => vec![[a, b, c]],
                _ => vec![],
            };
            for t in tris {
                sink.polygon(&t.map(|g| point(&g, -1.0)), s.color);
                if pillow.both {
                    sink.polygon(&t.map(|g| point(&g, 1.0)), s.color);
                }
            }
        }
    }
    if !pillow.both {
        // The flat back.
        sink.cap(s, 0.0, Some(V3::new(0.0, 0.0, 1.0)));
    }
}

/// The shadow of the solid whose faces are `polys` (corners in the art's space), cast by a light
/// in direction `light` (as the viewer sees it) onto a plane below the solid (its own floor, the
/// art's bottom) or behind it, `distance` away; scaled by its bounds about its centre.
fn cast_shadow(view: &View, light: V3, polys: &[Vec<V3>], sh: &Shadow) -> Option<ShadowArt> {
    // From the light, through the solid.
    let mut dir = view.unturn_dir(light).scale(-1.0);
    let pts = polys.iter().flatten();
    let plane = if sh.below {
        dir.y = dir.y.max(0.05);
        pts.map(|p| p.y).fold(f64::NEG_INFINITY, f64::max) + sh.distance
    } else {
        dir.z = dir.z.max(0.05);
        pts.map(|p| p.z).fold(f64::NEG_INFINITY, f64::max) + sh.distance
    };
    if !plane.is_finite() {
        return None;
    }
    let cast = |p: &V3| {
        let t = if sh.below { (plane - p.y) / dir.y } else { (plane - p.z) / dir.z };
        p.add(dir.scale(t.max(0.0)))
    };
    let cast: Vec<Vec<V3>> = polys.iter().map(|poly| poly.iter().map(cast).collect()).collect();
    let (lo, hi) = cast.iter().flatten().fold(
        (V3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY), V3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY)),
        |(lo, hi), p| (V3::new(lo.x.min(p.x), lo.y.min(p.y), lo.z.min(p.z)), V3::new(hi.x.max(p.x), hi.y.max(p.y), hi.z.max(p.z))),
    );
    let mid = lo.add(hi).scale(0.5);
    if ![mid.x, mid.y, mid.z].iter().all(|v| v.is_finite()) {
        return None;
    }
    let mut path = BezPath::new();
    for poly in &cast {
        let pts: Vec<Point> = poly.iter().map(|p| view.project(view.turn(mid.add(p.sub(mid).scale(sh.bounds))))).collect();
        if pts.len() < 3 {
            continue;
        }
        // Every piece wound the same way, so the non-zero fill is their union.
        let area: f64 = pts.iter().zip(pts.iter().cycle().skip(1)).map(|(a, b)| a.x * b.y - b.x * a.y).sum();
        if area.is_nan() || area.abs() <= 1e-9 {
            continue;
        }
        let mut it: Box<dyn Iterator<Item = &Point>> = if area > 0.0 { Box::new(pts.iter()) } else { Box::new(pts.iter().rev()) };
        if let Some(first) = it.next() {
            path.move_to(*first);
            for p in it {
                path.line_to(*p);
            }
            path.close_path();
        }
    }
    (!path.elements().is_empty()).then_some(ShadowArt { path, opacity: sh.opacity, softness: sh.softness })
}
