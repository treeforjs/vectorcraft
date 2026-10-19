//! Effects on every object kind: raster effects on type, images and symbol instances through an
//! offscreen layer of their art, geometry effects through their outlines, and one fill or
//! stroke's own raster effects.

use std::sync::Arc;

use serde_json::{Value, json};
use vectorcraft_color::{Color, Paint};
use vectorcraft_doc::text::{CharStyle, TextObject};
use vectorcraft_doc::{Appearance, Effect, ImageBlob, ImageObject, Node, NodeKind, Symbol};
use vectorcraft_geom::{Point, shapes};

use super::*;

fn fx(id: &str, params: Value) -> Effect {
    Effect { id: id.into(), params, visible: true }
}

/// A 100×100 document with `n` (its id allocated) on the first layer.
fn doc_with(mut d: Document, mut n: Node) -> Document {
    n.id = d.alloc_id();
    let l = d.layers[0].id;
    d.insert(Some(l), 0, n).unwrap();
    d
}

fn render_threads(d: &Document, threads: u16) -> Rendered {
    let mut r = Renderer::new();
    r.threads = threads;
    r.render(d, 100, 100, Affine::IDENTITY, &RenderOptions { background: Some([255, 255, 255, 255]), ..Default::default() })
}

fn render(d: &Document) -> Rendered {
    render_threads(d, 0)
}

fn lum(p: [u8; 4]) -> u32 {
    p[0] as u32 + p[1] as u32 + p[2] as u32
}

const WHITE: u32 = 765;

/// A 30×30 red image placed at (20, 20).
fn image_doc(effects: Vec<Effect>) -> Document {
    let mut d = Document::new(100.0, 100.0);
    let img = image::RgbaImage::from_pixel(30, 30, image::Rgba([255, 0, 0, 255]));
    let mut png = vec![];
    img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
    d.images.insert("red".into(), ImageBlob::new("image/png", png));
    let mut n = Node::new(
        NodeId(0),
        NodeKind::Image(ImageObject {
            key: "red".into(),
            width: 30,
            height: 30,
            xf: Affine::translate((20.0, 20.0)),
            link: None,
            placement: Default::default(),
        }),
    );
    n.appearance.effects = effects;
    doc_with(d, n)
}

/// An instance at (50, 50) of a symbol whose art is a black 20×20 square centred on its origin.
fn symbol_doc(effects: Vec<Effect>) -> Document {
    let mut d = Document::new(100.0, 100.0);
    let art = Node::path(
        d.alloc_id(),
        shapes::rectangle(Rect::new(-10.0, -10.0, 10.0, 10.0)),
        Appearance::basic(Paint::solid(Color::BLACK), Paint::None, 0.0),
    );
    d.symbols.push(Symbol { name: "Square".into(), art: Arc::new(art), slice: None });
    let mut n = Node::new(NodeId(0), NodeKind::SymbolInstance { symbol: "Square".into(), xf: Affine::translate((50.0, 50.0)) });
    n.appearance.effects = effects;
    doc_with(d, n)
}

/// Black 40 pt "IIII" with its baseline at (5, 60).
fn text_doc(effects: Vec<Effect>) -> Document {
    let style = CharStyle { size: 40.0, fill: Paint::solid(Color::BLACK), ..Default::default() };
    let mut n = Node::new(NodeId(0), NodeKind::Text(Box::new(TextObject::point(Point::new(5.0, 60.0), "IIII", style))));
    n.appearance.effects = effects;
    doc_with(Document::new(100.0, 100.0), n)
}

/// Columns (x) holding any dark pixel.
fn inked_columns(img: &Rendered) -> Vec<u32> {
    (0..100).filter(|x| (0..100).any(|y| lum(img.pixel(*x, y)) < 300)).collect()
}

