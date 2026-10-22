//! Window → Layouts: saved panel/bar arrangements (Default, Focus, Drawing, … Classic), Reset,
//! New Layout…, Manage Layouts…. A layout (a "workspace" in the code and the command ids) is a
//! snapshot of `UiState` layout flags; UI brightness and everything else is untouched when
//! switching. The built-in layouts have plain names; their old names still load ([`ALIASES`]).

use std::sync::atomic::Ordering;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::state::{Dialog, DockTab, FloatingFlyout, FloatingPanels, UiState};
use crate::theme::Tokens;
use crate::{VectorcraftApp, widgets};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Workspace {
    pub name: String,
    pub control_bar: bool,
    pub rulers: bool,
    pub toolbar: bool,
    pub toolbar_double: bool,
    pub toolbar_advanced: bool,
    pub task_bar: bool,
    /// The insert bar over the canvas (older saved workspaces: shown).
    pub insert_bar: bool,
    pub dock: bool,
    pub dock_tab: DockTab,
    /// The dock's tabbed group is collapsed to icons (built-in workspaces: expanded).
    pub dock_collapsed: bool,
    pub open_panel: Option<String>,
    /// The dock's column of panel icons (built-in layouts: Classic only). A layout saved before
    /// it existed had one, and keeps it.
    #[serde(default = "yes")]
    pub panel_icons: bool,
    pub status_bar: bool,
    /// Tool groups torn off the toolbar into floating strips (built-in workspaces: none).
    pub floating_flyouts: Vec<FloatingFlyout>,
    /// Panels dragged out of the dock, where they float (built-in workspaces: none).
    pub floating_panels: Vec<FloatingPanels>,
    /// Where the Tools panel floats (built-in workspaces: docked).
    pub toolbar_pos: Option<[f32; 2]>,
}

impl Default for Workspace {
    fn default() -> Self {
        let ui = UiState::default();
        capture(&ui, DEFAULT)
    }
}

fn yes() -> bool {
    true
}

/// The layout new users start in.
pub const DEFAULT: &str = "Default";

/// The built-in layouts, in menu order: plain names for what each is for.
pub const BUILTIN_NAMES: [&str; 8] = [DEFAULT, "Focus", "Drawing", "Type", "Color", "Print & Export", "Web", "Classic"];

/// Names older versions gave the built-in layouts → the layout each is now. Settings, saved
/// layouts, scripts and agents that name one still get it (case-insensitive, as names are).
pub const ALIASES: &[(&str, &str)] = &[
    ("Essentials", DEFAULT),
    ("Essentials Classic", "Classic"),
    ("Automation", "Classic"),
    ("Layout", DEFAULT),
    ("Painting", "Drawing"),
    ("Printing and Proofing", "Print & Export"),
    ("Tracing", "Drawing"),
    ("Typography", "Type"),
];

/// A built-in layout's current name for `name` (an old name maps to its new one); any other
/// name as it is.
pub fn canonical(name: &str) -> &str {
    ALIASES.iter().find(|(old, _)| old.eq_ignore_ascii_case(name)).map_or(name, |(_, new)| new)
}

/// Built-in layouts, in menu order (Default first).
pub fn builtins() -> Vec<Workspace> {
    let base = Workspace::default();
    let w = |name: &str, f: &dyn Fn(&mut Workspace)| {
        let mut x = Workspace { name: name.into(), ..base.clone() };
        f(&mut x);
        x
    };
    vec![
        w(DEFAULT, &|_| {}),
        // The canvas first: the panels collapse to icons.
        w("Focus", &|x| x.dock_collapsed = true),
        w("Drawing", &|x| x.open_panel = Some("brushes".into())),
        w("Type", &|x| {
            x.rulers = true;
            x.open_panel = Some("character".into());
        }),
        w("Color", &|x| x.open_panel = Some("swatches".into())),
        w("Print & Export", &|x| {
            x.rulers = true;
            x.open_panel = Some("separations".into());
        }),
        w("Web", &|x| {
            x.rulers = true;
            x.open_panel = Some(crate::panels::asset_export::ID.into());
        }),
        // Every bar and the whole panel column, for people used to them.
        w("Classic", &|x| {
            x.control_bar = true;
            x.rulers = true;
            x.toolbar_advanced = true;
            x.task_bar = false;
            x.insert_bar = false;
            x.panel_icons = true;
            x.open_panel = Some("color".into());
        }),
    ]
}

