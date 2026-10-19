//! First steps for a beginner: the Home screen's examples, Help › Learn the Basics (a short guided
//! tour whose steps advance when the user does them), the empty-canvas hint and the Undo toast after
//! destructive commands. Everything here is cheap per frame: the tour looks at the document only
//! when its revision or the selection changes, and the example thumbnails are decoded once.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use vectorcraft_doc::{NodeId, NodeKind};

use crate::VectorcraftApp;
use crate::theme::{self, Tokens};

/// One of the examples bundled with the app (`examples/*.vectorcraft`, saved with a preview).
pub struct Example {
    /// Its id for `help.openExample` (the file stem).
    pub name: &'static str,
    /// The card's caption (the artwork's title, not translated).
    pub title: &'static str,
    pub bytes: &'static [u8],
}

pub const EXAMPLES: &[Example] = &[
    Example { name: "dusk-poster", title: "Dusk Poster", bytes: include_bytes!("../../../examples/dusk-poster.vectorcraft") },
    Example { name: "neon-drive", title: "Neon Drive", bytes: include_bytes!("../../../examples/neon-drive.vectorcraft") },
    Example { name: "ribbons", title: "Ribbons", bytes: include_bytes!("../../../examples/ribbons.vectorcraft") },
    Example { name: "perspective-city", title: "Perspective City", bytes: include_bytes!("../../../examples/perspective-city.vectorcraft") },
    Example { name: "feature-sheet", title: "Feature Sheet", bytes: include_bytes!("../../../examples/feature-sheet.vectorcraft") },
];

/// The tour's saved state (`ui.tour`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Tour {
    /// The step shown (`STEPS.len()`: the closing card), or none while the tour is closed.
    pub step: Option<usize>,
    /// The tour was finished once (Home marks it done).
    pub completed: bool,
}

/// What the app keeps for this module between frames (not saved).
#[derive(Default)]
pub struct Runtime {
    /// The Undo toasts showing.
    pub toasts: craft_ui::Toasts,
    /// The example thumbnails, decoded from each file's preview the first time Home shows.
    thumbs: Option<Vec<Option<egui::TextureHandle>>>,
    /// The state when the current step began, and the one it is compared with now.
    start: Option<Obs>,
    base: Option<Obs>,
    /// What [`Obs::key`] was when the tour last looked.
    key: Option<ObsKey>,
}

/// Where a step's card sits: by the toolbar (the canvas's left), by the panels (its right), or at
/// the top centre.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Place {
    Left,
    Right,
    Top,
}

/// One step of the tour.
struct Step {
    /// The title and text, translated; `{key}` in the text is replaced with the shortcut of `key`
    /// (a command id), when it has one.
    title: fn() -> &'static str,
    body: fn() -> &'static str,
    key: Option<&'static str>,
    place: Place,
    /// Done: from the state the step started in (`start`), the latest state of the same selected
    /// object (`base`) and now.
    done: fn(start: &Obs, base: &Obs, now: &Obs) -> bool,
}

const STEPS: &[Step] = &[
    Step {
        title: || tl!("Draw a rectangle"),
        body: || tl!("Choose the Rectangle tool in the toolbar (or press M), then drag on the page."),
        key: None,
        place: Place::Left,
        done: |start, _, now| now.doc == start.doc && now.revision != start.revision && now.rect && now.sel != start.sel,
    },
    Step {
        title: || tl!("Change its fill color"),
        body: || tl!("With the shape selected, pick a new Fill color in the Properties panel on the right."),
        key: None,
        place: Place::Right,
        done: |_, base, now| now.sel.is_some() && now.sel == base.sel && now.revision != base.revision && now.fill != base.fill,
    },
    Step {
        title: || tl!("Move and resize it"),
        body: || tl!("With the Selection tool (V), drag the shape to move it, or drag a corner handle to resize it."),
        key: None,
        place: Place::Top,
        done: |_, base, now| now.sel.is_some() && now.sel == base.sel && now.revision != base.revision && now.bounds != base.bounds,
    },
    Step {
        title: || tl!("Add some text"),
        body: || tl!("Choose the Type tool (or press T), click on the page and type a few words. Press Esc when you are done."),
        key: None,
        place: Place::Left,
        done: |start, _, now| now.doc == start.doc && now.revision != start.revision && now.text,
    },
    Step {
        title: || tl!("Undo a step"),
        body: || tl!("Press {key} (or Edit › Undo) to take back the last change. Everything can be undone, so feel free to try things."),
        key: Some("edit.undo"),
        place: Place::Top,
        done: |start, _, now| now.doc == start.doc && now.redo > start.redo,
    },
    Step {
        title: || tl!("Find anything"),
        body: || tl!("Press {key} and type what you want to do, such as “align” or “export”, to find any command."),
        key: Some("help.commandPalette"),
        place: Place::Top,
        done: |_, _, now| now.palette,
    },
];

