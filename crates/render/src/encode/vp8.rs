//! Lossy WebP: the image as one VP8 key frame (RFC 6386), with its transparency, if any, in a
//! losslessly compressed alpha plane.
//!
//! Each 16 × 16 macroblock is predicted whole (DC, vertical, horizontal or TrueMotion, whichever
//! is closest to the source), and its chroma likewise; the residual goes through the 4 × 4 DCT
//! (and the luma DCs through the Walsh–Hadamard transform), is quantized at one step size for the
//! frame and coded with the boolean entropy coder. The image is encoded twice: once to count the
//! tokens, so the frame can update the token probabilities that pay for themselves, then for real.
//! Prediction reads the encoder's own reconstruction, which is exactly what a decoder rebuilds.
//! Memory stays proportional to the width (one row of macroblocks at a time).

use super::vp8_tables::{AC_QUANT, COEFF_PROBS, COEFF_UPDATE_PROBS, DC_QUANT, TokenProbs};

/// Most pixels a side VP8 stores (14 bits).
pub const MAX_SIDE: u32 = 16383;

/// Coefficient order in the bitstream (§13).
const ZIGZAG: [usize; 16] = [0, 1, 4, 8, 5, 2, 3, 6, 9, 12, 13, 10, 7, 11, 14, 15];
/// The band of each coefficient position, which picks its probabilities (§13.3).
const BANDS: [usize; 16] = [0, 1, 2, 3, 6, 4, 5, 6, 6, 6, 6, 6, 6, 6, 6, 7];
/// The extra-bit probabilities of DCT_CAT1…DCT_CAT6 and the smallest value of each (§13.2).
const CAT_PROBS: [&[u8]; 6] = [
    &[159],
    &[165, 145],
    &[173, 148, 140],
    &[176, 155, 140, 135],
    &[180, 157, 141, 134, 130],
    &[254, 254, 243, 230, 196, 177, 153, 140, 133, 130, 129],
];
const CAT_BASE: [i32; 6] = [5, 7, 11, 19, 35, 67];
/// The largest coefficient level the tokens can code.
const MAX_LEVEL: i32 = 2048;
/// The first partition's size is stored in 19 bits.
const MAX_FIRST_PARTITION: usize = (1 << 19) - 1;

/// Token planes (§13.3).
const PLANE_Y_AFTER_Y2: usize = 0;
const PLANE_Y2: usize = 1;
const PLANE_UV: usize = 2;

/// A 16 × 16 (or 8 × 8 chroma) prediction mode, in the order the bitstream numbers them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Dc,
    V,
    H,
    Tm,
}

/// Encode straight-alpha RGBA `w` × `h` pixels as a lossy WebP file at `quality` 0–100 (100 the
/// finest quantizer). Transparency is kept exactly, in a lossless alpha plane.
pub fn encode(rgba: &[u8], w: u32, h: u32, quality: u8) -> Result<Vec<u8>, String> {
    if w == 0 || h == 0 || w > MAX_SIDE || h > MAX_SIDE {
        return Err(format!("WebP images are 1 to {MAX_SIDE} pixels a side (this one is {w} × {h})"));
    }
    let n = (w as usize).checked_mul(h as usize).and_then(|n| n.checked_mul(4)).ok_or("WebP encoding failed: the image is too large")?;
    let px = rgba.get(..n).ok_or("WebP encoding failed: pixel buffer doesn't match the image size")?;
    let src = Source { px, w: w as usize, h: h as usize };
    let vp8 = frame(&src, quality)?;
    let alpha = src.has_alpha().then(|| alpha_chunk(&src)).transpose()?;
    Ok(container(&vp8, alpha.as_deref(), w, h))
}

/// The straight-alpha source pixels.
struct Source<'a> {
    px: &'a [u8],
    w: usize,
    h: usize,
}

impl Source<'_> {
    /// The RGBA pixel at (x, y), the edge pixel repeated past the right and bottom edges.
    fn at(&self, x: usize, y: usize) -> [u8; 4] {
        let (x, y) = (x.min(self.w - 1), y.min(self.h - 1));
        let i = (y * self.w + x) * 4;
        match self.px.get(i..i + 4) {
            Some(&[r, g, b, a]) => [r, g, b, a],
            _ => [0; 4],
        }
    }
    fn has_alpha(&self) -> bool {
        self.px.chunks_exact(4).any(|p| p.get(3).is_some_and(|&a| a < 255))
    }
}

/// BT.601 studio-range luma and chroma, as VP8 stores colour.
fn yuv([r, g, b, _]: [u8; 4]) -> (i32, i32, i32) {
    let (r, g, b) = (i32::from(r), i32::from(g), i32::from(b));
    let y = ((66 * r + 129 * g + 25 * b + 128) >> 8) + 16;
    // Chroma at 4× precision, for averaging 2 × 2 pixels.
    let u = (-38 * r - 74 * g + 112 * b) + (128 << 8);
    let v = (112 * r - 94 * g - 18 * b) + (128 << 8);
    (y, u, v)
}

