//! Colour, paint and blend-mode types for DrawCraft.
//!
//! Colours keep the model the user picked them in (RGB, CMYK, Gray, HSB is a UI view of RGB), so
//! documents don't drift when converting back and forth. Rendering asks for [`Color::to_rgba`].
#![forbid(unsafe_code)]

pub mod blend;
pub mod gradient;
pub mod harmony;
pub mod swatch;

pub use blend::BlendMode;
pub use gradient::{Gradient, GradientGeom, GradientKind, GradientPaint, GradientStop};
pub use swatch::{Swatch, SwatchGroup, default_swatches};

use serde::{Deserialize, Serialize};

/// A colour in its authoring model. Components are 0..=1.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "model", rename_all = "lowercase")]
pub enum Color {
    Rgb { r: f32, g: f32, b: f32 },
    Cmyk { c: f32, m: f32, y: f32, k: f32 },
    Gray { k: f32 },
}

impl Default for Color {
    fn default() -> Self {
        Color::BLACK
    }
}

impl Color {
    pub const BLACK: Color = Color::Rgb { r: 0.0, g: 0.0, b: 0.0 };
    pub const WHITE: Color = Color::Rgb { r: 1.0, g: 1.0, b: 1.0 };

    pub fn rgb(r: f32, g: f32, b: f32) -> Self {
        Color::Rgb { r, g, b }
    }
    pub fn rgb8(r: u8, g: u8, b: u8) -> Self {
        Color::Rgb { r: r as f32 / 255.0, g: g as f32 / 255.0, b: b as f32 / 255.0 }
    }
    pub fn cmyk(c: f32, m: f32, y: f32, k: f32) -> Self {
        Color::Cmyk { c, m, y, k }
    }
    pub fn gray(k: f32) -> Self {
        Color::Gray { k }
    }

    /// Display RGB (naive, profile-free conversion; colour management comes later).
    pub fn to_rgb(&self) -> [f32; 3] {
        match *self {
            Color::Rgb { r, g, b } => [r, g, b],
            Color::Cmyk { c, m, y, k } => [(1.0 - c) * (1.0 - k), (1.0 - m) * (1.0 - k), (1.0 - y) * (1.0 - k)],
            // Illustrator's Gray is ink percentage: 0 = white, 1 = black.
            Color::Gray { k } => [1.0 - k; 3],
        }
    }
    pub fn to_rgba8(&self, alpha: f32) -> [u8; 4] {
        let [r, g, b] = self.to_rgb();
        let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        [q(r), q(g), q(b), q(alpha)]
    }
    pub fn to_cmyk(&self) -> [f32; 4] {
        match *self {
            Color::Cmyk { c, m, y, k } => [c, m, y, k],
            _ => {
                let [r, g, b] = self.to_rgb();
                let k = 1.0 - r.max(g).max(b);
                if k >= 1.0 {
                    return [0.0, 0.0, 0.0, 1.0];
                }
                [(1.0 - r - k) / (1.0 - k), (1.0 - g - k) / (1.0 - k), (1.0 - b - k) / (1.0 - k), k]
            }
        }
    }
    /// HSB with hue in degrees 0..360, saturation and brightness 0..1.
    pub fn to_hsb(&self) -> [f32; 3] {
        let [r, g, b] = self.to_rgb();
        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let d = max - min;
        let h = if d <= 0.0 {
            0.0
        } else if max == r {
            60.0 * (((g - b) / d).rem_euclid(6.0))
        } else if max == g {
            60.0 * ((b - r) / d + 2.0)
        } else {
            60.0 * ((r - g) / d + 4.0)
        };
        let s = if max <= 0.0 { 0.0 } else { d / max };
        [h, s, max]
    }
    pub fn from_hsb(h: f32, s: f32, v: f32) -> Self {
        let h = h.rem_euclid(360.0) / 60.0;
        let c = v * s;
        let x = c * (1.0 - (h % 2.0 - 1.0).abs());
        let (r, g, b) = match h as u32 {
            0 => (c, x, 0.0),
            1 => (x, c, 0.0),
            2 => (0.0, c, x),
            3 => (0.0, x, c),
            4 => (x, 0.0, c),
            _ => (c, 0.0, x),
        };
        let m = v - c;
        Color::Rgb { r: r + m, g: g + m, b: b + m }
    }
    /// `#rrggbb` (display RGB).
    pub fn to_hex(&self) -> String {
        let [r, g, b, _] = self.to_rgba8(1.0);
        format!("#{r:02x}{g:02x}{b:02x}")
    }
    /// Parse `#rgb`, `#rrggbb` or a few CSS names.
    pub fn from_hex(s: &str) -> Option<Self> {
        let s = s.trim();
        match s.to_ascii_lowercase().as_str() {
            "black" => return Some(Color::BLACK),
            "white" => return Some(Color::WHITE),
            "red" => return Some(Color::rgb(1.0, 0.0, 0.0)),
            "green" => return Some(Color::rgb8(0, 128, 0)),
            "blue" => return Some(Color::rgb(0.0, 0.0, 1.0)),
            _ => {}
        }
        let h = s.strip_prefix('#').unwrap_or(s);
        let p = |i: usize, n: usize| u8::from_str_radix(h.get(i..i + n)?, 16).ok();
        match h.len() {
            3 => Some(Color::rgb8(p(0, 1)? * 17, p(1, 1)? * 17, p(2, 1)? * 17)),
            6 | 8 => Some(Color::rgb8(p(0, 2)?, p(2, 2)?, p(4, 2)?)),
            _ => None,
        }
    }
    /// Complement (Illustrator's Edit → Edit Colors → Invert is 1 - rgb; this is hue + 180).
    pub fn complement(&self) -> Self {
        let [h, s, v] = self.to_hsb();
        Color::from_hsb(h + 180.0, s, v)
    }
    pub fn invert(&self) -> Self {
        let [r, g, b] = self.to_rgb();
        Color::rgb(1.0 - r, 1.0 - g, 1.0 - b)
    }
    /// Linear interpolation in display RGB.
    pub fn lerp(&self, other: &Color, t: f32) -> Color {
        let a = self.to_rgb();
        let b = other.to_rgb();
        Color::rgb(a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t)
    }
    pub fn model_name(&self) -> &'static str {
        match self {
            Color::Rgb { .. } => "RGB",
            Color::Cmyk { .. } => "CMYK",
            Color::Gray { .. } => "Grayscale",
        }
    }
}

