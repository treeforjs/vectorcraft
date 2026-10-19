//! The scripting subcommands: `describe`, `script`, `app`, and `convert` over many files.

use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::{Value, json};
use vectorcraft_mcp::{Backend, DEFAULT_ADDR, Headless, Remote};

/// Most steps a script may hold: a runaway generator fails fast instead of filling memory.
const MAX_STEPS: usize = 100_000;

/// The command catalogue (engine commands plus the host commands `run` and `script` accept).
pub fn catalogue() -> Result<Vec<Value>, String> {
    let v = Headless::new().call("engine.commands", json!({}))?;
    match v {
        Value::Array(a) => Ok(a),
        _ => Err("the command catalogue is not a list".into()),
    }
}

/// Commands whose id, label, menu path or parameter notes contain `filter` (any case).
pub fn filter_commands(all: Vec<Value>, filter: &str) -> Vec<Value> {
    let f = filter.to_lowercase();
    all.into_iter()
        .filter(|c| ["id", "label", "menu", "params"].iter().any(|k| c.get(*k).is_some_and(|v| v.to_string().to_lowercase().contains(&f))))
        .collect()
}

/// `describe ID`: one command's label, menu path, shortcut and parameters. An unknown id names
/// the commands in the same group (`shape.rect` → `shape.rectangle`, `shape.ellipse`, …).
pub fn describe(id: Option<&String>) -> Result<Value, String> {
    let id = id.ok_or("describe needs a command id (see `vectorcraft-cli commands`)")?;
    let all = catalogue()?;
    let Some(c) = all.iter().find(|c| c["id"] == id.as_str()) else {
        let group = id.split('.').next().unwrap_or_default();
        let near: Vec<&str> = all.iter().filter_map(|c| c["id"].as_str()).filter(|c| c.split('.').next() == Some(group)).take(12).collect();
        let hint = if near.is_empty() { String::new() } else { format!(" (similar: {})", near.join(", ")) };
        return Err(format!("no command `{id}`{hint}"));
    };
    let mut c = c.clone();
    // Enablement depends on a live document and selection, which a lookup has not got.
    if let Some(o) = c.as_object_mut() {
        o.remove("enabled");
        o.remove("disabled_reason");
    }
    Ok(c)
}

/// `127.0.0.1:7979` as given; a bare port (`7979`) on the loopback address.
fn control_addr(a: &str) -> String {
    if a.bytes().all(|b| b.is_ascii_digit()) { format!("127.0.0.1:{a}") } else { a.to_string() }
}

fn connect(addr: &str) -> Result<Remote, String> {
    let addr = control_addr(addr);
    Remote::connect(&addr).map_err(|e| format!("cannot connect to the VectorCraft app at {addr}: {e} (start it with `vectorcraft --control 7979`)"))
}

/// One script step: a command id and its parameters (`$N.path` references still in them).
#[derive(Debug, PartialEq)]
pub struct Step {
    pub command: String,
    pub params: Value,
}

/// The steps of a script: a JSON array, or one step per line (blank lines and lines starting
/// with `#` or `//` are skipped). A step is `{"command": ID, "params": {…}}` (`id` for `command`
/// as in MCP `command_batch`; `params` optional) or a bare `"ID"` string.
pub fn parse_steps(text: &str) -> Result<Vec<Step>, String> {
    if text.trim_start().starts_with('[') {
        let v: Value = serde_json::from_str(text).map_err(|e| format!("script: {e}"))?;
        let a = v.as_array().ok_or("script: expected an array of steps")?;
        if a.len() > MAX_STEPS {
            return Err(format!("script: more than {MAX_STEPS} steps"));
        }
        return a.iter().enumerate().map(|(i, s)| step(s).map_err(|e| format!("script step {i}: {e}"))).collect();
    }
    let mut steps = vec![];
    for (n, line) in text.lines().enumerate() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') || t.starts_with("//") {
            continue;
        }
        if steps.len() >= MAX_STEPS {
            return Err(format!("script: more than {MAX_STEPS} steps"));
        }
        let v: Value = serde_json::from_str(t).map_err(|e| format!("script line {}: {e}", n + 1))?;
        steps.push(step(&v).map_err(|e| format!("script line {}: {e}", n + 1))?);
    }
    Ok(steps)
}

