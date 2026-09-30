//! Panels. Each panel reads engine state and acts only through `app.run(...)`.

pub mod layers;
pub mod properties;

use drawcraft_color::{BlendMode, Color, Paint};
use drawcraft_doc::{AppearanceItem, Node};
use egui::{CornerRadius, Sense, Stroke, StrokeKind, Ui, vec2};
use serde_json::json;

use crate::theme::Tokens;
use crate::widgets::{self, dim_label, paint_chip};
use crate::{DrawcraftApp, icons};

/// The first selected node (cloned), if any.
pub fn first_selected(app: &DrawcraftApp) -> Option<Node> {
    let st = app.session.active()?;
    st.selection.objects.first().and_then(|id| st.doc.node(*id)).cloned()
}

pub fn show_icon_panel(app: &mut DrawcraftApp, ui: &mut Ui, id: &str) {
    match id {
        "swatches" => swatches(app, ui),
        "color" => color(app, ui),
        "colorGuide" => color_guide(app, ui),
        "stroke" => stroke(app, ui),
        "transparency" => transparency(app, ui),
        "appearance" => appearance(app, ui),
        "graphicStyles" => graphic_styles(app, ui),
        "align" => align(app, ui),
        "pathfinder" => pathfinder(app, ui),
        "transform" => properties::transform_section(app, ui),
        "history" => history(app, ui),
        "info" => info(app, ui),
        "artboards" => artboards(app, ui),
        "gradient" => gradient(app, ui),
        "character" | "paragraph" => properties::type_sections(app, ui),
        "navigator" => navigator(app, ui),
        _ => {
            dim_label(ui, "This panel is on the roadmap (see the parity plan).");
        }
    }
}

pub fn libraries(_app: &mut DrawcraftApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    ui.add_space(20.0);
    ui.vertical_centered(|ui| {
        icons::icon(ui, "library", 40.0, t.text_dim);
        ui.add_space(8.0);
        ui.label(egui::RichText::new("Local Libraries").size(14.0).color(t.text));
        dim_label(
            ui,
            "Drag art, colors and text styles here to reuse them across documents. Libraries are stored on this machine — no account required.",
        );
    });
}

fn paint_target(app: &DrawcraftApp) -> &'static str {
    if app.session.fill_active { "paint.setFill" } else { "paint.setStroke" }
}

pub fn swatches(app: &mut DrawcraftApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let Some(st) = app.session.active() else {
        dim_label(ui, "Open a document to see its swatches.");
        return;
    };
    let mut all: Vec<(String, Paint)> = st.doc.swatches.iter().map(|s| (s.name.clone(), s.paint.clone())).collect();
    for g in &st.doc.swatch_groups {
        for s in &g.swatches {
            all.push((s.name.clone(), s.paint.clone()));
        }
    }
    let target = paint_target(app);
    let mut chosen = None;
    ui.horizontal(|ui| {
        let (f, s) = (app.session.paint.fill.clone(), app.session.paint.stroke.clone());
        let (a, b, _, _) = widgets::fill_stroke_proxy(ui, &f, &s, app.session.fill_active, 34.0);
        if a {
            app.session.fill_active = true;
        }
        if b {
            app.session.fill_active = false;
        }
    });
    ui.add_space(6.0);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(2.0, 2.0);
        for (name, paint) in &all {
            let (r, resp) = ui.allocate_exact_size(vec2(19.0, 19.0), Sense::click());
            paint_chip(ui, r, paint);
            ui.painter().rect_stroke(r, 0.0, Stroke::new(1.0, if resp.hovered() { t.text } else { t.border }), StrokeKind::Inside);
            if resp.on_hover_text(name).clicked() {
                chosen = Some(name.clone());
            }
        }
    });
    if let Some(name) = chosen {
        app.run(target, json!({"swatch": name})).ok();
    }
    widgets::divider(ui);
    ui.horizontal(|ui| {
        if widgets::icon_button(ui, "plus", "New Swatch", false, 22.0).clicked() {
            app.run("swatch.new", json!({})).ok();
        }
    });
}