/// When the tour looks again: the active document, its revision, the first selected object and
/// whether the command search is open.
type ObsKey = (Option<u64>, u64, Option<NodeId>, bool);

/// What the tour watches, read from the active document.
#[derive(Clone, Debug, Default, PartialEq)]
struct Obs {
    doc: Option<u64>,
    revision: u64,
    redo: usize,
    sel: Option<NodeId>,
    /// The first selected object is a rectangle / a text object.
    rect: bool,
    text: bool,
    /// Its fill and bounds (as text and numbers: compared, never shown).
    fill: String,
    bounds: Option<[f64; 4]>,
    palette: bool,
}

impl Obs {
    fn key(app: &VectorcraftApp) -> ObsKey {
        let st = app.session.active();
        (st.map(|d| d.uid), st.map_or(0, |d| d.revision), st.and_then(|d| d.selection.objects.first().copied()), app.ui.palette_open)
    }

    fn of(app: &VectorcraftApp) -> Obs {
        let mut o = Obs { palette: app.ui.palette_open, ..Obs::default() };
        let Some(st) = app.session.active() else { return o };
        o.doc = Some(st.uid);
        o.revision = st.revision;
        o.redo = st.history.redo.len();
        o.sel = st.selection.objects.first().copied();
        if let Some(n) = o.sel.and_then(|id| st.doc.node(id)) {
            o.rect = matches!(&n.kind, NodeKind::Path { live: Some(vectorcraft_doc::LiveShape::Rectangle { .. }), .. });
            o.text = matches!(&n.kind, NodeKind::Text(_));
            o.fill = format!("{:?}", n.appearance.fill_paint());
            o.bounds = n.geometric_bounds().map(|r| [r.x0, r.y0, r.x1, r.y1]);
        }
        o
    }
}

/// The UI commands of this module (`help.tour`, `help.openExample`), else `None`.
pub fn run_command(app: &mut VectorcraftApp, id: &str, p: &Value) -> Option<Result<Value, String>> {
    match id {
        "help.tour" => Some(tour_command(app, p.get("action").and_then(Value::as_str).unwrap_or("start"))),
        "help.openExample" => Some(open_example(app, p.get("name").and_then(Value::as_str).unwrap_or(""))),
        _ => None,
    }
}

fn tour_command(app: &mut VectorcraftApp, action: &str) -> Result<Value, String> {
    let step = app.ui.tour.step;
    match action {
        "start" => {
            // The steps draw on a page: a new one when none is open.
            if app.session.active().is_none() {
                app.run("file.new", json!({}))?;
            }
            app.ui.home = None;
            go_to(app, Some(0));
        }
        "back" => go_to(app, Some(step.unwrap_or(0).saturating_sub(1))),
        "skip" | "next" => go_to(app, step.map(|s| (s + 1).min(STEPS.len()))),
        "end" => {
            if step == Some(STEPS.len()) {
                app.ui.tour.completed = true;
            }
            go_to(app, None);
        }
        other => return Err(format!("action must be start, back, skip or end (not `{other}`)")),
    }
    Ok(serde_json::to_value(&app.ui.tour).unwrap_or_default())
}

