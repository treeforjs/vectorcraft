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
    assert_eq!(fx("blur.smart", json!({})), PixelFx::SmartBlur { radius: 3.0, threshold: 25.0, samples: 7, mode: SmartMode::Normal });
    assert_eq!(
        fx("blur.smart", json!({"radius": "1e999", "threshold": -1, "quality": "low"})),
        PixelFx::SmartBlur { radius: 3.0, threshold: 0.1, samples: 5, mode: SmartMode::Normal }
    );
    assert_eq!(fx("sharpen.unsharpMask", json!({})), PixelFx::UnsharpMask { amount: 0.5, radius: 1.0, threshold: 0.0 });
    assert_eq!(
        fx("sharpen.unsharpMask", json!({"amount": 1e9, "radius": 0, "threshold": 1e9})),
        PixelFx::UnsharpMask { amount: 5.0, radius: 0.1, threshold: 255.0 }
    );
    assert_eq!(fx("stylize.glowingEdges", json!({})), PixelFx::GlowingEdges { width: 2.0, brightness: 6.0, smoothness: 5.0 });
    assert_eq!(fx("pixelate.mosaic", json!({"cellSize": 1e308})), PixelFx::Mosaic { cell_size: 200.0 });
    assert_eq!(fx("pixelate.mosaic", json!({"cellSize": "1e999"})), PixelFx::Mosaic { cell_size: 10.0 });
    assert_eq!(
        fx("pixelate.colorHalftone", json!({"radius": -1e308, "angle1": -45, "angle2": 450, "angle3": null})),
        PixelFx::ColorHalftone { radius: 2.0, angles: [315.0, 90.0, 90.0] }
    );
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
        for p in [
            json!({"amount": 1e308, "radius": 1e308, "threshold": 1e308, "cellSize": 1e308, "angle1": 1e308, "quality": "best"}),
            json!({"amount": 0, "radius": 0, "cellSize": -1e308, "mode": "edgeOnly"}),
        ] {
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
fn smart_blur_modes_detect_transitions_without_outlining_transparency() {
    let (w, h) = (24, 8);
    let source = image(w, h, |x, _| {
        if x < 2 {
            [0; 4]
        } else if x < 12 {
            [40, 20, 0, 128]
        } else {
            [100, 100, 100, 128]
        }
    });
    let mut edge = source.clone();
    fx("blur.smart", json!({"radius": 2, "threshold": 30, "mode": "edgeOnly"})).apply(&mut edge, w, h, &space(w, h));
    assert_eq!(at(&edge, w, 11, 4), [128; 4], "contrast produces a white edge at the original alpha");
    assert_eq!(at(&edge, w, 7, 4), [0, 0, 0, 128], "flat colours become black");
    assert_eq!(at(&edge, w, 2, 4), [0, 0, 0, 128], "transparency does not become a colour transition");
    assert_eq!(at(&edge, w, 0, 4), [0; 4]);
    let mut overlay = source.clone();
    fx("blur.smart", json!({"radius": 2, "threshold": 30, "mode": "overlayEdge"})).apply(&mut overlay, w, h, &space(w, h));
    assert_eq!(at(&overlay, w, 11, 4), [128; 4]);
    assert_eq!(at(&overlay, w, 7, 4), at(&source, w, 7, 4), "overlay retains the original colour away from transitions");
    let mut old = source.clone();
    let mut explicit = source.clone();
    fx("blur.smart", json!({})).apply(&mut old, w, h, &space(w, h));
    fx("blur.smart", json!({"mode": "normal"})).apply(&mut explicit, w, h, &space(w, h));
    assert_eq!(old, explicit, "old files without a mode keep Normal behavior");
}

#[test]
fn mosaic_averages_covered_colours_and_preserves_alpha() {
    let (w, h) = (8, 4);
    let mut data = image(w, h, |x, _| match x {
        2 => [100, 0, 0, 100],
        4 => [0, 0, 200, 200],
        _ => [0; 4],
    });
    fx("pixelate.mosaic", json!({"cellSize": 4})).apply(&mut data, w, h, &space(w, h));
    assert_eq!(at(&data, w, 2, 2), [33, 0, 67, 100]);
    assert_eq!(at(&data, w, 4, 2), [67, 0, 133, 200]);
    assert_eq!(at(&data, w, 3, 2), [0; 4], "holes retain zero coverage");
}

#[test]
fn pixelate_grid_is_unchanged_by_crop_translation_or_resolution() {
    let (w, h) = (100, 100);
    let source = image(w, h, |x, y| [((x * 17 + y * 7) % 256) as u8, ((x * 3 + y * 11) % 256) as u8, 60, 255]);
    for id in [
        "pixelate.mosaic",
        "pixelate.colorHalftone",
        "pixelate.crystallize",
        "pixelate.pointillize",
        "pixelate.fragment",
        "pixelate.mezzotint",
        "pixelate.facet",
    ] {
        let effect = fx(id, json!({"cellSize": 8, "radius": 6}));
        let mut whole = source.clone();
        effect.apply(&mut whole, w, h, &space(w, h));
        let mut crop = image(80, 80, |x, y| at(&source, w, x + 10, y + 10));
        let cropped_space = PixelSpace { to_doc: Affine::translate((10.0, 10.0)), ..space(w, h) };
        effect.apply(&mut crop, 80, 80, &cropped_space);
        for y in 40..60 {
            for x in 40..60 {
                assert_eq!(at(&whole, w, x, y), at(&crop, 80, x - 10, y - 10), "{id} resets phase or averages an incomplete cell");
            }
        }
        let mut translated = source.clone();
        let offset = vectorcraft_geom::Vec2::new(17.25, -13.75);
        effect.apply(&mut translated, w, h, &PixelSpace { to_doc: Affine::translate(offset), center: space(w, h).center + offset, px: 1.0 });
        assert_eq!(whole, translated, "{id} follows a translated object without changing its pattern");
    }
    let mut one = source.clone();
    let mut two = image(w * 2, h * 2, |x, y| at(&source, w, x / 2, y / 2));
    let effect = fx("pixelate.mosaic", json!({"cellSize": 8}));
    effect.apply(&mut one, w, h, &space(w, h));
    effect.apply(&mut two, w * 2, h * 2, &PixelSpace { to_doc: Affine::scale(0.5), px: 0.5, ..space(w, h) });
    for y in 0..h {
        for x in 0..w {
            assert_eq!(at(&one, w, x, y), at(&two, w * 2, x * 2, y * 2), "Mosaic cell size must be in document units");
        }
    }
}

#[test]
fn halftone_preserves_solid_channels_and_responds_to_screen_angles() {
    let (w, h) = (128, 128);
    for color in [[0, 0, 0, 255], [255; 4], [128, 0, 128, 128]] {
        let mut data = image(w, h, |_, _| color);
        fx("pixelate.colorHalftone", json!({})).apply(&mut data, w, h, &space(w, h));
        assert!(data.as_chunks::<4>().0.iter().all(|p| *p == color), "solid channel/alpha changed: {color:?}");
    }
    let original = image(w, h, |_, _| grey(96));
    let mut a = original.clone();
    let mut b = original.clone();
    fx("pixelate.colorHalftone", json!({"radius": 8.0 * std::f64::consts::SQRT_2, "angle1": 0, "angle2": 0, "angle3": 0})).apply(
        &mut a,
        w,
        h,
        &space(w, h),
    );
    fx("pixelate.colorHalftone", json!({"radius": 8.0 * std::f64::consts::SQRT_2, "angle1": 45, "angle2": 0, "angle3": 0})).apply(
        &mut b,
        w,
        h,
        &space(w, h),
    );
    assert!(a.as_chunks::<4>().0.iter().zip(b.as_chunks::<4>().0).any(|(a, b)| a[0] != b[0]));
    assert!(a.as_chunks::<4>().0.iter().zip(b.as_chunks::<4>().0).all(|(a, b)| a[1..] == b[1..]), "one screen angle changes only its channel");
    let mean = a.as_chunks::<4>().0.iter().map(|p| f64::from(p[0])).sum::<f64>() / (w * h) as f64;
    assert!((mean - 96.0).abs() < 4.0, "dot coverage should preserve the cell's mean tone: {mean}");
}

#[test]
fn crystal_seeds_and_pointillist_background_are_effective_and_repeatable() {
    let (w, h) = (60, 60);
    let source = image(w, h, |x, y| [((x * 4) % 128) as u8, ((y * 4) % 128) as u8, 0, 128]);
    for id in ["pixelate.crystallize", "pixelate.pointillize"] {
        let mut a = source.clone();
        let mut b = source.clone();
        fx(id, json!({"cellSize": 12, "seed": 1})).apply(&mut a, w, h, &space(w, h));
        fx(id, json!({"cellSize": 12, "seed": 2})).apply(&mut b, w, h, &space(w, h));
        assert_ne!(a, b, "{id} seed should choose different sites");
        assert!(a.as_chunks::<4>().0.iter().all(|p| p[3] == 128));
    }
    let mut dots = image(w, h, |_, _| [128, 0, 0, 128]);
    fx("pixelate.pointillize", json!({"cellSize": 12, "backgroundColor": "#0000ff"})).apply(&mut dots, w, h, &space(w, h));
    assert!(dots.as_chunks::<4>().0.contains(&[128, 0, 0, 128]), "dot interiors retain source colour");
    assert!(dots.as_chunks::<4>().0.contains(&[0, 0, 128, 128]), "spaces between dots take the selected background");
    assert!(dots.as_chunks::<4>().0.iter().all(|p| p[0] <= p[3] && p[1] == 0 && p[2] <= p[3] && p[3] == 128));
}

#[test]
fn fragment_places_four_copies_and_accounts_for_alpha_reach() {
    let (w, h) = (21, 21);
    let mut data = image(w, h, |x, y| if x == 10 && y == 10 { [200, 0, 0, 200] } else { [0; 4] });
    let effect = fx("pixelate.fragment", json!({"offset": 3}));
    effect.apply(&mut data, w, h, &space(w, h));
    for (x, y) in [(7, 7), (13, 7), (7, 13), (13, 13)] {
        assert_eq!(at(&data, w, x, y), [50, 0, 0, 50], "each translated copy has one quarter coverage");
    }
    assert_eq!(at(&data, w, 10, 10), [0; 4]);
    assert_eq!(data.as_chunks::<4>().0.iter().map(|p| u32::from(p[3])).sum::<u32>(), 200);
    assert_eq!(effect.outset(Rect::new(0.0, 0.0, 10.0, 10.0)), 3.0);
    assert!(effect.reach().unwrap() >= 3.0 * std::f64::consts::SQRT_2);
}

#[test]
fn mezzotint_types_and_seeds_produce_binary_neutral_or_saturated_colours() {
    let (w, h) = (160, 160);
    let source = image(w, h, |_, _| [80, 40, 80, 128]);
    let mut previous = None;
    for kind in [
        "fineDots",
        "mediumDots",
        "grainyDots",
        "coarseDots",
        "shortStrokes",
        "mediumStrokes",
        "longStrokes",
        "shortLines",
        "mediumLines",
        "longLines",
    ] {
        let mut data = source.clone();
        fx("pixelate.mezzotint", json!({"type": kind, "seed": 12})).apply(&mut data, w, h, &space(w, h));
        assert!(data.as_chunks::<4>().0.iter().all(|p| p[3] == 128 && p[..3].iter().all(|v| *v == 0 || *v == 128) && p[0] == p[2]));
        if let Some(last) = previous {
            assert_ne!(data, last, "{kind} changes the grain dimensions");
        }
        previous = Some(data);
    }
    let mut grey_image = image(w, h, |_, _| grey(100));
    fx("pixelate.mezzotint", json!({})).apply(&mut grey_image, w, h, &space(w, h));
    assert!(grey_image.as_chunks::<4>().0.iter().all(|p| p[0] == p[1] && p[1] == p[2]));
    let mean = grey_image.as_chunks::<4>().0.iter().map(|p| f64::from(p[0])).sum::<f64>() / (w * h) as f64;
    assert!((mean - 100.0).abs() < 5.0, "dither preserves expected tone: {mean}");
}

#[test]
fn facet_groups_similar_colours_without_mixing_high_contrast_regions() {
    let (w, h) = (8, 8);
    let mut data = image(w, h, |x, _| {
        grey(if x < 2 {
            80
        } else if x < 4 {
            90
        } else {
            210
        })
    });
    fx("pixelate.facet", json!({"cellSize": 8, "levels": 8})).apply(&mut data, w, h, &space(w, h));
    for x in 0..4 {
        assert_eq!(at(&data, w, x, 3), grey(85), "similar colours in the cell become one flat colour");
    }
    assert_eq!(at(&data, w, 5, 3), grey(210), "the high-contrast region stays separate");
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

/// Timings on a 2000 × 2000 raster by default. VECTORCRAFT_PIXEL_SIDE (1..2900),
/// VECTORCRAFT_PIXEL_PX (0.01..100) and VECTORCRAFT_PIXEL_FILTER select small/full-resolution
/// or downsampled worst-case runs. Run this test executable under `/usr/bin/time -v` for peak RSS.
#[test]
#[ignore]
fn pixel_timings() {
    let size = std::env::var("VECTORCRAFT_PIXEL_SIDE").ok().and_then(|s| s.parse::<usize>().ok()).unwrap_or(2000).clamp(1, 2900);
    let px =
        std::env::var("VECTORCRAFT_PIXEL_PX").ok().and_then(|s| s.parse::<f64>().ok()).filter(|n| n.is_finite()).unwrap_or(1.0).clamp(0.01, 100.0);
    let filter = std::env::var("VECTORCRAFT_PIXEL_FILTER").ok();
    let (w, h) = (size, size);
    let src = image(w, h, |x, y| [(x % 256) as u8, (y % 256) as u8, ((x ^ y) % 256) as u8, 255]);
    let s = PixelSpace { to_doc: Affine::scale(px), px, center: Point::new(w as f64 * px / 2.0, h as f64 * px / 2.0) };
    let cases = [
        ("blur.radial", json!({"quality": "draft"})),
        ("blur.radial", json!({"quality": "good"})),
        ("blur.radial", json!({"quality": "best", "method": "zoom"})),
        ("blur.smart", json!({"quality": "low"})),
        ("blur.smart", json!({"quality": "high", "radius": 30})),
        ("sharpen.unsharpMask", json!({"radius": 20})),
        ("pixelate.mosaic", json!({"cellSize": 10})),
        ("pixelate.colorHalftone", json!({"radius": 8})),
        ("pixelate.crystallize", json!({"cellSize": 10})),
        ("pixelate.pointillize", json!({"cellSize": 10})),
        ("pixelate.fragment", json!({"offset": 2})),
        ("pixelate.mezzotint", json!({"type": "fineDots"})),
        ("pixelate.facet", json!({"cellSize": 3})),
    ];
    for (id, p) in cases {
        if filter.as_ref().is_some_and(|wanted| wanted != id) {
            continue;
        }
        let mut d = src.clone();
        let t = std::time::Instant::now();
        fx(id, p.clone()).apply(&mut d, w, h, &s);
        println!("{id} {p} {w}x{h} {px}pt/px: {:?}", t.elapsed());
    }
}
