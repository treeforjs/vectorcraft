//! Brush Options (double-click a brush in the Brushes panel, or the panel menu's Brush Options…).
//! Calligraphic Brush Options: Name, then Angle, Roundness and Size, each Fixed, Random or
//! Pressure with its Variation, over live previews of the nib and of a stroke (light, heavy, light
//! pressure). Bristle Brush Options: Name, Shape and the bristle sliders. Scatter, Art and Pattern
//! Brush Options: their values, flips and fit, and Colorization, over a live stroke preview. OK
//! runs `brush.options` (strokes painted with the brush update); a name another brush has keeps
//! the dialog open.
//!
//! Fields: `__brush` (the brush's name), `__type` (`calligraphic` | `bristle` | `scatter` | `art`
//! | `pattern`), `name`. Calligraphic: `angle` (−180..180°), `roundness` (0..100 %), `size`
//! (0..1296 pt), and per value `angleMode`, `roundnessMode`, `sizeMode` (`fixed` | `random` |
//! `pressure`) and `angleVariation`, `roundnessVariation`, `sizeVariation`. Bristle: `shape`
//! (`roundPoint`…), `size` (pt), `length` (25..300 %), `density`, `thickness`, `opacity`,
//! `stiffness` (1..100 %). Scatter: per value (`size`, `spacing`, `scatter`, `rotation`) `…Mode`
//! (`fixed` | `random`), `…Min`, `…Max`; `rotationRelativeTo` (`page` | `path`). Art: `width` (%),
//! `direction` (`leftToRight`…), `scale` (`proportional` | `stretch` | `betweenGuides`),
//! `guideStart`, `guideEnd` (% of the art's length), `flipAlong`, `flipAcross`. Pattern: `scale`,
//! `spacing` (%), `fit` (`stretch` | `addSpace` | `approximate`), `flipAlong`, `flipAcross`.
//! Scatter, Art and Pattern: `colorization` (`none` | `tints` | `tintsAndShades` | `hueShift`) and
//! `keyColor` (`#rrggbb`); `__def` holds the brush's definition for the preview.

use egui::{Color32, Sense, Stroke, pos2, vec2};
use serde_json::{Map, Value, json};
use vectorcraft_brush::{ArtScale, Brush, BrushKind, Calligraphic, Colorization, Direction, PatternFit, Variation};
use vectorcraft_doc::color::Color;

use super::swatch_options::{grid, label};
use super::{DialogSpec, form};
use crate::state::Dialog;
use crate::theme::Tokens;
use crate::{VectorcraftApp, widgets};

/// The dialog kind of Brush Options.
pub const KIND: &str = "brushOptions";

pub(super) const SPEC: DialogSpec = DialogSpec { heading, body, confirm, min_width: 440.0, ..DialogSpec::FORM };

/// The brush types that have an options dialog.
pub const TYPES: [&str; 5] = ["calligraphic", "bristle", "scatter", "art", "pattern"];

/// Calligraphic values: (field, label, suffix, decimals), in [`Calligraphic::RANGES`] order.
const CALLI: [(&str, &str, &str, usize); 3] = [("angle", "Angle:", "°", 0), ("roundness", "Roundness:", "%", 0), ("size", "Size:", " pt", 1)];

/// Bristle sliders: (field, label, min, max), in percent.
const BRISTLE: [(&str, &str, f64, f64); 5] = [
    ("length", "Bristle Length:", 25.0, 300.0),
    ("density", "Bristle Density:", 1.0, 100.0),
    ("thickness", "Bristle Thickness:", 1.0, 100.0),
    ("opacity", "Paint Opacity:", 1.0, 100.0),
    ("stiffness", "Stiffness:", 1.0, 100.0),
];

/// Bristle tip shapes: (value, label).
const SHAPES: [(&str, &str); 10] = [
    ("roundPoint", "Round Point"),
    ("flatPoint", "Flat Point"),
    ("roundBlunt", "Round Blunt"),
    ("flatBlunt", "Flat Blunt"),
    ("roundCurve", "Round Curve"),
    ("flatCurve", "Flat Curve"),
    ("roundAngle", "Round Angle"),
    ("flatAngle", "Flat Angle"),
    ("roundFan", "Round Fan"),
    ("flatFan", "Flat Fan"),
];

