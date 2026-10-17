//! Character panel: font family / style, a variable font's axes, size, leading, kerning, tracking,
//! vertical and horizontal scale, baseline shift, character rotation and the caps / underline /
//! strikethrough toggles.

use std::sync::{Mutex, OnceLock};

use egui::{Key, Ui, vec2};
use serde_json::{Value, json};
use vectorcraft_doc::{CharStyle, NodeId, NodeKind};
use vectorcraft_tools::{Mods, ToolKey};

use super::{first_selected, pstate, set_pstate};
use crate::VectorcraftApp;
use crate::theme::Tokens;
use crate::widgets::{self, SpinPick, menu_item};

/// The Font Size and Leading dropdowns' presets in points (Leading's after its Auto entry), shown
/// in the type unit.
pub const SIZE_PRESETS: [f64; 15] = [6.0, 7.0, 8.0, 9.0, 10.0, 11.0, 12.0, 14.0, 18.0, 21.0, 24.0, 36.0, 48.0, 60.0, 72.0];
/// The Kerning and Tracking dropdowns' presets in 1/1000 em.
pub const TRACKING_PRESETS: [f64; 14] = [-100.0, -75.0, -50.0, -25.0, -10.0, -5.0, 0.0, 5.0, 10.0, 25.0, 50.0, 75.0, 100.0, 200.0];
pub const SCALE_PRESETS: [f64; 8] = [25.0, 50.0, 75.0, 90.0, 100.0, 110.0, 125.0, 150.0];

/// The character style shown by the panel: the selected range's (or the caret's) while the Type
/// tool edits text, else the first selected text object's first run. With it, the paragraph
/// attributes of the paragraph the selection starts in (else of the first paragraph).
pub(crate) fn text_style(app: &VectorcraftApp) -> Option<(CharStyle, vectorcraft_doc::ParaStyle)> {
    if let Some((id, a, b)) = text_editing(app)
        && let Some(NodeKind::Text(t)) = app.session.active().and_then(|d| d.doc.node(id)).map(|n| &n.kind)
    {
        return Some((vectorcraft_text::edit::insertion_style(&t.runs, a, b), t.para_at(t.paragraphs_in(a, a).start).clone()));
    }
    let n = first_selected(app)?;
    match &n.kind {
        NodeKind::Text(t) => Some((t.first_style(), t.para_at(0).clone())),
        _ => None,
    }
}

/// `f` of the character styles the panels act on: the runs the Type tool's selection covers (the
/// caret's style at a caret), else every run of the selected type, groups' included. `None` (a
/// blank field) where they differ, or with no type.
pub(crate) fn shared<T: PartialEq>(app: &VectorcraftApp, f: impl Fn(&CharStyle) -> T) -> Option<T> {
    let st = app.session.active()?;
    let text = |id: NodeId| match st.doc.node(id).map(|n| &n.kind) {
        Some(NodeKind::Text(t)) => Some(t),
        _ => None,
    };
    let mut acc = Shared::None;
    if let Some((id, a, b)) = text_editing(app) {
        let t = text(id)?;
        if b <= a {
            return Some(f(&vectorcraft_text::edit::insertion_style(&t.runs, a, b)));
        }
        let mut end = 0;
        for r in &t.runs {
            let start = end;
            end += r.text.len();
            if start < b && end > a {
                acc.add(f(&r.style));
            }
        }
    } else {
        for n in st.selection.objects.iter().filter_map(|id| st.doc.node(*id)) {
            n.walk(&mut |c| {
                if let NodeKind::Text(t) = &c.kind {
                    t.runs.iter().for_each(|r| acc.add(f(&r.style)));
                }
            });
        }
    }
    acc.value()
}

/// The value a run of styles share so far ([`shared`]).
enum Shared<T> {
    None,
    Same(T),
    Differ,
}

impl<T: PartialEq> Shared<T> {
    fn add(&mut self, v: T) {
        match self {
            Shared::None => *self = Shared::Same(v),
            Shared::Same(x) if *x != v => *self = Shared::Differ,
            _ => {}
        }
    }
    fn value(self) -> Option<T> {
        match self {
            Shared::Same(v) => Some(v),
            _ => None,
        }
    }
}

fn style(app: &mut VectorcraftApp, p: Value) {
    char_cmd(app, "text.setStyle", p);
}
fn format(app: &mut VectorcraftApp, p: Value) {
    char_cmd(app, "text.setFormat", p);
}

