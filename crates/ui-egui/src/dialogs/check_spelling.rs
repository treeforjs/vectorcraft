//! Edit › Spelling › Check Spelling… (`type.checkSpelling`): steps through the document's
//! misspelled and repeated words and sentences that start in lower case, one at a time, with the
//! word in context and suggestions. Ignore skips it, Ignore All skips the word everywhere until
//! the dialog closes, Change and Change All replace it (`type.spelling.change` /
//! `type.spelling.changeAll`), and Add puts it in the custom dictionary. Options toggles what the
//! check looks for (`type.spelling.options`).
//!
//! Edit › Spelling › Edit Custom Dictionary… (dialog `customDictionary`) lists the dictionary's
//! words, with Add and Remove (`type.spelling.dictionary`).
//!
//! Check Spelling's fields: `findings` (the open problems, the current one first), `ignore`
//! (Ignore All words), `skipped` (`[id, start, word]` of each Ignore), `to` (the replacement) and
//! the option bools. Set `action` to `ignore`, `ignoreAll`, `change`, `changeAll`, `add` or
//! `recheck` and confirm to take that step. Custom Dictionary's fields: `words`, `word` (the
//! entry) and `selected`; `action` `add` or `remove`.

use serde_json::{Value, json};

use super::DialogSpec;
use crate::state::Dialog;
use crate::theme::{self, Tokens};
use crate::{VectorcraftApp, widgets};

/// The dialog kind of Check Spelling.
pub const KIND: &str = "checkSpelling";
/// The dialog kind of Edit Custom Dictionary.
pub const DICTIONARY: &str = "customDictionary";

pub(super) const SPEC: DialogSpec = DialogSpec {
    heading: |d| if d.kind == DICTIONARY { tl!("Edit Custom Dictionary").into() } else { tl!("Check Spelling").into() },
    body,
    confirm,
    // Close only: the steps are buttons of the body.
    ok: None,
    min_width: 420.0,
    max_width: Some(520.0),
    ..DialogSpec::FORM
};

const OPTIONS: [(&str, &str); 5] = [
    ("repeatedWords", "Repeated Words"),
    ("uncapitalizedStart", "Uncapitalized Start of Sentence"),
    ("ignoreUppercase", "Words That Are All Uppercase"),
    ("ignoreRomanNumerals", "Roman Numerals"),
    ("ignoreWordsWithNumbers", "Words with Numbers"),
];

/// Open Check Spelling on the document (its first problem shown, or "no problems found").
pub fn open(app: &mut VectorcraftApp) -> Result<Value, String> {
    let mut d = Dialog::new(KIND, app.run("type.spelling.options", json!({}))?);
    d.fields.insert("ignore".into(), json!([]));
    d.fields.insert("skipped".into(), json!([]));
    recheck(app, &mut d)?;
    let n = d.fields.get("findings").and_then(Value::as_array).map_or(0, Vec::len);
    app.ui.dialog = Some(d);
    Ok(json!({ "findings": n }))
}

/// Open Edit Custom Dictionary.
pub fn open_dictionary(app: &mut VectorcraftApp) -> Result<Value, String> {
    let r = app.run("type.spelling.dictionary", json!({}))?;
    app.ui.dialog = Some(Dialog::new(DICTIONARY, json!({"words": r["words"], "word": "", "selected": ""})));
    Ok(Value::Null)
}

/// Check again, leaving out what was ignored; the replacement starts as the first suggestion.
fn recheck(app: &mut VectorcraftApp, d: &mut Dialog) -> Result<(), String> {
    let ignore = d.fields.get("ignore").cloned().unwrap_or(json!([]));
    let skipped = d.fields.get("skipped").and_then(Value::as_array).cloned().unwrap_or_default();
    let r = app.run("type.checkSpelling", json!({ "ignore": ignore }))?;
    let findings: Vec<Value> = r["findings"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|f| !skipped.contains(&json!([f["id"], f["start"], f["word"]])))
        .collect();
    let to = findings.first().map_or(String::new(), |f| f["suggestions"][0].as_str().unwrap_or_default().to_string());
    d.fields.insert("to".into(), json!(to));
    d.fields.insert("findings".into(), json!(findings));
    Ok(())
}

fn current(d: &Dialog) -> Option<Value> {
    d.fields.get("findings").and_then(Value::as_array).and_then(|f| f.first()).cloned()
}

