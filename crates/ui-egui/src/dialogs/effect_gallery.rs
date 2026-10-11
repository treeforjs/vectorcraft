//! A transactional editor for live effect stacks. The gallery adds the raster effects already
//! supported by our renderer; vector effects in an existing stack keep their position and data.
//! Public workflow reference: https://helpx.adobe.com/illustrator/using/effects.html
//! Algorithms, UI and synthetic tests are original; no reference application is used.

use egui::{TextureHandle, vec2};
use serde_json::{Value, json};
use vectorcraft_doc::Effect;

use super::{DialogSpec, effect, form};
use crate::state::Dialog;
use crate::{VectorcraftApp, widgets};

pub const KIND: &str = "effectGallery";
const COMMAND: &str = "effect.setStack";
const MAX_EFFECTS: usize = 128;

pub(super) const SPEC: DialogSpec = DialogSpec {
    heading: |_| tl!("Effect Gallery").into(),
    body,
    confirm,
    preview: true,
    min_width: 690.0,
    max_width: Some(690.0),
    ..DialogSpec::FORM
};

pub fn open(app: &mut VectorcraftApp, p: &Value) -> Result<Value, String> {
    if app.ui.dialog.is_some() || app.session.in_interaction() {
        return Err("Close the current dialog or finish the current interaction first".into());
    }
    let st = app.session.active().ok_or("Effect Gallery: open a document")?;
    let ids = st.selection.subjects();
    if ids.is_empty() {
        return Err("Effect Gallery: select artwork".into());
    }
    let item = p.get("item").cloned().unwrap_or_else(|| json!(app.session.appearance_item()));
    let index = if item.is_null() {
        None
    } else {
        Some(item.as_u64().and_then(|n| usize::try_from(n).ok()).ok_or("`item` must be an appearance item index or null")?)
    };
    let mut stack: Option<Vec<Effect>> = None;
    for id in ids {
        let node = st.doc.node(*id).ok_or("The selected object no longer exists")?;
        let effects = node.appearance.effects_at(index).ok_or("The targeted appearance item is missing on selected artwork")?;
        if let Some(first) = &stack {
            if first != effects {
                return Err("Selected objects have different effect stacks. Edit them separately.".into());
            }
        } else {
            stack = Some(effects.clone());
        }
    }
    let stack = stack.unwrap_or_default();
    if stack.len() > MAX_EFFECTS {
        return Err("Effect Gallery supports at most 128 effects in one stack".into());
    }
    if stack.iter().any(|e| vectorcraft_effects::effect_info(&e.id).is_none()) {
        return Err("This stack contains unavailable effects. Install their plug-ins before editing the stack.".into());
    }
    app.ui.dialog = Some(Dialog::new(
        KIND,
        json!({
            "effects": stack, "selected": 0, "preview": true,
            "__original": stack, "__ids": ids.iter().map(|id| id.0).collect::<Vec<_>>(),
            "__item": item, "__doc": st.uid, "__catalog": "blur.gaussian"
        }),
    ));
    Ok(Value::Null)
}

fn command(app: &VectorcraftApp, d: &Dialog) -> Result<Value, String> {
    if app.session.active().map(|s| s.uid) != d.fields.get("__doc").and_then(Value::as_u64) {
        return Err("Return to the document where Effect Gallery was opened".into());
    }
    Ok(json!({"ids": d.fields.get("__ids"), "item": d.fields.get("__item"), "effects": d.fields.get("effects")}))
}

/// A failed preview restores the baseline and advances the revision through cancellation,
/// preventing a stale thumbnail/canvas from displaying an earlier successful preview.
fn preview_stack(app: &mut VectorcraftApp, params: &Value) -> Result<(), String> {
    if let Err(error) = app.session.preview(COMMAND, params) {
        app.session.cancel_interaction().map_err(|e| e.to_string())?;
        return Err(error.to_string());
    }
    Ok(())
}

/// Rebuild from the interaction snapshot, so drag previews never accumulate filters.
fn update_preview(app: &mut VectorcraftApp, d: &mut Dialog) -> Result<(), String> {
    let p = command(app, d)?;
    if !d.bool("preview") || d.fields.get("effects") == d.fields.get("__original") {
        if app.session.in_interaction() {
            app.session.cancel_interaction().map_err(|e| e.to_string())?;
        }
        d.fields.remove("__previewed");
        d.fields.remove("__failed");
    } else if d.fields.get("__previewed") != Some(&p) || !app.session.in_interaction() {
        let revision = app.session.active().map(|s| s.revision);
        if let Some(failed) = d.fields.get("__failed")
            && failed.get("params") == Some(&p)
            && failed.get("revision").and_then(Value::as_u64) == revision
        {
            return Err(failed.get("error").and_then(Value::as_str).unwrap_or_default().into());
        }
        app.session.begin_interaction("Effect Gallery").map_err(|e| e.to_string())?;
        if let Err(error) = preview_stack(app, &p) {
            d.fields.insert("__failed".into(), json!({"params": p, "revision": app.session.active().map(|s| s.revision), "error": error}));
            return Err(error);
        }
        d.fields.remove("__failed");
        d.fields.insert("__previewed".into(), p);
    }
    Ok(())
}

