//! The command palette (⌘K): type what you want to do in plain words and run it.
//!
//! It lists every command a person can run from the menus, every tool and every panel, each with
//! where it lives in the menus and its shortcut. Matches are ranked by relevance with craft-ui's
//! scorer, and a small table of everyday words ("change color", "round corners", "bigger") points
//! at the commands people mean. Agent and debug commands (`document.inspect`, `document.json`…)
//! are left out. With nothing typed it shows what was run last, then a few popular commands.
//!
//! Speed: the entries are built when the language, the plug-ins or the shortcuts change, and the
//! ranking only when the query changes; a frame only draws the dozen rows shown.

use std::sync::Arc;

use craft_ui::palette::score;
use egui::{Key, Sense, vec2};
use serde_json::json;

use crate::theme::Tokens;
use crate::{VectorcraftApp, icons, menus};

/// How many commands the palette remembers as recently used.
pub const MAX_RECENT: usize = 8;
/// How many of them show while nothing is typed.
const SHOWN_RECENT: usize = 5;
/// How many matches show.
const MAX_ROWS: usize = 12;

/// Commands for agents and debugging, never offered to people.
const INTERNAL: &[&str] = &[
    "document.inspect",
    "document.json",
    "document.node",
    "document.activate",
    "document.open",
    "document.save",
    "document.export",
    "file.recentFiles",
    "prefs.get",
    "prefs.set",
    "prefs.list",
    "shortcuts.set",
    "shortcuts.list",
    "shortcuts.conflicts",
    "shortcuts.preset",
    "tool.select",
    "tool.setOption",
    "select.set",
];

/// Everyday words → what people mean by them, best first (palette keys: a command id,
/// `tool:<id>` or `panel:<id>`). A query that is one of the words, starts one (3 letters or
/// more) or contains one as whole words ranks these above every other match. Keep it small;
/// the words are English (other languages match the translated labels).
pub const SYNONYMS: &[(&[&str], &[&str])] = &[
    (
        &["change color", "change colour", "color", "colour", "fill", "fill color", "recolor", "paint"],
        &["ui.colorPicker", "panel:color", "panel:swatches"],
    ),
    (&["round corners", "rounded corners", "corners", "round"], &["ui.corners", "tool:roundedRectangle"]),
    (
        &["combine shapes", "combine", "merge", "merge shapes", "join shapes", "unite", "union", "weld"],
        &["object.pathfinder.unite", "panel:pathfinder", "tool:shapeBuilder"],
    ),
    (&["cut out", "subtract", "punch out", "remove shape", "minus"], &["object.pathfinder.minusFront", "panel:pathfinder"]),
    (&["outline", "border", "stroke"], &["panel:stroke", "object.path.outlineStroke"]),
    (&["line thickness", "line width", "thickness", "stroke weight", "thicker", "thinner"], &["panel:stroke"]),
    (&["curve", "curves", "bezier", "draw path", "draw line"], &["tool:pen", "tool:curvature", "tool:pencil"]),
    (&["erase", "eraser", "rubber"], &["tool:eraser", "tool:pathEraser"]),
    (&["text", "words", "write", "type", "add text"], &["tool:type", "panel:character"]),
    (&["picture", "photo", "image", "import", "insert image"], &["file.place"]),
    (&["group"], &["object.group", "object.ungroup"]),
    (&["align", "line up", "center", "centre", "middle", "distribute"], &["panel:align"]),
    (&["flip", "mirror"], &["object.reflect", "tool:reflect"]),
    (&["rotate", "turn", "spin"], &["object.rotate", "tool:rotate"]),
    (&["bigger", "smaller", "resize", "enlarge", "shrink", "make bigger", "make smaller"], &["object.scale", "tool:scale"]),
    (&["undo", "go back", "oops"], &["edit.undo", "panel:history"]),
];

/// What the palette offers while nothing is typed, after the recently used ones.
pub const POPULAR: &[&str] =
    &["file.newDialog", "file.place", "ui.colorPicker", "tool:type", "tool:pen", "object.group", "panel:align", "file.exportAs"];

/// Everything the palette searches: (label, palette key, shortcut), in English.
pub fn items() -> Vec<(String, String, String)> {
    build(crate::i18n::Lang::EN).into_iter().map(|e| (e.label, e.key, e.shortcut)).collect()
}