#[test]
fn a_shadow_on_an_image_paints_offset_pixels() {
    let plain = render(&image_doc(vec![]));
    assert_eq!(lum(plain.pixel(55, 55)), WHITE);
    let d = image_doc(vec![fx("stylize.dropShadow", json!({"x": 10, "y": 10, "blur": 0, "opacity": 100, "mode": "normal"}))]);
    let img = render(&d);
    assert!(lum(img.pixel(55, 55)) < 100, "shadow below-right: {:?}", img.pixel(55, 55));
    assert_eq!(img.pixel(35, 35), [255, 0, 0, 255], "the image stays on top");
    assert_eq!(lum(img.pixel(15, 15)), WHITE);
    // The multithreaded pipeline filters offscreen and matches.
    let mt = render_threads(&d, 3);
    let worst = img.pixels.iter().zip(&mt.pixels).map(|(a, b)| a.abs_diff(*b)).max().unwrap();
    assert!(worst <= 4, "max channel difference {worst}");
}

#[test]
fn blur_on_a_symbol_instance_softens_its_edges() {
    let plain = render(&symbol_doc(vec![]));
    assert_eq!(lum(plain.pixel(38, 50)), WHITE);
    assert_eq!(lum(plain.pixel(50, 50)), 0);
    let img = render(&symbol_doc(vec![fx("blur.gaussian", json!({"radius": 8}))]));
    let edge = lum(img.pixel(38, 50));
    assert!(edge > 0 && edge < WHITE, "the blur spreads past the edge: {edge}");
    assert!(lum(img.pixel(50, 50)) < 200, "the centre stays dark");
}

#[test]
fn geometry_effects_reshape_type_through_its_outlines() {
    let plain = inked_columns(&render(&text_doc(vec![])));
    assert!(!plain.is_empty() && plain[0] < 30, "{plain:?}");
    // Transform moves the outlines as one piece.
    let moved = inked_columns(&render(&text_doc(vec![fx("distort.transform", json!({"moveH": 30}))])));
    assert_eq!(moved.first().copied(), plain.first().map(|x| x + 30));
    // Roughen redraws the glyphs as rough outlines.
    let d = text_doc(vec![fx("distort.roughen", json!({"size": 4, "relative": false, "detail": 30, "points": "corner"}))]);
    let rough = render(&d);
    let smooth = render(&text_doc(vec![]));
    let changed = rough.pixels.chunks(4).zip(smooth.pixels.chunks(4)).filter(|(a, b)| a != b).count();
    assert!(changed > 100, "{changed} pixels changed");
    // The reshaped art is plain paths painted with the text's fill.
    let n = &d.layers[0].children().unwrap()[0];
    let art = vectorcraft_effects::reshape(n, None).unwrap();
    let (mut text, mut black) = (0, 0);
    art.walk(&mut |m| {
        text += usize::from(matches!(m.kind, NodeKind::Text(_)));
        black += usize::from(m.appearance.fill_paint() == Paint::solid(Color::BLACK));
    });
    assert!(text == 0 && black > 0, "{art:?}");
}

/// Outline Object draws type as its glyph outlines: the same picture, made of plain paths that
/// Expand Appearance keeps.
#[test]
fn outline_object_draws_type_as_its_outlines() {
    let d = text_doc(vec![fx("path.outlineObject", json!({}))]);
    let (outlined, plain) = (render(&d), render(&text_doc(vec![])));
    let worst = outlined.pixels.iter().zip(&plain.pixels).map(|(a, b)| a.abs_diff(*b)).max().unwrap();
    assert!(worst <= 8, "max channel difference {worst}");
    let n = &d.layers[0].children().unwrap()[0];
    for art in [vectorcraft_effects::reshape(n, None).unwrap(), vectorcraft_effects::expand_outlined(n, None).unwrap()] {
        let (mut text, mut paths) = (0, 0);
        art.walk(&mut |m| {
            text += usize::from(matches!(m.kind, NodeKind::Text(_)));
            paths += usize::from(matches!(m.kind, NodeKind::Path { .. }));
        });
        assert!(text == 0 && paths > 0, "{art:?}");
    }
    // On a path it changes nothing.
    let sq = shapes::rectangle(Rect::new(0.0, 0.0, 10.0, 10.0));
    let out = vectorcraft_effects::apply_geometry(&[fx("path.outlineObject", json!({}))], &sq, Rect::new(0.0, 0.0, 10.0, 10.0));
    assert_eq!(out, sq);
}