fn confirm(app: &mut VectorcraftApp, d: &Dialog) -> Result<Value, String> {
    let p = command(app, d)?;
    if d.fields.get("effects") == d.fields.get("__original") {
        if app.session.in_interaction() {
            app.session.cancel_interaction().map_err(|e| e.to_string())?;
        }
    } else if app.session.in_interaction() {
        // An invalid edit must neither commit an earlier preview nor close the dialog.
        preview_stack(app, &p)?;
        app.session.commit_interaction().map_err(|e| e.to_string())?;
    } else {
        app.run(COMMAND, p)?;
    }
    app.ui.dialog = None;
    Ok(Value::Null)
}

fn body(app: &mut VectorcraftApp, ui: &mut egui::Ui, d: &mut Dialog) -> bool {
    let raw = d.fields.get("effects").and_then(Value::as_array);
    if raw.is_none_or(|rows| rows.len() > MAX_EFFECTS) {
        ui.label(crate::i18n::msg("The effect stack must be an array with at most 128 effects."));
        return false;
    }
    let parsed: Result<Vec<Effect>, _> = serde_json::from_value(d.fields.get("effects").cloned().unwrap_or_default());
    let Ok(mut rows) = parsed else {
        ui.label(crate::i18n::msg("The effect stack contains an invalid entry."));
        return false;
    };
    let mut selected = d.fields.get("selected").and_then(Value::as_u64).and_then(|n| usize::try_from(n).ok()).unwrap_or(0);
    selected = selected.min(rows.len().saturating_sub(1));
    let narrow = ui.available_width() < 670.0;
    let layout = if narrow { egui::Layout::top_down(egui::Align::Min) } else { egui::Layout::left_to_right(egui::Align::Min) };
    egui::ScrollArea::vertical().id_salt("gallery-body").max_height((ui.ctx().content_rect().height() - 180.0).max(120.0)).show(ui, |ui| {
        ui.scope_builder(egui::UiBuilder::new().layout(layout), |ui| {
            ui.vertical(|ui| {
                ui.set_width(300.0_f32.min(ui.available_width()));
                preview_image(app, ui, d);
                ui.add_space(6.0);
                let mut enabled = d.bool("preview");
                if ui.checkbox(&mut enabled, tl!("Preview")).changed() {
                    d.fields.insert("preview".into(), json!(enabled));
                }
                ui.label(egui::RichText::new(tl!("Preview of the first selected object")).small());
            });
            ui.separator();
            ui.vertical(|ui| {
                ui.set_width(355.0_f32.min(ui.available_width()));
                widgets::subheader(ui, tl!("Raster Effects"));
                let catalog: Vec<_> = vectorcraft_effects::effect_catalog()
                    .into_iter()
                    .filter(|e| matches!(e.id, "blur.gaussian" | "stylize.feather") || vectorcraft_effects::pixel::PIXEL_EFFECTS.contains(&e.id))
                    .collect();
                let labels: Vec<String> = catalog
                    .iter()
                    .map(|e| format!("{} / {}", tl!(e.menu.last().copied().unwrap_or("Effect")), tl!(e.label.trim_end_matches('…'))))
                    .collect();
                let names: Vec<&str> = labels.iter().map(String::as_str).collect();
                let chosen = catalog.iter().position(|e| e.id == d.str("__catalog")).unwrap_or(0);
                if let Some(i) = widgets::dropdown_names(ui, "gallery-catalog", names.get(chosen).copied().unwrap_or_default(), &names, 260.0)
                    && let Some(entry) = catalog.get(i)
                {
                    d.fields.insert("__catalog".into(), json!(entry.id));
                }
                if ui.add_enabled(rows.len() < MAX_EFFECTS, egui::Button::new(tl!("Add Effect"))).clicked()
                    && let Some(fx) = vectorcraft_effects::new_effect(&d.str("__catalog"), &json!({}))
                {
                    rows.push(fx);
                    selected = rows.len() - 1;
                }
                ui.add_space(8.0);
                widgets::subheader(ui, tl!("Effect Stack"));
                ui.label(egui::RichText::new(tl!("Raster filters run from top to bottom")).small());
                egui::ScrollArea::vertical().id_salt("gallery-stack").max_height(150.0).show(ui, |ui| {
                    for (index, row) in rows.iter_mut().enumerate() {
                        ui.push_id(index, |ui| {
                            ui.horizontal(|ui| {
                                ui.checkbox(&mut row.visible, "");
                                let label = crate::panels::appearance::effect_label(&row.id);
                                if ui.selectable_label(index == selected, tl!(&label)).clicked() {
                                    selected = index;
                                }
                            });
                        });
                    }
                });
                ui.horizontal(|ui| {
                    if ui.add_enabled(selected > 0 && !rows.is_empty(), egui::Button::new(tl!("Move Up"))).clicked() {
                        rows.swap(selected, selected - 1);
                        selected -= 1;
                    }
                    if ui.add_enabled(selected + 1 < rows.len(), egui::Button::new(tl!("Move Down"))).clicked() {
                        rows.swap(selected, selected + 1);
                        selected += 1;
                    }
                    if ui.add_enabled(!rows.is_empty(), egui::Button::new(tl!("Delete"))).clicked() {
                        rows.remove(selected);
                        selected = selected.min(rows.len().saturating_sub(1));
                    }
                });
                if let Some(row) = rows.get_mut(selected) {
                    ui.separator();
                    let id = row.id.clone();
                    let mut fields = Dialog::new("galleryParameters", vectorcraft_effects::merged_params(&id, &row.params));
                    let before = form::params(&fields);
                    let relative = fields.bool("relative");
                    let doc = vectorcraft_effects::effect_info(&id).map(|e| e.params).unwrap_or_default();
                    ui.push_id(("gallery-params", selected, &id), |ui| {
                        egui::ScrollArea::vertical().id_salt("gallery-parameters").max_height(190.0).show(ui, |ui| {
                            form::param_fields(
                                ui,
                                &mut fields,
                                &|k| vectorcraft_effects::is_length(&id, k, relative),
                                &|k| effect::choices(&id, k),
                                &|k| effect::doc_rank(doc, k),
                                app.session.general_unit(),
                            );
                        });
                    });
                    let after = form::params(&fields);
                    if after != before {
                        row.params = after;
                    }
                }
            });
        });
    });
    d.fields.insert("effects".into(), json!(rows));
    d.fields.insert("selected".into(), json!(selected));
    let revision = app.session.active().map(|s| s.revision);
    if let Err(error) = update_preview(app, d) {
        ui.colored_label(crate::theme::Tokens::get(ui.ctx()).error, crate::i18n::msg(&error));
    }
    if revision != app.session.active().map(|s| s.revision) {
        ui.ctx().request_repaint();
    }
    false
}