/// Scatter values: (field, label, range, suffix).
const SCATTER: [(&str, &str, (f64, f64), &str); 4] = [
    ("size", "Size:", (1.0, 1000.0), "%"),
    ("spacing", "Spacing:", (1.0, 10000.0), "%"),
    ("scatter", "Scatter:", (-1000.0, 1000.0), "%"),
    ("rotation", "Rotation:", (-360.0, 360.0), "°"),
];

/// Scatter value modes: (value, label).
const SCATTER_MODES: [(&str, &str); 2] = [("fixed", "Fixed"), ("random", "Random")];

const ROTATION_RELATIVE: [(&str, &str); 2] = [("page", "Page"), ("path", "Path")];

const COLORIZATION: [(&str, &str); 4] = [("none", "None"), ("tints", "Tints"), ("tintsAndShades", "Tints and Shades"), ("hueShift", "Hue Shift")];

const DIRECTIONS: [(&str, &str); 4] =
    [("leftToRight", "Left to Right"), ("rightToLeft", "Right to Left"), ("topToBottom", "Top to Bottom"), ("bottomToTop", "Bottom to Top")];

const ART_SCALES: [(&str, &str); 3] =
    [("proportional", "Scale Proportionately"), ("stretch", "Stretch to Fit Stroke Length"), ("betweenGuides", "Stretch Between Guides")];

const FITS: [(&str, &str); 3] = [("stretch", "Stretch to Fit"), ("addSpace", "Add Space to Fit"), ("approximate", "Approximate Path")];

/// Width of the bristle dialog's label column.
const LABEL_W: f32 = 112.0;

/// Widths of the nib preview and of the whole preview row.
const NIB_W: f32 = 120.0;
const STROKE_W: f32 = 400.0;

fn heading(d: &Dialog) -> String {
    match d.str("__type").as_str() {
        "bristle" => tl!("Bristle Brush Options").into(),
        "scatter" => tl!("Scatter Brush Options").into(),
        "art" => tl!("Art Brush Options").into(),
        "pattern" => tl!("Pattern Brush Options").into(),
        _ => tl!("Calligraphic Brush Options").into(),
    }
}

fn mode_key(key: &str) -> String {
    format!("{key}Mode")
}

fn variation_key(key: &str) -> String {
    format!("{key}Variation")
}

/// Open the options of brush `name` (default: the selected path's brush, else the current one).
pub fn open(app: &mut VectorcraftApp, name: Option<&str>) -> Result<Value, String> {
    let name = match name {
        Some(n) => n.to_string(),
        None => target(app).ok_or("no brush selected")?,
    };
    let def = app.run("brush.get", json!({ "name": name }))?;
    let b: Brush = serde_json::from_value(def).map_err(|e| e.to_string())?;
    let mut f = Map::new();
    f.insert("__brush".into(), json!(b.name));
    f.insert("name".into(), json!(b.name));
    f.insert("__type".into(), json!(b.kind.type_id()));
    match &b.kind {
        BrushKind::Calligraphic(c) => {
            let values = [c.angle, c.roundness, c.size];
            for (i, (key, ..)) in CALLI.iter().enumerate() {
                let (Some(v), Some(var), Some(mode)) = (values.get(i), c.variation.get(i), c.modes().get(i).copied()) else { continue };
                f.insert((*key).into(), json!(v));
                f.insert(variation_key(key), json!(var));
                f.insert(mode_key(key), json!(mode.id()));
            }
        }
        BrushKind::Bristle(b) => {
            let v = serde_json::to_value(b).map_err(|e| e.to_string())?;
            for key in ["shape", "size"].into_iter().chain(BRISTLE.map(|r| r.0)) {
                f.insert(key.into(), v.get(key).cloned().unwrap_or(Value::Null));
            }
        }
        BrushKind::Scatter(s) => {
            for ((key, ..), (lo, hi)) in SCATTER.iter().zip([s.size, s.spacing, s.scatter, s.rotation]) {
                f.insert(format!("{key}Min"), json!(lo.min(hi)));
                f.insert(format!("{key}Max"), json!(lo.max(hi)));
                f.insert(mode_key(key), json!(if lo == hi { "fixed" } else { "random" }));
            }
            f.insert("rotationRelativeTo".into(), json!(if s.rotation_relative_to_path { "path" } else { "page" }));
            colorization_fields(&mut f, &s.colorization);
        }
        BrushKind::Art(a) => {
            f.insert("width".into(), json!(a.width));
            f.insert("direction".into(), serde_json::to_value(a.direction).unwrap_or_default());
            let (scale, start, end) = match a.scale {
                ArtScale::Proportional => ("proportional", 0.25, 0.75),
                ArtScale::Stretch => ("stretch", 0.25, 0.75),
                ArtScale::BetweenGuides { start, end } => ("betweenGuides", start, end),
            };
            f.insert("scale".into(), json!(scale));
            f.insert("guideStart".into(), json!(start * 100.0));
            f.insert("guideEnd".into(), json!(end * 100.0));
            f.insert("flipAlong".into(), json!(a.flip_along));
            f.insert("flipAcross".into(), json!(a.flip_across));
            colorization_fields(&mut f, &a.colorization);
        }
        BrushKind::Pattern(p) => {
            f.insert("scale".into(), json!(p.scale));
            f.insert("spacing".into(), json!(p.spacing));
            f.insert("fit".into(), serde_json::to_value(p.fit).unwrap_or_default());
            f.insert("flipAlong".into(), json!(p.flip_along));
            f.insert("flipAcross".into(), json!(p.flip_across));
            colorization_fields(&mut f, &p.colorization);
        }
    }
    if matches!(b.kind, BrushKind::Scatter(_) | BrushKind::Art(_) | BrushKind::Pattern(_)) {
        f.insert("__def".into(), serde_json::to_value(&b).map_err(|e| e.to_string())?);
    }
    app.ui.dialog = Some(Dialog { kind: KIND.into(), fields: f });
    Ok(Value::Null)
}