/// One row of macroblocks of the source in YUV: 16 luma rows, 8 rows of each chroma plane.
struct Row {
    y: Vec<u8>,
    u: Vec<u8>,
    v: Vec<u8>,
}

impl Row {
    fn read(src: &Source, mby: usize, mbw: usize) -> Self {
        let (yw, cw) = (mbw * 16, mbw * 8);
        let mut row = Row { y: vec![0; yw * 16], u: vec![0; cw * 8], v: vec![0; cw * 8] };
        for cy in 0..8 {
            for cx in 0..cw {
                let (mut su, mut sv) = (0, 0);
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let (x, ly) = (cx * 2 + dx, cy * 2 + dy);
                    let (l, u, v) = yuv(src.at(x, mby * 16 + ly));
                    if let Some(p) = row.y.get_mut(ly * yw + x) {
                        *p = l.clamp(0, 255) as u8;
                    }
                    su += u;
                    sv += v;
                }
                // Four samples at ×256: divide by 1024 with rounding.
                let c = |s: i32| ((s + 512) >> 10).clamp(0, 255) as u8;
                if let (Some(u), Some(v)) = (row.u.get_mut(cy * cw + cx), row.v.get_mut(cy * cw + cx)) {
                    *u = c(su);
                    *v = c(sv);
                }
            }
        }
        row
    }
}

/// The quantizer steps of a frame (§9.6, §14.1); the luma DCs go through Y2.
#[derive(Clone, Copy)]
struct Quant {
    y_ac: i32,
    y2_dc: i32,
    y2_ac: i32,
    uv_dc: i32,
    uv_ac: i32,
}

impl Quant {
    fn new(index: usize) -> Self {
        let i = index.min(127);
        let (dc, ac) = (DC_QUANT[i], AC_QUANT[i]);
        Self { y_ac: ac, y2_dc: dc * 2, y2_ac: (ac * 155 / 100).max(8), uv_dc: dc.min(132), uv_ac: ac }
    }
}

/// The quantizer index for `quality` 0–100.
fn quant_index(quality: u8) -> usize {
    let q = u32::from(quality.min(100));
    ((100 - q) * 127 / 100) as usize
}

/// A coefficient quantized at step `q` (DCs round to nearest, the rest towards zero a little).
fn quantize(c: i32, q: i32, dc: bool) -> i32 {
    let q = q.max(1);
    let bias = if dc { q / 2 } else { q * 3 / 8 };
    let l = ((c.abs() + bias) / q).min(MAX_LEVEL);
    if c < 0 { -l } else { l }
}

// ---------- Transforms (§14.3, §14.4, and their forward counterparts) ----------

fn fdct(d: &[i32; 16]) -> [i32; 16] {
    let mut t = [0i32; 16];
    for i in 0..4 {
        let r = [d[i * 4], d[i * 4 + 1], d[i * 4 + 2], d[i * 4 + 3]];
        let a1 = (r[0] + r[3]) * 8;
        let b1 = (r[1] + r[2]) * 8;
        let c1 = (r[1] - r[2]) * 8;
        let d1 = (r[0] - r[3]) * 8;
        t[i * 4] = a1 + b1;
        t[i * 4 + 2] = a1 - b1;
        t[i * 4 + 1] = (c1 * 2217 + d1 * 5352 + 14500) >> 12;
        t[i * 4 + 3] = (d1 * 2217 - c1 * 5352 + 7500) >> 12;
    }
    let mut o = [0i32; 16];
    for i in 0..4 {
        let a1 = t[i] + t[12 + i];
        let b1 = t[4 + i] + t[8 + i];
        let c1 = t[4 + i] - t[8 + i];
        let d1 = t[i] - t[12 + i];
        o[i] = (a1 + b1 + 7) >> 4;
        o[8 + i] = (a1 - b1 + 7) >> 4;
        o[4 + i] = ((c1 * 2217 + d1 * 5352 + 12000) >> 16) + i32::from(d1 != 0);
        o[12 + i] = (d1 * 2217 - c1 * 5352 + 51000) >> 16;
    }
    o
}

fn idct(b: &mut [i32; 16]) {
    const C1: i64 = 20091;
    const C2: i64 = 35468;
    let mut t = [0i64; 16];
    for i in 0..4 {
        let (x0, x1, x2, x3) = (i64::from(b[i]), i64::from(b[4 + i]), i64::from(b[8 + i]), i64::from(b[12 + i]));
        let a1 = x0 + x2;
        let b1 = x0 - x2;
        let c1 = ((x1 * C2) >> 16) - (x3 + ((x3 * C1) >> 16));
        let d1 = (x1 + ((x1 * C1) >> 16)) + ((x3 * C2) >> 16);
        t[i] = a1 + d1;
        t[4 + i] = b1 + c1;
        t[8 + i] = b1 - c1;
        t[12 + i] = a1 - d1;
    }
    for i in 0..4 {
        let (x0, x1, x2, x3) = (t[4 * i], t[4 * i + 1], t[4 * i + 2], t[4 * i + 3]);
        let a1 = x0 + x2;
        let b1 = x0 - x2;
        let c1 = ((x1 * C2) >> 16) - (x3 + ((x3 * C1) >> 16));
        let d1 = (x1 + ((x1 * C1) >> 16)) + ((x3 * C2) >> 16);
        b[4 * i] = ((a1 + d1 + 4) >> 3) as i32;
        b[4 * i + 3] = ((a1 - d1 + 4) >> 3) as i32;
        b[4 * i + 1] = ((b1 + c1 + 4) >> 3) as i32;
        b[4 * i + 2] = ((b1 - c1 + 4) >> 3) as i32;
    }
}

