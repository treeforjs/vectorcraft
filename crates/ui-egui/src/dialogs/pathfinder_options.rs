//! Pathfinder Options (the Pathfinder panel menu): the precision the Pathfinder buttons compute
//! with and how they tidy their results. OK runs `object.pathfinder.options` (the options live in
//! the preferences); Defaults puts the default values back in the fields.
//!
//! Fields: `precision` (pt), `removeRedundantPoints` and `removeUnpainted` (bools).

use serde_json::{Value, json};
use vectorcraft_doc::Unit;
use vectorcraft_engine::cmd::PathfinderOptions;

use super::swatch_options::grid;
use super::{DialogSpec, form};
use crate::VectorcraftApp;
use crate::state::Dialog;

/// The dialog kind of Pathfinder Options.
pub const KIND: &str = "pathfinderOptions";

pub(super) const SPEC: DialogSpec = DialogSpec { heading: |_| tl!("Pathfinder Options").into(), body, confirm, min_width: 340.0, ..DialogSpec::FORM };

/// Open the dialog with the current options.
pub fn open(app: &mut VectorcraftApp) {
    app.ui.dialog = Some(Dialog::new(KIND, json!(app.session.prefs.pathfinder)));
}

fn confirm(app: &mut VectorcraftApp, d: &Dialog) -> Result<Value, String> {
    let r = app.run("object.pathfinder.options", form::params(d));
    if r.is_ok() {
        app.ui.dialog = None;
    }
    r
}

fn body(_app: &mut VectorcraftApp, ui: &mut egui::Ui, d: &mut Dialog) -> bool {
    grid(ui, |ui| form::length_field(ui, d, "precision", "Precision:", Unit::Points));
    form::caption(ui, tl!("How closely results follow the exact shapes; smaller is more precise."));
    ui.add_space(6.0);
    form::check(ui, d, "removeRedundantPoints", "Remove Redundant Points");
    form::check(ui, d, "removeUnpainted", "Divide and Outline Will Remove Unpainted Artwork");
    ui.add_space(6.0);
    if ui.button(tl!("Defaults")).clicked()
        && let Value::Object(o) = json!(PathfinderOptions::default())
    {
        d.fields.extend(o);
    }
    false
}
