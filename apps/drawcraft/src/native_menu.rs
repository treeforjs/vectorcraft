//! macOS: DrawCraft's menu tree as the native system menu bar (like Illustrator on the Mac).
//! Items dispatch through the same command path as the in-window menus; enablement, check marks
//! and dynamic labels ("Undo Move") are refreshed a few times per second.

use std::collections::HashMap;
use std::str::FromStr;

use drawcraft_ui_egui::DrawcraftApp;
use drawcraft_ui_egui::menus::{self, Item};
use muda::accelerator::Accelerator;
use muda::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use serde_json::Value;

enum Handle {
    Plain(MenuItem),
    Check(CheckMenuItem),
}

pub struct NativeMenu {
    _menu: Menu,
    items: HashMap<String, (String, Value, Handle, String)>,
    last_refresh: f64,
}

/// Accelerator for a shortcut like "Cmd+Shift+]" (modifier-less shortcuts stay in the app so they
/// don't steal keys from text fields).
fn accel(sc: &str) -> Option<Accelerator> {
    if !(sc.contains("Cmd") || sc.contains("Ctrl") || sc.contains("Alt") || sc.starts_with('F')) {
        return None;
    }
    Accelerator::from_str(&sc.replace("Cmd", "CMD").replace("Alt", "ALT").replace("Shift", "SHIFT").replace("Ctrl", "CTRL")).ok()
}

impl NativeMenu {
    pub fn install(app: &mut DrawcraftApp) -> Self {
        let menu = Menu::new();
        let mut items = HashMap::new();
        let mut counter = 0usize;
        for (title, entries) in menus::menu_tree() {
            let sub = Submenu::new(title, true);
            build(app, &sub, &entries, &mut items, &mut counter);
            let _ = menu.append(&sub);
        }
        menu.init_for_nsapp();
        app.native_menu = true;
        app.native_shortcuts = items
            .values()
            .filter(|(_, p, _, c)| (p.is_null() || p.as_object().is_some_and(|o| o.is_empty())) && menus::shortcut_of(c).and_then(accel).is_some())
            .map(|(_, _, _, c)| c.clone())
            .collect();
        Self { _menu: menu, items, last_refresh: 0.0 }
    }

    /// Dispatch clicked items and refresh state.
    pub fn poll(&mut self, app: &mut DrawcraftApp) {
        while let Ok(ev) = MenuEvent::receiver().try_recv() {
            if let Some((cmd, params, _, _)) = self.items.get(ev.id.as_ref()) {
                let p = if params.is_null() { serde_json::json!({}) } else { params.clone() };
                menus::invoke(app, cmd, p);
            }
        }
        let now = drawcraft_ui_egui::now_ms();
        if now - self.last_refresh < 250.0 {
            return;
        }
        self.last_refresh = now;
        for (run, rp, h, cmd) in self.items.values() {
            let null = Value::Null;
            let params = if run == cmd { rp } else { &null };
            let en = menus::enabled(app, cmd);
            match h {
                Handle::Plain(i) => {
                    i.set_enabled(en);
                    if matches!(
                        cmd.as_str(),
                        "edit.undo"
                            | "edit.redo"
                            | "view.outline"
                            | "view.rulers"
                            | "view.guides"
                            | "view.grid"
                            | "view.edges"
                            | "view.artboards"
                            | "view.boundingBox"
                            | "view.transparencyGrid"
                    ) {
                        i.set_text(menus::dynamic_label(app, cmd, ""));
                    }
                }
                Handle::Check(c) => {
                    c.set_enabled(en);
                    c.set_checked(menus::checked(app, cmd, params).unwrap_or(false));
                }
            }
        }
    }
}

fn build(app: &DrawcraftApp, parent: &Submenu, entries: &[Item], items: &mut HashMap<String, (String, Value, Handle, String)>, counter: &mut usize) {
    for e in entries {
        match e {
            Item::Sep => {
                let _ = parent.append(&PredefinedMenuItem::separator());
            }
            Item::Header(h) => {
                let _ = parent.append(&MenuItem::new(*h, false, None));
            }
            Item::Todo(label, sc) => {
                let _ = parent.append(&MenuItem::new(*label, false, accel(sc)));
            }
            Item::Sub(label, children) => {
                let sub = Submenu::new(*label, true);
                build(app, &sub, children, items, counter);
                let _ = parent.append(&sub);
            }
            Item::Cmd(label, cmd, params) => {
                *counter += 1;
                let id = format!("dc{counter}");
                let sc = if params.is_null() { menus::shortcut_of(cmd).and_then(accel) } else { None };
                let handle = if menus::checked(app, cmd, params).is_some() {
                    let c = CheckMenuItem::with_id(id.clone(), *label, true, false, sc);
                    let _ = parent.append(&c);
                    Handle::Check(c)
                } else {
                    let i = MenuItem::with_id(id.clone(), *label, true, sc);
                    let _ = parent.append(&i);
                    Handle::Plain(i)
                };
                let (run, run_params) = menus::click_target(label, cmd, params);
                let _ = &run_params;
                items.insert(id, (run, run_params, handle, cmd.to_string()));
            }
        }
    }
}