fn fwht(d: &[i32; 16]) -> [i32; 16] {
    let mut t = [0i32; 16];
    for i in 0..4 {
        let r = [d[i * 4], d[i * 4 + 1], d[i * 4 + 2], d[i * 4 + 3]];
        let a1 = (r[0] + r[2]) * 4;
        let d1 = (r[1] + r[3]) * 4;
        let c1 = (r[1] - r[3]) * 4;
        let b1 = (r[0] - r[2]) * 4;
        t[i * 4] = a1 + d1 + i32::from(a1 != 0);
        t[i * 4 + 1] = b1 + c1;
        t[i * 4 + 2] = b1 - c1;
        t[i * 4 + 3] = a1 - d1;
    }
    let mut o = [0i32; 16];
    for i in 0..4 {
        let a1 = t[i] + t[8 + i];
        let d1 = t[4 + i] + t[12 + i];
        let c1 = t[4 + i] - t[12 + i];
        let b1 = t[i] - t[8 + i];
        let r = |x: i32| (x + i32::from(x < 0) + 3) >> 3;
        o[i] = r(a1 + d1);
        o[4 + i] = r(b1 + c1);
        o[8 + i] = r(b1 - c1);
        o[12 + i] = r(a1 - d1);
    }
    o
}

fn iwht(b: &mut [i32; 16]) {
    let mut t = [0i32; 16];
    for i in 0..4 {
        let a1 = b[i] + b[12 + i];
        let b1 = b[4 + i] + b[8 + i];
        let c1 = b[4 + i] - b[8 + i];
        let d1 = b[i] - b[12 + i];
        t[i] = a1 + b1;
        t[4 + i] = c1 + d1;
        t[8 + i] = a1 - b1;
        t[12 + i] = d1 - c1;
    }
    for i in 0..4 {
        let a1 = t[4 * i] + t[4 * i + 3];
        let b1 = t[4 * i + 1] + t[4 * i + 2];
        let c1 = t[4 * i + 1] - t[4 * i + 2];
        let d1 = t[4 * i] - t[4 * i + 3];
        b[4 * i] = (a1 + b1 + 3) >> 3;
        b[4 * i + 1] = (c1 + d1 + 3) >> 3;
        b[4 * i + 2] = (a1 - b1 + 3) >> 3;
        b[4 * i + 3] = (d1 - c1 + 3) >> 3;
    }
}

// ---------- Prediction ----------

/// What a block's prediction can see of its reconstructed neighbours.
struct Edges<'a> {
    /// The row above (`n` pixels), when there is one.
    above: Option<&'a [u8]>,
    /// The column to the left (`n` pixels), when there is one.
    left: Option<&'a [u8]>,
    /// The pixel above-left (with both).
    corner: u8,
}

impl Edges<'_> {
    /// The modes this block may use: DC always, the others when the pixels they read exist.
    fn modes(&self) -> Vec<Mode> {
        let mut m = vec![Mode::Dc];
        if self.above.is_some() {
            m.push(Mode::V);
        }
        if self.left.is_some() {
            m.push(Mode::H);
        }
        if self.above.is_some() && self.left.is_some() {
            m.push(Mode::Tm);
        }
        m
    }

    /// The `n` × `n` prediction for `mode` (row-major).
    fn predict(&self, mode: Mode, n: usize) -> Vec<u8> {
        let at = |s: Option<&[u8]>, i: usize| s.and_then(|s| s.get(i)).map_or(128, |&v| i32::from(v));
        let mut out = vec![128u8; n * n];
        match mode {
            Mode::Dc => {
                let mut sum = 0;
                let mut shift = if n == 8 { 2 } else { 3 };
                if self.left.is_some() {
                    sum += (0..n).map(|i| at(self.left, i)).sum::<i32>();
                    shift += 1;
                }
                if self.above.is_some() {
                    sum += (0..n).map(|i| at(self.above, i)).sum::<i32>();
                    shift += 1;
                }
                let dc = if self.left.is_none() && self.above.is_none() { 128 } else { (sum + (1 << (shift - 1))) >> shift };
                out.fill(dc.clamp(0, 255) as u8);
            }
            Mode::V => {
                for (i, p) in out.iter_mut().enumerate() {
                    *p = at(self.above, i % n) as u8;
                }
            }
            Mode::H => {
                for (i, p) in out.iter_mut().enumerate() {
                    *p = at(self.left, i / n) as u8;
                }
            }
            Mode::Tm => {
                let c = i32::from(self.corner);
                for (i, p) in out.iter_mut().enumerate() {
                    *p = (at(self.left, i / n) + at(self.above, i % n) - c).clamp(0, 255) as u8;
                }
            }
        }
        out
    }
}

