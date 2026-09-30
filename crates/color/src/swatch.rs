//! Swatches and the default swatch set (our own palette, not Adobe's).

use serde::{Deserialize, Serialize};

use crate::{Color, Gradient, GradientKind, GradientStop, Paint};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Swatch {
    pub name: String,
    pub paint: Paint,
    /// Global swatches update every object that uses them.
    #[serde(default)]
    pub global: bool,
    /// Spot colour (prints on its own plate).
    #[serde(default)]
    pub spot: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SwatchGroup {
    pub name: String,
    pub swatches: Vec<Swatch>,
}

fn solid(name: &str, hex: &str) -> Swatch {
    Swatch { name: name.into(), paint: Paint::solid(Color::from_hex(hex).unwrap()), global: false, spot: false }
}

/// The default document swatches: None, Registration-like black, white, black, a spectrum, greys,
/// gradients. (Composition is ours; Illustrator's layout grammar — None first, then specials — is kept.)
pub fn default_swatches() -> (Vec<Swatch>, Vec<SwatchGroup>) {
    let mut s =
        vec![Swatch { name: "[None]".into(), paint: Paint::None, global: false, spot: false }, solid("White", "#ffffff"), solid("Black", "#000000")];
    let spectrum = [
        ("Red", "#ed1c24"),
        ("Orange Red", "#f15a24"),
        ("Orange", "#f7931e"),
        ("Amber", "#fbb03b"),
        ("Yellow", "#fcee21"),
        ("Yellow Green", "#d9e021"),
        ("Lime", "#8cc63f"),
        ("Green", "#39b54a"),
        ("Emerald", "#009245"),
        ("Teal", "#006837"),
        ("Aqua", "#22b573"),
        ("Cyan Green", "#00a99d"),
        ("Cyan", "#29abe2"),
        ("Azure", "#0071bc"),
        ("Blue", "#2e3192"),
        ("Indigo", "#1b1464"),
        ("Violet", "#662d91"),
        ("Purple", "#93278f"),
        ("Magenta", "#9e005d"),
        ("Rose", "#d4145a"),
        ("Pink", "#ed1e79"),
        ("Brown", "#c7b299"),
        ("Tan", "#998675"),
        ("Coffee", "#736357"),
        ("Chocolate", "#534741"),
        ("Sand", "#c69c6d"),
        ("Copper", "#a67c52"),
        ("Rust", "#8c6239"),
        ("Walnut", "#754c24"),
        ("Espresso", "#603813"),
    ];
    s.extend(spectrum.iter().map(|(n, h)| solid(n, h)));
    let grays: Vec<Swatch> = (1..=9)
        .rev()
        .map(|i| {
            let k = i as f32 / 10.0;
            let c = Color::gray(k);
            Swatch { name: format!("K={}", (k * 100.0) as u32), paint: Paint::solid(c), global: false, spot: false }
        })
        .collect();
    let grad = |name: &str, a: &str, b: &str, kind: GradientKind| Swatch {
        name: name.into(),
        paint: Paint::Gradient(Box::new(crate::GradientPaint::new(Gradient {
            kind,
            stops: vec![
                GradientStop { offset: 0.0, color: Color::from_hex(a).unwrap(), opacity: 1.0, midpoint: 0.5 },
                GradientStop { offset: 1.0, color: Color::from_hex(b).unwrap(), opacity: 1.0, midpoint: 0.5 },
            ],
        }))),
        global: false,
        spot: false,
    };
    s.push(grad("White, Black", "#ffffff", "#000000", GradientKind::Linear));
    s.push(grad("Radial White, Black", "#ffffff", "#000000", GradientKind::Radial));
    s.push(grad("Sunset", "#fbb03b", "#d4145a", GradientKind::Linear));
    s.push(grad("Ocean", "#29abe2", "#2e3192", GradientKind::Linear));
    let groups = vec![
        SwatchGroup { name: "Grays".into(), swatches: grays },
        SwatchGroup {
            name: "Brights".into(),
            swatches: vec![
                solid("Bright Red", "#ff1d25"),
                solid("Bright Yellow", "#ffe600"),
                solid("Bright Green", "#00e676"),
                solid("Bright Blue", "#2979ff"),
                solid("Bright Violet", "#d500f9"),
            ],
        },
    ];
    (s, groups)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_start_with_none() {
        let (s, g) = default_swatches();
        assert!(s[0].paint.is_none());
        assert!(s.len() > 30);
        assert_eq!(g.len(), 2);
        assert_eq!(g[0].swatches.len(), 9);
    }
}
