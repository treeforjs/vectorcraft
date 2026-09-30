//! Line breaking and glyph placement for point, area and on-path type.

use std::ops::Range;

use drawcraft_doc::{CharStyle, Justify, ParaStyle, TextKind, TextObject};
use kurbo::{Affine, BezPath, ParamCurve, ParamCurveArclen, PathEl, PathSeg, Point, Rect, Shape, Vec2};

use crate::fontdb::FontDb;
use crate::shape::{SGlyph, shape_range, style_metrics};
use crate::{LineInfo, PositionedGlyph, TextLayout};

const EPS: f64 = 1e-6;

struct Ctx<'a> {
    db: &'a FontDb,
    text: &'a str,
    runs: Vec<(Range<usize>, &'a CharStyle)>,
    default: CharStyle,
    out: TextLayout,
}

impl Ctx<'_> {
    fn style_at(&self, b: usize) -> &CharStyle {
        let find = |b: usize| self.runs.iter().find(|(r, _)| r.start <= b && b < r.end).map(|(_, s)| *s);
        find(b).or_else(|| b.checked_sub(1).and_then(find)).or_else(|| self.runs.first().map(|(_, s)| *s)).unwrap_or(&self.default)
    }

    fn shape_para(&self, r: Range<usize>) -> Vec<SGlyph> {
        let mut v = Vec::with_capacity(r.len());
        shape_range(self.db, self.text, r, &self.runs, &mut v);
        v
    }

    fn emit(&mut self, g: &SGlyph, pre: Affine, origin: Point, angle: f64, advance: f64, line: usize) {
        let local = Affine::rotate(-g.rotation.to_radians()) * Affine::translate((g.dx, g.dy - g.bshift)) * Affine::scale_non_uniform(g.sx, g.sy);
        let src = self.db.outline(&g.face, g.gid);
        let outline = if src.elements().is_empty() {
            BezPath::new()
        } else {
            let mut p = (*src).clone();
            p.apply_affine(pre * local);
            p
        };
        self.out.glyphs.push(PositionedGlyph { outline, run: g.run, byte: g.byte, origin, advance, len: g.len, angle, line, font_id: g.face.id() });
    }
}

/// Lay out a text object into text-space glyph outlines.
pub fn layout(db: &FontDb, t: &TextObject) -> TextLayout {
    let text = t.plain_text();
    let mut runs = Vec::with_capacity(t.runs.len());
    let mut off = 0;
    for r in &t.runs {
        runs.push((off..off + r.text.len(), &r.style));
        off += r.text.len();
    }
    let mut paras = Vec::new();
    let mut s = 0;
    for (i, c) in text.char_indices() {
        if c == '\n' {
            paras.push(s..i);
            s = i + 1;
        }
    }
    paras.push(s..text.len());
    let mut cx = Ctx { db, text: &text, runs, default: CharStyle::default(), out: TextLayout::default() };
    match &t.kind {
        TextKind::Point => flow(&mut cx, &paras, &t.para, None),
        TextKind::Area { frame } => {
            let region = Region::new(&frame.to_bezpath());
            flow(&mut cx, &paras, &t.para, Some(&region));
        }
        TextKind::OnPath { path, start } => on_path(&mut cx, &paras, &t.para, &path.to_bezpath(), *start, path.is_closed()),
    }
    finish_bounds(&mut cx.out);
    cx.out
}

fn finish_bounds(out: &mut TextLayout) {
    let mut b: Option<Rect> = None;
    let mut add = |r: Rect| b = Some(b.map_or(r, |b| b.union(r)));
    for g in &out.glyphs {
        if !g.outline.elements().is_empty() {
            add(g.outline.bounding_box());
        }
    }
    if !out.on_path {
        for l in &out.lines {
            add(Rect::new(l.x0.min(l.x1), l.baseline - l.ascent, l.x0.max(l.x1), l.baseline + l.descent));
        }
    }
    out.bounds = b.unwrap_or_default();
}

/// A flattened area-type frame.
struct Region {
    bbox: Rect,
    polys: Vec<Vec<Point>>,
}