/// The prediction mode whose prediction is closest (least squared error) to `src` (`n` × `n`,
/// or the sum over several planes).
fn best_mode(planes: &[(&Edges, &[u8])], n: usize) -> Mode {
    let Some((first, _)) = planes.first() else { return Mode::Dc };
    let mut best = (u64::MAX, Mode::Dc);
    for mode in first.modes() {
        let err: u64 = planes
            .iter()
            .map(|(e, src)| {
                e.predict(mode, n).iter().zip(src.iter()).map(|(&p, &s)| (i64::from(p) - i64::from(s)).unsigned_abs().pow(2)).sum::<u64>()
            })
            .sum();
        if err < best.0 {
            best = (err, mode);
        }
    }
    best.1
}

// ---------- Macroblocks ----------

/// One coded macroblock: its modes and quantized coefficient levels (natural order).
struct Mb {
    y_mode: Mode,
    uv_mode: Mode,
    y2: [i32; 16],
    y: [[i32; 16]; 16],
    u: [[i32; 16]; 4],
    v: [[i32; 16]; 4],
}

impl Mb {
    fn skip(&self) -> bool {
        let zero = |b: &[i32; 16]| b.iter().all(|&c| c == 0);
        zero(&self.y2) && self.y.iter().all(zero) && self.u.iter().all(zero) && self.v.iter().all(zero)
    }
}

/// The reconstruction a decoder holds: the bottom row of the macroblock row above, the right
/// column of the macroblock to the left and the pixel above-left of the current one, per plane.
struct Recon {
    top: Vec<u8>,
    left: Vec<u8>,
    corner: u8,
    /// Block size (16 luma, 8 chroma).
    n: usize,
}

impl Recon {
    fn new(width: usize, n: usize) -> Self {
        Self { top: vec![0; width], left: vec![0; n], corner: 0, n }
    }
    fn edges(&self, mbx: usize, mby: usize) -> Edges<'_> {
        let x0 = mbx * self.n;
        Edges { above: (mby > 0).then(|| self.top.get(x0..x0 + self.n)).flatten(), left: (mbx > 0).then_some(&self.left[..]), corner: self.corner }
    }
    /// Keep macroblock `mbx`'s reconstruction `rec` (`n` × `n`) for its neighbours.
    fn store(&mut self, mbx: usize, rec: &[u8]) {
        let (n, x0) = (self.n, mbx * self.n);
        // The next macroblock's above-left pixel is this one's above-right, before it's replaced.
        self.corner = self.top.get(x0 + n - 1).copied().unwrap_or(0);
        for (i, l) in self.left.iter_mut().enumerate() {
            *l = rec.get(i * n + n - 1).copied().unwrap_or(0);
        }
        if let (Some(top), Some(bottom)) = (self.top.get_mut(x0..x0 + n), rec.get((n - 1) * n..n * n)) {
            top.copy_from_slice(bottom);
        }
    }
    /// A new row of macroblocks: nothing to the left yet.
    fn new_row(&mut self) {
        self.left.fill(0);
        self.corner = 0;
    }
}

/// The `n` × `n` block at macroblock column `mbx` of plane rows `plane` (`stride` wide).
fn block_of(plane: &[u8], stride: usize, mbx: usize, n: usize) -> Vec<u8> {
    (0..n).flat_map(|y| (0..n).map(move |x| (y, x))).map(|(y, x)| plane.get(y * stride + mbx * n + x).copied().unwrap_or(0)).collect()
}

/// The 4 × 4 sub-block `b` (row-major among the `n / 4` × `n / 4`) of an `n` × `n` block.
fn sub(px: &[u8], n: usize, b: usize) -> [i32; 16] {
    let per = n / 4;
    let (bx, by) = (b % per * 4, b / per * 4);
    std::array::from_fn(|i| i32::from(px.get((by + i / 4) * n + bx + i % 4).copied().unwrap_or(0)))
}

/// Add the 4 × 4 residual `r` to sub-block `b` of `px` (`n` wide), clamped.
fn add_sub(px: &mut [u8], n: usize, b: usize, r: &[i32; 16]) {
    let per = n / 4;
    let (bx, by) = (b % per * 4, b / per * 4);
    for (i, &d) in r.iter().enumerate() {
        if let Some(p) = px.get_mut((by + i / 4) * n + bx + i % 4) {
            *p = (i32::from(*p) + d).clamp(0, 255) as u8;
        }
    }
}

