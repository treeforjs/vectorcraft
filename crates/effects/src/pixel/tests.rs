use serde_json::json;
use vectorcraft_doc::Effect;
use vectorcraft_geom::{Affine, Point, Rect};

use super::*;
use crate::{RasterFx, raster_effects};

/// A `w` × `h` raster whose pixels are document points (origin top-left).
fn space(w: usize, h: usize) -> PixelSpace {
    PixelSpace { to_doc: Affine::IDENTITY, px: 1.0, center: Point::new(w as f64 / 2.0, h as f64 / 2.0) }
}

fn image(w: usize, h: usize, f: impl Fn(usize, usize) -> [u8; 4]) -> Vec<u8> {
    let mut out = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        for x in 0..w {
            out.extend(f(x, y));
        }
    }
    out
}

fn at(d: &[u8], w: usize, x: usize, y: usize) -> [u8; 4] {
    let i = (y * w + x) * 4;
    [d[i], d[i + 1], d[i + 2], d[i + 3]]
}

fn fx(id: &str, p: serde_json::Value) -> PixelFx {
    let e = Effect { id: id.into(), params: p, visible: true };
    match raster_effects(&[e]).first() {
        Some(RasterFx::Pixel(p)) => *p,
        other => panic!("{id}: {other:?}"),
    }
}

fn grey(v: u8) -> [u8; 4] {
    [v, v, v, 255]
}

#[test]
fn params_take_defaults_and_stay_in_range() {
    assert_eq!(fx("blur.radial", json!({})), PixelFx::RadialBlur { amount: 10.0, zoom: false, passes: 6 });
    assert_eq!(
        fx("blur.radial", json!({"amount": 1e308, "method": "ZOOM", "quality": "best"})),
        PixelFx::RadialBlur { amount: 100.0, zoom: true, passes: 8 }
    );
    assert_eq!(fx("blur.radial", json!({"amount": -5, "quality": "nonsense"})), PixelFx::RadialBlur { amount: 1.0, zoom: false, passes: 6 });
    assert_eq!(fx("blur.smart", json!({})), PixelFx::SmartBlur { radius: 3.0, threshold: 25.0, samples: 7 });
    assert_eq!(
        fx("blur.smart", json!({"radius": "1e999", "threshold": -1, "quality": "low"})),
        PixelFx::SmartBlur { radius: 3.0, threshold: 0.1, samples: 5 }
    );
    assert_eq!(fx("sharpen.unsharpMask", json!({})), PixelFx::UnsharpMask { amount: 0.5, radius: 1.0, threshold: 0.0 });
    assert_eq!(
        fx("sharpen.unsharpMask", json!({"amount": 1e9, "radius": 0, "threshold": 1e9})),
        PixelFx::UnsharpMask { amount: 5.0, radius: 0.1, threshold: 255.0 }
    );
    assert_eq!(fx("stylize.glowingEdges", json!({})), PixelFx::GlowingEdges { width: 2.0, brightness: 6.0, smoothness: 5.0 });
    for id in PIXEL_EFFECTS {
        assert!(crate::is_raster(id) && crate::effect_info(id).is_some_and(|e| e.raster), "{id}");
    }
}

#[test]
fn outsets_follow_the_object() {
    let b = Rect::new(0.0, 0.0, 100.0, 50.0);
    let spin = fx("blur.radial", json!({}));
    let far = 0.5 * 100f64.hypot(50.0);
    assert!((spin.outset(b) - (far - 25.0)).abs() < 1e-9);
    assert!(fx("blur.radial", json!({"method": "zoom", "amount": 100})).outset(b) > 0.4 * far);
    assert_eq!(fx("blur.smart", json!({"radius": 4})).outset(b), 4.0);
    assert_eq!(fx("sharpen.unsharpMask", json!({})).outset(b), 0.0);
    assert_eq!(spin.reach(), None);
    assert_eq!(fx("sharpen.unsharpMask", json!({"radius": 2})).reach(), Some(6.0));
}

#[test]
fn wrong_sizes_and_empty_rasters_are_left_alone() {
    for id in PIXEL_EFFECTS {
        let f = fx(id, json!({}));
        let mut d = vec![200u8; 4 * 10];
        f.apply(&mut d, 3, 3, &space(3, 3));
        f.apply(&mut d, 0, 10, &space(0, 10));
        f.apply(&mut [], 0, 0, &space(0, 0));
        assert!(d.iter().all(|v| *v == 200), "{id}");
    }
}