/// Can Brush Options open without a name (its brush has a type with a dialog)?
pub fn available(app: &VectorcraftApp) -> bool {
    let Some(st) = app.session.active() else { return false };
    target(app).and_then(|n| vectorcraft_brush::find(&st.doc, &n)).is_some_and(|b| TYPES.contains(&b.kind.type_id()))
}

/// The Colorization fields of `c`: its method and key colour.
fn colorization_fields(f: &mut Map<String, Value>, c: &Colorization) {
    let method = serde_json::to_value(c).ok().and_then(|v| v.get("method").and_then(Value::as_str).map(str::to_owned));
    f.insert("colorization".into(), json!(method.unwrap_or_else(|| "none".into())));
    let key = match c {
        Colorization::HueShift { key } => *key,
        _ => Color::BLACK,
    };
    f.insert("keyColor".into(), json!(key.to_hex()));
}

/// The Colorization the fields describe.
fn colorization(d: &Dialog) -> Colorization {
    match d.str("colorization").as_str() {
        "tints" => Colorization::Tints,
        "tintsAndShades" => Colorization::TintsAndShades,
        "hueShift" => Colorization::HueShift { key: Color::from_hex(&d.str("keyColor")).unwrap_or(Color::BLACK) },
        _ => Colorization::None,
    }
}

/// The brush Brush Options opens on without a name: the selected path's brush, else the current one.
fn target(app: &VectorcraftApp) -> Option<String> {
    let st = app.session.active()?;
    let selected = st.selection.subjects().first().and_then(|id| st.doc.node(*id)).and_then(|n| n.appearance.stroke()?.brush.clone());
    selected.or_else(|| vectorcraft_brush::current(&st.doc))
}

/// The value of field `key` within `range`.
fn clamped(d: &Dialog, key: &str, (lo, hi): (f64, f64)) -> f64 {
    d.f64(key, lo).clamp(lo, hi)
}

/// The Calligraphic brush the fields describe.
fn calligraphic(d: &Dialog) -> Calligraphic {
    let value = |i: usize| CALLI.get(i).zip(Calligraphic::RANGES.get(i)).map_or(0.0, |((key, ..), r)| clamped(d, key, *r));
    let variation =
        |i: usize| CALLI.get(i).zip(Calligraphic::MAX_VARIATION.get(i)).map_or(0.0, |((key, ..), max)| clamped(d, &variation_key(key), (0.0, *max)));
    let mode = |i: usize| CALLI.get(i).and_then(|(key, ..)| Variation::parse(&d.str(&mode_key(key)))).unwrap_or_default();
    Calligraphic { angle: value(0), roundness: value(1), size: value(2), variation: [0, 1, 2].map(variation), modes: Some([0, 1, 2].map(mode)) }
}