/// Code chroma plane `src` (8 × 8) from prediction `pred`: its levels, and `pred` becomes the
/// reconstruction.
fn code_chroma(src: &[u8], pred: &mut [u8], q: &Quant) -> [[i32; 16]; 4] {
    let mut levels = [[0i32; 16]; 4];
    for (b, lv) in levels.iter_mut().enumerate() {
        let (s, p) = (sub(src, 8, b), sub(pred, 8, b));
        let c = fdct(&std::array::from_fn(|i| s[i] - p[i]));
        for i in 0..16 {
            lv[i] = quantize(c[i], if i == 0 { q.uv_dc } else { q.uv_ac }, i == 0);
        }
        if lv.iter().any(|&l| l != 0) {
            let mut r: [i32; 16] = std::array::from_fn(|i| lv[i] * if i == 0 { q.uv_dc } else { q.uv_ac });
            idct(&mut r);
            add_sub(pred, 8, b, &r);
        }
    }
    levels
}

/// Code macroblock `mbx` of `row`, updating the reconstruction.
fn code_mb(row: &Row, mbx: usize, mby: usize, mbw: usize, rec: &mut [Recon; 3], q: &Quant) -> Mb {
    let src_y = block_of(&row.y, mbw * 16, mbx, 16);
    let src_u = block_of(&row.u, mbw * 8, mbx, 8);
    let src_v = block_of(&row.v, mbw * 8, mbx, 8);
    let [ry, ru, rv] = rec;
    let (ey, eu, ev) = (ry.edges(mbx, mby), ru.edges(mbx, mby), rv.edges(mbx, mby));
    let y_mode = best_mode(&[(&ey, &src_y)], 16);
    let uv_mode = best_mode(&[(&eu, &src_u), (&ev, &src_v)], 8);
    let mut pred_y = ey.predict(y_mode, 16);
    let mut pred_u = eu.predict(uv_mode, 8);
    let mut pred_v = ev.predict(uv_mode, 8);
    // Luma: 16 DCTs, their DCs through the WHT.
    let mut coeffs = [[0i32; 16]; 16];
    for (b, c) in coeffs.iter_mut().enumerate() {
        let (s, p) = (sub(&src_y, 16, b), sub(&pred_y, 16, b));
        *c = fdct(&std::array::from_fn(|i| s[i] - p[i]));
    }
    let dcs = fwht(&std::array::from_fn(|b| coeffs[b][0]));
    let y2: [i32; 16] = std::array::from_fn(|i| quantize(dcs[i], if i == 0 { q.y2_dc } else { q.y2_ac }, i == 0));
    let mut y = [[0i32; 16]; 16];
    for (lv, c) in y.iter_mut().zip(&coeffs) {
        for i in 1..16 {
            lv[i] = quantize(c[i], q.y_ac, false);
        }
    }
    // Reconstruct as a decoder does.
    let mut rdc: [i32; 16] = std::array::from_fn(|i| y2[i] * if i == 0 { q.y2_dc } else { q.y2_ac });
    iwht(&mut rdc);
    for (b, lv) in y.iter().enumerate() {
        let mut r: [i32; 16] = std::array::from_fn(|i| if i == 0 { rdc[b] } else { lv[i] * q.y_ac });
        if r.iter().any(|&v| v != 0) {
            idct(&mut r);
            add_sub(&mut pred_y, 16, b, &r);
        }
    }
    let u = code_chroma(&src_u, &mut pred_u, q);
    let v = code_chroma(&src_v, &mut pred_v, q);
    ry.store(mbx, &pred_y);
    ru.store(mbx, &pred_u);
    rv.store(mbx, &pred_v);
    Mb { y_mode, uv_mode, y2, y, u, v }
}

// ---------- Entropy coding ----------

/// Where boolean decisions go: a coder, or a tally of the token decisions.
trait Sink {
    /// A decision at probability `prob` (of a zero).
    fn put(&mut self, prob: u8, bit: bool);
    /// A token tree decision at probability slot (plane, band, context, node).
    fn token(&mut self, slot: (usize, usize, usize, usize), prob: u8, bit: bool) {
        let _ = slot;
        self.put(prob, bit);
    }
}

/// The boolean entropy encoder (§7.3).
struct BoolWriter {
    out: Vec<u8>,
    range: u32,
    bottom: u32,
    bit_count: i32,
}

impl BoolWriter {
    fn new() -> Self {
        Self { out: Vec::new(), range: 255, bottom: 0, bit_count: 24 }
    }
    fn carry(&mut self) {
        for b in self.out.iter_mut().rev() {
            if *b == 255 {
                *b = 0;
            } else {
                *b += 1;
                return;
            }
        }
    }
    fn literal(&mut self, v: u32, bits: u32) {
        for i in (0..bits).rev() {
            self.put(128, (v >> i) & 1 == 1);
        }
    }
    fn finish(mut self) -> Vec<u8> {
        let mut c = self.bit_count;
        let mut v = self.bottom;
        if v & 1u32.checked_shl((32 - c) as u32).unwrap_or(0) != 0 {
            self.carry();
        }
        v = v.wrapping_shl((c & 7) as u32);
        c >>= 3;
        while c > 0 {
            c -= 1;
            v = v.wrapping_shl(8);
        }
        for _ in 0..4 {
            self.out.push((v >> 24) as u8);
            v = v.wrapping_shl(8);
        }
        self.out
    }
}

