//! `drawcraft-cli`: DrawCraft from the command line.
//!
//! ```text
//! drawcraft-cli mcp [--connect 127.0.0.1:7979 | --headless]
//! drawcraft-cli run [--in file.drawcraft|file.svg] [--cmd id [--params '{json}']]... [--export out.svg|.png|.drawcraft]... [--scale 2]
//! drawcraft-cli commands
//! drawcraft-cli bench FILE [--size 2880x1800] [--iters 5]
//! ```
#![forbid(unsafe_code)]

use std::io::Write;
use std::process::ExitCode;

use drawcraft_mcp::{Backend, DEFAULT_ADDR, Headless, Remote, Server};
use serde_json::{Value, json};

const USAGE: &str = "\
drawcraft-cli — DrawCraft automation

USAGE:
  drawcraft-cli mcp [--connect ADDR | --headless]
      Run the MCP server on stdio. Default: connect to a running app at 127.0.0.1:7979
      (drawcraft --control 7979), falling back to a headless in-process session.

  drawcraft-cli run [--in FILE] [--cmd ID [--params JSON]]... [--export FILE]... [--scale N]
      Headless batch: open FILE (.drawcraft/.svg) or start a new document, run commands in
      order, export (.svg, .png, .drawcraft by extension). Prints one JSON result per step.

  drawcraft-cli commands
      Print the command catalogue as JSON.

  drawcraft-cli bench FILE [--size WxH] [--iters N]
      Render FILE (.drawcraft/.svg) fitted to WxH (default 2880x1800) and print ms per frame
      (warm), multithreaded and single-threaded.
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let r = match args.first().map(String::as_str) {
        Some("mcp") => mcp(&args[1..]),
        Some("run") => run(&args[1..]),
        Some("commands") => commands(),
        Some("bench") => bench(&args[1..]),
        Some("-h" | "--help" | "help") | None => {
            print!("{USAGE}");
            Ok(())
        }
        Some("-V" | "--version") => {
            println!("drawcraft-cli {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Some(other) => Err(format!("unknown subcommand `{other}`\n\n{USAGE}")),
    };
    match r {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("drawcraft-cli: {e}");
            ExitCode::FAILURE
        }
    }
}

fn mcp(args: &[String]) -> Result<(), String> {
    let mut connect: Option<String> = None;
    let mut headless = false;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--connect" => connect = Some(it.next().cloned().ok_or("--connect needs an address")?),
            "--headless" => headless = true,
            other => return Err(format!("unknown mcp option `{other}`")),
        }
    }
    if headless && connect.is_some() {
        return Err("use either --connect or --headless".into());
    }
    let backend: Box<dyn Backend> = if headless {
        Box::new(Headless::with_document())
    } else if let Some(addr) = connect {
        // Explicit address: fail loudly if the app isn't there.
        Box::new(Remote::connect(&addr).map_err(|e| format!("cannot connect to {addr}: {e}"))?)
    } else {
        match Remote::connect(DEFAULT_ADDR) {
            Ok(r) => Box::new(r),
            Err(_) => Box::new(Headless::with_document()),
        }
    };
    eprintln!("drawcraft-cli: MCP server on stdio ({})", backend.describe());
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    Server::new(backend).serve(stdin.lock(), stdout.lock()).map_err(|e| e.to_string())
}

fn commands() -> Result<(), String> {
    let mut h = Headless::new();
    let v = h.call("engine.commands", json!({}))?;
    println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
    Ok(())
}

enum Step {
    Cmd(String, Value),
    Export(String),
}