/// Show step `step` (or close the tour); it starts from the state now.
fn go_to(app: &mut VectorcraftApp, step: Option<usize>) {
    app.ui.tour.step = step.map(|s| s.min(STEPS.len()));
    let now = Obs::of(app);
    app.onboarding.start = Some(now.clone());
    app.onboarding.base = Some(now);
    app.onboarding.key = Some(Obs::key(app));
}

/// Open bundled example `name` as a new untitled document (Save asks where: the bundled file is
/// never written).
fn open_example(app: &mut VectorcraftApp, name: &str) -> Result<Value, String> {
    let Some(ex) = EXAMPLES.iter().find(|e| e.name == name) else {
        let names: Vec<&str> = EXAMPLES.iter().map(|e| e.name).collect();
        return Err(format!("no example `{name}` (examples: {})", names.join(", ")));
    };
    let r = crate::io::open_bytes(app, &format!("{}.{}", ex.name, vectorcraft_format::EXTENSION), ex.bytes, None)?;
    app.ui.home = None;
    Ok(r)
}

/// Per frame, while the tour is open: advance it when the step is done. Looks at the document only
/// when its revision, the selection or the command search changed.
pub fn watch(app: &mut VectorcraftApp) {
    let Some(i) = app.ui.tour.step else { return };
    let key = Obs::key(app);
    if app.onboarding.key == Some(key) {
        return;
    }
    app.onboarding.key = Some(key);
    let now = Obs::of(app);
    let (Some(step), Some(start)) = (STEPS.get(i), app.onboarding.start.clone()) else { return };
    let base = app.onboarding.base.clone().unwrap_or_else(|| now.clone());
    if (step.done)(&start, &base, &now) {
        go_to(app, Some(i + 1));
        return;
    }
    // Another object selected (or another document): compare with it from now on.
    if base.sel != now.sel || base.doc != now.doc {
        app.onboarding.base = Some(now);
    }
}

/// The tour's card for the current step, near the part of the window it is about.
pub fn tour_card(app: &mut VectorcraftApp, ctx: &egui::Context) {
    const W: f32 = 300.0;
    let Some(i) = app.ui.tour.step else { return };
    if crate::menus::home_showing(app) || app.ui.screen_mode >= 3 {
        return;
    }
    let t = Tokens::get(ctx);
    let area = app.canvas_rect.unwrap_or_else(|| ctx.content_rect());
    let step = STEPS.get(i);
    let pos = match step.map_or(Place::Top, |s| s.place) {
        Place::Left => area.left_top() + egui::vec2(16.0, 16.0),
        Place::Right => egui::pos2(area.right() - W - 16.0, area.top() + 16.0),
        Place::Top => egui::pos2(area.center().x - W / 2.0, area.top() + 16.0),
    };
    let mut action = None;
    egui::Area::new(egui::Id::new("tour-card")).order(egui::Order::Foreground).fixed_pos(pos).show(ctx, |ui| {
        egui::Frame::NONE
            .fill(t.panel)
            .stroke(egui::Stroke::new(1.0, t.accent))
            .corner_radius(egui::CornerRadius::same(craft_ui::tokens::radius::LG))
            .inner_margin(egui::Margin::same(14))
            .shadow(egui::Shadow { offset: [0, 4], blur: 16, spread: 0, color: egui::Color32::from_black_alpha(60) })
            .show(ui, |ui| {
                ui.set_width(W - 28.0);
                ui.horizontal(|ui| {
                    let progress = match step {
                        Some(_) => crate::i18n::fmt(tl!("Step {n} of {total}"), &[("n", &(i + 1).to_string()), ("total", &STEPS.len().to_string())]),
                        None => tl!("Learn the Basics").to_string(),
                    };
                    ui.label(egui::RichText::new(progress).size(11.5).color(t.text_dim));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let close = ui.add(egui::Button::new(egui::RichText::new("×").size(15.0).color(t.text_dim)).frame(false));
                        if close.on_hover_text(tl!("End the tour")).clicked() {
                            action = Some("end");
                        }
                    });
                });
                ui.add_space(2.0);
                match step {
                    Some(s) => {
                        ui.label(egui::RichText::new((s.title)()).font(theme::semibold(15.0)).color(t.text_strong));
                        ui.add_space(4.0);
                        ui.label(egui::RichText::new(step_body(s)).size(12.5).color(t.text));
                        ui.add_space(10.0);
                        ui.horizontal(|ui| {
                            if i > 0 && crate::widgets::secondary_button(ui, tl!("Back")).clicked() {
                                action = Some("back");
                            }
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if crate::widgets::secondary_button(ui, tl!("Skip")).on_hover_text(tl!("Go to the next step")).clicked() {
                                    action = Some("skip");
                                }
                            });
                        });
                    }
                    None => {
                        ui.label(egui::RichText::new(tl!("You know the basics")).font(theme::semibold(15.0)).color(t.text_strong));
                        ui.add_space(4.0);
                        let body = tl!("Keep drawing, or open an example from Home to see what VectorCraft can do. Help › Learn the Basics starts this tour again.");
                        ui.label(egui::RichText::new(body).size(12.5).color(t.text));
                        ui.add_space(10.0);
                        ui.horizontal(|ui| {
                            if crate::widgets::secondary_button(ui, tl!("Back")).clicked() {
                                action = Some("back");
                            }
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if crate::widgets::primary_button(ui, tl!("Done")).clicked() {
                                    action = Some("end");
                                }
                            });
                        });
                    }
                }
            });
    });
    if let Some(a) = action {
        let _ = tour_command(app, a);
    }
}