/// `brush.options` params from the fields.
fn params(d: &Dialog) -> Value {
    match d.str("__type").as_str() {
        "bristle" => {
            let mut p = json!({ "shape": d.str("shape"), "size": clamped(d, "size", (0.1, 1296.0)) });
            for (key, _, lo, hi) in BRISTLE {
                p[key] = json!(clamped(d, key, (lo, hi)));
            }
            p
        }
        "scatter" => {
            let mut p = json!({ "rotation_relative_to_path": d.str("rotationRelativeTo") == "path" });
            for (key, _, range, _) in SCATTER {
                let lo = clamped(d, &format!("{key}Min"), range);
                let hi = if d.str(&mode_key(key)) == "random" { clamped(d, &format!("{key}Max"), range) } else { lo };
                p[key] = json!([lo.min(hi), lo.max(hi)]);
            }
            p["colorization"] = serde_json::to_value(colorization(d)).unwrap_or_default();
            p
        }
        "art" => {
            let scale = match d.str("scale").as_str() {
                "proportional" => ArtScale::Proportional,
                "betweenGuides" => {
                    let (a, b) = (clamped(d, "guideStart", (0.0, 100.0)), clamped(d, "guideEnd", (0.0, 100.0)));
                    ArtScale::BetweenGuides { start: a.min(b) / 100.0, end: a.max(b) / 100.0 }
                }
                _ => ArtScale::Stretch,
            };
            let direction: Direction = serde_json::from_value(json!(d.str("direction"))).unwrap_or_default();
            json!({
                "width": clamped(d, "width", (1.0, 1000.0)),
                "direction": serde_json::to_value(direction).unwrap_or_default(),
                "scale": serde_json::to_value(scale).unwrap_or_default(),
                "flip_along": d.bool("flipAlong"),
                "flip_across": d.bool("flipAcross"),
                "colorization": serde_json::to_value(colorization(d)).unwrap_or_default(),
            })
        }
        "pattern" => {
            let fit: PatternFit = serde_json::from_value(json!(d.str("fit"))).unwrap_or_default();
            json!({
                "scale": clamped(d, "scale", (1.0, 10000.0)),
                "spacing": clamped(d, "spacing", (0.0, 10000.0)),
                "fit": serde_json::to_value(fit).unwrap_or_default(),
                "flip_along": d.bool("flipAlong"),
                "flip_across": d.bool("flipAcross"),
                "colorization": serde_json::to_value(colorization(d)).unwrap_or_default(),
            })
        }
        _ => serde_json::to_value(calligraphic(d)).unwrap_or_default(),
    }
}

/// The brush's definition with the fields' values, for the preview.
fn preview_def(d: &Dialog) -> Value {
    let mut def = d.fields.get("__def").cloned().unwrap_or_else(|| json!({}));
    if let (Some(o), Some(p)) = (def.as_object_mut(), params(d).as_object()) {
        for (k, v) in p {
            o.insert(k.clone(), v.clone());
        }
    }
    def
}

fn body(app: &mut VectorcraftApp, ui: &mut egui::Ui, d: &mut Dialog) -> bool {
    grid(ui, |ui| {
        label(ui, tl!("Name:"));
        form::text(ui, d, "name", 240.0);
        ui.end_row();
    });
    ui.add_space(10.0);
    match d.str("__type").as_str() {
        "bristle" => bristle_body(app, ui, d),
        "scatter" => scatter_body(ui, d),
        "art" => art_body(ui, d),
        "pattern" => pattern_body(ui, d),
        _ => calligraphic_body(ui, d),
    }
    false
}

fn scatter_body(ui: &mut egui::Ui, d: &mut Dialog) {
    egui::Grid::new("brush-options-scatter").num_columns(4).spacing([8.0, 8.0]).show(ui, |ui| {
        for (key, text, (lo, hi), suffix) in SCATTER {
            label(ui, tl!(text));
            let (min, max) = (format!("{key}Min"), format!("{key}Max"));
            if let Some(v) = widgets::range_field(ui, ("brush-min", key), clamped(d, &min, (lo, hi)), lo..=hi, suffix, 0, 64.0) {
                d.fields.insert(min, json!(v));
            }
            let mode = d.str(&mode_key(key));
            let random = mode == "random";
            let shown = SCATTER_MODES.iter().find(|(v, _)| *v == mode).map_or("Fixed", |(_, l)| l);
            let labels = SCATTER_MODES.map(|(_, l)| l);
            if let Some((v, _)) = widgets::dropdown(ui, ("brush-mode", key), shown, &labels, 96.0).and_then(|i| SCATTER_MODES.get(i)) {
                d.fields.insert(mode_key(key), json!(v));
            }
            ui.add_enabled_ui(random, |ui| {
                if let Some(v) = widgets::range_field(ui, ("brush-max", key), clamped(d, &max, (lo, hi)), lo..=hi, suffix, 0, 64.0) {
                    d.fields.insert(max, json!(v));
                }
            });
            ui.end_row();
        }
    });
    ui.add_space(6.0);
    form::choice(ui, d, "rotationRelativeTo", tl!("Rotation relative to:"), (LABEL_W + 40.0, 100.0), &ROTATION_RELATIVE);
    colorization_body(ui, d);
    ui.add_space(8.0);
    stroke_preview(ui, "scatter", preview_def(d), vec2(STROKE_W, 64.0));
}

