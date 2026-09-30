//! Keyboard shortcuts: command shortcuts from the registry, single-key tool shortcuts, arrows,
//! and tool keys (Enter/Esc/↑/↓ while drawing).

use drawcraft_tools::{Mods, ToolKey};
use egui::{Key, KeyboardShortcut, Modifiers};
use serde_json::json;

use crate::DrawcraftApp;

/// Parse "Cmd+Shift+]" into an egui shortcut. `Cmd` is Command on macOS and Ctrl elsewhere.
pub fn parse(s: &str) -> Option<KeyboardShortcut> {
    if s.is_empty() {
        return None;
    }
    let (mods_part, key_part) =
        if let Some(stripped) = s.strip_suffix("++") { (stripped.trim_end_matches('+'), "+") } else { s.rsplit_once('+').unwrap_or(("", s)) };
    let mut m = Modifiers::NONE;
    for part in mods_part.split('+').filter(|p| !p.is_empty()) {
        match part {
            "Cmd" => m |= Modifiers::COMMAND,
            "Shift" => m |= Modifiers::SHIFT,
            "Alt" => m |= Modifiers::ALT,
            "Ctrl" => m |= Modifiers::CTRL,
            _ => return None,
        }
    }
    let key = match key_part {
        "]" => Key::CloseBracket,
        "[" => Key::OpenBracket,
        ";" => Key::Semicolon,
        "'" => Key::Quote,
        "/" => Key::Slash,
        "\\" => Key::Backslash,
        "=" => Key::Equals,
        "-" => Key::Minus,
        "+" => Key::Plus,
        "Delete" => Key::Delete,
        "Backspace" => Key::Backspace,
        "Tab" => Key::Tab,
        "~" => Key::Backtick,
        k => Key::from_name(k)?,
    };
    Some(KeyboardShortcut::new(m, key))
}