#[test]
fn extreme_parameters_finish() {
    let (w, h) = (40, 30);
    let src = image(w, h, |x, y| if (x / 5 + y / 5) % 2 == 0 { grey(250) } else { [0, 0, 0, 0] });
    let tiny = PixelSpace { px: 1e-9, ..space(w, h) };
    let huge = PixelSpace { px: 1e9, center: Point::new(f64::MAX, -1e300), ..space(w, h) };
    for id in PIXEL_EFFECTS {
        for p in [json!({"amount": 1e308, "radius": 1e308, "threshold": 1e308, "quality": "best"}), json!({"amount": 0, "radius": 0})] {
            for s in [tiny, huge, space(w, h)] {
                let mut d = src.clone();
                fx(id, p.clone()).apply(&mut d, w, h, &s);
            }
        }
    }
}

#[test]
fn every_effect_is_deterministic() {
    let (w, h) = (32, 24);
    let src = image(w, h, |x, y| {
        let a = if x > 4 { 255 } else { 128 };
        [((x * 8) as u8).min(a), ((y * 10) as u8).min(a), ((x * y % 256) as u8).min(a), a]
    });
    for id in PIXEL_EFFECTS {
        let f = fx(id, json!({"amount": 40, "radius": 3, "threshold": 40}));
        let (mut a, mut b) = (src.clone(), src.clone());
        f.apply(&mut a, w, h, &space(w, h));
        f.apply(&mut b, w, h, &space(w, h));
        assert_eq!(a, b, "{id}");
        assert_ne!(a, src, "{id} changes the image");
        // Premultiplied stays premultiplied.
        assert!(a.as_chunks::<4>().0.iter().all(|p| p[0] <= p[3] && p[1] <= p[3] && p[2] <= p[3]), "{id}");
    }
}

#[test]
fn glowing_edges_draw_a_bright_outline_and_preserve_premultiplication() {
    let (w, h) = (9, 9);
    let src = image(w, h, |x, _| if x < 4 { [0, 0, 0, 200] } else { [200, 200, 200, 200] });
    let mut out = src.clone();
    fx("stylize.glowingEdges", json!({"edgeWidth": 1, "smoothness": 1, "edgeBrightness": 20})).apply(&mut out, w, h, &space(w, h));
    assert!(at(&out, w, 3, 4)[0] > 0 || at(&out, w, 4, 4)[0] > 0);
    assert!(out.as_chunks::<4>().0.iter().all(|p| p[0] <= p[3] && p[1] <= p[3] && p[2] <= p[3]));
}

/// Glowing Edges sees transparency beyond every side of the raster: a solid square glows alike at
/// its left and right edges (the right one doesn't read the next row's first pixel).
#[test]
fn glowing_edges_glow_alike_on_every_side() {
    let (w, h) = (9, 9);
    let mut out = image(w, h, |_, _| [180, 180, 180, 255]);
    fx("stylize.glowingEdges", json!({"edgeWidth": 1, "smoothness": 1, "edgeBrightness": 20})).apply(&mut out, w, h, &space(w, h));
    let (left, right, top, bottom) = (at(&out, w, 0, 4), at(&out, w, 8, 4), at(&out, w, 4, 0), at(&out, w, 4, 8));
    assert!(left[0] > 0 && left == right && top == bottom, "{left:?} {right:?} {top:?} {bottom:?}");
    assert!(at(&out, w, 4, 4)[0] < left[0], "the inside stays darker than the edges");
}

#[test]
fn glowing_edges_defaults_keep_the_outline_bright_and_use_source_colour() {
    let (w, h) = (17, 17);
    let src = image(w, h, |x, y| if (4..13).contains(&x) && (4..13).contains(&y) { [180, 0, 0, 255] } else { [0; 4] });
    let mut out = src.clone();
    fx("stylize.glowingEdges", json!({})).apply(&mut out, w, h, &space(w, h));

    let outline = at(&out, w, 4, 8);
    assert!(outline[0] >= 170, "default outline should retain a near-full-color peak: {outline:?}");
    let outside = at(&out, w, 3, 8);
    assert_eq!(outside[1], 0, "transparent halo must not be grey: {outside:?}");
    assert_eq!(outside[2], 0, "transparent halo must retain source hue: {outside:?}");
    // The glow outside keeps the edge's colour as it fades, rather than darkening towards grey.
    assert!(outside[3] > 0 && u32::from(outside[0]) * 255 >= 240 * u32::from(outside[3]), "{outside:?}");
    assert!(out.as_chunks::<4>().0.iter().all(|p| p[0] <= p[3] && p[1] <= p[3] && p[2] <= p[3]));
}

