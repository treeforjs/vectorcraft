//! The insert bar: a slim strip over the canvas, under the document tabs, and the obvious place to
//! start making something. Shapes (a gallery: rectangles, ellipse, triangle, star, arrows, lines,
//! speech bubbles), Text, Icons (a gallery), Image, the quick style and colour theme, and Copy as
//! PNG / Export PNG for slides and chats. One click drops the object at the centre of the page in
//! view, at a size in proportion to the page and in the document's quick style, selected; a shape
//! or icon can also be dragged from its gallery onto the canvas. Every insert is one undoable
//! engine command (`insert.shape`, `insert.textBox`, `insert.icon`), so agents and the ⌘K palette
//! reach them too.
//!
//! Cheap per frame: the gallery outlines are flattened once ([`previews`]), the icons are the
//! rasterised SVGs egui caches, and the galleries are only laid out while open.

use std::sync::OnceLock;

use egui::{Color32, CornerRadius, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use serde_json::{Value, json};
use vectorcraft_engine::cmd::clipboard::{Flavour, PNG};
use vectorcraft_engine::cmd::quick::{self, ICONS, SHAPES};

use crate::VectorcraftApp;
use crate::theme::Tokens;

/// The bar's height in points.
pub const HEIGHT: f32 = 38.0;

/// Where the bar's items were drawn last frame, by automation id (`ui.inspect` › `insertBar`).
type Items = Vec<(&'static str, Rect)>;

fn items_id() -> egui::Id {
    egui::Id::new("insert-bar-items")
}

/// A shape's name in the UI language.
pub fn shape_label(id: &str) -> &'static str {
    match id {
        "rectangle" => tl!("Rectangle"),
        "roundedRectangle" => tl!("Rounded Rectangle"),
        "ellipse" => tl!("Ellipse"),
        "triangle" => tl!("Triangle"),
        "diamond" => tl!("Diamond"),
        "star" => tl!("Star"),
        "arrowRight" => tl!("Arrow"),
        "arrowDouble" => tl!("Double Arrow"),
        "chevron" => tl!("Chevron"),
        "line" => tl!("Line"),
        "arrowLine" => tl!("Line Arrow"),
        "speechBubble" => tl!("Speech Bubble"),
        "callout" => tl!("Callout"),
        _ => "",
    }
}

/// An icon's name in the UI language.
pub fn icon_label(id: &str) -> &'static str {
    match id {
        "check" => tl!("Check"),
        "heart" => tl!("Heart"),
        "user" => tl!("Person"),
        "star" => tl!("Star"),
        "sparkles" => tl!("Sparkles"),
        "sun" => tl!("Sun"),
        "cloud" => tl!("Cloud"),
        "globe" => tl!("Globe"),
        "house" => tl!("House"),
        "info" => tl!("Info"),
        "lock" => tl!("Lock"),
        "map" => tl!("Map"),
        "message-circle" => tl!("Message"),
        "monitor" => tl!("Monitor"),
        "search" => tl!("Search"),
        "settings" => tl!("Settings"),
        "chart-column" => tl!("Chart"),
        "pin" => tl!("Pin"),
        "share-2" => tl!("Share"),
        "image" => tl!("Picture"),
        _ => "",
    }
}

/// Each shape's outline in a unit box (polylines, closed or not), flattened once.
/// A shape's flattened outline: its subpaths' points (0..1 in the tile) and whether each is closed.
type Outline = Vec<(Vec<[f32; 2]>, bool)>;

