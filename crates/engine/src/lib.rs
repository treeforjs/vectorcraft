//! The DrawCraft engine façade.
//!
//! Every user-visible action is a command with a stable id (`object.group`, `select.same.fillColor`,
//! `shape.rectangle`…) and JSON parameters. The egui UI, the CLI, the control channel and the MCP
//! server all go through [`Session::execute`]. Tools (pointer gestures) are hosted here too and
//! reduce to commands, so every gesture is journaled and replayable.
#![forbid(unsafe_code)]

pub mod cmd;
pub mod inspect;
mod tooling;

use std::sync::Arc;

use drawcraft_color::{Color, Paint};
use drawcraft_doc::{Document, NodeId, Selection};
use drawcraft_geom::Affine;
use drawcraft_tools::{PaintDefaults, Tool};
use serde_json::Value;

pub use cmd::{CommandInfo, CommandSpec, command_specs, find_command};
pub use drawcraft_doc as doc;
pub use drawcraft_tools as tools;
pub use tooling::{UiRequest, ViewInfo};

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("unknown command `{0}`")]
    UnknownCommand(String),
    #[error("command `{0}` is not available right now: {1}")]
    Disabled(String, String),
    #[error("invalid parameters for `{cmd}`: {msg}")]
    BadParams { cmd: String, msg: String },
    #[error("no active document")]
    NoDocument,
    #[error("no such object {0}")]
    NoNode(NodeId),
    #[error("{0}")]
    Other(String),
}

impl From<drawcraft_doc::DocError> for EngineError {
    fn from(e: drawcraft_doc::DocError) -> Self {
        EngineError::Other(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, EngineError>;

/// One undo step.
#[derive(Clone, Debug)]
pub struct HistoryEntry {
    pub label: String,
    pub doc: Arc<Document>,
    pub selection: Selection,
}

#[derive(Clone, Debug, Default)]
pub struct History {
    pub undo: Vec<HistoryEntry>,
    pub redo: Vec<HistoryEntry>,
    /// Maximum undo depth (Illustrator has no hard limit; snapshots are cheap here).
    pub limit: usize,
}

/// An in-progress drag: the snapshot the previews are applied to.
#[derive(Clone, Debug)]
pub struct Interaction {
    pub label: String,
    pub doc: Arc<Document>,
    pub selection: Selection,
    pub preview: Option<(String, Value)>,
    /// Per-document state restored on cancel (current layer, isolation).
    pub active_layer: Option<NodeId>,
    pub isolation: Option<NodeId>,
}

/// Per-document editing state.
#[derive(Clone, Debug)]
pub struct DocState {
    pub doc: Arc<Document>,
    pub selection: Selection,
    pub history: History,
    pub path: Option<String>,
    /// Increments on every change; UIs re-render when it moves.
    pub revision: u64,
    pub saved_revision: u64,
    /// The layer new art goes into (the "current layer" in the Layers panel).
    pub active_layer: Option<NodeId>,
    /// Isolation mode container.
    pub isolation: Option<NodeId>,
    pub interaction: Option<Interaction>,
    /// For Object → Transform → Transform Again (⌘D).
    pub last_transform: Option<(Affine, bool)>,
    /// Selection saved by Select → Reselect.
    pub last_selection_cmd: Option<(String, Value)>,
}

impl DocState {
    pub fn new(doc: Document, path: Option<String>) -> Self {
        let active_layer = doc.default_layer();
        Self {
            doc: Arc::new(doc),
            selection: Selection::default(),
            history: History { limit: 500, ..Default::default() },
            path,
            revision: 1,
            saved_revision: 1,
            active_layer,
            isolation: None,
            interaction: None,
            last_transform: None,
            last_selection_cmd: None,
        }
    }
    pub fn is_dirty(&self) -> bool {
        self.revision != self.saved_revision
    }
    pub fn title(&self) -> String {
        self.path
            .as_deref()
            .and_then(|p| std::path::Path::new(p).file_name())
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| self.doc.title.clone())
    }
    /// Where new art is inserted: the isolation container, else the active layer.
    pub fn insertion_parent(&self) -> Option<NodeId> {
        if let Some(i) = self.isolation
            && self.doc.node(i).is_some()
        {
            return Some(i);
        }
        self.active_layer.filter(|l| self.doc.node(*l).is_some_and(|n| !n.locked)).or_else(|| self.doc.default_layer())
    }
}

/// Coordinates beyond this (points) are rejected: ~1,400 m, far past Illustrator's large canvas.
pub const MAX_COORD: f64 = 4.0e6;

/// Cheap sanity check after an edit: artboards and the objects just touched (the selection) must
/// have finite, in-range geometry, so saved files always reload and renderers never see NaN/∞.
fn doc_sane(d: &Document, sel: &Selection) -> bool {
    let ok = |r: drawcraft_geom::Rect| [r.x0, r.y0, r.x1, r.y1].iter().all(|v| v.is_finite() && v.abs() <= MAX_COORD);
    d.artboards.iter().all(|a| ok(a.rect)) && sel.objects.iter().all(|id| d.node(*id).and_then(|n| n.geometric_bounds()).is_none_or(ok))
}

/// Where new art goes (Illustrator's drawing modes, Shift+D cycles).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DrawMode {
    #[default]
    Normal,
    Behind,
    Inside,
}

/// Application preferences that affect commands.
#[derive(Clone, Debug)]
pub struct Prefs {
    /// Arrow-key nudge distance (General → Keyboard Increment), points.
    pub keyboard_increment: f64,
    /// Scale Strokes & Effects.
    pub scale_strokes: bool,
    /// Offset for Paste / duplicate (Illustrator pastes to the view centre; we offset by this).
    pub paste_offset: f64,
    pub corner_radius: f64,
}

impl Default for Prefs {
    fn default() -> Self {
        Self { keyboard_increment: 1.0, scale_strokes: true, paste_offset: 10.0, corner_radius: 12.0 }
    }
}

pub struct Session {
    docs: Vec<DocState>,
    active: Option<usize>,
    pub prefs: Prefs,
    /// Fill/stroke for new art (the toolbar proxy).
    pub paint: PaintDefaults,
    /// Which proxy is in front (true = Fill, false = Stroke) — the X key toggles.
    pub fill_active: bool,
    /// Internal clipboard (serialized nodes).
    pub clipboard: Vec<drawcraft_doc::Node>,
    /// Executed commands (for actions and debugging).
    pub journal: Vec<(String, Value)>,
    pub(crate) tool: Box<dyn Tool>,
    pub(crate) last_view: ViewInfo,
    depth: u32,
    /// Draw Normal / Behind / Inside (toolbar drawing modes).
    pub draw_mode: DrawMode,
    /// The path new art is drawn inside (Draw Inside).
    pub draw_inside: Option<NodeId>,
    untitled_counter: u32,
    /// Session-level state of the menu commands (saved selections, guide lock).
    pub(crate) menu: cmd::menucmds::MenuState,
}

impl Default for Session {
    fn default() -> Self {
        Self::new()
    }
}

impl Session {
    pub fn new() -> Self {
        Self {
            docs: vec![],
            active: None,
            prefs: Prefs::default(),
            paint: PaintDefaults { fill: Paint::solid(Color::WHITE), stroke: Paint::solid(Color::BLACK), stroke_width: 1.0 },
            fill_active: true,
            clipboard: vec![],
            journal: vec![],
            tool: drawcraft_tools::create("selection"),
            last_view: ViewInfo::default(),
            depth: 0,
            draw_mode: DrawMode::Normal,
            draw_inside: None,
            untitled_counter: 0,
            menu: Default::default(),
        }
    }

