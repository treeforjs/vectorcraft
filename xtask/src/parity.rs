//! `cargo xtask parity`: validate `parity/illustrator-features.toml` and report progress.
//!
//! Checks:
//! - schema: unique ids (`area.kebab-name`), known area letters, tiers, milestones and statuses;
//! - for `shipped` and `partial` features, every listed command exists in the command registry
//!   (engine commands from `vectorcraft-cli commands`, plus the UI-only `UI_COMMANDS`) and every
//!   tool exists in the tool catalogue, so the claim is reachable from the UI and headlessly;
//! - `shipped` features cite at least one test, and every cited `path::fn` exists.
//!
//! The report gives the shipped share per tier and per area (partial counts half in the weighted
//! figure). `--json` prints the numbers; `--partial` and `--planned` list those features with
//! their notes. Fails on any validation error.

use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use serde::Deserialize;

use crate::{cargo, root};

const CHECKLIST: &str = "parity/illustrator-features.toml"; // brand-ok: named after the app it tracks

pub const AREAS: &[(&str, &str)] = &[
    ("A", "Documents, files and output"),
    ("B", "Viewing, rulers, guides, grids"),
    ("C", "Selection"),
    ("D", "Drawing and shape tools"),
    ("E", "Path editing and construction"),
    ("F", "Transform, arrange, distort"),
    ("G", "Paint, colour and transparency"),
    ("H", "Appearance and effects"),
    ("I", "Type"),
    ("J", "Brushes, symbols, patterns"),
    ("K", "Layers, artboards, organisation"),
    ("L", "Images and tracing"),
    ("M", "Automation and data"),
    ("N", "Print and production"),
    ("O", "Interface and preferences"),
    ("P", "Cloud and generative AI"),
];
const TIERS: &[&str] = &["P0", "P1", "P2", "P3", "N/A"];
const STATUSES: &[&str] = &["planned", "partial", "shipped", "na"];

#[derive(Debug, Deserialize)]
struct File {
    schema: Schema,
    #[serde(rename = "feature")]
    features: Vec<Feature>,
}

#[derive(Debug, Deserialize)]
struct Schema {
    version: u32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Feature {
    pub id: String,
    pub title: String,
    pub area: String,
    pub tier: String,
    pub milestone: String,
    pub status: String,
    #[serde(default)]
    pub commands: Vec<String>,
    #[serde(default)]
    pub tools: Vec<String>,
    #[serde(default)]
    pub evidence: Vec<String>,
    #[serde(default)]
    pub notes: String,
}

/// Quoted ids that follow `prefix(` in a source file: `t("pen", …)` → `pen`.
pub fn quoted_after(src: &str, prefixes: &[&str]) -> HashSet<String> {
    let mut out = HashSet::new();
    for p in prefixes {
        let pat = format!("{p}(");
        let mut from = 0;
        while let Some(i) = src.get(from..).and_then(|s| s.find(&pat)).map(|i| i + from) {
            from = i + pat.len();
            // Whole identifiers only: `t(` must not match inside `ct(`.
            let boundary = src[..i].chars().next_back().is_none_or(|c| !(c.is_alphanumeric() || c == '_'));
            // The id may start on the next line (`(\n    "app.language",`).
            let rest = src[from..].trim_start();
            if boundary
                && let Some(body) = rest.strip_prefix('"')
                && let Some(end) = body.find('"')
            {
                out.insert(body[..end].to_string());
            }
        }
    }
    out
}

/// The UI-only command ids: the first string of each tuple in `menus.rs`'s `UI_COMMANDS`.
pub fn ui_commands(menus_rs: &str) -> HashSet<String> {
    let Some(start) = menus_rs.find("pub const UI_COMMANDS") else { return HashSet::new() };
    let body = &menus_rs[start..];
    let body = body.find("\n];").map_or(body, |end| &body[..end]);
    // Tuples open with `(` on their own line or inline; the bare-`(` prefix matches both.
    quoted_after(body, &[""])
}

/// Engine command ids, from the CLI's `commands` dump (`[{id, …}]` or `{commands: [{id, …}]}`).
fn engine_commands() -> Result<HashSet<String>, String> {
    let out =
        cargo().args(["run", "--quiet", "-p", "vectorcraft-cli", "--", "commands"]).output().map_err(|e| format!("vectorcraft-cli commands: {e}"))?;
    if !out.status.success() {
        return Err(format!("vectorcraft-cli commands failed:\n{}", String::from_utf8_lossy(&out.stderr)));
    }
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).map_err(|e| format!("vectorcraft-cli commands: bad JSON: {e}"))?;
    Ok(ids_in(&v))
}

fn ids_in(v: &serde_json::Value) -> HashSet<String> {
    let list = v.as_array().or_else(|| v.get("commands").and_then(serde_json::Value::as_array));
    list.into_iter().flatten().filter_map(|c| c.get("id").and_then(serde_json::Value::as_str)).map(str::to_owned).collect()
}

fn valid_id(id: &str) -> bool {
    let Some((area, name)) = id.split_once('.') else { return false };
    !area.is_empty()
        && area.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && !name.is_empty()
        && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '.')
}