fn previews() -> &'static [(&'static str, Outline)] {
    static P: OnceLock<Vec<(&'static str, Outline)>> = OnceLock::new();
    P.get_or_init(|| {
        SHAPES
            .iter()
            .map(|s| {
                let (w, h) = (s.w.max(0.01), s.h);
                let k = 1.0 / w.max(h);
                let r = vectorcraft_geom::Rect::new(0.5 - w * k / 2.0, 0.5 - h * k / 2.0, 0.5 + w * k / 2.0, 0.5 + h * k / 2.0);
                let mut lines: Vec<(Vec<[f32; 2]>, bool)> = vec![];
                if let Ok((path, _)) = quick::shape_path(s.id, r) {
                    vectorcraft_geom::kurbo::flatten(path.to_bezpath(), 0.004, |el| match el {
                        vectorcraft_geom::PathEl::MoveTo(p) => lines.push((vec![[p.x as f32, p.y as f32]], false)),
                        vectorcraft_geom::PathEl::LineTo(p) => {
                            if let Some(l) = lines.last_mut() {
                                l.0.push([p.x as f32, p.y as f32]);
                            }
                        }
                        vectorcraft_geom::PathEl::ClosePath => {
                            if let Some(l) = lines.last_mut() {
                                l.1 = true;
                            }
                        }
                        _ => {}
                    });
                }
                (s.id, lines)
            })
            .collect()
    })
}

/// Paint shape `id`'s outline into `r`.
fn paint_shape(ui: &Ui, id: &str, r: Rect, color: Color32) {
    let Some((_, lines)) = previews().iter().find(|p| p.0 == id) else { return };
    let at = |p: &[f32; 2]| pos2(r.left() + p[0] * r.width(), r.top() + p[1] * r.height());
    for (pts, closed) in lines {
        let pts: Vec<Pos2> = pts.iter().map(at).collect();
        let stroke = Stroke::new(1.5, color);
        let shape = if *closed { egui::epaint::PathShape::closed_line(pts, stroke) } else { egui::epaint::PathShape::line(pts, stroke) };
        ui.painter().add(shape);
    }
    if id == "arrowLine" {
        // Its arrowhead (a stroke property, not in the outline).
        let (y, x1) = (r.center().y, r.right() - 0.0);
        let s = r.width() * 0.12;
        ui.painter().add(egui::Shape::convex_polygon(vec![pos2(x1, y), pos2(x1 - s, y - s * 0.6), pos2(x1 - s, y + s * 0.6)], color, Stroke::NONE));
    }
}

/// The icons' SVGs drawn white (tinted at draw time), made once.
fn icon_bytes(id: &str) -> Option<&'static [u8]> {
    static B: OnceLock<Vec<(&'static str, &'static [u8])>> = OnceLock::new();
    let all = B.get_or_init(|| {
        ICONS
            .iter()
            .map(|(id, _, svg)| {
                let s = svg.replace("currentColor", "white").replace("stroke-width=\"2\"", "stroke-width=\"1.6\"");
                let leaked: &'static [u8] = Box::leak(s.into_bytes().into_boxed_slice());
                (*id, leaked)
            })
            .collect()
    });
    all.iter().find(|b| b.0 == id).map(|b| b.1)
}

fn paint_icon(ui: &Ui, id: &str, r: Rect, tint: Color32) {
    let Some(bytes) = icon_bytes(id) else { return };
    let src = egui::ImageSource::Bytes { uri: format!("bytes://quick-icon-{id}.svg").into(), bytes: egui::load::Bytes::Static(bytes) };
    egui::Image::new(src).tint(tint).fit_to_exact_size(r.size()).paint_at(ui, r);
}

/// Where an insert goes: the page in view (the artboard under the view's centre, else the
/// active one) and its centre in view (`center`, `artboard` params of `insert.*`).
pub fn target(app: &VectorcraftApp) -> Value {
    let (Some(st), Some(v)) = (app.session.active(), app.view()) else { return json!({}) };
    let d = &st.doc;
    if v.fitted
        && let Some(i) = d.artboard_at(v.center)
    {
        return json!({ "center": [v.center.x, v.center.y], "artboard": i });
    }
    match d.artboards.get(v.artboard).or(d.artboards.first()) {
        Some(a) => json!({ "center": [a.rect.center().x, a.rect.center().y] }),
        None => json!({}),
    }
}

