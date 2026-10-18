//! Lossy WebP: files decode (with the image crate's independent decoder) close to the source.

use super::vp8::{MAX_SIDE, encode};

/// A `w`×`h` RGBA image of `f(x, y)`.
fn image(w: u32, h: u32, f: impl Fn(u32, u32) -> [u8; 4]) -> Vec<u8> {
    (0..h).flat_map(|y| (0..w).map(move |x| (x, y))).flat_map(|(x, y)| f(x, y)).collect()
}

/// Something like exported art: a gradient sky, a disc with a hard edge, stripes and a dot grid.
fn art(w: u32, h: u32) -> Vec<u8> {
    image(w, h, |x, y| {
        let (fx, fy) = (x as f64 / w as f64, y as f64 / h as f64);
        let d = ((fx - 0.4).powi(2) + (fy - 0.5).powi(2)).sqrt();
        if d < 0.25 {
            [220, 60, 40, 255]
        } else if (x / 6) % 2 == 0 && fy > 0.8 {
            [30, 30, 120, 255]
        } else if x % 9 == 0 && y % 9 == 0 {
            [255, 255, 255, 255]
        } else {
            [(40.0 + 150.0 * fx) as u8, (90.0 + 120.0 * fy) as u8, 200, 255]
        }
    })
}

fn decode(file: &[u8]) -> image::RgbaImage {
    image::load_from_memory_with_format(file, image::ImageFormat::WebP).unwrap().to_rgba8()
}

/// The luma of RGBA pixels, as grey RGBA (chroma is subsampled 2 × 2, so single-pixel colour
/// detail is lost by any 4:2:0 encoder; luma shows the coding quality).
fn luma(p: &[u8]) -> Vec<u8> {
    p.as_chunks::<4>()
        .0
        .iter()
        .flat_map(|c| {
            let y = ((66 * i32::from(c[0]) + 129 * i32::from(c[1]) + 25 * i32::from(c[2]) + 128) >> 8) as u8 + 16;
            [y, y, y, 255]
        })
        .collect()
}

/// A smooth two-way gradient.
fn smooth(w: u32, h: u32) -> Vec<u8> {
    image(w, h, |x, y| [(x * 2 % 256) as u8, (y * 3 % 256) as u8, 128, 255])
}

/// Peak signal-to-noise ratio of the colour channels, in dB.
fn psnr(a: &[u8], b: &[u8]) -> f64 {
    let (mut se, mut n) = (0.0, 0.0);
    for (p, q) in a.as_chunks::<4>().0.iter().zip(b.as_chunks::<4>().0.iter()) {
        for c in 0..3 {
            se += (f64::from(p[c]) - f64::from(q[c])).powi(2);
            n += 1.0;
        }
    }
    if se == 0.0 { f64::INFINITY } else { 10.0 * (255.0f64.powi(2) / (se / n)).log10() }
}

#[test]
fn lossy_files_decode_close_to_the_source_and_shrink_with_quality() {
    let (w, h) = (96, 64);
    let src = art(w, h);
    let mut sizes = Vec::new();
    for (q, min_luma, min_smooth) in [(100, 44.0, 46.0), (90, 41.0, 43.0), (60, 33.0, 37.0), (10, 25.0, 31.0)] {
        let file = encode(&src, w, h, q).unwrap();
        assert_eq!((&file[..4], &file[8..16]), (&b"RIFF"[..], &b"WEBPVP8 "[..]), "a plain lossy file");
        let img = decode(&file);
        assert_eq!(img.dimensions(), (w, h));
        let db = psnr(&luma(&src), &luma(img.as_raw()));
        assert!(db >= min_luma, "quality {q}: luma {db:.1} dB");
        let g = smooth(w, h);
        let db = psnr(&g, decode(&encode(&g, w, h, q).unwrap()).as_raw());
        assert!(db >= min_smooth, "quality {q}: gradient {db:.1} dB");
        assert!(img.pixels().all(|p| p.0[3] == 255), "opaque");
        sizes.push(file.len());
    }
    assert!(sizes.windows(2).all(|s| s[0] > s[1]), "lower quality, smaller file: {sizes:?}");
    // Photo-like content (a gradient with grain) is where lossy pays: a fraction of lossless.
    let grain = image(w, h, |x, y| {
        let n = ((x * 48 + y + 1).wrapping_mul(2_654_435_761) >> 28) as u8;
        [(x * 2) as u8 + n, (y * 3) as u8 + n, 128 + n, 255]
    });
    let lossless = super::webp(&crate::Rendered { width: w, height: h, pixels: grain.clone() }).unwrap();
    let lossy = encode(&grain, w, h, 75).unwrap();
    assert!(lossy.len() * 3 < lossless.len(), "lossy {} vs lossless {}", lossy.len(), lossless.len());
}

#[test]
fn transparency_stays_exact() {
    let (w, h) = (40, 30);
    let src = image(w, h, |x, y| if x < 20 { [10, 200, 90, 255] } else { [10, 200, 90, ((x * 7 + y * 3) % 256) as u8] });
    let file = encode(&src, w, h, 80).unwrap();
    assert_eq!(&file[12..16], b"VP8X");
    let img = decode(&file);
    for (p, q) in img.pixels().zip(src.as_chunks::<4>().0.iter()) {
        assert_eq!(p.0[3], q[3]);
    }
    // The colour of an opaque pixel survives too.
    let p = img.get_pixel(5, 5).0;
    assert!(p[..3].iter().zip([10, 200, 90]).all(|(&a, b): (&u8, i32)| (i32::from(a) - b).abs() <= 6), "{p:?}");
}

#[test]
fn odd_sizes_and_flat_images_encode() {
    for (w, h) in [(1, 1), (17, 33), (16, 16), (31, 2), (300, 1)] {
        let src = smooth(w, h);
        let img = decode(&encode(&src, w, h, 75).unwrap());
        assert_eq!(img.dimensions(), (w, h));
        assert!(psnr(&src, img.as_raw()) > 30.0, "{w}×{h}");
        assert_eq!(decode(&encode(&art(w, h), w, h, 75).unwrap()).dimensions(), (w, h));
    }
    let flat = image(64, 48, |_, _| [128, 64, 32, 255]);
    let file = encode(&flat, 64, 48, 90).unwrap();
    assert!(file.len() < 200, "a flat image is tiny: {}", file.len());
    let img = decode(&file);
    assert!(img.pixels().all(|p| p.0[..3].iter().zip([128, 64, 32]).all(|(&a, b): (&u8, i32)| (i32::from(a) - b).abs() <= 3)));
}

#[test]
fn noise_and_extremes_round_trip() {
    // Hostile content: full-range noise and saturated colours make large coefficients.
    let src = image(48, 48, |x, y| {
        let seed = (x * 48 + y + 1).wrapping_mul(2_654_435_761).wrapping_add(12345);
        let v = (seed >> 16) as u8;
        [v, 255 - v, if v > 127 { 255 } else { 0 }, 255]
    });
    for q in [0, 50, 100] {
        let img = decode(&encode(&src, 48, 48, q).unwrap());
        assert_eq!(img.dimensions(), (48, 48));
    }
}

#[test]
fn bad_sizes_are_errors() {
    assert!(encode(&[], 0, 1, 90).is_err());
    assert!(encode(&[0; 4], 1, 2, 90).is_err(), "short buffer");
    assert!(encode(&[0; 8], MAX_SIDE + 1, 1, 90).is_err());
}