impl Region {
    fn new(path: &BezPath) -> Self {
        let mut polys: Vec<Vec<Point>> = Vec::new();
        kurbo::flatten(path, 0.1, |el| match el {
            PathEl::MoveTo(p) => polys.push(vec![p]),
            PathEl::LineTo(p) => {
                if let Some(v) = polys.last_mut() {
                    v.push(p);
                }
            }
            _ => {}
        });
        polys.retain(|p| p.len() >= 3);
        Self { bbox: path.bounding_box(), polys }
    }

    /// Inside intervals (even-odd) of the horizontal line at `y`.
    fn intervals(&self, y: f64) -> Vec<(f64, f64)> {
        let mut xs = Vec::new();
        for poly in &self.polys {
            for i in 0..poly.len() {
                let a = poly[i];
                let b = poly[(i + 1) % poly.len()];
                if (a.y <= y) != (b.y <= y) {
                    xs.push(a.x + (y - a.y) / (b.y - a.y) * (b.x - a.x));
                }
            }
        }
        xs.sort_by(f64::total_cmp);
        xs.chunks_exact(2).map(|c| (c[0], c[1])).collect()
    }

    /// Widest horizontal span inside the frame over the band `top..bottom`.
    fn span(&self, top: f64, bottom: f64) -> Option<(f64, f64)> {
        if self.polys.is_empty() {
            return (self.bbox.width() > 0.0).then_some((self.bbox.x0, self.bbox.x1));
        }
        let clamp = |y: f64| y.clamp(self.bbox.y0 + 1e-4, self.bbox.y1 - 1e-4);
        let mid = self.intervals(clamp((top + bottom) * 0.5));
        let others = [self.intervals(clamp(top)), self.intervals(clamp(bottom))];
        mid.into_iter()
            .filter_map(|(mut a, mut b)| {
                for o in &others {
                    let best =
                        o.iter().map(|&(c, d)| (a.max(c), b.min(d))).filter(|(c, d)| d > c).max_by(|x, y| (x.1 - x.0).total_cmp(&(y.1 - y.0)))?;
                    a = best.0;
                    b = best.1;
                }
                (b > a).then_some((a, b))
            })
            .max_by(|x, y| (x.1 - x.0).total_cmp(&(y.1 - y.0)))
    }
}

/// Greedy break: returns the end glyph index for a line starting at `i` of the given width.
fn break_line(g: &[SGlyph], i: usize, width: f64) -> usize {
    if !width.is_finite() {
        return g.len();
    }
    let mut x = 0.0;
    let mut last_break = None;
    let mut j = i;
    while j < g.len() {
        let gl = &g[j];
        if j > i && !gl.is_space() && x + gl.adv > width + EPS {
            break;
        }
        x += gl.adv;
        if gl.break_after() {
            last_break = Some(j + 1);
        }
        j += 1;
    }
    if j >= g.len() {
        return g.len();
    }
    // Don't separate a cluster's glyphs.
    let mut end = match last_break {
        Some(b) if b > i => b,
        _ => j,
    };
    while end > i + 1 && end < g.len() && g[end].byte == g[end - 1].byte {
        end -= 1;
    }
    end
}

fn max_metrics(g: &[SGlyph]) -> Option<(f64, f64, f64)> {
    (!g.is_empty()).then(|| g.iter().fold((0.0f64, 0.0f64, 0.0f64), |m, g| (m.0.max(g.ascent), m.1.max(g.descent), m.2.max(g.leading))))
}

