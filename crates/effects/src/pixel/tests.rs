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

#[test]
fn video_params_take_defaults() {
    assert_eq!(fx("video.deInterlace", json!({})), PixelFx::DeInterlace { odd: true, interpolate: true });
    assert_eq!(
        fx("video.deInterlace", json!({"eliminate": "Even", "create": "duplication"})),
        PixelFx::DeInterlace { odd: false, interpolate: false }
    );
    assert_eq!(fx("video.deInterlace", json!({"eliminate": 3, "create": null})), PixelFx::DeInterlace { odd: true, interpolate: true });
    assert_eq!(fx("video.ntscColors", json!({"anything": 1})), PixelFx::NtscColors);
}

/// De-Interlace rebuilds the eliminated field's lines from the other field's.
#[test]
fn de_interlace_rebuilds_one_field_from_the_other() {
    let (w, h) = (8, 10);
    let (red, blue) = ([255, 0, 0, 255], [0, 0, 255, 255]);
    let fields = image(w, h, |_, y| if y % 2 == 0 { red } else { blue });
    let run = |p: serde_json::Value| {
        let mut d = fields.clone();
        fx("video.deInterlace", p).apply(&mut d, w, h, &space(w, h));
        d
    };
    for create in ["duplication", "interpolation"] {
        let d = run(json!({"eliminate": "odd", "create": create}));
        for y in 0..h - 1 {
            assert_eq!(at(&d, w, 3, y), red, "{create}: line {y}");
        }
        let d = run(json!({"eliminate": "even", "create": create}));
        for y in 1..h {
            assert_eq!(at(&d, w, 3, y), blue, "{create}: line {y}");
        }
    }
    // The last line has nothing below it to interpolate with: half of it fades.
    let d = run(json!({"eliminate": "odd"}));
    assert_eq!(at(&d, w, 3, h - 1), [128, 0, 0, 128]);
}

/// NTSC Colors tames colours too strong for broadcast, keeping their hue and luma, and leaves
/// greys, gentle colours and transparency alone.
#[test]
fn ntsc_colors_desaturate_only_illegal_colours() {
    let luma = |p: [u8; 4]| 0.299 * f64::from(p[0]) + 0.587 * f64::from(p[1]) + 0.114 * f64::from(p[2]);
    let colours =
        [[255, 0, 0, 255], [255, 255, 0, 255], [0, 255, 255, 255], [128, 128, 128, 255], [140, 110, 100, 255], [0, 0, 0, 0], [100, 0, 0, 100]];
    let mut d: Vec<u8> = colours.iter().flatten().copied().collect();
    fx("video.ntscColors", json!({})).apply(&mut d, colours.len(), 1, &space(colours.len(), 1));
    let out: Vec<[u8; 4]> = d.as_chunks::<4>().0.to_vec();
    for i in [0, 1, 2, 6] {
        let (before, after) = (colours[i], out[i]);
        assert_ne!(before, after, "{before:?} is too strong");
        assert!((luma(before) - luma(after)).abs() <= 2.0, "{before:?} → {after:?} keeps its luma");
        assert_eq!(after[3], before[3]);
    }
    // Red stays the reddest channel, yellow keeps red and green level.
    assert!(out[0][0] > out[0][1] && out[0][1] == out[0][2], "{:?}", out[0]);
    assert!(out[1][0].abs_diff(out[1][1]) <= 1 && out[1][2] > 0, "{:?}", out[1]);
    for i in [3, 4, 5] {
        assert_eq!(out[i], colours[i], "{:?} is legal", colours[i]);
    }
    // Legal colours stay as they are: a second pass changes nothing.
    let mut again = d.clone();
    fx("video.ntscColors", json!({})).apply(&mut again, colours.len(), 1, &space(colours.len(), 1));
    for (a, b) in again.iter().zip(&d) {
        assert!(a.abs_diff(*b) <= 1);
    }
}