/// Apply character attributes: to the selected range while the Type tool edits text (the whole
/// object when nothing is selected), else to the selected text objects.
fn char_cmd(app: &mut VectorcraftApp, cmd: &str, p: Value) {
    if text_editing(app).is_none() {
        app.run(cmd, p).ok();
        return;
    }
    range_style(app, p);
}

/// The faces Type › Bold and Type › Italic switch to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Face {
    Bold,
    Italic,
}

/// Words of a style name that set its weight: a style without any is the family's regular weight.
const WEIGHTS: [&str; 16] = [
    "thin",
    "hairline",
    "extralight",
    "ultralight",
    "light",
    "book",
    "medium",
    "semibold",
    "demibold",
    "demi",
    "bold",
    "extrabold",
    "ultrabold",
    "heavy",
    "black",
    "ultra",
];

/// Words that name the regular weight, upright: not part of what Bold and Italic keep.
const PLAIN: [&str; 5] = ["regular", "normal", "roman", "plain", "upright"];

/// A style name's (bold, regular weight, italic, the other words): `Bold Italic` is (true, false,
/// true, []), `Narrow Italic` (false, true, true, ["narrow"]), `SemiBold` (false, false, false, []).
fn face_of(style: &str) -> (bool, bool, bool, Vec<String>) {
    let words: Vec<String> = style.split([' ', '-', '_']).filter(|w| !w.is_empty()).map(str::to_ascii_lowercase).collect();
    let italic = words.iter().any(|w| w == "italic" || w == "oblique");
    let bold = words.iter().any(|w| w == "bold");
    let regular = !words.iter().any(|w| WEIGHTS.contains(&w.as_str()));
    let mut rest: Vec<String> =
        words.into_iter().filter(|w| !WEIGHTS.contains(&w.as_str()) && !PLAIN.contains(&w.as_str()) && w != "italic" && w != "oblique").collect();
    rest.sort();
    (bold, regular, italic, rest)
}

/// The style of `styles` that is bold (else of regular weight) and italic (else upright), with the
/// same other words as style `like` (a Narrow face stays Narrow, a plain one plain).
fn pick_face<'a>(styles: &'a [String], bold: bool, italic: bool, like: &str) -> Option<&'a str> {
    let (.., keep) = face_of(like);
    styles
        .iter()
        .map(String::as_str)
        .filter(|s| {
            let (b, r, i, rest) = face_of(s);
            i == italic && (if bold { b } else { r }) && rest == keep
        })
        // `Regular` before `Regular Text`-like doubles: the plainest name is the family's own face.
        .min_by_key(|s| s.len())
}

/// Type › Bold and Type › Italic (Cmd+Shift+B / Cmd+Shift+I while the Type tool edits text, as in
/// page layout apps): the family's own Bold or Italic face for the text the panels act on, or back
/// to its regular weight (upright) when it already is bold (italic). No fake bold or slant: a family
/// without that face is left as it is, and the message says so (#724).
pub(crate) fn toggle_face(app: &mut VectorcraftApp, face: Face) -> Result<Value, String> {
    let (s, _) = text_style(app).ok_or("select some type first")?;
    let styles = vectorcraft_text::FontDb::global().styles(&s.font_family);
    let (bold, _, italic, _) = face_of(&s.font_style);
    let (want_bold, want_italic) = match face {
        Face::Bold => (!bold, italic),
        Face::Italic => (bold, !italic),
    };
    let Some(name) = pick_face(&styles, want_bold, want_italic, &s.font_style) else {
        let wanted = match (want_bold, want_italic) {
            (true, true) => "Bold Italic",
            (true, false) => "Bold",
            (false, true) => "Italic",
            (false, false) => "Regular",
        };
        return Err(format!("{} has no {} style", s.font_family, wanted));
    };
    let name = name.to_string();
    style(app, json!({ "style": name }));
    Ok(json!({ "style": name }))
}

/// `text.setRangeStyle` with `p` on the range the Type tool has selected (the whole text when
/// nothing is selected). Does nothing unless the Type tool edits text.
pub(crate) fn range_style(app: &mut VectorcraftApp, p: Value) {
    let Some((id, a, b)) = text_editing(app) else { return };
    end_typing(app);
    let mut q = p;
    q["id"] = json!(id.0);
    if b > a {
        q["start"] = json!(a);
        q["end"] = json!(b);
    }
    app.run("text.setRangeStyle", q).ok();
}

// ---------- Type tool editing glue (selection, clipboard, keys) ----------