pub fn color(app: &mut DrawcraftApp, ui: &mut Ui) {
    let current = match first_selected(app) {
        Some(n) => {
            if app.session.fill_active {
                n.appearance.fill_paint()
            } else {
                n.appearance.stroke_paint()
            }
        }
        None => {
            if app.session.fill_active {
                app.session.paint.fill.clone()
            } else {
                app.session.paint.stroke.clone()
            }
        }
    };
    let c = current.color().unwrap_or(Color::BLACK);
    let mut rgb = c.to_rgba8(1.0);
    let mut changed = false;
    for (i, lbl) in ["R", "G", "B"].iter().enumerate() {
        ui.horizontal(|ui| {
            ui.label(*lbl);
            let mut v = rgb[i] as i32;
            if ui.add(egui::Slider::new(&mut v, 0..=255)).changed() {
                rgb[i] = v as u8;
                changed = true;
            }
        });
    }
    let mut hex = c.to_hex();
    ui.horizontal(|ui| {
        ui.label("#");
        if ui.add(egui::TextEdit::singleline(&mut hex).desired_width(80.0)).lost_focus()
            && let Some(nc) = Color::from_hex(&hex)
        {
            rgb = nc.to_rgba8(1.0);
            changed = true;
        }
    });
    // Spectrum ramp.
    let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 40.0), Sense::click_and_drag());
    let n = 64;
    for i in 0..n {
        for j in 0..4 {
            let h = i as f32 / n as f32 * 360.0;
            let v = 1.0 - j as f32 / 4.0 * 0.8;
            let [cr, cg, cb, _] = Color::from_hsb(h, 1.0, v).to_rgba8(1.0);
            let cell =
                egui::Rect::from_min_size(r.min + vec2(i as f32 * r.width() / n as f32, j as f32 * 10.0), vec2(r.width() / n as f32 + 0.5, 10.0));
            ui.painter().rect_filled(cell, 0.0, egui::Color32::from_rgb(cr, cg, cb));
        }
    }
    if (resp.clicked() || resp.dragged())
        && let Some(p) = resp.interact_pointer_pos()
    {
        let h = ((p.x - r.left()) / r.width()).clamp(0.0, 1.0) * 360.0;
        let v = 1.0 - ((p.y - r.top()) / r.height()).clamp(0.0, 1.0) * 0.8;
        rgb = Color::from_hsb(h, 1.0, v).to_rgba8(1.0);
        changed = true;
    }
    if changed {
        let hex = format!("#{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2]);
        app.run(paint_target(app), json!({"color": hex})).ok();
    }
}

fn color_guide(app: &mut DrawcraftApp, ui: &mut Ui) {
    let base =
        first_selected(app).and_then(|n| n.appearance.fill_paint().color()).or(app.session.paint.fill.color()).unwrap_or(Color::rgb(0.9, 0.3, 0.1));
    for h in drawcraft_color::harmony::Harmony::ALL {
        ui.horizontal(|ui| {
            ui.add_sized(vec2(110.0, 18.0), egui::Label::new(egui::RichText::new(h.label()).size(11.0)));
            for c in h.apply(base) {
                let (r, resp) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::click());
                paint_chip(ui, r, &Paint::solid(c));
                if resp.clicked() {
                    app.run(paint_target(app), json!({"color": c.to_hex()})).ok();
                }
            }
        });
    }
}

