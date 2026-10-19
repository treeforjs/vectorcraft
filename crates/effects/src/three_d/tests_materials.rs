//! 3D and Materials: object types, bevels, twist and taper, inflate, materials, lights, shadows
//! and hostile parameters.

use serde_json::{Value, json};
use vectorcraft_doc::{Appearance, Effect, Node, NodeId};
use vectorcraft_geom::{Rect, shapes};

use super::materials::{Lighting, Object, Params, Profile};
use super::*;

/// A red `w` × `h` rectangle at the origin with 3D and Materials `params`.
fn rect(w: f64, h: f64, params: Value) -> Node {
    let mut n = Node::path(
        NodeId(7),
        shapes::rectangle(Rect::new(0.0, 0.0, w, h)),
        Appearance::basic(Paint::solid(Color::rgb(1.0, 0.0, 0.0)), Paint::None, 0.0),
    );
    n.appearance.effects.push(Effect { id: MATERIALS.into(), params, visible: true });
    n
}

fn art(n: &Node) -> Node {
    three_d_art(n).unwrap()
}

fn faces(art: &Node) -> Vec<&Node> {
    art.children().unwrap().iter().map(|c| &**c).collect()
}

/// The nearest face (painted last).
fn last(art: &Node) -> Rect {
    art.children().unwrap().last().unwrap().geometric_bounds().unwrap()
}