fn push(d: &mut Dialog, key: &str, v: Value) {
    if let Some(Value::Array(a)) = d.fields.get_mut(key) {
        a.push(v);
    } else {
        d.fields.insert(key.into(), json!([v]));
    }
}

/// Take one step of Check Spelling.
fn step(app: &mut VectorcraftApp, d: &mut Dialog, action: &str) -> Result<Value, String> {
    let f = current(d);
    let word = f.as_ref().and_then(|f| f["word"].as_str()).unwrap_or_default().to_string();
    let to = d.str("to");
    match (action, &f) {
        ("recheck", _) => {}
        ("ignore", Some(f)) => push(d, "skipped", json!([f["id"], f["start"], f["word"]])),
        ("ignoreAll", Some(_)) => push(d, "ignore", json!(word)),
        ("change", Some(f)) => {
            app.run("type.spelling.change", json!({"id": f["id"], "start": f["start"], "end": f["end"], "word": word, "to": to}))?;
        }
        ("changeAll", Some(_)) => {
            app.run("type.spelling.changeAll", json!({"word": word, "to": to}))?;
        }
        ("add", Some(_)) => {
            app.run("type.spelling.dictionary", json!({ "add": word }))?;
        }
        (_, None) => return Err("nothing left to check".into()),
        (_, _) => return Err("unknown action".into()),
    }
    // Options set in the dialog apply to the next check.
    let opts: serde_json::Map<String, Value> = OPTIONS.iter().filter_map(|(k, _)| Some(((*k).to_string(), d.fields.get(*k)?.clone()))).collect();
    app.run("type.spelling.options", Value::Object(opts))?;
    recheck(app, d)?;
    Ok(json!({ "findings": d.fields.get("findings").and_then(Value::as_array).map_or(0, Vec::len) }))
}

fn confirm(app: &mut VectorcraftApp, d: &Dialog) -> Result<Value, String> {
    let mut d = d.clone();
    let action = d.str("action");
    d.fields.remove("action");
    let r = if d.kind == DICTIONARY { dictionary_step(app, &mut d, &action) } else { step(app, &mut d, &action) };
    // A failed step leaves the dialog as it was (without the action).
    if let Some(open) = app.ui.dialog.as_mut() {
        if r.is_ok() {
            *open = d;
        } else {
            open.fields.remove("action");
        }
    }
    r
}

fn dictionary_step(app: &mut VectorcraftApp, d: &mut Dialog, action: &str) -> Result<Value, String> {
    let r = match action {
        "add" => app.run("type.spelling.dictionary", json!({ "add": d.str("word") }))?,
        "remove" => {
            let w = if d.str("selected").is_empty() { d.str("word") } else { d.str("selected") };
            app.run("type.spelling.dictionary", json!({ "remove": w }))?
        }
        _ => return Err("unknown action".into()),
    };
    d.fields.insert("words".into(), r["words"].clone());
    d.fields.insert("word".into(), json!(""));
    d.fields.insert("selected".into(), json!(""));
    Ok(r)
}