/// The text object the Type tool is editing and its selection (clamped to the text).
pub(crate) fn text_editing(app: &VectorcraftApp) -> Option<(NodeId, usize, usize)> {
    let o = app.session.tool_options();
    let id = NodeId(o.get("editing")?.as_u64()?);
    let len = match &app.session.active()?.doc.node(id)?.kind {
        NodeKind::Text(t) => t.plain_text().len(),
        _ => return None,
    };
    let g = |k: &str| o.get(k).and_then(Value::as_u64).map_or(0, |v| (v as usize).min(len));
    Some((id, g("start"), g("end")))
}

/// Close the Type tool's typing session (one undo step) before another command edits the text.
pub(crate) fn end_typing(app: &mut VectorcraftApp) {
    if app.session.tool_options().get("typing").and_then(Value::as_bool) == Some(true) {
        app.session.set_tool_option("commitTyping", &json!(true));
        app.session.commit_interaction().ok();
        app.sync_views();
    }
}

static CTX: OnceLock<egui::Context> = OnceLock::new();
/// Plain text of the last text copied inside the Type tool (paste from the native menu, which
/// can't read the system clipboard).
static CLIP: Mutex<String> = Mutex::new(String::new());

fn text_copy(app: &mut VectorcraftApp) -> bool {
    let Some((id, a, b)) = text_editing(app) else { return false };
    if a == b {
        return false;
    }
    let Ok(r) = app.session.execute("text.getRange", &json!({"id": id.0, "start": a, "end": b})) else { return false };
    app.session.set_tool_option("copy", &r["runs"]);
    let text = r["text"].as_str().unwrap_or("").to_string();
    if let Some(ctx) = CTX.get() {
        ctx.copy_text(text.clone());
    }
    *CLIP.lock().unwrap_or_else(|e| e.into_inner()) = text;
    true
}

fn text_key(app: &mut VectorcraftApp, k: ToolKey, m: Mods) {
    let v = app.view_info();
    if let Err(e) = app.session.tool_key(k, m, v) {
        app.status(e.to_string());
    }
}

fn text_paste(app: &mut VectorcraftApp, s: Option<String>) {
    let s = s.unwrap_or_else(|| CLIP.lock().unwrap_or_else(|e| e.into_inner()).clone());
    if !s.is_empty() {
        let v = app.view_info();
        if let Err(e) = app.session.tool_text(&s, v) {
            app.status(e.to_string());
        }
    }
}

/// Edit-menu commands while the Type tool edits text act on the text (Cut/Copy/Paste/Clear;
/// Select All is the engine's `select.all`). `None` = not intercepted.
pub(crate) fn intercept_text_command(app: &mut VectorcraftApp, id: &str) -> Option<Result<Value, String>> {
    if !app.session.tool_wants_text() {
        return None;
    }
    match id {
        "edit.copy" => {
            text_copy(app);
        }
        "edit.cut" => {
            if text_copy(app) {
                text_key(app, ToolKey::Delete, Mods::default());
            }
        }
        "edit.paste" | "edit.pasteWithoutFormatting" => text_paste(app, None),
        "edit.clear" => text_key(app, ToolKey::Delete, Mods::default()),
        _ => return None,
    }
    app.sync_views();
    Some(Ok(Value::Null))
}

enum TextInput {
    Key(ToolKey, Mods),
    Copy,
    Cut,
    Paste(Option<String>),
    Face(Face),
}