fn step(v: &Value) -> Result<Step, String> {
    if let Some(id) = v.as_str() {
        return Ok(Step { command: id.to_string(), params: json!({}) });
    }
    let o = v.as_object().ok_or("a step is {\"command\": ID, \"params\": {…}} or \"ID\"")?;
    if let Some(k) = o.keys().find(|k| !matches!(k.as_str(), "command" | "id" | "params")) {
        return Err(format!("unknown step key `{k}` (expected command, params)"));
    }
    let command = o.get("command").or(o.get("id")).and_then(Value::as_str).ok_or("missing `command`")?.to_string();
    let params = match o.get("params") {
        None | Some(Value::Null) => json!({}),
        Some(p @ Value::Object(_)) => p.clone(),
        Some(_) => return Err(format!("{command}: `params` must be an object")),
    };
    Ok(Step { command, params })
}

/// `script [FILE|-] [--in FILE] [--connect ADDR] [--save OUT] [--export OUT]... [--scale N] [--keep-going]`:
/// run a script's steps in one session, then save and export. Returns the report and whether a
/// step failed.
pub fn script(args: &[String]) -> Result<(Value, bool), String> {
    let (mut file, mut input, mut connect_to, mut save) = (None::<String>, None::<String>, None::<String>, None::<String>);
    let (mut exports, mut scale, mut keep_going) = (vec![], 1.0f64, false);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut val = |what: &str| it.next().cloned().ok_or_else(|| format!("{a} needs {what}"));
        match a.as_str() {
            "--in" | "-i" => input = Some(val("a file")?),
            "--connect" => connect_to = Some(val("an address")?),
            "--save" => save = Some(val("a file")?),
            "--export" | "-o" => exports.push(val("a file")?),
            "--scale" | "-s" => {
                let raw = val("a number")?;
                scale =
                    raw.parse::<f64>().ok().filter(|s| s.is_finite() && *s > 0.0).ok_or_else(|| format!("--scale {raw}: not a positive number"))?;
            }
            "--keep-going" | "-k" => keep_going = true,
            f if file.is_none() && (f == "-" || !f.starts_with('-')) => file = Some(f.to_string()),
            other => return Err(format!("unknown script option `{other}`")),
        }
    }
    let text = match file.as_deref() {
        None | Some("-") => {
            let mut t = String::new();
            std::io::Read::read_to_string(&mut std::io::stdin(), &mut t).map_err(|e| format!("stdin: {e}"))?;
            t
        }
        Some(f) => std::fs::read_to_string(f).map_err(|e| format!("{f}: {e}"))?,
    };
    let steps = parse_steps(&text)?;

    let mut backend: Box<dyn Backend> = match &connect_to {
        Some(a) => Box::new(connect(a)?),
        None => {
            let mut h = Headless::new();
            // A script that starts a document itself gets no default one in front of it.
            if input.is_none() && !matches!(steps.first(), Some(s) if s.command == "file.new" || s.command == "file.open") {
                h.ensure_document();
            }
            Box::new(h)
        }
    };
    let b = &mut *backend;
    let mut report = serde_json::Map::new();
    if let Some(path) = &input {
        let r = b.call("app.open", json!({"path": path})).map_err(|e| format!("open {path}: {e}"))?;
        report.insert("opened".into(), json!({"path": path, "result": r}));
    }
    let (results, failed) = run_steps(b, &steps, keep_going);
    report.insert("completed".into(), json!(results.iter().filter(|r| r["ok"] == true).count()));
    report.insert("failed".into(), json!(failed));
    report.insert("results".into(), Value::Array(results));
    // A failed script leaves its files alone: half-done art never overwrites a good save.
    if failed == 0 {
        if let Some(path) = &save {
            let r = b.call("app.save", json!({"path": path})).map_err(|e| format!("save {path}: {e}"))?;
            report.insert("saved".into(), r);
        }
        let mut written = vec![];
        for path in &exports {
            written.push(b.call("app.export", json!({"path": path, "scale": scale})).map_err(|e| format!("export {path}: {e}"))?);
        }
        if !written.is_empty() {
            report.insert("exported".into(), Value::Array(written));
        }
    }
    Ok((Value::Object(report), failed > 0))
}