impl Sink for BoolWriter {
    fn put(&mut self, prob: u8, bit: bool) {
        let split = 1 + (((self.range - 1) * u32::from(prob)) >> 8);
        if bit {
            self.bottom = self.bottom.wrapping_add(split);
            self.range -= split;
        } else {
            self.range = split;
        }
        while self.range < 128 {
            self.range <<= 1;
            if self.bottom & (1 << 31) != 0 {
                self.carry();
            }
            self.bottom = self.bottom.wrapping_shl(1);
            self.bit_count -= 1;
            if self.bit_count == 0 {
                self.out.push((self.bottom >> 24) as u8);
                self.bottom &= (1 << 24) - 1;
                self.bit_count = 8;
            }
        }
    }
}

/// How often each token probability slot decided 0 and 1.
/// A value per token probability slot (plane, band, context, node).
type PerSlot<T> = [[[[T; 11]; 3]; 8]; 4];

struct Tally(Box<PerSlot<[u64; 2]>>);

impl Sink for Tally {
    fn put(&mut self, _: u8, _: bool) {}
    fn token(&mut self, (p, b, c, n): (usize, usize, usize, usize), _: u8, bit: bool) {
        if let Some(t) = self.0.get_mut(p).and_then(|t| t.get_mut(b)).and_then(|t| t.get_mut(c)).and_then(|t| t.get_mut(n)) {
            t[usize::from(bit)] += 1;
        }
    }
}

/// Code one block's levels (natural order) from position `first`, in context `ctx`; true when
/// it has a coefficient (its neighbours' context).
fn block_tokens<S: Sink>(s: &mut S, probs: &TokenProbs, plane: usize, ctx: usize, levels: &[i32; 16], first: usize) -> bool {
    let zz: [i32; 16] = std::array::from_fn(|i| levels[ZIGZAG[i]]);
    let Some(last) = (first..16).rev().find(|&i| zz[i] != 0) else {
        let band = BANDS[first];
        s.token((plane, band, ctx, 0), probs[plane][band][ctx.min(2)][0], false);
        return false;
    };
    let mut ctx = ctx.min(2);
    let mut after_zero = false;
    for (i, &v) in zz.iter().enumerate().take(last + 1).skip(first) {
        let band = BANDS[i];
        let p = &probs[plane][band][ctx];
        let node = |s: &mut S, n: usize, bit: bool| s.token((plane, band, ctx, n), p[n], bit);
        if !after_zero {
            node(s, 0, true);
        }
        let a = v.abs();
        if a == 0 {
            node(s, 1, false);
            after_zero = true;
            ctx = 0;
            continue;
        }
        node(s, 1, true);
        if a == 1 {
            node(s, 2, false);
        } else {
            node(s, 2, true);
            if a <= 4 {
                node(s, 3, false);
                if a == 2 {
                    node(s, 4, false);
                } else {
                    node(s, 4, true);
                    node(s, 5, a == 4);
                }
            } else {
                node(s, 3, true);
                let cat = CAT_BASE.iter().rposition(|&b| a >= b).unwrap_or(0);
                match cat {
                    0 | 1 => {
                        node(s, 6, false);
                        node(s, 7, cat == 1);
                    }
                    2 | 3 => {
                        node(s, 6, true);
                        node(s, 8, false);
                        node(s, 9, cat == 3);
                    }
                    _ => {
                        node(s, 6, true);
                        node(s, 8, true);
                        node(s, 10, cat == 5);
                    }
                }
                if let (Some(&bits), Some(&base)) = (CAT_PROBS.get(cat), CAT_BASE.get(cat)) {
                    let extra = (a - base).max(0);
                    for (k, &prob) in bits.iter().enumerate() {
                        s.put(prob, (extra >> (bits.len() - 1 - k)) & 1 == 1);
                    }
                }
            }
        }
        s.put(128, v < 0);
        after_zero = false;
        ctx = if a == 1 { 1 } else { 2 };
    }
    if last < 15 {
        let band = BANDS[last + 1];
        s.token((plane, band, ctx, 0), probs[plane][band][ctx][0], false);
    }
    true
}

/// The token contexts of the macroblocks around the current one (§13.3): per column above and
/// for the row to the left, Y2, four luma, two U and two V flags.
struct Contexts {
    top: Vec<[bool; 9]>,
    left: [bool; 9],
}

/// Code macroblock `mb`'s tokens (it isn't skipped).
fn mb_tokens(s: &mut impl Sink, probs: &TokenProbs, mb: &Mb, mbx: usize, cx: &mut Contexts) {
    let Some(top) = cx.top.get_mut(mbx) else { return };
    let left = &mut cx.left;
    let ctx = |a: bool, b: bool| usize::from(a) + usize::from(b);
    let nz = block_tokens(s, probs, PLANE_Y2, ctx(top[0], left[0]), &mb.y2, 0);
    top[0] = nz;
    left[0] = nz;
    for (b, lv) in mb.y.iter().enumerate() {
        let (x, y) = (b % 4, b / 4);
        let nz = block_tokens(s, probs, PLANE_Y_AFTER_Y2, ctx(top[1 + x], left[1 + y]), lv, 1);
        top[1 + x] = nz;
        left[1 + y] = nz;
    }
    for (base, plane) in [(5, &mb.u), (7, &mb.v)] {
        for (b, lv) in plane.iter().enumerate() {
            let (x, y) = (b % 2, b / 2);
            let nz = block_tokens(s, probs, PLANE_UV, ctx(top[base + x], left[base + y]), lv, 0);
            top[base + x] = nz;
            left[base + y] = nz;
        }
    }
}