pub fn stroke(app: &mut DrawcraftApp, ui: &mut Ui) {
    let n = first_selected(app);
    let st = n.as_ref().and_then(|n| n.appearance.stroke().cloned());
    let weight = st.as_ref().map(|s| s.width).unwrap_or(app.session.paint.stroke_width);
    ui.horizontal(|ui| {
        dim_label(ui, "Weight:");
        if let Some(w) = widgets::num_field(ui, "stroke-w", Some(weight), drawcraft_doc::Unit::Points, 80.0) {
            app.run("stroke.set", json!({"weight": w})).ok();
        }
    });
    let cap = st.as_ref().map(|s| s.cap).unwrap_or_default();
    let join = st.as_ref().map(|s| s.join).unwrap_or_default();
    let align = st.as_ref().map(|s| s.align).unwrap_or_default();
    ui.horizontal(|ui| {
        dim_label(ui, "Cap:");
        for (v, lbl, name) in [
            (drawcraft_doc::LineCap::Butt, "Butt", "butt"),
            (drawcraft_doc::LineCap::Round, "Round", "round"),
            (drawcraft_doc::LineCap::Square, "Projecting", "square"),
        ] {
            if ui.selectable_label(cap == v, lbl).clicked() {
                app.run("stroke.set", json!({"cap": name})).ok();
            }
        }
    });
    ui.horizontal(|ui| {
        dim_label(ui, "Corner:");
        for (v, lbl, name) in [
            (drawcraft_doc::LineJoin::Miter, "Miter", "miter"),
            (drawcraft_doc::LineJoin::Round, "Round", "round"),
            (drawcraft_doc::LineJoin::Bevel, "Bevel", "bevel"),
        ] {
            if ui.selectable_label(join == v, lbl).clicked() {
                app.run("stroke.set", json!({"join": name})).ok();
            }
        }
    });
    ui.horizontal(|ui| {
        dim_label(ui, "Align:");
        for (v, lbl, name) in [
            (drawcraft_doc::StrokeAlign::Center, "Center", "center"),
            (drawcraft_doc::StrokeAlign::Inside, "Inside", "inside"),
            (drawcraft_doc::StrokeAlign::Outside, "Outside", "outside"),
        ] {
            if ui.selectable_label(align == v, lbl).clicked() {
                app.run("stroke.set", json!({"align": name})).ok();
            }
        }
    });
    let mut dashed = st.as_ref().is_some_and(|s| s.dash.is_some());
    if ui.checkbox(&mut dashed, "Dashed Line").changed() {
        app.run("stroke.set", if dashed { json!({"dash": [12, 6]}) } else { json!({"dash": null}) }).ok();
    }
    ui.horizontal(|ui| {
        dim_label(ui, "Arrowheads:");
        let end = st.as_ref().and_then(|s| s.end_arrow).map(|a| format!("{a:?}")).unwrap_or("None".into());
        let opts = ["None", "Arrow", "Triangle", "Circle", "Square", "Diamond", "Bar"];
        if let Some(i) = widgets::dropdown(ui, "arrow-end", &end, &opts, 110.0) {
            let v = if i == 0 { json!(null) } else { json!(opts[i]) };
            app.run("stroke.set", json!({"endArrow": v})).ok();
        }
    });
    ui.horizontal(|ui| {
        dim_label(ui, "Profile:");
        let opts = ["Uniform", "Width Profile 1 (lens)", "Taper Start", "Taper End"];
        let ids = ["uniform", "lens", "taperStart", "taperEnd"];
        if let Some(i) = widgets::dropdown(ui, "profile", "Uniform", &opts, 150.0) {
            app.run("stroke.set", json!({"profile": ids[i]})).ok();
        }
    });
}

fn blend_dropdown(app: &mut DrawcraftApp, ui: &mut Ui, current: BlendMode) {
    let labels: Vec<&str> = BlendMode::ALL.iter().map(|b| b.label()).collect();
    if let Some(i) = widgets::dropdown(ui, "blend", current.label(), &labels, 130.0) {
        app.run("object.setProps", json!({"blend": labels[i]})).ok();
    }
}

pub fn transparency(app: &mut DrawcraftApp, ui: &mut Ui) {
    let n = first_selected(app);
    let (blend, op) = n.as_ref().map(|n| (n.blend, n.opacity)).unwrap_or((BlendMode::Normal, 1.0));
    ui.horizontal(|ui| {
        blend_dropdown(app, ui, blend);
        dim_label(ui, "Opacity:");
        if let Some(o) = widgets::plain_field(ui, "tr-op", op as f64 * 100.0, "%", 0, 60.0) {
            app.run("object.setProps", json!({"opacity": o.clamp(0.0, 100.0) / 100.0})).ok();
        }
    });
    let mut o = op * 100.0;
    if ui.add(egui::Slider::new(&mut o, 0.0..=100.0).show_value(false)).drag_stopped() {
        app.run("object.setProps", json!({"opacity": o / 100.0})).ok();
    }
}