/// Route editing keys (with modifiers), clipboard events and Cmd+C/X/V to the Type tool (Cmd+A is
/// left to the Select All shortcut, which selects the text being edited).
pub(crate) fn route_type_input(app: &mut VectorcraftApp, ctx: &egui::Context) {
    CTX.get_or_init(|| ctx.clone());
    let mut todo = vec![];
    ctx.input_mut(|i| {
        i.events.retain(|e| match e {
            egui::Event::Copy => {
                todo.push(TextInput::Copy);
                false
            }
            egui::Event::Cut => {
                todo.push(TextInput::Cut);
                false
            }
            egui::Event::Paste(s) => {
                todo.push(TextInput::Paste(Some(s.clone())));
                false
            }
            egui::Event::Key { key, pressed, modifiers: m, .. } => {
                let mods = Mods { shift: m.shift, alt: m.alt, cmd: m.command, ctrl: m.ctrl && !m.command, space: false };
                let tk = match key {
                    Key::ArrowLeft => Some(ToolKey::Left),
                    Key::ArrowRight => Some(ToolKey::Right),
                    Key::ArrowUp => Some(ToolKey::Up),
                    Key::ArrowDown => Some(ToolKey::Down),
                    Key::Home => Some(ToolKey::Home),
                    Key::End => Some(ToolKey::End),
                    Key::Backspace => Some(ToolKey::Backspace),
                    Key::Delete => Some(ToolKey::Delete),
                    Key::Tab if !m.command => Some(ToolKey::Tab),
                    _ => None,
                };
                if let Some(tk) = tk {
                    if *pressed {
                        todo.push(TextInput::Key(tk, mods));
                    }
                    return false;
                }
                // Type › Bold and Italic, as page layout apps have them while text is edited.
                if m.command && m.shift && !m.alt && matches!(key, Key::B | Key::I) {
                    if *pressed {
                        todo.push(TextInput::Face(if *key == Key::B { Face::Bold } else { Face::Italic }));
                    }
                    return false;
                }
                if m.command && !m.shift && !m.alt && matches!(key, Key::C | Key::X | Key::V) {
                    if *pressed {
                        todo.push(match key {
                            Key::C => TextInput::Copy,
                            Key::X => TextInput::Cut,
                            _ => TextInput::Paste(None),
                        });
                    }
                    return false;
                }
                true
            }
            _ => true,
        })
    });
    for t in todo {
        match t {
            TextInput::Key(k, m) => text_key(app, k, m),
            TextInput::Copy => {
                text_copy(app);
            }
            TextInput::Cut => {
                if text_copy(app) {
                    text_key(app, ToolKey::Delete, Mods::default());
                }
            }
            TextInput::Paste(s) => text_paste(app, s),
            TextInput::Face(f) => {
                if let Err(e) = toggle_face(app, f) {
                    app.status(e);
                }
            }
        }
    }
}

/// A compact labelled cell: short glyph label + spinner field.
fn cell(ui: &mut Ui, label: &str, tip: &str, add: impl FnOnce(&mut Ui)) {
    let t = Tokens::get(ui.ctx());
    ui.horizontal(|ui| {
        let l =
            ui.add_sized(vec2(22.0, 24.0), egui::Label::new(egui::RichText::new(label).size(11.5).strong().color(t.text))).on_hover_text(tl!(tip));
        crate::scrub::note_label(ui, l.rect);
        add(ui);
    });
}

/// The Character panel's grid of numeric fields, `w` wide: Font Size, Leading, Kerning and
/// Tracking, then with `more` the scales, Baseline Shift and Character Rotation (`s`: the style
/// shown). The Properties panel shows its first rows.
pub(crate) fn metrics_grid(app: &mut VectorcraftApp, ui: &mut Ui, id: &str, s: &CharStyle, w: f32, more: bool) {
    let fw = ((w - 66.0) / 2.0).clamp(60.0, 100.0);
    let unit = app.session.type_unit();
    egui::Grid::new(id).num_columns(2).spacing([6.0, 4.0]).show(ui, |ui| {
        cell(ui, "T", tl!("Font Size"), |ui| size_field(app, ui, "ch-size", fw));
        cell(ui, "A↕", tl!("Leading"), |ui| {
            let leading = shared(app, CharStyle::effective_leading);
            let auto = shared(app, |s| s.leading.is_none()) == Some(true);
            match widgets::spin_field_auto(ui, "ch-lead", leading, Some(auto), unit, fw, 1.0, 0.1, &SIZE_PRESETS) {
                Some(SpinPick::Value(v)) => style(app, json!({"leading": v})),
                Some(SpinPick::Auto) => style(app, json!({"leading": "auto"})),
                None => {}
            }
        });
        ui.end_row();
        cell(ui, "VA", tl!("Kerning (0 = Auto)"), |ui| {
            let kerning = shared(app, |s| s.kerning.unwrap_or(0.0));
            if let Some(v) = widgets::spin_plain(ui, "ch-kern", kerning, "", 0, fw, 10.0, -1000.0, &TRACKING_PRESETS) {
                format(app, if v == 0.0 { json!({"kerning": "auto"}) } else { json!({"kerning": v}) });
            }
        });
        cell(ui, "VA↔", tl!("Tracking"), |ui| {
            if let Some(v) = widgets::spin_plain(ui, "ch-track", shared(app, |s| s.tracking), "", 0, fw, 10.0, -1000.0, &TRACKING_PRESETS) {
                style(app, json!({"tracking": v}));
            }
        });
        ui.end_row();
        if !more {
            return;
        }
        cell(ui, "IT", tl!("Vertical Scale %"), |ui| {
            if let Some(v) = widgets::spin_plain(ui, "ch-vs", s.v_scale, "%", 1, fw, 1.0, 1.0, &SCALE_PRESETS) {
                format(app, json!({"vScale": v}));
            }
        });
        cell(ui, "T↔", tl!("Horizontal Scale %"), |ui| {
            if let Some(v) = widgets::spin_plain(ui, "ch-hs", s.h_scale, "%", 1, fw, 1.0, 1.0, &SCALE_PRESETS) {
                format(app, json!({"hScale": v}));
            }
        });
        ui.end_row();
        cell(ui, "Aª", tl!("Baseline Shift"), |ui| {
            if let Some(v) = widgets::spin_field(ui, "ch-bs", Some(s.baseline_shift), unit, fw, 1.0, -1296.0, &[]) {
                format(app, json!({"baselineShift": v}));
            }
        });
        cell(ui, "⟲T", tl!("Character Rotation"), |ui| {
            if let Some(v) = widgets::spin_plain(ui, "ch-rot", s.rotation, "°", 1, fw, 15.0, -360.0, &super::transform::ANGLE_PRESETS) {
                format(app, json!({"rotation": v}));
            }
        });
        ui.end_row();
    });
}