/// Run the whole frame through `each(mbx, mb)`, macroblock by macroblock in raster order.
fn for_each_mb(src: &Source, q: &Quant, mut each: impl FnMut(usize, &Mb)) {
    let (mbw, mbh) = (src.w.div_ceil(16), src.h.div_ceil(16));
    let mut rec = [Recon::new(mbw * 16, 16), Recon::new(mbw * 8, 8), Recon::new(mbw * 8, 8)];
    for mby in 0..mbh {
        let row = Row::read(src, mby, mbw);
        rec.iter_mut().for_each(Recon::new_row);
        for mbx in 0..mbw {
            let mb = code_mb(&row, mbx, mby, mbw, &mut rec, q);
            each(mbx, &mb);
        }
    }
}

/// Bits to code `bit` at probability `prob` (of a zero), in 1/256ths of a bit.
fn cost(prob: u8, bit: bool) -> f64 {
    let p = f64::from(prob) / 256.0;
    -(if bit { 1.0 - p } else { p }).log2()
}

/// The token probabilities to code with: the defaults, updated where the tally says an update
/// saves more than it costs to send. Also gives which slots changed.
fn adapt(tally: &Tally) -> (TokenProbs, Box<PerSlot<bool>>) {
    let mut probs = COEFF_PROBS;
    let mut changed = Box::new([[[[false; 11]; 3]; 8]; 4]);
    for p in 0..4 {
        for b in 0..8 {
            for c in 0..3 {
                for n in 0..11 {
                    let [n0, n1] = tally.0[p][b][c][n];
                    let total = n0 + n1;
                    if total == 0 {
                        continue;
                    }
                    let new = ((n0 * 256 + total / 2) / total).clamp(1, 255) as u8;
                    let old = probs[p][b][c][n];
                    let bits = |pr: u8| n0 as f64 * cost(pr, false) + n1 as f64 * cost(pr, true);
                    let up = COEFF_UPDATE_PROBS[p][b][c][n];
                    let saving = bits(old) - bits(new) - 8.0 - (cost(up, true) - cost(up, false));
                    if saving > 0.0 {
                        probs[p][b][c][n] = new;
                        changed[p][b][c][n] = true;
                    }
                }
            }
        }
    }
    (probs, changed)
}

/// The loop filter level for quantizer index `qi` (a smoothing that grows with the step size).
fn filter_level(qi: usize) -> u32 {
    if qi < 6 { 0 } else { (qi * 5 / 8).min(63) as u32 }
}