fn art_body(ui: &mut egui::Ui, d: &mut Dialog) {
    widgets::label_row(ui, tl!("Width:"), LABEL_W, |ui| {
        if let Some(v) = widgets::range_field(ui, "brush-art-width", clamped(d, "width", (1.0, 1000.0)), 1.0..=1000.0, "%", 0, 64.0) {
            d.fields.insert("width".into(), json!(v));
        }
    });
    form::choice(ui, d, "scale", tl!("Brush Scale:"), (LABEL_W, 220.0), &ART_SCALES);
    if d.str("scale") == "betweenGuides" {
        widgets::label_row(ui, tl!("Guides:"), LABEL_W, |ui| {
            for key in ["guideStart", "guideEnd"] {
                if let Some(v) = widgets::range_field(ui, ("brush-guide", key), clamped(d, key, (0.0, 100.0)), 0.0..=100.0, "%", 0, 56.0) {
                    d.fields.insert(key.into(), json!(v));
                }
            }
        });
    }
    form::choice(ui, d, "direction", tl!("Direction:"), (LABEL_W, 160.0), &DIRECTIONS);
    flips(ui, d);
    colorization_body(ui, d);
    ui.add_space(8.0);
    stroke_preview(ui, "art", preview_def(d), vec2(STROKE_W, 64.0));
}

fn pattern_body(ui: &mut egui::Ui, d: &mut Dialog) {
    for (key, text, range) in [("scale", "Scale:", (1.0, 10000.0)), ("spacing", "Spacing:", (0.0, 10000.0))] {
        widgets::label_row(ui, tl!(text), LABEL_W, |ui| {
            if let Some(v) = widgets::range_field(ui, ("brush-pattern", key), clamped(d, key, range), range.0..=range.1, "%", 0, 64.0) {
                d.fields.insert(key.into(), json!(v));
            }
        });
    }
    form::choice(ui, d, "fit", tl!("Fit:"), (LABEL_W, 160.0), &FITS);
    flips(ui, d);
    colorization_body(ui, d);
    ui.add_space(8.0);
    stroke_preview(ui, "pattern", preview_def(d), vec2(STROKE_W, 64.0));
}

fn flips(ui: &mut egui::Ui, d: &mut Dialog) {
    widgets::label_row(ui, tl!("Flip:"), LABEL_W, |ui| {
        form::check(ui, d, "flipAlong", "Flip Along");
        form::check(ui, d, "flipAcross", "Flip Across");
    });
}

/// Colorization: the method and, for Hue Shift, the key colour.
fn colorization_body(ui: &mut egui::Ui, d: &mut Dialog) {
    form::choice(ui, d, "colorization", tl!("Colorization:"), (LABEL_W, 160.0), &COLORIZATION);
    if d.str("colorization") == "hueShift" {
        widgets::label_row(ui, tl!("Key Color:"), LABEL_W, |ui| {
            form::text(ui, d, "keyColor", 90.0);
            let [r, g, b, _] = Color::from_hex(&d.str("keyColor")).unwrap_or(Color::BLACK).to_rgba8(1.0);
            let (rect, _) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::hover());
            ui.painter().rect_filled(rect, 2.0, Color32::from_rgb(r, g, b));
        });
    }
}