/// One palette entry, with what it shows and what a query matches worked out once.
#[derive(Clone, Debug)]
pub struct Entry {
    /// What running it does: a command id, `tool:<id>` or `panel:<id>`.
    pub key: String,
    /// The label in the UI language.
    pub label: String,
    /// Where it lives ("Object › Path"), in the UI language.
    pub detail: String,
    /// The shortcut as stored ("Cmd+G"; empty: none).
    pub shortcut: String,
    /// Lowercased: the shown label, the English label.
    hay_label: [String; 2],
    /// Lowercased: the everyday words that find it and its id.
    hay_keywords: String,
    /// Lowercased: where it lives, shown and in English.
    hay_detail: String,
}

/// Where each command and panel shows in the menus: key → (English label, English menu path).
/// The first place wins, and only items that run a command as it is (no params), or show a panel.
type Places = std::collections::HashMap<String, (&'static str, Vec<&'static str>)>;

fn menu_places() -> Places {
    fn walk(items: &[menus::Item], path: &mut Vec<&'static str>, out: &mut Places, depth: usize) {
        if depth > 8 {
            return;
        }
        for it in items {
            match it {
                menus::Item::Cmd(label, "window.panel", p) => {
                    if let Some(panel) = p.get("panel").and_then(serde_json::Value::as_str) {
                        out.entry(format!("panel:{panel}")).or_insert_with(|| (*label, path.clone()));
                    }
                }
                menus::Item::Cmd(label, id, p) if p.is_null() || p.as_object().is_some_and(|o| o.is_empty()) => {
                    out.entry(id.to_string()).or_insert_with(|| (*label, path.clone()));
                }
                menus::Item::Sub(label, children) => {
                    path.push(*label);
                    walk(children, path, out, depth + 1);
                    path.pop();
                }
                _ => {}
            }
        }
    }
    let mut out = Places::new();
    for (title, items) in menus::menu_tree() {
        let mut path = vec![title];
        walk(&items, &mut path, &mut out, 0);
    }
    out
}

/// Whether a command is offered at all: people's commands are in the menus, have a menu path or
/// a shortcut, or are what an everyday word means; agent, debug and placeholder ones aren't.
fn offered(id: &str, in_menus: bool, has_menu_path: bool) -> bool {
    !INTERNAL.contains(&id)
        && !vectorcraft_engine::cmd::is_alias(id)
        && !menus::is_slot(id)
        && (in_menus || has_menu_path || crate::shortcut_editor::default_command_shortcut(id).is_some() || is_synonym_target(id))
}

fn is_synonym_target(key: &str) -> bool {
    SYNONYMS.iter().any(|(_, targets)| targets.contains(&key))
}

/// The everyday words that point at `key`.
fn synonym_words(key: &str) -> String {
    SYNONYMS.iter().filter(|(_, targets)| targets.contains(&key)).flat_map(|(words, _)| words.iter().copied()).collect::<Vec<_>>().join(" | ")
}

/// Every entry in `lang`, in catalog order (commands, UI commands, tools, panels).
fn build(lang: crate::i18n::Lang) -> Vec<Entry> {
    let places = menu_places();
    let tr = |s: &str| crate::i18n::tr(lang, s).to_string();
    let path_shown = |path: &[&str]| path.iter().map(|s| tr(s)).collect::<Vec<_>>().join(" › ");
    let mut out = vec![];
    let mut push = |key: String, label_en: &str, path: &[&str], shortcut: Option<&str>| {
        let label = tr(label_en);
        let detail = path_shown(path);
        let path_en = path.join(" › ");
        out.push(Entry {
            hay_label: [label.to_lowercase(), label_en.to_lowercase()],
            hay_keywords: format!("{} | {}", synonym_words(&key), key).to_lowercase(),
            hay_detail: format!("{} | {}", detail.to_lowercase(), path_en.to_lowercase()),
            key,
            label,
            detail,
            shortcut: shortcut.unwrap_or("").to_string(),
        });
    };
    let mut seen = std::collections::HashSet::new();
    for c in vectorcraft_engine::command_specs() {
        let place = places.get(c.id);
        if !offered(c.id, place.is_some(), !c.menu.is_empty()) || !seen.insert(c.id) {
            continue;
        }
        let (label, path) = place.map_or((c.label, c.menu.to_vec()), |(l, p)| (*l, p.clone()));
        push(c.id.to_string(), label, &path, menus::shortcut_of(c.id));
    }
    for c in menus::UI_COMMANDS {
        let place = places.get(c.0);
        if !offered(c.0, place.is_some(), false) || !seen.insert(c.0) {
            continue;
        }
        let (label, path) = place.map_or((c.1, vec![]), |(l, p)| (*l, p.clone()));
        push(c.0.to_string(), label, &path, menus::shortcut_of(c.0));
    }
    for tool in vectorcraft_tools::catalog::all_tools() {
        push(format!("tool:{}", tool.id), tool.label, &["Tools"], crate::shortcut_editor::tool_shortcut(tool.id));
    }
    for (panel, label) in crate::state::all_panels() {
        let key = format!("panel:{panel}");
        let path = places.get(&key).map_or(vec!["Window"], |(_, p)| p.clone());
        let label = places.get(&key).map_or(label, |(l, _)| *l);
        push(key, label, &path, crate::shortcut_editor::panel_shortcut(panel));
    }
    out
}