fn near(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

const FRONT: [(&str, f64); 3] = [("rotateX", 0.0), ("rotateY", 0.0), ("rotateZ", 0.0)];

/// `params` seen straight from the front (or with `rotateY`).
fn front(mut params: Value, rotate_y: f64) -> Value {
    for (k, v) in FRONT {
        params[k] = json!(v);
    }
    params["rotateY"] = json!(rotate_y);
    params
}

#[test]
fn params_take_defaults_and_stay_in_range() {
    let p = Params::parse(&crate::merged_params(MATERIALS, &json!({})));
    assert_eq!((p.rotate, p.perspective, p.shadow), ([15.0, -25.0, 0.0], 0.0, None));
    let Object::Extrude(ex) = p.object else { panic!("extrude by default: {:?}", p.object) };
    assert_eq!((ex.depth, ex.cap, ex.twist, ex.taper, ex.bevel), (50.0, true, 0.0, 1.0, None));
    assert_eq!(p.lighting.lamps.len(), 1, "the Standard lighting");
    let p = Params::parse(
        &json!({"depth": 1e12, "taper": -5, "twist": 1e9, "rotateX": -1e9, "perspective": "x", "lights": [{}, {}, {}, {}, {}, {}], "bevel": true, "bevelRepeat": 1e9}),
    );
    let Object::Extrude(ex) = p.object else { panic!() };
    assert_eq!((ex.depth, ex.taper, ex.twist, p.rotate[0], p.perspective), (2000.0, 0.01, 3600.0, -180.0, 0.0));
    assert_eq!(ex.bevel.unwrap().repeat, 10);
    assert_eq!(p.lighting.lamps.len(), MAX_LIGHTS, "at most four lights");
    assert!(matches!(Params::parse(&json!({"object": "INFLATE"})).object, Object::Inflate { .. }));
    assert!(matches!(Params::parse(&json!({"object": "plane"})).object, Object::Plane));
    assert!(matches!(Params::parse(&json!({"object": "revolve", "angle": 0})).object, Object::Revolve { angle: 1.0, .. }));
    assert!(is_three_d(MATERIALS) && crate::effect_info(MATERIALS).is_some_and(|e| e.menu == ["Effect", "3D and Materials"]));
}

/// Presets write the parameters they stand for, and unknown ones nothing.
#[test]
fn presets_set_their_params() {
    assert_eq!(rotation_preset("back").unwrap(), json!({"rotation": "back", "rotateX": 0.0, "rotateY": 180.0, "rotateZ": 0.0}));
    assert_eq!(material_preset("gold").unwrap()["metallic"], json!(1.0));
    assert_eq!(material_preset("default").unwrap()["materialColor"], Value::Null);
    let diffuse = lighting_preset("diffuse").unwrap();
    assert_eq!(diffuse["lights"].as_array().unwrap().len(), 2);
    for id in ["nope", ""] {
        assert!(rotation_preset(id).is_none() && material_preset(id).is_none() && lighting_preset(id).is_none());
    }
    for (id, ..) in ROTATION_PRESETS {
        assert!(rotation_preset(id).is_some());
    }
    for (id, ..) in LIGHTING_PRESETS {
        assert!(lighting_preset(id).is_some());
    }
}

/// The isometric views foreshorten the art's three axes alike.
#[test]
fn isometric_presets_foreshorten_the_axes_alike() {
    for id in ["isometricLeft", "isometricRight"] {
        let [x, y, z] = ROTATION_PRESETS.iter().find(|r| r.0 == id).unwrap().2;
        let view = View::new([x, y, z], 0.0, V3::new(0.0, 0.0, 0.0), 1.0);
        let lens: Vec<f64> = [V3::new(1.0, 0.0, 0.0), V3::new(0.0, 1.0, 0.0), V3::new(0.0, 0.0, 1.0)]
            .map(|a| {
                let t = view.turn(a);
                t.x.hypot(t.y)
            })
            .to_vec();
        assert!(lens.iter().all(|l| near(*l, (2.0f64 / 3.0).sqrt(), 1e-3)), "{id}: {lens:?}");
    }
}

/// Plane: the flat art turned in space, one face, edge-on from the side.
#[test]
fn plane_is_flat() {
    let a = art(&rect(100.0, 50.0, front(json!({"object": "plane"}), 0.0)));
    assert_eq!(faces(&a).len(), 1);
    let b = last(&a);
    assert!(near(b.width(), 100.0, 1e-6) && near(b.height(), 50.0, 1e-6), "{b:?}");
    let side = art(&rect(100.0, 50.0, front(json!({"object": "plane"}), 90.0)));
    assert!(last(&side).width() < 1e-6, "edge-on: {:?}", last(&side));
}

/// Twist turns the back face about the centre: a 100 × 50 back turned a quarter is 50 × 100.
#[test]
fn twist_turns_the_back_face() {
    let plain = art(&rect(100.0, 50.0, front(json!({"depth": 40}), 180.0)));
    let b = last(&plain);
    assert!(near(b.width(), 100.0, 1e-6) && near(b.height(), 50.0, 1e-6), "seen from the back: {b:?}");
    let twisted = art(&rect(100.0, 50.0, front(json!({"depth": 40, "twist": 90}), 180.0)));
    let b = last(&twisted);
    assert!(near(b.width(), 50.0, 1e-6) && near(b.height(), 100.0, 1e-6), "{b:?}");
    assert!(near(b.center().x, 50.0, 1e-6) && near(b.center().y, 25.0, 1e-6), "about the centre: {b:?}");
    // Sliced to follow the twist: 15 slices of 6°, four walls each, and the two caps' worth.
    assert!(faces(&twisted).len() >= 15 * 4, "{}", faces(&twisted).len());
}

/// Taper scales the back face about the centre; the front keeps its size.
#[test]
fn taper_shrinks_the_back_face() {
    let back = last(&art(&rect(100.0, 50.0, front(json!({"depth": 40, "taper": 50}), 180.0))));
    assert!(near(back.width(), 50.0, 1e-6) && near(back.height(), 25.0, 1e-6) && near(back.center().x, 50.0, 1e-6), "{back:?}");
    let face = last(&art(&rect(100.0, 50.0, front(json!({"depth": 40, "taper": 50}), 0.0))));
    assert!(near(face.width(), 100.0, 1e-6), "{face:?}");
}

/// A bevel cuts into the front edge: the front face shrinks by the bevel width and the bevel's
/// faces are added; outside, the front keeps its size and the solid grows around it. Rounded and
/// repeated profiles add more faces.
#[test]
fn bevel_insets_the_front_and_adds_faces() {
    let plain = art(&rect(100.0, 100.0, front(json!({"depth": 40}), 0.0)));
    let bevel = art(&rect(100.0, 100.0, front(json!({"depth": 40, "bevel": true, "bevelWidth": 10, "bevelHeight": 10}), 0.0)));
    let f = last(&bevel);
    assert!(near(f.width(), 80.0, 1e-6) && near(f.x0, 10.0, 1e-6), "{f:?}");
    assert!(faces(&bevel).len() > faces(&plain).len(), "{} vs {}", faces(&bevel).len(), faces(&plain).len());
    // Seen from the front the bevel's slope shows around the face: the whole solid stays 100 wide.
    assert!(near(bevel.geometric_bounds().unwrap().width(), 100.0, 1e-6));
    let out = art(&rect(100.0, 100.0, front(json!({"depth": 40, "bevel": true, "bevelWidth": 10, "bevelOutside": true}), 0.0)));
    assert!(near(last(&out).width(), 100.0, 1e-6) && near(out.geometric_bounds().unwrap().width(), 120.0, 1e-6));
    let count = |params: Value| faces(&art(&rect(100.0, 100.0, params))).len();
    let straight = count(json!({"bevel": true}));
    assert!(count(json!({"bevel": true, "bevelShape": "rounded"})) > straight);
    assert!(count(json!({"bevel": true, "bevelRepeat": 3})) > straight);
    assert!(count(json!({"bevel": true, "bevelBothSides": true, "rotateY": 120})) > straight);
    // Every profile runs from the wall to the cap.
    for (id, _) in BEVEL_PROFILES {
        let pts = Profile::points_of(id, 2);
        assert_eq!((pts.first(), pts.last()), (Some(&(0.0, 0.0)), Some(&(1.0, 1.0))), "{id}");
    }
}

/// Inflate: a domed pillow whose middle is nearest the viewer, flat-backed unless both sides.
#[test]
fn inflate_domes_with_the_centre_nearest() {
    let a = art(&rect(100.0, 60.0, front(json!({"object": "inflate", "depth": 30}), 0.0)));
    let fs = faces(&a);
    assert!(fs.len() > 100, "{}", fs.len());
    let b = a.geometric_bounds().unwrap();
    assert!(near(b.width(), 100.0, 1e-6) && near(b.height(), 60.0, 1e-6), "the outline stays: {b:?}");
    // The middle of a rectangle is a ridge along its length, of a square its centre.
    let nearest = last(&a).center();
    assert!(near(nearest.y, 30.0, 4.0) && near(nearest.x, 50.0, 21.0), "{nearest:?}");
    let square = art(&rect(100.0, 100.0, front(json!({"object": "inflate", "depth": 30}), 0.0)));
    let nearest = last(&square).center();
    assert!(near(nearest.x, 50.0, 4.0) && near(nearest.y, 50.0, 4.0), "{nearest:?}");
    // From the side the dome stands out of the flat back by its depth.
    let side = art(&rect(100.0, 60.0, front(json!({"object": "inflate", "depth": 30}), 90.0))).geometric_bounds().unwrap();
    assert!(near(side.width(), 30.0, 1.0), "{side:?}");
    let both = art(&rect(100.0, 60.0, front(json!({"object": "inflate", "depth": 30, "bothSides": true}), 90.0))).geometric_bounds().unwrap();
    assert!(near(both.width(), 60.0, 1.0), "{both:?}");
    // More volume: fuller (near the edge it already stands higher out of the flat back, which
    // is at x = 50 seen from the side).
    let height_at = |volume: f64| {
        let side = art(&rect(100.0, 60.0, front(json!({"object": "inflate", "depth": 30, "volume": volume}), 90.0)));
        50.0 - faces(&side).iter().filter_map(|f| f.geometric_bounds()).filter(|r| r.center().y < 8.0).map(|r| r.x0).fold(f64::INFINITY, f64::min)
    };
    assert!(height_at(100.0) > height_at(0.0), "{} vs {}", height_at(100.0), height_at(0.0));
}

/// Revolve sweeps the art around its left edge, as 3D (Classic) does.
#[test]
fn revolve_sweeps_around_the_edge() {
    let a = art(&rect(100.0, 100.0, front(json!({"object": "revolve"}), 0.0)));
    let b = a.geometric_bounds().unwrap();
    assert!(near(b.x0, -100.0, 1.0) && near(b.x1, 100.0, 1.0), "{b:?}");
}

fn lighting(params: Value) -> Lighting {
    Params::parse(&crate::merged_params(MATERIALS, &params)).lighting
}

fn lum(c: [f64; 3]) -> f64 {
    c.iter().sum::<f64>() / 3.0
}

/// The normal that mirrors the first light toward the viewer (the highlight's middle).
fn highlight_normal(l: &Lighting) -> V3 {
    l.lamps[0].dir.add(V3::new(0.0, 0.0, -1.0)).unit().unwrap()
}

/// A normal `deg` degrees from `n`, tilted toward +x.
fn tilted(n: V3, deg: f64) -> V3 {
    let (s, c) = deg.to_radians().sin_cos();
    let side = n.cross(V3::new(0.0, 1.0, 0.0)).unit().unwrap();
    n.scale(c).add(side.scale(s)).unit().unwrap()
}

/// A smooth material has a small, bright highlight; a rough one a wide, dim one.
#[test]
fn roughness_widens_and_dims_the_highlight() {
    let base = [0.2, 0.2, 0.2];
    let smooth = lighting(json!({"roughness": 0.1}));
    let rough = lighting(json!({"roughness": 0.7}));
    let h = highlight_normal(&smooth);
    assert!(lum(smooth.shade(base, h)) > lum(rough.shade(base, h)) + 0.3, "{:?} vs {:?}", smooth.shade(base, h), rough.shade(base, h));
    let off = tilted(h, 25.0);
    let spec = |l: &Lighting, n: V3| lum(l.shade(base, n)) - lum(lighting(json!({"roughness": 1.0, "metallic": 0.0})).shade(base, n));
    assert!(
        spec(&rough, off) > spec(&smooth, off),
        "25° off the highlight the rough one still shines: {} vs {}",
        spec(&rough, off),
        spec(&smooth, off)
    );
}

/// A plastic's highlight is white; a metal's takes the colour, and its diffuse colour darkens.
#[test]
fn metallic_tints_the_highlight_and_darkens_the_diffuse() {
    let red = [1.0, 0.0, 0.0];
    let plastic = lighting(json!({"roughness": 0.25, "metallic": 0}));
    let metal = lighting(json!({"roughness": 0.25, "metallic": 1}));
    let h = highlight_normal(&plastic);
    let [_, g, b] = plastic.shade(red, h);
    assert!(g > 0.5 && b > 0.5, "white highlight: {:?}", plastic.shade(red, h));
    let [r, g, b] = metal.shade(red, h);
    assert!(r > 0.8 && g < 0.2 && b < 0.2, "red highlight: {:?}", metal.shade(red, h));
    // Away from the highlight, facing the light: the plastic's red is lit, the metal's dark.
    let lit = tilted(h, -40.0);
    assert!(plastic.shade(red, lit)[0] > metal.shade(red, lit)[0] + 0.2, "{:?} vs {:?}", plastic.shade(red, lit), metal.shade(red, lit));
    // A material colour replaces the art's.
    let gold = lighting(material_preset("gold").unwrap());
    let c = gold.shade([0.0, 0.0, 1.0], lit);
    assert!(c[0] > c[2] && c[1] > c[2], "{c:?}");
}

/// The lit side follows the light around the view; no light leaves the ambient only.
#[test]
fn light_rotation_moves_the_lit_side() {
    let at = |rotation: f64| {
        let l = lighting(json!({"lights": [{"rotation": rotation, "height": 20, "softness": 0}], "roughness": 1}));
        let left = lum(l.shade([0.8; 3], V3::new(-0.7, 0.0, -0.7).unit().unwrap()));
        let right = lum(l.shade([0.8; 3], V3::new(0.7, 0.0, -0.7).unit().unwrap()));
        (left, right)
    };
    let (l, r) = at(-90.0);
    assert!(l > r + 0.2, "from the left: {l} vs {r}");
    let (l, r) = at(90.0);
    assert!(r > l + 0.2, "from the right: {l} vs {r}");
    let dark = lighting(json!({"lights": [], "ambient": 0}));
    assert_eq!(dark.shade([0.8; 3], V3::new(0.0, 0.0, -1.0)), [0.0; 3]);
    // On the art: lit from the right, the right half of an inflated pillow is the brighter.
    let pillow = |rotation: f64| {
        let a = art(&rect(100.0, 100.0, front(json!({"object": "inflate", "depth": 40, "lights": [{"rotation": rotation, "height": 30}]}), 0.0)));
        let side = |right: bool| {
            let v: Vec<f64> = faces(&a)
                .iter()
                .filter(|f| f.geometric_bounds().is_some_and(|b| if right { b.x0 > 60.0 } else { b.x1 < 40.0 }))
                .map(|f| f64::from(f.appearance.fill_paint().color().unwrap().to_rgb()[0]))
                .collect();
            v.iter().sum::<f64>() / v.len().max(1) as f64
        };
        (side(false), side(true))
    };
    let (l, r) = pillow(90.0);
    assert!(r > l, "{l} vs {r}");
    let (l, r) = pillow(-90.0);
    assert!(l > r, "{l} vs {r}");
}

/// Shadows: translucent, blurred, painted first; below the object they fall under its bottom,
/// behind it they shift away from the light.
#[test]
fn shadows_fall_below_or_behind() {
    let params = |pos: &str| json!({"depth": 40, "shadow": true, "shadowPosition": pos, "rotateX": 20, "rotateY": -20, "rotateZ": 0});
    let none = art(&rect(100.0, 100.0, json!({"depth": 40})));
    let below = art(&rect(100.0, 100.0, params("below")));
    assert_eq!(faces(&below).len(), faces(&none).len() + 1);
    let shadow = faces(&below)[0];
    assert!(shadow.opacity < 1.0 && shadow.appearance.effects.iter().any(|e| e.id == "blur.gaussian"));
    let solid = none.geometric_bounds().unwrap();
    let sb = shadow.geometric_bounds().unwrap();
    assert!(sb.y1 > solid.y1 - 1.0 && sb.center().y > solid.center().y, "below: {sb:?} vs {solid:?}");
    // Behind, lit from the upper left: down and to the right of the solid.
    let behind = art(&rect(
        100.0,
        100.0,
        json!({"depth": 40, "shadow": true, "shadowPosition": "behind", "rotateX": 0, "rotateY": 0, "rotateZ": 0, "shadowDistance": 20}),
    ));
    let sb = faces(&behind)[0].geometric_bounds().unwrap();
    assert!(sb.center().x > 50.0 + 1.0 && sb.center().y > 50.0 + 1.0, "{sb:?}");
    // Its bounds scale it about its centre.
    let big = art(&rect(
        100.0,
        100.0,
        json!({"depth": 40, "shadow": true, "shadowPosition": "behind", "rotateX": 0, "rotateY": 0, "rotateZ": 0, "shadowDistance": 20, "shadowBounds": 200}),
    ));
    assert!(faces(&big)[0].geometric_bounds().unwrap().width() > sb.width() * 1.8);
    // Every object type casts one.
    for object in ["plane", "revolve", "inflate"] {
        let a = art(&rect(100.0, 100.0, json!({"object": object, "shadow": true, "shadowSoftness": 0})));
        let s = faces(&a)[0];
        assert!(s.appearance.effects.is_empty() && s.opacity < 1.0, "{object}");
    }
}

/// Hostile numbers, wrong types and empty art never panic; the solid is drawn from what makes sense.
#[test]
fn hostile_params_never_panic() {
    let hostile = [
        json!({"depth": f64::MAX, "twist": f64::MIN, "taper": 1e308, "bevel": true, "bevelWidth": 1e308, "bevelHeight": -1, "bevelRepeat": -3, "perspective": 160, "rotateX": 90}),
        json!({"object": "inflate", "depth": 1e308, "volume": "lots", "bothSides": "yes", "shadow": true, "shadowDistance": 1e308, "shadowBounds": 0}),
        json!({"object": "revolve", "angle": -1e308, "offset": 1e308, "from": 3, "shadow": 1, "shadowPosition": "behind", "lights": [{"rotation": "x", "height": 1e308, "color": "#zz"}]}),
        json!({"object": 7, "lights": "none", "ambient": -1e308, "roughness": "rough", "metallic": 1e308, "materialColor": 5}),
        json!({"object": "plane", "lights": [null, 1, [], {"intensity": -5}], "shadow": true, "shadowPosition": 9}),
        json!({"bevel": true, "bevelShape": "stepped", "bevelRepeat": 10, "bevelBothSides": true, "twist": 3600, "depth": 2000}),
        json!(null),
        json!([]),
    ];
    for params in hostile {
        for (w, h) in [(100.0, 100.0), (1e-9, 1e-9), (1e9, 1.0), (0.0, 50.0)] {
            if let Some(a) = three_d_art(&rect(w, h, params.clone())) {
                assert!(faces(&a).len() <= MAX_FACES + 8, "{params}: {}", faces(&a).len());
                assert!(a.geometric_bounds().is_none_or(|b| b.is_finite()), "{params}");
            }
        }
    }
    let mut empty = Node::path(NodeId(1), PathData::default(), Appearance::default());
    empty.appearance.effects.push(Effect { id: MATERIALS.into(), params: json!({}), visible: true });
    assert!(three_d_art(&empty).is_none());
    let _ = preview_shade(&json!({"roughness": f64::MAX, "lights": [{"color": null}]}), [f64::NAN, 2.0, -1.0], [0.0, 0.0, 0.0]);
}

/// Many shapes and fine detail stay within the face budget; the same object is built once.
#[test]
fn the_face_budget_holds_and_solids_are_reused() {
    let mut bp = BezPath::new();
    for i in 0..400 {
        let x = (i % 20) as f64 * 10.0;
        let y = (i / 20) as f64 * 10.0;
        bp.extend(kurbo::Shape::path_elements(&kurbo::Circle::new((x, y), 4.0), 0.01));
    }
    for object in ["extrude", "inflate", "revolve"] {
        let mut n = Node::path(NodeId(9), PathData::from_bezpath(&bp), Appearance::basic(Paint::solid(Color::rgb(0.2, 0.4, 0.8)), Paint::None, 0.0));
        n.appearance.effects.push(Effect {
            id: MATERIALS.into(),
            params: json!({"object": object, "twist": 360, "bevel": true, "bevelShape": "rounded", "bevelBothSides": true}),
            visible: true,
        });
        let a = art(&n);
        assert!(faces(&a).len() <= MAX_FACES + 8, "{object}: {}", faces(&a).len());
        assert_eq!(three_d_art(&n), Some(a), "{object}: the same solid again");
    }
}