/// A built-in layout's name, current or old. Allocation-free: the app bar asks every frame.
pub fn is_builtin(name: &str) -> bool {
    BUILTIN_NAMES.contains(&name) || ALIASES.iter().any(|(old, _)| *old == name)
}

/// A name a user layout can't take: a built-in one, current or old, in any case.
fn reserved(name: &str) -> bool {
    let name = canonical(name);
    BUILTIN_NAMES.iter().any(|b| b.eq_ignore_ascii_case(name))
}

/// Snapshot the layout flags of `ui` as a workspace called `name`.
pub fn capture(ui: &UiState, name: &str) -> Workspace {
    Workspace {
        name: name.into(),
        control_bar: ui.control_bar,
        rulers: ui.view.rulers,
        toolbar: ui.toolbar,
        toolbar_double: ui.toolbar_double,
        toolbar_advanced: ui.toolbar_advanced,
        task_bar: ui.task_bar,
        insert_bar: ui.insert_bar,
        dock: ui.dock,
        dock_tab: ui.dock_tab,
        dock_collapsed: ui.dock_collapsed,
        open_panel: ui.open_panel.clone(),
        panel_icons: ui.panel_icons,
        status_bar: ui.status_bar,
        floating_flyouts: ui.floating_flyouts.clone(),
        floating_panels: ui.floating_panels.clone(),
        toolbar_pos: ui.toolbar_pos,
    }
}

/// Apply a workspace's layout flags (brightness, prefs, shortcuts etc. untouched).
pub fn apply(ui: &mut UiState, w: &Workspace) {
    ui.control_bar = w.control_bar;
    ui.view.rulers = w.rulers;
    ui.toolbar = w.toolbar;
    ui.toolbar_double = w.toolbar_double;
    ui.toolbar_advanced = w.toolbar_advanced;
    ui.task_bar = w.task_bar;
    ui.insert_bar = w.insert_bar;
    ui.dock = w.dock;
    ui.dock_tab = w.dock_tab;
    ui.dock_collapsed = w.dock_collapsed;
    ui.open_panel = w.open_panel.clone();
    ui.panel_icons = w.panel_icons;
    ui.status_bar = w.status_bar;
    ui.floating_flyouts = w.floating_flyouts.clone();
    ui.floating_panels = w.floating_panels.clone();
    ui.toolbar_pos = w.toolbar_pos;
    FloatingPanels::sanitize(&mut ui.floating_panels, &mut ui.toolbar_pos);
    ui.flyout = None;
    ui.workspace = w.name.clone();
}

/// A workspace by name (user workspaces shadow nothing: names are unique across both lists).
pub fn find(ui: &UiState, name: &str) -> Option<Workspace> {
    let name = canonical(name);
    ui.custom_workspaces
        .iter()
        .find(|w| w.name.eq_ignore_ascii_case(name))
        .cloned()
        .or_else(|| builtins().into_iter().find(|w| w.name.eq_ignore_ascii_case(name)))
}

/// Every workspace name: built-ins then user workspaces.
pub fn names(ui: &UiState) -> Vec<String> {
    builtins().into_iter().map(|w| w.name).chain(ui.custom_workspaces.iter().map(|w| w.name.clone())).collect()
}

/// Save the current layout as `name` (replacing a user workspace of that name).
pub fn save_as(ui: &mut UiState, name: &str) -> Result<(), String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("layout name can't be empty".into());
    }
    if reserved(name) {
        return Err(format!("`{name}` is a built-in layout; choose another name"));
    }
    let w = capture(ui, name);
    match ui.custom_workspaces.iter_mut().find(|x| x.name == name) {
        Some(x) => *x = w,
        None => ui.custom_workspaces.push(w),
    }
    ui.workspace = name.into();
    Ok(())
}

pub fn delete(ui: &mut UiState, name: &str) -> Result<(), String> {
    let before = ui.custom_workspaces.len();
    ui.custom_workspaces.retain(|w| w.name != name);
    if ui.custom_workspaces.len() == before {
        return Err(if is_builtin(name) { format!("`{name}` is built in and can't be deleted") } else { format!("no layout `{name}`") });
    }
    if ui.workspace == name {
        ui.workspace = DEFAULT.into();
    }
    Ok(())
}

pub fn rename(ui: &mut UiState, from: &str, to: &str) -> Result<(), String> {
    let to = to.trim();
    if to.is_empty() || reserved(to) || ui.custom_workspaces.iter().any(|w| w.name == to) {
        return Err(format!("`{to}` is not an available name"));
    }
    let w = ui.custom_workspaces.iter_mut().find(|w| w.name == from).ok_or_else(|| format!("no user layout `{from}`"))?;
    w.name = to.into();
    if ui.workspace == from {
        ui.workspace = to.into();
    }
    Ok(())
}