/// The Font Size combo (Character and Properties panels, Control bar): typed, stepped, scrubbed or
/// picked from the presets, in the type unit, blank where the selected text's sizes differ.
pub(crate) fn size_field(app: &mut VectorcraftApp, ui: &mut Ui, id: &str, width: f32) {
    let size = shared(app, |s| s.size);
    if let Some(v) = widgets::spin_field(ui, id, size, app.session.type_unit(), width, 1.0, 0.1, &SIZE_PRESETS) {
        style(app, json!({"size": v}));
    }
}

/// The font family menu and the font style dropdown of style `s` (Character and Properties panels,
/// Control bar), with their `ids` and `widths`.
pub(crate) fn font_pickers(app: &mut VectorcraftApp, ui: &mut Ui, s: &CharStyle, ids: (&str, &str), widths: (f32, f32)) {
    let sample = crate::font_menu::sample_text(app);
    if let Some(pick) = crate::font_menu::font_menu(ui, ids.0, &s.font_family, widths.0, sample.as_deref(), crate::font_menu::MenuLook::of(app)) {
        crate::font_menu::apply(app, ui.ctx(), pick);
    }
    let styles = vectorcraft_text::FontDb::global().styles(&s.font_family);
    let names: Vec<&str> = styles.iter().map(String::as_str).collect();
    if let Some(name) = widgets::dropdown_names(ui, ids.1, &s.font_style, &names, widths.1).and_then(|i| names.get(i)) {
        style(app, json!({"style": name}));
    }
}

/// A variable font's axes (those it doesn't hide) for style `s`, `w` wide: a slider and a field
/// per axis, set through `text.setStyle`'s `fontVariations` when a drag ends or a value is typed;
/// Reset returns every axis to the font style's own setting. Nothing for a static font.
fn variable_axes(app: &mut VectorcraftApp, ui: &mut Ui, s: &CharStyle, w: f32) {
    let Some(face) = vectorcraft_text::FontDb::global().face_version(&s.font_family, &s.font_style, s.font_version.as_deref()) else { return };
    let axes: Vec<_> = face.axes().into_iter().filter(|a| !a.hidden && a.max > a.min).collect();
    if axes.is_empty() {
        return;
    }
    let t = Tokens::get(ui.ctx());
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(tl!("Variable Font")).size(11.5).strong().color(t.text_dim));
        if !s.font_variations.is_empty() {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button(tl!("Reset")).on_hover_text(tl!("Return the axes to the font style's own settings")).clicked() {
                    let reset: serde_json::Map<String, Value> = s.font_variations.keys().map(|k| (k.clone(), Value::Null)).collect();
                    style(app, json!({ "fontVariations": reset }));
                }
            });
        }
    });
    let (label_w, field_w) = (72.0, 52.0);
    let slider_w = (w - label_w - field_w - 16.0).max(40.0);
    for a in &axes {
        let value = s.font_variations.get(&a.tag).copied().unwrap_or_else(|| face.axis_value(&a.tag).unwrap_or(a.default));
        let decimals = if a.max - a.min >= 20.0 { 0 } else { 1 };
        ui.horizontal(|ui| {
            let l = ui.add_sized(vec2(label_w, 20.0), egui::Label::new(egui::RichText::new(&a.name).size(12.0).color(t.text)).truncate());
            l.on_hover_text(format!("{} ({}–{})", a.tag, a.min, a.max));
            // The drag's value stays local until it ends: one undo step per change.
            let id = ui.id().with(("ch-axis", &a.tag));
            let mut v = ui.data(|d| d.get_temp::<f64>(id)).unwrap_or(value);
            ui.visuals_mut().widgets.inactive.bg_fill = t.input_border;
            ui.visuals_mut().selection.bg_fill = t.accent;
            let r = ui.add_sized(vec2(slider_w, 20.0), egui::Slider::new(&mut v, a.min..=a.max).show_value(false).trailing_fill(true));
            if r.dragged() {
                ui.data_mut(|d| d.insert_temp(id, v));
            }
            let mut pick = (r.drag_stopped() || (r.changed() && !r.dragged())).then_some(v);
            if pick.is_some() {
                ui.data_mut(|d| d.remove::<f64>(id));
            }
            if let Some(x) = widgets::range_field(ui, ("ch-axis-field", &a.tag), value, a.min..=a.max, "", decimals, field_w) {
                pick = Some(x);
            }
            if let Some(v) = pick {
                let v = widgets::round_to(v, decimals as i32);
                style(app, json!({ "fontVariations": { a.tag.clone(): v } }));
            }
        });
    }
}

