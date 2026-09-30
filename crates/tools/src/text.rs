//! The Type tool (T): click places point type, drag draws an area-type frame, clicking into
//! existing text places the caret. Typing is coalesced into one undo step per editing session.

use drawcraft_doc::hit::hit_test;
use drawcraft_doc::{NodeId, NodeKind};
use drawcraft_geom::{Point, Rect};
use serde_json::json;

use crate::{Action, Cursor, Mods, Overlay, PointerEvent, PointerKind, Tool, ToolContext, ToolKey};

#[derive(Default)]
pub struct TypeTool {
    /// Text object being edited and the caret (byte offset).
    editing: Option<(NodeId, usize)>,
    /// Current text of the edited object (mirrors the preview).
    buffer: String,
    typing: bool,
    press: Option<Point>,
    drag: Option<Point>,
}

impl TypeTool {
    fn text_of(cx: &ToolContext, id: NodeId) -> Option<String> {
        match &cx.doc.node(id)?.kind {
            NodeKind::Text(t) => Some(t.plain_text()),
            _ => None,
        }
    }
    fn finish(&mut self) -> Vec<Action> {
        let mut out = vec![];
        if self.typing {
            out.push(Action::Commit);
        }
        self.typing = false;
        self.editing = None;
        out
    }
    fn edit(&mut self, new_text: String, caret: usize) -> Vec<Action> {
        let Some((id, _)) = self.editing else { return vec![] };
        let mut out = vec![];
        if !self.typing {
            out.push(Action::Begin("Typing".into()));
            self.typing = true;
        }
        self.buffer = new_text;
        self.editing = Some((id, caret));
        out.push(Action::Preview("text.setText".into(), json!({"id": id.0, "text": self.buffer})));
        out
    }
}

impl Tool for TypeTool {
    fn id(&self) -> &'static str {
        "type"
    }
    fn busy(&self) -> bool {
        self.press.is_some()
    }
    fn wants_text(&self) -> bool {
        self.editing.is_some()
    }
    fn pointer(&mut self, cx: &ToolContext, ev: &PointerEvent) -> Vec<Action> {
        match ev.kind {
            PointerKind::Down => {
                self.press = Some(ev.pos);
                self.drag = None;
                vec![]
            }
            PointerKind::Drag => {
                if self.press.is_some() {
                    self.drag = Some(ev.pos);
                }
                vec![]
            }
            PointerKind::Up => {
                let Some(start) = self.press.take() else { return vec![] };
                let mut out = self.finish();
                // Click into existing text: place the caret.
                if let Some(h) = hit_test(cx.doc, start, cx.hit_options())
                    && let Some(NodeKind::Text(t)) = cx.doc.node(h.leaf).map(|n| &n.kind)
                {
                    let lay = drawcraft_text::layout(drawcraft_text::FontDb::global(), t);
                    let local = t.xf.inverse() * start;
                    let byte = drawcraft_text::hit_byte(&lay, local);
                    self.editing = Some((h.leaf, byte));
                    self.buffer = t.plain_text();
                    out.push(Action::Exec("select.set".into(), json!({"ids": [h.leaf.0]})));
                    return out;
                }
                let area = self.drag.take().map(|d| Rect::from_points(start, d)).filter(|r| r.width() > cx.tol(6.0) && r.height() > cx.tol(6.0));
                let params = match area {
                    Some(r) => json!({"x": r.x0, "y": r.y0 + 12.0, "text": "", "area": {"width": r.width(), "height": r.height()}}),
                    None => json!({"x": start.x, "y": start.y, "text": ""}),
                };
                out.push(Action::Exec("text.create".into(), params));
                out.push(Action::Notify("text.editNew".into()));
                out
            }
            _ => vec![],
        }
    }
    fn text_input(&mut self, cx: &ToolContext, s: &str) -> Vec<Action> {
        let Some((id, caret)) = self.editing else { return vec![] };
        if !self.typing {
            self.buffer = Self::text_of(cx, id).unwrap_or_default();
        }
        let caret = caret.min(self.buffer.len());
        let mut t = self.buffer.clone();
        t.insert_str(caret, s);
        self.edit(t, caret + s.len())
    }
    fn key(&mut self, cx: &ToolContext, key: ToolKey, _mods: Mods) -> Vec<Action> {
        let Some((id, caret)) = self.editing else { return vec![] };
        if !self.typing {
            self.buffer = Self::text_of(cx, id).unwrap_or_default();
        }
        let caret = caret.min(self.buffer.len());
        match key {
            ToolKey::Escape => {
                let out = self.finish();
                self.editing = None;
                out
            }
            ToolKey::Enter => self.text_input(cx, "\n"),
            ToolKey::Backspace if caret > 0 => {
                let prev = self.buffer[..caret].char_indices().last().map(|(i, _)| i).unwrap_or(0);
                let mut t = self.buffer.clone();
                t.replace_range(prev..caret, "");
                self.edit(t, prev)
            }
            ToolKey::Delete if caret < self.buffer.len() => {
                let next = self.buffer[caret..].char_indices().nth(1).map(|(i, _)| caret + i).unwrap_or(self.buffer.len());
                let mut t = self.buffer.clone();
                t.replace_range(caret..next, "");
                self.edit(t, caret)
            }
            ToolKey::Left => {
                let prev = self.buffer[..caret].char_indices().last().map(|(i, _)| i).unwrap_or(0);
                self.editing = Some((id, prev));
                vec![]
            }
            ToolKey::Right => {
                let next = self.buffer[caret..].char_indices().nth(1).map(|(i, _)| caret + i).unwrap_or(self.buffer.len());
                self.editing = Some((id, next));
                vec![]
            }
            _ => vec![],
        }
    }
    fn deactivate(&mut self, _cx: &ToolContext) -> Vec<Action> {
        self.finish()
    }
    /// After `text.create` the engine selects the new object; start editing it.
    fn notify(&mut self, cx: &ToolContext, what: &str) {
        if what == "text.editNew"
            && let Some(id) = cx.selection.objects.first().copied()
        {
            self.editing = Some((id, 0));
            self.buffer.clear();
        }
    }
    fn overlays(&self, cx: &ToolContext) -> Vec<Overlay> {
        let mut o = vec![];
        if let (Some(s), Some(d)) = (self.press, self.drag) {
            o.push(Overlay::Marquee(Rect::from_points(s, d)));
        }
        if let Some((id, caret)) = self.editing
            && let Some(NodeKind::Text(t)) = cx.doc.node(id).map(|n| &n.kind)
        {
            let lay = drawcraft_text::layout(drawcraft_text::FontDb::global(), t);
            let (a, b) = drawcraft_text::caret_position(&lay, caret.min(t.plain_text().len()));
            o.push(Overlay::Line { a: t.xf * a, b: t.xf * b, color: [0, 0, 0], dashed: false });
        }
        o
    }
    fn cursor(&self, _cx: &ToolContext, _p: Point, _m: Mods) -> Cursor {
        Cursor::Text
    }
}
