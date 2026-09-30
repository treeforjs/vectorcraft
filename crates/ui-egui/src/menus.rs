//! The menu bar (Illustrator's menu tree) and UI-level commands.
//!
//! Items bound to a command id run through [`DrawcraftApp::run`]. Items not implemented yet are
//! listed (disabled, with their shortcut) so the full surface is visible and discoverable; the
//! parity tracker drives them to "done".

use serde::Serialize;
use serde_json::{Value, json};

use crate::DrawcraftApp;
use crate::io;
use crate::state::{DockTab, ICON_PANELS, next_zoom};
use crate::theme::{self, Brightness, Tokens};

#[derive(Clone, Debug)]
pub enum Item {
    /// (label, command id, params)
    Cmd(&'static str, &'static str, Value),
    /// Not implemented yet: (label, shortcut)
    Todo(&'static str, &'static str),
    Sub(&'static str, Vec<Item>),
    Sep,
    /// Section header (disabled label, e.g. "Illustrator Effects").
    Header(&'static str),
}

fn c(label: &'static str, id: &'static str) -> Item {
    Item::Cmd(label, id, Value::Null)
}
fn cp(label: &'static str, id: &'static str, p: Value) -> Item {
    Item::Cmd(label, id, p)
}
fn todo(label: &'static str) -> Item {
    Item::Todo(label, "")
}
fn todos(label: &'static str, sc: &'static str) -> Item {
    Item::Todo(label, sc)
}
fn sub(label: &'static str, items: Vec<Item>) -> Item {
    Item::Sub(label, items)
}
use Item::Sep;

/// UI-level commands: (id, label, shortcut, params doc).
pub const UI_COMMANDS: &[(&str, &str, &str, &str)] = &[
    ("file.open", "Open…", "Cmd+O", "{path?}"),
    ("file.save", "Save", "Cmd+S", "{path?}"),
    ("file.saveAs", "Save As…", "Cmd+Shift+S", "{path?}"),
    ("file.saveCopy", "Save a Copy…", "Cmd+Alt+S", "{path?}"),
    ("file.revert", "Revert", "F12", "{}"),
    ("file.place", "Place…", "Cmd+Shift+P", "{path?}"),
    ("file.export.svg", "Export As SVG…", "", "{path?}"),
    ("file.export.png", "Export As PNG…", "", "{path?, scale?: 1}"),
    ("file.exportForScreens", "Export for Screens…", "Cmd+Alt+E", "{path?, scale?}"),
    ("file.documentSetup", "Document Setup…", "Cmd+Alt+P", "{}"),
    ("file.newDialog", "New…", "Cmd+N", "{} opens the New Document dialog"),
    ("edit.preferences", "Preferences…", "Cmd+K", "{}"),
    ("view.outline", "Outline", "Cmd+Y", "{} toggle Outline/Preview"),
    ("view.pixelPreview", "Pixel Preview", "Cmd+Alt+Y", "{}"),
    ("view.zoomIn", "Zoom In", "Cmd+=", "{}"),
    ("view.zoomOut", "Zoom Out", "Cmd+-", "{}"),
    ("view.fitArtboard", "Fit Artboard in Window", "Cmd+0", "{}"),
    ("view.fitAll", "Fit All in Window", "Cmd+Alt+0", "{}"),
    ("view.actualSize", "Actual Size", "Cmd+1", "{}"),
    ("view.setZoom", "Set Zoom", "", "{zoom: percent, center?: [x,y]}"),
    ("view.edges", "Hide Edges", "Cmd+H", "{}"),
    ("view.artboards", "Hide Artboards", "Cmd+Shift+H", "{}"),
    ("view.rulers", "Show Rulers", "Cmd+R", "{}"),
    ("view.boundingBox", "Hide Bounding Box", "Cmd+Shift+B", "{}"),
    ("view.transparencyGrid", "Show Transparency Grid", "Cmd+Shift+D", "{}"),
    ("view.guides", "Hide Guides", "Cmd+;", "{}"),
    ("view.smartGuides", "Smart Guides", "Cmd+U", "{}"),
    ("view.grid", "Show Grid", "Cmd+'", "{}"),
    ("view.snapToGrid", "Snap to Grid", "Cmd+Shift+'", "{}"),
    ("view.snapToPoint", "Snap to Point", "Cmd+Alt+'", "{}"),
    ("view.presentation", "Presentation Mode", "Shift+F", "{}"),
    ("view.screenMode", "Screen Mode", "F", "{mode?: 0..2} (no param cycles)"),
    ("view.rotateReset", "Reset Rotate View", "Cmd+Shift+1", "{}"),
    ("window.control", "Control", "", "{}"),
    ("window.toolbar", "Tools", "", "{}"),
    ("window.toolbarColumns", "Toolbar: Single/Double Column", "", "{}"),
    ("window.toolbarAdvanced", "Toolbar: Advanced / Basic", "", "{}"),
    ("window.taskBar", "Contextual Task Bar", "", "{}"),
    ("window.dock", "Panels", "Tab", "{} show/hide all panels"),
    ("window.panel", "Show Panel", "", "{panel: id} e.g. layers, swatches, stroke"),
    ("window.brightness", "UI Brightness", "", "{brightness: dark|mediumDark|mediumLight|light}"),
    ("window.workspace.reset", "Reset Essentials", "", "{}"),
    ("window.newWindow", "New Window", "", "{}"),
    ("tool.select", "Select Tool", "", "{tool: id} (see tools)"),
    ("tool.setOption", "Tool Option", "", "{key, value}"),
    ("effect.dialog", "Effect…", "", "{effect: id} open the effect's dialog with live preview"),
    ("ui.paramDialog", "Command Dialog", "", "{command, label?, params} open a parameter dialog for any command"),
    ("effect.applyLast", "Apply Last Effect", "Cmd+Shift+E", "{}"),
    ("file.export.pdf", "Save as PDF…", "", "{path?}"),
    ("help.about", "About DrawCraft", "", "{}"),
    ("help.commandPalette", "Search Commands…", "Cmd+Shift+/", "{}"),
    ("app.quit", "Quit DrawCraft", "Cmd+Q", "{}"),
];

/// Handle a UI command. `None` = not a UI command (the engine handles it).
pub fn run_ui_command(app: &mut DrawcraftApp, id: &str, p: &Value) -> Option<Result<Value, String>> {
    let s = |k: &str| p.get(k).and_then(Value::as_str).map(str::to_string);
    let flag = |b: &mut bool| {
        *b = !*b;
        Ok(json!(*b))
    };
    let r = match id {
        "file.newDialog" => {
            app.ui.dialog = Some(crate::state::Dialog::new(
                "newDocument",
                json!({"preset": "Letter", "width": "612 pt", "height": "792 pt", "units": "Points", "artboards": 1, "colorMode": "RGB", "name": "Untitled-1"}),
            ));
            Ok(Value::Null)
        }
        "file.open" => match s("path") {
            Some(path) => io::open_path(app, &path).map(|_| Value::Null),
            None => io::open_dialog(app).map(|_| Value::Null),
        },
        "file.save" => io::save(app, s("path"), false).map(|p| json!({"path": p})),
        "file.saveAs" | "file.saveCopy" => io::save(app, s("path"), true).map(|p| json!({"path": p})),
        "file.revert" => {
            let path = app.session.active().and_then(|d| d.path.clone());
            match path {
                Some(path) => {
                    let i = app.session.active_index().unwrap_or(0);
                    app.session.close_document(i);
                    io::open_path(app, &path).map(|_| Value::Null)
                }
                None => Err("document has never been saved".into()),
            }
        }
        "file.place" => {
            let path = match s("path") {
                Some(p) => Some(p),
                None => app.services.pick_open.as_mut().and_then(|f| f()),
            };
            match path {
                Some(path) => {
                    let bytes = app.services.read.as_ref().ok_or("no reader".to_string()).and_then(|r| r(&path));
                    let name = std::path::Path::new(&path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                    bytes.and_then(|b| io::place_bytes(app, &name, &b)).map(|_| Value::Null)
                }
                None => Err("cancelled".into()),
            }
        }
        "file.export.svg" => io::export(app, "svg", s("path"), 1.0).map(|p| json!({"path": p})),
        "file.export.png" | "file.exportForScreens" => {
            io::export(app, "png", s("path"), p.get("scale").and_then(Value::as_f64).unwrap_or(1.0)).map(|p| json!({"path": p}))
        }
        "file.documentSetup" => {
            let units = app.session.active().map(|d| d.doc.units.label()).unwrap_or("Points");
            app.ui.dialog = Some(crate::state::Dialog::new("documentSetup", json!({"units": units})));
            Ok(Value::Null)
        }
        "edit.preferences" => {
            app.ui.dialog = Some(crate::state::Dialog::new(
                "preferences",
                json!({"keyboardIncrement": app.session.prefs.keyboard_increment, "scaleStrokes": app.session.prefs.scale_strokes}),
            ));
            Ok(Value::Null)
        }
        "view.outline" => flag(&mut app.ui.view.outline),
        "view.pixelPreview" => flag(&mut app.ui.view.pixel_preview),
        "view.edges" => flag(&mut app.ui.view.edges),
        "view.artboards" => flag(&mut app.ui.view.artboards),
        "view.rulers" => flag(&mut app.ui.view.rulers),
        "view.boundingBox" => flag(&mut app.ui.view.bounding_box),
        "view.transparencyGrid" => flag(&mut app.ui.view.transparency_grid),
        "view.guides" => flag(&mut app.ui.view.guides),
        "view.smartGuides" => flag(&mut app.ui.view.smart_guides),
        "view.grid" => flag(&mut app.ui.view.grid),
        "view.snapToGrid" => flag(&mut app.ui.view.snap_to_grid),
        "view.snapToPoint" => flag(&mut app.ui.view.snap_to_point),
        "view.zoomIn" | "view.zoomOut" => {
            let up = id == "view.zoomIn";
            match app.view_mut() {
                Some(v) => {
                    v.zoom = next_zoom(v.zoom, up);
                    Ok(json!({"zoom": v.zoom * 100.0}))
                }
                None => Err("no document".into()),
            }
        }
        "view.setZoom" => {
            let z = p.get("zoom").and_then(Value::as_f64).unwrap_or(100.0) / 100.0;
            let center =
                p.get("center").and_then(Value::as_array).and_then(|a| Some(drawcraft_geom::Point::new(a.first()?.as_f64()?, a.get(1)?.as_f64()?)));
            match app.view_mut() {
                Some(v) => {
                    v.zoom = z.clamp(0.0313, 640.0);
                    v.fitted = true;
                    if let Some(c) = center {
                        v.center = c;
                    }
                    Ok(Value::Null)
                }
                None => Err("no document".into()),
            }
        }
        "view.fitArtboard" | "view.fitAll" | "view.actualSize" => {
            crate::canvas::fit(app, id);
            Ok(Value::Null)
        }
        "view.presentation" => {
            app.ui.screen_mode = if app.ui.screen_mode == 3 { 0 } else { 3 };
            Ok(json!(app.ui.screen_mode))
        }
        "view.screenMode" => {
            app.ui.screen_mode = match p.get("mode").and_then(Value::as_u64) {
                Some(m) => m.min(3) as u8,
                None => (app.ui.screen_mode + 1) % 3,
            };
            Ok(json!(app.ui.screen_mode))
        }
        "view.rotateReset" => {
            if let Some(v) = app.view_mut() {
                v.rotation = 0.0;
            }
            Ok(Value::Null)
        }
        "window.control" => flag(&mut app.ui.control_bar),
        "window.toolbar" => flag(&mut app.ui.toolbar),
        "window.toolbarColumns" => flag(&mut app.ui.toolbar_double),
        "window.toolbarAdvanced" => flag(&mut app.ui.toolbar_advanced),
        "window.taskBar" => flag(&mut app.ui.task_bar),
        "window.dock" => {
            let on = !(app.ui.dock && app.ui.toolbar);
            app.ui.dock = on;
            app.ui.toolbar = on;
            app.ui.control_bar = on;
            Ok(json!(on))
        }
        "window.panel" => {
            let panel = s("panel").unwrap_or_default();
            match panel.as_str() {
                "properties" => {
                    app.ui.dock_tab = DockTab::Properties;
                    Ok(Value::Null)
                }
                "layers" => {
                    app.ui.dock_tab = DockTab::Layers;
                    Ok(Value::Null)
                }
                "libraries" => {
                    app.ui.dock_tab = DockTab::Libraries;
                    Ok(Value::Null)
                }
                p if ICON_PANELS.iter().any(|(id, _, _)| *id == p) => {
                    app.ui.open_panel = if app.ui.open_panel.as_deref() == Some(p) { None } else { Some(p.to_string()) };
                    app.ui.dock = true;
                    Ok(json!({"open": app.ui.open_panel}))
                }
                other => Err(format!("unknown panel `{other}`")),
            }
        }
        "window.brightness" => match s("brightness").as_deref().and_then(Brightness::parse) {
            Some(b) => {
                app.ui.brightness = b;
                app.canvas.key = None;
                Ok(json!(b.id()))
            }
            None => Err("brightness must be dark|mediumDark|mediumLight|light".into()),
        },
        "window.workspace.reset" => {
            let b = app.ui.brightness;
            app.ui = crate::state::UiState { brightness: b, ..Default::default() };
            Ok(Value::Null)
        }
        "window.newWindow" => Err("multiple windows land with M11.5".into()),
        "tool.select" => match s("tool") {
            Some(t) if drawcraft_tools::tool_info(&t).is_some() => {
                app.select_tool(&t);
                Ok(json!({"tool": app.session.tool_id()}))
            }
            Some(t) => Err(format!("unknown tool `{t}`")),
            None => Err("missing `tool`".into()),
        },
        "tool.setOption" => {
            let k = s("key").unwrap_or_default();
            app.session.set_tool_option(&k, p.get("value").unwrap_or(&Value::Null));
            Ok(app.session.tool_options())
        }
        "effect.dialog" => {
            let id = s("effect").unwrap_or_default();
            match drawcraft_effects::effect_catalog().into_iter().find(|e| e.id == id) {
                Some(e) => {
                    let mut fields = e.defaults.as_object().cloned().unwrap_or_default();
                    fields.insert("__effect".into(), json!(e.id));
                    fields.insert("__label".into(), json!(e.label.trim_end_matches('…')));
                    fields.insert("preview".into(), json!(true));
                    app.ui.dialog = Some(crate::state::Dialog { kind: "effect".into(), fields });
                    Ok(Value::Null)
                }
                None => Err(format!("unknown effect `{id}`")),
            }
        }
        "ui.paramDialog" => {
            let cmd = s("command").unwrap_or_default();
            let mut fields = p.get("params").and_then(Value::as_object).cloned().unwrap_or_default();
            fields.insert("__command".into(), json!(cmd));
            fields.insert("__label".into(), json!(s("label").unwrap_or(cmd.clone())));
            app.ui.dialog = Some(crate::state::Dialog { kind: "command".into(), fields });
            Ok(Value::Null)
        }
        "effect.applyLast" => match app.last_effect.clone() {
            Some((e, params)) => app.run("effect.apply", json!({"effect": e, "params": params})),
            None => Err("no effect applied yet".into()),
        },
        "file.export.pdf" => io::export(app, "pdf", s("path"), 1.0).map(|p| json!({"path": p})),
        "help.about" => {
            app.ui.about = true;
            Ok(Value::Null)
        }
        "help.commandPalette" => {
            app.ui.palette_open = !app.ui.palette_open;
            app.ui.palette_query.clear();
            Ok(Value::Null)
        }
        "app.quit" => {
            app.ui.status = "quit".into();
            Ok(Value::Null)
        }
        _ => return None,
    };
    Some(r)
}

/// Checked state for toggle items.
pub fn checked(app: &DrawcraftApp, id: &str, p: &Value) -> Option<bool> {
    let v = &app.ui.view;
    Some(match id {
        "view.outline" => v.outline,
        "view.pixelPreview" => v.pixel_preview,
        "view.smartGuides" => v.smart_guides,
        "view.grid" => v.grid,
        "view.snapToGrid" => v.snap_to_grid,
        "view.snapToPoint" => v.snap_to_point,
        "view.rulers" => v.rulers,
        "window.control" => app.ui.control_bar,
        "window.toolbar" => app.ui.toolbar,
        "window.toolbarAdvanced" => app.ui.toolbar_advanced,
        "window.taskBar" => app.ui.task_bar,
        "window.panel" => {
            let panel = p.get("panel").and_then(Value::as_str).unwrap_or("");
            match panel {
                "properties" => app.ui.dock_tab == DockTab::Properties,
                "layers" => app.ui.dock_tab == DockTab::Layers,
                "libraries" => app.ui.dock_tab == DockTab::Libraries,
                _ => app.ui.open_panel.as_deref() == Some(panel),
            }
        }
        "window.brightness" => p.get("brightness").and_then(Value::as_str).and_then(Brightness::parse) == Some(app.ui.brightness),
        "file.documentColorMode" => {
            let cmyk = app.session.active().is_some_and(|d| d.doc.color_mode == drawcraft_engine::doc::ColorMode::Cmyk);
            p.get("mode").and_then(Value::as_str) == Some(if cmyk { "cmyk" } else { "rgb" })
        }
        _ => return None,
    })
}

/// Label for toggles whose text flips (Outline/Preview, Hide/Show …).
pub fn dynamic_label(app: &DrawcraftApp, id: &str, label: &str) -> String {
    let v = &app.ui.view;
    match id {
        "view.outline" => if v.outline { "Preview" } else { "Outline" }.into(),
        "view.edges" => if v.edges { "Hide Edges" } else { "Show Edges" }.into(),
        "view.artboards" => if v.artboards { "Hide Artboards" } else { "Show Artboards" }.into(),
        "view.rulers" => if v.rulers { "Hide Rulers" } else { "Show Rulers" }.into(),
        "view.boundingBox" => if v.bounding_box { "Hide Bounding Box" } else { "Show Bounding Box" }.into(),
        "view.transparencyGrid" => if v.transparency_grid { "Hide Transparency Grid" } else { "Show Transparency Grid" }.into(),
        "view.guides" => if v.guides { "Hide Guides" } else { "Show Guides" }.into(),
        "view.grid" => if v.grid { "Hide Grid" } else { "Show Grid" }.into(),
        "view.guides.lock" => if app.session.guides_locked() { "Unlock Guides" } else { "Lock Guides" }.into(),
        "edit.undo" => app.session.active().and_then(|d| d.history.undo.last()).map(|h| format!("Undo {}", h.label)).unwrap_or_else(|| "Undo".into()),
        "edit.redo" => app.session.active().and_then(|d| d.history.redo.last()).map(|h| format!("Redo {}", h.label)).unwrap_or_else(|| "Redo".into()),
        _ => label.into(),
    }
}

pub fn shortcut_of(id: &str) -> Option<&'static str> {
    if let Some(c) = drawcraft_engine::find_command(id) {
        return c.shortcut;
    }
    UI_COMMANDS.iter().find(|c| c.0 == id).map(|c| c.2).filter(|s| !s.is_empty())
}

/// Is a command currently enabled?
pub fn enabled(app: &DrawcraftApp, id: &str) -> bool {
    if let Some(c) = drawcraft_engine::find_command(id) {
        return (c.enabled)(&app.session).is_ok();
    }
    match id {
        "file.save"
        | "file.saveAs"
        | "file.saveCopy"
        | "file.place"
        | "file.export.svg"
        | "file.export.png"
        | "file.exportForScreens"
        | "file.documentSetup"
        | "view.zoomIn"
        | "view.zoomOut"
        | "view.fitArtboard"
        | "view.fitAll"
        | "view.actualSize" => app.session.active().is_some(),
        "file.revert" => app.session.active().is_some_and(|d| d.path.is_some() && d.is_dirty()),
        "effect.dialog" => app.session.active().is_some_and(|d| !d.selection.is_empty()),
        "effect.applyLast" => app.last_effect.is_some() && app.session.active().is_some_and(|d| !d.selection.is_empty()),
        "file.export.pdf" => app.session.active().is_some(),
        _ => true,
    }
}

pub fn menu_tree() -> Vec<(&'static str, Vec<Item>)> {
    let panel = |label: &'static str, id: &'static str| cp(label, "window.panel", json!({ "panel": id }));
    vec![
        (
            "DrawCraft",
            vec![
                c("About DrawCraft", "help.about"),
                Sep,
                c("Settings…", "edit.preferences"),
                Sep,
                sub("UI Brightness", Brightness::ALL.iter().map(|b| cp(b.label(), "window.brightness", json!({"brightness": b.id()}))).collect()),
                Sep,
                c("Quit DrawCraft", "app.quit"),
            ],
        ),
        (
            "File",
            vec![
                c("New…", "file.newDialog"),
                todos("New from Template…", "Cmd+Shift+N"),
                c("Open…", "file.open"),
                todo("Open Recent Files"),
                Sep,
                c("Close", "file.close"),
                c("Close All", "file.closeAll"),
                c("Save", "file.save"),
                c("Save As…", "file.saveAs"),
                c("Save a Copy…", "file.saveCopy"),
                todo("Save as Template…"),
                c("Revert", "file.revert"),
                Sep,
                c("Place…", "file.place"),
                Sep,
                sub(
                    "Export",
                    vec![
                        c("Export for Screens…", "file.exportForScreens"),
                        c("Export As SVG…", "file.export.svg"),
                        c("Export As PNG…", "file.export.png"),
                        todos("Save for Web (Legacy)…", "Cmd+Alt+Shift+S"),
                    ],
                ),
                todo("Export Selection…"),
                Sep,
                todos("Package…", "Cmd+Alt+Shift+P"),
                sub("Scripts", vec![todos("Other Script…", "Cmd+F12")]),
                Sep,
                c("Document Setup…", "file.documentSetup"),
                sub(
                    "Document Color Mode",
                    vec![
                        cp("CMYK Color", "file.documentColorMode", json!({"mode": "cmyk"})),
                        cp("RGB Color", "file.documentColorMode", json!({"mode": "rgb"})),
                    ],
                ),
                c("File Info…", "file.info"),
                Sep,
                todos("Print…", "Cmd+P"),
            ],
        ),
        (
            "Edit",
            vec![
                c("Undo", "edit.undo"),
                c("Redo", "edit.redo"),
                Sep,
                c("Cut", "edit.cut"),
                c("Copy", "edit.copy"),
                c("Paste", "edit.paste"),
                c("Paste in Front", "edit.pasteInFront"),
                c("Paste in Back", "edit.pasteInBack"),
                c("Paste in Place", "edit.pasteInPlace"),
                c("Paste on All Artboards", "edit.pasteOnAllArtboards"),
                c("Paste without Formatting", "edit.pasteWithoutFormatting"),
                c("Clear", "edit.clear"),
                Sep,
                cp("Find and Replace…", "edit.findReplace", json!({"find": "", "replace": "", "matchCase": false, "wholeWord": false})),
                cp("Find Next", "edit.findNext", json!({"find": ""})),
                sub("Spelling", vec![todo("Auto Spell Check"), todos("Check Spelling…", "Cmd+I"), todo("Edit Custom Dictionary…")]),
                Sep,
                sub(
                    "Edit Colors",
                    vec![
                        todo("Recolor Artwork…"),
                        cp("Adjust Color Balance…", "edit.colors.adjustBalance", json!({"r": 0, "g": 0, "b": 0})),
                        c("Blend Front to Back", "edit.colors.blendFrontToBack"),
                        c("Blend Horizontally", "edit.colors.blendHorizontally"),
                        c("Blend Vertically", "edit.colors.blendVertically"),
                        c("Convert to CMYK", "edit.colors.toCMYK"),
                        c("Convert to Grayscale", "edit.colors.toGrayscale"),
                        c("Convert to RGB", "edit.colors.toRGB"),
                        c("Invert Colors", "edit.colors.invert"),
                        todo("Overprint Black…"),
                        cp("Saturate…", "edit.colors.saturate", json!({"intensity": 20})),
                    ],
                ),
                todo("Edit Original"),
                Sep,
                todo("Transparency Flattener Presets…"),
                todo("Print Presets…"),
                todo("Adobe PDF Presets…"),
                todo("Perspective Grid Presets…"),
                Sep,
                todos("Color Settings…", "Cmd+Shift+K"),
                todo("Assign Profile…"),
                Sep,
                todos("Keyboard Shortcuts…", "Cmd+Alt+Shift+K"),
                c("Preferences…", "edit.preferences"),
            ],
        ),
        (
            "Object",
            vec![
                sub(
                    "Transform",
                    vec![
                        c("Transform Again", "object.transformAgain"),
                        Sep,
                        c("Move…", "object.move"),
                        c("Rotate…", "object.rotate"),
                        c("Reflect…", "object.reflect"),
                        c("Scale…", "object.scale"),
                        c("Shear…", "object.shear"),
                        Sep,
                        cp(
                            "Transform Each…",
                            "object.transformEach",
                            json!({"scaleH": 100, "scaleV": 100, "moveH": 0, "moveV": 0, "rotate": 0, "reflectX": false, "reflectY": false, "random": false, "reference": 4}),
                        ),
                        Sep,
                        c("Reset Bounding Box", "object.resetBoundingBox"),
                    ],
                ),
                sub(
                    "Arrange",
                    vec![
                        c("Bring to Front", "object.arrange.bringToFront"),
                        c("Bring Forward", "object.arrange.bringForward"),
                        c("Send Backward", "object.arrange.sendBackward"),
                        c("Send to Back", "object.arrange.sendToBack"),
                        Sep,
                        c("Send to Current Layer", "object.arrange.sendToCurrentLayer"),
                    ],
                ),
                sub(
                    "Align",
                    vec![
                        cp("Horizontal Align Left", "object.align", json!({"horizontal": "left"})),
                        cp("Horizontal Align Center", "object.align", json!({"horizontal": "center"})),
                        cp("Horizontal Align Right", "object.align", json!({"horizontal": "right"})),
                        cp("Vertical Align Top", "object.align", json!({"vertical": "top"})),
                        cp("Vertical Align Center", "object.align", json!({"vertical": "center"})),
                        cp("Vertical Align Bottom", "object.align", json!({"vertical": "bottom"})),
                    ],
                ),
                Sep,
                c("Group", "object.group"),
                c("Ungroup", "object.ungroup"),
                sub(
                    "Lock",
                    vec![c("Selection", "object.lock"), c("All Artwork Above", "object.lock.above"), c("Other Layers", "object.lock.otherLayers")],
                ),
                c("Unlock All", "object.unlockAll"),
                sub(
                    "Hide",
                    vec![c("Selection", "object.hide"), c("All Artwork Above", "object.hide.above"), c("Other Layers", "object.hide.otherLayers")],
                ),
                c("Show All", "object.showAll"),
                Sep,
                cp("Expand…", "object.expand", json!({"object": true, "fill": true, "stroke": true})),
                c("Expand Appearance", "effect.expandAppearance"),
                c("Crop Image", "object.cropImage"),
                cp("Rasterize…", "object.rasterize", json!({"ppi": 72, "background": "transparent"})),
                todo("Create Gradient Mesh…"),
                todo("Create Object Mosaic…"),
                c("Create Trim Marks", "object.createTrimMarks"),
                todo("Flatten Transparency…"),
                Sep,
                todo("Make Pixel Perfect"),
                Sep,
                sub("Slice", vec![todo("Make"), todo("Release"), todo("Create from Guides"), todo("Create from Selection")]),
                Sep,
                sub(
                    "Path",
                    vec![
                        c("Join", "path.join"),
                        c("Average…", "path.average"),
                        Sep,
                        c("Outline Stroke", "object.path.outlineStroke"),
                        c("Offset Path…", "object.path.offsetPath"),
                        c("Reverse Path Direction", "path.reverse"),
                        Sep,
                        c("Simplify…", "object.path.simplify"),
                        c("Add Anchor Points", "object.path.addAnchorPoints"),
                        c("Remove Anchor Points", "path.deleteAnchors"),
                        c("Divide Objects Below", "object.path.divideObjectsBelow"),
                        c("Split Into Grid…", "object.path.splitIntoGrid"),
                        Sep,
                        cp("Clean Up…", "object.path.cleanUp", json!({"strayPoints": true, "unpaintedObjects": true, "emptyTextPaths": true})),
                    ],
                ),
                sub("Shape", vec![c("Convert to Shape", "object.shape.convertToShape"), c("Expand Shape", "object.expandShape")]),
                sub("Pattern", vec![todo("Make"), todo("Edit Pattern"), todo("Tile Edge Color…")]),
                sub("Repeat", vec![todo("Radial"), todo("Grid"), todo("Mirror"), Sep, todo("Release"), todo("Options…")]),
                sub(
                    "Blend",
                    vec![
                        c("Make", "object.blend.make"),
                        c("Release", "object.blend.release"),
                        Sep,
                        cp("Blend Options…", "object.blend.options", json!({"steps": 5})),
                        Sep,
                        c("Expand", "object.blend.expand"),
                        Sep,
                        todo("Replace Spine"),
                        c("Reverse Spine", "object.blend.reverseSpine"),
                        c("Reverse Front to Back", "object.blend.reverseFrontToBack"),
                    ],
                ),
                sub(
                    "Envelope Distort",
                    vec![
                        todos("Make with Warp…", "Cmd+Alt+Shift+W"),
                        todos("Make with Mesh…", "Cmd+Alt+M"),
                        todos("Make with Top Object", "Cmd+Alt+C"),
                        Sep,
                        todo("Release"),
                        todo("Envelope Options…"),
                        todo("Expand"),
                    ],
                ),
                sub("Perspective", vec![todo("Attach to Active Plane"), todo("Release with Perspective")]),
                sub("Live Paint", vec![todos("Make", "Cmd+Alt+X"), todo("Merge"), todo("Release"), Sep, todo("Gap Options…"), Sep, todo("Expand")]),
                sub("Image Trace", vec![todo("Make"), todo("Make and Expand"), todo("Release"), todo("Expand")]),
                sub("Text Wrap", vec![todo("Make"), todo("Release"), todo("Text Wrap Options…")]),
                Sep,
                sub("Clipping Mask", vec![c("Make", "object.clippingMask.make"), c("Release", "object.clippingMask.release"), todo("Edit Contents")]),
                sub("Compound Path", vec![c("Make", "object.compoundPath.make"), c("Release", "object.compoundPath.release")]),
                sub(
                    "Artboards",
                    vec![
                        c("Convert to Artboards", "artboard.convertToArtboards"),
                        cp("Rearrange All Artboards…", "artboard.rearrange", json!({"columns": 2, "spacing": 20, "moveArtwork": true})),
                        Sep,
                        c("Fit to Artwork Bounds", "artboard.fitToArt"),
                        c("Fit to Selected Art", "artboard.fitToSelection"),
                    ],
                ),
                sub("Graph", vec![todo("Type…"), todo("Data…"), todo("Design…"), todo("Column…"), todo("Marker…")]),
            ],
        ),
        (
            "Type",
            vec![
                sub("Font", font_items()),
                todo("Recent Fonts"),
                sub("Size", TYPE_SIZES.iter().map(|(l, n)| cp(l, "text.setStyle", json!({ "size": n }))).collect()),
                Sep,
                todo("Glyphs"),
                sub("Insert Special Character", insert_items(INSERT_SPECIAL)),
                sub("Insert Whitespace Character", insert_items(INSERT_WHITESPACE)),
                sub("Insert Break Character", insert_items(INSERT_BREAK)),
                Sep,
                c("Convert To Area Type", "type.convertToAreaType"),
                c("Convert To Point Type", "type.convertToPointType"),
                todo("Area Type Options…"),
                sub(
                    "Type on a Path",
                    vec![
                        todo("Rainbow"),
                        todo("Skew"),
                        todo("3D Ribbon"),
                        todo("Stair Step"),
                        todo("Gravity"),
                        Sep,
                        cp("Type on a Path Options…", "type.pathOptions", json!({"start": 0})),
                    ],
                ),
                sub("Threaded Text", vec![todo("Create"), todo("Release Selection"), todo("Remove Threading")]),
                todo("Fit Headline"),
                Sep,
                todo("Find Font…"),
                sub(
                    "Change Case",
                    vec![
                        cp("UPPERCASE", "type.changeCase", json!({"case": "upper"})),
                        cp("lowercase", "type.changeCase", json!({"case": "lower"})),
                        cp("Title Case", "type.changeCase", json!({"case": "title"})),
                        cp("Sentence case", "type.changeCase", json!({"case": "sentence"})),
                    ],
                ),
                cp("Smart Punctuation…", "type.smartPunctuation", json!({"quotes": true, "dashes": true, "ellipsis": true, "scope": "selection"})),
                Sep,
                c("Create Outlines", "type.createOutlines"),
                todo("Optical Margin Alignment"),
                Sep,
                c("Fill with Placeholder Text", "type.fillPlaceholder"),
                Sep,
                todos("Show Hidden Characters", "Cmd+Alt+I"),
                sub("Type Orientation", vec![todo("Horizontal"), todo("Vertical")]),
            ],
        ),
        (
            "Select",
            vec![
                c("All", "select.all"),
                c("All on Active Artboard", "select.allOnArtboard"),
                c("Deselect", "select.none"),
                c("Reselect", "select.reselect"),
                Sep,
                c("Inverse", "select.inverse"),
                Sep,
                c("Next Object Above", "select.nextAbove"),
                c("Next Object Below", "select.nextBelow"),
                Sep,
                sub(
                    "Same",
                    vec![
                        c("Appearance", "select.same.appearance"),
                        todo("Appearance Attribute"),
                        c("Blending Mode", "select.same.blendingMode"),
                        c("Fill & Stroke", "select.same.fillAndStroke"),
                        c("Fill Color", "select.same.fillColor"),
                        c("Opacity", "select.same.opacity"),
                        c("Stroke Color", "select.same.strokeColor"),
                        c("Stroke Weight", "select.same.strokeWeight"),
                        todo("Graphic Style"),
                        c("Shape", "select.same.shapeType"),
                        c("Symbol Instance", "select.same.symbolInstance"),
                        Sep,
                        c("Font Family", "select.same.fontFamily"),
                        c("Font Family & Style", "select.same.fontFamilyStyle"),
                        c("Font Family, Style & Size", "select.same.fontFamilyStyleSize"),
                        c("Font Size", "select.same.fontSize"),
                        c("Text Fill Color", "select.same.textFillColor"),
                        c("Text Stroke Color", "select.same.textStrokeColor"),
                    ],
                ),
                sub(
                    "Object",
                    vec![
                        c("All on Same Layers", "select.object.allOnSameLayers"),
                        c("Direction Handles", "select.object.directionHandles"),
                        Sep,
                        c("Brush Strokes", "select.object.brushStrokes"),
                        c("Bristle Brush Strokes", "select.object.bristleBrushStrokes"),
                        c("Clipping Masks", "select.object.clippingMasks"),
                        c("Stray Points", "select.object.strayPoints"),
                        c("Open Paths", "select.object.openPaths"),
                        Sep,
                        c("All Text Objects", "select.object.textObjects"),
                        c("Point Text Objects", "select.object.pointText"),
                        c("Area Text Objects", "select.object.areaText"),
                    ],
                ),
                todo("Start Global Edit"),
                Sep,
                c("Save Selection…", "select.save"),
                todo("Edit Selection…"),
            ],
        ),
        ("Effect", effect_menu()),
        (
            "View",
            vec![
                c("Outline", "view.outline"),
                todos("Overprint Preview", "Cmd+Alt+Shift+Y"),
                c("Pixel Preview", "view.pixelPreview"),
                todo("Trim View"),
                c("Presentation Mode", "view.presentation"),
                sub(
                    "Screen Mode",
                    vec![
                        cp("Normal Screen Mode", "view.screenMode", json!({"mode": 0})),
                        cp("Full Screen Mode with Menu Bar", "view.screenMode", json!({"mode": 1})),
                        cp("Full Screen Mode", "view.screenMode", json!({"mode": 2})),
                    ],
                ),
                Sep,
                sub(
                    "Proof Setup",
                    vec![
                        todo("Working CMYK"),
                        todo("Internet Standard RGB (sRGB)"),
                        todo("Color blindness – Protanopia-type"),
                        todo("Color blindness – Deuteranopia-type"),
                    ],
                ),
                todo("Proof Colors"),
                Sep,
                c("Zoom In", "view.zoomIn"),
                c("Zoom Out", "view.zoomOut"),
                c("Fit Artboard in Window", "view.fitArtboard"),
                c("Fit All in Window", "view.fitAll"),
                c("Actual Size", "view.actualSize"),
                Sep,
                c("Reset Rotate View", "view.rotateReset"),
                Sep,
                c("Hide Edges", "view.edges"),
                c("Hide Artboards", "view.artboards"),
                todo("Show Print Tiling"),
                Sep,
                sub("Rulers", vec![c("Show Rulers", "view.rulers"), todos("Change to Global Rulers", "Cmd+Alt+R"), todo("Show Video Rulers")]),
                c("Hide Bounding Box", "view.boundingBox"),
                c("Show Transparency Grid", "view.transparencyGrid"),
                todos("Hide Text Threads", "Cmd+Shift+Y"),
                todos("Hide Gradient Annotator", "Cmd+Alt+G"),
                todo("Hide Corner Widget"),
                Sep,
                sub(
                    "Guides",
                    vec![
                        c("Hide Guides", "view.guides"),
                        c("Lock Guides", "view.guides.lock"),
                        c("Make Guides", "view.guides.make"),
                        c("Release Guides", "view.guides.release"),
                        c("Clear Guides", "view.guides.clear"),
                    ],
                ),
                c("Smart Guides", "view.smartGuides"),
                sub("Perspective Grid", vec![todos("Show Grid", "Cmd+Shift+I"), todo("Define Grid…")]),
                c("Show Grid", "view.grid"),
                c("Snap to Grid", "view.snapToGrid"),
                todo("Snap to Pixel"),
                c("Snap to Point", "view.snapToPoint"),
                todo("Snap to Glyph"),
                Sep,
                todo("New View…"),
                todo("Edit Views…"),
            ],
        ),
        (
            "Window",
            vec![
                c("New Window", "window.newWindow"),
                sub("Arrange", vec![todo("Cascade"), todo("Tile"), todo("Float in Window"), todo("Consolidate All Windows")]),
                sub(
                    "Workspace",
                    vec![
                        todo("Essentials"),
                        todo("Essentials Classic"),
                        todo("Painting"),
                        todo("Typography"),
                        Sep,
                        c("Reset Essentials", "window.workspace.reset"),
                    ],
                ),
                Sep,
                c("Control", "window.control"),
                c("Contextual Task Bar", "window.taskBar"),
                c("Tools", "window.toolbar"),
                sub("Toolbars", vec![c("Advanced", "window.toolbarAdvanced"), c("Single / Double Column", "window.toolbarColumns")]),
                Sep,
                todo("Actions"),
                panel("Align", "align"),
                panel("Appearance", "appearance"),
                panel("Artboards", "artboards"),
                todo("Asset Export"),
                todo("Attributes"),
                panel("Brushes", "brushes"),
                panel("Color", "color"),
                panel("Color Guide", "colorGuide"),
                todo("Document Info"),
                panel("Gradient", "gradient"),
                panel("Graphic Styles", "graphicStyles"),
                panel("History", "history"),
                todo("Image Trace"),
                panel("Info", "info"),
                panel("Layers", "layers"),
                panel("Libraries", "libraries"),
                todo("Links"),
                todo("Magic Wand"),
                panel("Navigator", "navigator"),
                panel("Pathfinder", "pathfinder"),
                todo("Pattern Options"),
                panel("Properties", "properties"),
                todo("Separations Preview"),
                panel("Stroke", "stroke"),
                todo("SVG Interactivity"),
                panel("Swatches", "swatches"),
                panel("Symbols", "symbols"),
                panel("Transform", "transform"),
                panel("Transparency", "transparency"),
                sub(
                    "Type",
                    vec![
                        panel("Character", "character"),
                        todo("Character Styles"),
                        todo("Glyphs"),
                        todo("OpenType"),
                        panel("Paragraph", "paragraph"),
                        todo("Paragraph Styles"),
                        todo("Tabs"),
                    ],
                ),
                todo("Variables"),
                Sep,
                sub("Brush Libraries", vec![todo("Arrows"), todo("Artistic"), todo("Borders"), todo("Decorative")]),
                sub("Graphic Style Libraries", vec![todo("Additive"), todo("Artistic Effects"), todo("Buttons and Rollovers")]),
                sub("Swatch Libraries", vec![todo("Art History"), todo("Celebration"), todo("Color Properties"), todo("Nature"), todo("Web")]),
                sub("Symbol Libraries", vec![todo("Arrows"), todo("Charts"), todo("Web Buttons and Bars")]),
            ],
        ),
        ("Help", vec![c("Search Commands…", "help.commandPalette"), todos("DrawCraft Help…", "F1"), Sep, c("About DrawCraft", "help.about")]),
    ]
}

/// Render the menu bar.
pub fn menu_bar(app: &mut DrawcraftApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let mut clicked: Option<(String, Value)> = None;
    let tree = menu_tree();
    egui::MenuBar::new().ui(ui, |ui| {
        for (i, (title, items)) in tree.iter().enumerate() {
            let text = if i == 0 {
                egui::RichText::new(*title).font(theme::semibold(13.0)).color(t.text)
            } else {
                egui::RichText::new(*title).size(13.0).color(t.text)
            };
            ui.menu_button(text, |ui| {
                ui.set_min_width(230.0);
                render_items(app, ui, items, &mut clicked);
            });
        }
    });
    if let Some((id, p)) = clicked {
        invoke(app, &id, p);
    }
}

fn render_items(app: &DrawcraftApp, ui: &mut egui::Ui, items: &[Item], clicked: &mut Option<(String, Value)>) {
    let t = Tokens::get(ui.ctx());
    for it in items {
        match it {
            Item::Sep => {
                ui.separator();
            }
            Item::Header(h) => {
                ui.label(egui::RichText::new(*h).size(11.0).color(t.text_dim));
            }
            Item::Sub(label, children) => {
                ui.menu_button(*label, |ui| {
                    ui.set_min_width(200.0);
                    render_items(app, ui, children, clicked);
                });
            }
            Item::Todo(label, sc) => {
                ui.add_enabled_ui(false, |ui| {
                    ui.add(egui::Button::new(*label).shortcut_text(pretty_shortcut(sc)).min_size(egui::vec2(ui.available_width(), 0.0)));
                })
                .response
                .on_disabled_hover_text("Coming soon — tracked in the parity plan");
            }
            Item::Cmd(label, id, p) => {
                let en = enabled(app, id);
                let label = dynamic_label(app, id, label);
                let sc = if p.is_null() { shortcut_of(id).map(pretty_shortcut).unwrap_or_default() } else { String::new() };
                let chk = checked(app, id, p);
                let text = match chk {
                    Some(true) => format!("✓  {label}"),
                    Some(false) => format!("     {label}"),
                    None => label,
                };
                let r = ui.add_enabled(en, egui::Button::new(text).shortcut_text(sc).min_size(egui::vec2(ui.available_width(), 0.0)));
                if r.clicked() {
                    *clicked = Some(click_target(label_of(it), id, p));
                    ui.close();
                }
            }
        }
    }
}

fn label_of(it: &Item) -> &'static str {
    match it {
        Item::Cmd(l, _, _) => l,
        _ => "",
    }
}

/// What a menu click runs: items whose label ends with "…" and that carry default params open a
/// generic parameter dialog (fields = the defaults) instead of running immediately.
pub fn click_target(label: &str, id: &str, p: &Value) -> (String, Value) {
    let dialog = label.ends_with('…')
        && p.as_object().is_some_and(|o| !o.is_empty())
        && !matches!(id, "effect.dialog" | "window.panel" | "window.brightness" | "view.screenMode");
    if dialog {
        return ("ui.paramDialog".into(), json!({"command": id, "label": label.trim_end_matches('…'), "params": p}));
    }
    (id.to_string(), if p.is_null() { json!({}) } else { p.clone() })
}

/// Invoke a menu/command id with UI side effects (dialogs for "…" commands that need input).
pub fn invoke(app: &mut DrawcraftApp, id: &str, p: Value) {
    // Commands whose menu item opens a dialog in Illustrator.
    let dialog = match id {
        "object.move" => Some(("move", json!({"dx": "0 pt", "dy": "0 pt"}))),
        "object.rotate" => Some(("rotate", json!({"angle": 0}))),
        "object.scale" => Some(("scale", json!({"sx": 100, "sy": 100, "uniform": true}))),
        "object.reflect" => Some(("reflect", json!({"axis": "vertical"}))),
        "object.shear" => Some(("shear", json!({"angle": 0, "axis": "horizontal"}))),
        "path.average" => Some(("average", json!({"axis": "both"}))),
        "object.path.offsetPath" => Some(("offsetPath", json!({"offset": "10 pt", "joins": "miter", "miterLimit": 4}))),
        "object.path.simplify" => Some(("simplify", json!({"tolerance": "1 pt"}))),
        "object.path.splitIntoGrid" => Some(("splitIntoGrid", json!({"rows": 2, "columns": 2, "gutter": "12 pt"}))),
        _ => None,
    };
    if let Some((kind, fields)) = dialog
        && p.as_object().is_none_or(|o| o.is_empty())
    {
        app.ui.dialog = Some(crate::state::Dialog::new(kind, fields));
        return;
    }
    if let Err(e) = app.run(id, p) {
        app.status(e);
    }
}

/// `Cmd+Shift+]` → `⇧⌘]` on macOS, `Ctrl+Shift+]` elsewhere.
pub fn pretty_shortcut(s: &str) -> String {
    if s.is_empty() {
        return String::new();
    }
    if cfg!(target_os = "macos") {
        let mut mods = String::new();
        let parts: Vec<&str> = s.split('+').collect();
        let (key, ms) = if s.ends_with("++") { ("+", &parts[..parts.len() - 2]) } else { (*parts.last().unwrap(), &parts[..parts.len() - 1]) };
        for m in ["Ctrl", "Alt", "Shift", "Cmd"] {
            if ms.contains(&m) {
                mods.push_str(match m {
                    "Ctrl" => "⌃",
                    "Alt" => "⌥",
                    "Shift" => "⇧",
                    _ => "⌘",
                });
            }
        }
        format!("{mods}{key}")
    } else {
        s.replace("Cmd", "Ctrl")
    }
}

#[derive(Serialize)]
pub struct MenuEntry {
    pub path: Vec<String>,
    pub label: String,
    pub command: Option<String>,
    pub params: Value,
    pub enabled: bool,
    pub shortcut: String,
}

/// Flattened menu for `ui.menu.list`.
pub fn menu_entries(app: &DrawcraftApp) -> Vec<MenuEntry> {
    fn walk(app: &DrawcraftApp, path: Vec<String>, items: &[Item], out: &mut Vec<MenuEntry>) {
        for it in items {
            match it {
                Item::Cmd(l, id, p) => out.push(MenuEntry {
                    path: path.clone(),
                    label: dynamic_label(app, id, l),
                    command: Some(id.to_string()),
                    params: p.clone(),
                    enabled: enabled(app, id),
                    shortcut: shortcut_of(id).unwrap_or("").to_string(),
                }),
                Item::Todo(l, sc) => out.push(MenuEntry {
                    path: path.clone(),
                    label: l.to_string(),
                    command: None,
                    params: Value::Null,
                    enabled: false,
                    shortcut: sc.to_string(),
                }),
                Item::Sub(l, ch) => {
                    let mut p = path.clone();
                    p.push(l.to_string());
                    walk(app, p, ch, out);
                }
                _ => {}
            }
        }
    }
    let mut out = vec![];
    for (title, items) in menu_tree() {
        walk(app, vec![title.to_string()], &items, &mut out);
    }
    out
}

/// Type → Size presets.
const TYPE_SIZES: [(&str, u32); 14] = [
    ("6 pt", 6),
    ("8 pt", 8),
    ("9 pt", 9),
    ("10 pt", 10),
    ("11 pt", 11),
    ("12 pt", 12),
    ("14 pt", 14),
    ("18 pt", 18),
    ("24 pt", 24),
    ("30 pt", 30),
    ("36 pt", 36),
    ("48 pt", 48),
    ("60 pt", 60),
    ("72 pt", 72),
];

const INSERT_SPECIAL: &[(&str, &str)] = &[
    ("Bullet", "bullet"),
    ("Copyright Symbol", "copyright"),
    ("Ellipsis", "ellipsis"),
    ("Paragraph Symbol", "paragraph"),
    ("Registered Trademark Symbol", "registered"),
    ("Section Symbol", "section"),
    ("Trademark Symbol", "trademark"),
    ("Em Dash", "emDash"),
    ("En Dash", "enDash"),
    ("Discretionary Hyphen", "discretionaryHyphen"),
    ("Nonbreaking Hyphen", "nonBreakingHyphen"),
    ("Double Left Quotation Marks", "doubleLeftQuote"),
    ("Double Right Quotation Marks", "doubleRightQuote"),
    ("Single Left Quotation Mark", "singleLeftQuote"),
    ("Single Right Quotation Mark", "singleRightQuote"),
];
const INSERT_WHITESPACE: &[(&str, &str)] = &[
    ("Em Space", "emSpace"),
    ("En Space", "enSpace"),
    ("Hair Space", "hairSpace"),
    ("Sixth Space", "sixthSpace"),
    ("Thin Space", "thinSpace"),
    ("Nonbreaking Space", "nonBreakingSpace"),
    ("Figure Space", "figureSpace"),
    ("Punctuation Space", "punctuationSpace"),
    ("Third Space", "thirdSpace"),
    ("Quarter Space", "quarterSpace"),
];
const INSERT_BREAK: &[(&str, &str)] = &[("Forced Line Break", "forcedLineBreak"), ("Tab", "tab")];

fn insert_items(list: &[(&'static str, &'static str)]) -> Vec<Item> {
    list.iter().map(|(l, c)| cp(l, "type.insert", json!({ "char": c }))).collect()
}

/// Type → Font: one item per installed family (labels interned once so the per-frame menu build
/// doesn't allocate forever).
fn font_items() -> Vec<Item> {
    static NAMES: std::sync::Mutex<Vec<&'static str>> = std::sync::Mutex::new(Vec::new());
    let fams = drawcraft_text::FontDb::global().families();
    let Ok(mut names) = NAMES.lock() else { return vec![] };
    fams.iter()
        .map(|f| {
            let label: &'static str = match names.iter().find(|n| **n == f.as_str()) {
                Some(n) => n,
                None => {
                    let n: &'static str = Box::leak(f.clone().into_boxed_str());
                    names.push(n);
                    n
                }
            };
            cp(label, "text.setStyle", json!({ "font": label }))
        })
        .collect()
}