/// The Control bar's type controls while type is selected or edited: the Character link (the
/// Character panel in a popover), the font family and style menus and the Font Size combo.
pub(crate) fn control_bar(app: &mut VectorcraftApp, ui: &mut Ui) {
    let Some((s, _)) = text_style(app) else { return };
    let link = ui
        .link(egui::RichText::new(tl!("Character:")).size(12.0).color(Tokens::get(ui.ctx()).text).underline())
        .on_hover_text(tl!("Character options"));
    widgets::popover(&link, link.clicked(), |ui| {
        ui.set_width(260.0);
        show(app, ui);
    });
    font_pickers(app, ui, &s, ("cb-font", "cb-font-style"), (150.0, 96.0));
    size_field(app, ui, "cb-font-size", 96.0);
    ui.add_space(4.0);
    ui.separator();
}

pub fn show(app: &mut VectorcraftApp, ui: &mut Ui) {
    let Some((s, _)) = text_style(app) else {
        super::empty_state(ui, "type", tl!("No text selected"), tl!("Select a text object to edit its character attributes."));
        return;
    };
    let w = ui.available_width();
    font_pickers(app, ui, &s, ("ch-font", "ch-style"), (w - 4.0, w - 4.0));
    variable_axes(app, ui, &s, w);
    ui.add_space(2.0);
    let hidden = pstate::<bool>(ui.ctx(), "ch-hide-options");
    metrics_grid(app, ui, "ch-grid", &s, w, !hidden);
    if hidden {
        return;
    }
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        for (kind, tip, key, on, enabled) in [
            (Glyph::AllCaps, tl!("All Caps"), "allCaps", s.all_caps, true),
            (Glyph::SmallCaps, tl!("Small Caps (on the roadmap)"), "", false, false),
            (Glyph::Super, tl!("Superscript (on the roadmap)"), "", false, false),
            (Glyph::Sub, tl!("Subscript (on the roadmap)"), "", false, false),
            (Glyph::Underline, tl!("Underline"), "underline", s.underline, true),
            (Glyph::Strike, tl!("Strikethrough"), "strikethrough", s.strikethrough, true),
        ] {
            if style_toggle(ui, kind, tip, on, enabled) && !key.is_empty() {
                format(app, json!({key: !on}));
            }
        }
    });
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Glyph {
    AllCaps,
    SmallCaps,
    Super,
    Sub,
    Underline,
    Strike,
}