/// Run insert command `cmd` with `params` plus where it goes (`at`: a canvas point it was
/// dropped on, else the page's centre in view).
pub fn insert(app: &mut VectorcraftApp, cmd: &str, mut params: Value, at: Option<Pos2>) {
    let place = match (at, app.canvas_rect, app.view()) {
        (Some(p), Some(rect), Some(v)) if rect.contains(p) => {
            let q = crate::canvas::Xf::new(rect, v).to_doc(p);
            json!({ "center": [q.x, q.y] })
        }
        _ => target(app),
    };
    if let (Some(o), Some(t)) = (params.as_object_mut(), place.as_object()) {
        for (k, v) in t {
            o.entry(k.clone()).or_insert_with(|| v.clone());
        }
    }
    // A selection tool, so the new object can be moved and resized at once.
    if !vectorcraft_tools::catalog::is_selection_tool(app.session.tool_id()) {
        app.select_tool("selection");
    }
    crate::menus::invoke(app, cmd, params);
}

/// A bar button: icon, label and an optional ▾ → its response.
fn button(ui: &mut Ui, icon: &str, label: &str, menu: bool) -> egui::Response {
    let tint = Tokens::get(ui.ctx()).icon;
    button_with(ui, label, menu, |ui, r| crate::icons::paint(ui, icon, r, tint))
}

/// [`button`] with its own picture (`paint` into the icon's square).
fn button_with(ui: &mut Ui, label: &str, menu: bool, paint: impl FnOnce(&Ui, Rect)) -> egui::Response {
    let t = Tokens::get(ui.ctx());
    let galley = ui.painter().layout_no_wrap(label.to_string(), egui::FontId::proportional(13.0), t.text_strong);
    let w = galley.size().x + 34.0 + if menu { 12.0 } else { 0.0 };
    let (r, resp) = ui.allocate_exact_size(vec2(w, 28.0), Sense::click_and_drag());
    if resp.hovered() || resp.is_pointer_button_down_on() {
        ui.painter().rect_filled(r, CornerRadius::same(4), t.hover);
    }
    paint(ui, Rect::from_min_size(r.min + vec2(8.0, 6.0), vec2(16.0, 16.0)));
    ui.painter().galley(pos2(r.left() + 29.0, r.center().y - galley.size().y / 2.0), galley, t.text_strong);
    if menu {
        crate::icons::paint(ui, "chevron-down", Rect::from_min_size(pos2(r.right() - 15.0, r.center().y - 5.0), vec2(10.0, 10.0)), t.text_dim);
    }
    resp
}

fn separator(ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let (r, _) = ui.allocate_exact_size(vec2(9.0, 22.0), Sense::hover());
    ui.painter().line_segment([pos2(r.center().x, r.top()), pos2(r.center().x, r.bottom())], Stroke::new(1.0, t.divider));
}

/// A gallery tile → its response (click inserts; a drag onto the canvas inserts there).
fn tile(ui: &mut Ui, tip: &str, paint: impl FnOnce(&Ui, Rect, Color32)) -> egui::Response {
    let t = Tokens::get(ui.ctx());
    let (r, resp) = ui.allocate_exact_size(vec2(44.0, 40.0), Sense::click_and_drag());
    ui.painter().rect_filled(r, CornerRadius::same(4), if resp.hovered() { t.hover } else { t.input });
    ui.painter().rect_stroke(r, CornerRadius::same(4), Stroke::new(1.0, t.button_border), StrokeKind::Inside);
    paint(ui, r.shrink(9.0), t.icon);
    if resp.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    }
    resp.on_hover_text(tip)
}