/// Run `steps` in order, resolving `$N.path` references from earlier results (a failed step's
/// result is null). Stops at the first failure unless `keep_going`. Returns one
/// `{command, ok, result | error}` per step run, and how many failed.
pub fn run_steps(b: &mut dyn Backend, steps: &[Step], keep_going: bool) -> (Vec<Value>, usize) {
    let (mut out, mut values, mut failed) = (vec![], vec![], 0);
    for s in steps {
        let r = vectorcraft_engine::guard::catch_panic(|| {
            let params = vectorcraft_engine::steps::resolve(&s.params, &values)?;
            b.call("engine.execute", json!({"command": s.command, "params": params}))
        })
        .unwrap_or_else(|e| Err(format!("internal error: {e}")));
        match r {
            Ok(v) => {
                out.push(json!({"command": s.command, "ok": true, "result": v}));
                values.push(v);
            }
            Err(e) => {
                failed += 1;
                out.push(json!({"command": s.command, "ok": false, "error": e}));
                values.push(Value::Null);
                if !keep_going {
                    break;
                }
            }
        }
    }
    (out, failed)
}

/// `app [--connect ADDR] COMMAND [JSON]` or `app [--connect ADDR] --method METHOD [JSON]`: one
/// command or control-channel method in the running app.
pub fn app(args: &[String]) -> Result<Value, String> {
    let (mut addr, mut method, mut rest) = (DEFAULT_ADDR.to_string(), None::<String>, vec![]);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--connect" | "--port" => addr = it.next().cloned().ok_or_else(|| format!("{a} needs an address"))?,
            "--method" | "-m" => method = Some(it.next().cloned().ok_or("--method needs a method name")?),
            _ => rest.push(a.as_str()),
        }
    }
    let json_arg = |s: Option<&&str>| -> Result<Value, String> {
        let Some(t) = s else { return Ok(json!({})) };
        let v: Value = serde_json::from_str(t).map_err(|e| format!("bad JSON {t}: {e}"))?;
        if v.is_object() { Ok(v) } else { Err(format!("parameters must be a JSON object, got {t}")) }
    };
    let (call, params) = match method {
        Some(m) => {
            if rest.len() > 1 {
                return Err("app --method takes one JSON argument".into());
            }
            (m, json_arg(rest.first())?)
        }
        None => {
            let [cmd, p @ ..] = rest.as_slice() else { return Err("usage: vectorcraft-cli app [--connect ADDR] COMMAND [JSON]".into()) };
            if p.len() > 1 {
                return Err("app takes one command and one JSON argument".into());
            }
            ("engine.execute".to_string(), json!({"command": cmd, "params": json_arg(p.first())?}))
        }
    };
    connect(&addr)?.call(&call, params)
}

/// Options shared by every file of a conversion.
pub struct ConvertOpts {
    pub scale: f64,
    pub artboard: Option<u64>,
    pub range: Option<String>,
    pub outline_text: bool,
    pub replace_lossy: bool,
}

/// Open `input` in its own session and export `output`.
pub fn convert_one(input: &str, output: &str, o: &ConvertOpts) -> Result<Value, String> {
    let mut h = Headless::new();
    h.call("app.open", json!({"path": input})).map_err(|e| format!("open {input}: {e}"))?;
    // A conversion keeps hidden layers and objects, written hidden, where the format can (SVG,
    // PSD).
    let params = json!({"path": output, "scale": o.scale, "artboard": o.artboard, "range": o.range, "outlineText": o.outline_text, "hiddenLayers": true, "acknowledgeLoss": o.replace_lossy});
    h.call("engine.execute", json!({"command": "document.export", "params": params})).map_err(|e| format!("export {output}: {e}"))
}

/// Where `input` goes when converted to `ext`: next to it, or in `out_dir`.
pub fn output_path(input: &str, ext: &str, out_dir: Option<&str>) -> Result<String, String> {
    let p = std::path::Path::new(input);
    let stem = p.file_stem().ok_or_else(|| format!("{input}: no file name"))?;
    let dir = match out_dir {
        Some(d) => std::path::PathBuf::from(d),
        None => p.parent().map(std::path::Path::to_path_buf).unwrap_or_default(),
    };
    let mut name = stem.to_os_string();
    name.push(".");
    name.push(ext.trim_start_matches('.'));
    Ok(dir.join(name).to_string_lossy().into_owned())
}