/// The VP8 key frame of `src` at `quality`.
fn frame(src: &Source, quality: u8) -> Result<Vec<u8>, String> {
    let qi = quant_index(quality);
    let q = Quant::new(qi);
    let mbw = src.w.div_ceil(16);
    // Pass 1: tally the tokens and the skipped macroblocks.
    let mut tally = Tally(Box::new([[[[[0; 2]; 11]; 3]; 8]; 4]));
    let (mut skipped, mut total) = (0u64, 0u64);
    {
        let mut cx = Contexts { top: vec![[false; 9]; mbw], left: [false; 9] };
        for_each_mb(src, &q, |mbx, mb| {
            if mbx == 0 {
                cx.left = [false; 9];
            }
            total += 1;
            if mb.skip() {
                skipped += 1;
                reset(&mut cx, mbx);
            } else {
                mb_tokens(&mut tally, &COEFF_PROBS, mb, mbx, &mut cx);
            }
        });
    }
    let (probs, changed) = adapt(&tally);
    let skip_prob = ((total - skipped) * 256 / total.max(1)).clamp(1, 255) as u8;
    // Pass 2: the frame header and modes (first partition), the tokens (second).
    let mut head = BoolWriter::new();
    head.literal(0, 1); // colour space
    head.literal(0, 1); // clamping required
    head.literal(0, 1); // no segmentation
    head.literal(0, 1); // normal loop filter
    head.literal(filter_level(qi), 6);
    head.literal(0, 3); // sharpness
    head.literal(0, 1); // no loop filter deltas
    head.literal(0, 2); // one token partition
    head.literal(qi as u32, 7);
    for _ in 0..5 {
        head.literal(0, 1); // no quantizer deltas
    }
    head.literal(0, 1); // refresh entropy probs (unused for a single key frame)
    for p in 0..4 {
        for b in 0..8 {
            for c in 0..3 {
                for n in 0..11 {
                    let up = changed[p][b][c][n];
                    head.put(COEFF_UPDATE_PROBS[p][b][c][n], up);
                    if up {
                        head.literal(u32::from(probs[p][b][c][n]), 8);
                    }
                }
            }
        }
    }
    head.literal(1, 1); // macroblocks may skip
    head.literal(u32::from(skip_prob), 8);
    let mut tokens = BoolWriter::new();
    let mut cx = Contexts { top: vec![[false; 9]; mbw], left: [false; 9] };
    for_each_mb(src, &q, |mbx, mb| {
        if mbx == 0 {
            cx.left = [false; 9];
        }
        let skip = mb.skip();
        head.put(skip_prob, skip);
        let [a, b, c] = [145, 156, 163];
        head.put(a, true); // not B_PRED
        match mb.y_mode {
            Mode::Dc | Mode::V => {
                head.put(b, false);
                head.put(c, mb.y_mode == Mode::V);
            }
            Mode::H | Mode::Tm => {
                head.put(b, true);
                head.put(128, mb.y_mode == Mode::Tm);
            }
        }
        let [a, b, c] = [142, 114, 183];
        match mb.uv_mode {
            Mode::Dc => head.put(a, false),
            Mode::V => {
                head.put(a, true);
                head.put(b, false);
            }
            Mode::H | Mode::Tm => {
                head.put(a, true);
                head.put(b, true);
                head.put(c, mb.uv_mode == Mode::Tm);
            }
        }
        if skip {
            reset(&mut cx, mbx);
        } else {
            mb_tokens(&mut tokens, &probs, mb, mbx, &mut cx);
        }
    });
    let first = head.finish();
    if first.len() > MAX_FIRST_PARTITION {
        return Err("WebP encoding failed: the image is too large for one lossy frame; export it smaller or lossless".into());
    }
    let tokens = tokens.finish();
    let mut out = Vec::with_capacity(10 + first.len() + tokens.len());
    let tag = (first.len() as u32) << 5 | 1 << 4; // key frame, version 0, shown
    out.extend_from_slice(&tag.to_le_bytes()[..3]);
    out.extend_from_slice(&[0x9d, 0x01, 0x2a]);
    out.extend_from_slice(&(src.w as u16).to_le_bytes());
    out.extend_from_slice(&(src.h as u16).to_le_bytes());
    out.extend_from_slice(&first);
    out.extend_from_slice(&tokens);
    Ok(out)
}

/// A skipped macroblock leaves no coefficients: its contexts read as empty.
fn reset(cx: &mut Contexts, mbx: usize) {
    if let Some(t) = cx.top.get_mut(mbx) {
        *t = [false; 9];
    }
    cx.left = [false; 9];
}

/// The ALPH chunk's payload: the alpha plane as a lossless image stream (no filtering).
fn alpha_chunk(src: &Source) -> Result<Vec<u8>, String> {
    let alpha: Vec<u8> = src.px.chunks_exact(4).map(|p| p.get(3).copied().unwrap_or(255)).collect();
    let mut file = Vec::new();
    let enc = image::codecs::webp::WebPEncoder::new_lossless(&mut file);
    image::ImageEncoder::write_image(enc, &alpha, src.w as u32, src.h as u32, image::ExtendedColorType::L8)
        .map_err(|e| format!("WebP alpha encoding failed: {e}"))?;
    // The lossless bitstream follows its 5-byte header (signature, size, flags) in the VP8L chunk.
    let stream = chunk(&file, b"VP8L").and_then(|d| d.get(5..)).ok_or("WebP alpha encoding failed: no lossless stream")?;
    let mut out = Vec::with_capacity(stream.len() + 1);
    out.push(1); // lossless compression, no filtering or pre-processing
    out.extend_from_slice(stream);
    Ok(out)
}

/// The payload of the first `id` chunk of a RIFF WebP file.
fn chunk<'a>(file: &'a [u8], id: &[u8; 4]) -> Option<&'a [u8]> {
    let mut at = 12;
    while let Some(head) = file.get(at..at + 8) {
        let size = u32::from_le_bytes([head[4], head[5], head[6], head[7]]) as usize;
        let data = file.get(at + 8..at + 8 + size)?;
        if &head[..4] == id {
            return Some(data);
        }
        at += 8 + size + (size & 1);
    }
    None
}

/// The RIFF container: the VP8 frame alone, or VP8X + ALPH + VP8 with transparency.
fn container(vp8: &[u8], alpha: Option<&[u8]>, w: u32, h: u32) -> Vec<u8> {
    let mut body = b"WEBP".to_vec();
    let mut put = |id: &[u8; 4], data: &[u8]| {
        body.extend_from_slice(id);
        body.extend_from_slice(&(data.len() as u32).to_le_bytes());
        body.extend_from_slice(data);
        if data.len() % 2 == 1 {
            body.push(0);
        }
    };
    if let Some(a) = alpha {
        let mut x = vec![0x10, 0, 0, 0];
        x.extend_from_slice(&(w - 1).to_le_bytes()[..3]);
        x.extend_from_slice(&(h - 1).to_le_bytes()[..3]);
        put(b"VP8X", &x);
        put(b"ALPH", a);
    }
    put(b"VP8 ", vp8);
    let mut out = b"RIFF".to_vec();
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(&body);
    out
}
