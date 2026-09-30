//! The Pen tool (P).
//!
//! Click adds a corner anchor; click-drag adds a smooth anchor with symmetric handles (Alt-drag
//! breaks the handles); Shift constrains to 45°. Clicking the first anchor closes the path.
//! Enter/Esc (or switching tools) ends the path. Clicking the end of a selected open path continues
//! it. The rubber-band preview shows the next segment.

use drawcraft_doc::{NodeId, NodeKind};
use drawcraft_geom::{BezPath, Point};
use serde_json::json;

use crate::{Action, Cursor, Mods, Overlay, PointerEvent, PointerKind, Tool, ToolContext, ToolKey};

#[derive(Default)]
pub struct PenTool {
    /// The path being drawn (set once the first anchor exists and the engine selected it).
    drawing: bool,
    drag: Option<(Point, bool)>,
    hover: Option<Point>,
}

/// The open path the pen is extending: the single selected open path.
fn active_path(cx: &ToolContext) -> Option<(NodeId, Point, Point, Point)> {
    if cx.selection.objects.len() != 1 {
        return None;
    }
    let id = cx.selection.objects[0];
    let n = cx.doc.node(id)?;
    let NodeKind::Path { path, .. } = &n.kind else { return None };
    let sp = path.subpaths.last()?;
    if sp.closed {
        return None;
    }
    let first = sp.anchors.first()?.p;
    let last = sp.anchors.last()?;
    Some((id, first, last.p, last.h_out))
}

impl Tool for PenTool {
    fn id(&self) -> &'static str {
        "pen"
    }
    fn busy(&self) -> bool {
        self.drag.is_some()
    }
    fn pointer(&mut self, cx: &ToolContext, ev: &PointerEvent) -> Vec<Action> {
        let mut p = ev.pos;
        let tol = cx.tol(5.0);
        let active = if self.drawing { active_path(cx) } else { None };
        if self.drawing && active.is_none() && ev.kind == PointerKind::Down {
            self.drawing = false;
        }
        match ev.kind {
            PointerKind::Move => {
                self.hover = Some(p);
                vec![]
            }
            PointerKind::Down => {
                if let Some((id, first, last, _)) = active {
                    if ev.mods.shift {
                        p = last + drawcraft_geom::constrain_angle(p - last, 45.0);
                    }
                    if p.distance(first) <= tol {
                        self.drag = Some((first, true));
                        return vec![Action::Begin("Close Path".into()), Action::Preview("path.close".into(), json!({"id": id.0}))];
                    }
                    self.drag = Some((p, false));
                    return vec![Action::Begin("Pen".into()), Action::Preview("path.appendAnchor".into(), json!({"id": id.0, "x": p.x, "y": p.y}))];
                }
                // Continue a selected open path when clicking on one of its ends.
                if let Some((_, first, last, _)) = active_path(cx)
                    && (p.distance(last) <= tol || p.distance(first) <= tol)
                {
                    self.drawing = true;
                    if p.distance(first) <= tol && p.distance(last) > tol {
                        return vec![Action::Exec("path.reverse".into(), json!({}))];
                    }
                    return vec![];
                }
                self.drawing = true;
                self.drag = Some((p, false));
                vec![Action::Begin("Pen".into()), Action::Preview("path.create".into(), json!({"anchors": [{"x": p.x, "y": p.y}]}))]
            }
            PointerKind::Drag => {
                let Some((a, closing)) = self.drag else { return vec![] };
                let mut out_h = ev.pos;
                if ev.mods.shift {
                    out_h = a + drawcraft_geom::constrain_angle(ev.pos - a, 45.0);
                }
                let alt = ev.mods.alt;
                let Some((id, ..)) = active_path(cx).or(active) else {
                    // First anchor of a new path: re-issue create with handles.
                    let in_h = a - (out_h - a);
                    return vec![Action::Preview(
                        "path.create".into(),
                        json!({"anchors": [{"x": a.x, "y": a.y, "out": [out_h.x, out_h.y], "in": [in_h.x, in_h.y]}]}),
                    )];
                };
                if closing {
                    return vec![Action::Preview(
                        "path.close".into(),
                        json!({"id": id.0, "in": [2.0 * a.x - out_h.x, 2.0 * a.y - out_h.y], "independent": alt}),
                    )];
                }
                let in_h = if alt { a } else { a - (out_h - a) };
                vec![Action::Preview(
                    "path.appendAnchor".into(),
                    json!({"id": id.0, "x": a.x, "y": a.y, "in": [in_h.x, in_h.y], "out": [out_h.x, out_h.y]}),
                )]
            }
            PointerKind::Up => {
                let Some((_, closing)) = self.drag.take() else { return vec![] };
                if closing {
                    self.drawing = false;
                }
                vec![Action::Commit]
            }
            PointerKind::DoubleClick => vec![],
        }
    }
    fn key(&mut self, _cx: &ToolContext, key: ToolKey, _m: Mods) -> Vec<Action> {
        match key {
            ToolKey::Enter | ToolKey::Escape => {
                self.drawing = false;
                self.drag = None;
                vec![]
            }
            _ => vec![],
        }
    }
    fn deactivate(&mut self, _cx: &ToolContext) -> Vec<Action> {
        self.drawing = false;
        vec![]
    }
    fn overlays(&self, cx: &ToolContext) -> Vec<Overlay> {
        if !self.drawing || self.drag.is_some() {
            return vec![];
        }
        let (Some((id, _, last, out)), Some(h)) = (active_path(cx), self.hover) else { return vec![] };
        let mut bp = BezPath::new();
        bp.move_to(last);
        if out.distance(last) > 1e-9 {
            bp.quad_to(out, h);
        } else {
            bp.line_to(h);
        }
        let c = cx.doc.layer_color(id);
        vec![Overlay::Path { path: bp, color: c, width: 1.0, dashed: false }]
    }
    fn cursor(&self, cx: &ToolContext, p: Point, _m: Mods) -> Cursor {
        if let Some((_, first, last, _)) = active_path(cx) {
            if self.drawing && p.distance(first) <= cx.tol(5.0) {
                return Cursor::PenClose;
            }
            if !self.drawing && (p.distance(last) <= cx.tol(5.0) || p.distance(first) <= cx.tol(5.0)) {
                return Cursor::PenContinue;
            }
        }
        Cursor::Pen
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::*;
    use drawcraft_doc::Selection;

    #[test]
    fn first_click_creates_path() {
        let (d, _) = doc_with_rect();
        let s = Selection::default();
        let p = paint();
        let cx = cx(&d, &s, &p);
        let mut t = PenTool::default();
        let a = t.pointer(&cx, &PointerEvent::new(PointerKind::Down, 10.0, 10.0));
        assert_eq!(a[0], Action::Begin("Pen".into()));
        assert!(matches!(&a[1], Action::Preview(c, _) if c == "path.create"));
        assert_eq!(t.pointer(&cx, &PointerEvent::new(PointerKind::Up, 10.0, 10.0)), vec![Action::Commit]);
    }
}