/// A drawn "T" style toggle (All Caps, Small Caps, Superscript, Subscript, Underline, Strikethrough).
fn style_toggle(ui: &mut Ui, g: Glyph, tip: &str, on: bool, enabled: bool) -> bool {
    let t = Tokens::get(ui.ctx());
    let (r, resp) = ui.allocate_exact_size(vec2(30.0, 26.0), if enabled { egui::Sense::click() } else { egui::Sense::hover() });
    if on {
        ui.painter().rect_filled(r, 3, t.tool_active);
    } else if resp.hovered() && enabled {
        ui.painter().rect_filled(r, 3, t.hover);
    }
    let col = if enabled { t.text_strong } else { t.text_disabled };
    let big = egui::FontId::proportional(15.0);
    let small = egui::FontId::proportional(10.5);
    let c = r.center();
    let p = ui.painter();
    match g {
        Glyph::AllCaps => {
            p.text(c - vec2(5.0, 0.0), egui::Align2::CENTER_CENTER, "T", big.clone(), col);
            p.text(c + vec2(5.0, 0.0), egui::Align2::CENTER_CENTER, "T", big, col);
        }
        Glyph::SmallCaps => {
            p.text(c - vec2(4.0, 0.0), egui::Align2::CENTER_CENTER, "T", big, col);
            p.text(c + vec2(6.0, 2.0), egui::Align2::CENTER_CENTER, "T", small, col);
        }
        Glyph::Super => {
            p.text(c - vec2(3.0, 0.0), egui::Align2::CENTER_CENTER, "T", big, col);
            p.text(c + vec2(6.0, -5.0), egui::Align2::CENTER_CENTER, "1", small, col);
        }
        Glyph::Sub => {
            p.text(c - vec2(3.0, 0.0), egui::Align2::CENTER_CENTER, "T", big, col);
            p.text(c + vec2(6.0, 5.0), egui::Align2::CENTER_CENTER, "1", small, col);
        }
        Glyph::Underline => {
            p.text(c - vec2(0.0, 1.0), egui::Align2::CENTER_CENTER, "T", big, col);
            p.line_segment([c + vec2(-5.0, 7.0), c + vec2(5.0, 7.0)], egui::Stroke::new(1.2, col));
        }
        Glyph::Strike => {
            p.text(c, egui::Align2::CENTER_CENTER, "T", big, col);
            p.line_segment([c + vec2(-6.0, 1.0), c + vec2(6.0, 1.0)], egui::Stroke::new(1.2, col));
        }
    }
    let resp = resp.on_hover_text(tl!(tip));
    enabled && resp.clicked()
}