fn calligraphic_body(ui: &mut egui::Ui, d: &mut Dialog) {
    let c = calligraphic(d);
    ui.horizontal(|ui| {
        nib_preview(ui, &c, vec2(NIB_W, 84.0));
        ui.add_space(8.0);
        stroke_preview(ui, "calligraphic", serde_json::to_value(&c).unwrap_or_default(), vec2(STROKE_W - NIB_W - 8.0, 84.0));
    });
    ui.add_space(12.0);
    egui::Grid::new("brush-options-calli").num_columns(5).spacing([8.0, 8.0]).show(ui, |ui| {
        for (i, (key, text, suffix, decimals)) in CALLI.into_iter().enumerate() {
            let (Some(&range), Some(&max)) = (Calligraphic::RANGES.get(i), Calligraphic::MAX_VARIATION.get(i)) else { continue };
            label(ui, tl!(text));
            if let Some(v) = widgets::range_field(ui, ("brush-value", key), clamped(d, key, range), range.0..=range.1, suffix, decimals, 64.0) {
                d.fields.insert(key.into(), json!(v));
            }
            let mode = Variation::parse(&d.str(&mode_key(key))).unwrap_or_default();
            let labels = Variation::ALL.map(Variation::label);
            if let Some(m) = widgets::dropdown(ui, ("brush-mode", key), mode.label(), &labels, 104.0).and_then(|i| Variation::ALL.get(i)) {
                d.fields.insert(mode_key(key), json!(m.id()));
            }
            label(ui, tl!("Variation:"));
            let vkey = variation_key(key);
            ui.add_enabled_ui(mode != Variation::Fixed, |ui| {
                if let Some(v) = widgets::range_field(ui, ("brush-var", key), clamped(d, &vkey, (0.0, max)), 0.0..=max, suffix, decimals, 64.0) {
                    d.fields.insert(vkey.clone(), json!(v));
                }
            });
            ui.end_row();
        }
    });
}

fn bristle_body(app: &mut VectorcraftApp, ui: &mut egui::Ui, d: &mut Dialog) {
    form::choice(ui, d, "shape", tl!("Shape:"), (LABEL_W, 150.0), &SHAPES);
    widgets::label_row(ui, tl!("Size:"), LABEL_W, |ui| {
        form::length(ui, d, "size", app.session.general_unit(), 80.0);
    });
    let rail = Tokens::get(ui.ctx()).input_border;
    for (key, text, lo, hi) in BRISTLE {
        widgets::label_row(ui, tl!(text), LABEL_W, |ui| form::slider_field(ui, d, key, lo..=hi, "%", &|_| rail));
    }
    ui.add_space(8.0);
    let p = params(d);
    stroke_preview(ui, "bristle", p, vec2(STROKE_W, 64.0));
}

/// A stroke painted with the brush the fields describe (type `ty`, definition fields `def`).
fn stroke_preview(ui: &mut egui::Ui, ty: &str, mut def: Value, size: egui::Vec2) {
    def["name"] = json!("preview");
    def["type"] = json!(ty);
    let (r, _) = ui.allocate_exact_size(size, Sense::hover());
    crate::panels::brushes::chip(ui, r, &def);
    ui.painter().rect_stroke(r, 0.0, Stroke::new(1.0, Tokens::get(ui.ctx()).input_border), egui::StrokeKind::Inside);
}

/// The nib of `c` at its values (black), and — for values that vary — at their lightest and
/// heaviest (grey), on white like the brush previews, its largest nib filling the box.
fn nib_preview(ui: &mut egui::Ui, c: &Calligraphic, size: egui::Vec2) {
    let t = Tokens::get(ui.ctx());
    let (r, _) = ui.allocate_exact_size(size, Sense::hover());
    let p = ui.painter_at(r);
    p.rect_filled(r, 0.0, Color32::WHITE);
    let guide = Stroke::new(1.0, Color32::from_gray(220));
    p.line_segment([pos2(r.left(), r.center().y), pos2(r.right(), r.center().y)], guide);
    p.line_segment([pos2(r.center().x, r.top()), pos2(r.center().x, r.bottom())], guide);
    let values = [c.angle, c.roundness, c.size];
    let at = |sign: f64| -> [f64; 3] {
        let mut v = values;
        for (i, (x, mode)) in v.iter_mut().zip(c.modes()).enumerate() {
            let (Some(&var), Some(&(lo, hi))) = (c.variation.get(i), Calligraphic::RANGES.get(i)) else { continue };
            if mode != Variation::Fixed {
                *x = (*x + sign * var).clamp(lo, hi);
            }
        }
        v
    };
    let nibs = [at(-1.0), at(1.0), values];
    let largest = nibs.iter().map(|n| n[2]).fold(0.0, f64::max).max(1e-6);
    let scale = (r.height().min(r.width()) - 12.0) / largest as f32;
    for (k, [angle, round, size]) in nibs.into_iter().enumerate() {
        let fill = if k < 2 { Color32::from_gray(200) } else { Color32::BLACK };
        let (a, b) = ((size as f32 * scale / 2.0).max(0.5), (size * round.clamp(1.0, 100.0) / 100.0) as f32 * scale / 2.0);
        let th = (angle as f32).to_radians();
        let (u, v) = (vec2(th.cos(), -th.sin()), vec2(th.sin(), th.cos()));
        let pts: Vec<egui::Pos2> = (0..48)
            .map(|i| {
                let s = i as f32 / 48.0 * std::f32::consts::TAU;
                r.center() + u * (a * s.cos()) + v * (b.max(0.5) * s.sin())
            })
            .collect();
        p.add(egui::Shape::convex_polygon(pts, fill, Stroke::NONE));
    }
    p.rect_stroke(r, 0.0, Stroke::new(1.0, t.input_border), egui::StrokeKind::Inside);
}