#[test]
fn texture_params_take_defaults_and_stay_in_range() {
    assert_eq!(fx("texture.craquelure", json!({})), PixelFx::Craquelure { spacing: 15.0, depth: 6.0, brightness: 9.0 });
    assert_eq!(fx("texture.grain", json!({})), PixelFx::Grain { intensity: 40.0, contrast: 50.0, kind: Grain::Regular });
    assert_eq!(
        fx("texture.grain", json!({"intensity": 1e9, "contrast": -5, "grainType": "Speckle"})),
        PixelFx::Grain { intensity: 100.0, contrast: 0.0, kind: Grain::Speckle }
    );
    assert_eq!(fx("texture.mosaicTiles", json!({"tileSize": 0})), PixelFx::MosaicTiles { tile: 2.0, grout: 3.0, lighten: 9.0 });
    assert_eq!(fx("texture.patchwork", json!({})), PixelFx::Patchwork { square: 6.0, relief: 8.0 });
    assert_eq!(fx("texture.stainedGlass", json!({"cellSize": 999})), PixelFx::StainedGlass { size: 50.0, border: 4.0, light: 3.0 });
    let canvas = Texturizer { surface: Surface::Canvas, scaling: 1.0, relief: 4.0, light: Light::Top, invert: false };
    assert_eq!(fx("texture.texturizer", json!({})), PixelFx::Texturizer(canvas));
    assert_eq!(
        fx("texture.texturizer", json!({"texture": "Brick", "scaling": 10, "light": "Bottom Right", "invert": true})),
        PixelFx::Texturizer(Texturizer { surface: Surface::Brick, scaling: 0.5, light: Light::BottomRight, invert: true, ..canvas })
    );
}

/// A smooth colourful ramp to texture.
fn ramp(w: usize, h: usize) -> Vec<u8> {
    image(w, h, |x, y| [(40 + x * 3) as u8, (60 + y * 2) as u8, 120, 255])
}

/// How many pixels differ between two rasters.
fn differing(a: &[u8], b: &[u8]) -> usize {
    a.chunks(4).zip(b.chunks(4)).filter(|(p, q)| p != q).count()
}

/// Craquelure darkens thin cracks between plates, and its pattern stays on the art.
#[test]
fn craquelure_cracks_the_art_into_plates() {
    let (w, h) = (64, 64);
    let mut d = image(w, h, |_, _| grey(200));
    fx("texture.craquelure", json!({"crackSpacing": 12})).apply(&mut d, w, h, &space(w, h));
    let levels: Vec<u8> = d.chunks(4).map(|p| p[0]).collect();
    let dark = levels.iter().filter(|v| **v < 150).count();
    assert!(dark > w * h / 50 && dark < w * h / 2, "{dark} crack pixels");
    // The same art moved by whole cells cracks the same way where they overlap.
    let mut shifted = image(w, h, |_, _| grey(200));
    let moved = PixelSpace { to_doc: Affine::translate((12.0, 0.0)), px: 1.0, center: Point::new(44.0, 32.0) };
    fx("texture.craquelure", json!({"crackSpacing": 12})).apply(&mut shifted, w, h, &moved);
    for y in 2..h - 2 {
        for x in 2..w - 14 {
            assert_eq!(at(&d, w, x + 12, y), at(&shifted, w, x, y), "({x}, {y})");
        }
    }
}