/// A gallery item clicked (no drop point) or dropped at a point.
enum Pick {
    Shape(&'static str, Option<Pos2>),
    Icon(&'static str, Option<Pos2>),
}

/// Whether `resp` is a finished click or a drag released outside the gallery → (picked, dropped at).
fn picked(resp: &egui::Response) -> Option<Option<Pos2>> {
    if resp.clicked() {
        return Some(None);
    }
    if resp.drag_stopped() {
        let at = resp.ctx.input(|i| i.pointer.interact_pos());
        return at.filter(|p| !resp.rect.contains(*p)).map(Some);
    }
    None
}

fn shapes_gallery(ui: &mut Ui) -> Option<Pick> {
    let mut pick = None;
    ui.label(egui::RichText::new(tl!("Click to add, or drag onto the page")).size(11.5).color(Tokens::get(ui.ctx()).text_dim));
    for row in SHAPES.chunks(5) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            for s in row {
                let resp = ui.push_id(s.id, |ui| tile(ui, shape_label(s.id), |ui, r, c| paint_shape(ui, s.id, r, c))).inner;
                if let Some(at) = picked(&resp) {
                    pick = Some(Pick::Shape(s.id, at));
                }
            }
        });
    }
    pick
}

fn icons_gallery(ui: &mut Ui) -> Option<Pick> {
    let mut pick = None;
    ui.label(egui::RichText::new(tl!("Click to add, or drag onto the page")).size(11.5).color(Tokens::get(ui.ctx()).text_dim));
    for row in ICONS.chunks(5) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            for (id, _, _) in row {
                let resp = ui.push_id(*id, |ui| tile(ui, icon_label(id), |ui, r, c| paint_icon(ui, id, r, c))).inner;
                if let Some(at) = picked(&resp) {
                    pick = Some(Pick::Icon(id, at));
                }
            }
        });
    }
    pick
}

/// Is the bar shown: a document is open, Home isn't, and the layout keeps it.
pub fn visible(app: &VectorcraftApp) -> bool {
    app.ui.insert_bar && app.ui.home.is_none() && app.session.active().is_some()
}