fn valid_milestone(m: &str) -> bool {
    m == "none" || m.strip_prefix('M').and_then(|n| n.parse::<u32>().ok()).is_some_and(|n| n <= 14)
}

/// Validate features against the registry, tool catalogue and source tree; returns the errors.
pub fn validate(features: &[Feature], commands: &HashSet<String>, tools: &HashSet<String>, repo: &Path) -> Vec<String> {
    let mut errors = Vec::new();
    let mut ids = HashSet::new();
    for f in features {
        let e = |m: String| format!("{}: {m}", f.id);
        if !ids.insert(f.id.as_str()) {
            errors.push(e("duplicate id".into()));
        }
        if !valid_id(&f.id) {
            errors.push(e("id must look like `area.kebab-name`".into()));
        }
        if f.title.trim().is_empty() {
            errors.push(e("empty title".into()));
        }
        if !AREAS.iter().any(|(a, _)| *a == f.area) {
            errors.push(e(format!("unknown area {:?} (A–P)", f.area)));
        }
        if !TIERS.contains(&f.tier.as_str()) {
            errors.push(e(format!("unknown tier {:?}", f.tier)));
        }
        if !STATUSES.contains(&f.status.as_str()) {
            errors.push(e(format!("unknown status {:?}", f.status)));
        }
        if !valid_milestone(&f.milestone) {
            errors.push(e(format!("milestone {:?} must be M0–M14 or none", f.milestone)));
        }
        if matches!(f.status.as_str(), "shipped" | "partial") {
            for c in f.commands.iter().filter(|c| !commands.contains(*c)) {
                errors.push(e(format!("command {c:?} is not in the command registry")));
            }
            for t in f.tools.iter().filter(|t| !tools.contains(*t)) {
                errors.push(e(format!("tool {t:?} is not in the tool catalogue")));
            }
        }
        if f.status == "shipped" && f.evidence.is_empty() {
            errors.push(e("shipped features must cite at least one test in `evidence`".into()));
        }
        if f.status == "partial" && f.notes.trim().is_empty() {
            errors.push(e("partial features must say in `notes` what is missing".into()));
        }
        for ev in &f.evidence {
            let Some((path, func)) = ev.split_once("::") else {
                errors.push(e(format!("evidence {ev:?} must be `path.rs::test_fn`")));
                continue;
            };
            match std::fs::read_to_string(repo.join(path)) {
                Ok(src) if src.contains(&format!("fn {func}(")) => {}
                Ok(_) => errors.push(e(format!("evidence: no `fn {func}` in {path}"))),
                Err(_) => errors.push(e(format!("evidence: {path} does not exist"))),
            }
        }
    }
    errors
}

type Score = (usize, usize, usize, f64, f64);

fn score(fs: &[&Feature]) -> Score {
    let shipped = fs.iter().filter(|f| f.status == "shipped").count();
    let partial = fs.iter().filter(|f| f.status == "partial").count();
    let pct = |x: f64| if fs.is_empty() { 0.0 } else { 100.0 * x / fs.len() as f64 };
    (fs.len(), shipped, partial, pct(shipped as f64), pct(shipped as f64 + partial as f64 / 2.0))
}

