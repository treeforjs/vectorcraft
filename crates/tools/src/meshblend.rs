//! Blend tool (W) and Mesh tool (U).
//!
//! - Blend: click object A, then object B → `object.blend.make {ids: [A, B]}`. Double-click opens
//!   Blend Options. Clicking empty canvas resets.
//! - Mesh: click inside a filled path → `object.mesh.create {ids, at}` (1×1 mesh plus lines through
//!   the click); click inside a mesh → `object.mesh.addLine` (new point takes the current fill
//!   colour unless Shift); drag a mesh point → `object.mesh.movePoint` previews; Alt-click a mesh
//!   point → `object.mesh.deletePoint`.

use drawcraft_color::Paint;
use drawcraft_doc::{NodeId, NodeKind};
use drawcraft_geom::Point;
use serde_json::json;

use crate::{Action, Cursor, Mods, Overlay, PointerEvent, PointerKind, Tool, ToolContext};

const FEEDBACK: [u8; 3] = [0x4f, 0x9d, 0xff];

pub fn create(id: &str) -> Option<Box<dyn Tool>> {
    match id {
        "blend" => Some(Box::new(BlendTool::default())),
        "mesh" => Some(Box::new(MeshTool::default())),
        _ => None,
    }
}

fn hit_top(cx: &ToolContext, p: Point) -> Option<(NodeId, NodeId)> {
    let h = drawcraft_doc::hit::hit_test(cx.doc, p, cx.hit_options())?;
    Some((h.top_object(cx.isolation), h.leaf))
}

#[derive(Default)]
pub struct BlendTool {
    first: Option<NodeId>,
}

impl Tool for BlendTool {
    fn id(&self) -> &'static str {
        "blend"
    }
    fn pointer(&mut self, cx: &ToolContext, ev: &PointerEvent) -> Vec<Action> {
        match ev.kind {
            PointerKind::DoubleClick => {
                self.first = None;
                vec![Action::Dialog("object.blend.options".into(), json!({}))]
            }
            PointerKind::Down => match (self.first, hit_top(cx, ev.pos)) {
                (_, None) => {
                    self.first = None;
                    vec![]
                }
                (None, Some((top, _))) => {
                    self.first = Some(top);
                    vec![Action::Exec("select.set".into(), json!({"ids": [top.0]}))]
                }
                (Some(a), Some((b, _))) if a != b && cx.doc.node(a).is_some() => {
                    self.first = None;
                    vec![Action::Exec("object.blend.make".into(), json!({"ids": [a.0, b.0]}))]
                }
                (Some(_), Some(_)) => vec![],
            },
            _ => vec![],
        }
    }
    fn overlays(&self, cx: &ToolContext) -> Vec<Overlay> {
        let Some(b) = self.first.and_then(|a| cx.doc.node(a)).and_then(|n| n.geometric_bounds()) else { return vec![] };
        vec![Overlay::Anchor { p: b.center(), color: FEEDBACK, filled: true, size: 6.0 }]
    }
    fn cursor(&self, _cx: &ToolContext, _p: Point, _mods: Mods) -> Cursor {
        Cursor::Crosshair
    }
    fn busy(&self) -> bool {
        self.first.is_some()
    }
    fn deactivate(&mut self, _cx: &ToolContext) -> Vec<Action> {
        self.first = None;
        vec![]
    }
}

#[derive(Default)]
pub struct MeshTool {
    drag: Option<(NodeId, usize)>,
}

impl MeshTool {
    /// Meshes to consider for point hits: selected ones first, then any mesh in the document.
    fn meshes(cx: &ToolContext) -> Vec<NodeId> {
        let mut v: Vec<NodeId> =
            cx.selection.objects.iter().copied().filter(|id| cx.doc.node(*id).is_some_and(|n| matches!(n.kind, NodeKind::Mesh(_)))).collect();
        let mut rest = vec![];
        cx.doc.walk(|n| {
            if matches!(n.kind, NodeKind::Mesh(_)) {
                rest.push(n.id);
            }
        });
        for id in rest.into_iter().rev() {
            if !v.contains(&id) {
                v.push(id);
            }
        }
        v.retain(|id| cx.doc.is_editable(*id));
        v
    }

    fn point_at(cx: &ToolContext, p: Point) -> Option<(NodeId, usize)> {
        let tol = cx.tol(5.0);
        Self::meshes(cx).into_iter().find_map(|id| match &cx.doc.node(id)?.kind {
            NodeKind::Mesh(m) => m.point_near(p, tol).map(|i| (id, i)),
            _ => None,
        })
    }
}