pub fn appearance(app: &mut DrawcraftApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let Some(n) = first_selected(app) else {
        dim_label(ui, "No Selection");
        return;
    };
    ui.label(egui::RichText::new(n.kind_label()).strong());
    let items: Vec<(usize, AppearanceItem)> = n.appearance.items.iter().cloned().enumerate().rev().collect();
    for (i, it) in items {
        let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::hover());
        ui.painter().line_segment([r.left_bottom(), r.right_bottom()], Stroke::new(1.0, t.divider));
        let (label, paint, extra) = match &it {
            AppearanceItem::Fill(f) => ("Fill:", f.paint.clone(), String::new()),
            AppearanceItem::Stroke(s) => ("Stroke:", s.paint.clone(), format!("{} pt", s.width)),
        };
        ui.painter().text(r.left_center() + vec2(22.0, 0.0), egui::Align2::LEFT_CENTER, label, egui::FontId::proportional(12.0), t.text);
        let chip = egui::Rect::from_min_size(r.left_center() + vec2(74.0, -8.0), vec2(16.0, 16.0));
        paint_chip(ui, chip, &paint);
        ui.painter().text(r.left_center() + vec2(98.0, 0.0), egui::Align2::LEFT_CENTER, extra, egui::FontId::proportional(12.0), t.text_dim);
        let del = egui::Rect::from_center_size(r.right_center() - vec2(12.0, 0.0), vec2(14.0, 14.0));
        let dr = ui.interact(del, ui.id().with(("ap-del", i)), Sense::click());
        icons::paint(ui, "trash-2", del, if dr.hovered() { t.text } else { t.text_disabled });
        if dr.clicked() {
            app.run("appearance.removeItem", json!({"index": i})).ok();
        }
        icons::paint(ui, "eye", egui::Rect::from_center_size(r.left_center() + vec2(9.0, 0.0), vec2(13.0, 13.0)), t.icon);
    }
    ui.horizontal(|ui| {
        dim_label(ui, "Opacity:");
        dim_label(ui, &format!("{:.0}%  {}", n.opacity * 100.0, n.blend.label()));
    });
    widgets::divider(ui);
    ui.horizontal(|ui| {
        if widgets::flat_button(ui, "+ Stroke", 70.0).clicked() {
            app.run("appearance.addStroke", json!({})).ok();
        }
        if widgets::flat_button(ui, "+ Fill", 60.0).clicked() {
            app.run("appearance.addFill", json!({})).ok();
        }
        if widgets::flat_button(ui, "Clear", 50.0).clicked() {
            app.run("appearance.clear", json!({})).ok();
        }
    });
}

fn graphic_styles(app: &mut DrawcraftApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let styles: Vec<(String, drawcraft_doc::Appearance)> =
        app.session.active().map(|d| d.doc.graphic_styles.iter().map(|g| (g.name.clone(), g.appearance.clone())).collect()).unwrap_or_default();
    ui.horizontal_wrapped(|ui| {
        for (name, ap) in styles {
            let (r, resp) = ui.allocate_exact_size(vec2(44.0, 44.0), Sense::click());
            ui.painter().rect_filled(r, 0.0, egui::Color32::WHITE);
            let inner = r.shrink(8.0);
            paint_chip(ui, inner, &ap.fill_paint());
            if let Some(s) = ap.stroke() {
                let col = s.paint.color().map(|c| {
                    let [a, b, cc, _] = c.to_rgba8(1.0);
                    egui::Color32::from_rgb(a, b, cc)
                });
                if let Some(col) = col {
                    ui.painter().rect_stroke(inner, 0.0, Stroke::new((s.width as f32).clamp(1.0, 4.0), col), StrokeKind::Middle);
                }
            }
            ui.painter().rect_stroke(r, 0.0, Stroke::new(1.0, if resp.hovered() { t.text } else { t.border }), StrokeKind::Inside);
            if resp.on_hover_text(&name).clicked() {
                app.run("graphicStyle.apply", json!({"name": name})).ok();
            }
        }
    });
}

