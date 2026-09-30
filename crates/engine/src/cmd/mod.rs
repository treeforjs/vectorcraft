//! The command registry. Ids follow Illustrator's menu structure.

mod create;
mod draw2;
mod effectcmd;
mod xform;
mod edit;
mod layer;
mod object;
mod paint;
mod path;
mod pathops;
mod select;
mod typecmd;

use drawcraft_color::Color;
use drawcraft_doc::NodeId;
use drawcraft_geom::{Affine, Point};
use serde::Serialize;
use serde_json::Value;

use crate::{EngineError, Result, Session};

pub type Run = fn(&mut Session, &Value) -> Result<Value>;
pub type Enabled = fn(&Session) -> std::result::Result<(), String>;

/// Metadata + implementation for one command.
pub struct CommandSpec {
    pub id: &'static str,
    pub label: &'static str,
    /// Menu placement, e.g. `["Object", "Arrange"]`. Empty = not in menus.
    pub menu: &'static [&'static str],
    /// Default shortcut (`Cmd+Shift+]`), mapped per platform by the UI.
    pub shortcut: Option<&'static str>,
    /// Human/agent-readable parameter description.
    pub params: &'static str,
    pub enabled: Enabled,
    pub run: Run,
    /// Record in the journal (false for queries and selection-only helpers).
    pub journal: bool,
}

/// Serializable command metadata.
#[derive(Clone, Debug, Serialize)]
pub struct CommandInfo {
    pub id: &'static str,
    pub label: &'static str,
    pub menu: Vec<&'static str>,
    pub shortcut: Option<&'static str>,
    pub params: &'static str,
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disabled_reason: Option<String>,
}

impl CommandSpec {
    pub fn info(&self, s: &Session) -> CommandInfo {
        let e = (self.enabled)(s);
        CommandInfo {
            id: self.id,
            label: self.label,
            menu: self.menu.to_vec(),
            shortcut: self.shortcut,
            params: self.params,
            enabled: e.is_ok(),
            disabled_reason: e.err(),
        }
    }
}

// ---------- enablement predicates ----------

pub fn always(_: &Session) -> std::result::Result<(), String> {
    Ok(())
}
pub fn has_doc(s: &Session) -> std::result::Result<(), String> {
    s.active().map(|_| ()).ok_or_else(|| "no document open".into())
}
pub fn has_selection(s: &Session) -> std::result::Result<(), String> {
    has_doc(s)?;
    if s.active().unwrap().selection.is_empty() { Err("nothing selected".into()) } else { Ok(()) }
}
pub fn has_multi(s: &Session) -> std::result::Result<(), String> {
    has_doc(s)?;
    if s.active().unwrap().selection.len() < 2 { Err("select at least two objects".into()) } else { Ok(()) }
}
pub fn can_undo(s: &Session) -> std::result::Result<(), String> {
    s.active().filter(|d| !d.history.undo.is_empty()).map(|_| ()).ok_or_else(|| "nothing to undo".into())
}
pub fn can_redo(s: &Session) -> std::result::Result<(), String> {
    s.active().filter(|d| !d.history.redo.is_empty()).map(|_| ()).ok_or_else(|| "nothing to redo".into())
}
pub fn has_clipboard(s: &Session) -> std::result::Result<(), String> {
    has_doc(s)?;
    if s.clipboard.is_empty() { Err("clipboard is empty".into()) } else { Ok(()) }
}

macro_rules! cmd {
    ($id:literal, $label:literal, [$($m:literal),*], $sc:expr, $params:literal, $en:expr, $run:expr) => {
        $crate::cmd::CommandSpec { id: $id, label: $label, menu: &[$($m),*], shortcut: $sc, params: $params, enabled: $en, run: $run, journal: true }
    };
    (query $id:literal, $label:literal, [$($m:literal),*], $sc:expr, $params:literal, $en:expr, $run:expr) => {
        $crate::cmd::CommandSpec { id: $id, label: $label, menu: &[$($m),*], shortcut: $sc, params: $params, enabled: $en, run: $run, journal: false }
    };
}
pub(crate) use cmd;

