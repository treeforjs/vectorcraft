//! The Photoshop-style raster effects (Effect › Blur › Radial Blur and Smart Blur, Sharpen ›
//! Unsharp Mask): applied and edited as commands, listed in the catalogue, drawn on the canvas,
//! and written to PDF and SVG as images of the effected object.

use serde_json::{Value, json};
use vectorcraft_doc::NodeKind;
use vectorcraft_geom::Rect;
use vectorcraft_render::{Rendered, Renderer};

use super::*;

/// The new effects with parameters that change a striped square visibly.
const EFFECTS: [(&str, &str); 3] = [
    ("blur.radial", r#"{"amount": 30, "quality": "draft"}"#),
    ("blur.smart", r#"{"radius": 8, "threshold": 100}"#),
    ("sharpen.unsharpMask", r#"{"amount": 300, "radius": 3}"#),
];

/// A group of a red square and a darker red stripe across it; returns the group's id.
fn striped() -> (Session, u64) {
    let mut s = Session::new();
    s.execute("file.new", &json!({"width": 200, "height": 200})).unwrap();
    let id = s.execute("shape.rectangle", &json!({"x": 50, "y": 50, "width": 100, "height": 100})).unwrap()["id"].as_u64().unwrap();
    s.execute("paint.setFill", &json!({"color": "#e03020", "ids": [id]})).unwrap();
    s.execute("paint.setStroke", &json!({"none": true, "ids": [id]})).unwrap();
    let stripe = s.execute("shape.rectangle", &json!({"x": 90, "y": 50, "width": 20, "height": 100})).unwrap()["id"].as_u64().unwrap();
    s.execute("paint.setFill", &json!({"color": "#b04848", "ids": [stripe]})).unwrap();
    s.execute("paint.setStroke", &json!({"none": true, "ids": [stripe]})).unwrap();
    s.execute("select.set", &json!({"ids": [id, stripe]})).unwrap();
    let g = s.execute("object.group", &json!({})).unwrap()["id"].as_u64().unwrap();
    (s, g)
}

fn render(s: &Session) -> Rendered {
    Renderer::new().render_region(&s.doc().unwrap().doc, Rect::new(0.0, 0.0, 200.0, 200.0), 1.0, true)
}

fn mean_diff(a: &Rendered, b: &Rendered) -> f64 {
    let sum: u64 = a.pixels.iter().zip(&b.pixels).map(|(x, y)| x.abs_diff(*y) as u64).sum();
    sum as f64 / a.pixels.len() as f64 / 255.0
}

#[test]
fn listed_applied_edited_and_undone() {
    let (mut s, g) = striped();
    let list = s.execute("effect.list", &json!({})).unwrap();
    for (id, _) in EFFECTS {
        let e = list["catalog"].as_array().unwrap().iter().find(|e| e["id"] == id).unwrap_or_else(|| panic!("{id} listed"));
        assert_eq!(e["raster"], json!(true), "{id}");
        assert!(e["params"].as_str().unwrap().contains("radius") || id == "blur.radial", "{id}");
    }
    let menus: Vec<Value> =
        EFFECTS.iter().map(|(id, _)| list["catalog"].as_array().unwrap().iter().find(|e| e["id"] == *id).unwrap()["menu"].clone()).collect();
    assert_eq!(menus, [json!(["Effect", "Blur"]), json!(["Effect", "Blur"]), json!(["Effect", "Sharpen"])]);
    // Junk parameters are stored as given and read clamped; the canvas still draws.
    for (id, _) in EFFECTS {
        s.execute("effect.apply", &json!({"effect": id, "ids": [g], "params": {"amount": 1e308, "radius": -1e308, "threshold": "x", "quality": 7}}))
            .unwrap();
    }
    let _ = render(&s);
    let n = s.doc().unwrap().doc.node(NodeId(g)).unwrap().clone();
    assert_eq!(n.appearance.effects.len(), 3);
    assert_eq!(n.appearance.effects[0].params["method"], json!("spin"), "defaults fill the rest");
    s.execute("effect.setParams", &json!({"index": 0, "ids": [g], "params": {"method": "zoom"}})).unwrap();
    assert_eq!(s.doc().unwrap().doc.node(NodeId(g)).unwrap().appearance.effects[0].params["method"], json!("zoom"));
    for _ in 0..4 {
        s.execute("edit.undo", &json!({})).unwrap();
    }
    assert!(s.doc().unwrap().doc.node(NodeId(g)).unwrap().appearance.effects.is_empty());
}

#[test]
fn each_effect_changes_the_canvas_and_draws_the_same_twice() {
    for (id, params) in EFFECTS {
        let (mut s, g) = striped();
        let plain = render(&s);
        let params: Value = serde_json::from_str(params).unwrap();
        s.execute("effect.apply", &json!({"effect": id, "ids": [g], "params": params})).unwrap();
        let a = render(&s);
        assert!(mean_diff(&plain, &a) > 0.0005, "{id} shows: {}", mean_diff(&plain, &a));
        assert_eq!(a.pixels, render(&s).pixels, "{id} is deterministic");
        // The multithreaded pipeline draws the same.
        let mut mt = Renderer::new();
        mt.threads = 4;
        let b = mt.render_region(&s.doc().unwrap().doc, Rect::new(0.0, 0.0, 200.0, 200.0), 1.0, true);
        assert!(mean_diff(&a, &b) < 0.002, "{id}: multithreaded differs by {}", mean_diff(&a, &b));
    }
}

#[test]
fn spin_blur_reaches_past_the_corners() {
    let (mut s, g) = striped();
    s.execute("effect.apply", &json!({"effect": "blur.radial", "ids": [g], "params": {"amount": 60, "quality": "good"}})).unwrap();
    let img = render(&s);
    // Turned corners sweep beyond the square's sides (centre 100, 100; corner 70.7 pt out).
    let p = img.pixel(100, 46);
    assert!(p[1] < 250, "the arc of the corners reaches above the square: {p:?}");
    assert!(img.pixel(100, 20)[1] > 250, "but not beyond the corners' circle");
}

#[test]
fn pdf_and_svg_write_the_effected_object_as_an_image() {
    for (id, params) in EFFECTS {
        let (mut s, g) = striped();
        let params: Value = serde_json::from_str(params).unwrap();
        s.execute("effect.apply", &json!({"effect": id, "ids": [g], "params": params})).unwrap();
        let doc = s.doc().unwrap().doc.clone();
        // PDF: an image that reads back as the canvas shows it.
        let r = crate::cmd::rasterfx::export_pdf_with_report(&doc, &Default::default()).unwrap();
        assert!(r.warnings.is_empty(), "{id}: {:?}", r.warnings);
        let back = vectorcraft_pdf::import(&r.bytes).unwrap();
        let diff = mean_diff(&render(&s), &Renderer::new().render_region(&back, Rect::new(0.0, 0.0, 200.0, 200.0), 1.0, true));
        assert!(diff < 0.03, "{id}: PDF reads back {:.2}% off", diff * 100.0);
        // SVG: the object becomes an image; a Gaussian Blur alone stays a live filter.
        let svg = s.execute("document.serialize", &json!({"format": "svg"})).unwrap()["text"].as_str().unwrap().to_string();
        assert!(svg.contains("<image") && !svg.contains("<filter"), "{id}: {svg}");
        let flat = crate::cmd::rasterfx::flatten_pixel_effects(&doc).unwrap();
        assert!(matches!(flat.layers[0].children().unwrap()[0].kind, NodeKind::Image(_)), "{id}");
    }
    let (mut s, g) = striped();
    s.execute("effect.apply", &json!({"effect": "blur.gaussian", "ids": [g]})).unwrap();
    assert!(crate::cmd::rasterfx::flatten_pixel_effects(&s.doc().unwrap().doc).is_none());
    let svg = s.execute("document.serialize", &json!({"format": "svg"})).unwrap()["text"].as_str().unwrap().to_string();
    assert!(svg.contains("feGaussianBlur") && !svg.contains("<image"));
}

#[test]
fn expand_appearance_makes_an_image() {
    let (mut s, g) = striped();
    s.execute("effect.apply", &json!({"effect": "sharpen.unsharpMask", "ids": [g]})).unwrap();
    s.execute("effect.expandAppearance", &json!({"ids": [g]})).unwrap();
    assert!(matches!(s.doc().unwrap().doc.node(NodeId(g)).unwrap().kind, NodeKind::Image(_)));
}

#[test]
fn gallery_stack_is_atomic_bounded_and_does_not_dirty_unchanged_artwork() {
    let (mut s, g) = striped();
    let before = s.doc().unwrap().doc.clone();
    let history = s.doc().unwrap().history.undo.len();
    for effects in [
        json!([{"id": "missing.effect"}]),
        json!([{"id": "blur.gaussian", "params": null}]),
        json!([{"id": "blur.gaussian", "visible": 1}]),
        json!(vec![json!({"id": "blur.gaussian"}); 129]),
    ] {
        assert!(s.execute("effect.setStack", &json!({"ids": [g], "effects": effects})).is_err());
        assert_eq!(s.doc().unwrap().doc, before);
    }
    let rows = json!([{"id": "blur.radial", "params": {"amount": 30}}, {"id": "sharpen.unsharpMask", "params": {"amount": 250}}]);
    s.execute("effect.setStack", &json!({"ids": [g], "effects": rows})).unwrap();
    assert_eq!(s.doc().unwrap().history.undo.len(), history + 1);
    s.execute("effect.setStack", &json!({"ids": [g], "effects": rows})).unwrap();
    assert_eq!(s.doc().unwrap().history.undo.len(), history + 1, "no-op does not add history");
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().doc, before);
}

#[test]
fn gallery_stack_order_and_visibility_survive_native_svg_pdf_and_expand() {
    let (mut s, g) = striped();
    let mut rows = vec![
        json!({"id": "blur.gaussian", "params": {"radius": 3}}),
        json!({"id": "stylize.glowingEdges", "params": {"edgeWidth": 2, "edgeBrightness": 12, "smoothness": 3}}),
    ];
    s.execute("effect.setStack", &json!({"ids": [g], "effects": rows})).unwrap();
    let first = render(&s);
    rows.reverse();
    s.execute("effect.setStack", &json!({"ids": [g], "effects": rows})).unwrap();
    let second = render(&s);
    assert!(mean_diff(&first, &second) > 0.001, "reordering noncommuting filters visibly changes output");
    // Export both the complete noncommuting stack and a hidden-row variant. Testing only
    // the latter would miss an exporter that accidentally reordered two active filters.
    for visible in [true, false] {
        rows[1]["visible"] = json!(visible);
        s.execute("effect.setStack", &json!({"ids": [g], "effects": rows})).unwrap();
        let expected = render(&s);
        if !visible {
            assert!(mean_diff(&expected, &second) > 0.001, "visibility affects the rendered result");
        }
        let doc = s.doc().unwrap().doc.clone();
        let native = vectorcraft_format::load(&vectorcraft_format::save(&doc, false)).unwrap();
        assert_eq!(native.node(NodeId(g)).unwrap().appearance.effects, doc.node(NodeId(g)).unwrap().appearance.effects);
        let pdf = crate::cmd::rasterfx::export_pdf_with_report(&doc, &Default::default()).unwrap();
        let pdf_back = vectorcraft_pdf::import(&pdf.bytes).unwrap();
        let svg = s.execute("document.serialize", &json!({"format": "svg"})).unwrap()["text"].as_str().unwrap().to_string();
        let svg_back = vectorcraft_svg::import(&svg).unwrap();
        for back in [&native, &pdf_back, &svg_back] {
            let actual = Renderer::new().render_region(back, Rect::new(0.0, 0.0, 200.0, 200.0), 1.0, true);
            assert!(mean_diff(&expected, &actual) < 0.03, "roundtrip must preserve filtered appearance: {}", mean_diff(&expected, &actual));
        }
        s.execute("effect.expandAppearance", &json!({"ids": [g]})).unwrap();
        assert!(mean_diff(&expected, &render(&s)) < 0.03);
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().doc, doc);
    }
}

fn new_filter_cases() -> [(&'static str, Value); 9] {
    [
        ("blur.smart", json!({"radius": 3, "threshold": 20, "mode": "edgeOnly"})),
        ("blur.smart", json!({"radius": 3, "threshold": 20, "mode": "overlayEdge"})),
        ("pixelate.mosaic", json!({"cellSize": 27})),
        ("pixelate.fragment", json!({"offset": 4})),
        ("pixelate.facet", json!({"cellSize": 27, "levels": 2})),
        ("pixelate.mezzotint", json!({"type": "shortStrokes", "grainSize": 1.5, "seed": 8})),
        ("pixelate.crystallize", json!({"cellSize": 19, "seed": 5})),
        ("pixelate.pointillize", json!({"cellSize": 12, "seed": 5, "backgroundColor": "#f0e0d0"})),
        ("pixelate.colorHalftone", json!({"radius": 7, "angle1": 30, "angle2": 60, "angle3": 90})),
    ]
}

#[test]
fn pixelate_and_smart_edge_modes_survive_history_native_and_exports() {
    for (id, params) in new_filter_cases() {
        let (mut session, id_node) = striped();
        let before = session.doc().unwrap().doc.clone();
        let plain = render(&session);
        let rows = json!([{"id": id, "params": params, "visible": true}]);
        session.execute("effect.setStack", &json!({"ids": [id_node], "effects": rows})).unwrap();
        let expected = render(&session);
        assert!(mean_diff(&plain, &expected) > 0.0005, "{id} {params} did not change the artwork");
        let doc = session.doc().unwrap().doc.clone();
        session.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(session.doc().unwrap().doc, before);
        session.execute("edit.redo", &json!({})).unwrap();
        assert_eq!(session.doc().unwrap().doc, doc);
        let native = vectorcraft_format::load(&vectorcraft_format::save(&doc, false)).unwrap();
        assert_eq!(native.node(NodeId(id_node)).unwrap().appearance.effects, doc.node(NodeId(id_node)).unwrap().appearance.effects);
        let pdf = crate::cmd::rasterfx::export_pdf_with_report(&doc, &Default::default()).unwrap();
        assert!(pdf.warnings.is_empty(), "{id}: {:?}", pdf.warnings);
        let pdf_back = vectorcraft_pdf::import(&pdf.bytes).unwrap();
        let svg = session.execute("document.serialize", &json!({"format": "svg"})).unwrap()["text"].as_str().unwrap().to_string();
        assert!(svg.contains("<image"), "{id} exports actual filtered pixels");
        let svg_back = vectorcraft_svg::import(&svg).unwrap();
        for back in [&native, &pdf_back, &svg_back] {
            let actual = Renderer::new().render_region(back, Rect::new(0.0, 0.0, 200.0, 200.0), 1.0, true);
            assert!(mean_diff(&expected, &actual) < 0.03, "{id} {params}: exported appearance differs by {}", mean_diff(&expected, &actual));
        }
        session.execute("effect.expandAppearance", &json!({"ids": [id_node]})).unwrap();
        assert!(mean_diff(&expected, &render(&session)) < 0.03, "{id}: Expand changed appearance");
        session.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(session.doc().unwrap().doc, doc);
    }
}

#[test]
fn pixelate_and_smart_modes_follow_rotated_views_and_large_cropped_viewports() {
    use vectorcraft_geom::Affine;
    use vectorcraft_render::RenderOptions;
    let options = RenderOptions { background: None, artboards: false, ..Default::default() };
    for (id, params) in new_filter_cases() {
        let (mut session, group) = striped();
        session.execute("effect.setStack", &json!({"ids": [group], "effects": [{"id": id, "params": params}]})).unwrap();
        let doc = &session.doc().unwrap().doc;
        let first = Renderer::new().render(doc, 200, 200, Affine::IDENTITY, &options);
        let rotation = Affine::translate((200.0, 0.0)) * Affine::rotate(std::f64::consts::FRAC_PI_2);
        let rotated = Renderer::new().render(doc, 200, 200, rotation, &options);
        let mut total = 0u64;
        for y in 0..200 {
            for x in 0..200 {
                total += first.pixel(x, y).iter().zip(rotated.pixel(199 - y, x)).map(|(a, b)| u64::from(a.abs_diff(b))).sum::<u64>();
            }
        }
        let difference = total as f64 / (200.0 * 200.0 * 4.0 * 255.0);
        assert!(difference < 0.005, "{id} {params}: camera rotation changed the filter: {difference}");

        // A 25M-pixel object exceeds the renderer's 8.4M budget: these small views force the
        // cropped path and exercise the source halo rather than the whole-object cache.
        let mut large = Session::new();
        large.execute("file.new", &json!({"width": 5000, "height": 5000})).unwrap();
        let background = large.execute("shape.rectangle", &json!({"x": 0, "y": 0, "width": 5000, "height": 5000})).unwrap()["id"].as_u64().unwrap();
        large.execute("paint.setFill", &json!({"color": "#e03020"})).unwrap();
        large.execute("paint.setStroke", &json!({"none": true})).unwrap();
        let stripe = large.execute("shape.rectangle", &json!({"x": 2490, "y": 0, "width": 20, "height": 5000})).unwrap()["id"].as_u64().unwrap();
        large.execute("paint.setFill", &json!({"color": "#b04848"})).unwrap();
        large.execute("paint.setStroke", &json!({"none": true})).unwrap();
        large.execute("select.set", &json!({"ids": [background, stripe]})).unwrap();
        let group = large.execute("object.group", &json!({})).unwrap()["id"].as_u64().unwrap();
        large.execute("effect.setStack", &json!({"ids": [group], "effects": [{"id": id, "params": params}]})).unwrap();
        let doc = &large.doc().unwrap().doc;
        let a = Renderer::new().render(doc, 200, 200, Affine::translate((-2400.0, -2400.0)), &options);
        let b = Renderer::new().render(doc, 200, 200, Affine::translate((-2383.0, -2389.0)), &options);
        for y in 0..189 {
            for x in 0..183 {
                assert_eq!(a.pixel(x, y), b.pixel(x + 17, y + 11), "{id} {params}: crop changed phase/halo at {x},{y}");
            }
        }
    }
}