#[test]
fn spin_blur_smears_along_arcs_and_keeps_the_centre() {
    let (w, h) = (41, 41);
    // A white dot 12 px right of the centre (20.5, 20.5), and one on the centre.
    let src = image(w, h, |x, y| if (x == 32 || x == 20) && y == 20 { grey(255) } else { [0, 0, 0, 0] });
    let mut d = src.clone();
    fx("blur.radial", json!({"amount": 60, "quality": "best"})).apply(&mut d, w, h, &space(w, h));
    // 60° of arc around the centre: up to 30° either way, 12 px out: (20.5 + 12 cos 20°, 20.5 ± 12 sin 20°).
    assert!(at(&d, w, 31, 16)[3] > 0 && at(&d, w, 31, 24)[3] > 0, "the dot spreads along its circle");
    assert_eq!(at(&d, w, 32, 10)[3], 0, "not beyond the arc");
    assert_eq!(at(&d, w, 26, 20)[3], 0, "not towards the centre");
    assert!(at(&d, w, 20, 20)[3] > 100, "the centre barely moves: {:?}", at(&d, w, 20, 20));
    // Zoom smears along the ray instead.
    let mut z = src.clone();
    fx("blur.radial", json!({"amount": 60, "method": "zoom", "quality": "best"})).apply(&mut z, w, h, &space(w, h));
    assert!(at(&z, w, 35, 20)[3] > 0 && at(&z, w, 29, 20)[3] > 0, "the dot streaks along the ray");
    assert_eq!(at(&z, w, 32, 16)[3], 0, "not across it");
}

#[test]
fn smart_blur_smooths_noise_and_keeps_edges() {
    let (w, h) = (40, 20);
    // Left: grey 100 with ±6 noise; right: white.
    let src = image(w, h, |x, y| if x < 20 { grey(if (x + y) % 2 == 0 { 94 } else { 106 }) } else { grey(255) });
    let mut d = src.clone();
    fx("blur.smart", json!({"radius": 3, "threshold": 25})).apply(&mut d, w, h, &space(w, h));
    let v = at(&d, w, 8, 10)[0];
    assert!((97..=103).contains(&v), "noise smoothed: {v}");
    assert!(at(&d, w, 19, 10)[0] < 120, "the edge's dark side doesn't take the white");
    assert_eq!(at(&d, w, 20, 10)[0], 255, "the edge's white side stays white");
    // A threshold above the step blurs across it.
    let mut all = src.clone();
    fx("blur.smart", json!({"radius": 3, "threshold": 100})).apply(&mut all, w, h, &space(w, h));
    assert_eq!(at(&all, w, 20, 10)[0], 255, "a 155-level step is still past 100");
}

#[test]
fn unsharp_mask_overshoots_at_edges() {
    let (w, h) = (40, 10);
    let src = image(w, h, |x, _| grey(if x < 20 { 100 } else { 150 }));
    let mut d = src.clone();
    fx("sharpen.unsharpMask", json!({"amount": 100, "radius": 2})).apply(&mut d, w, h, &space(w, h));
    assert!(at(&d, w, 19, 5)[0] < 95, "dark side darker: {:?}", at(&d, w, 19, 5));
    assert!(at(&d, w, 20, 5)[0] > 155, "light side lighter: {:?}", at(&d, w, 20, 5));
    assert_eq!(at(&d, w, 5, 5), grey(100), "flat areas stay");
    assert_eq!(at(&d, w, 35, 5), grey(150));
    // Differences under the threshold are left alone.
    let mut t = src.clone();
    fx("sharpen.unsharpMask", json!({"amount": 100, "radius": 2, "threshold": 60})).apply(&mut t, w, h, &space(w, h));
    assert_eq!(t, src);
    // Coverage stays, and a shape's edge against transparency doesn't darken.
    let shape = image(w, h, |x, _| if x < 20 { [200, 0, 0, 255] } else { [0, 0, 0, 0] });
    let mut s = shape.clone();
    fx("sharpen.unsharpMask", json!({"amount": 300, "radius": 3})).apply(&mut s, w, h, &space(w, h));
    assert_eq!(s, shape);
}