/// The Effect menu, built from the effects catalogue (Illustrator Effects), plus raster effects.
fn effect_menu() -> Vec<Item> {
    let cat = drawcraft_effects::effect_catalog();
    let mut out = vec![
        c("Apply Last Effect", "effect.applyLast"),
        todos("Last Effect", "Cmd+Alt+Shift+E"),
        Sep,
        todo("Document Raster Effects Settings…"),
        Sep,
        Item::Header("Illustrator Effects"),
    ];
    // Submenus in Illustrator's order.
    let order = ["3D and Materials", "Convert to Shape", "Distort & Transform", "Path", "Pathfinder", "Stylize", "SVG Filters", "Warp", "Blur"];
    for sub_name in order {
        let items: Vec<Item> = cat
            .iter()
            .filter(|e| e.menu.last().copied() == Some(sub_name))
            .map(|e| Item::Cmd(e.label, "effect.dialog", json!({ "effect": e.id })))
            .collect();
        if sub_name == "Blur" {
            if !items.is_empty() {
                out.push(Sep);
                out.push(Item::Header("Raster Effects"));
                out.push(sub(sub_name, items));
            }
            continue;
        }
        if items.is_empty() {
            let placeholder = match sub_name {
                "3D and Materials" => vec![todo("Extrude & Bevel…"), todo("Revolve…"), todo("Inflate…"), todo("Rotate…"), todo("Materials…")],
                "Pathfinder" => vec![todo("Add"), todo("Intersect"), todo("Exclude"), todo("Subtract")],
                "SVG Filters" => vec![todo("Apply SVG Filter…")],
                _ => continue,
            };
            out.push(sub(sub_name, placeholder));
        } else {
            out.push(sub(sub_name, items));
        }
    }
    // Anything not placed above (future effects) still shows up.
    for e in cat.iter().filter(|e| !e.menu.last().is_some_and(|m| order.contains(m))) {
        out.push(Item::Cmd(e.label, "effect.dialog", json!({ "effect": e.id })));
    }
    out.push(Sep);
    out.push(c("Expand Appearance", "effect.expandAppearance"));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_bound_menu_command_exists() {
        fn walk(items: &[Item], bad: &mut Vec<String>) {
            for it in items {
                match it {
                    Item::Cmd(_, id, _) => {
                        if drawcraft_engine::find_command(id).is_none()
                            && !UI_COMMANDS.iter().any(|c| c.0 == *id)
                            && !id.starts_with("object.path.")
                            && *id != "type.createOutlines"
                        {
                            bad.push(id.to_string());
                        }
                    }
                    Item::Sub(_, ch) => walk(ch, bad),
                    _ => {}
                }
            }
        }
        let mut bad = vec![];
        for (_, items) in menu_tree() {
            walk(&items, &mut bad);
        }
        assert!(bad.is_empty(), "menu items bound to unknown commands: {bad:?}");
    }

    #[test]
    fn shortcut_pretty() {
        if cfg!(target_os = "macos") {
            assert_eq!(pretty_shortcut("Cmd+Shift+]"), "⇧⌘]");
            assert_eq!(pretty_shortcut("Cmd+Alt+2"), "⌥⌘2");
        }
        assert_eq!(pretty_shortcut(""), "");
    }
}
