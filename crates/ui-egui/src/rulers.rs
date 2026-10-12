//! Ruler display coordinates and bounded tick generation.

use vectorcraft_geom::Point;

use crate::VectorcraftApp;

pub fn active_artboard(app: &VectorcraftApp) -> usize {
    let count = app.session.active().map_or(0, |st| st.doc.artboards.len());
    let index = if app.session.tool_id() == "artboard" {
        app.session.tool_options().get("active").and_then(serde_json::Value::as_u64).and_then(|i| usize::try_from(i).ok()).unwrap_or(0)
    } else {
        app.view().map_or(0, |v| v.artboard)
    };
    if index < count { index } else { 0 }
}

pub fn origin(app: &VectorcraftApp) -> Point {
    app.session.active().map_or(Point::ZERO, |st| st.rulers.origin(&st.doc.artboards, active_artboard(app)))
}

/// Display a document point without moving artwork or changing engine coordinates.
pub fn display(app: &VectorcraftApp, point: Point) -> Point {
    let origin = origin(app);
    Point::new(point.x - origin.x, point.y - origin.y)
}

/// Convert an absolute X/Y position field back to document space; lengths stay lengths.
pub fn document_value(app: &VectorcraftApp, axis: &str, value: f64) -> f64 {
    let origin = origin(app);
    match axis {
        "x" => value + origin.x,
        "y" => value + origin.y,
        _ => value,
    }
}

/// Each tick is an offset in screen points and a value in ruler units. The hard cap also
/// prevents malformed huge coordinates from causing a non-progressing float loop.
pub(crate) fn ticks(from: f64, to: f64, per: f64, minor: f64, zoom: f64) -> impl Iterator<Item = (f32, f64)> {
    let first = (from / per / minor).floor();
    let valid = from.is_finite()
        && to.is_finite()
        && to >= from
        && per.is_finite()
        && per > 0.0
        && minor.is_finite()
        && minor > 0.0
        && zoom.is_finite()
        && zoom > 0.0
        && first.is_finite();
    (0..8192u32)
        .map(move |i| {
            let value = (first + f64::from(i)) * minor;
            (((value * per - from) * zoom) as f32, value)
        })
        .take_while(move |(offset, value)| valid && offset.is_finite() && value.is_finite() && *value * per <= to)
}

/// A corner drag stays attached to the document and coordinate system it started in.
#[derive(Clone)]
struct OriginDrag {
    uid: u64,
    revision: u64,
    board_id: Option<u32>,
    artboard: usize,
    settings: vectorcraft_doc::rulers::RulerSettings,
    origin: Point,
}

impl OriginDrag {
    fn capture(app: &VectorcraftApp) -> Option<Self> {
        let st = app.session.active()?;
        Some(Self {
            uid: st.uid,
            revision: st.revision,
            board_id: st.doc.artboards.get(active_artboard(app)).map(|b| b.id),
            artboard: active_artboard(app),
            settings: st.rulers.clone(),
            origin: origin(app),
        })
    }

    fn matches(&self, app: &VectorcraftApp) -> bool {
        app.session.active().is_some_and(|st| {
            st.uid == self.uid
                && st.revision == self.revision
                && st.doc.artboards.get(self.artboard).map(|b| b.id) == self.board_id
                && st.rulers == self.settings
        }) && self.artboard == active_artboard(app)
            && self.origin == origin(app)
    }
}