/// What the entries depend on: the language, the installed plug-ins and the shortcuts.
fn cache_key(lang: crate::i18n::Lang) -> (&'static str, u64, u64) {
    (lang.code(), menus::plugin_revision(), crate::shortcut_editor::GENERATION.load(std::sync::atomic::Ordering::Relaxed))
}

/// The palette's entries in `lang`, built when the language, the plug-ins or the shortcuts change.
fn entries(ctx: &egui::Context, lang: crate::i18n::Lang) -> Arc<Vec<Entry>> {
    type Cached = ((&'static str, u64, u64), Arc<Vec<Entry>>);
    let key = cache_key(lang);
    let id = egui::Id::new("palette-entries");
    if let Some((k, v)) = ctx.data(|d| d.get_temp::<Cached>(id))
        && k == key
    {
        return v;
    }
    let built = Arc::new(build(lang));
    ctx.data_mut(|d| d.insert_temp::<Cached>(id, (key, built.clone())));
    built
}

/// The query lowercased with its words single-spaced.
fn normalize(q: &str) -> String {
    q.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

/// How well `q` names an everyday word: 0 it is one, 1 it starts one, 2 it holds one as whole
/// words; `None`: none.
fn synonym_quality(q: &str, word: &str) -> Option<u32> {
    if q == word {
        Some(0)
    } else if q.chars().count() >= 3 && word.starts_with(q) {
        Some(1)
    } else if format!(" {q} ").contains(&format!(" {word} ")) {
        Some(2)
    } else {
        None
    }
}

/// The everyday-word score of each key `q` points at (lower is better).
fn synonym_hits(q: &str) -> Vec<(&'static str, u32)> {
    let mut hits: Vec<(&'static str, u32)> = vec![];
    for &(words, targets) in SYNONYMS {
        let Some(quality) = words.iter().filter_map(|w| synonym_quality(q, w)).min() else { continue };
        for (rank, &key) in targets.iter().enumerate() {
            // A word inside a longer query ("make it bigger") ranks below a label that starts
            // with the query ("outline stroke"), above weaker text matches.
            let band = match quality {
                0 => 0,
                1 => 10,
                _ => 150,
            };
            let s = band + u32::try_from(rank).unwrap_or(9);
            match hits.iter_mut().find(|h| h.0 == key) {
                Some(h) => h.1 = h.1.min(s),
                None => hits.push((key, s)),
            }
        }
    }
    hits
}

/// craft-ui's score of `word` in `text`, without the letters-in-order fallback unless `fuzzy`.
fn word_score(text: &str, word: &str, fuzzy: bool) -> Option<u32> {
    score(text, word).filter(|s| fuzzy || *s < 2000)
}

/// How well `e` matches `q` by its text (lower is better): the label first, then the everyday
/// words and the id, then where it lives. Each word of a longer query has to match.
fn text_score(e: &Entry, q: &str) -> Option<u32> {
    let one = |w: &str, fuzzy: bool| -> Option<u32> {
        [
            e.hay_label.iter().filter_map(|h| word_score(h, w, fuzzy)).min(),
            word_score(&e.hay_keywords, w, false).map(|s| s.saturating_add(5)),
            word_score(&e.hay_detail, w, false).map(|s| s.saturating_add(50)),
        ]
        .into_iter()
        .flatten()
        .min()
    };
    let words: Vec<&str> = q.split(' ').filter(|w| !w.is_empty()).collect();
    let whole = one(q, words.len() == 1 && q.chars().count() >= 3);
    let each =
        if words.len() > 1 { words.iter().try_fold(0u32, |acc, w| one(w, false).map(|s| acc.saturating_add(s).saturating_add(3))) } else { None };
    [whole, each].into_iter().flatten().min()
}

/// Ranked indices into `entries` for `query`: everyday words first, then text matches; a
/// command that can't run now (`enabled` false) drops a little. Best first, at most `limit`.
pub fn rank(entries: &[Entry], query: &str, enabled: &dyn Fn(&str) -> bool, limit: usize) -> Vec<usize> {
    let q = normalize(query);
    if q.is_empty() {
        return vec![];
    }
    let synonyms = synonym_hits(&q);
    let mut hits: Vec<(u32, usize, usize)> = entries
        .iter()
        .enumerate()
        .filter_map(|(i, e)| {
            let syn = synonyms.iter().find(|(k, _)| *k == e.key).map(|(_, s)| *s);
            let text = text_score(e, &q).map(|s| s.saturating_add(100));
            let s = [syn, text].into_iter().flatten().min()?;
            Some((s, e.label.chars().count(), i))
        })
        .collect();
    // Disabled ones drop below the enabled ones near them, not below weak matches.
    hits.sort_unstable();
    hits.truncate(limit.saturating_mul(4).max(limit));
    for h in &mut hits {
        if entries.get(h.2).is_some_and(|e| !enabled(&e.key)) {
            h.0 = h.0.saturating_add(40);
        }
    }
    hits.sort_unstable();
    hits.into_iter().take(limit).map(|(_, _, i)| i).collect()
}

/// The section headings (English; translated as drawn).
const RECENT_HEADING: &str = "Recently used";
const POPULAR_HEADING: &str = "Popular";

/// A row of the palette: a section heading or an entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Row {
    Heading(&'static str),
    Entry(usize),
}

/// The rows while nothing is typed: the recently used, then the popular ones not among them.
fn empty_rows(entries: &[Entry], recent: &[String]) -> Vec<Row> {
    let find = |key: &str| entries.iter().position(|e| e.key == key);
    let recent: Vec<usize> = recent.iter().filter_map(|k| find(k)).take(SHOWN_RECENT).collect();
    let mut rows = vec![];
    if !recent.is_empty() {
        rows.push(Row::Heading(RECENT_HEADING));
        rows.extend(recent.iter().map(|&i| Row::Entry(i)));
    }
    let popular: Vec<usize> = POPULAR.iter().filter_map(|k| find(k)).filter(|i| !recent.contains(i)).collect();
    if !popular.is_empty() {
        rows.push(Row::Heading(POPULAR_HEADING));
        rows.extend(popular.into_iter().map(Row::Entry));
    }
    rows
}

/// Whether a palette key can run now.
fn key_enabled(app: &VectorcraftApp, key: &str) -> bool {
    if key.starts_with("tool:") || key.starts_with("panel:") {
        return true;
    }
    menus::enabled(app, key)
}

/// Why a command can't run now, in a few plain words.
fn why_disabled(app: &VectorcraftApp, key: &str) -> &'static str {
    let Some(doc) = app.session.active() else { return tl!("Open or create a document first") };
    let engine = vectorcraft_engine::find_command(key).and_then(|c| (c.enabled)(&app.session).err());
    if engine.as_deref() == Some("nothing selected") || (engine.is_none() && doc.selection.is_empty()) {
        tl!("Select something first")
    } else {
        tl!("Not available right now")
    }
}

/// The rows for the current query, ranked once per query (and per entries / recents).
fn rows(app: &VectorcraftApp, ctx: &egui::Context, entries: &Arc<Vec<Entry>>) -> Arc<Vec<Row>> {
    type Key = (String, usize, Vec<String>);
    type Cached = (Key, Arc<Vec<Row>>);
    let key: Key = (app.ui.palette_query.clone(), Arc::as_ptr(entries) as usize, app.ui.palette_recent.clone());
    let id = egui::Id::new("palette-rows");
    if let Some((k, v)) = ctx.data(|d| d.get_temp::<Cached>(id))
        && k == key
    {
        return v;
    }
    let built: Vec<Row> = if normalize(&app.ui.palette_query).is_empty() {
        empty_rows(entries, &app.ui.palette_recent)
    } else {
        rank(entries, &app.ui.palette_query, &|k| key_enabled(app, k), MAX_ROWS).into_iter().map(Row::Entry).collect()
    };
    let built = Arc::new(built);
    ctx.data_mut(|d| d.insert_temp::<Cached>(id, (key, built.clone())));
    built
}

/// Open the palette with nothing typed.
pub fn open(app: &mut VectorcraftApp) {
    app.ui.palette_open = true;
    app.ui.palette_query.clear();
    app.ui.palette_selected = 0;
}

/// Run a palette key and remember it as recently used.
pub fn run_key(app: &mut VectorcraftApp, key: &str) {
    app.ui.palette_open = false;
    app.ui.palette_recent.retain(|k| k != key);
    app.ui.palette_recent.insert(0, key.to_string());
    app.ui.palette_recent.truncate(MAX_RECENT);
    if let Some(tool) = key.strip_prefix("tool:") {
        app.select_tool(tool);
    } else if let Some(panel) = key.strip_prefix("panel:") {
        menus::invoke(app, "window.panel", json!({ "panel": panel }));
    } else {
        menus::invoke(app, key, json!({}));
    }
}

pub fn show(app: &mut VectorcraftApp, ctx: &egui::Context) {
    if !app.ui.palette_open {
        return;
    }
    let t = Tokens::get(ctx);
    let entries = entries(ctx, crate::i18n::current());
    let rows = rows(app, ctx, &entries);
    // Only entries can be highlighted, not headings.
    let selectable: Vec<usize> = rows.iter().filter_map(|r| if let Row::Entry(i) = r { Some(*i) } else { None }).collect();
    let last = selectable.len().saturating_sub(1);
    // Taken before the text field sees them (it would move its cursor).
    let (down, up, enter) = ctx.input_mut(|i| {
        (i.consume_key(egui::Modifiers::NONE, Key::ArrowDown), i.consume_key(egui::Modifiers::NONE, Key::ArrowUp), i.key_pressed(Key::Enter))
    });
    let mut sel = app.ui.palette_selected.min(last);
    if down {
        sel = if sel >= last { 0 } else { sel + 1 };
    }
    if up {
        sel = if sel == 0 { last } else { sel - 1 };
    }
    let mut run: Option<String> = None;
    let mut hovered: Option<usize> = None;
    let before = app.ui.palette_query.clone();
    let width = (ctx.content_rect().width() - 32.0).clamp(280.0, 560.0);
    egui::Area::new(egui::Id::new("palette")).order(egui::Order::Foreground).anchor(egui::Align2::CENTER_TOP, [0.0, 90.0]).show(ctx, |ui| {
        egui::Frame::popup(ui.style()).fill(t.panel).corner_radius(egui::CornerRadius::same(14)).inner_margin(egui::Margin::same(10)).show(
            ui,
            |ui| {
                ui.set_width(width);
                ui.horizontal(|ui| {
                    icons::icon(ui, "search", 16.0, t.text_dim);
                    let r = ui.add(
                        egui::TextEdit::singleline(&mut app.ui.palette_query)
                            .id_salt("palette-query")
                            .hint_text(tl!("Type what you want to do…"))
                            .frame(egui::Frame::NONE)
                            .font(egui::FontId::proportional(15.0))
                            .desired_width(f32::INFINITY),
                    );
                    r.request_focus();
                });
                ui.add_space(4.0);
                let y = ui.cursor().top();
                ui.painter().hline(ui.min_rect().x_range(), y, egui::Stroke::new(1.0, t.divider));
                ui.add_space(6.0);
                if selectable.is_empty() {
                    no_matches(ui, &t, &app.ui.palette_query);
                }
                let mut n = 0usize;
                for row in rows.iter() {
                    let i = match *row {
                        Row::Heading(h) => {
                            ui.add_space(2.0);
                            let h = if h == POPULAR_HEADING { tl!("Popular") } else { tl!("Recently used") };
                            ui.label(egui::RichText::new(h).size(11.0).color(t.text_dim));
                            continue;
                        }
                        Row::Entry(i) => i,
                    };
                    let Some(e) = entries.get(i) else { continue };
                    let here = n;
                    n += 1;
                    let enabled = key_enabled(app, &e.key);
                    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::hover());
                    let resp = ui.interact(rect, egui::Id::new(("palette-row", here)), Sense::click());
                    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, &e.label));
                    if resp.hovered() && ui.input(|inp| inp.pointer.delta() != egui::Vec2::ZERO) {
                        hovered = Some(here);
                    }
                    if here == sel {
                        ui.painter().rect_filled(rect, egui::CornerRadius::same(6), t.row_selected);
                    } else if resp.hovered() {
                        ui.painter().rect_filled(rect, egui::CornerRadius::same(6), t.hover);
                    }
                    let fg = if enabled { t.text } else { t.text_disabled };
                    let right = if enabled { menus::pretty_shortcut(&e.shortcut) } else { why_disabled(app, &e.key).to_string() };
                    let right = ui.painter().layout_no_wrap(right, egui::FontId::proportional(11.5), t.text_dim);
                    let rx = rect.right() - 10.0 - right.size().x;
                    let mut job = egui::text::LayoutJob::default();
                    job.append(&e.label, 0.0, egui::TextFormat::simple(egui::FontId::proportional(13.0), fg));
                    if !e.detail.is_empty() {
                        job.append(&e.detail, 10.0, egui::TextFormat::simple(egui::FontId::proportional(11.5), t.text_dim));
                    }
                    job.wrap.max_width = (rx - rect.left() - 20.0).max(40.0);
                    job.wrap.max_rows = 1;
                    job.wrap.break_anywhere = true;
                    let left = ui.painter().layout_job(job);
                    ui.painter().galley(egui::pos2(rect.left() + 10.0, rect.center().y - left.size().y / 2.0), left, fg);
                    ui.painter().galley(egui::pos2(rx, rect.center().y - right.size().y / 2.0), right, t.text_dim);
                    if resp.clicked() && enabled {
                        run = Some(e.key.clone());
                    }
                    if enter && here == sel && enabled {
                        run = Some(e.key.clone());
                    }
                }
                ui.add_space(4.0);
                ui.label(egui::RichText::new(tl!("↑↓ move · Enter runs · Esc closes")).size(10.5).color(t.text_dim));
            },
        );
    });
    if let Some(h) = hovered {
        sel = h;
    }
    app.ui.palette_selected = if app.ui.palette_query != before { 0 } else { sel };
    if let Some(key) = run {
        run_key(app, &key);
    }
}