// ---------- menu support (menu_tree has no app access) ----------

crate::shortcut_editor::ui_mirror!(fn names_store() -> Vec<&'static str>);

/// Mirror the user workspace names for the (static) menu tree.
pub fn sync(ui: &UiState) {
    let same = names_store()
        .read()
        .map(|v| v.len() == ui.custom_workspaces.len() && v.iter().zip(&ui.custom_workspaces).all(|(a, b)| *a == b.name))
        .unwrap_or(true);
    if !same {
        if let Ok(mut v) = names_store().write() {
            *v = ui.custom_workspaces.iter().map(|w| crate::shortcut_editor::intern(&w.name)).collect();
        }
        crate::shortcut_editor::GENERATION.fetch_add(1, Ordering::Relaxed);
    }
}

/// Window → Workspace submenu items.
pub fn menu_items() -> Vec<crate::menus::Item> {
    use crate::menus::Item;
    let mut v: Vec<Item> =
        builtins().into_iter().map(|w| Item::Cmd(crate::shortcut_editor::intern(&w.name), "window.workspace", json!({"name": w.name}))).collect();
    let custom = names_store().read().map(|v| v.clone()).unwrap_or_default();
    if !custom.is_empty() {
        v.push(Item::Sep);
        v.extend(custom.into_iter().map(|n| Item::Cmd(n, "window.workspace", json!({"name": n}))));
    }
    v.push(Item::Sep);
    v.push(Item::Cmd("Reset Layout", "window.workspace.reset", Value::Null));
    v.push(Item::Cmd("New Layout…", "window.workspace.new", Value::Null));
    v.push(Item::Cmd("Manage Layouts…", "window.workspace.manage", Value::Null));
    v
}

/// The app-bar workspace switcher's menu.
pub fn popup(app: &mut VectorcraftApp, ui: &mut egui::Ui) {
    ui.set_min_width(210.0);
    ui.set_max_width(240.0);
    let mut clicked: Option<(&'static str, Value)> = None;
    for it in menu_items() {
        match it {
            crate::menus::Item::Sep => {
                ui.separator();
            }
            crate::menus::Item::Cmd(label, id, p) => {
                // As the Window menu draws it: custom workspace names (and Reset <name>) as they are.
                let label = crate::menus::display_label(app, id, label);
                let text = match crate::menus::checked(app, id, &p) {
                    Some(true) => format!("✓  {label}"),
                    Some(false) => format!("     {label}"),
                    None => format!("     {label}"),
                };
                if ui.add(egui::Button::new(text).min_size(egui::vec2(220.0, 0.0))).clicked() {
                    clicked = Some((id, if p.is_null() { json!({}) } else { p }));
                    ui.close();
                }
            }
            _ => {}
        }
    }
    if let Some((id, p)) = clicked {
        crate::menus::invoke(app, id, p);
    }
}

// ---------- commands ----------

pub fn run_command(app: &mut VectorcraftApp, id: &str, p: &Value) -> Option<Result<Value, String>> {
    let s = |k: &str| p.get(k).and_then(Value::as_str).map(str::to_string);
    let r = match id {
        "window.workspace" => match s("name").and_then(|n| find(&app.ui, &n)) {
            Some(w) => {
                apply(&mut app.ui, &w);
                Ok(json!({"workspace": w.name}))
            }
            None => Err(format!("unknown layout (one of {})", names(&app.ui).join(", "))),
        },
        "window.workspace.reset" => {
            let w = find(&app.ui, &app.ui.workspace.clone()).unwrap_or_default();
            apply(&mut app.ui, &w);
            Ok(json!({"workspace": w.name}))
        }
        "window.workspace.new" => match s("name") {
            Some(n) => save_as(&mut app.ui, &n).map(|_| json!({"workspace": n})),
            None => {
                let n = (1..).map(|i| format!("Layout {i}")).find(|n| find(&app.ui, n).is_none()).unwrap_or_default();
                app.ui.dialog = Some(Dialog::new("newWorkspace", json!({"name": n})));
                Ok(Value::Null)
            }
        },
        "window.workspace.manage" => {
            app.ui.dialog = Some(Dialog::new("manageWorkspaces", json!({"selected": "", "name": ""})));
            Ok(Value::Null)
        }
        "window.workspace.delete" => delete(&mut app.ui, &s("name").unwrap_or_default()).map(|_| Value::Null),
        "window.workspace.rename" => rename(&mut app.ui, &s("name").unwrap_or_default(), &s("to").unwrap_or_default()).map(|_| Value::Null),
        "window.workspace.list" => Ok(json!({"current": app.ui.workspace, "workspaces": names(&app.ui), "custom": app.ui.custom_workspaces})),
        _ => return None,
    };
    Some(r)
}

/// OK in New Workspace / Manage Workspaces.
pub fn confirm(app: &mut VectorcraftApp) -> Result<Value, String> {
    let Some(d) = app.ui.dialog.clone() else { return Err("no dialog open".into()) };
    let r = match d.kind.as_str() {
        "newWorkspace" => save_as(&mut app.ui, &d.str("name")).map(|_| Value::Null),
        _ => Ok(Value::Null),
    };
    if r.is_ok() {
        app.ui.dialog = None;
    }
    r
}

pub fn show(app: &mut VectorcraftApp, ctx: &egui::Context) {
    let Some(mut d) = app.ui.dialog.clone() else { return };
    let t = Tokens::get(ctx);
    let (mut ok, mut cancel) = (false, false);
    let mut action: Option<(&str, Value)> = None;
    let manage = d.kind == "manageWorkspaces";
    crate::dialogs::modal::show(ctx, "Layout", egui::Id::new("dialog-workspace"), -40.0, 22, |ui| {
        ui.set_width(340.0);
        crate::dialogs::modal::heading(ui, if manage { tl!("Manage Layouts") } else { tl!("New Layout") });
        ui.add_space(12.0);
        if manage {
            egui::Frame::NONE.fill(t.input).stroke(egui::Stroke::new(1.0, t.input_border)).inner_margin(egui::Margin::same(4)).show(ui, |ui| {
                ui.set_min_height(140.0);
                ui.set_width(ui.available_width());
                for n in names(&app.ui) {
                    let builtin = is_builtin(&n);
                    let shown = crate::panels::label_or_name(&n, builtin);
                    let text = if builtin { egui::RichText::new(shown).color(t.text_dim) } else { egui::RichText::new(shown).color(t.text) };
                    if ui.selectable_label(d.str("selected") == n, text).clicked() && !builtin {
                        d.fields.insert("selected".into(), json!(n));
                        d.fields.insert("name".into(), json!(n));
                    }
                }
            });
            ui.add_space(8.0);
            let sel = d.str("selected");
            ui.horizontal(|ui| {
                let mut name = d.str("name");
                if ui.add(egui::TextEdit::singleline(&mut name).desired_width(180.0)).changed() {
                    d.fields.insert("name".into(), json!(name));
                }
                if ui.button(tl!("New")).on_hover_text(tl!("Save the current layout under this name")).clicked() {
                    action = Some(("window.workspace.new", json!({"name": d.str("name")})));
                }
                ui.add_enabled_ui(!sel.is_empty(), |ui| {
                    if ui.button(tl!("Rename")).clicked() {
                        action = Some(("window.workspace.rename", json!({"name": sel, "to": d.str("name")})));
                    }
                    if ui.button(tl!("Delete")).clicked() {
                        action = Some(("window.workspace.delete", json!({"name": sel})));
                    }
                });
            });
        } else {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(tl!("Name:")).color(t.text_dim));
                let mut name = d.str("name");
                let r = ui.add(egui::TextEdit::singleline(&mut name).desired_width(240.0));
                if r.changed() {
                    d.fields.insert("name".into(), json!(name));
                }
            });
            ui.label(egui::RichText::new(tl!("Saves the current bars, toolbar and panel layout.")).color(t.text_dim).size(11.0));
        }
        ui.add_space(16.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if widgets::primary_button(ui, tl!("OK")).clicked() {
                ok = true;
            }
            ui.add_space(8.0);
            if !manage && widgets::secondary_button(ui, tl!("Cancel")).clicked() {
                cancel = true;
            }
        });
    });
    if !manage && ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
        ok = true;
    }
    app.ui.dialog = Some(d);
    if let Some((id, p)) = action {
        match run_command(app, id, &p) {
            Some(Err(e)) => app.status(e),
            _ => {
                if let Some(d) = app.ui.dialog.as_mut() {
                    d.fields.insert("selected".into(), json!(""));
                }
            }
        }
    }
    if cancel {
        app.ui.dialog = None;
    } else if ok && let Err(e) = confirm(app) {
        app.status(e);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_the_default_layout() {
        let ui = UiState::default();
        assert_eq!(capture(&ui, DEFAULT), builtins()[0]);
        assert_eq!(ui.workspace, DEFAULT);
        assert!(!ui.panel_icons, "new users get the inspector and one Panels button, not the icon column");
    }

    #[test]
    fn apply_classic_then_reset_to_essentials_keeps_brightness() {
        let mut ui = UiState { brightness: crate::theme::Brightness::Light, ..Default::default() };
        let w = find(&ui, "Essentials Classic").unwrap();
        apply(&mut ui, &w);
        assert!(ui.control_bar && ui.view.rulers && ui.toolbar_advanced && ui.panel_icons);
        assert_eq!(ui.open_panel.as_deref(), Some("color"));
        assert_eq!(ui.workspace, "Classic", "the old name loads the layout under its new name");
        let w = find(&ui, "essentials").unwrap();
        apply(&mut ui, &w);
        assert!(!ui.control_bar && !ui.view.rulers && !ui.panel_icons);
        assert_eq!(ui.workspace, DEFAULT);
        assert_eq!(ui.brightness, crate::theme::Brightness::Light);
    }

    #[test]
    fn save_apply_round_trip() {
        let mut ui = UiState { control_bar: true, dock_tab: DockTab::Layers, open_panel: Some("stroke".into()), ..Default::default() };
        save_as(&mut ui, "Mine").unwrap();
        let w = find(&ui, DEFAULT).unwrap();
        apply(&mut ui, &w);
        assert!(!ui.control_bar);
        let w = find(&ui, "Mine").unwrap();
        apply(&mut ui, &w);
        assert!(ui.control_bar && ui.dock_tab == DockTab::Layers && ui.open_panel.as_deref() == Some("stroke"));
        // Survives the UiState JSON round trip.
        let back: UiState = serde_json::from_value(serde_json::to_value(&ui).unwrap()).unwrap();
        assert_eq!(back.custom_workspaces, ui.custom_workspaces);
        assert_eq!(back.workspace, "Mine");
    }

    #[test]
    fn builtin_names_are_protected() {
        let mut ui = UiState::default();
        assert!(save_as(&mut ui, "Painting").is_err());
        assert!(save_as(&mut ui, "drawing").is_err(), "names are case-insensitive");
        assert!(save_as(&mut ui, "  ").is_err());
        assert!(delete(&mut ui, "Essentials").is_err());
        save_as(&mut ui, "A").unwrap();
        assert!(rename(&mut ui, "A", "Web").is_err());
        rename(&mut ui, "A", "B").unwrap();
        assert_eq!(ui.workspace, "B");
        delete(&mut ui, "B").unwrap();
        assert_eq!(ui.workspace, DEFAULT);
    }

    #[test]
    fn every_layout_and_every_old_name_loads() {
        let ui = UiState::default();
        for n in BUILTIN_NAMES {
            assert_eq!(find(&ui, n).map(|w| w.name), Some(n.to_string()), "{n}");
        }
        for (old, new) in ALIASES {
            assert_eq!(find(&ui, old).map(|w| w.name).as_deref(), Some(*new), "{old}");
            assert!(is_builtin(old) && !BUILTIN_NAMES.contains(old), "{old} is only an alias");
        }
        assert_eq!(names(&ui), BUILTIN_NAMES.map(String::from).to_vec(), "the menu lists the new names only");
    }

    /// Settings and user layouts saved by older versions: their layout name and panel column
    /// carry over (the old built-in name is its new one; a saved column stays).
    #[test]
    fn layouts_saved_by_older_versions_keep_their_panels() {
        let old = json!({"workspace": "Essentials", "custom_workspaces": [{"name": "Mine", "openPanel": "stroke"}]});
        let ui: UiState = serde_json::from_value(old).unwrap();
        let ui = ui.sanitized();
        assert_eq!(ui.workspace, DEFAULT);
        assert!(ui.panel_icons, "a settings file from before keeps its icon column");
        assert!(ui.custom_workspaces[0].panel_icons, "so does a layout saved before");
        let mut fresh = UiState::default();
        let w = find(&ui, "Mine").unwrap();
        apply(&mut fresh, &w);
        assert!(fresh.panel_icons);
        let back: UiState = serde_json::from_value(serde_json::to_value(&fresh).unwrap()).unwrap();
        assert!(back.panel_icons);
        let fresh_back: UiState = serde_json::from_value(serde_json::to_value(UiState::default()).unwrap()).unwrap();
        assert!(!fresh_back.panel_icons, "a new user's choice survives a save");
    }
}