fn flow(cx: &mut Ctx<'_>, paras: &[Range<usize>], para: &ParaStyle, region: Option<&Region>) {
    let mut prev_baseline: Option<f64> = None;
    let mut pending_space = 0.0;
    'paras: for (pi, pr) in paras.iter().enumerate() {
        let sg = cx.shape_para(pr.clone());
        let pm = style_metrics(cx.db, cx.style_at(pr.start));
        if pi > 0 {
            pending_space += para.space_before;
        }
        let n = sg.len();
        let mut i = 0;
        let mut first_line = true;
        loop {
            let est = if i < n { (sg[i].ascent, sg[i].descent, sg[i].leading) } else { pm };
            let first_ind = if first_line { para.first_line_indent } else { 0.0 };
            let ind_l = para.left_indent + first_ind;
            let mut baseline = match (prev_baseline, region) {
                (None, Some(r)) => r.bbox.y0 + est.0,
                (None, None) => 0.0,
                (Some(b), _) => b + est.2 + pending_space,
            };
            let (x0, x1) = match region {
                None => (f64::NEG_INFINITY, f64::INFINITY),
                Some(r) => loop {
                    if baseline + est.1 > r.bbox.y1 + 0.01 {
                        cx.out.overflow = cx.text.len() > if i < n { sg[i].byte } else { pr.start };
                        break 'paras;
                    }
                    match r.span(baseline - est.0, baseline + est.1) {
                        Some((a, b)) if b - a - ind_l - para.right_indent > est.0.max(1.0) => break (a, b),
                        _ => baseline += est.2.max(1.0),
                    }
                },
            };
            let (ax0, ax1) = (x0 + ind_l, x1 - para.right_indent);
            let width = ax1 - ax0;
            let end = if i < n { break_line(&sg, i, width) } else { n };
            let m = max_metrics(&sg[i..end]).unwrap_or(pm);
            baseline += if prev_baseline.is_none() { m.0 - est.0 } else { m.2 - est.2 };
            if let Some(r) = region
                && baseline + m.1 > r.bbox.y1 + 0.01
            {
                cx.out.overflow = cx.text.len() > if i < n { sg[i].byte } else { pr.start };
                break 'paras;
            }
            pending_space = 0.0;
            let last_of_para = end >= n;
            let mut trimmed = end;
            while trimmed > i && sg[trimmed - 1].is_space() {
                trimmed -= 1;
            }
            let w: f64 = sg[i..trimmed].iter().map(|g| g.adv).sum();
            let (align, justify) = match para.justify {
                Justify::Left => (0, false),
                Justify::Center => (1, false),
                Justify::Right => (2, false),
                Justify::JustifyLeft => (0, !last_of_para),
                Justify::JustifyCenter => (1, !last_of_para),
                Justify::JustifyRight => (2, !last_of_para),
                Justify::JustifyAll => (0, true),
            };
            let justify = justify && region.is_some();
            let (mut per_space, mut per_gap) = (0.0, 0.0);
            if justify && width - w > EPS {
                let spaces = sg[i..trimmed].iter().filter(|g| g.is_space()).count();
                if spaces > 0 {
                    per_space = (width - w) / spaces as f64;
                } else if para.justify == Justify::JustifyAll && trimmed - i > 1 {
                    per_gap = (width - w) / (trimmed - i - 1) as f64;
                }
            }
            let start_x = if justify {
                ax0
            } else if region.is_none() {
                match align {
                    0 => ind_l,
                    1 => (ind_l - para.right_indent - w) * 0.5,
                    _ => -para.right_indent - w,
                }
            } else {
                match align {
                    0 => ax0,
                    1 => ax0 + (width - w) * 0.5,
                    _ => ax1 - w,
                }
            };
            let li = cx.out.lines.len();
            let glyph_start = cx.out.glyphs.len();
            let mut x = start_x;
            let mut x_end = start_x;
            for (j, g) in sg.iter().enumerate().take(end).skip(i) {
                let mut adv = g.adv;
                if j < trimmed {
                    if g.is_space() {
                        adv += per_space;
                    } else if j + 1 < trimmed {
                        adv += per_gap;
                    }
                }
                cx.emit(g, Affine::translate((x, baseline)), Point::new(x, baseline), 0.0, adv, li);
                x += adv;
                if j + 1 == trimmed {
                    x_end = x;
                }
            }
            cx.out.lines.push(LineInfo {
                baseline,
                x0: start_x,
                x1: x_end,
                ascent: m.0,
                descent: m.1,
                start: if i < n { sg[i].byte } else { pr.start },
                end: if last_of_para { pr.end } else { sg[end].byte },
                glyph_start,
                glyph_end: cx.out.glyphs.len(),
            });
            prev_baseline = Some(baseline);
            first_line = false;
            i = end;
            if i >= n {
                break;
            }
        }
        pending_space += para.space_after;
    }
}