/// What the palette says when nothing matches.
fn no_matches(ui: &mut egui::Ui, t: &Tokens, query: &str) {
    ui.add_space(10.0);
    ui.vertical_centered(|ui| {
        icons::icon(ui, "search", 24.0, t.text_disabled);
        ui.add_space(4.0);
        let q = query.trim();
        ui.label(egui::RichText::new(crate::i18n::fmt(tl!("No matches for “{query}”"), &[("query", q)])).size(13.0).color(t.text));
        ui.label(egui::RichText::new(tl!("Try a plainer word, such as “color”, “text”, “group” or “undo”.")).size(11.5).color(t.text_dim));
    });
    ui.add_space(10.0);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::Lang;

    fn ranked(q: &str) -> Vec<String> {
        let e = build(Lang::EN);
        rank(&e, q, &|_| true, MAX_ROWS).into_iter().filter_map(|i| e.get(i).map(|e| e.key.clone())).collect()
    }

    #[test]
    fn entries_are_built_once_per_language_and_match_english_shown_text_and_ids() {
        let ctx = egui::Context::default();
        let zh = Lang::from_code("zh-hant").unwrap();
        // Tests running alongside edit shortcuts and install plug-ins, which rebuilds the entries:
        // look twice until nothing changed in between.
        let (en, again) = (0..100)
            .find_map(|_| {
                let key = cache_key(Lang::EN);
                let pair = (entries(&ctx, Lang::EN), entries(&ctx, Lang::EN));
                (cache_key(Lang::EN) == key).then_some(pair)
            })
            .unwrap();
        assert!(Arc::ptr_eq(&en, &again), "kept while the language stays");
        let zh_entries = entries(&ctx, zh);
        assert!(!Arc::ptr_eq(&en, &zh_entries), "rebuilt for another language");
        // Editing a shortcut rebuilds them, so the palette shows the new one.
        crate::shortcut_editor::GENERATION.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        assert!(!Arc::ptr_eq(&zh_entries, &entries(&ctx, zh)), "rebuilt after a shortcut edit");
        let group = zh_entries.iter().find(|e| e.key == "object.group").unwrap();
        assert_eq!(group.label, crate::i18n::tr(zh, "Group"));
        assert_eq!(group.detail, crate::i18n::tr(zh, "Object"), "where it lives, translated");
        // A query matches the English label, the shown label or the id.
        for word in ["group", crate::i18n::tr(zh, "Group"), "object.group"] {
            assert_eq!(
                rank(&zh_entries, word, &|_| true, 3).first().and_then(|&i| zh_entries.get(i)).map(|e| e.key.as_str()),
                Some("object.group"),
                "{word}"
            );
        }
    }

    /// The menu bar draws every frame from a tree built once, again only when what it lists
    /// changes (a shortcut edit here).
    #[test]
    fn the_menu_bar_tree_is_built_once() {
        let ctx = egui::Context::default();
        let (a, b) = (0..100)
            .find_map(|_| {
                let pair = (menus::shared_menu_tree(&ctx), menus::shared_menu_tree(&ctx));
                Arc::ptr_eq(&pair.0, &pair.1).then_some(pair)
            })
            .unwrap();
        assert_eq!(a.len(), menus::menu_tree().len());
        drop(b);
        crate::shortcut_editor::GENERATION.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        assert!(!Arc::ptr_eq(&a, &menus::shared_menu_tree(&ctx)), "rebuilt after a shortcut edit");
    }

    #[test]
    fn everyday_words_find_what_people_mean() {
        for (q, first) in [
            ("change color", "ui.colorPicker"),
            ("change colour", "ui.colorPicker"),
            ("fill", "ui.colorPicker"),
            ("round corners", "ui.corners"),
            ("combine shapes", "object.pathfinder.unite"),
            ("merge", "object.pathfinder.unite"),
            ("cut out", "object.pathfinder.minusFront"),
            ("subtract", "object.pathfinder.minusFront"),
            ("outline", "panel:stroke"),
            ("line thickness", "panel:stroke"),
            ("curve", "tool:pen"),
            ("erase", "tool:eraser"),
            ("text", "tool:type"),
            ("words", "tool:type"),
            ("picture", "file.place"),
            ("photo", "file.place"),
            ("group", "object.group"),
            ("align", "panel:align"),
            ("center", "panel:align"),
            ("flip", "object.reflect"),
            ("rotate", "object.rotate"),
            ("bigger", "object.scale"),
            ("make it bigger", "object.scale"),
            ("resize", "object.scale"),
            ("undo", "edit.undo"),
        ] {
            let got = ranked(q);
            assert_eq!(got.first().map(String::as_str), Some(first), "{q}: {got:?}");
        }
        // "change color" also offers the Color and Swatches panels, before anything else.
        assert_eq!(ranked("change color").get(1..3), Some(&["panel:color".to_string(), "panel:swatches".to_string()][..]));
        // Plain text matching still works and ranks by relevance, not catalog order.
        assert_eq!(ranked("unite").first().map(String::as_str), Some("object.pathfinder.unite"));
        assert_eq!(ranked("pen tool").first().map(String::as_str), Some("tool:pen"));
        assert_eq!(ranked("outline stroke").first().map(String::as_str), Some("object.path.outlineStroke"));
        assert!(ranked("zzqxj").is_empty(), "nothing matches gibberish");
        assert!(ranked("change xyzzy").is_empty(), "every word has to match");
    }

    #[test]
    fn every_everyday_word_and_popular_entry_points_at_an_entry() {
        let e = build(Lang::EN);
        for key in SYNONYMS.iter().flat_map(|(_, t)| t.iter()).chain(POPULAR) {
            assert!(e.iter().any(|e| e.key == *key), "{key}");
        }
    }

    #[test]
    fn internal_commands_never_show() {
        let e = build(Lang::EN);
        for id in INTERNAL {
            assert!(!e.iter().any(|e| e.key == *id), "{id}");
        }
        for q in ["inspect", "document json", "node", "activate document", "json"] {
            let got = ranked(q);
            assert!(got.iter().all(|k| !INTERNAL.contains(&k.as_str())), "{q}: {got:?}");
        }
        // Placeholder slots (recent files, saved views) aren't commands to search for.
        assert!(!e.iter().any(|e| e.key.starts_with("file.openRecent") || e.key.starts_with("type.recentFont")));
        let mut seen = std::collections::HashSet::new();
        assert!(e.iter().all(|e| seen.insert(e.key.clone())), "each entry once");
    }

    #[test]
    fn entries_show_where_they_live_and_their_shortcut() {
        let e = build(Lang::EN);
        let find = |k: &str| e.iter().find(|e| e.key == k).unwrap();
        assert_eq!((find("object.group").detail.as_str(), find("object.group").shortcut.as_str()), ("Object", "Cmd+G"));
        assert_eq!(find("object.pathfinder.unite").detail, "Window › Pathfinder");
        assert_eq!(find("object.path.outlineStroke").detail, "Object › Path");
        assert_eq!(find("panel:align").detail, "Window");
        assert_eq!(find("tool:pen").shortcut, "P");
        assert_eq!(find("help.commandPalette").shortcut, "Cmd+K");
    }

    #[test]
    fn disabled_commands_drop_below_enabled_ones() {
        let e = build(Lang::EN);
        let off = |k: &str| k != "object.group";
        let got: Vec<&str> = rank(&e, "group", &off, MAX_ROWS).into_iter().filter_map(|i| e.get(i).map(|e| e.key.as_str())).collect();
        assert_eq!(got.first(), Some(&"object.ungroup"), "{got:?}");
        assert!(got.contains(&"object.group"), "still listed: {got:?}");
    }

    #[test]
    fn nothing_typed_lists_recent_then_popular() {
        let e = build(Lang::EN);
        let rows = empty_rows(&e, &[]);
        assert_eq!(rows.first(), Some(&Row::Heading("Popular")));
        assert_eq!(rows.len(), POPULAR.len() + 1, "the popular ones, not the whole catalog");
        let rows = empty_rows(&e, &["object.group".into(), "gone.command".into()]);
        let key = |r: &Row| if let Row::Entry(i) = r { e.get(*i).map(|e| e.key.as_str()) } else { None };
        assert_eq!(rows.first(), Some(&Row::Heading("Recently used")));
        assert_eq!(rows.get(1).and_then(key), Some("object.group"));
        assert_eq!(rows.get(2), Some(&Row::Heading("Popular")));
        assert_eq!(rows.iter().filter(|r| key(r) == Some("object.group")).count(), 1, "not listed twice");
    }

    fn frame(app: &mut VectorcraftApp, ctx: &egui::Context, events: Vec<egui::Event>) {
        let input = egui::RawInput { events, ..Default::default() };
        let mut out = ctx.run_ui(input, |ui| {
            crate::shortcuts::handle(app, ui.ctx());
            show(app, ui.ctx());
        });
        out.textures_delta.clear();
    }

    fn key(key: Key, modifiers: egui::Modifiers) -> egui::Event {
        egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers }
    }

    #[test]
    fn cmd_k_opens_it_arrows_move_enter_runs_and_esc_closes() {
        let ctx = egui::Context::default();
        let mut app = VectorcraftApp::new(vectorcraft_engine::Session::new(), Default::default());
        app.session.execute("file.new", &json!({"width": 200, "height": 200})).unwrap();
        frame(&mut app, &ctx, vec![key(Key::K, egui::Modifiers::COMMAND)]);
        assert!(app.ui.palette_open, "Cmd+K opens the palette");
        assert!(app.ui.dialog.is_none(), "not Preferences");
        frame(&mut app, &ctx, vec![key(Key::Escape, egui::Modifiers::NONE)]);
        assert!(!app.ui.palette_open, "Esc closes it");
        // The old chord still opens it.
        frame(&mut app, &ctx, vec![key(Key::Questionmark, egui::Modifiers::COMMAND | egui::Modifiers::SHIFT)]);
        assert!(app.ui.palette_open, "Cmd+Shift+/ too");
        // Typing ranks; Down moves to the second match; Enter runs it and remembers it.
        frame(&mut app, &ctx, vec![egui::Event::Text("pen".into())]);
        assert_eq!(app.ui.palette_query, "pen");
        let e = build(Lang::EN);
        let want = rank(&e, "pen", &|k| key_enabled(&app, k), MAX_ROWS).get(1).and_then(|&i| e.get(i)).map(|e| e.key.clone()).unwrap();
        frame(&mut app, &ctx, vec![key(Key::ArrowDown, egui::Modifiers::NONE)]);
        assert_eq!(app.ui.palette_selected, 1);
        frame(&mut app, &ctx, vec![key(Key::Enter, egui::Modifiers::NONE)]);
        assert!(!app.ui.palette_open, "ran");
        assert_eq!(app.ui.palette_recent.first(), Some(&want));
        // Preferences moved to Cmd+, (Ctrl+, elsewhere).
        app.ui.dialog = None;
        frame(&mut app, &ctx, vec![]);
        frame(&mut app, &ctx, vec![key(Key::Comma, egui::Modifiers::COMMAND)]);
        assert!(app.ui.dialog.is_some(), "Cmd+, opens Settings");
    }
}