/// Grain adds noise as strong as its intensity; its kinds add specks, dots or streaks.
#[test]
fn grain_adds_noise_of_each_kind() {
    let (w, h) = (48, 48);
    let src = ramp(w, h);
    let run = |p: serde_json::Value| {
        let mut d = src.clone();
        fx("texture.grain", p).apply(&mut d, w, h, &space(w, h));
        d
    };
    assert_eq!(run(json!({"intensity": 0})), src, "no grain, unchanged contrast");
    assert!(differing(&run(json!({"intensity": 60})), &src) > w * h / 2);
    let count = |d: &[u8], v: u8| d.chunks(4).filter(|p| p[..3] == [v; 3]).count();
    assert!(count(&run(json!({"intensity": 90, "grainType": "sprinkles"})), 255) > 20);
    assert!(count(&run(json!({"intensity": 90, "grainType": "speckle"})), 0) > 20);
    let stippled = run(json!({"intensity": 100, "grainType": "stippled"}));
    assert_eq!(count(&stippled, 0) + count(&stippled, 255), w * h, "stippled is black and white");
    // Horizontal streaks run along the rows: neighbours across them differ far more than along.
    let streaks = run(json!({"intensity": 80, "grainType": "horizontal"}));
    let along =
        (0..h).flat_map(|y| (0..w - 1).map(move |x| (x, y))).filter(|&(x, y)| at(&streaks, w, x, y)[2] != at(&streaks, w, x + 1, y)[2]).count();
    let across =
        (0..h - 1).flat_map(|y| (0..w).map(move |x| (x, y))).filter(|&(x, y)| at(&streaks, w, x, y)[2] != at(&streaks, w, x, y + 1)[2]).count();
    assert!(across > 3 * along, "across {across}, along {along}");
}

/// Mosaic Tiles lightens the grout and leaves the middle of each tile as it was.
#[test]
fn mosaic_tiles_set_the_art_in_light_grout() {
    let (w, h) = (48, 48);
    let mut d = image(w, h, |_, _| grey(100));
    fx("texture.mosaicTiles", json!({"tileSize": 12, "groutWidth": 5})).apply(&mut d, w, h, &space(w, h));
    assert_eq!(at(&d, w, 18, 18), grey(100), "the middle of a tile is flat");
    assert!(at(&d, w, 24, 18)[0] > 150, "grout between tiles: {:?}", at(&d, w, 24, 18));
    assert_eq!(at(&d, w, 24, 18)[3], 255);
}

/// Patchwork makes flat squares of each square's average colour; relief only shades their edges.
#[test]
fn patchwork_makes_squares_of_their_average_colour() {
    let (w, h) = (48, 48);
    let src = ramp(w, h);
    let mut flat = src.clone();
    fx("texture.patchwork", json!({"squareSize": 4, "relief": 0})).apply(&mut flat, w, h, &space(w, h));
    for (x, y) in [(6, 6), (7, 11), (11, 7)] {
        assert_eq!(at(&flat, w, x, y), at(&flat, w, 9, 9), "one square, one colour");
    }
    assert_ne!(at(&flat, w, 9, 9), at(&flat, w, 15, 9));
    let mut raised = src;
    fx("texture.patchwork", json!({"squareSize": 4, "relief": 25})).apply(&mut raised, w, h, &space(w, h));
    assert_eq!(at(&raised, w, 9, 9), at(&flat, w, 9, 9), "a square's middle is flat");
    assert!(differing(&raised, &flat) > w * h / 10);
}

/// Stained Glass leads flat panes with black.
#[test]
fn stained_glass_leads_flat_panes() {
    let (w, h) = (64, 64);
    let mut d = ramp(w, h);
    fx("texture.stainedGlass", json!({"cellSize": 12, "lightIntensity": 0})).apply(&mut d, w, h, &space(w, h));
    let lead = d.chunks(4).filter(|p| *p == [0, 0, 0, 255]).count();
    assert!(lead > w * h / 20 && lead < w * h / 2, "{lead} lead pixels");
    let colours: std::collections::HashSet<&[u8]> = d.chunks(4).collect();
    assert!(colours.len() < 80, "{} colours: the panes are flat", colours.len());
    let mut lit = ramp(w, h);
    fx("texture.stainedGlass", json!({"cellSize": 12, "lightIntensity": 10})).apply(&mut lit, w, h, &space(w, h));
    let sum = |d: &[u8]| d.iter().map(|v| u64::from(*v)).sum::<u64>();
    assert!(sum(&lit) > sum(&d), "light brightens the panes");
}