impl Tool for MeshTool {
    fn id(&self) -> &'static str {
        "mesh"
    }
    fn pointer(&mut self, cx: &ToolContext, ev: &PointerEvent) -> Vec<Action> {
        let p = ev.pos;
        match ev.kind {
            PointerKind::Down => {
                if let Some((id, index)) = Self::point_at(cx, p) {
                    if ev.mods.alt {
                        return vec![Action::Exec("object.mesh.deletePoint".into(), json!({"id": id.0, "index": index}))];
                    }
                    self.drag = Some((id, index));
                    return vec![Action::Begin("Move Mesh Point".into())];
                }
                let Some((top, leaf)) = hit_top(cx, p) else { return vec![] };
                let color = match (&cx.paint.fill, ev.mods.shift) {
                    (Paint::Solid { color, .. }, false) => Some(color.to_hex()),
                    _ => None,
                };
                for id in [leaf, top] {
                    match cx.doc.node(id).map(|n| &n.kind) {
                        Some(NodeKind::Mesh(_)) => {
                            let mut params = json!({"id": id.0, "x": p.x, "y": p.y});
                            if let Some(c) = &color {
                                params["color"] = json!(c);
                            }
                            return vec![Action::Exec("object.mesh.addLine".into(), params)];
                        }
                        Some(NodeKind::Path { guide: false, .. }) | Some(NodeKind::Compound { .. }) => {
                            return vec![Action::Exec("object.mesh.create".into(), json!({"ids": [id.0], "at": [p.x, p.y]}))];
                        }
                        _ => {}
                    }
                }
                vec![]
            }
            PointerKind::Drag => match self.drag {
                Some((id, index)) => vec![Action::Preview("object.mesh.movePoint".into(), json!({"id": id.0, "index": index, "x": p.x, "y": p.y}))],
                None => vec![],
            },
            PointerKind::Up => match self.drag.take() {
                Some(_) => vec![Action::Commit],
                None => vec![],
            },
            _ => vec![],
        }
    }
    fn overlays(&self, cx: &ToolContext) -> Vec<Overlay> {
        let mut out = vec![];
        for id in &cx.selection.objects {
            if let Some(NodeKind::Mesh(m)) = cx.doc.node(*id).map(|n| &n.kind) {
                out.push(Overlay::Path { path: m.lines().to_bezpath(), color: FEEDBACK, width: 1.0, dashed: false });
                for q in &m.points {
                    out.push(Overlay::Anchor { p: q.p, color: FEEDBACK, filled: false, size: 5.0 });
                }
            }
        }
        out
    }
    fn cursor(&self, cx: &ToolContext, p: Point, mods: Mods) -> Cursor {
        match Self::point_at(cx, p) {
            Some(_) if mods.alt => Cursor::PenDelete,
            Some(_) => Cursor::Move,
            None => Cursor::PenAdd,
        }
    }
    fn busy(&self) -> bool {
        self.drag.is_some()
    }
    fn deactivate(&mut self, _cx: &ToolContext) -> Vec<Action> {
        if self.drag.take().is_some() { vec![Action::Commit] } else { vec![] }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::*;
    use drawcraft_doc::Selection;

    #[test]
    fn blend_tool_two_clicks_make_blend() {
        let (mut d, a) = doc_with_rect();
        let l = d.layers[0].id;
        let b = d.alloc_id();
        let r = drawcraft_geom::shapes::rectangle(drawcraft_geom::Rect::new(300.0, 100.0, 350.0, 150.0));
        d.insert(Some(l), 1, drawcraft_doc::Node::path(b, r, drawcraft_doc::Appearance::default_art())).unwrap();
        let (s, p) = (Selection::default(), paint());
        let c = cx(&d, &s, &p);
        let mut t = create("blend").unwrap();
        t.pointer(&c, &PointerEvent::new(PointerKind::Down, 150.0, 150.0));
        let acts = t.pointer(&c, &PointerEvent::new(PointerKind::Down, 320.0, 120.0));
        assert_eq!(acts, vec![Action::Exec("object.blend.make".into(), json!({"ids": [a.0, b.0]}))]);
    }

    #[test]
    fn mesh_tool_click_path_creates_mesh() {
        let (d, a) = doc_with_rect();
        let (s, p) = (Selection::default(), paint());
        let c = cx(&d, &s, &p);
        let mut t = create("mesh").unwrap();
        let acts = t.pointer(&c, &PointerEvent::new(PointerKind::Down, 150.0, 160.0));
        assert_eq!(acts, vec![Action::Exec("object.mesh.create".into(), json!({"ids": [a.0], "at": [150.0, 160.0]}))]);
    }
}