/// Every command shortcut in effect (user overrides from Edit → Keyboard Shortcuts win).
fn all_shortcuts() -> Vec<(KeyboardShortcut, &'static str)> {
    let mut v: Vec<(KeyboardShortcut, &'static str)> = vec![];
    for c in drawcraft_engine::command_specs() {
        if let Some(sc) = crate::menus::shortcut_of(c.id).and_then(parse) {
            v.push((sc, c.id));
        }
    }
    for c in crate::menus::UI_COMMANDS {
        if let Some(sc) = crate::menus::shortcut_of(c.0).and_then(parse) {
            v.push((sc, c.0));
        }
    }
    // Most specific (most modifiers) first so Cmd+Shift+Z isn't eaten by Cmd+Z.
    v.sort_by_key(|(sc, _)| {
        std::cmp::Reverse(sc.modifiers.shift as u8 + sc.modifiers.alt as u8 + sc.modifiers.command as u8 + sc.modifiers.ctrl as u8)
    });
    v
}

pub fn handle(app: &mut DrawcraftApp, ctx: &egui::Context) {
    if app.ui.dialog.is_some() || app.ui.palette_open {
        if crate::shortcut_editor::is_recording(app) {
            return;
        }
        if ctx.input(|i| i.key_pressed(Key::Escape)) {
            app.ui.dialog = None;
            app.ui.palette_open = false;
        }
        return;
    }
    let typing = ctx.egui_wants_keyboard_input();
    let view = app.view_info();
    // Tool keys first (Enter/Escape end paths; arrows change polygon sides while dragging).
    let busy = app.session.tool_busy();
    for (k, tk) in [(Key::Enter, ToolKey::Enter), (Key::Escape, ToolKey::Escape)] {
        if !typing && ctx.input(|i| i.key_pressed(k)) {
            let _ = app.session.tool_key(tk, Mods::default(), view);
            if k == Key::Escape && !busy {
                if app.session.active().is_some_and(|d| d.doc.pattern_edit.is_some()) {
                    let _ = app.run("object.pattern.done", json!({}));
                } else if app.session.active().is_some_and(|d| d.isolation.is_some()) {
                    let _ = app.run("object.exitIsolation", json!({}));
                } else if app.ui.flyout.is_some() {
                    app.ui.flyout = None;
                }
            }
        }
    }
    if typing {
        return;
    }
    // Type tool editing: text and editing keys go to the tool.
    if app.session.tool_wants_text() {
        let texts: Vec<String> =
            ctx.input(|i| i.events.iter().filter_map(|e| if let egui::Event::Text(t) = e { Some(t.clone()) } else { None }).collect());
        for t in texts {
            let _ = app.session.tool_text(&t, view);
        }
        // Editing keys with modifiers, clipboard and Cmd+A.
        crate::panels::character::route_type_input(app, ctx);
        // Enter was already delivered above as ToolKey::Enter (newline).
        let fire = all_shortcuts()
            .into_iter()
            .filter(|(sc, _)| sc.modifiers.command)
            .find(|(sc, _)| ctx.input_mut(|i| i.consume_shortcut(sc)))
            .map(|(_, id)| id);
        if let Some(id) = fire {
            crate::menus::invoke(app, id, json!({}));
        }
        return;
    }
    if busy {
        for (k, tk) in [(Key::ArrowUp, ToolKey::Up), (Key::ArrowDown, ToolKey::Down)] {
            if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, k)) {
                let _ = app.session.tool_key(tk, Mods::default(), view);
            }
        }
        return;
    }
    // Command shortcuts.
    let mut fire: Option<&'static str> = None;
    for (sc, id) in all_shortcuts() {
        // Letter / slash keys without Cmd/Alt/Ctrl are handled below as text (tool shortcuts, X, D, /).
        let plain = !(sc.modifiers.command || sc.modifiers.alt || sc.modifiers.ctrl);
        if plain && (sc.logical_key.name().len() == 1 || sc.logical_key == Key::Slash) {
            continue;
        }
        if app.native_shortcuts.contains(id) {
            continue;
        }
        if ctx.input_mut(|i| i.consume_shortcut(&sc)) {
            fire = Some(id);
            break;
        }
    }
    if let Some(id) = fire {
        crate::menus::invoke(app, id, json!({}));
        return;
    }
    // Arrow nudges (Shift = ×10, Alt = copy).
    let arrows = [(Key::ArrowLeft, -1.0, 0.0), (Key::ArrowRight, 1.0, 0.0), (Key::ArrowUp, 0.0, -1.0), (Key::ArrowDown, 0.0, 1.0)];
    for (k, dx, dy) in arrows {
        let m = ctx.input(|i| i.modifiers);
        if ctx.input_mut(|i| i.consume_key(m, k)) && app.session.active().is_some_and(|d| !d.selection.is_empty()) {
            let _ = app.run("object.nudge", json!({"dx": dx, "dy": dy, "big": m.shift, "copy": m.alt}));
        }
    }
    // Delete / Backspace clear the selection.
    if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Backspace)) && app.session.active().is_some_and(|d| !d.selection.is_empty()) {
        let _ = app.run("edit.clear", json!({}));
    }
    // Single-key tool shortcuts (no Cmd/Ctrl/Alt).
    let events: Vec<(String, Modifiers)> = ctx.input(|i| {
        i.events
            .iter()
            .filter_map(|e| match e {
                egui::Event::Text(t) => Some((t.clone(), i.modifiers)),
                _ => None,
            })
            .collect()
    });
    for (text, m) in events {
        if m.command || m.alt || m.ctrl {
            continue;
        }
        let upper = text.to_uppercase();
        let key = if m.shift && text.chars().all(|c| c.is_alphabetic()) { format!("Shift+{upper}") } else { upper.clone() };
        // Single-key command shortcuts (X, Shift+X, D, /, Shift+D, F, Shift+F by default).
        if let Some(id) = crate::shortcut_editor::command_for_key(&key).or_else(|| crate::shortcut_editor::command_for_key(&text)) {
            let _ = app.run(id, json!({}));
            continue;
        }
        if let Some(t) = crate::shortcut_editor::tool_for_key(&key).or_else(|| crate::shortcut_editor::tool_for_key(&text)) {
            app.select_tool(t);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses() {
        let s = parse("Cmd+Shift+]").unwrap();
        assert_eq!(s.logical_key, Key::CloseBracket);
        assert!(s.modifiers.shift && s.modifiers.command);
        assert_eq!(parse("Cmd+=").unwrap().logical_key, Key::Equals);
        assert_eq!(parse("Cmd+Alt+2").unwrap().logical_key, Key::Num2);
        assert_eq!(parse("F12").unwrap().logical_key, Key::F12);
        assert!(parse("").is_none());
    }

    #[test]
    fn all_registered_shortcuts_parse() {
        for c in drawcraft_engine::command_specs() {
            if let Some(s) = c.shortcut
                && s != "D"
                && s != "X"
                && s != "Shift+X"
                && s != "/"
            {
                assert!(parse(s).is_some(), "{} has unparsable shortcut {s}", c.id);
            }
        }
        for c in crate::menus::UI_COMMANDS {
            if !c.2.is_empty() && c.2 != "F" && c.2 != "Shift+F" {
                assert!(parse(c.2).is_some(), "{} has unparsable shortcut {}", c.0, c.2);
            }
        }
    }
}