/// A step's text, its `{key}` the platform's name for the shortcut.
fn step_body(s: &Step) -> String {
    let key = s.key.and_then(crate::menus::shortcut_of).map(crate::menus::pretty_shortcut).unwrap_or_default();
    crate::i18n::fmt((s.body)(), &[("key", &key)])
}

/// The tour's step titles, in the interface language.
pub fn step_titles() -> Vec<&'static str> {
    STEPS.iter().map(|s| (s.title)()).collect()
}

/// Home's "Learn the Basics" entry and "Open an Example" cards.
pub fn home_sections(app: &mut VectorcraftApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    ui.add_space(28.0);
    ui.label(egui::RichText::new(tl!("Open an Example")).font(theme::semibold(14.0)).color(t.text));
    ui.add_space(10.0);
    let thumbs = thumbs(app, ui.ctx());
    let mut open = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(14.0, 14.0);
        if learn_card(ui, app.ui.tour.completed).clicked() {
            open = Some(None);
        }
        for (ex, thumb) in EXAMPLES.iter().zip(thumbs) {
            if example_card(ui, ex, thumb.as_ref()).clicked() {
                open = Some(Some(ex.name));
            }
        }
    });
    match open {
        Some(Some(name)) => {
            if let Err(e) = app.run("help.openExample", json!({ "name": name })) {
                app.status(e);
            }
        }
        Some(None) => {
            if let Err(e) = app.run("help.tour", json!({})) {
                app.status(e);
            }
        }
        None => {}
    }
}

const CARD: egui::Vec2 = egui::vec2(146.0, 150.0);

/// The "Learn the Basics" card: starts the tour (with a check once it was finished).
fn learn_card(ui: &mut egui::Ui, completed: bool) -> egui::Response {
    let t = Tokens::get(ui.ctx());
    let (r, resp) = ui.allocate_exact_size(CARD, egui::Sense::click());
    let p = ui.painter();
    p.rect_filled(r, egui::CornerRadius::same(6), if resp.hovered() { t.hover } else { t.panel });
    p.rect_stroke(r, egui::CornerRadius::same(6), egui::Stroke::new(1.5, t.accent), egui::StrokeKind::Inside);
    let icon = egui::Rect::from_center_size(r.center_top() + egui::vec2(0.0, 46.0), egui::vec2(36.0, 36.0));
    crate::icons::paint(ui, "sparkles", icon, t.accent);
    let title = ui.painter().layout(tl!("Learn the Basics").to_string(), theme::semibold(12.5), t.text_strong, r.width() - 14.0);
    let below = r.top() + 88.0;
    ui.painter().galley(egui::pos2(r.center().x - title.size().x / 2.0, below), title, t.text_strong);
    let sub = if completed { tl!("Done · take it again") } else { tl!("A two-minute tour") };
    ui.painter().text(egui::pos2(r.center().x, below + 22.0), egui::Align2::CENTER_TOP, sub, egui::FontId::proportional(11.0), t.text_dim);
    resp.on_hover_text(tl!("Six short steps: draw, color, move, type, undo and search"))
}