/// OK: edit the brush (and rename it). A name another brush has keeps the dialog open.
fn confirm(app: &mut VectorcraftApp, d: &Dialog) -> Result<Value, String> {
    let name = d.str("name").trim().to_string();
    let r = app.run("brush.options", json!({ "name": d.str("__brush"), "newName": name, "params": params(d) }));
    if r.is_ok() {
        app.ui.dialog = None;
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;
    use vectorcraft_engine::Session;

    fn app() -> VectorcraftApp {
        let mut app = VectorcraftApp::new(Session::new(), Default::default());
        app.run("file.new", json!({"width": 300, "height": 200})).unwrap();
        app
    }

    fn set(app: &mut VectorcraftApp, fields: Value) {
        let d = app.ui.dialog.as_mut().unwrap();
        for (k, v) in fields.as_object().unwrap() {
            d.fields.insert(k.clone(), v.clone());
        }
    }

    fn shown(app: &mut VectorcraftApp) -> String {
        crate::tests_labels::painted_text(app, |app, ui| super::super::show(app, ui.ctx()))
    }

    #[test]
    fn calligraphic_options_show_each_value_with_its_variation_and_ok_applies_them() {
        let mut app = app();
        app.run("ui.brushOptions", json!({"name": "6 pt. Flat"})).unwrap();
        let text = shown(&mut app);
        for label in ["Calligraphic Brush Options", "Name:", "Angle:", "Roundness:", "Size:", "Variation:", "Fixed", "OK", "Cancel"] {
            assert!(text.contains(label), "{label} in {text}");
        }
        let d = app.ui.dialog.as_ref().unwrap();
        assert_eq!((d.f64("angle", 0.0), d.f64("roundness", 0.0), d.f64("size", 0.0)), (45.0, 15.0, 6.0));
        // Out-of-range values are kept within their ranges.
        set(&mut app, json!({"angleMode": "random", "angleVariation": 500, "roundness": 140, "sizeMode": "pressure", "sizeVariation": 4}));
        assert!(shown(&mut app).contains("Pressure"));
        super::super::confirm(&mut app).unwrap();
        let def = app.run("brush.get", json!({"name": "6 pt. Flat"})).unwrap();
        assert_eq!(def["modes"], json!(["random", "fixed", "pressure"]));
        assert_eq!((def["variation"].clone(), def["roundness"].clone()), (json!([180.0, 0.0, 4.0]), json!(100.0)));
        // Opened again, it shows what was set.
        app.run("ui.brushOptions", json!({"name": "6 pt. Flat"})).unwrap();
        let d = app.ui.dialog.as_ref().unwrap();
        assert_eq!((d.str("angleMode"), d.f64("sizeVariation", 0.0)), ("random".into(), 4.0));
    }

    #[test]
    fn a_taken_name_keeps_the_dialog_open() {
        let mut app = app();
        app.run("ui.brushOptions", json!({"name": "6 pt. Flat"})).unwrap();
        set(&mut app, json!({"name": "3 pt. Round"}));
        assert!(super::super::confirm(&mut app).is_err());
        assert!(app.ui.dialog.is_some());
    }

    #[test]
    fn bristle_options_and_brushes_without_a_dialog() {
        let mut app = app();
        app.run("ui.brushOptions", json!({"name": "Bristle Round"})).unwrap();
        let text = shown(&mut app);
        for label in ["Bristle Brush Options", "Shape:", "Bristle Length:", "Stiffness:", "Paint Opacity:"] {
            assert!(text.contains(label), "{label} in {text}");
        }
        set(&mut app, json!({"shape": "flatFan", "density": 0, "length": 120}));
        super::super::confirm(&mut app).unwrap();
        let def = app.run("brush.get", json!({"name": "Bristle Round"})).unwrap();
        assert_eq!((def["shape"].clone(), def["density"].clone(), def["length"].clone()), (json!("flatFan"), json!(1.0), json!(120.0)));
        assert!(app.run("ui.brushOptions", json!({"name": "Nope"})).is_err());
        assert!(app.ui.dialog.is_none());
    }

    #[test]
    fn scatter_options_show_ranges_and_ok_applies_them() {
        let mut app = app();
        app.run("ui.brushOptions", json!({"name": "Confetti"})).unwrap();
        let text = shown(&mut app);
        for label in ["Scatter Brush Options", "Size:", "Spacing:", "Scatter:", "Rotation:", "Rotation relative to:", "Colorization:"] {
            assert!(text.contains(label), "{label} in {text}");
        }
        set(
            &mut app,
            json!({"sizeMode": "random", "sizeMin": 150, "sizeMax": 50, "spacingMode": "fixed", "spacingMin": 300, "rotationMode": "random", "rotationMin": -9999, "rotationMax": 45, "rotationRelativeTo": "path", "colorization": "hueShift", "keyColor": "#00ff00"}),
        );
        assert!(shown(&mut app).contains("Key Color:"));
        super::super::confirm(&mut app).unwrap();
        let def = app.run("brush.get", json!({"name": "Confetti"})).unwrap();
        assert_eq!(
            (def["size"].clone(), def["spacing"].clone(), def["rotation"].clone()),
            (json!([50.0, 150.0]), json!([300.0, 300.0]), json!([-360.0, 45.0]))
        );
        assert_eq!(def["rotation_relative_to_path"], json!(true));
        assert_eq!(def["colorization"]["method"], json!("hueShift"));
        // Opened again, it shows what was set.
        app.run("ui.brushOptions", json!({"name": "Confetti"})).unwrap();
        let d = app.ui.dialog.as_ref().unwrap();
        assert_eq!((d.str("sizeMode"), d.str("spacingMode"), d.str("keyColor")), ("random".into(), "fixed".into(), "#00ff00".into()));
    }

    #[test]
    fn art_and_pattern_options_apply_and_strokes_keep_drawing() {
        let mut app = app();
        app.run("ui.brushOptions", json!({"name": "Arrow"})).unwrap();
        let text = shown(&mut app);
        for label in ["Art Brush Options", "Width:", "Brush Scale:", "Direction:", "Flip Along", "Flip Across", "Colorization:"] {
            assert!(text.contains(label), "{label} in {text}");
        }
        set(
            &mut app,
            json!({"width": 5000, "scale": "betweenGuides", "guideStart": 80, "guideEnd": 20, "direction": "bottomToTop", "flipAcross": true, "colorization": "tints"}),
        );
        assert!(shown(&mut app).contains("Guides:"));
        super::super::confirm(&mut app).unwrap();
        let def = app.run("brush.get", json!({"name": "Arrow"})).unwrap();
        assert_eq!((def["width"].clone(), def["direction"].clone(), def["flip_across"].clone()), (json!(1000.0), json!("bottomToTop"), json!(true)));
        assert_eq!(def["scale"], json!({"mode": "betweenGuides", "start": 0.2, "end": 0.8}));
        assert_eq!(def["colorization"], json!({"method": "tints"}));

        app.run("ui.brushOptions", json!({"name": "Chain"})).unwrap();
        let text = shown(&mut app);
        for label in ["Pattern Brush Options", "Scale:", "Spacing:", "Fit:", "Stretch to Fit"] {
            assert!(text.contains(label), "{label} in {text}");
        }
        set(&mut app, json!({"scale": 50, "spacing": -5, "fit": "addSpace", "flipAlong": true}));
        super::super::confirm(&mut app).unwrap();
        let def = app.run("brush.get", json!({"name": "Chain"})).unwrap();
        assert_eq!(
            (def["scale"].clone(), def["spacing"].clone(), def["fit"].clone(), def["flip_along"].clone()),
            (json!(50.0), json!(0.0), json!("addSpace"), json!(true))
        );
        // The tiles stay (the definition keeps its art).
        assert!(def.get("side").is_some_and(|s| !s.is_null()));
    }
}
