//! The Actions panel: record commands from the engine journal, play them back as one undo step.
//! Every recorded step is an ordinary command invocation, so actions are also plain JSON that
//! agents can author and replay (`command.batch`). Steps can be turned off, menu items and stops
//! inserted, and a step that opens a dialog (or a stop) pauses playback until Play continues it.
//! The `actions.*` UI commands do all of it for agents too.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::theme::Tokens;
use crate::widgets::{self, dim_label, menu_item};
use crate::{VectorcraftApp, icons};

/// The pseudo-command of an inserted stop: `{message, allowContinue}`.
pub const STOP: &str = "actions.stopStep";
/// The most sets, actions per set and steps per action the panel keeps.
const MAX_SETS: usize = 256;
const MAX_ACTIONS: usize = 1024;
const MAX_STEPS: usize = 4096;

/// One recorded or inserted step. Older preferences stored steps as `[command, params]` pairs;
/// those still load.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(from = "StepRepr")]
pub struct Step {
    pub command: String,
    pub params: Value,
    /// Turned off in the panel: playback skips it.
    pub enabled: bool,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum StepRepr {
    Pair(String, Value),
    Full {
        command: String,
        #[serde(default)]
        params: Value,
        #[serde(default = "enabled_default")]
        enabled: bool,
    },
}

fn enabled_default() -> bool {
    true
}

impl From<StepRepr> for Step {
    fn from(r: StepRepr) -> Self {
        match r {
            StepRepr::Pair(command, params) => Step { command, params, enabled: true },
            StepRepr::Full { command, params, enabled } => Step { command, params, enabled },
        }
    }
}

impl Step {
    pub fn new(command: &str, params: Value) -> Self {
        Step { command: command.to_string(), params, enabled: true }
    }
    /// What the panel shows for the step: the command's label, or the stop's message.
    pub fn title(&self) -> String {
        if self.command == STOP {
            let m = self.params.get("message").and_then(Value::as_str).unwrap_or("");
            return if m.is_empty() { tl!("Stop").into() } else { crate::i18n::fmt(tl!("Stop: {message}"), &[("message", m)]) };
        }
        let label = vectorcraft_engine::find_command(&self.command)
            .map(|c| c.label.to_string())
            .or_else(|| crate::menus::UI_COMMANDS.iter().find(|c| c.0 == self.command).map(|c| c.1.to_string()));
        label.map_or_else(|| self.command.clone(), |l| tl!(&l).to_string())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Action {
    pub name: String,
    pub steps: Vec<Step>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ActionSet {
    pub name: String,
    pub actions: Vec<Action>,
}

/// Playback paused by a stop or a dialog: Play continues at `step`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Resume {
    pub set: usize,
    pub action: usize,
    pub step: usize,
}

/// A recording in progress: into `action` of `set` (a new action named `name` when `None`) from
/// journal entry `start`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Recording {
    pub set: usize,
    pub action: Option<usize>,
    pub name: String,
    pub start: usize,
}

/// Built-in actions (our own, handy defaults).
pub fn default_sets() -> Vec<ActionSet> {
    vec![ActionSet {
        name: "Default Actions".into(),
        actions: vec![
            Action { name: "Duplicate and Offset 10 pt".into(), steps: vec![Step::new("object.move", json!({"dx": 10, "dy": 10, "copy": true}))] },
            Action {
                name: "Unite and Simplify".into(),
                steps: vec![Step::new("object.pathfinder.unite", json!({})), Step::new("object.path.simplify", json!({"tolerance": 0.5}))],
            },
            Action {
                name: "Center on Artboard".into(),
                steps: vec![Step::new("object.align", json!({"horizontal": "center", "vertical": "center", "to": "artboard"}))],
            },
            Action { name: "Outline Text".into(), steps: vec![Step::new("type.createOutlines", json!({}))] },
            Action { name: "Rotate 90° CW".into(), steps: vec![Step::new("object.rotate", json!({"angle": -90}))] },
            Action { name: "Reflect Horizontal".into(), steps: vec![Step::new("object.reflect", json!({"axis": "vertical"}))] },
        ],
    }]
}

fn action(app: &VectorcraftApp, set: usize, idx: usize) -> Result<&Action, String> {
    app.ui.action_sets.get(set).and_then(|s| s.actions.get(idx)).ok_or_else(|| "no such action".into())
}
fn action_mut(app: &mut VectorcraftApp, set: usize, idx: usize) -> Result<&mut Action, String> {
    app.ui.action_sets.get_mut(set).and_then(|s| s.actions.get_mut(idx)).ok_or_else(|| "no such action".into())
}

/// Play an action from step `from`. Runs of engine commands each run as one undoable batch; a
/// stop, or a step that leaves a dialog open, pauses playback (Play continues after it).
pub fn play_from(app: &mut VectorcraftApp, set: usize, idx: usize, from: usize) -> Result<Value, String> {
    let a = action(app, set, idx)?.clone();
    app.ui.action_resume = None;
    let mut batch: Vec<Value> = vec![];
    let flush = |app: &mut VectorcraftApp, batch: &mut Vec<Value>| -> Result<(), String> {
        if batch.is_empty() {
            return Ok(());
        }
        let commands = std::mem::take(batch);
        app.run("command.batch", json!({"label": a.name, "commands": commands})).map(|_| ())
    };
    let mut played = 0usize;
    for (i, step) in a.steps.iter().enumerate().skip(from) {
        if !step.enabled {
            continue;
        }
        played += 1;
        if step.command == STOP {
            flush(app, &mut batch)?;
            let message = step.params.get("message").and_then(Value::as_str).unwrap_or("").to_string();
            let resume = step.params.get("allowContinue").and_then(Value::as_bool).unwrap_or(true) && i + 1 < a.steps.len();
            if resume {
                app.ui.action_resume = Some(Resume { set, action: idx, step: i + 1 });
            }
            if !message.is_empty() {
                app.status(message.clone());
            }
            return Ok(json!({"stopped": message, "next": resume.then_some(i + 1)}));
        }
        if vectorcraft_engine::find_command(&step.command).is_some() {
            batch.push(json!({"command": step.command, "params": step.params}));
            continue;
        }
        flush(app, &mut batch)?;
        app.run(&step.command, step.params.clone())?;
        if app.ui.dialog.is_some() && i + 1 < a.steps.len() {
            app.ui.action_resume = Some(Resume { set, action: idx, step: i + 1 });
            return Ok(json!({"paused": step.command, "next": i + 1}));
        }
    }
    flush(app, &mut batch)?;
    Ok(json!({"played": played}))
}

/// Run an action from its first step.
pub fn play(app: &mut VectorcraftApp, set: usize, idx: usize) -> Result<Value, String> {
    play_from(app, set, idx, 0)
}

/// Finish the recording in progress: its steps go into its action.
fn stop_recording(app: &mut VectorcraftApp) -> Option<(usize, usize)> {
    let r = app.ui.recording.take()?;
    let steps: Vec<Step> = app.session.journal_for_action(r.start).into_iter().map(|(c, p)| Step::new(&c, p)).collect();
    let s = app.ui.action_sets.get_mut(r.set)?;
    match r.action.filter(|i| *i < s.actions.len()) {
        Some(i) => {
            let a = s.actions.get_mut(i)?;
            a.steps.extend(steps);
            a.steps.truncate(MAX_STEPS);
            Some((r.set, i))
        }
        None => {
            if s.actions.len() >= MAX_ACTIONS {
                return None;
            }
            s.actions.push(Action { name: r.name, steps });
            Some((r.set, s.actions.len() - 1))
        }
    }
}

fn unique(names: impl Iterator<Item = String>, base: &str) -> String {
    let names: Vec<String> = names.collect();
    (1..).map(|i| format!("{base} {i}")).find(|n| !names.contains(n)).unwrap_or_else(|| base.to_string())
}

fn index(p: &Value, key: &str) -> Option<usize> {
    p.get(key).and_then(Value::as_u64).and_then(|v| usize::try_from(v).ok())
}

/// `actions.*` UI commands.
pub fn run_command(app: &mut VectorcraftApp, id: &str, p: &Value) -> Option<Result<Value, String>> {
    if !id.starts_with("actions.") {
        return None;
    }
    let s = |k: &str| p.get(k).and_then(Value::as_str).map(str::to_string);
    let at = |app: &VectorcraftApp| -> Result<(usize, usize), String> {
        let (set, a) = (index(p, "set").unwrap_or(0), index(p, "action").ok_or("missing action")?);
        action(app, set, a).map(|_| (set, a))
    };
    let r = (|| -> Result<Value, String> {
        match id {
            "actions.list" => serde_json::to_value(&app.ui.action_sets)
                .map(|sets| json!({"sets": sets, "recording": app.ui.recording.is_some(), "paused": app.ui.action_resume.map(|r| json!([r.set, r.action, r.step]))}))
                .map_err(|e| e.to_string()),
            "actions.newSet" => {
                if app.ui.action_sets.len() >= MAX_SETS {
                    return Err("too many action sets".into());
                }
                let name = s("name").filter(|n| !n.trim().is_empty()).unwrap_or_else(|| unique(app.ui.action_sets.iter().map(|s| s.name.clone()), "Set"));
                app.ui.action_sets.push(ActionSet { name, actions: vec![] });
                Ok(json!({"set": app.ui.action_sets.len() - 1}))
            }
            "actions.newAction" => {
                let set = index(p, "set").unwrap_or(0);
                if app.ui.action_sets.is_empty() {
                    app.ui.action_sets.push(ActionSet { name: "Set 1".into(), actions: vec![] });
                }
                let st = app.ui.action_sets.get_mut(set).ok_or("no such set")?;
                if st.actions.len() >= MAX_ACTIONS {
                    return Err("too many actions in this set".into());
                }
                let name = s("name").filter(|n| !n.trim().is_empty()).unwrap_or_else(|| unique(st.actions.iter().map(|a| a.name.clone()), "Action"));
                st.actions.push(Action { name, steps: vec![] });
                let a = st.actions.len() - 1;
                if p.get("record").and_then(Value::as_bool).unwrap_or(false) {
                    let start = app.session.journal.len();
                    app.ui.recording = Some(Recording { set, action: Some(a), name: String::new(), start });
                }
                Ok(json!({"set": set, "action": a}))
            }
            "actions.record" => {
                if app.ui.recording.is_some() {
                    return Err("already recording".into());
                }
                if app.ui.action_sets.is_empty() {
                    app.ui.action_sets.push(ActionSet { name: "Set 1".into(), actions: vec![] });
                }
                let set = index(p, "set").unwrap_or(0);
                let st = app.ui.action_sets.get(set).ok_or("no such set")?;
                let target = index(p, "action");
                if target.is_some_and(|a| a >= st.actions.len()) {
                    return Err("no such action".into());
                }
                let name = s("name").unwrap_or_else(|| unique(st.actions.iter().map(|a| a.name.clone()), "Action"));
                let start = app.session.journal.len();
                app.ui.recording = Some(Recording { set, action: target, name, start });
                Ok(json!({"recording": true}))
            }
            "actions.stop" => {
                app.ui.action_resume = None;
                Ok(match stop_recording(app) {
                    Some((set, a)) => json!({"set": set, "action": a}),
                    None => Value::Null,
                })
            }
            "actions.play" => {
                let (set, a) = at(app)?;
                let from = match (index(p, "step"), app.ui.action_resume) {
                    (Some(step), _) => step,
                    (None, Some(r)) if r.set == set && r.action == a => r.step,
                    _ => 0,
                };
                play_from(app, set, a, from)
            }
            "actions.insertMenuItem" => {
                let (set, a) = at(app)?;
                let command = s("command").ok_or("missing command")?;
                let command = resolve_command(&command).ok_or_else(|| format!("no menu command `{command}`"))?;
                let params = p.get("params").cloned().unwrap_or_else(|| json!({}));
                insert(app, set, a, index(p, "at"), Step::new(&command, params))
            }
            "actions.insertStop" => {
                let (set, a) = at(app)?;
                let message = s("message").unwrap_or_default();
                let allow = p.get("allowContinue").and_then(Value::as_bool).unwrap_or(true);
                insert(app, set, a, index(p, "at"), Step::new(STOP, json!({"message": message, "allowContinue": allow})))
            }
            "actions.setStepEnabled" => {
                let (set, a) = at(app)?;
                let i = index(p, "step").ok_or("missing step")?;
                let on = p.get("enabled").and_then(Value::as_bool).ok_or("missing enabled")?;
                let step = action_mut(app, set, a)?.steps.get_mut(i).ok_or("no such step")?;
                step.enabled = on;
                Ok(json!(on))
            }
            "actions.delete" => {
                let set = index(p, "set").unwrap_or(0);
                match (index(p, "action"), index(p, "step")) {
                    (Some(a), Some(i)) => {
                        let steps = &mut action_mut(app, set, a)?.steps;
                        if i >= steps.len() {
                            return Err("no such step".into());
                        }
                        steps.remove(i);
                    }
                    (Some(a), None) => {
                        let st = app.ui.action_sets.get_mut(set).ok_or("no such set")?;
                        if a >= st.actions.len() {
                            return Err("no such action".into());
                        }
                        st.actions.remove(a);
                    }
                    _ => {
                        if set >= app.ui.action_sets.len() {
                            return Err("no such set".into());
                        }
                        app.ui.action_sets.remove(set);
                    }
                }
                app.ui.action_resume = None;
                Ok(Value::Null)
            }
            "actions.duplicate" => {
                let (set, a) = at(app)?;
                let mut copy = action(app, set, a)?.clone();
                let st = app.ui.action_sets.get_mut(set).ok_or("no such set")?;
                if st.actions.len() >= MAX_ACTIONS {
                    return Err("too many actions in this set".into());
                }
                copy.name = crate::i18n::fmt(tl!("{name} copy"), &[("name", &copy.name)]);
                st.actions.insert(a + 1, copy);
                Ok(json!({"set": set, "action": a + 1}))
            }
            "actions.rename" => {
                let to = s("to").filter(|n| !n.trim().is_empty()).ok_or("missing to")?;
                let set = index(p, "set").unwrap_or(0);
                match index(p, "action") {
                    Some(a) => action_mut(app, set, a)?.name = to,
                    None => app.ui.action_sets.get_mut(set).ok_or("no such set")?.name = to,
                }
                Ok(Value::Null)
            }
            "actions.reset" => {
                app.ui.action_sets = default_sets();
                app.ui.action_resume = None;
                app.ui.recording = None;
                Ok(Value::Null)
            }
            "actions.insertMenuItemDialog" | "actions.insertStopDialog" | "actions.newSetDialog" => {
                let kind = match id {
                    "actions.insertMenuItemDialog" => crate::dialogs::action_options::MENU_ITEM,
                    "actions.insertStopDialog" => crate::dialogs::action_options::STOP,
                    _ => crate::dialogs::action_options::NEW_SET,
                };
                if kind != crate::dialogs::action_options::NEW_SET {
                    at(app)?;
                }
                crate::dialogs::action_options::open(app, kind, index(p, "set").unwrap_or(0), index(p, "action").unwrap_or(0));
                if let (Some(at), Some(d)) = (index(p, "at"), app.ui.dialog.as_mut()) {
                    d.fields.insert("__at".into(), json!(at));
                }
                Ok(Value::Null)
            }
            _ => Err(format!("unknown command `{id}`")),
        }
    })();
    Some(r)
}

/// A command id, or a menu label (case and ellipsis ignored), as the command id it names.
pub fn resolve_command(text: &str) -> Option<String> {
    let t = text.trim();
    if t.is_empty() {
        return None;
    }
    if vectorcraft_engine::find_command(t).is_some() || crate::menus::UI_COMMANDS.iter().any(|c| c.0 == t) {
        return Some(t.to_string());
    }
    let norm = |s: &str| s.trim_end_matches('…').trim_end_matches("...").trim().to_lowercase();
    let want = norm(t);
    let labels = |c: &vectorcraft_engine::CommandSpec| {
        // "All" in the Select menu is also "Select All" and "Select > All".
        let menu = c.menu.last().copied().unwrap_or("");
        [c.label.to_string(), format!("{menu} {}", c.label), format!("{menu} > {}", c.label), tl!(c.label).to_string()]
    };
    vectorcraft_engine::command_specs()
        .iter()
        .find(|c| labels(c).iter().any(|l| norm(l) == want))
        .map(|c| c.id)
        .or_else(|| crate::menus::UI_COMMANDS.iter().find(|c| norm(c.1) == want || norm(tl!(c.1)) == want).map(|c| c.0))
        .map(str::to_string)
}

fn insert(app: &mut VectorcraftApp, set: usize, a: usize, at: Option<usize>, step: Step) -> Result<Value, String> {
    let steps = &mut action_mut(app, set, a)?.steps;
    if steps.len() >= MAX_STEPS {
        return Err("too many steps in this action".into());
    }
    let at = at.unwrap_or(steps.len()).min(steps.len());
    steps.insert(at, step);
    Ok(json!({"step": at}))
}

/// What is selected in the panel: a set, an action, or a step of one.
type Selected = (usize, Option<usize>, Option<usize>);

pub fn show(app: &mut VectorcraftApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let selected_id = egui::Id::new("actions-selected");
    let open_id = egui::Id::new("actions-open");
    let mut selected: Option<Selected> = ui.data(|d| d.get_temp(selected_id));
    let mut open: Vec<(usize, usize)> = ui.data(|d| d.get_temp(open_id)).unwrap_or_default();
    let mut play_req = None;
    let mut toggle: Option<(usize, usize, usize, bool)> = None;
    egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
        for (si, set) in app.ui.action_sets.iter().enumerate() {
            let sel = selected == Some((si, None, None));
            let (r, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 22.0), egui::Sense::click());
            if sel {
                ui.painter().rect_filled(r, 0.0, t.row_selected);
            }
            ui.painter().text(
                r.left_center() + egui::vec2(4.0, 0.0),
                egui::Align2::LEFT_CENTER,
                format!("▾ {}", set.name),
                egui::FontId::proportional(12.5),
                t.text,
            );
            if resp.clicked() {
                selected = Some((si, None, None));
            }
            for (ai, a) in set.actions.iter().enumerate() {
                let sel = selected == Some((si, Some(ai), None));
                let expanded = open.contains(&(si, ai));
                let (r, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 22.0), egui::Sense::click());
                if sel {
                    ui.painter().rect_filled(r, 0.0, t.row_selected);
                } else if resp.hovered() {
                    ui.painter().rect_filled(r, 0.0, t.hover);
                }
                let arrow = egui::Rect::from_min_size(r.min + egui::vec2(14.0, 0.0), egui::vec2(14.0, r.height()));
                ui.painter().text(
                    arrow.center(),
                    egui::Align2::CENTER_CENTER,
                    if expanded { "▾" } else { "▸" },
                    egui::FontId::proportional(11.0),
                    t.text_dim,
                );
                let paused = app.ui.action_resume.is_some_and(|p| p.set == si && p.action == ai);
                let name = if paused { format!("{} ⏸", a.name) } else { a.name.clone() };
                ui.painter().text(
                    r.left_center() + egui::vec2(30.0, 0.0),
                    egui::Align2::LEFT_CENTER,
                    name,
                    egui::FontId::proportional(12.5),
                    t.text_strong,
                );
                ui.painter().text(
                    r.right_center() - egui::vec2(6.0, 0.0),
                    egui::Align2::RIGHT_CENTER,
                    crate::i18n::tn(a.steps.len() as u64, "{n} step", "{n} steps"),
                    egui::FontId::proportional(11.0),
                    t.text_dim,
                );
                if resp.clicked() {
                    let on_arrow = resp.interact_pointer_pos().is_some_and(|p| arrow.contains(p));
                    if on_arrow {
                        if expanded {
                            open.retain(|o| *o != (si, ai));
                        } else {
                            open.push((si, ai));
                        }
                    }
                    selected = Some((si, Some(ai), None));
                }
                if resp.double_clicked() {
                    play_req = Some((si, ai, None));
                }
                if !expanded {
                    continue;
                }
                for (i, step) in a.steps.iter().enumerate() {
                    let sel = selected == Some((si, Some(ai), Some(i)));
                    let (r, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 20.0), egui::Sense::click());
                    if sel {
                        ui.painter().rect_filled(r, 0.0, t.row_selected);
                    } else if resp.hovered() {
                        ui.painter().rect_filled(r, 0.0, t.hover);
                    }
                    let check = egui::Rect::from_center_size(r.left_center() + egui::vec2(38.0, 0.0), egui::vec2(12.0, 12.0));
                    ui.painter().rect_stroke(check, 2, egui::Stroke::new(1.0, t.text_dim), egui::StrokeKind::Inside);
                    if step.enabled {
                        ui.painter().text(check.center(), egui::Align2::CENTER_CENTER, "✓", egui::FontId::proportional(11.0), t.text);
                    }
                    let col = if step.enabled { t.text } else { t.text_disabled };
                    ui.painter().text(
                        r.left_center() + egui::vec2(52.0, 0.0),
                        egui::Align2::LEFT_CENTER,
                        step.title(),
                        egui::FontId::proportional(12.0),
                        col,
                    );
                    if resp.clicked() {
                        if resp.interact_pointer_pos().is_some_and(|p| check.expand(3.0).contains(p)) {
                            toggle = Some((si, ai, i, !step.enabled));
                        }
                        selected = Some((si, Some(ai), Some(i)));
                    }
                    if resp.double_clicked() {
                        play_req = Some((si, ai, Some(i)));
                    }
                }
            }
        }
    });
    ui.data_mut(|d| {
        d.insert_temp(selected_id, selected);
        d.insert_temp(open_id, open);
    });
    if let Some((set, action, step, enabled)) = toggle {
        app.run("actions.setStepEnabled", json!({"set": set, "action": action, "step": step, "enabled": enabled})).ok();
    }
    widgets::divider(ui);
    // Bottom bar: stop, record, play, new set, new action, delete.
    ui.horizontal(|ui| {
        let recording = app.ui.recording.is_some();
        if widgets::icon_button(ui, "square", tl!("Stop Playing/Recording"), false, 24.0).clicked()
            && let Some((set, a)) = app.run("actions.stop", json!({})).ok().and_then(|v| Some((index(&v, "set")?, index(&v, "action")?)))
        {
            selected = Some((set, Some(a), None));
        }
        let (r, resp) = ui.allocate_exact_size(egui::vec2(24.0, 24.0), egui::Sense::click());
        ui.painter().circle_filled(r.center(), 6.0, if recording { egui::Color32::from_rgb(0xe0, 0x30, 0x30) } else { t.icon });
        if resp.on_hover_text(tl!("Begin Recording")).clicked() && !recording {
            let params = match selected {
                Some((set, Some(a), _)) => json!({"set": set, "action": a}),
                Some((set, None, _)) => json!({"set": set}),
                None => json!({}),
            };
            app.run("actions.record", params).ok();
        }
        if widgets::icon_button(ui, "dc-actions", tl!("Play Current Selection"), false, 24.0).clicked()
            && let Some((si, Some(ai), step)) = selected
        {
            play_req = Some((si, ai, step));
        }
        if widgets::icon_button(ui, "file-plus", tl!("Create New Action"), false, 24.0).clicked() {
            let set = selected.map_or(0, |s| s.0);
            if let Ok(v) = app.run("actions.newAction", json!({"set": set})) {
                selected = index(&v, "action").map(|a| (set, Some(a), None));
            }
        }
        if widgets::icon_button(ui, "trash-2", tl!("Delete Selection"), false, 24.0).clicked()
            && let Some((si, ai, step)) = selected
        {
            app.run("actions.delete", json!({"set": si, "action": ai, "step": step})).ok();
            selected = None;
        }
        if recording {
            ui.label(egui::RichText::new(tl!("● Recording")).color(egui::Color32::from_rgb(0xe0, 0x30, 0x30)));
        } else if app.ui.action_resume.is_some() {
            dim_label(ui, tl!("Paused: Play continues"));
        }
    });
    ui.data_mut(|d| d.insert_temp(selected_id, selected));
    if let Some((si, ai, step)) = play_req {
        let mut params = json!({"set": si, "action": ai});
        if let Some(i) = step {
            params["step"] = json!(i);
        }
        if let Err(e) = app.run("actions.play", params) {
            app.status(e);
        }
    }
    let _ = icons::exists;
    if app.ui.action_sets.is_empty() {
        dim_label(ui, tl!("No actions. Press ● to record."));
    }
}