/// An example's card: its thumbnail and title.
fn example_card(ui: &mut egui::Ui, ex: &Example, thumb: Option<&egui::TextureHandle>) -> egui::Response {
    let t = Tokens::get(ui.ctx());
    let (r, resp) = ui.allocate_exact_size(CARD, egui::Sense::click());
    let p = ui.painter();
    p.rect_filled(r, egui::CornerRadius::same(6), if resp.hovered() { t.hover } else { t.panel_darker });
    let frame = egui::Rect::from_min_size(r.min + egui::vec2(8.0, 8.0), egui::vec2(r.width() - 16.0, 100.0));
    match thumb {
        Some(tex) => {
            let size = tex.size_vec2();
            let k = (frame.width() / size.x.max(1.0)).min(frame.height() / size.y.max(1.0));
            let img = egui::Rect::from_center_size(frame.center(), size * k);
            p.rect_filled(img.translate(egui::vec2(2.0, 2.0)), 0.0, egui::Color32::from_black_alpha(80));
            p.image(tex.id(), img, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
        }
        None => {
            p.rect_filled(frame.shrink(10.0), 0.0, egui::Color32::WHITE);
        }
    }
    p.text(egui::pos2(r.center().x, r.top() + 118.0), egui::Align2::CENTER_TOP, ex.title, theme::semibold(11.5), t.text_strong);
    resp.on_hover_text(tl!("Open a copy of this example"))
}

/// The example thumbnails: each file's embedded preview, decoded and uploaded once.
fn thumbs(app: &mut VectorcraftApp, ctx: &egui::Context) -> Vec<Option<egui::TextureHandle>> {
    app.onboarding
        .thumbs
        .get_or_insert_with(|| {
            EXAMPLES
                .iter()
                .map(|ex| {
                    let png = vectorcraft_format::preview(ex.bytes)?;
                    let img = image::load_from_memory_with_format(&png, image::ImageFormat::Png).ok()?.to_rgba8();
                    let size = [img.width() as usize, img.height() as usize];
                    let color = egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw());
                    Some(ctx.load_texture(format!("example-{}", ex.name), color, egui::TextureOptions::LINEAR))
                })
                .collect()
        })
        .clone()
}

/// Whether the document has no art (only empty layers).
pub fn is_empty(doc: &vectorcraft_doc::Document) -> bool {
    doc.layers.iter().all(|l| l.children().is_none_or(Vec::is_empty))
}

