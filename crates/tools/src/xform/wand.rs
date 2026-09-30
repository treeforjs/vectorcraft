//! Magic Wand (Y) and Lasso (Q).
//!
//! Magic Wand: click an object to select every object whose fill colour is within `tolerance`
//! (0..255 RGB distance); Shift adds, Alt subtracts. Lasso: freehand loop selecting anchor points
//! inside it (direct-selection style); Shift adds, Alt subtracts.

use std::collections::BTreeSet;

use drawcraft_color::Paint;
use drawcraft_doc::hit::hit_test;
use drawcraft_doc::{AnchorRef, Document, NodeId, NodeKind};
use drawcraft_geom::Point;
use serde_json::{Value, json};

use super::paint_owner;
use crate::{Action, Cursor, Mods, Overlay, PointerEvent, PointerKind, Tool, ToolContext, ToolKey, json_ids};

pub struct MagicWandTool {
    /// RGB distance tolerance, 0..255.
    pub tolerance: f64,
}

impl Default for MagicWandTool {
    fn default() -> Self {
        Self { tolerance: 32.0 }
    }
}

fn fill_of(n: &drawcraft_doc::Node) -> Paint {
    match &n.kind {
        NodeKind::Text(t) => t.runs.first().map(|r| r.style.fill.clone()).unwrap_or_default(),
        _ => n.appearance.fill_paint(),
    }
}

/// Are two fills "similar" within `tol` (0..255 Euclidean RGB distance)? Non-solid paints must match exactly.
pub fn similar_fill(a: &Paint, b: &Paint, tol: f64) -> bool {
    match (a.color(), b.color()) {
        (Some(x), Some(y)) => {
            let (x, y) = (x.to_rgb(), y.to_rgb());
            let d: f64 = (0..3).map(|i| ((x[i] - y[i]) as f64 * 255.0).powi(2)).sum::<f64>().sqrt();
            d <= tol + 1e-6
        }
        _ => a == b,
    }
}

/// Paintable leaves (paths, compounds, text, images) that are visible and editable.
fn paintable(doc: &Document) -> Vec<NodeId> {
    let mut out = vec![];
    let mut compounds: Vec<NodeId> = vec![];
    doc.walk(|n| {
        if matches!(n.kind, NodeKind::Compound { .. }) {
            compounds.push(n.id);
            out.push(n.id);
        } else if !n.is_container() {
            out.push(n.id);
        }
    });
    out.retain(|id| {
        !doc.parent_of(*id).is_some_and(|p| compounds.contains(&p))
            && doc.is_editable(*id)
            && doc.is_visible(*id)
            && doc.node(*id).is_some_and(|n| n.visible)
    });
    out
}

impl Tool for MagicWandTool {
    fn id(&self) -> &'static str {
        "magicWand"
    }

    fn pointer(&mut self, cx: &ToolContext, ev: &PointerEvent) -> Vec<Action> {
        if ev.kind != PointerKind::Down {
            return vec![];
        }
        let Some(h) = hit_test(cx.doc, ev.pos, cx.hit_options()) else {
            return if ev.mods.shift || ev.mods.alt { vec![] } else { vec![Action::Exec("select.none".into(), json!({}))] };
        };
        let src = paint_owner(cx.doc, h.leaf);
        let Some(reference) = cx.doc.node(src).map(fill_of) else { return vec![] };
        let hits: Vec<NodeId> = paintable(cx.doc)
            .into_iter()
            .filter(|id| cx.doc.node(*id).is_some_and(|n| similar_fill(&fill_of(n), &reference, self.tolerance)))
            .collect();
        if ev.mods.alt {
            let keep: Vec<NodeId> = cx.selection.objects.iter().copied().filter(|id| !hits.contains(id)).collect();
            return vec![Action::Exec("select.set".into(), json!({ "ids": json_ids(&keep) }))];
        }
        let cmd = if ev.mods.shift { "select.add" } else { "select.set" };
        vec![Action::Exec(cmd.into(), json!({ "ids": json_ids(&hits) }))]
    }

    fn cursor(&self, _cx: &ToolContext, _p: Point, _m: Mods) -> Cursor {
        Cursor::Crosshair
    }

    fn options(&self) -> Value {
        json!({ "tolerance": self.tolerance })
    }

    fn set_option(&mut self, key: &str, value: &Value) {
        if key == "tolerance"
            && let Some(v) = value.as_f64()
        {
            self.tolerance = v.clamp(0.0, 255.0);
        }
    }
}