/// A bounded thumbnail cached by document revision, never a full-resolution render per frame.
fn preview_image(app: &VectorcraftApp, ui: &mut egui::Ui, d: &Dialog) {
    let Some(st) = app.session.active().filter(|s| Some(s.uid) == d.fields.get("__doc").and_then(Value::as_u64)) else { return };
    let Some(id) = d.fields.get("__ids").and_then(Value::as_array).and_then(|ids| ids.first()).and_then(Value::as_u64) else { return };
    let key = (st.uid, st.revision, id);
    let cache_id = ui.id().with("gallery-preview");
    let cached: Option<((u64, u64, u64), TextureHandle)> = ui.data(|m| m.get_temp(cache_id));
    let texture = match cached.filter(|(stamp, _)| *stamp == key) {
        Some((_, texture)) => Some(texture),
        None => {
            let mut renderer = vectorcraft_render::Renderer::new();
            renderer.render_thumbnail(&st.doc, vectorcraft_doc::NodeId(id), 300).map(|image| {
                let image = egui::ColorImage::from_rgba_premultiplied([image.width as usize, image.height as usize], &image.pixels);
                let texture = ui.ctx().load_texture("effect-gallery", image, egui::TextureOptions::LINEAR);
                ui.data_mut(|m| m.insert_temp(cache_id, (key, texture.clone())));
                texture
            })
        }
    };
    if let Some(texture) = texture {
        let (rect, _) = ui.allocate_exact_size(vec2(300.0_f32.min(ui.available_width()), 300.0_f32.min(ui.available_width())), egui::Sense::hover());
        ui.painter().rect_filled(rect, 0.0, egui::Color32::from_gray(235));
        ui.painter().image(texture.id(), rect, egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
    }
}

#[cfg(test)]
mod tests;