pub fn run(args: &[&str]) -> Result<(), String> {
    let repo = root();
    let path = repo.join(CHECKLIST);
    let text = std::fs::read_to_string(&path).map_err(|e| format!("reading {}: {e}", path.display()))?;
    let file: File = toml::from_str(&text).map_err(|e| format!("parsing {}: {e}", path.display()))?;
    if file.schema.version != 1 {
        return Err(format!("unsupported parity schema version {}", file.schema.version));
    }
    let read = |p: &str| std::fs::read_to_string(repo.join(p)).map_err(|e| format!("reading {p}: {e}"));
    let mut commands = engine_commands()?;
    commands.extend(ui_commands(&read("crates/ui-egui/src/menus.rs")?));
    let tools = quoted_after(&read("crates/tools/src/catalog.rs")?, &["t"]);
    let errors = validate(&file.features, &commands, &tools, &repo);

    // Progress per tier and per area. The N/A tier and `na` status are left out of percentages.
    let counted: Vec<&Feature> = file.features.iter().filter(|f| f.status != "na" && f.tier != "N/A").collect();
    let mut by_tier = BTreeMap::new();
    for t in TIERS.iter().filter(|t| **t != "N/A") {
        let fs: Vec<&Feature> = counted.iter().copied().filter(|f| f.tier == *t).collect();
        by_tier.insert(*t, score(&fs));
    }
    let by_area: Vec<_> = AREAS
        .iter()
        .map(|(a, name)| {
            let fs: Vec<&Feature> = counted.iter().copied().filter(|f| f.area == *a).collect();
            (*a, *name, score(&fs))
        })
        .collect();
    let total = score(&counted);

    if args.contains(&"--json") {
        let json = serde_json::json!({
            "features": file.features.len(),
            "total": { "counted": total.0, "shipped": total.1, "partial": total.2, "shipped_pct": total.3, "weighted_pct": total.4 },
            "tiers": by_tier.iter().map(|(t, s)| (t.to_string(), serde_json::json!({ "counted": s.0, "shipped": s.1, "partial": s.2, "shipped_pct": s.3, "weighted_pct": s.4 }))).collect::<serde_json::Map<_, _>>(),
            "areas": by_area.iter().map(|(a, _, s)| (a.to_string(), serde_json::json!({ "counted": s.0, "shipped": s.1, "partial": s.2, "weighted_pct": s.4 }))).collect::<serde_json::Map<_, _>>(),
            "errors": errors,
        });
        println!("{}", serde_json::to_string_pretty(&json).map_err(|e| e.to_string())?);
    } else {
        println!("parity: {} features ({} counted; {} na)", file.features.len(), counted.len(), file.features.len() - counted.len());
        println!("\n  tier  features  shipped  partial  shipped%  weighted%");
        for (t, s) in &by_tier {
            println!("  {t:<4}  {:>8}  {:>7}  {:>7}  {:>7.1}%  {:>8.1}%", s.0, s.1, s.2, s.3, s.4);
        }
        println!("  all   {:>8}  {:>7}  {:>7}  {:>7.1}%  {:>8.1}%", total.0, total.1, total.2, total.3, total.4);
        println!("\n  area                                features  shipped  partial  weighted%");
        for (a, name, s) in &by_area {
            println!("  {a} {name:<33} {:>8}  {:>7}  {:>7}  {:>8.1}%", s.0, s.1, s.2, s.4);
        }
        for (flag, status) in [("--partial", "partial"), ("--planned", "planned")] {
            if args.contains(&flag) {
                println!("\n{status} features:");
                for f in file.features.iter().filter(|f| f.status == status) {
                    println!("  {:<4} {:<44} {}", f.tier, f.id, f.notes);
                }
            }
        }
    }
    if !errors.is_empty() {
        for e in &errors {
            eprintln!("parity error: {e}");
        }
        return Err(format!("{} parity error(s)", errors.len()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(id: &str, status: &str) -> Feature {
        Feature {
            id: id.into(),
            title: "T".into(),
            area: "D".into(),
            tier: "P0".into(),
            milestone: "M2".into(),
            status: status.into(),
            commands: vec![],
            tools: vec![],
            evidence: vec![],
            notes: String::new(),
        }
    }

    #[test]
    fn tool_and_ui_command_ids_are_extracted_from_source() {
        let catalog = "&[t(\"pen\", \"Pen Tool\", Some(\"P\"), \"tool-pen\"), ct(\"x\"), t(\n    \"arc\",)]";
        assert_eq!(quoted_after(catalog, &["t"]), HashSet::from(["pen".to_string(), "arc".to_string()]));
        let menus = "fn a() { f(\"no\") }\npub const UI_COMMANDS: &[(&str, &str, &str, &str)] = &[\n    (\n        \"app.language\",\n        \"Interface Language\",\n    ),\n    (\"view.zoomIn\", \"Zoom In\", \"\", \"\"),\n];\nfn b() { g(\"no\") }";
        let ids = ui_commands(menus);
        assert!(ids.contains("app.language") && ids.contains("view.zoomIn"));
        assert!(!ids.contains("no"));
    }

    #[test]
    fn command_dumps_in_either_shape_give_ids() {
        let a = serde_json::json!([{"id": "file.open"}, {"id": "edit.undo"}]);
        let b = serde_json::json!({"commands": [{"id": "file.open"}]});
        assert_eq!(ids_in(&a).len(), 2);
        assert!(ids_in(&b).contains("file.open"));
    }

    #[test]
    fn shipped_needs_evidence_and_known_commands() {
        let mut s = f("draw.pen", "shipped");
        s.commands = vec!["path.nope".into()];
        let errs = validate(&[s], &HashSet::new(), &HashSet::new(), Path::new("."));
        assert!(errs.iter().any(|e| e.contains("not in the command registry")));
        assert!(errs.iter().any(|e| e.contains("at least one test")));
    }

    #[test]
    fn partial_needs_notes_and_planned_may_name_future_commands() {
        let errs = validate(&[f("draw.x", "partial")], &HashSet::new(), &HashSet::new(), Path::new("."));
        assert!(errs.iter().any(|e| e.contains("notes")));
        let mut p = f("draw.y", "planned");
        p.commands = vec!["effect.3d.extrude".into()];
        assert!(validate(&[p], &HashSet::new(), &HashSet::new(), Path::new(".")).is_empty());
    }

    #[test]
    fn bad_ids_tiers_and_duplicates_are_reported() {
        let mut a = f("Bad Id", "planned");
        a.tier = "P9".into();
        let b = f("draw.pen", "planned");
        let c = f("draw.pen", "planned");
        let errs = validate(&[a, b, c], &HashSet::new(), &HashSet::new(), Path::new("."));
        assert!(errs.iter().any(|e| e.contains("area.kebab-name")));
        assert!(errs.iter().any(|e| e.contains("unknown tier")));
        assert!(errs.iter().any(|e| e.contains("duplicate id")));
    }
}