    pub fn documents(&self) -> &[DocState] {
        &self.docs
    }
    pub fn active_index(&self) -> Option<usize> {
        self.active
    }
    pub fn active(&self) -> Option<&DocState> {
        self.active.and_then(|i| self.docs.get(i))
    }
    pub fn active_mut(&mut self) -> Option<&mut DocState> {
        self.active.and_then(|i| self.docs.get_mut(i))
    }
    pub fn doc(&self) -> Result<&DocState> {
        self.active().ok_or(EngineError::NoDocument)
    }
    pub fn doc_mut(&mut self) -> Result<&mut DocState> {
        self.active_mut().ok_or(EngineError::NoDocument)
    }
    pub fn set_active(&mut self, index: usize) -> bool {
        if index < self.docs.len() {
            self.active = Some(index);
            true
        } else {
            false
        }
    }
    pub fn next_untitled(&mut self) -> String {
        self.untitled_counter += 1;
        format!("Untitled-{}", self.untitled_counter)
    }
    /// Add a document and make it active.
    pub fn add_document(&mut self, doc: Document, path: Option<String>) -> usize {
        self.docs.push(DocState::new(doc, path));
        let i = self.docs.len() - 1;
        self.active = Some(i);
        i
    }
    pub fn close_document(&mut self, index: usize) -> bool {
        if index >= self.docs.len() {
            return false;
        }
        self.docs.remove(index);
        self.active = if self.docs.is_empty() { None } else { Some(index.min(self.docs.len() - 1)) };
        true
    }

    /// Execute a command by id. This is THE entry point for every frontend.
    pub fn execute(&mut self, id: &str, params: &Value) -> Result<Value> {
        let spec = find_command(id).ok_or_else(|| EngineError::UnknownCommand(id.to_string()))?;
        if let Err(why) = (spec.enabled)(self) {
            return Err(EngineError::Disabled(id.to_string(), why));
        }
        // Only top-level commands are journaled (commands that call other commands would otherwise
        // be recorded twice and replay differently).
        self.depth += 1;
        let r = (spec.run)(self, params);
        self.depth -= 1;
        let r = r?;
        if spec.journal && self.depth == 0 && self.active().is_none_or(|d| d.interaction.is_none()) {
            self.journal.push((id.to_string(), params.clone()));
        }
        Ok(r)
    }