fn run(args: &[String]) -> Result<(), String> {
    let mut input: Option<String> = None;
    let mut steps: Vec<Step> = vec![];
    let mut scale = 1.0;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut val = |what: &str| it.next().cloned().ok_or_else(|| format!("{a} needs {what}"));
        match a.as_str() {
            "--in" | "-i" => input = Some(val("a file")?),
            "--cmd" | "-c" => steps.push(Step::Cmd(val("a command id")?, json!({}))),
            "--params" | "-p" => {
                let raw = val("JSON")?;
                let p: Value = serde_json::from_str(&raw).map_err(|e| format!("--params {raw}: {e}"))?;
                if !p.is_object() {
                    return Err(format!("--params must be a JSON object, got {raw}"));
                }
                match steps.last_mut() {
                    Some(Step::Cmd(_, params)) => *params = p,
                    _ => return Err("--params must follow a --cmd".into()),
                }
            }
            "--export" | "-o" => steps.push(Step::Export(val("a file")?)),
            "--scale" | "-s" => {
                let raw = val("a number")?;
                scale = raw.parse::<f64>().map_err(|_| format!("--scale {raw}: not a number"))?;
            }
            other => return Err(format!("unknown run option `{other}`")),
        }
    }

    let mut h = Headless::new();
    let mut out = std::io::stdout().lock();
    let mut emit = |v: Value| writeln!(out, "{v}").map_err(|e| e.to_string());
    if let Some(path) = &input {
        let r = h.call("app.open", json!({"path": path})).map_err(|e| format!("open {path}: {e}"))?;
        emit(json!({"step": "open", "path": path, "result": r}))?;
    } else if !matches!(steps.first(), Some(Step::Cmd(id, _)) if id == "file.new") {
        h.ensure_document();
    }
    for step in steps {
        match step {
            Step::Cmd(id, params) => {
                let r = h.call("engine.execute", json!({"command": id, "params": params})).map_err(|e| format!("{id}: {e}"))?;
                emit(json!({"step": "cmd", "command": id, "result": r}))?;
            }
            Step::Export(path) => {
                let r = h.call("app.export", json!({"path": path, "scale": scale})).map_err(|e| format!("export {path}: {e}"))?;
                emit(json!({"step": "export", "result": r}))?;
            }
        }
    }
    Ok(())
}

fn bench(args: &[String]) -> Result<(), String> {
    let mut file = None;
    let (mut w, mut h, mut iters) = (2880u32, 1800u32, 5u32);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--size" => {
                let v = it.next().ok_or("--size needs WxH")?;
                let (a, b) = v.split_once('x').ok_or("--size needs WxH")?;
                w = a.parse().map_err(|_| "bad width")?;
                h = b.parse().map_err(|_| "bad height")?;
            }
            "--iters" => iters = it.next().and_then(|v| v.parse().ok()).ok_or("--iters needs a number")?,
            f if file.is_none() => file = Some(f.to_string()),
            other => return Err(format!("unknown bench option `{other}`")),
        }
    }
    let file = file.ok_or("bench needs a FILE")?;
    let mut hl = Headless::new();
    hl.call("app.open", json!({ "path": file })).map_err(|e| format!("open {file}: {e}"))?;
    let doc = hl.session.doc().map_err(|e| e.to_string())?.doc.clone();
    let b = doc.artboards.first().map(|a| a.rect).ok_or("document has no artboard")?;
    let z = (w as f64 / b.width()).min(h as f64 / b.height()) * 0.95;
    let view = drawcraft_geom::Affine::translate((w as f64 / 2.0, h as f64 / 2.0))
        * drawcraft_geom::Affine::scale(z)
        * drawcraft_geom::Affine::translate(-b.center().to_vec2());
    let opts = drawcraft_render::RenderOptions::default();
    println!("{file}: {} nodes, {w}x{h}", doc.layers.iter().map(|l| l.count()).sum::<usize>());
    for threads in [drawcraft_render::default_threads(), 0] {
        let mut r = drawcraft_render::Renderer::new();
        r.threads = threads;
        r.render(&doc, w, h, view, &opts);
        let t = std::time::Instant::now();
        for _ in 0..iters {
            r.render(&doc, w, h, view, &opts);
        }
        println!(
            "  threads {threads}: {:.1} ms/frame (drawn {}, culled {})",
            t.elapsed().as_secs_f64() * 1000.0 / iters as f64,
            r.stats.drawn,
            r.stats.culled
        );
    }
    Ok(())
}