/// Even-odd point-in-polygon.
pub fn point_in_polygon(poly: &[Point], p: Point) -> bool {
    let mut inside = false;
    let n = poly.len();
    if n < 3 {
        return false;
    }
    let mut j = n - 1;
    for i in 0..n {
        let (a, b) = (poly[i], poly[j]);
        if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
            inside = !inside;
        }
        j = i;
    }
    inside
}

#[derive(Default)]
pub struct LassoTool {
    points: Vec<Point>,
    active: bool,
}

fn anchors_json(v: &BTreeSet<AnchorRef>) -> Value {
    Value::Array(v.iter().map(|(s, a)| json!([s, a])).collect())
}

impl Tool for LassoTool {
    fn id(&self) -> &'static str {
        "lasso"
    }

    fn busy(&self) -> bool {
        self.active
    }

    fn pointer(&mut self, cx: &ToolContext, ev: &PointerEvent) -> Vec<Action> {
        let p = ev.pos;
        match ev.kind {
            PointerKind::Down => {
                self.points = vec![p];
                self.active = true;
                vec![]
            }
            PointerKind::Drag if self.active => {
                if self.points.last().is_none_or(|q| q.distance(p) >= cx.tol(1.0)) {
                    self.points.push(p);
                }
                vec![]
            }
            PointerKind::Up if self.active => {
                self.active = false;
                let poly = std::mem::take(&mut self.points);
                let (add, sub) = (ev.mods.shift, ev.mods.alt);
                if poly.len() < 3 {
                    return if add || sub { vec![] } else { vec![Action::Exec("select.none".into(), json!({}))] };
                }
                // Anchors inside the loop for every editable path.
                let mut hits: Vec<(NodeId, BTreeSet<AnchorRef>)> = vec![];
                cx.doc.walk(|n| {
                    if let NodeKind::Path { path, .. } = &n.kind {
                        let v: BTreeSet<AnchorRef> =
                            path.anchors().filter(|(_, _, a)| point_in_polygon(&poly, a.p)).map(|(s, i, _)| (s, i)).collect();
                        if !v.is_empty() {
                            hits.push((n.id, v));
                        }
                    }
                });
                hits.retain(|(id, _)| cx.doc.is_editable(*id) && cx.doc.is_visible(*id));
                if sub {
                    // Current anchor selection minus the lassoed anchors.
                    let mut items = vec![];
                    for id in &cx.selection.objects {
                        let Some(path) = cx.doc.node(*id).and_then(|n| n.path_data()) else { continue };
                        let cur: BTreeSet<AnchorRef> = match cx.selection.partial(*id) {
                            Some(s) => s.clone(),
                            None => path.anchors().map(|(s, i, _)| (s, i)).collect(),
                        };
                        let remove = hits.iter().find(|(h, _)| h == id).map(|(_, v)| v.clone()).unwrap_or_default();
                        let rest: BTreeSet<AnchorRef> = cur.difference(&remove).copied().collect();
                        if !rest.is_empty() {
                            items.push(json!({ "id": id.0, "anchors": anchors_json(&rest) }));
                        }
                    }
                    return vec![Action::Exec("select.anchorsMany".into(), json!({ "items": items, "add": false }))];
                }
                let items: Vec<Value> = hits.iter().map(|(id, v)| json!({ "id": id.0, "anchors": anchors_json(v) })).collect();
                vec![Action::Exec("select.anchorsMany".into(), json!({ "items": items, "add": add }))]
            }
            _ => vec![],
        }
    }

    fn key(&mut self, _cx: &ToolContext, key: ToolKey, _mods: Mods) -> Vec<Action> {
        if key == ToolKey::Escape {
            self.active = false;
            self.points.clear();
        }
        vec![]
    }

    fn overlays(&self, _cx: &ToolContext) -> Vec<Overlay> {
        if !self.active || self.points.len() < 2 {
            return vec![];
        }
        vec![Overlay::Path { path: super::polygon(&self.points, false), color: [0x40, 0x40, 0x40], width: 1.0, dashed: true }]
    }

    fn cursor(&self, _cx: &ToolContext, _p: Point, _m: Mods) -> Cursor {
        Cursor::Crosshair
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::*;
    use drawcraft_color::Color;
    use drawcraft_doc::{Appearance, Node, Selection};
    use drawcraft_geom::{Rect, shapes};

    fn add_rect(d: &mut Document, r: Rect, fill: Color) -> NodeId {
        let l = d.layers[0].id;
        let id = d.alloc_id();
        d.insert(Some(l), usize::MAX, Node::path(id, shapes::rectangle(r), Appearance::basic(Paint::solid(fill), Paint::None, 1.0))).unwrap();
        id
    }

    #[test]
    fn magic_wand_selects_similar_fills() {
        let (mut d, a) = doc_with_rect(); // white fill
        let b = add_rect(&mut d, Rect::new(300.0, 300.0, 350.0, 350.0), Color::rgb8(250, 250, 250));
        let c = add_rect(&mut d, Rect::new(400.0, 400.0, 450.0, 450.0), Color::rgb8(255, 0, 0));
        let s = Selection::default();
        let p = paint();
        let cx = cx(&d, &s, &p);
        let mut t = MagicWandTool::default();
        let r = t.pointer(&cx, &PointerEvent::new(PointerKind::Down, 150.0, 150.0));
        assert_eq!(r, vec![Action::Exec("select.set".into(), json!({"ids": [a.0, b.0]}))]);
        t.set_option("tolerance", &json!(0));
        let r = t.pointer(&cx, &PointerEvent::new(PointerKind::Down, 150.0, 150.0));
        assert_eq!(r, vec![Action::Exec("select.set".into(), json!({"ids": [a.0]}))]);
        let shift = Mods { shift: true, ..Default::default() };
        let r = t.pointer(&cx, &PointerEvent::new(PointerKind::Down, 420.0, 420.0).with_mods(shift));
        assert_eq!(r, vec![Action::Exec("select.add".into(), json!({"ids": [c.0]}))]);
    }

    #[test]
    fn lasso_selects_anchors_inside() {
        let (d, id) = doc_with_rect();
        let s = Selection::default();
        let p = paint();
        let cx = cx(&d, &s, &p);
        let mut t = LassoTool::default();
        // A triangle around the top-left corner (100,100) only.
        t.pointer(&cx, &PointerEvent::new(PointerKind::Down, 80.0, 80.0));
        t.pointer(&cx, &PointerEvent::new(PointerKind::Drag, 140.0, 80.0));
        t.pointer(&cx, &PointerEvent::new(PointerKind::Drag, 80.0, 140.0));
        assert_eq!(t.overlays(&cx).len(), 1);
        let a = t.pointer(&cx, &PointerEvent::new(PointerKind::Up, 80.0, 140.0));
        let Action::Exec(c, v) = &a[0] else { panic!() };
        assert_eq!(c, "select.anchorsMany");
        assert_eq!(v["items"][0]["id"], id.0);
        assert_eq!(v["items"][0]["anchors"].as_array().unwrap().len(), 1);
        assert_eq!(v["add"], false);
        assert!(point_in_polygon(&[Point::new(0.0, 0.0), Point::new(10.0, 0.0), Point::new(0.0, 10.0)], Point::new(2.0, 2.0)));
    }
}
