//! Symbol Options: 9-slice scaling for a symbol (`symbol.options`). Fields: `nineSlice` and the
//! guides `left`, `top`, `right`, `bottom` (points in from the art's edges), plus `__name`, the
//! symbol, and `__size`, its art's natural width and height (for the hint).

use serde_json::{Value, json};
use vectorcraft_doc::nine_slice::NineSlice;

use super::swatch_options::{grid, label};
use super::{DialogSpec, form};
use crate::VectorcraftApp;
use crate::state::Dialog;

pub const KIND: &str = "symbolOptions";

pub(super) const SPEC: DialogSpec = DialogSpec { heading: |_| tl!("Symbol Options").into(), body, confirm, min_width: 320.0, ..DialogSpec::FORM };

/// Open Symbol Options on symbol `name`.
pub fn open(app: &mut VectorcraftApp, name: &str) -> Result<Value, String> {
    let d = &app.session.active().ok_or("no document open")?.doc;
    let sym = d.symbols.iter().find(|s| s.name == name).ok_or_else(|| format!("no symbol `{name}`"))?;
    let n = d.symbol_natural_xf(name).as_coeffs();
    let (w, h) = sym.art.geometric_bounds().map_or((0.0, 0.0), |b| (b.width() * n[0].abs(), b.height() * n[3].abs()));
    let g = sym.slice.unwrap_or_else(|| NineSlice::quarter(w, h));
    let fields = json!({
        "nineSlice": sym.slice.is_some(),
        "left": g.left, "top": g.top, "right": g.right, "bottom": g.bottom,
        "__name": name, "__size": [w, h],
    });
    app.ui.dialog = Some(Dialog::new(KIND, fields));
    Ok(Value::Null)
}

fn body(app: &mut VectorcraftApp, ui: &mut egui::Ui, d: &mut Dialog) -> bool {
    let unit = app.session.general_unit();
    grid(ui, |ui| {
        label(ui, tl!("Name:"));
        ui.label(d.str("__name"));
        ui.end_row();
    });
    form::check(ui, d, "nineSlice", "Enable Guides for 9-Slice Scaling");
    ui.add_enabled_ui(d.bool("nineSlice"), |ui| {
        grid(ui, |ui| {
            for (k, l) in [("left", tl!("Left:")), ("top", tl!("Top:")), ("right", tl!("Right:")), ("bottom", tl!("Bottom:"))] {
                label(ui, l);
                form::length(ui, d, k, unit, 90.0);
                ui.end_row();
            }
        });
        form::caption(ui, tl!("Corners keep their size when instances are scaled; the edges and centre stretch."));
    });
    false
}

fn confirm(app: &mut VectorcraftApp, d: &Dialog) -> Result<Value, String> {
    let mut p = json!({"name": d.str("__name"), "nineSlice": d.bool("nineSlice")});
    if d.bool("nineSlice") {
        p["guides"] = json!({"left": d.f64("left", 0.0), "top": d.f64("top", 0.0), "right": d.f64("right", 0.0), "bottom": d.f64("bottom", 0.0)});
    }
    let r = app.run("symbol.options", p);
    if r.is_ok() {
        app.ui.dialog = None;
    }
    r
}