/// Lay the bar out across the top of the canvas area.
pub fn show(app: &mut VectorcraftApp, ui: &mut Ui) {
    if !visible(app) {
        return;
    }
    let t = Tokens::get(ui.ctx());
    let (strip, _) = ui.allocate_exact_size(vec2(ui.available_width(), HEIGHT), Sense::hover());
    ui.painter().rect_filled(strip, 0.0, t.panel);
    ui.painter().line_segment([strip.left_bottom(), strip.right_bottom()], Stroke::new(1.0, t.border));
    let mut items: Items = Vec::with_capacity(10);
    let mut pick: Option<Pick> = None;
    let mut run: Option<(&str, Value)> = None;
    let inner = strip.shrink2(vec2(8.0, 0.0));
    let mut bar = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(egui::Layout::left_to_right(egui::Align::Center)));
    bar.spacing_mut().item_spacing.x = 2.0;
    bar.set_clip_rect(inner);
    let ui = &mut bar;

    let shapes = button(ui, "shapes", tl!("Shapes"), true).on_hover_text(tl!("Add a shape: rectangles, circles, arrows, lines, speech bubbles"));
    items.push(("insert.shapes", shapes.rect));
    egui::Popup::menu(&shapes).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
        if let Some(p) = shapes_gallery(ui) {
            pick = Some(p);
            ui.close();
        }
    });
    let text = button(ui, "type", tl!("Text"), false).on_hover_text(tl!("Add a text box in the middle of the page"));
    items.push(("insert.text", text.rect));
    if text.clicked() {
        run = Some(("insert.textBox", json!({ "text": tl!("Your text here") })));
    }
    let icons = button(ui, "sparkles", tl!("Icons"), true).on_hover_text(tl!("Add an icon"));
    items.push(("insert.icons", icons.rect));
    egui::Popup::menu(&icons).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
        if let Some(p) = icons_gallery(ui) {
            pick = Some(p);
            ui.close();
        }
    });
    let image = button(ui, "image", tl!("Image"), false).on_hover_text(tl!("Place a picture from a file"));
    items.push(("insert.image", image.rect));
    if image.clicked() {
        run = Some(("file.place", json!({})));
    }
    separator(ui);

    // The look new inserts get (and the selection, when it is picked).
    let (theme, now) = crate::quick_styles::current(app);
    let style = button_with(ui, crate::quick_styles::style_label(now), true, |ui, r| {
        ui.painter().rect_filled(r.expand(2.0), CornerRadius::same(3), Color32::WHITE);
        crate::quick_styles::paint_preview(ui, r.expand(4.0), now, theme);
    })
    .on_hover_text(tl!("Quick styles: one-click looks in the document's colors"));
    items.push(("insert.style", style.rect));
    egui::Popup::menu(&style).show(|ui| {
        if let Some(s) = crate::quick_styles::style_grid(app, ui, 4, vec2(52.0, 38.0)) {
            run = Some(("quick.style.pick", json!({ "style": s })));
        }
    });
    let th = crate::quick_styles::theme_button(ui, theme, 128.0);
    items.push(("insert.theme", th.rect));
    egui::Popup::menu(&th).show(|ui| {
        if let Some(id) = crate::quick_styles::theme_list(app, ui) {
            run = Some(("quick.theme", json!({ "theme": id })));
        }
    });

    // Getting the graphic out: right-aligned.
    let rest = ui.available_rect_before_wrap();
    let mut right = ui.new_child(egui::UiBuilder::new().max_rect(rest).layout(egui::Layout::right_to_left(egui::Align::Center)));
    right.spacing_mut().item_spacing.x = 2.0;
    let export = button(&mut right, "save", tl!("Export PNG"), false).on_hover_text(tl!("Save the selection, or the page, as a PNG file"));
    items.push(("insert.exportPng", export.rect));
    if export.clicked() {
        run = Some(("quick.exportPng", json!({})));
    }
    let copy = button(&mut right, "copy", tl!("Copy as PNG"), false)
        .on_hover_text(tl!("Copy the selection, or the page, as a picture to paste into slides and chats"));
    items.push(("insert.copyPng", copy.rect));
    if copy.clicked() {
        run = Some(("quick.copyPng", json!({})));
    }

    ui.ctx().data_mut(|d| d.insert_temp(items_id(), std::sync::Arc::new(items)));
    match pick {
        Some(Pick::Shape(id, at)) => insert(app, "insert.shape", json!({ "shape": id }), at),
        Some(Pick::Icon(id, at)) => insert(app, "insert.icon", json!({ "icon": id }), at),
        None => {}
    }
    match run {
        Some(("insert.textBox", p)) => insert(app, "insert.textBox", p, None),
        Some(("quick.style.pick", p)) => {
            if let Some(s) = p["style"].as_str() {
                crate::quick_styles::apply_style(app, s);
            }
        }
        Some((cmd, p)) => crate::menus::invoke(app, cmd, p),
        None => {}
    }
}

/// The bar's items by automation id with their screen rects (`ui.inspect` › `insertBar`): null
/// while the bar isn't shown.
pub fn inspect(app: &VectorcraftApp, ctx: &egui::Context) -> Value {
    if !visible(app) {
        return Value::Null;
    }
    let items: Option<std::sync::Arc<Items>> = ctx.data(|d| d.get_temp(items_id()));
    let items: Vec<Value> =
        items.iter().flat_map(|v| v.iter()).map(|(id, r)| json!({ "id": id, "rect": [r.left(), r.top(), r.width(), r.height()] })).collect();
    json!({ "items": items })
}

// ---------- UI commands ----------

