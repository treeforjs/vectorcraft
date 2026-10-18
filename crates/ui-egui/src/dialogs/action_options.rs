//! Actions panel dialogs: New Set (field `name`), Insert Menu Item (field `command`: a command id
//! or a menu label) and Record Stop (fields `message`, `allowContinue`). Hidden fields `__set`,
//! `__action` and `__at` name where the step goes.

use serde_json::{Value, json};

use super::swatch_options::{grid, label};
use super::{DialogSpec, form};
use crate::VectorcraftApp;
use crate::state::Dialog;

pub const NEW_SET: &str = "actionNewSet";
pub const MENU_ITEM: &str = "actionInsertMenuItem";
pub const STOP: &str = "actionInsertStop";

pub(super) const SPEC: DialogSpec = DialogSpec { heading, body, confirm, min_width: 340.0, ..DialogSpec::FORM };

fn heading(d: &Dialog) -> String {
    match d.kind.as_str() {
        NEW_SET => tl!("New Set").into(),
        MENU_ITEM => tl!("Insert Menu Item").into(),
        _ => tl!("Record Stop").into(),
    }
}

/// Open dialog `kind` for action `action` of set `set`.
pub fn open(app: &mut VectorcraftApp, kind: &str, set: usize, action: usize) {
    let fields = match kind {
        NEW_SET => json!({"name": crate::i18n::fmt(tl!("Set {n}"), &[("n", &(app.ui.action_sets.len() + 1).to_string())])}),
        MENU_ITEM => json!({"command": "", "__set": set, "__action": action}),
        _ => json!({"message": "", "allowContinue": true, "__set": set, "__action": action}),
    };
    app.ui.dialog = Some(Dialog::new(kind, fields));
}

fn body(_: &mut VectorcraftApp, ui: &mut egui::Ui, d: &mut Dialog) -> bool {
    match d.kind.as_str() {
        NEW_SET => grid(ui, |ui| {
            label(ui, tl!("Name:"));
            form::text(ui, d, "name", 220.0);
            ui.end_row();
        }),
        MENU_ITEM => {
            grid(ui, |ui| {
                label(ui, tl!("Menu Item:"));
                form::text(ui, d, "command", 220.0);
                ui.end_row();
            });
            let found = crate::panels::actions::resolve_command(&d.str("command"));
            let note = match found {
                Some(id) => crate::panels::actions::Step::new(&id, Value::Null).title(),
                None => tl!("Type a menu command's name, e.g. Select All").into(),
            };
            ui.label(egui::RichText::new(note).small());
        }
        _ => {
            grid(ui, |ui| {
                label(ui, tl!("Message:"));
                form::text(ui, d, "message", 240.0);
                ui.end_row();
            });
            form::check(ui, d, "allowContinue", "Allow Continue");
        }
    }
    false
}

fn confirm(app: &mut VectorcraftApp, d: &Dialog) -> Result<Value, String> {
    let at = |p: &mut Value| {
        for k in ["set", "action", "at"] {
            if let Some(v) = d.fields.get(&format!("__{k}")).filter(|v| !v.is_null()) {
                p[k] = v.clone();
            }
        }
    };
    let r = match d.kind.as_str() {
        NEW_SET => app.run("actions.newSet", json!({"name": d.str("name")})),
        MENU_ITEM => {
            let mut p = json!({"command": d.str("command")});
            at(&mut p);
            app.run("actions.insertMenuItem", p)
        }
        _ => {
            let mut p = json!({"message": d.str("message"), "allowContinue": d.bool("allowContinue")});
            at(&mut p);
            app.run("actions.insertStop", p)
        }
    };
    if r.is_ok() {
        app.ui.dialog = None;
    }
    r
}