#[test]
fn lengths_are_document_units() {
    // The same art at twice the resolution, sharpened with the same radius in points: the
    // overshoot spans twice the pixels, the same distance.
    let edge = |scale: usize| image(40 * scale, 4, move |x, _| grey(if x < 20 * scale { 100 } else { 150 }));
    let run = |scale: usize| {
        let mut d = edge(scale);
        let s = PixelSpace { to_doc: Affine::scale(1.0 / scale as f64), px: 1.0 / scale as f64, center: Point::ZERO };
        fx("sharpen.unsharpMask", json!({"amount": 100, "radius": 2})).apply(&mut d, 40 * scale, 4, &s);
        (0..40 * scale).filter(|x| at(&d, 40 * scale, *x, 2)[0] < 99).count()
    };
    let (one, two) = (run(1), run(2));
    assert!(one > 1 && (two as f64 / one as f64 - 2.0).abs() < 0.5, "{one} px at 1×, {two} px at 2×");
}

#[test]
fn gaussian_and_plane_blurs_keep_their_mass() {
    let (w, h) = (30, 30);
    let mut d = image(w, h, |x, y| if (10..20).contains(&x) && (10..20).contains(&y) { grey(255) } else { [0, 0, 0, 0] });
    let before: u32 = d.as_chunks::<4>().0.iter().map(|p| p[3] as u32).sum();
    gaussian_rgba(&mut d, w, h, 2.0);
    let after: u32 = d.as_chunks::<4>().0.iter().map(|p| p[3] as u32).sum();
    assert!((before as f64 - after as f64).abs() / (before as f64) < 0.02, "{before} → {after}");
    assert!(at(&d, w, 9, 15)[3] > 0 && at(&d, w, 10, 15)[3] < 255);
    let mut plane: Vec<f32> = (0..w * h).map(|i| if i == 15 * w + 15 { 100.0 } else { 0.0 }).collect();
    blur_plane(&mut plane, w, h, 2.0);
    assert!((plane.iter().sum::<f32>() - 100.0).abs() < 0.5 && plane[15 * w + 13] > 0.0 && plane[13 * w + 15] > 0.0);
}

#[test]
fn pixelate_params_take_defaults_and_stay_in_range() {
    assert_eq!(fx("pixelate.crystallize", json!({})), PixelFx::Crystallize { size: 10.0 });
    assert_eq!(fx("pixelate.crystallize", json!({"cellSize": 1e308})), PixelFx::Crystallize { size: 300.0 });
    assert_eq!(fx("pixelate.pointillize", json!({"cellSize": -4})), PixelFx::Pointillize { size: 3.0 });
    assert_eq!(fx("pixelate.colorHalftone", json!({})), PixelFx::ColorHalftone { radius: 8.0, angles: [108.0, 162.0, 90.0] });
    assert_eq!(
        fx("pixelate.colorHalftone", json!({"maxRadius": 1, "angle1": 1e300, "angle2": "nan", "angle3": -45})),
        PixelFx::ColorHalftone { radius: 4.0, angles: [360.0, 162.0, -45.0] }
    );
    assert_eq!(fx("pixelate.mezzotint", json!({})), PixelFx::Mezzotint { kind: Mezzotint::FineDots });
    assert_eq!(fx("pixelate.mezzotint", json!({"type": "Long Strokes"})), PixelFx::Mezzotint { kind: Mezzotint::LongStrokes });
    assert_eq!(fx("pixelate.mezzotint", json!({"type": "nonsense"})), PixelFx::Mezzotint { kind: Mezzotint::FineDots });
}

/// Crystallize fills cells with one colour each: a flat colour stays, a gradient turns into far
/// fewer colours than it had, and the cells stay on the art when the raster's origin moves.
#[test]
fn crystallize_makes_flat_cells_fixed_to_the_art() {
    let (w, h) = (60, 40);
    let mut flat = image(w, h, |_, _| [40, 80, 120, 255]);
    let before = flat.clone();
    fx("pixelate.crystallize", json!({"cellSize": 8})).apply(&mut flat, w, h, &space(w, h));
    assert_eq!(flat, before);
    let ramp = |dx: usize| image(w, h, move |x, y| [((x + dx) * 4) as u8, (y * 6) as u8, 90, 255]);
    let mut d = ramp(0);
    fx("pixelate.crystallize", json!({"cellSize": 8})).apply(&mut d, w, h, &space(w, h));
    let colours: std::collections::HashSet<[u8; 4]> = d.as_chunks::<4>().0.iter().copied().collect();
    assert!(colours.len() < 80 && colours.len() > 10, "{} cells", colours.len());
    // The same art rendered 10 px further left: away from the raster's edges the cells match.
    let mut shifted = ramp(10);
    let s = PixelSpace { to_doc: Affine::translate((10.0, 0.0)), ..space(w, h) };
    fx("pixelate.crystallize", json!({"cellSize": 8})).apply(&mut shifted, w, h, &s);
    let same = (20..40).all(|x| (12..28).all(|y| at(&shifted, w, x - 10, y) == at(&d, w, x, y)));
    assert!(same, "the cells follow the art, not the raster");
}

