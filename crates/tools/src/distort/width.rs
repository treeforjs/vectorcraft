//! Width tool (Shift+W).
//!
//! Hovering a stroked path shows a hollow width-point diamond with the stroke's width at that
//! spot. Dragging outward from the path creates (or, on an existing width point's handle end,
//! edits) a width point — symmetric by default, Alt changes only the side being dragged. Dragging
//! a width point's centre slides it along the path. Delete/Backspace removes the selected width
//! point. Gestures preview `stroke.widthPoint.set {id, t, left, right, index?}` (side widths in
//! points) and commit on release; Delete runs `stroke.widthPoint.remove {id, index}`.

use serde_json::json;
use vectorcraft_doc::{Document, NodeId, NodeKind, StrokeLayer};
use vectorcraft_geom::{Point, Vec2};

use super::pathutil::{eval_fraction, left_normal, nearest_fraction};
use super::{BLUE, diamond};
use crate::{Action, Cursor, Mods, Overlay, PointerEvent, PointerKind, Tool, ToolContext, ToolKey};

/// Where the pointer is relative to a stroked path.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Spot {
    id: NodeId,
    sub: usize,
    /// Fraction along the subpath.
    t: f64,
    p: Point,
    /// Left normal.
    n: Vec2,
    /// Side widths (points) at `t`.
    left: f64,
    right: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Mode {
    /// Change widths; `Some(left?)` = only one side (Alt).
    Width(Option<bool>),
    /// Slide along the path.
    Move,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Drag {
    spot: Spot,
    index: Option<usize>,
    mode: Mode,
    left: f64,
    right: f64,
    t: f64,
}

#[derive(Default)]
pub struct WidthTool {
    hover: Option<Spot>,
    drag: Option<Drag>,
    /// Selected width point: (path, position t).
    selected: Option<(NodeId, f64)>,
}

fn stroke_of(doc: &Document, id: NodeId) -> Option<&StrokeLayer> {
    doc.node(id)?.appearance.stroke().filter(|s| s.visible && s.width > 0.0 && !s.paint.is_none())
}

/// Side widths (points) of a stroke at fraction `t`.
pub fn sides_at(st: &StrokeLayer, t: f64) -> (f64, f64) {
    let (l, r) = st.profile.as_ref().map(|p| p.at(t)).unwrap_or((1.0, 1.0));
    (l * st.width / 2.0, r * st.width / 2.0)
}

impl WidthTool {
    /// The stroked path nearest to `p` within reach (stroke half-width + a few pixels).
    fn spot_at(cx: &ToolContext, p: Point) -> Option<Spot> {
        let consider = |id: NodeId| -> Option<(f64, Spot)> {
            if !cx.doc.is_editable(id) {
                return None;
            }
            let n = cx.doc.node(id)?;
            let NodeKind::Path { path, guide: false, .. } = &n.kind else { return None };
            let st = stroke_of(cx.doc, id)?;
            let widest = st.profile.as_ref().map_or(1.0, |pr| pr.points.iter().map(|q| q.1.max(q.2)).fold(1.0, f64::max));
            let reach = widest * st.width / 2.0 + cx.tol(4.0);
            if !path.bounds()?.inflate(reach, reach).contains(p) {
                return None;
            }
            let (sub, t, q, tan, d) = nearest_fraction(path, p)?;
            if d > reach {
                return None;
            }
            let (left, right) = sides_at(st, t);
            Some((d, Spot { id, sub, t, p: q, n: left_normal(tan), left, right }))
        };
        let best_of = |ids: &mut dyn Iterator<Item = NodeId>| ids.filter_map(consider).min_by(|a, b| a.0.total_cmp(&b.0));
        if let Some(b) = best_of(&mut cx.selection.objects.iter().copied()) {
            return Some(b.1);
        }
        let mut all = vec![];
        cx.doc.walk(|n| {
            if matches!(n.kind, NodeKind::Path { .. }) {
                all.push(n.id);
            }
        });
        best_of(&mut all.into_iter().rev()).map(|b| b.1)
    }

    /// Width points of `id`: (index, t, on-path point, left normal, left, right).
    fn points_of(cx: &ToolContext, id: NodeId, sub: usize) -> Vec<(usize, f64, Point, Vec2, f64, f64)> {
        let Some(st) = stroke_of(cx.doc, id) else { return vec![] };
        let Some(pr) = &st.profile else { return vec![] };
        let Some(NodeKind::Path { path, .. }) = cx.doc.node(id).map(|n| &n.kind) else { return vec![] };
        let Some(sp) = path.subpaths.get(sub) else { return vec![] };
        pr.points
            .iter()
            .enumerate()
            .filter_map(|(i, (t, l, r))| {
                let (p, tan) = eval_fraction(sp, *t)?;
                Some((i, *t, p, left_normal(tan), l * st.width / 2.0, r * st.width / 2.0))
            })
            .collect()
    }

    fn params(d: &Drag) -> serde_json::Value {
        let mut v = json!({"id": d.spot.id.0, "t": d.t, "left": d.left, "right": d.right});
        if let Some(i) = d.index {
            v["index"] = json!(i);
        }
        v
    }
}

impl Tool for WidthTool {
    fn id(&self) -> &'static str {
        "width"
    }
    fn pointer(&mut self, cx: &ToolContext, ev: &PointerEvent) -> Vec<Action> {
        let p = ev.pos;
        match ev.kind {
            PointerKind::Move => {
                self.hover = Self::spot_at(cx, p);
                vec![]
            }
            PointerKind::Down => {
                let Some(spot) = Self::spot_at(cx, p) else {
                    self.selected = None;
                    return vec![];
                };
                let tol = cx.tol(5.0);
                // An existing width point under the pointer: its centre (move) or a handle end (width).
                let mut hit = None;
                for (i, t, q, n, l, r) in Self::points_of(cx, spot.id, spot.sub) {
                    if q.distance(p) <= tol {
                        hit = Some((i, t, q, n, l, r, Mode::Move));
                        break;
                    }
                    if (q + n * l).distance(p) <= tol {
                        hit = Some((i, t, q, n, l, r, Mode::Width(ev.mods.alt.then_some(true))));
                        break;
                    }
                    if (q - n * r).distance(p) <= tol {
                        hit = Some((i, t, q, n, l, r, Mode::Width(ev.mods.alt.then_some(false))));
                        break;
                    }
                }
                let drag = match hit {
                    Some((i, t, q, n, l, r, mode)) => {
                        Drag { spot: Spot { t, p: q, n, left: l, right: r, ..spot }, index: Some(i), mode, left: l, right: r, t }
                    }
                    None => {
                        let side = (p - spot.p).dot(spot.n) >= 0.0;
                        Drag { spot, index: None, mode: Mode::Width(ev.mods.alt.then_some(side)), left: spot.left, right: spot.right, t: spot.t }
                    }
                };
                self.selected = Some((spot.id, drag.t));
                self.drag = Some(drag);
                vec![Action::Begin("Width Point".into())]
            }
            PointerKind::Drag => {
                let Some(d) = &mut self.drag else { return vec![] };
                match d.mode {
                    Mode::Width(side) => {
                        let off = (p - d.spot.p).dot(d.spot.n);
                        match side {
                            None => {
                                d.left = off.abs();
                                d.right = off.abs();
                            }
                            Some(true) => d.left = off.max(0.0),
                            Some(false) => d.right = (-off).max(0.0),
                        }
                    }
                    Mode::Move => {
                        if let Some(NodeKind::Path { path, .. }) = cx.doc.node(d.spot.id).map(|n| &n.kind)
                            && let Some(sp) = path.subpaths.get(d.spot.sub)
                        {
                            let one = vectorcraft_geom::PathData::single(sp.clone());
                            if let Some((_, t, q, tan, _)) = nearest_fraction(&one, p) {
                                d.t = t;
                                d.spot.p = q;
                                d.spot.n = left_normal(tan);
                            }
                        }
                    }
                }
                let v = Self::params(d);
                vec![Action::Preview("stroke.widthPoint.set".into(), v)]
            }
            PointerKind::Up => match self.drag.take() {
                Some(d) => {
                    self.selected = Some((d.spot.id, d.t));
                    vec![Action::Commit]
                }
                None => vec![],
            },
            _ => vec![],
        }
    }
    fn key(&mut self, cx: &ToolContext, key: ToolKey, _mods: Mods) -> Vec<Action> {
        if !matches!(key, ToolKey::Delete | ToolKey::Backspace) {
            return vec![];
        }
        let Some((id, t)) = self.selected else { return vec![] };
        let Some(pr) = stroke_of(cx.doc, id).and_then(|s| s.profile.as_ref()) else { return vec![] };
        let Some((index, _)) = pr.points.iter().enumerate().min_by(|a, b| (a.1.0 - t).abs().total_cmp(&(b.1.0 - t).abs())) else { return vec![] };
        self.selected = None;
        vec![Action::Exec("stroke.widthPoint.remove".into(), json!({"id": id.0, "index": index}))]
    }
    fn overlays(&self, cx: &ToolContext) -> Vec<Overlay> {
        let mut out = vec![];
        let r = cx.tol(4.0);
        let focus = self.drag.map(|d| d.spot).or(self.hover);
        let show = |id: NodeId, sub: usize, out: &mut Vec<Overlay>| {
            for (_, t, q, n, l, rr) in Self::points_of(cx, id, sub) {
                let sel = self.selected.is_some_and(|(sid, st)| sid == id && (st - t).abs() < 1e-6);
                out.push(Overlay::Line { a: q + n * l, b: q - n * rr, color: BLUE, dashed: false });
                out.push(Overlay::Path { path: diamond(q, r), color: BLUE, width: if sel { 2.5 } else { 1.0 }, dashed: false });
                out.push(Overlay::Handle { p: q + n * l, color: BLUE });
                out.push(Overlay::Handle { p: q - n * rr, color: BLUE });
            }
        };
        if let Some(s) = focus {
            show(s.id, s.sub, &mut out);
        }
        if let Some((id, _)) = self.selected
            && focus.is_none_or(|s| s.id != id)
        {
            show(id, 0, &mut out);
        }
        if let Some(d) = &self.drag {
            let q = d.spot.p;
            out.push(Overlay::Line { a: q + d.spot.n * d.left, b: q - d.spot.n * d.right, color: BLUE, dashed: false });
            out.push(Overlay::Path { path: diamond(q, r), color: BLUE, width: 2.0, dashed: false });
            out.push(Overlay::Measure { p: q + d.spot.n * (d.left + r * 3.0), text: format!("W: {:.2} pt", d.left + d.right) });
        } else if let Some(h) = &self.hover {
            out.push(Overlay::Line { a: h.p + h.n * h.left, b: h.p - h.n * h.right, color: BLUE, dashed: true });
            out.push(Overlay::Path { path: diamond(h.p, r), color: BLUE, width: 1.0, dashed: false });
        }
        out
    }
    fn cursor(&self, _cx: &ToolContext, _p: Point, _mods: Mods) -> Cursor {
        Cursor::Crosshair
    }
    fn busy(&self) -> bool {
        self.drag.is_some()
    }
    fn deactivate(&mut self, _cx: &ToolContext) -> Vec<Action> {
        self.hover = None;
        if self.drag.take().is_some() { vec![Action::Commit] } else { vec![] }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::*;
    use vectorcraft_doc::{Appearance, Node, Selection, WidthProfile};
    use vectorcraft_geom::{PathData, SubPath};

    fn doc_line() -> (Document, NodeId) {
        let mut d = Document::new(500.0, 500.0);
        let l = d.layers[0].id;
        let id = d.alloc_id();
        let mut ap = Appearance::default_art();
        ap.stroke_mut().unwrap().width = 10.0;
        let path = PathData::single(SubPath::polyline(&[Point::new(100.0, 200.0), Point::new(300.0, 200.0)], false));
        d.insert(Some(l), 0, Node::path(id, path, ap)).unwrap();
        (d, id)
    }

    #[test]
    fn drag_outward_creates_symmetric_width_point() {
        let (d, id) = doc_line();
        let (s, p) = (Selection::default(), paint());
        let c = cx(&d, &s, &p);
        let mut t = WidthTool::default();
        assert_eq!(t.pointer(&c, &PointerEvent::new(PointerKind::Down, 150.0, 201.0)), vec![Action::Begin("Width Point".into())]);
        let acts = t.pointer(&c, &PointerEvent::new(PointerKind::Drag, 150.0, 220.0));
        let Action::Preview(cmd, v) = &acts[0] else { panic!("{acts:?}") };
        assert_eq!(cmd, "stroke.widthPoint.set");
        assert_eq!(v["id"], json!(id.0));
        assert!((v["t"].as_f64().unwrap() - 0.25).abs() < 1e-3);
        assert!((v["left"].as_f64().unwrap() - 20.0).abs() < 1e-6 && (v["right"].as_f64().unwrap() - 20.0).abs() < 1e-6);
        assert_eq!(t.pointer(&c, &PointerEvent::new(PointerKind::Up, 150.0, 220.0)), vec![Action::Commit]);
    }

    #[test]
    fn alt_drag_changes_one_side_and_move_slides() {
        let (mut d, id) = doc_line();
        let n = d.node_mut(id).unwrap();
        n.appearance.stroke_mut().unwrap().profile = Some(WidthProfile { points: vec![(0.0, 1.0, 1.0), (0.5, 2.0, 2.0), (1.0, 1.0, 1.0)] });
        let (s, p) = (Selection::default(), paint());
        let c = cx(&d, &s, &p);
        let mut t = WidthTool::default();
        // Alt-drag the left handle end (y = 200 - 10) further up.
        let alt = Mods { alt: true, ..Default::default() };
        t.pointer(&c, &PointerEvent::new(PointerKind::Down, 200.0, 190.0).with_mods(alt));
        let acts = t.pointer(&c, &PointerEvent::new(PointerKind::Drag, 200.0, 170.0).with_mods(alt));
        let Action::Preview(_, v) = &acts[0] else { panic!() };
        assert_eq!(v["index"], json!(1));
        assert!((v["left"].as_f64().unwrap() - 30.0).abs() < 1e-6 && (v["right"].as_f64().unwrap() - 10.0).abs() < 1e-6);
        t.pointer(&c, &PointerEvent::new(PointerKind::Up, 200.0, 170.0));
        // Drag the centre along the path.
        t.pointer(&c, &PointerEvent::new(PointerKind::Down, 200.0, 200.0));
        let acts = t.pointer(&c, &PointerEvent::new(PointerKind::Drag, 250.0, 205.0));
        let Action::Preview(_, v) = &acts[0] else { panic!() };
        assert!((v["t"].as_f64().unwrap() - 0.75).abs() < 1e-3);
        t.pointer(&c, &PointerEvent::new(PointerKind::Up, 250.0, 205.0));
        let del = t.key(&c, ToolKey::Delete, Mods::default());
        assert!(matches!(&del[0], Action::Exec(c, _) if c == "stroke.widthPoint.remove"));
    }
}
