//! Text objects (model only; layout and glyph outlines live in `drawcraft-text`).

use drawcraft_color::{Color, Paint};
use drawcraft_geom::{Affine, PathData, Point, Rect};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Justify {
    #[default]
    Left,
    Center,
    Right,
    JustifyLeft,
    JustifyCenter,
    JustifyRight,
    JustifyAll,
}

/// Character attributes (the Character panel).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CharStyle {
    pub font_family: String,
    #[serde(default = "regular")]
    pub font_style: String,
    /// Size in points.
    pub size: f64,
    /// Leading in points; None = Auto (120% of size).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub leading: Option<f64>,
    /// Tracking in 1/1000 em.
    #[serde(default)]
    pub tracking: f64,
    /// Kerning: None = Auto (metrics), Some(v) = manual in 1/1000 em.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kerning: Option<f64>,
    #[serde(default)]
    pub baseline_shift: f64,
    #[serde(default = "hundred")]
    pub h_scale: f64,
    #[serde(default = "hundred")]
    pub v_scale: f64,
    #[serde(default)]
    pub rotation: f64,
    pub fill: Paint,
    #[serde(default)]
    pub stroke: Paint,
    #[serde(default)]
    pub stroke_width: f64,
    #[serde(default)]
    pub underline: bool,
    #[serde(default)]
    pub strikethrough: bool,
    #[serde(default)]
    pub all_caps: bool,
    /// Character style (Character Styles panel) these attributes come from; None = Normal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style_name: Option<String>,
}

fn regular() -> String {
    "Regular".into()
}
fn hundred() -> f64 {
    100.0
}

impl Default for CharStyle {
    fn default() -> Self {
        Self {
            font_family: "Source Sans 3".into(),
            font_style: "Regular".into(),
            size: 12.0,
            leading: None,
            tracking: 0.0,
            kerning: None,
            baseline_shift: 0.0,
            h_scale: 100.0,
            v_scale: 100.0,
            rotation: 0.0,
            fill: Paint::solid(Color::BLACK),
            stroke: Paint::None,
            stroke_width: 0.0,
            underline: false,
            strikethrough: false,
            all_caps: false,
            style_name: None,
        }
    }
}

impl CharStyle {
    pub fn effective_leading(&self) -> f64 {
        self.leading.unwrap_or(self.size * 1.2)
    }
}

/// Paragraph attributes (the Paragraph panel).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ParaStyle {
    #[serde(default)]
    pub justify: Justify,
    #[serde(default)]
    pub left_indent: f64,
    #[serde(default)]
    pub right_indent: f64,
    #[serde(default)]
    pub first_line_indent: f64,
    #[serde(default)]
    pub space_before: f64,
    #[serde(default)]
    pub space_after: f64,
    #[serde(default)]
    pub hyphenate: bool,
    /// Paragraph style (Paragraph Styles panel) these attributes come from; None = Normal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style_name: Option<String>,
}

/// A named character or paragraph style: the attributes it sets (a subset of [`CharStyle`] or
/// [`ParaStyle`] fields, by their serialized names). Text using it records the name.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TextStyleDef {
    pub name: String,
    #[serde(default)]
    pub attrs: serde_json::Map<String, serde_json::Value>,
}

/// A run of text sharing one character style.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TextRun {
    pub text: String,
    pub style: CharStyle,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum TextKind {
    /// Point type: anchored at the baseline origin of the first line.
    Point,
    /// Area type flowed inside `frame` (document coordinates, untransformed by `xf`).
    Area { frame: PathData },
    /// Type on a path, starting at `start` (0..1 of the path length).
    OnPath { path: PathData, start: f64 },
}

/// A text object. `runs` split into paragraphs at `\n`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TextObject {
    pub kind: TextKind,
    /// Maps text space (origin = first baseline start for point type) to the document.
    pub xf: Affine,
    pub runs: Vec<TextRun>,
    #[serde(default)]
    pub para: ParaStyle,
    /// Cached layout bounds in text space, filled in by the layout engine (not serialized).
    #[serde(skip)]
    pub cached_bounds: Option<Rect>,
}

impl TextObject {
    pub fn point(origin: Point, text: &str, style: CharStyle) -> Self {
        Self {
            kind: TextKind::Point,
            xf: Affine::translate(origin.to_vec2()),
            runs: vec![TextRun { text: text.into(), style }],
            para: ParaStyle::default(),
            cached_bounds: None,
        }
    }
    pub fn plain_text(&self) -> String {
        self.runs.iter().map(|r| r.text.as_str()).collect()
    }
    pub fn first_style(&self) -> CharStyle {
        self.runs.first().map(|r| r.style.clone()).unwrap_or_default()
    }
    /// Approximate bounds when no layout cache is available (0.55 em average advance).
    pub fn estimate_bounds(&self) -> Rect {
        let st = self.first_style();
        let text = self.plain_text();
        let lines: Vec<&str> = text.split('\n').collect();
        let w = lines.iter().map(|l| l.chars().count()).max().unwrap_or(0) as f64 * st.size * 0.55;
        let lead = st.effective_leading();
        Rect::new(0.0, -st.size * 0.8, w.max(1.0), -st.size * 0.8 + lead * lines.len().max(1) as f64)
    }
    pub fn bounds(&self) -> Option<Rect> {
        match &self.kind {
            TextKind::Area { frame } => frame.bounds().map(|b| self.xf.transform_rect_bbox(b)),
            TextKind::OnPath { path, .. } => path.bounds().map(|b| self.xf.transform_rect_bbox(b)),
            TextKind::Point => Some(self.xf.transform_rect_bbox(self.cached_bounds.unwrap_or_else(|| self.estimate_bounds()))),
        }
    }
    pub fn transform(&mut self, a: Affine) {
        self.xf = a * self.xf;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn point_text_basics() {
        let t = TextObject::point(Point::new(10.0, 20.0), "Hello", CharStyle::default());
        assert_eq!(t.plain_text(), "Hello");
        let b = t.bounds().unwrap();
        assert!(b.x0 >= 10.0 - 1e-9 && b.y1 > 20.0);
        assert_eq!(CharStyle::default().effective_leading(), 14.399999999999999);
    }
}