pub(crate) fn corner(app: &mut VectorcraftApp, ui: &egui::Ui, response: &egui::Response, canvas: egui::Rect, xf: &crate::canvas::Xf) {
    let id = egui::Id::new("ruler-origin-drag");
    if response.double_clicked() {
        ui.data_mut(|d| d.remove::<OriginDrag>(id));
        if let Err(e) = app.run("view.rulers.origin", serde_json::json!({"reset":true})) {
            app.status(e);
        }
        return;
    }
    if response.drag_started_by(egui::PointerButton::Primary)
        && let Some(drag) = OriginDrag::capture(app)
    {
        ui.data_mut(|d| d.insert_temp(id, drag));
    }
    let Some(drag) = ui.data(|d| d.get_temp::<OriginDrag>(id)) else { return };
    if !drag.matches(app) || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        ui.data_mut(|d| d.remove::<OriginDrag>(id));
        return;
    }
    let pointer = response.interact_pointer_pos().or_else(|| ui.input(|i| i.pointer.latest_pos()));
    if response.drag_stopped_by(egui::PointerButton::Primary) {
        ui.data_mut(|d| d.remove::<OriginDrag>(id));
        if let Some(pointer) = pointer.filter(|p| canvas.contains(*p)) {
            let point = xf.to_doc(pointer);
            if let Err(e) = app.run("view.rulers.origin", serde_json::json!({"point":[point.x,point.y], "artboard":drag.artboard})) {
                app.status(e);
            }
        }
    } else if response.dragged_by(egui::PointerButton::Primary) {
        if let Some(pointer) = pointer.filter(|p| canvas.contains(*p)) {
            let paint = ui.painter().clone().with_layer_id(egui::LayerId::new(egui::Order::Foreground, id)).with_clip_rect(canvas);
            let stroke = egui::Stroke::new(1.0, crate::theme::Tokens::get(ui.ctx()).ruler_tick);
            paint.line_segment([egui::pos2(canvas.left(), pointer.y), egui::pos2(canvas.right(), pointer.y)], stroke);
            paint.line_segment([egui::pos2(pointer.x, canvas.top()), egui::pos2(pointer.x, canvas.bottom())], stroke);
        }
    } else if !ui.input(|i| i.pointer.primary_down()) {
        ui.data_mut(|d| d.remove::<OriginDrag>(id));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn ruler_drag_binds_board_identity_and_deleted_active_indexes_fall_back() {
        let mut app = VectorcraftApp::new(vectorcraft_engine::Session::new(), Default::default());
        app.run("file.new", json!({"width":100,"height":100})).unwrap();
        app.run("artboard.new", json!({"x":0,"y":0,"width":100,"height":100})).unwrap();
        app.run("view.rulers.global", json!({"on":false})).unwrap();
        app.view_mut().unwrap().artboard = 1;
        let drag = OriginDrag::capture(&app).unwrap();
        assert!(drag.matches(&app));
        std::sync::Arc::make_mut(&mut app.session.active_mut().unwrap().doc).artboards.swap(0, 1);
        assert!(!drag.matches(&app), "overlapping artboards still have distinct identities");
        app.run("artboard.delete", json!({"index":1})).unwrap();
        assert_eq!(active_artboard(&app), 0);
        app.run("view.rulers.origin", json!({"reset":true})).unwrap();
        assert_eq!(app.run("view.rulers.info", json!({})).unwrap()["origin"], json!([0.0, 0.0]));
    }

    #[test]
    fn ruler_readouts_invert_position_fields_without_changing_lengths() {
        let mut app = VectorcraftApp::new(vectorcraft_engine::Session::new(), Default::default());
        app.run("file.new", json!({})).unwrap();
        app.run("view.rulers.origin", json!({"point":[100,-20]})).unwrap();
        let p = display(&app, Point::new(135.0, 50.0));
        assert_eq!(p, Point::new(35.0, 70.0));
        assert_eq!(document_value(&app, "x", p.x), 135.0);
        assert_eq!(document_value(&app, "y", p.y), 50.0);
        assert_eq!(document_value(&app, "width", 35.0), 35.0);
    }

    #[test]
    fn ruler_ticks_are_bounded_for_hostile_coordinates() {
        assert_eq!(ticks(0.0, 100.0, 1.0, 10.0, 1.0).collect::<Vec<_>>(), (0..=10).map(|n| (n as f32 * 10.0, n as f64 * 10.0)).collect::<Vec<_>>());
        assert_eq!(ticks(f64::NAN, 100.0, 1.0, 10.0, 1.0).count(), 0);
        assert_eq!(ticks(0.0, 100.0, 1.0, 0.0, 1.0).count(), 0);
        assert_eq!(ticks(0.0, 100.0, 1.0, 10.0, f64::INFINITY).count(), 0);
        assert_eq!(ticks(1e300, 1e300, 1.0, 1.0, 1.0).count(), 8192);
    }
}