/// Texturizer shades the art by its surface; no relief leaves it alone.
#[test]
fn texturizer_shades_the_art_by_its_surface() {
    let (w, h) = (48, 48);
    let src = image(w, h, |_, _| grey(128));
    let run = |p: serde_json::Value| {
        let mut d = src.clone();
        fx("texture.texturizer", p).apply(&mut d, w, h, &space(w, h));
        d
    };
    assert_eq!(run(json!({"relief": 0})), src);
    for surface in Surface::KINDS.map(|k| k.0) {
        let d = run(json!({"texture": surface, "relief": 20}));
        assert!(differing(&d, &src) > w * h / 10, "{surface}");
        assert_ne!(d, run(json!({"texture": surface, "relief": 20, "invert": true})), "{surface} inverted");
        assert_ne!(d, run(json!({"texture": surface, "relief": 20, "light": "bottom"})), "{surface} lit from below");
    }
}

#[test]
fn brush_stroke_params_take_defaults_and_stay_in_range() {
    let b = |id: &str, p: serde_json::Value| match fx(id, p) {
        PixelFx::Brush(b) => b,
        other => panic!("{id}: {other:?}"),
    };
    assert_eq!(b("brushStrokes.accentedEdges", json!({})), BrushStroke::AccentedEdges { width: 2.0, brightness: 38.0, smoothness: 5.0 });
    assert_eq!(
        b("brushStrokes.crosshatch", json!({"strength": 9, "strokeLength": 1})),
        BrushStroke::Crosshatch { length: 3.0, sharpness: 6.0, strength: 3 }
    );
    assert_eq!(
        b("brushStrokes.sprayedStrokes", json!({"strokeDirection": "Vertical", "sprayRadius": -1})),
        BrushStroke::SprayedStrokes { length: 12.0, radius: 0.0, direction: Direction::Vertical }
    );
    assert_eq!(b("brushStrokes.sumiE", json!({"contrast": 1e9})), BrushStroke::SumiE { width: 10.0, pressure: 2.0, contrast: 40.0 });
    assert_eq!(fx("brushStrokes.spatter", json!({"sprayRadius": 8})).outset(Rect::ZERO), 4.0);
    assert_eq!(fx("brushStrokes.angledStrokes", json!({"strokeLength": 20})).reach(), Some(20.0));
    assert_eq!(fx("brushStrokes.accentedEdges", json!({})).outset(Rect::ZERO), 0.0);
}

/// Stripes to stroke: alternating red and blue bands 6 px wide.
fn bands(w: usize, h: usize) -> Vec<u8> {
    image(w, h, |x, _| if (x / 6) % 2 == 0 { [220, 40, 40, 255] } else { [30, 40, 200, 255] })
}

/// Accented Edges lightens or darkens the edges, by its brightness, and leaves flat areas alone.
#[test]
fn accented_edges_light_or_darken_the_edges() {
    let (w, h) = (48, 24);
    let src = bands(w, h);
    let run = |brightness: f64| {
        let mut d = src.clone();
        fx("brushStrokes.accentedEdges", json!({"edgeBrightness": brightness, "edgeWidth": 1, "smoothness": 1})).apply(&mut d, w, h, &space(w, h));
        d
    };
    let sum = |p: [u8; 4]| u32::from(p[0]) + u32::from(p[1]) + u32::from(p[2]);
    let (light, dark) = (run(50.0), run(0.0));
    assert!(sum(at(&light, w, 12, 12)) > sum(at(&src, w, 12, 12)) + 60, "{:?}", at(&light, w, 12, 12));
    assert!(sum(at(&dark, w, 12, 12)) + 60 < sum(at(&src, w, 12, 12)), "{:?}", at(&dark, w, 12, 12));
    let flat = (at(&light, w, 9, 12), at(&src, w, 9, 12));
    assert!(flat.0.iter().zip(flat.1).all(|(a, b)| a.abs_diff(b) <= 2), "flat areas stay: {flat:?}");
    assert_eq!(run(25.0), src, "25 leaves the edges as they are");
}

