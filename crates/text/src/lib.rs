//! DrawCraft text: font database, shaping, layout, glyph outlines.
//!
//! API contract used by `drawcraft-render`, `drawcraft-tools` and the UI:
//! - [`FontDb::global`]: process-wide database preloaded with the bundled OFL fonts.
//! - [`layout`]: lays out a [`TextObject`] into glyph outlines in *text space* (apply `t.xf` to
//!   get document coordinates), plus line and caret information.
//! - [`caret_position`] / [`hit_byte`]: caret geometry and hit testing for the Type tool.
//!
//! Frames (`TextKind::Area`) and paths (`TextKind::OnPath`) are interpreted in text space.
#![forbid(unsafe_code)]

mod fontdb;
mod layout;
mod shape;

pub use drawcraft_doc::TextObject;
pub use fontdb::{FALLBACK_FAMILY, FontDb, FontFace};
use kurbo::{BezPath, Point, Rect, Vec2};
pub use layout::layout;

/// One laid-out glyph.
#[derive(Clone, Debug)]
pub struct PositionedGlyph {
    /// Outline in text space.
    pub outline: BezPath,
    /// Index of the run (`TextObject::runs`) the glyph came from.
    pub run: usize,
    /// Byte offset of the source character (cluster start) in the plain text.
    pub byte: usize,
    /// Pen position on the baseline (leading edge of the glyph).
    pub origin: Point,
    /// Advance along the baseline (includes tracking and justification space).
    pub advance: f64,
    /// Number of source bytes covered by the glyph's cluster.
    pub len: usize,
    /// Baseline direction in radians (0 = horizontal; non-zero for type on a path).
    pub angle: f64,
    /// Index into [`TextLayout::lines`].
    pub line: usize,
    /// [`FontFace::id`] of the face that supplied the glyph.
    pub font_id: u32,
}

/// One line of laid-out text.
///
/// For type on a path there is a single line; `x0, baseline` is the start point of the text on the
/// path and `x1` the x of its end point.
#[derive(Clone, Debug)]
pub struct LineInfo {
    pub baseline: f64,
    /// Left edge of the line content (after alignment).
    pub x0: f64,
    /// Right edge of the line content, trailing spaces excluded.
    pub x1: f64,
    pub ascent: f64,
    pub descent: f64,
    /// Byte range of the line in the plain text (excludes the paragraph's `\n`).
    pub start: usize,
    pub end: usize,
    /// Range of the line's glyphs in [`TextLayout::glyphs`].
    pub glyph_start: usize,
    pub glyph_end: usize,
}

#[derive(Clone, Debug, Default)]
pub struct TextLayout {
    pub glyphs: Vec<PositionedGlyph>,
    pub lines: Vec<LineInfo>,
    /// Ink/advance bounds in text space.
    pub bounds: Rect,
    /// True if area text did not fit its frame (the red overflow "+" marker), or on-path text ran
    /// past the end of the path.
    pub overflow: bool,
    /// True for type on a path (glyphs have individual `angle`s).
    pub on_path: bool,
}

impl TextLayout {
    /// All glyph outlines combined (e.g. for Create Outlines).
    pub fn to_bezpath(&self) -> BezPath {
        let mut p = BezPath::new();
        for g in &self.glyphs {
            p.extend(g.outline.iter());
        }
        p
    }
    /// Index of the line containing byte offset `byte`.
    pub fn line_of(&self, byte: usize) -> usize {
        self.lines.iter().rposition(|l| l.start <= byte).unwrap_or(0)
    }
}

fn dir(angle: f64) -> Vec2 {
    Vec2::new(angle.cos(), angle.sin())
}

/// Caret for byte offset `byte`, as a (top, bottom) segment in text space.
pub fn caret_position(layout: &TextLayout, byte: usize) -> (Point, Point) {
    let Some(line) = layout.lines.get(layout.line_of(byte)) else {
        return (Point::ZERO, Point::ZERO);
    };
    let glyphs = &layout.glyphs[line.glyph_start..line.glyph_end];
    let (pos, angle) = if let Some(g) = glyphs.iter().find(|g| g.byte + g.len > byte) {
        let frac = if byte <= g.byte { 0.0 } else { (byte - g.byte) as f64 / g.len.max(1) as f64 };
        (g.origin + dir(g.angle) * (g.advance * frac), g.angle)
    } else if let Some(g) = glyphs.last() {
        (g.origin + dir(g.angle) * g.advance, g.angle)
    } else {
        (Point::new(line.x0, line.baseline), 0.0)
    };
    // Up vector in y-down space, rotated with the baseline.
    let d = dir(angle);
    let up = Vec2::new(d.y, -d.x);
    (pos + up * line.ascent, pos - up * line.descent)
}

/// Byte offset nearest to the text-space point `p` (for clicking/dragging with the Type tool).
pub fn hit_byte(layout: &TextLayout, p: Point) -> usize {
    if layout.on_path {
        let best = layout.glyphs.iter().min_by(|a, b| {
            let ca = a.origin + dir(a.angle) * (a.advance * 0.5);
            let cb = b.origin + dir(b.angle) * (b.advance * 0.5);
            (p - ca).hypot2().total_cmp(&(p - cb).hypot2())
        });
        return match best {
            Some(g) if (p - g.origin).dot(dir(g.angle)) < g.advance * 0.5 => g.byte,
            Some(g) => g.byte + g.len,
            None => layout.lines.first().map_or(0, |l| l.start),
        };
    }
    let Some(li) = layout.lines.iter().position(|l| p.y < l.baseline + l.descent).or(layout.lines.len().checked_sub(1)) else {
        return 0;
    };
    let line = &layout.lines[li];
    let glyphs = &layout.glyphs[line.glyph_start..line.glyph_end];
    if let Some(g) = glyphs.iter().find(|g| p.x < g.origin.x + g.advance * 0.5) {
        return g.byte;
    }
    // Past the end: a soft-wrapped line ends before its last (usually space) glyph so the caret
    // stays on this line.
    let soft_wrap = layout.lines.get(li + 1).is_some_and(|n| n.start == line.end);
    match glyphs.last() {
        Some(g) if soft_wrap => g.byte,
        _ => line.end,
    }
}

#[cfg(test)]
mod tests;