/// The panel menu.
pub fn menu(app: &mut VectorcraftApp, ui: &mut egui::Ui) {
    let selected: Option<Selected> = ui.data(|d| d.get_temp(egui::Id::new("actions-selected")));
    let on_action = selected.and_then(|(s, a, _)| Some((s, a?)));
    let set = selected.map_or(0, |s| s.0);
    let recording = app.ui.recording.is_some();
    let run = |app: &mut VectorcraftApp, id: &str, p: Value| {
        if let Err(e) = app.run(id, p) {
            app.status(e);
        }
    };
    if menu_item(ui, tl!("New Action…"), true, false) {
        run(app, "actions.newAction", json!({"set": set}));
    }
    if menu_item(ui, tl!("New Set…"), true, false) {
        run(app, "actions.newSetDialog", json!({}));
    }
    if menu_item(ui, tl!("Duplicate"), on_action.is_some(), false)
        && let Some((s, a)) = on_action
    {
        run(app, "actions.duplicate", json!({"set": s, "action": a}));
    }
    if menu_item(ui, tl!("Delete"), selected.is_some(), false)
        && let Some((s, a, step)) = selected
    {
        run(app, "actions.delete", json!({"set": s, "action": a, "step": step}));
    }
    if menu_item(ui, tl!("Play"), on_action.is_some(), false)
        && let Some((s, a)) = on_action
    {
        run(app, "actions.play", json!({"set": s, "action": a}));
    }
    ui.separator();
    if menu_item(ui, tl!("Start Recording"), !recording, false) {
        let p = match on_action {
            Some((s, a)) => json!({"set": s, "action": a}),
            None => json!({"set": set}),
        };
        run(app, "actions.record", p);
    }
    let after = |p: Value| match selected {
        Some((_, _, Some(i))) => {
            let mut p = p;
            p["at"] = json!(i + 1);
            p
        }
        _ => p,
    };
    if menu_item(ui, tl!("Insert Menu Item…"), on_action.is_some(), false)
        && let Some((s, a)) = on_action
    {
        run(app, "actions.insertMenuItemDialog", after(json!({"set": s, "action": a})));
    }
    if menu_item(ui, tl!("Insert Stop…"), on_action.is_some(), false)
        && let Some((s, a)) = on_action
    {
        run(app, "actions.insertStopDialog", after(json!({"set": s, "action": a})));
    }
    ui.separator();
    if menu_item(ui, tl!("Reset Actions"), true, false) {
        run(app, "actions.reset", json!({}));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vectorcraft_engine::Session;

    fn app() -> VectorcraftApp {
        let mut app = VectorcraftApp::new(Session::new(), Default::default());
        app.run("file.new", json!({"width": 200, "height": 200})).unwrap();
        app
    }
    fn objects(app: &VectorcraftApp) -> usize {
        let mut n = 0;
        app.session.active().unwrap().doc.walk(|node| n += usize::from(matches!(node.kind, vectorcraft_engine::doc::NodeKind::Path { .. })));
        n
    }

    #[test]
    fn defaults_are_valid_commands() {
        for set in default_sets() {
            for a in set.actions {
                for s in a.steps {
                    assert!(vectorcraft_engine::find_command(&s.command).is_some(), "{}", s.command);
                }
            }
        }
    }

    #[test]
    fn old_pairs_still_load() {
        let a: Action =
            serde_json::from_value(json!({"name": "a", "steps": [["object.move", {"dx": 1}], {"command": "edit.copy", "enabled": false}]})).unwrap();
        assert_eq!(
            a.steps,
            vec![Step::new("object.move", json!({"dx": 1})), Step { command: "edit.copy".into(), params: Value::Null, enabled: false }]
        );
    }

    #[test]
    fn record_insert_toggle_stop_and_continue() {
        let mut app = app();
        let set = app.run("actions.newSet", json!({"name": "Mine"})).unwrap()["set"].as_u64().unwrap() as usize;
        app.run("actions.record", json!({"set": set, "name": "Two boxes"})).unwrap();
        app.run("shape.rectangle", json!({"x": 0, "y": 0, "width": 10, "height": 10})).unwrap();
        app.run("shape.rectangle", json!({"x": 20, "y": 0, "width": 10, "height": 10})).unwrap();
        let done = app.run("actions.stop", json!({})).unwrap();
        let a = done["action"].as_u64().unwrap() as usize;
        assert_eq!(action(&app, set, a).unwrap().steps.len(), 2);
        assert_eq!(action(&app, set, a).unwrap().steps[0].title(), "Rectangle");
        // A stop between them pauses playback; Play continues after it.
        app.run("actions.insertStop", json!({"set": set, "action": a, "at": 1, "message": "Check the first box"})).unwrap();
        assert_eq!(objects(&app), 2);
        let r = app.run("actions.play", json!({"set": set, "action": a})).unwrap();
        assert_eq!(r["stopped"], json!("Check the first box"));
        assert_eq!(objects(&app), 3);
        assert_eq!(app.ui.action_resume, Some(Resume { set, action: a, step: 2 }));
        app.run("actions.play", json!({"set": set, "action": a})).unwrap();
        assert_eq!(objects(&app), 4);
        assert!(app.ui.action_resume.is_none());
        // A turned-off step is skipped.
        app.run("actions.setStepEnabled", json!({"set": set, "action": a, "step": 0, "enabled": false})).unwrap();
        app.run("actions.setStepEnabled", json!({"set": set, "action": a, "step": 1, "enabled": false})).unwrap();
        app.run("actions.play", json!({"set": set, "action": a})).unwrap();
        assert_eq!(objects(&app), 5);
        // Insert Menu Item takes a command id or its menu label.
        app.run("actions.insertMenuItem", json!({"set": set, "action": a, "command": "Select All"})).unwrap();
        assert_eq!(action(&app, set, a).unwrap().steps.last().unwrap().command, "select.all");
        assert!(app.run("actions.insertMenuItem", json!({"set": set, "action": a, "command": "no such thing"})).is_err());
        // Duplicate, rename, delete; bad indices are errors.
        let d = app.run("actions.duplicate", json!({"set": set, "action": a})).unwrap();
        assert_eq!(action(&app, set, d["action"].as_u64().unwrap() as usize).unwrap().name, "Two boxes copy");
        app.run("actions.rename", json!({"set": set, "action": a, "to": "Boxes"})).unwrap();
        assert_eq!(action(&app, set, a).unwrap().name, "Boxes");
        for bad in [json!({"set": 99, "action": 0}), json!({"set": set, "action": 99}), json!({"set": set})] {
            assert!(app.run("actions.play", bad.clone()).is_err(), "{bad}");
        }
        assert!(app.run("actions.setStepEnabled", json!({"set": set, "action": a, "step": 999, "enabled": true})).is_err());
        app.run("actions.delete", json!({"set": set, "action": a, "step": 0})).unwrap();
        app.run("actions.delete", json!({"set": set})).unwrap();
        assert!(app.ui.action_sets.iter().all(|s| s.name != "Mine"));
        let list = app.run("actions.list", json!({})).unwrap();
        assert!(list["sets"].as_array().is_some_and(|s| !s.is_empty()));
        app.run("actions.reset", json!({})).unwrap();
        assert_eq!(app.ui.action_sets.len(), default_sets().len());
    }

    #[test]
    fn recording_into_an_action_appends_and_a_dialog_pauses() {
        let mut app = app();
        let a = app.run("actions.newAction", json!({"set": 0, "name": "Grow"})).unwrap()["action"].as_u64().unwrap() as usize;
        app.run("actions.record", json!({"set": 0, "action": a})).unwrap();
        app.run("shape.rectangle", json!({"x": 0, "y": 0, "width": 10, "height": 10})).unwrap();
        app.run("actions.stop", json!({})).unwrap();
        app.run("actions.record", json!({"set": 0, "action": a})).unwrap();
        app.run("shape.ellipse", json!({"x": 0, "y": 0, "width": 10, "height": 10})).unwrap();
        app.run("actions.stop", json!({})).unwrap();
        assert_eq!(action(&app, 0, a).unwrap().steps.len(), 2, "both recordings went into the action");
        // A menu item that opens a dialog pauses playback after it.
        app.run("actions.insertMenuItem", json!({"set": 0, "action": a, "at": 1, "command": "window.workspace.new"})).unwrap();
        let before = objects(&app);
        let r = app.run("actions.play", json!({"set": 0, "action": a})).unwrap();
        assert!(app.ui.dialog.is_some(), "New Workspace is open");
        assert_eq!(r["next"], json!(2));
        assert_eq!(objects(&app), before + 1, "playback waits for the dialog");
        app.ui.dialog = None;
        app.run("actions.play", json!({"set": 0, "action": a})).unwrap();
        assert_eq!(objects(&app), before + 2);
    }

    #[test]
    fn the_panel_draws_steps_and_its_menu() {
        let mut app = app();
        let ctx = egui::Context::default();
        ctx.data_mut(|d| d.insert_temp(egui::Id::new("actions-open"), vec![(0usize, 1usize)]));
        ctx.run_ui(Default::default(), |ui| {
            show(&mut app, ui);
            menu(&mut app, ui);
        })
        .textures_delta
        .clear();
    }
}