/// Arc-length parameterised path.
struct ArcPath {
    segs: Vec<(PathSeg, f64, f64)>,
    len: f64,
}

impl ArcPath {
    fn new(p: &BezPath) -> Self {
        let mut segs = Vec::new();
        let mut cum = 0.0;
        for s in p.segments() {
            let l = s.arclen(1e-4);
            if l > 1e-9 {
                segs.push((s, cum, l));
                cum += l;
            }
        }
        Self { segs, len: cum }
    }

    /// Point and unit tangent at arc length `s`.
    fn at(&self, s: f64) -> (Point, Vec2) {
        let s = s.clamp(0.0, self.len);
        let i = self.segs.partition_point(|(_, c, _)| *c <= s).saturating_sub(1);
        let (seg, c, l) = self.segs[i];
        let t = seg.inv_arclen((s - c).min(l), 1e-4).clamp(0.0, 1.0);
        let p = seg.eval(t);
        let (t0, t1) = ((t - 1e-4).max(0.0), (t + 1e-4).min(1.0));
        let d = seg.eval(t1) - seg.eval(t0);
        let len = d.hypot();
        (p, if len > 1e-12 { d / len } else { Vec2::new(1.0, 0.0) })
    }
}

fn on_path(cx: &mut Ctx<'_>, paras: &[Range<usize>], para: &ParaStyle, path: &BezPath, start: f64, closed: bool) {
    cx.out.on_path = true;
    let mut sg = Vec::new();
    for pr in paras {
        sg.extend(cx.shape_para(pr.clone()));
    }
    let ap = ArcPath::new(path);
    let m = max_metrics(&sg).unwrap_or_else(|| style_metrics(cx.db, cx.style_at(0)));
    let text_len = cx.text.len();
    if ap.segs.is_empty() {
        cx.out.overflow = !sg.is_empty();
        cx.out.lines.push(LineInfo {
            baseline: 0.0,
            x0: 0.0,
            x1: 0.0,
            ascent: m.0,
            descent: m.1,
            start: 0,
            end: text_len,
            glyph_start: 0,
            glyph_end: 0,
        });
        return;
    }
    let s_start = start.clamp(0.0, 1.0) * ap.len;
    let avail = if closed { ap.len } else { ap.len - s_start };
    let w: f64 = sg.iter().map(|g| g.adv).sum();
    let s0 = match para.justify {
        Justify::Center | Justify::JustifyCenter => s_start + ((avail - w) * 0.5).max(0.0),
        Justify::Right | Justify::JustifyRight => s_start + (avail - w).max(0.0),
        _ => s_start,
    };
    let mut x = 0.0;
    for g in &sg {
        let s = s0 + x;
        if s + g.adv - s_start > avail + 1e-6 {
            cx.out.overflow = true;
            break;
        }
        let mut mid = s + g.adv * 0.5;
        if closed {
            mid = mid.rem_euclid(ap.len);
        }
        let (p, dir) = ap.at(mid);
        let angle = dir.y.atan2(dir.x);
        let pre = Affine::translate(p.to_vec2()) * Affine::rotate(angle) * Affine::translate((-g.adv * 0.5, 0.0));
        cx.emit(g, pre, p - dir * (g.adv * 0.5), angle, g.adv, 0);
        x += g.adv;
    }
    let (ps, _) = ap.at(if closed { s0.rem_euclid(ap.len) } else { s0 });
    let (pe, _) = ap.at(if closed { (s0 + x).rem_euclid(ap.len) } else { s0 + x });
    cx.out.lines.push(LineInfo {
        baseline: ps.y,
        x0: ps.x,
        x1: pe.x,
        ascent: m.0,
        descent: m.1,
        start: 0,
        end: text_len,
        glyph_start: 0,
        glyph_end: cx.out.glyphs.len(),
    });
}