pub fn align(app: &mut DrawcraftApp, ui: &mut Ui) {
    dim_label(ui, "Align Objects:");
    ui.horizontal(|ui| {
        for (icon, tip, p) in [
            ("align-start-vertical", "Horizontal Align Left", json!({"horizontal": "left"})),
            ("align-center-vertical", "Horizontal Align Center", json!({"horizontal": "center"})),
            ("align-end-vertical", "Horizontal Align Right", json!({"horizontal": "right"})),
            ("align-start-horizontal", "Vertical Align Top", json!({"vertical": "top"})),
            ("align-center-horizontal", "Vertical Align Center", json!({"vertical": "center"})),
            ("align-end-horizontal", "Vertical Align Bottom", json!({"vertical": "bottom"})),
        ] {
            if widgets::icon_button(ui, icon, tip, false, 28.0).clicked() {
                app.run("object.align", p).ok();
            }
        }
    });
    dim_label(ui, "Distribute Objects:");
    ui.horizontal(|ui| {
        for (icon, tip, p) in [
            ("align-vertical-justify-start", "Vertical Distribute Top", json!({"vertical": "top"})),
            ("align-vertical-justify-center", "Vertical Distribute Center", json!({"vertical": "center"})),
            ("align-vertical-justify-end", "Vertical Distribute Bottom", json!({"vertical": "bottom"})),
            ("align-horizontal-justify-start", "Horizontal Distribute Left", json!({"horizontal": "left"})),
            ("align-horizontal-justify-center", "Horizontal Distribute Center", json!({"horizontal": "center"})),
            ("align-horizontal-justify-end", "Horizontal Distribute Right", json!({"horizontal": "right"})),
        ] {
            if widgets::icon_button(ui, icon, tip, false, 28.0).clicked() {
                app.run("object.distribute", p).ok();
            }
        }
    });
    dim_label(ui, "Distribute Spacing:");
    ui.horizontal(|ui| {
        if widgets::icon_button(ui, "arrow-up-down", "Vertical Distribute Space", false, 28.0).clicked() {
            app.run("object.distributeSpacing", json!({"axis": "vertical"})).ok();
        }
        if widgets::icon_button(ui, "arrow-left-right", "Horizontal Distribute Space", false, 28.0).clicked() {
            app.run("object.distributeSpacing", json!({"axis": "horizontal"})).ok();
        }
    });
}

pub fn pathfinder(app: &mut DrawcraftApp, ui: &mut Ui) {
    dim_label(ui, "Shape Modes:");
    ui.horizontal(|ui| {
        for (icon, tip, op) in [
            ("squares-unite", "Unite", "unite"),
            ("squares-subtract", "Minus Front", "minusFront"),
            ("squares-intersect", "Intersect", "intersect"),
            ("squares-exclude", "Exclude", "exclude"),
        ] {
            if widgets::icon_button(ui, icon, tip, false, 30.0).clicked() {
                app.run(&format!("object.pathfinder.{op}"), json!({})).ok();
            }
        }
    });
    dim_label(ui, "Pathfinders:");
    ui.horizontal_wrapped(|ui| {
        for (tip, op) in
            [("Divide", "divide"), ("Trim", "trim"), ("Merge", "merge"), ("Crop", "crop"), ("Outline", "outline"), ("Minus Back", "minusBack")]
        {
            if widgets::flat_button(ui, tip, 72.0).clicked() {
                app.run(&format!("object.pathfinder.{op}"), json!({})).ok();
            }
        }
    });
}