pub fn command_specs() -> &'static [CommandSpec] {
    static SPECS: std::sync::OnceLock<Vec<CommandSpec>> = std::sync::OnceLock::new();
    SPECS.get_or_init(|| {
        let mut v = Vec::new();
        v.extend(edit::specs());
        v.extend(create::specs());
        v.extend(object::specs());
        v.extend(path::specs());
        v.extend(select::specs());
        v.extend(paint::specs());
        v.extend(layer::specs());
        v.extend(draw2::specs());
        v.extend(xform::specs());
        v.extend(effectcmd::specs());
        v.extend(pathops::specs());
        v.extend(typecmd::specs());
        v
    })
}

pub fn find_command(id: &str) -> Option<&'static CommandSpec> {
    command_specs().iter().find(|c| c.id == id)
}

// ---------- param helpers ----------

pub(crate) fn bad(cmd: &str, msg: impl Into<String>) -> EngineError {
    EngineError::BadParams { cmd: cmd.into(), msg: msg.into() }
}
pub(crate) fn f64_or(p: &Value, key: &str, default: f64) -> f64 {
    p.get(key).and_then(Value::as_f64).unwrap_or(default)
}
pub(crate) fn f64_req(p: &Value, key: &str, cmd: &str) -> Result<f64> {
    p.get(key).and_then(Value::as_f64).ok_or_else(|| bad(cmd, format!("missing number `{key}`")))
}
pub(crate) fn bool_or(p: &Value, key: &str, default: bool) -> bool {
    p.get(key).and_then(Value::as_bool).unwrap_or(default)
}
pub(crate) fn str_param<'a>(p: &'a Value, key: &str) -> Option<&'a str> {
    p.get(key).and_then(Value::as_str)
}
pub(crate) fn id_param(p: &Value, key: &str) -> Option<NodeId> {
    p.get(key).and_then(Value::as_u64).map(NodeId)
}
pub(crate) fn ids_param(p: &Value, key: &str) -> Option<Vec<NodeId>> {
    p.get(key).and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_u64).map(NodeId).collect())
}
pub(crate) fn point_param(p: &Value, key: &str) -> Option<Point> {
    let a = p.get(key)?.as_array()?;
    Some(Point::new(a.first()?.as_f64()?, a.get(1)?.as_f64()?))
}
/// `[a, b, c, d, e, f]` affine coefficients.
pub fn matrix_param(p: &Value, key: &str) -> Option<Affine> {
    let a = p.get(key)?.as_array()?;
    if a.len() != 6 {
        return None;
    }
    let mut c = [0.0; 6];
    for (i, v) in a.iter().enumerate() {
        c[i] = v.as_f64()?;
    }
    Some(Affine::new(c))
}
/// A colour from `"#rrggbb"`, `[r,g,b]` (0..1), `{"c":..,"m":..,"y":..,"k":..}` or `{"gray":..}`.
pub(crate) fn color_value(v: &Value) -> Option<Color> {
    match v {
        Value::String(s) => Color::from_hex(s),
        Value::Array(a) if a.len() >= 3 => Some(Color::rgb(a[0].as_f64()? as f32, a[1].as_f64()? as f32, a[2].as_f64()? as f32)),
        Value::Object(o) => {
            if let (Some(c), Some(m), Some(y), Some(k)) = (o.get("c"), o.get("m"), o.get("y"), o.get("k")) {
                let f = |v: &Value| v.as_f64().map(|x| if x > 1.0 { x / 100.0 } else { x } as f32);
                return Some(Color::cmyk(f(c)?, f(m)?, f(y)?, f(k)?));
            }
            if let Some(g) = o.get("gray") {
                let g = g.as_f64()?;
                return Some(Color::gray(if g > 1.0 { g / 100.0 } else { g } as f32));
            }
            serde_json::from_value(v.clone()).ok()
        }
        _ => None,
    }
}

/// Objects a command targets: explicit `ids` param or the selection.
pub(crate) fn targets(s: &Session, p: &Value) -> Result<Vec<NodeId>> {
    if let Some(ids) = ids_param(p, "ids") {
        return Ok(ids);
    }
    if let Some(id) = id_param(p, "id") {
        return Ok(vec![id]);
    }
    Ok(s.doc()?.selection.objects.clone())
}

pub(crate) fn ok() -> Result<Value> {
    Ok(Value::Null)
}