/// The empty canvas's hint, centred on the canvas: what to do first. How to draw is the hint
/// bar's first step (`chrome::first_step`), so this points there and adds the other starts. Painted only (the canvas
/// keeps every click), and only while the document has no art and the tour isn't showing.
pub fn empty_canvas_hint(app: &VectorcraftApp, ui: &egui::Ui, rect: egui::Rect) -> bool {
    let Some(st) = app.session.active() else { return false };
    if !is_empty(&st.doc) || app.ui.tour.step.is_some() || app.session.tool_busy() || st.doc.mask_edit.is_some() {
        return false;
    }
    let painter = ui.painter_at(rect);
    let t = Tokens::get(ui.ctx());
    let paste = crate::menus::shortcut_of("edit.paste").map(crate::menus::pretty_shortcut).unwrap_or_default();
    let title = tl!("Your page is empty");
    let body = crate::i18n::fmt(
        tl!("The bar below says how to draw your first shape. You can also place an image with File › Place or paste with {key}."),
        &[("key", &paste)],
    );
    let width = 300.0_f32.min(rect.width() - 32.0).max(80.0);
    // Proportional only: the canvas also draws in contexts without the app's named fonts.
    let title_g = painter.layout(title.to_string(), egui::FontId::proportional(14.5), t.text, width);
    let body_g = painter.layout(body, egui::FontId::proportional(12.0), t.text_dim, width);
    let icon = 28.0;
    let h = icon + 8.0 + title_g.size().y + 4.0 + body_g.size().y;
    let box_r = egui::Rect::from_center_size(rect.center(), egui::vec2(width + 32.0, h + 28.0));
    painter.rect_filled(box_r, egui::CornerRadius::same(craft_ui::tokens::radius::LG), t.panel.gamma_multiply(0.92));
    painter.rect_stroke(box_r, egui::CornerRadius::same(craft_ui::tokens::radius::LG), egui::Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
    let mut y = box_r.top() + 14.0;
    let icon_r = egui::Rect::from_center_size(egui::pos2(rect.center().x, y + icon / 2.0), egui::vec2(icon, icon));
    crate::icons::paint(ui, "shapes", icon_r, t.text_disabled);
    y += icon + 8.0;
    painter.galley(egui::pos2(rect.center().x - title_g.size().x / 2.0, y), title_g.clone(), t.text);
    y += title_g.size().y + 4.0;
    painter.galley(egui::pos2(rect.center().x - body_g.size().x / 2.0, y), body_g, t.text_dim);
    true
}

/// What an Undo toast needs from before a destructive command: how many objects it acts on and
/// the document's revision. `None` for other commands (most of them: a match on the id).
pub struct Before {
    one: &'static str,
    other: &'static str,
    count: usize,
    doc: u64,
    revision: u64,
}

/// The destructive commands that show an Undo toast, and its message (one, other).
fn toast_message(id: &str) -> Option<(&'static str, &'static str)> {
    Some(match id {
        "edit.clear" => ("Deleted {n} object", "Deleted {n} objects"),
        "edit.cut" => ("Cut {n} object", "Cut {n} objects"),
        "object.ungroup" => ("Ungrouped {n} object", "Ungrouped {n} objects"),
        "object.compoundPath.release" => ("Released {n} compound path", "Released {n} compound paths"),
        "object.expand" | "object.expandShape" | "object.expandAppearance" | "object.expandBrush" | "object.path.outlineStroke" => {
            ("Expanded {n} object", "Expanded {n} objects")
        }
        "object.rasterize" | "object.flattenTransparency" => ("Flattened {n} object", "Flattened {n} objects"),
        _ if id.starts_with("object.pathfinder.") => ("Combined {n} object", "Combined {n} objects"),
        _ => return None,
    })
}

impl Before {
    pub fn of(app: &VectorcraftApp, id: &str) -> Option<Before> {
        let (one, other) = toast_message(id)?;
        let st = app.session.active()?;
        Some(Before { one, other, count: st.selection.objects.len().max(1), doc: st.uid, revision: st.revision })
    }
}

/// After a command [`Before::of`] noted ran: an Undo toast, when it changed the document.
pub fn after_command(app: &mut VectorcraftApp, before: Before) {
    let Some(st) = app.session.active() else { return };
    if st.uid != before.doc || st.revision == before.revision {
        return;
    }
    let text = crate::i18n::trn(crate::i18n::current(), before.count as u64, before.one, before.other);
    app.onboarding.toasts.push(craft_ui::Toast::info(text).with_action(tl!("Undo"), "edit.undo"));
}

/// Draw the toasts above the status and hint bars; an Undo click runs `edit.undo`.
pub fn show_toasts(app: &mut VectorcraftApp, ctx: &egui::Context) {
    if app.onboarding.toasts.items().is_empty() {
        return;
    }
    let inset = if app.ui.status_bar && app.ui.screen_mode < 3 { 56.0 } else { 8.0 };
    if let Some(cmd) = app.onboarding.toasts.show(ctx, inset) {
        crate::menus::invoke(app, &cmd, json!({}));
    }
}

/// The toasts showing, for `ui.inspect`: [{message, action}].
pub fn toasts_json(app: &VectorcraftApp) -> Value {
    Value::Array(
        app.onboarding.toasts.items().iter().map(|t| json!({"message": t.message, "action": t.action.as_ref().map(|a| a.1.clone())})).collect(),
    )
}