/// Convert every file in `inputs` to `ext`, `jobs` at a time, each in its own session. `emit`
/// gets one `{in, out, ok, result | error}` per file as it finishes (in finishing order); the
/// result is how many failed. A failure (or a panic) costs that file only. Two inputs that would
/// write the same file (`logo.svg` and `logo.ai` to `logo.pdf`) stop the run before it starts.
pub fn convert_many(
    inputs: &[String],
    ext: &str,
    out_dir: Option<&str>,
    jobs: usize,
    o: &ConvertOpts,
    emit: &(dyn Fn(Value) + Sync),
) -> Result<usize, String> {
    let plan: Vec<(&str, String)> = inputs.iter().map(|i| Ok((i.as_str(), output_path(i, ext, out_dir)?))).collect::<Result<_, String>>()?;
    let mut seen = std::collections::HashMap::new();
    for (input, out) in &plan {
        if let Some(first) = seen.insert(out.as_str(), *input) {
            return Err(format!("{first} and {input} would both be written to {out}; convert them into different --out-dir folders"));
        }
    }
    let next = AtomicUsize::new(0);
    let failed = AtomicUsize::new(0);
    let one = |input: &str, out: &str| -> Result<Value, String> {
        if std::path::Path::new(out) == std::path::Path::new(input) {
            return Err("the output would replace the input; pick another --to or an --out-dir".into());
        }
        vectorcraft_engine::guard::catch_panic(|| convert_one(input, out, o)).unwrap_or_else(|e| Err(format!("internal error: {e}")))
    };
    std::thread::scope(|s| {
        for _ in 0..jobs.clamp(1, plan.len().max(1)) {
            s.spawn(|| {
                while let Some((input, out)) = plan.get(next.fetch_add(1, Ordering::Relaxed)) {
                    match one(input, out) {
                        Ok(r) => emit(json!({"in": input, "out": out, "ok": true, "result": r})),
                        Err(e) => {
                            failed.fetch_add(1, Ordering::Relaxed);
                            emit(json!({"in": input, "out": out, "ok": false, "error": e}));
                        }
                    }
                }
            });
        }
    });
    Ok(failed.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_from_lines_and_arrays() {
        let lines =
            "# a comment\n\n\"select.all\"\n{\"command\":\"paint.setFill\",\"params\":{\"color\":\"#f00\"}}\n// another\n{\"id\":\"object.group\"}\n";
        let want = vec![
            Step { command: "select.all".into(), params: json!({}) },
            Step { command: "paint.setFill".into(), params: json!({"color": "#f00"}) },
            Step { command: "object.group".into(), params: json!({}) },
        ];
        assert_eq!(parse_steps(lines).unwrap(), want);
        let array = r##"["select.all", {"command":"paint.setFill","params":{"color":"#f00"}}, {"id":"object.group","params":null}]"##;
        assert_eq!(parse_steps(array).unwrap(), want);
    }

    #[test]
    fn bad_steps_name_their_line() {
        assert!(parse_steps("\"a.b\"\n{nope").unwrap_err().contains("line 2"));
        assert!(parse_steps("{\"command\":\"a.b\",\"parms\":{}}").unwrap_err().contains("parms"));
        assert!(parse_steps("{\"command\":\"a.b\",\"params\":[1]}").unwrap_err().contains("object"));
        assert!(parse_steps("[1]").unwrap_err().contains("step 0"));
        assert!(parse_steps("{}").unwrap_err().contains("command"));
    }

    #[test]
    fn output_paths() {
        assert_eq!(output_path("a/b/logo.svg", "pdf", None).unwrap(), "a/b/logo.pdf");
        assert_eq!(output_path("logo.svg", ".png", Some("out")).unwrap(), std::path::Path::new("out").join("logo.png").to_string_lossy());
        assert_eq!(output_path("x.tar.svg", "pdf", None).unwrap(), "x.tar.pdf");
    }

    #[test]
    fn control_addresses() {
        assert_eq!(control_addr("7979"), "127.0.0.1:7979");
        assert_eq!(control_addr("localhost:7000"), "localhost:7000");
    }
}