fn history(app: &mut DrawcraftApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let Some(st) = app.session.active() else { return };
    let undo: Vec<String> = st.history.undo.iter().map(|h| h.label.clone()).collect();
    let redo: Vec<String> = st.history.redo.iter().rev().map(|h| h.label.clone()).collect();
    let n_undo = undo.len();
    let mut steps: Option<i64> = None;
    egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
        let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::click());
        ui.painter().text(r.left_center() + vec2(6.0, 0.0), egui::Align2::LEFT_CENTER, "Open", egui::FontId::proportional(12.0), t.text_dim);
        if resp.clicked() {
            steps = Some(-(n_undo as i64));
        }
        for (i, l) in undo.iter().enumerate() {
            let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::click());
            if i + 1 == n_undo {
                ui.painter().rect_filled(r, 0.0, t.row_selected);
            }
            ui.painter().text(r.left_center() + vec2(6.0, 0.0), egui::Align2::LEFT_CENTER, l, egui::FontId::proportional(12.0), t.text);
            if resp.clicked() {
                steps = Some(i as i64 + 1 - n_undo as i64);
            }
        }
        for (i, l) in redo.iter().enumerate() {
            let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::click());
            ui.painter().text(r.left_center() + vec2(6.0, 0.0), egui::Align2::LEFT_CENTER, l, egui::FontId::proportional(12.0), t.text_disabled);
            if resp.clicked() {
                steps = Some(i as i64 + 1);
            }
        }
    });
    if let Some(s) = steps {
        let (cmd, n) = if s < 0 { ("edit.undo", -s) } else { ("edit.redo", s) };
        for _ in 0..n {
            app.run(cmd, json!({})).ok();
        }
    }
}

fn info(app: &mut DrawcraftApp, ui: &mut Ui) {
    let p = app.hover_doc.unwrap_or_default();
    ui.monospace(format!("X: {:>9.2} pt   Y: {:>9.2} pt", p.x, p.y));
    if let Some(st) = app.session.active()
        && let Some(b) = st.doc.bounds_of(&st.selection.objects, false)
    {
        ui.monospace(format!("W: {:>9.2} pt   H: {:>9.2} pt", b.width(), b.height()));
    }
    if let Some(n) = first_selected(app) {
        ui.label(format!("Fill: {}", n.appearance.fill_paint().label()));
        ui.label(format!("Stroke: {}", n.appearance.stroke_paint().label()));
    }
}

fn artboards(app: &mut DrawcraftApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let abs: Vec<(usize, String)> =
        app.session.active().map(|d| d.doc.artboards.iter().enumerate().map(|(i, a)| (i, a.name.clone())).collect()).unwrap_or_default();
    for (i, name) in abs {
        let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::click());
        if resp.hovered() {
            ui.painter().rect_filled(r, 0.0, t.hover);
        }
        ui.painter().text(
            r.left_center() + vec2(6.0, 0.0),
            egui::Align2::LEFT_CENTER,
            format!("{}", i + 1),
            egui::FontId::proportional(12.0),
            t.text_dim,
        );
        ui.painter().text(r.left_center() + vec2(28.0, 0.0), egui::Align2::LEFT_CENTER, name, egui::FontId::proportional(12.0), t.text);
    }
    ui.horizontal(|ui| {
        if widgets::icon_button(ui, "plus", "New Artboard", false, 22.0).clicked() {
            app.run("artboard.new", json!({})).ok();
        }
    });
}

fn gradient(app: &mut DrawcraftApp, ui: &mut Ui) {
    let n = first_selected(app);
    let paint = n.as_ref().map(|n| n.appearance.fill_paint()).unwrap_or(Paint::None);
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 20.0), Sense::hover());
    let preview = if matches!(paint, Paint::Gradient(_)) {
        paint.clone()
    } else {
        Paint::Gradient(Box::new(drawcraft_color::GradientPaint::new(Default::default())))
    };
    paint_chip(ui, r, &preview);
    ui.horizontal(|ui| {
        dim_label(ui, "Type:");
        if widgets::flat_button(ui, "Linear", 60.0).clicked() {
            app.run("paint.setFill", json!({"gradient": {"kind": "linear"}})).ok();
        }
        if widgets::flat_button(ui, "Radial", 60.0).clicked() {
            app.run("paint.setFill", json!({"gradient": {"kind": "radial"}})).ok();
        }
    });
}

fn navigator(app: &mut DrawcraftApp, ui: &mut Ui) {
    let zoom = app.view().map(|v| v.zoom).unwrap_or(1.0);
    let mut z = zoom * 100.0;
    if ui.add(egui::Slider::new(&mut z, 3.13..=6400.0).logarithmic(true).suffix("%")).changed() {
        app.run("view.setZoom", json!({"zoom": z})).ok();
    }
    let _ = CornerRadius::ZERO;
}