/// Pointillize leaves transparency alone and breaks a solid colour into its dots on white.
#[test]
fn pointillize_draws_dots_on_white() {
    let (w, h) = (40, 40);
    let mut clear = vec![0u8; w * h * 4];
    fx("pixelate.pointillize", json!({})).apply(&mut clear, w, h, &space(w, h));
    assert!(clear.iter().all(|v| *v == 0));
    let mut red = image(w, h, |_, _| [200, 0, 0, 255]);
    fx("pixelate.pointillize", json!({"cellSize": 6})).apply(&mut red, w, h, &space(w, h));
    let px = red.as_chunks::<4>().0;
    let dots = px.iter().filter(|p| **p == [200, 0, 0, 255]).count();
    let white = px.iter().filter(|p| **p == [255, 255, 255, 255]).count();
    assert!(dots > px.len() / 4 && white > 0, "{dots} dot and {white} white pixels");
    assert!(px.iter().all(|p| p[3] == 255 && p[0] >= p[1]));
}

/// Mezzotint turns each channel full or empty, about as often as its level says.
#[test]
fn mezzotint_thresholds_every_channel() {
    let (w, h) = (80, 80);
    for kind in Mezzotint::KINDS.map(|k| k.0) {
        let mut d = image(w, h, |_, _| [128, 0, 255, 255]);
        fx("pixelate.mezzotint", json!({"type": kind})).apply(&mut d, w, h, &space(w, h));
        let px = d.as_chunks::<4>().0;
        assert!(px.iter().all(|p| matches!(p[0], 0 | 255) && p[1] == 0 && p[2] == 255 && p[3] == 255), "{kind}");
        let on = px.iter().filter(|p| p[0] == 255).count() as f64 / px.len() as f64;
        assert!((0.2..0.8).contains(&on), "{kind}: {on}");
    }
    // Half-covered pixels keep their coverage, premultiplied.
    let mut half = image(4, 4, |_, _| [64, 64, 64, 128]);
    fx("pixelate.mezzotint", json!({})).apply(&mut half, 4, 4, &space(4, 4));
    assert!(half.as_chunks::<4>().0.iter().all(|p| p[3] == 128 && p[..3].iter().all(|c| matches!(c, 0 | 128))));
}

/// Color Halftone: a channel at full covers its screen, an empty one leaves it empty, and a middle
/// level covers about its share in dots.
#[test]
fn color_halftone_dots_follow_each_channels_level() {
    let (w, h) = (96, 96);
    let mut d = image(w, h, |_, _| [255, 0, 128, 255]);
    fx("pixelate.colorHalftone", json!({"maxRadius": 6})).apply(&mut d, w, h, &space(w, h));
    let px = d.as_chunks::<4>().0;
    assert!(px.iter().all(|p| p[0] == 255 && p[1] == 0 && p[3] == 255));
    let blue = px.iter().map(|p| f64::from(p[2]) / 255.0).sum::<f64>() / px.len() as f64;
    assert!((0.35..0.65).contains(&blue), "blue covers {blue}");
    assert!(px.iter().any(|p| p[2] == 0) && px.iter().any(|p| p[2] == 255), "dots, not a flat tone");
}

#[test]
fn distort_params_take_defaults_and_stay_in_range() {
    assert_eq!(fx("distort.diffuseGlow", json!({})), PixelFx::DiffuseGlow { grain: 6.0, glow: 10.0, clear: 15.0 });
    assert_eq!(
        fx("distort.diffuseGlow", json!({"graininess": 1e9, "glowAmount": -1, "clearAmount": "50"})),
        PixelFx::DiffuseGlow { grain: 10.0, glow: 0.0, clear: 20.0 }
    );
    let glass = Glass { distortion: 5.0, smoothness: 3.0, texture: GlassTexture::Frosted, scaling: 1.0, invert: false };
    assert_eq!(fx("distort.glass", json!({})), PixelFx::Glass(glass));
    assert_eq!(
        fx("distort.glass", json!({"distortion": 99, "smoothness": 0, "texture": "Tiny Lens", "scaling": 1e9, "invert": true})),
        PixelFx::Glass(Glass { distortion: 20.0, smoothness: 1.0, texture: GlassTexture::TinyLens, scaling: 2.0, invert: true })
    );
    assert_eq!(fx("distort.oceanRipple", json!({})), PixelFx::OceanRipple { size: 9.0, magnitude: 9.0 });
    assert_eq!(fx("distort.glass", json!({"distortion": 4})).outset(Rect::ZERO), 2.0);
}