/// The PNG of the selection, else the page in view: (bytes, width, height).
fn picture(app: &VectorcraftApp, p: &Value) -> Result<(Vec<u8>, u32, u32), String> {
    let fallback = app.view().map_or(0, |v| v.artboard);
    let artboard = p.get("artboard").and_then(Value::as_u64).map(|a| a as usize);
    let scale = p.get("scale").and_then(Value::as_f64).unwrap_or(2.0);
    app.session.quick_png(artboard, fallback, scale, p.get("background").and_then(Value::as_str)).map_err(|e| e.to_string())
}

fn copy_png(app: &mut VectorcraftApp, p: &Value) -> Result<Value, String> {
    let (png, w, h) = picture(app, p)?;
    // The copied objects stay on the app's own clipboard (pasting back here gets them, not the
    // picture); nothing selected, the picture does.
    let selection = app.session.active().is_some_and(|st| !st.selection.is_empty()) && p.get("artboard").is_none();
    let b64 = vectorcraft_format::base64_encode(&png);
    if selection {
        app.session.execute("edit.copy", &json!({})).map_err(|e| e.to_string())?;
    } else {
        app.session.execute("clipboard.importImage", &json!({ "dataBase64": b64, "mime": PNG })).map_err(|e| e.to_string())?;
    }
    match app.services.system_clipboard.as_mut() {
        Some(cb) => cb.write(&[Flavour { mime: PNG, data: png }]).map_err(|e| format!("Couldn't copy to the system clipboard: {e}"))?,
        None => return Err(tl!("Copying pictures needs the desktop app: use Export PNG").to_string()),
    }
    app.ui.status = tl!("Copied a {w} × {h} px PNG").replace("{w}", &w.to_string()).replace("{h}", &h.to_string());
    Ok(json!({ "width": w, "height": h }))
}

fn export_png(app: &mut VectorcraftApp, p: &Value) -> Result<Value, String> {
    let (png, w, h) = picture(app, p)?;
    let path = crate::io::target_path(app, p.get("path").and_then(Value::as_str).map(str::to_string), "png")?;
    if let Some(dl) = app.services.download.as_mut() {
        dl(&path, &png);
    } else {
        let write = app.services.write.as_mut().ok_or("no file writer")?;
        write(&path, &png)?;
    }
    app.ui.status = tl!("Exported {path}").replace("{path}", &path);
    Ok(json!({ "path": path, "width": w, "height": h }))
}

/// After a double-click switched a selection tool to the Type tool: when it picked the type
/// inside a shape, edit it (caret at its end) → whether it did.
pub fn edit_shape_text(app: &mut VectorcraftApp) -> bool {
    let Some(st) = app.session.active() else { return false };
    let [id] = st.selection.objects.as_slice() else { return false };
    if !vectorcraft_tools::select::shape_text_parts(&st.doc, *id).is_some_and(|(_, t)| t == *id) {
        return false;
    }
    let view = app.view_info();
    if let Err(e) = app.session.tool_notify("text.editNew", view) {
        app.status(e.to_string());
    }
    true
}

/// Put type inside the selected shape (or edit the type already there) and start typing.
fn type_in_shape(app: &mut VectorcraftApp, p: &Value) -> Result<Value, String> {
    let r = app.run("insert.textInShape", p.clone())?;
    app.select_tool("type");
    edit_shape_text(app);
    Ok(r)
}

/// The bar's UI commands (`window.insertBar`, `quick.typeInShape`, `quick.copyPng`,
/// `quick.exportPng`).
pub fn run_command(app: &mut VectorcraftApp, id: &str, p: &Value) -> Option<Result<Value, String>> {
    Some(match id {
        "window.insertBar" => {
            app.ui.insert_bar = p.get("show").and_then(Value::as_bool).unwrap_or(!app.ui.insert_bar);
            Ok(json!(app.ui.insert_bar))
        }
        "quick.typeInShape" => type_in_shape(app, p),
        "quick.copyPng" => copy_png(app, p),
        "quick.exportPng" => export_png(app, p),
        _ => return None,
    })
}