    /// Run `f` on a mutable copy of the active document. Outside an interaction this records one
    /// undo step labelled `label`; inside one it just mutates (the interaction commits later).
    pub fn edit<T>(&mut self, label: &str, f: impl FnOnce(&mut Document, &mut Selection) -> Result<T>) -> Result<T> {
        let st = self.doc_mut()?;
        let before = st.doc.clone();
        let before_sel = st.selection.clone();
        let doc = Arc::make_mut(&mut st.doc);
        let result = f(doc, &mut st.selection).and_then(|v| {
            if doc_sane(&st.doc, &st.selection) {
                Ok(v)
            } else {
                Err(EngineError::Other("result would exceed the canvas (coordinates out of range)".into()))
            }
        });
        match result {
            Ok(v) => {
                st.selection.prune(&st.doc);
                st.revision += 1;
                if st.interaction.is_none() {
                    st.history.undo.push(HistoryEntry { label: label.to_string(), doc: before, selection: before_sel });
                    if st.history.undo.len() > st.history.limit {
                        st.history.undo.remove(0);
                    }
                    st.history.redo.clear();
                }
                Ok(v)
            }
            Err(e) => {
                st.doc = before;
                st.selection = before_sel;
                Err(e)
            }
        }
    }

    /// Change only the selection (not an undo step).
    pub fn select(&mut self, f: impl FnOnce(&Document, &mut Selection)) -> Result<()> {
        let st = self.doc_mut()?;
        f(&st.doc, &mut st.selection);
        st.selection.prune(&st.doc);
        st.revision += 1;
        Ok(())
    }

    // ---------- interactions (live drags) ----------

    pub fn begin_interaction(&mut self, label: &str) -> Result<()> {
        let st = self.doc_mut()?;
        if st.interaction.is_some() {
            return Ok(());
        }
        st.interaction = Some(Interaction {
            label: label.to_string(),
            doc: st.doc.clone(),
            selection: st.selection.clone(),
            preview: None,
            active_layer: st.active_layer,
            isolation: st.isolation,
        });
        Ok(())
    }

    /// Re-apply `cmd` on top of the interaction snapshot (replacing the previous preview).
    pub fn preview(&mut self, cmd: &str, params: &Value) -> Result<Value> {
        {
            let st = self.doc_mut()?;
            let Some(it) = &st.interaction else { return Err(EngineError::Other("no interaction in progress".into())) };
            st.doc = it.doc.clone();
            st.selection = it.selection.clone();
        }
        let r = self.execute(cmd, params);
        let st = self.doc_mut()?;
        if let Some(it) = &mut st.interaction {
            it.preview = Some((cmd.to_string(), params.clone()));
        }
        r
    }

    pub fn commit_interaction(&mut self) -> Result<()> {
        let st = self.doc_mut()?;
        let Some(it) = st.interaction.take() else { return Ok(()) };
        let Some(preview) = it.preview else { return Ok(()) };
        if !Arc::ptr_eq(&it.doc, &st.doc) {
            st.history.undo.push(HistoryEntry { label: it.label, doc: it.doc, selection: it.selection });
            st.history.redo.clear();
            st.revision += 1;
        }
        if preview.0 == "object.transform" {
            let m = cmd::matrix_param(&preview.1, "matrix");
            let copy = preview.1.get("copy").and_then(Value::as_bool).unwrap_or(false);
            if let Some(m) = m {
                st.last_transform = Some((m, copy));
            }
        }
        self.journal.push(preview);
        Ok(())
    }

    pub fn cancel_interaction(&mut self) -> Result<()> {
        let st = self.doc_mut()?;
        if let Some(it) = st.interaction.take() {
            st.doc = it.doc;
            st.selection = it.selection;
            st.active_layer = it.active_layer;
            st.isolation = it.isolation;
            st.revision += 1;
        }
        Ok(())
    }

    pub fn in_interaction(&self) -> bool {
        self.active().is_some_and(|d| d.interaction.is_some())
    }

    /// Commands with enablement (for menus, palette, MCP `list_commands`).
    pub fn commands(&self) -> Vec<CommandInfo> {
        command_specs().iter().map(|c| c.info(self)).collect()
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_brushsym;
#[cfg(test)]
mod tests_build;
#[cfg(test)]
mod tests_draw2;
#[cfg(test)]
mod tests_file;
#[cfg(test)]
mod tests_live;
#[cfg(test)]
mod tests_menucmds;
#[cfg(test)]
mod tests_panelcmds;
#[cfg(test)]
mod tests_pathops;
#[cfg(test)]
mod tests_textedit;
#[cfg(test)]
mod tests_xform;