fn body(app: &mut VectorcraftApp, ui: &mut egui::Ui, d: &mut Dialog) -> bool {
    if d.kind == DICTIONARY {
        dictionary_body(app, ui, d);
        return false;
    }
    let t = Tokens::get(ui.ctx());
    let f = current(d);
    let mut act: Option<&str> = None;
    match &f {
        None => {
            widgets::dim_label(ui, tl!("No spelling problems found."));
        }
        Some(f) => {
            let issue = match f["issue"].as_str() {
                Some("repeated") => tl!("Repeated Word:"),
                Some("uncapitalized") => tl!("Uncapitalized Start of Sentence:"),
                _ => tl!("Not Found in Dictionary:"),
            };
            widgets::dim_label(ui, issue);
            ui.label(egui::RichText::new(f["word"].as_str().unwrap_or_default()).font(theme::semibold(14.0)).color(t.text_strong));
            if let Some(ctx) = context(app, f) {
                ui.label(egui::RichText::new(ctx).size(11.0).color(t.text_dim));
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(tl!("Change to:"));
                super::form::text(ui, d, "to", 220.0);
            });
            let sugg = f["suggestions"].as_array().cloned().unwrap_or_default();
            widgets::list_box(ui, |ui| {
                egui::ScrollArea::vertical().max_height(120.0).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    if sugg.is_empty() {
                        widgets::dim_label(ui, tl!("No suggestions"));
                    }
                    for s in sugg.iter().filter_map(Value::as_str) {
                        let on = d.str("to") == s;
                        let r = ui.selectable_label(on, s);
                        if r.clicked() {
                            d.fields.insert("to".into(), json!(s));
                        }
                        if r.double_clicked() {
                            act = Some("change");
                        }
                    }
                });
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                for (a, label) in [("ignore", tl!("Ignore")), ("ignoreAll", tl!("Ignore All")), ("add", tl!("Add"))] {
                    if ui.button(label).clicked() {
                        act = Some(a);
                    }
                }
            });
            ui.horizontal(|ui| {
                for (a, label) in [("change", tl!("Change")), ("changeAll", tl!("Change All"))] {
                    if ui.button(label).clicked() {
                        act = Some(a);
                    }
                }
            });
        }
    }
    ui.add_space(8.0);
    egui::CollapsingHeader::new(tl!("Options")).id_salt("checkSpellingOptions").show(ui, |ui| {
        let before: Vec<bool> = OPTIONS.iter().map(|(k, _)| d.bool(k)).collect();
        widgets::dim_label(ui, tl!("Find:"));
        for (k, label) in OPTIONS.iter().take(2) {
            super::form::check(ui, d, k, label);
        }
        widgets::dim_label(ui, tl!("Ignore:"));
        for (k, label) in OPTIONS.iter().skip(2) {
            super::form::check(ui, d, k, label);
        }
        if OPTIONS.iter().zip(&before).any(|((k, _), b)| d.bool(k) != *b) {
            act = Some("recheck");
        }
    });
    if let Some(a) = act
        && let Err(e) = step(app, d, a)
    {
        app.status(e);
    }
    false
}

/// The finding's sentence-ish surroundings: up to 30 characters each side, on one line.
fn context(app: &VectorcraftApp, f: &Value) -> Option<String> {
    let id = vectorcraft_doc::NodeId(f["id"].as_u64()?);
    let vectorcraft_doc::NodeKind::Text(t) = &app.session.active()?.doc.node(id)?.kind else { return None };
    let text = t.plain_text();
    let (a, b) = (usize::try_from(f["start"].as_u64()?).ok()?, usize::try_from(f["end"].as_u64()?).ok()?);
    let before: String = text.get(..a)?.chars().rev().take(30).collect::<Vec<_>>().into_iter().rev().collect();
    let after: String = text.get(b..)?.chars().take(30).collect();
    let flat = |s: &str| s.replace(['\n', '\r', '\t'], " ");
    Some(format!("…{}[{}]{}…", flat(&before), text.get(a..b)?, flat(&after)))
}

fn dictionary_body(app: &mut VectorcraftApp, ui: &mut egui::Ui, d: &mut Dialog) {
    let words = d.fields.get("words").and_then(Value::as_array).cloned().unwrap_or_default();
    let mut act = None;
    widgets::list_box(ui, |ui| {
        egui::ScrollArea::vertical().max_height(200.0).show(ui, |ui| {
            ui.set_width(ui.available_width());
            if words.is_empty() {
                widgets::dim_label(ui, tl!("No words yet"));
            }
            for w in words.iter().filter_map(Value::as_str) {
                if ui.selectable_label(d.str("selected") == w, w).clicked() {
                    d.fields.insert("selected".into(), json!(w));
                    d.fields.insert("word".into(), json!(w));
                }
            }
        });
    });
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.label(tl!("Word:"));
        let r = super::form::text_edit(ui, d, "word", 200.0);
        if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) && !d.str("word").trim().is_empty() {
            act = Some("add");
        }
    });
    ui.horizontal(|ui| {
        if ui.add_enabled(!d.str("word").trim().is_empty(), egui::Button::new(tl!("Add"))).clicked() {
            act = Some("add");
        }
        if ui.add_enabled(!d.str("selected").is_empty() || !d.str("word").trim().is_empty(), egui::Button::new(tl!("Remove"))).clicked() {
            act = Some("remove");
        }
    });
    if let Some(a) = act
        && let Err(e) = dictionary_step(app, d, a)
    {
        app.status(e);
    }
}