/// Extrude & Bevel draws the square's solid: the lit front face and darker walls beyond it.
#[test]
fn extrude_paints_the_solid_around_the_object() {
    let mut n = Node::path(
        NodeId(0),
        shapes::rectangle(Rect::new(35.0, 35.0, 65.0, 65.0)),
        Appearance::basic(Paint::solid(Color::rgb(1.0, 0.0, 0.0)), Paint::None, 0.0),
    );
    n.appearance.effects.push(fx("threeD.extrude", json!({"depth": 20})));
    let img = render(&doc_with(Document::new(100.0, 100.0), n));
    let reddish = |p: [u8; 4]| p[0] > 60 && p[1] < 40 && p[2] < 40;
    let outside = (0..100u32).flat_map(|y| (0..100u32).map(move |x| (x, y))).filter(|&(x, y)| !(33..=67).contains(&x) || !(33..=67).contains(&y));
    let walls: Vec<_> = outside.filter(|&(x, y)| reddish(img.pixel(x, y))).collect();
    assert!(walls.len() > 20, "walls beyond the square: {}", walls.len());
    let front = img.pixel(50, 50);
    assert!(reddish(front), "{front:?}");
    assert!(walls.iter().any(|&(x, y)| img.pixel(x, y)[0] < front[0]), "walls darker than the front");
}

/// 3D and Materials draws its lit solid, and behind it, cast away from the light at the upper
/// left, a translucent grey shadow; exports draw the same faces (`bake_document`).
#[test]
fn materials_paint_the_lit_solid_and_its_shadow() {
    let mut n = Node::path(
        NodeId(0),
        shapes::rectangle(Rect::new(35.0, 25.0, 65.0, 55.0)),
        Appearance::basic(Paint::solid(Color::rgb(1.0, 0.0, 0.0)), Paint::None, 0.0),
    );
    let params = json!({"depth": 20, "shadow": true, "shadowPosition": "behind", "shadowDistance": 20, "shadowSoftness": 0, "rotateX": 0, "rotateY": 0, "rotateZ": 0});
    n.appearance.effects.push(fx("threeD.materials", params));
    let d = doc_with(Document::new(100.0, 100.0), n);
    let img = render(&d);
    let front = img.pixel(50, 40);
    assert!(front[0] > 60 && front[1] < 60 && front[2] < 60, "the red front: {front:?}");
    let shadow = img.pixel(80, 72);
    assert!(shadow[0] == shadow[1] && shadow[1] == shadow[2] && shadow[0] < 230 && shadow[0] > 40, "a translucent grey shadow: {shadow:?}");
    assert_eq!(img.pixel(10, 10), [255, 255, 255, 255]);
    let baked = effects::bake_document(&d).expect("the solid is baked for export");
    let mut faces = 0;
    for l in &baked.layers {
        l.walk(&mut |n| faces += usize::from(matches!(n.kind, NodeKind::Path { .. })));
    }
    assert!(faces >= 6, "the shadow, four walls and the front: {faces}");
}

#[test]
fn a_strokes_own_glow_paints_around_that_stroke_only() {
    let mut d = Document::new(100.0, 100.0);
    let mut n = Node::path(
        NodeId(0),
        shapes::rectangle(Rect::new(30.0, 30.0, 70.0, 70.0)),
        Appearance::basic(Paint::solid(Color::WHITE), Paint::solid(Color::BLACK), 2.0),
    );
    n.appearance.items[1].effects_mut().push(fx("stylize.outerGlow", json!({"color": "#00ff00", "mode": "normal", "opacity": 100, "blur": 6})));
    d = doc_with(d, n);
    let img = render(&d);
    let g = img.pixel(27, 50);
    assert!(g[1] > g[0] + 20 && g[1] > g[2] + 20, "green glow outside the stroke: {g:?}");
    assert_eq!(img.pixel(50, 50), [255, 255, 255, 255], "the fill has no glow");
    let mt = render_threads(&d, 3);
    let worst = img.pixels.iter().zip(&mt.pixels).map(|(a, b)| a.abs_diff(*b)).max().unwrap();
    assert!(worst <= 4, "max channel difference {worst}");
}