/// What fills or strokes an object.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Paint {
    #[default]
    None,
    Solid {
        color: Color,
        /// Name of the global swatch this colour is linked to (edits to the swatch update it).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        swatch: Option<String>,
    },
    Gradient(Box<GradientPaint>),
    Pattern {
        pattern: String,
    },
}

impl Paint {
    pub fn solid(c: Color) -> Self {
        Paint::Solid { color: c, swatch: None }
    }
    pub fn is_none(&self) -> bool {
        matches!(self, Paint::None)
    }
    pub fn color(&self) -> Option<Color> {
        match self {
            Paint::Solid { color, .. } => Some(*color),
            _ => None,
        }
    }
    pub fn label(&self) -> String {
        match self {
            Paint::None => "None".into(),
            Paint::Solid { color, swatch: Some(n) } => format!("{n} ({})", color.to_hex()),
            Paint::Solid { color, .. } => color.to_hex(),
            Paint::Gradient(g) => format!("{} gradient", g.gradient.kind.label()),
            Paint::Pattern { pattern } => format!("pattern {pattern}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_roundtrip() {
        let c = Color::from_hex("#ff8000").unwrap();
        assert_eq!(c.to_hex(), "#ff8000");
        assert_eq!(Color::from_hex("#fff").unwrap().to_hex(), "#ffffff");
        assert_eq!(Color::from_hex("zz"), None);
    }

    #[test]
    fn hsb_roundtrip() {
        for hex in ["#ff0000", "#00ff00", "#0000ff", "#336699", "#ffffff", "#000000", "#c0ffee"] {
            let c = Color::from_hex(hex).unwrap();
            let [h, s, v] = c.to_hsb();
            assert_eq!(Color::from_hsb(h, s, v).to_hex(), hex, "{hex}");
        }
    }

    #[test]
    fn cmyk_conversions() {
        assert_eq!(Color::cmyk(0.0, 0.0, 0.0, 1.0).to_hex(), "#000000");
        assert_eq!(Color::cmyk(1.0, 0.0, 0.0, 0.0).to_hex(), "#00ffff");
        let k = Color::rgb(1.0, 0.0, 0.0).to_cmyk();
        assert_eq!(k, [0.0, 1.0, 1.0, 0.0]);
    }

    #[test]
    fn gray_is_ink() {
        assert_eq!(Color::gray(0.0).to_hex(), "#ffffff");
        assert_eq!(Color::gray(1.0).to_hex(), "#000000");
    }

    #[test]
    fn complement_and_invert() {
        assert_eq!(Color::rgb(1.0, 0.0, 0.0).complement().to_hex(), "#00ffff");
        assert_eq!(Color::rgb(1.0, 0.0, 0.0).invert().to_hex(), "#00ffff");
    }

    #[test]
    fn paint_serde() {
        let p = Paint::solid(Color::rgb8(10, 20, 30));
        let s = serde_json::to_string(&p).unwrap();
        assert!(s.contains("\"type\":\"solid\""));
        assert_eq!(serde_json::from_str::<Paint>(&s).unwrap(), p);
    }
}