/// The stroke filters smear colour along their strokes, keep a flat colour flat and leave
/// nothing transparent inside the art.
#[test]
fn stroke_filters_smear_along_their_strokes() {
    let (w, h) = (48, 48);
    let src = bands(w, h);
    for id in [
        "brushStrokes.angledStrokes",
        "brushStrokes.crosshatch",
        "brushStrokes.inkOutlines",
        "brushStrokes.sprayedStrokes",
        "brushStrokes.sumiE",
        "brushStrokes.darkStrokes",
        "brushStrokes.spatter",
    ] {
        let mut d = src.clone();
        fx(id, json!({})).apply(&mut d, w, h, &space(w, h));
        assert!(differing(&d, &src) > w * h / 4, "{id} strokes the bands");
        let mid: Vec<[u8; 4]> = (18..30).flat_map(|y| (18..30).map(move |x| (x, y))).map(|(x, y)| at(&d, w, x, y)).collect();
        assert!(mid.iter().all(|p| p[3] == 255), "{id}: the middle stays opaque");
        // Colours in between the bands' appear where strokes cross them.
        assert!(mid.iter().any(|p| p[0] > 60 && p[0] < 190), "{id} mixes the bands");
    }
    let flat = image(w, h, |_, _| [90, 150, 60, 255]);
    for id in ["brushStrokes.angledStrokes", "brushStrokes.sprayedStrokes", "brushStrokes.spatter"] {
        let mut d = flat.clone();
        fx(id, json!({"sharpness": 0})).apply(&mut d, w, h, &space(w, h));
        assert_eq!(at(&d, w, 24, 24), [90, 150, 60, 255], "{id} keeps a flat colour");
    }
}

/// Dark Strokes darkens the dark tones and lightens the light ones.
#[test]
fn dark_strokes_push_the_tones_apart() {
    let (w, h) = (48, 24);
    let src = image(w, h, |x, _| if x < 24 { grey(60) } else { grey(200) });
    let mut d = src.clone();
    fx("brushStrokes.darkStrokes", json!({"blackIntensity": 10, "whiteIntensity": 10})).apply(&mut d, w, h, &space(w, h));
    assert!(at(&d, w, 6, 12)[0] < 40, "{:?}", at(&d, w, 6, 12));
    assert!(at(&d, w, 40, 12)[0] > 220, "{:?}", at(&d, w, 40, 12));
}

/// Spatter scatters further with a larger radius and not at all at 0.
#[test]
fn spatter_scatters_by_its_radius() {
    let (w, h) = (48, 48);
    let src = bands(w, h);
    let changed = |r: f64| {
        let mut d = src.clone();
        fx("brushStrokes.spatter", json!({"sprayRadius": r})).apply(&mut d, w, h, &space(w, h));
        differing(&d, &src)
    };
    assert_eq!(changed(0.0), 0);
    assert!(changed(25.0) > changed(4.0), "{} then {}", changed(4.0), changed(25.0));
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
        ("video.deInterlace", json!({})),
        ("brushStrokes.accentedEdges", json!({})),
        ("brushStrokes.angledStrokes", json!({})),
        ("brushStrokes.crosshatch", json!({"strength": 3})),
        ("brushStrokes.darkStrokes", json!({})),
        ("brushStrokes.inkOutlines", json!({})),
        ("brushStrokes.spatter", json!({})),
        ("brushStrokes.sprayedStrokes", json!({})),
        ("brushStrokes.sumiE", json!({})),
        ("texture.craquelure", json!({})),
        ("texture.grain", json!({})),
        ("texture.mosaicTiles", json!({})),
        ("texture.patchwork", json!({})),
        ("texture.stainedGlass", json!({})),
        ("texture.texturizer", json!({})),
        ("video.ntscColors", json!({})),
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