pub fn menu(app: &mut VectorcraftApp, ui: &mut Ui) {
    let st = text_style(app);
    let has = st.is_some();
    let hidden: bool = pstate(ui.ctx(), "ch-hide-options");
    if menu_item(ui, if hidden { tl!("Show Options") } else { tl!("Hide Options") }, true, false) {
        set_pstate(ui.ctx(), "ch-hide-options", !hidden);
    }
    ui.separator();
    let s = st.map(|x| x.0);
    for (label, key, on) in [
        (tl!("All Caps"), "allCaps", s.as_ref().is_some_and(|s| s.all_caps)),
        (tl!("Underline"), "underline", s.as_ref().is_some_and(|s| s.underline)),
        (tl!("Strikethrough"), "strikethrough", s.as_ref().is_some_and(|s| s.strikethrough)),
    ] {
        if menu_item(ui, label, has, on) {
            format(app, json!({key: !on}));
        }
    }
    for l in ["Small Caps", "Superscript", "Subscript"] {
        menu_item(ui, tl!(l), false, false);
    }
    ui.separator();
    // Character Alignment: where characters smaller than the largest on their line line up, with
    // the East Asian options.
    if app.session.prefs.show_east_asian_options {
        let align = s.as_ref().map(|s| s.char_align);
        ui.add_enabled_ui(has, |ui| {
            // Indented like the items beside it (their check column).
            ui.menu_button(format!("   {}", tl!("Character Alignment")), |ui| {
                use vectorcraft_doc::CharAlign;
                for (label, a, key) in [
                    (tl!("Roman Baseline"), CharAlign::RomanBaseline, "romanBaseline"),
                    (tl!("Em Box Top/Right"), CharAlign::EmBoxTop, "emBoxTop"),
                    (tl!("Em Box Center"), CharAlign::EmBoxCenter, "emBoxCenter"),
                    (tl!("Em Box Bottom/Left"), CharAlign::EmBoxBottom, "emBoxBottom"),
                    (tl!("ICF Top/Right"), CharAlign::IcfTop, "icfTop"),
                    (tl!("ICF Bottom/Left"), CharAlign::IcfBottom, "icfBottom"),
                ] {
                    if menu_item(ui, label, true, align == Some(a)) {
                        format(app, json!({"charAlign": key}));
                    }
                }
            });
        });
        ui.separator();
    }
    for l in ["Standard Vertical Roman Alignment", "Tate-chu-yoko", "Fractional Widths", "System Layout", "No Break"] {
        menu_item(ui, tl!(l), false, l == "Fractional Widths");
    }
    ui.separator();
    // The installed fonts are always listed; this picks up fonts installed since the app started.
    #[cfg(not(target_arch = "wasm32"))]
    if menu_item(ui, tl!("Refresh Font List"), true, false)
        && let Ok(r) = app.run("text.rescanFonts", json!({}))
    {
        app.status(format!("{} font families available", r["families"]));
    }
    if menu_item(ui, tl!("Reset Panel"), has, false) {
        style(app, json!({"tracking": 0, "leading": "auto"}));
        format(
            app,
            json!({"kerning": "auto", "baselineShift": 0, "hScale": 100, "vScale": 100, "rotation": 0, "underline": false, "strikethrough": false, "allCaps": false}),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vectorcraft_engine::Session;

    /// Bold and Italic pick the family's own faces by name, and toggle back (#724).
    #[test]
    fn bold_and_italic_pick_the_familys_own_faces() {
        let styles: Vec<String> =
            ["Light", "Narrow", "Regular", "Italic", "Narrow Italic", "SemiBold", "Bold", "Bold Condensed", "Narrow Bold", "Bold Italic", "Black"]
                .iter()
                .map(|s| s.to_string())
                .collect();
        assert_eq!(pick_face(&styles, true, false, "Regular"), Some("Bold"), "not Bold Condensed, Narrow Bold or SemiBold");
        assert_eq!(pick_face(&styles, true, true, "Italic"), Some("Bold Italic"));
        assert_eq!(pick_face(&styles, false, true, "Regular"), Some("Italic"));
        assert_eq!(pick_face(&styles, false, false, "Italic"), Some("Regular"), "upright again, not Narrow");
        assert_eq!(pick_face(&styles, true, false, "Narrow"), Some("Narrow Bold"), "a Narrow face stays Narrow");
        assert_eq!(pick_face(&styles, false, true, "Narrow"), Some("Narrow Italic"));
        let oblique: Vec<String> = ["Book", "Oblique", "Bold-Oblique"].iter().map(|s| s.to_string()).collect();
        assert_eq!(pick_face(&oblique, true, true, "Oblique"), Some("Bold-Oblique"));
        assert_eq!(pick_face(&oblique, false, false, "Oblique"), None, "Book has a weight word: no regular face");
        assert_eq!(face_of("Bold Italic"), (true, false, true, vec![]));
        assert_eq!(face_of("SemiBold"), (false, false, false, vec![]));
        assert_eq!(face_of("Narrow Italic"), (false, true, true, vec!["narrow".to_string()]));
        // No type selected: a message, nothing changed.
        let mut app = VectorcraftApp::new(Session::new(), Default::default());
        app.run("file.new", json!({"width": 300, "height": 200})).unwrap();
        assert!(toggle_face(&mut app, Face::Bold).is_err());
        assert!(!crate::menus::enabled(&app, "type.bold"));
    }

    /// Character Alignment is in the panel menu with the East Asian options only (as Mojikumi Set
    /// and Top-to-Top Leading are in the Paragraph panel).
    #[test]
    fn character_alignment_shows_with_the_east_asian_options() {
        let mut app = VectorcraftApp::new(Session::new(), Default::default());
        app.run("file.new", json!({"width": 300, "height": 200})).unwrap();
        let id = app.session.execute("text.create", &json!({"x": 20, "y": 50, "text": "雅楽"})).unwrap()["id"].clone();
        app.session.execute("select.set", &json!({"ids": [id]})).unwrap();
        let shown = |app: &mut VectorcraftApp| crate::tests_labels::painted_text(app, menu).contains("Character Alignment");
        assert!(!shown(&mut app));
        app.session.prefs.show_east_asian_options = true;
        assert!(shown(&mut app));
    }

    /// A variable font shows its axes with the values the type is set at; a static one doesn't.
    #[test]
    fn variable_fonts_show_their_axes() {
        use vectorcraft_text::test_fonts::{VARIABLE_FAMILY, variable_font};
        vectorcraft_text::FontDb::global().add_font(variable_font().unwrap());
        let mut app = VectorcraftApp::new(Session::new(), Default::default());
        app.run("file.new", json!({"width": 300, "height": 200})).unwrap();
        let id = app.session.execute("text.create", &json!({"x": 20, "y": 50, "text": "lIn"})).unwrap()["id"].clone();
        app.session.execute("select.set", &json!({"ids": [id]})).unwrap();
        let painted = |app: &mut VectorcraftApp| crate::tests_labels::painted_text(app, show);
        assert!(!painted(&mut app).contains("Variable Font"), "a static font");
        app.session.execute("text.setStyle", &json!({"font": VARIABLE_FAMILY, "style": "SemiBold"})).unwrap();
        let text = painted(&mut app);
        assert!(text.contains("Variable Font") && text.contains("Weight") && text.contains("550"), "{text}");
        assert!(!text.contains("Reset"), "nothing to reset yet");
        app.session.execute("text.setStyle", &json!({"fontVariations": {"wght": 612}})).unwrap();
        let text = painted(&mut app);
        assert!(text.contains("612") && text.contains("Reset"), "{text}");
    }
}