/// Diffuse Glow brightens the light tones towards white and leaves black alone (but for grain).
#[test]
fn diffuse_glow_whitens_the_highlights() {
    let (w, h) = (40, 20);
    let mut d = image(w, h, |x, _| if x < 20 { grey(0) } else { grey(230) });
    fx("distort.diffuseGlow", json!({"graininess": 0})).apply(&mut d, w, h, &space(w, h));
    assert_eq!(at(&d, w, 5, 10), grey(0), "black doesn't glow");
    assert!(at(&d, w, 35, 10)[0] > 240, "highlights glow: {:?}", at(&d, w, 35, 10));
    let mut grain = image(w, h, |_, _| grey(0));
    fx("distort.diffuseGlow", json!({"graininess": 10})).apply(&mut grain, w, h, &space(w, h));
    let specks = grain.as_chunks::<4>().0.iter().filter(|p| p[0] > 0).count();
    assert!(specks > 0 && specks < w * h / 4, "{specks} grain specks");
}

/// Glass and Ocean Ripple move the content without changing its colours, and a stronger setting
/// moves it further.
#[test]
fn glass_and_ripples_displace_the_content() {
    let (w, h) = (64, 64);
    let stripes = image(w, h, |x, _| if (x / 5) % 2 == 0 { [255, 0, 0, 255] } else { [0, 0, 255, 255] });
    let changed = |id: &str, p: serde_json::Value| {
        let mut d = stripes.clone();
        fx(id, p).apply(&mut d, w, h, &space(w, h));
        (0..w * h).filter(|i| d[i * 4..i * 4 + 4] != stripes[i * 4..i * 4 + 4]).count()
    };
    for (id, key) in [("distort.glass", "distortion"), ("distort.oceanRipple", "rippleMagnitude")] {
        let (none, some, more) = (changed(id, json!({key: 0})), changed(id, json!({key: 4})), changed(id, json!({key: 20})));
        assert_eq!(none, 0, "{id}: nothing moves at 0");
        assert!(some > 0 && more > some, "{id}: {some} then {more} pixels move");
    }
    for texture in GlassTexture::KINDS.map(|k| k.0) {
        assert!(changed("distort.glass", json!({"texture": texture, "distortion": 10})) > 0, "{texture}");
    }
}

/// Timings on a 2000 × 2000 raster (`cargo test -p vectorcraft-effects --release -- --ignored
/// --nocapture pixel_timings`).
#[test]
#[ignore]
fn pixel_timings() {
    let (w, h) = (2000, 2000);
    let src = image(w, h, |x, y| [(x % 256) as u8, (y % 256) as u8, ((x ^ y) % 256) as u8, 255]);
    let s = PixelSpace { to_doc: Affine::IDENTITY, px: 1.0, center: Point::new(1000.0, 1000.0) };
    let cases = [
        ("blur.radial", json!({"quality": "draft"})),
        ("blur.radial", json!({"quality": "good"})),
        ("blur.radial", json!({"quality": "best", "method": "zoom"})),
        ("blur.smart", json!({"quality": "low"})),
        ("blur.smart", json!({"quality": "high", "radius": 30})),
        ("sharpen.unsharpMask", json!({"radius": 20})),
        ("pixelate.colorHalftone", json!({})),
        ("pixelate.crystallize", json!({})),
        ("pixelate.mezzotint", json!({"type": "longStrokes"})),
        ("pixelate.pointillize", json!({})),
        ("distort.diffuseGlow", json!({})),
        ("distort.glass", json!({})),
        ("distort.oceanRipple", json!({})),
    ];
    for (id, p) in cases {
        let mut d = src.clone();
        let t = std::time::Instant::now();
        fx(id, p.clone()).apply(&mut d, w, h, &s);
        println!("{id} {p}: {:?}", t.elapsed());
    }
}
