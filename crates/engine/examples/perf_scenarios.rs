//! Performance budget scenarios for `cargo xtask perf`: interactive edits on a dense document,
//! each timed through the paths the app takes (`Session::pointer` for tool drags,
//! `Session::execute` for commands, which the UI, CLI and MCP share) plus the canvas refresh the
//! app does after a change (`vectorcraft_render::Renderer` at a 1920×1080 viewport: the desktop
//! canvas renders on the CPU and uploads the image, see `ui-egui/src/canvas.rs`).
//!
//! ```sh
//! cargo run --release -p vectorcraft-engine --example perf_scenarios -- [--quick] [--json out.json] [--only text] [--reps N] [--paths N]
//! ```
//!
//! `cargo xtask perf` runs this and maps the rows to scenario ids in `perf/budgets.toml`, so keep
//! row names stable. `--quick` uses a 1,000-path document (seconds, for smoke runs); its numbers
//! are only comparable with other `--quick` runs. Every row records p50 / p95 / max over its
//! samples and the peak resident memory sampled while it ran (Linux).

use std::panic::AssertUnwindSafe;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use vectorcraft_color::{Color, Paint};
use vectorcraft_doc::{Appearance, Document, Node};
use vectorcraft_engine::tools::{Mods, PointerEvent, PointerKind};
use vectorcraft_engine::{Session, ViewInfo};
use vectorcraft_geom::{Affine, Anchor, PathData, Point, Rect, SubPath, Vec2, shapes};
use vectorcraft_render::{RenderOptions, Renderer};

type Res<T> = Result<T, String>;

/// The canvas viewport, in physical pixels.
const VIEW_W: u32 = 1920;
const VIEW_H: u32 = 1080;
/// The page of the synthetic document, in points.
const PAGE_W: f64 = 1600.0;
const PAGE_H: f64 = 1200.0;

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1).cloned())
}

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1000.0
}

fn exec(s: &mut Session, id: &str, p: Value) -> Res<Value> {
    s.execute(id, &p).map_err(|e| format!("{id}: {e}"))
}

fn id_of(v: &Value) -> Res<u64> {
    v.get("id").and_then(Value::as_u64).ok_or_else(|| format!("no id in {v}"))
}

// ---- memory ----------------------------------------------------------------------------------

/// A `kB` field of `/proc/self/status` (Linux), in bytes.
fn proc_status_bytes(key: &str) -> Option<u64> {
    let s = std::fs::read_to_string("/proc/self/status").ok()?;
    let line = s.lines().find(|l| l.starts_with(key))?;
    line.split_whitespace().nth(1)?.parse::<u64>().ok()?.checked_mul(1024)
}

/// Polls the resident set size on a background thread and keeps the maximum (Linux; nothing
/// elsewhere).
struct RssSampler {
    peak: Arc<AtomicU64>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl RssSampler {
    fn start() -> Self {
        let peak = Arc::new(AtomicU64::new(proc_status_bytes("VmRSS:").unwrap_or(0)));
        let stop = Arc::new(AtomicBool::new(false));
        let (p, st) = (Arc::clone(&peak), Arc::clone(&stop));
        let thread = std::thread::Builder::new()
            .name("rss-sampler".into())
            .spawn(move || {
                while !st.load(Ordering::Relaxed) {
                    if let Some(v) = proc_status_bytes("VmRSS:") {
                        p.fetch_max(v, Ordering::Relaxed);
                    }
                    std::thread::sleep(Duration::from_millis(3));
                }
            })
            .ok();
        Self { peak, stop, thread }
    }

    fn reset(&self) {
        self.peak.store(proc_status_bytes("VmRSS:").unwrap_or(0), Ordering::Relaxed);
    }

    fn peak(&self) -> Option<u64> {
        if let Some(v) = proc_status_bytes("VmRSS:") {
            self.peak.fetch_max(v, Ordering::Relaxed);
        }
        Some(self.peak.load(Ordering::Relaxed)).filter(|v| *v > 0)
    }
}

impl Drop for RssSampler {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            // A sampler thread that panicked has nothing to report.
            let _ = t.join();
        }
    }
}

// ---- rows ------------------------------------------------------------------------------------

/// Nearest-rank percentile `q` of sorted `v`.
fn rank(v: &[f64], q: f64) -> Option<f64> {
    let n = v.len();
    let k = ((q * n as f64).ceil() as usize).clamp(1, n.max(1));
    v.get(k - 1).copied()
}

/// One row in the shape `xtask perf` reads: p50 / p95 / max and the raw samples.
fn row(name: &str, samples: &[f64], peak_rss: Option<u64>) -> Value {
    let mut v: Vec<f64> = samples.iter().copied().filter(|x| x.is_finite()).collect();
    v.sort_by(f64::total_cmp);
    json!({
        "name": name,
        "n": v.len(),
        "p50_ms": rank(&v, 0.5),
        "p95_ms": rank(&v, 0.95),
        "max_ms": v.last(),
        "samples_ms": samples,
        "peak_rss_bytes": peak_rss,
    })
}

// ---- documents -------------------------------------------------------------------------------

/// Deterministic xorshift in 0..1.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % 1_000_000) as f64 / 1_000_000.0
    }
}

/// The dense document: `n` mixed paths (stroked ellipses, translucent rectangles, stroked stars)
/// and a point-type label per 50 paths on a 1600×1200 page, with an 80 pt square on top at the
/// page centre (the object the drags grab). Returns the document, the paths' ids and the square's.
fn dense(n: usize) -> Res<(Document, Vec<u64>, u64)> {
    let mut d = Document::new(PAGE_W, PAGE_H);
    let l = d.layers.first().map(|l| l.id);
    let mut r = Rng(42);
    let mut ids = Vec::with_capacity(n);
    for i in 0..n {
        let (x, y, s) = (r.next() * PAGE_W, r.next() * PAGE_H, 3.0 + r.next() * 22.0);
        let c = Color::rgb(r.next() as f32, r.next() as f32, r.next() as f32);
        let id = d.alloc_id();
        let mut node = match i % 3 {
            0 => Node::path(
                id,
                shapes::ellipse(Rect::from_center_size(Point::new(x, y), (2.0 * s, 2.0 * s))),
                Appearance::basic(Paint::solid(c), Paint::solid(Color::BLACK), 0.5),
            ),
            1 => Node::path(id, shapes::rectangle(Rect::new(x, y, x + 2.0 * s, y + s)), Appearance::basic(Paint::solid(c), Paint::None, 0.0)),
            _ => Node::path(id, shapes::star(Point::new(x, y), s, s / 2.0, 5, 0.0), Appearance::basic(Paint::None, Paint::solid(c), 2.0)),
        };
        if i % 3 == 1 {
            node.opacity = 0.8;
        }
        d.insert(l, usize::MAX, node).map_err(|e| format!("building the document: {e}"))?;
        ids.push(id.0);
    }
    for i in 0..n / 50 {
        let (x, y) = (r.next() * (PAGE_W - 100.0), r.next() * (PAGE_H - 40.0) + 20.0);
        let id = d.alloc_id();
        let style = vectorcraft_doc::CharStyle { size: 10.0 + (i % 5) as f64 * 4.0, ..Default::default() };
        let t = vectorcraft_doc::TextObject::point(Point::new(x, y), &format!("Label {i}"), style);
        d.insert(l, usize::MAX, Node::new(id, vectorcraft_doc::NodeKind::Text(Box::new(t)))).map_err(|e| format!("building the document: {e}"))?;
    }
    let target = d.alloc_id();
    let square = shapes::rectangle(Rect::new(PAGE_W / 2.0 - 40.0, PAGE_H / 2.0 - 40.0, PAGE_W / 2.0 + 40.0, PAGE_H / 2.0 + 40.0));
    let paint = Paint::solid(Color::rgb(0.9, 0.3, 0.1));
    d.insert(l, usize::MAX, Node::path(target, square, Appearance::basic(paint, Paint::solid(Color::BLACK), 1.0)))
        .map_err(|e| format!("building the document: {e}"))?;
    Ok((d, ids, target.0))
}

fn session_with(doc: Document) -> Session {
    let mut s = Session::new();
    s.add_document(doc, None);
    s
}

/// About `words` words of running text.
fn story(words: usize) -> String {
    const W: [&str; 12] = ["vector", "anchor", "stroke", "gradient", "artboard", "layer", "path", "curve", "swatch", "symbol", "glyph", "outline"];
    let mut out = String::with_capacity(words * 8);
    for i in 0..words {
        out.push_str(W.get(i.wrapping_mul(7) % W.len()).copied().unwrap_or("word"));
        out.push(if i % 97 == 96 { '\n' } else { ' ' });
    }
    out
}

// ---- canvas ----------------------------------------------------------------------------------

/// The canvas view: page centre at `center`, `zoom` (1 = 100 %), like `canvas.rs` builds it.
fn view(zoom: f64, center: Point) -> Affine {
    Affine::translate((f64::from(VIEW_W) / 2.0, f64::from(VIEW_H) / 2.0)) * Affine::scale(zoom) * Affine::translate(-center.to_vec2())
}

/// Fit-page zoom for the viewport.
fn fit_zoom() -> f64 {
    (f64::from(VIEW_W) / PAGE_W).min(f64::from(VIEW_H) / PAGE_H) * 0.95
}

fn canvas_opts() -> RenderOptions {
    RenderOptions { background: None, artboards: false, ..Default::default() }
}

struct Bench {
    renderer: Renderer,
    opts: RenderOptions,
    sampler: RssSampler,
    reps: usize,
    only: Option<String>,
    rows: Vec<Value>,
    /// Rows that couldn't run: (name, error).
    errors: Vec<(String, String)>,
    /// Time spent in [`Bench::render`] during the current sample.
    rendered: f64,
}

impl Bench {
    fn wanted(&self, name: &str) -> bool {
        self.only.as_ref().is_none_or(|o| name.contains(o.as_str()))
    }

    /// The canvas refresh after a change: the whole viewport re-rendered (the app re-renders on
    /// every document revision).
    fn render(&mut self, s: &Session, v: Affine) -> Res<f64> {
        let doc = Arc::clone(&s.doc().map_err(|e| e.to_string())?.doc);
        let t = Instant::now();
        std::hint::black_box(self.renderer.render(&doc, VIEW_W, VIEW_H, v, &self.opts));
        let took = ms(t);
        self.rendered += took;
        Ok(took)
    }

    /// Times `f` `reps` times after `warm` untimed runs; `f` returns its own time in ms. A panic
    /// inside is reported against the row instead of ending the run.
    fn time(&mut self, name: &str, s: &mut Session, reps: usize, warm: usize, mut f: impl FnMut(&mut Self, &mut Session, usize) -> Res<f64>) {
        if !self.wanted(name) {
            return;
        }
        self.sampler.reset();
        let mut samples = Vec::new();
        // Each sample less its canvas refresh, for rows that include one.
        let mut edit_only = Vec::new();
        let result = std::panic::catch_unwind(AssertUnwindSafe(|| -> Res<()> {
            for i in 0..warm {
                f(self, s, i)?;
            }
            for i in 0..reps.max(1) {
                self.rendered = 0.0;
                let v = f(self, s, warm + i)?;
                samples.push(v);
                if self.rendered > 0.0 {
                    edit_only.push((v - self.rendered).max(0.0));
                }
            }
            Ok(())
        }))
        .unwrap_or_else(|p| {
            let msg = p.downcast_ref::<String>().cloned().or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_default();
            Err(format!("panicked: {msg}"))
        });
        let ok = result.is_ok();
        self.record(name, &samples, result);
        // The same samples without the refresh (tracked in results.json, not budgeted): where a
        // slow frame's time goes. Rows that are only a render have nothing left.
        if ok && edit_only.len() == samples.len() && edit_only.iter().any(|v| *v > 0.05) {
            let r = row(&format!("{name} [without canvas render]"), &edit_only, None);
            let f = |k: &str| r.get(k).and_then(Value::as_f64).unwrap_or(f64::NAN);
            println!("  {:<62} p50 {:>9.2} ms  p95 {:>9.2} ms  (without the canvas render)", "", f("p50_ms"), f("p95_ms"));
            self.rows.push(r);
        }
    }

    fn record(&mut self, name: &str, samples: &[f64], result: Res<()>) {
        match result {
            Ok(()) => {
                let r = row(name, samples, self.sampler.peak());
                let f = |k: &str| r.get(k).and_then(Value::as_f64).unwrap_or(f64::NAN);
                println!(
                    "{name:<64} p50 {:>9.2} ms  p95 {:>9.2} ms  max {:>9.2} ms  (n {}, peak RSS {} MB)",
                    f("p50_ms"),
                    f("p95_ms"),
                    f("max_ms"),
                    samples.len(),
                    r.get("peak_rss_bytes").and_then(Value::as_u64).unwrap_or(0) >> 20,
                );
                self.rows.push(r);
            }
            Err(e) => {
                println!("{name:<64} ERROR {e}");
                self.errors.push((name.to_string(), e));
            }
        }
    }
}

// ---- scenarios -------------------------------------------------------------------------------

fn pointer(s: &mut Session, kind: PointerKind, pos: Point, zoom: f64) -> Res<()> {
    let ev = PointerEvent { kind, pos, mods: Mods::default(), pressure: 1.0 };
    s.pointer(&ev, ViewInfo { zoom, ..Default::default() }).map(drop).map_err(|e| format!("pointer {kind:?}: {e}"))
}

fn undo_len(s: &Session) -> Res<usize> {
    Ok(s.doc().map_err(|e| e.to_string())?.history.undo.len())
}

/// One drag with `tool` from `down`: every sample is one pointer move (the tool event and the
/// preview it applies) plus the canvas refresh. The drag must make one undo step, which is then
/// undone so the next scenario starts from the same document.
fn drag(b: &mut Bench, name: &str, s: &mut Session, tool: &str, down: Point, zoom: f64, v: Affine) {
    if !b.wanted(name) {
        return;
    }
    let reps = b.reps * 2;
    let setup = (|| -> Res<usize> {
        s.select_tool(tool, ViewInfo { zoom, ..Default::default() }).map_err(|e| e.to_string())?;
        let n = undo_len(s)?;
        pointer(s, PointerKind::Down, down, zoom)?;
        Ok(n)
    })();
    let before = match setup {
        Ok(n) => n,
        Err(e) => return b.record(name, &[], Err(e)),
    };
    let mut last = down;
    b.time(name, s, reps, 0, |b, s, i| {
        // A wandering drag: 3 pt steps, no snapping to one axis.
        last = down + Vec2::new(3.0 * (i + 1) as f64, 1.5 * (i + 1) as f64);
        let t = Instant::now();
        pointer(s, PointerKind::Drag, last, zoom)?;
        let event = ms(t);
        Ok(event + b.render(s, v)?)
    });
    let finish = (|| -> Res<()> {
        pointer(s, PointerKind::Up, last, zoom)?;
        if undo_len(s)? != before + 1 {
            return Err(format!("the {tool} drag made no undo step"));
        }
        exec(s, "edit.undo", json!({})).map(drop)
    })();
    if let Err(e) = finish {
        b.rows.retain(|r| r.get("name").and_then(Value::as_str) != Some(name));
        b.record(name, &[], Err(e));
    }
}

/// Drags, nudges, fills, undo and selection on the dense document.
fn edit_scenarios(b: &mut Bench, n: usize) {
    let (doc, ids, target) = match dense(n) {
        Ok(d) => d,
        Err(e) => return b.record("dense document", &[], Err(e)),
    };
    let mut s = session_with(doc);
    let (zoom, centre) = (fit_zoom(), Point::new(PAGE_W / 2.0, PAGE_H / 2.0));
    let fit = view(zoom, centre);
    // Warm the renderer's caches as the first frame on screen does.
    let _ = b.render(&s, fit);
    let reps = b.reps;
    let many: Vec<u64> = ids.iter().take(999).copied().chain([target]).collect();

    // Canvas refreshes: fit, a pan (the background refresh the canvas waits for) and 400 %.
    b.time("canvas render, fit page 1920x1080", &mut s, reps, 1, |b, s, _| b.render(s, fit));
    b.time("canvas render after a pan, 1920x1080", &mut s, reps, 1, |b, s, i| b.render(s, view(zoom, centre + Vec2::new(7.0 * (i + 1) as f64, 0.0))));
    let deep = view(zoom * 4.0, Point::new(PAGE_W * 0.4, PAGE_H * 0.4));
    b.time("canvas render at 400% zoom, 1920x1080", &mut s, reps, 1, |b, s, _| b.render(s, deep));

    // Selection tool drags of one and of 1,000 selected objects.
    let down = centre;
    if let Err(e) = exec(&mut s, "select.set", json!({"ids": [target]})) {
        b.record("selection drag, 1 object (per move + canvas render)", &[], Err(e));
    } else {
        drag(b, "selection drag, 1 object (per move + canvas render)", &mut s, "selection", down, zoom, fit);
    }
    if let Err(e) = exec(&mut s, "select.set", json!({"ids": many})) {
        b.record("selection drag, 1,000 objects (per move + canvas render)", &[], Err(e));
    } else {
        drag(b, "selection drag, 1,000 objects (per move + canvas render)", &mut s, "selection", down, zoom, fit);
    }

    // Commands on 1,000 selected objects: what Object › Transform › Move, a swatch click and the
    // Stroke panel run, then the canvas refresh.
    let _ = exec(&mut s, "select.set", json!({"ids": many}));
    b.time("object.move 1,000 objects + canvas render", &mut s, reps, 1, |b, s, i| {
        let d = if i % 2 == 0 { 2.0 } else { -2.0 };
        let t = Instant::now();
        exec(s, "object.move", json!({"dx": d, "dy": 0}))?;
        Ok(ms(t) + b.render(s, fit)?)
    });
    b.time("paint.setFill on 1,000 objects + canvas render", &mut s, reps, 1, |b, s, i| {
        let c = format!("#{:02x}{:02x}40", (i * 37) % 256, (i * 91) % 256);
        let t = Instant::now();
        exec(s, "paint.setFill", json!({"color": c}))?;
        Ok(ms(t) + b.render(s, fit)?)
    });
    b.time("stroke.set weight on 1,000 objects + canvas render", &mut s, reps, 1, |b, s, i| {
        let t = Instant::now();
        exec(s, "stroke.set", json!({"weight": 1.0 + (i % 4) as f64}))?;
        Ok(ms(t) + b.render(s, fit)?)
    });
    // Undo and redo of a move of the 1,000 objects, alternating.
    let _ = exec(&mut s, "object.move", json!({"dx": 5, "dy": 5}));
    b.time("undo / redo a move of 1,000 objects + canvas render", &mut s, reps * 2, 2, |b, s, i| {
        let id = if i % 2 == 0 { "edit.undo" } else { "edit.redo" };
        let t = Instant::now();
        exec(s, id, json!({}))?;
        Ok(ms(t) + b.render(s, fit)?)
    });
    b.time("select all", &mut s, reps, 1, |_, s, _| {
        let _ = exec(s, "select.set", json!({"ids": []}));
        let t = Instant::now();
        exec(s, "select.all", json!({}))?;
        Ok(ms(t))
    });

    // Live effects on 200 objects: applying a drop shadow, then a Gaussian Blur radius drag.
    let fx: Vec<u64> = ids.iter().step_by(ids.len().div_ceil(200).max(1)).take(200).copied().collect();
    let _ = exec(&mut s, "select.set", json!({"ids": fx}));
    b.time("effect.apply drop shadow on 200 objects + canvas render", &mut s, reps.min(7), 0, |b, s, _| {
        let t = Instant::now();
        exec(s, "effect.apply", json!({"effect": "stylize.dropShadow"}))?;
        let v = ms(t) + b.render(s, fit)?;
        exec(s, "edit.undo", json!({}))?;
        Ok(v)
    });
    let blur = exec(&mut s, "effect.apply", json!({"effect": "blur.gaussian", "params": {"radius": 2}}));
    match blur {
        Err(e) => b.record("Gaussian Blur radius change on 200 objects + canvas render", &[], Err(e)),
        Ok(_) => b.time("Gaussian Blur radius change on 200 objects + canvas render", &mut s, reps.min(9), 1, |b, s, i| {
            let t = Instant::now();
            exec(s, "effect.setParams", json!({"index": 0, "params": {"radius": 2.0 + (i % 5) as f64}}))?;
            Ok(ms(t) + b.render(s, fit)?)
        }),
    }
}

/// Direct Selection drags of a handle of a 5,000-anchor path among the dense document's paths.
fn anchor_scenarios(b: &mut Bench, n: usize) {
    let name = "direct selection handle drag, 5,000-anchor path (per move + canvas render)";
    if !b.wanted(name) {
        return;
    }
    let built = dense(n).and_then(|(mut doc, _, _)| {
        let l = doc.layers.first().map(|l| l.id);
        let id = doc.alloc_id();
        // Anchors 2 pt apart, up and down by 20 pt, their handles 0.6 pt long.
        let anchor = |i: usize| {
            let p = Point::new(i as f64 * 2.0, if i.is_multiple_of(2) { 500.0 } else { 520.0 });
            Anchor::smooth(p, p + Vec2::new(0.6, 0.0))
        };
        let path = PathData::single(SubPath::new((0..5000).map(anchor).collect(), false));
        doc.insert(l, usize::MAX, Node::path(id, path, Appearance::default_art())).map_err(|e| e.to_string())?;
        Ok((doc, id))
    });
    let (doc, id) = match built {
        Ok(d) => d,
        Err(e) => return b.record(name, &[], Err(e)),
    };
    let mut s = session_with(doc);
    // Zoomed to 800 % around anchor 400 at (800, 500), as one does to edit a point.
    let (zoom, centre) = (8.0, Point::new(800.0, 510.0));
    let v = view(zoom, centre);
    let _ = b.render(&s, v);
    if let Err(e) = exec(&mut s, "select.anchors", json!({"id": id.0, "anchors": [[0, 400]]})) {
        return b.record(name, &[], Err(e));
    }
    // The outgoing handle's end of anchor 400.
    drag(b, name, &mut s, "directSelection", Point::new(800.6, 500.0), zoom, v);
}

/// Typing into and resizing a 5,000-word area type frame.
fn text_scenarios(b: &mut Bench) {
    let names = ["type a character in a 5,000-word area text + canvas render", "resize a 5,000-word area text frame (reflow) + canvas render"];
    if !names.iter().any(|n| b.wanted(n)) {
        return;
    }
    let mut s = session_with(Document::new(PAGE_W, PAGE_H));
    let created = exec(&mut s, "text.create", json!({"x": 100, "y": 60, "text": story(5000), "size": 10, "area": {"width": 1400, "height": 1100}}));
    let id = match created.and_then(|v| id_of(&v)) {
        Ok(id) => id,
        Err(e) => {
            for n in names {
                b.record(n, &[], Err(e.clone()));
            }
            return;
        }
    };
    let fit = view(fit_zoom(), Point::new(PAGE_W / 2.0, PAGE_H / 2.0));
    let _ = b.render(&s, fit);
    let reps = b.reps;
    // Keystrokes in the middle of the story: each one lays the story out again.
    let mut at = story(2500).len();
    b.time(names[0], &mut s, reps * 2, 1, |b, s, _| {
        let t = Instant::now();
        exec(s, "text.editRange", json!({"id": id, "start": at, "end": at, "insert": "x"}))?;
        at += 1;
        Ok(ms(t) + b.render(s, fit)?)
    });
    b.time(names[1], &mut s, reps, 1, |b, s, i| {
        let dx = if i % 2 == 0 { -30.0 } else { 30.0 };
        let t = Instant::now();
        exec(s, "text.reshapeArea", json!({"id": id, "anchors": [[0, 1], [0, 2]], "dx": dx, "dy": 0}))?;
        Ok(ms(t) + b.render(s, fit)?)
    });
}

/// Pathfinder on many small paths and on two complex shapes.
fn pathfinder_scenarios(b: &mut Bench, quick: bool) {
    let unite = "Pathfinder Unite, 1,000 paths";
    let count = if quick { 300 } else { 1000 };
    if b.wanted(unite) {
        match dense(count) {
            Err(e) => b.record(unite, &[], Err(e)),
            Ok((doc, ids, _)) => {
                let mut s = session_with(doc);
                b.time(unite, &mut s, b.reps.min(5), 0, |_, s, _| {
                    exec(s, "select.set", json!({"ids": ids}))?;
                    let t = Instant::now();
                    exec(s, "object.pathfinder.unite", json!({}))?;
                    let v = ms(t);
                    exec(s, "edit.undo", json!({}))?;
                    Ok(v)
                });
            }
        }
    }
    let minus = "Pathfinder Minus Front, two 2,000-anchor stars";
    if b.wanted(minus) {
        let mut d = Document::new(PAGE_W, PAGE_H);
        let l = d.layers.first().map(|l| l.id);
        let mut ids = vec![];
        let built = (|| -> Res<()> {
            for (c, rot) in [(Point::new(800.0, 600.0), 0.0), (Point::new(830.0, 610.0), 0.07)] {
                let id = d.alloc_id();
                let star = shapes::star(c, 500.0, 420.0, 1000, rot);
                d.insert(l, usize::MAX, Node::path(id, star, Appearance::default_art())).map_err(|e| e.to_string())?;
                ids.push(id.0);
            }
            Ok(())
        })();
        if let Err(e) = built {
            return b.record(minus, &[], Err(e));
        }
        let mut s = session_with(d);
        b.time(minus, &mut s, b.reps.min(5), 0, |_, s, _| {
            exec(s, "select.set", json!({"ids": ids}))?;
            let t = Instant::now();
            exec(s, "object.pathfinder.minusFront", json!({}))?;
            let v = ms(t);
            exec(s, "edit.undo", json!({}))?;
            Ok(v)
        });
    }
}

/// Open and save of the dense document as SVG, PDF-compatible .ai and the native format: the
/// encoders and importers File › Save As and File › Open use, plus writing the file.
fn file_scenarios(b: &mut Bench, n: usize) {
    let doc = match dense(n) {
        Ok((d, _, _)) => d,
        Err(e) => return b.record("dense document", &[], Err(e)),
    };
    let dir = std::env::temp_dir().join(format!("vectorcraft-perf-{}", std::process::id()));
    if let Err(e) = std::fs::create_dir_all(&dir) {
        return b.record("save .svg", &[], Err(e.to_string()));
    }
    let reps = b.reps.min(5);
    let mut s = session_with(doc);
    for (format, ext, label) in [("svg", "svg", ".svg"), ("ai", "ai", ".ai"), ("vectorcraft", vectorcraft_format::EXTENSION, ".vectorcraft")] {
        let (save, open) = (format!("save {label} (dense document)"), format!("open {label} (dense document)"));
        let path = dir.join(format!("dense.{ext}"));
        let mut bytes: Vec<u8> = vec![];
        b.time(&save, &mut s, reps, 1, |_, s, _| {
            let t = Instant::now();
            let doc = Arc::clone(&s.doc().map_err(|e| e.to_string())?.doc);
            bytes = vectorcraft_engine::cmd::fileio::encode(&doc, format, &json!({})).map_err(|e| format!("{format}: {e}"))?;
            std::fs::write(&path, &bytes).map_err(|e| e.to_string())?;
            Ok(ms(t))
        });
        if bytes.is_empty() {
            b.record(&open, &[], Err(format!("no {label} file was written")));
            continue;
        }
        let name = format!("dense.{ext}");
        b.time(&open, &mut s, reps, 1, |_, _, _| {
            let mut fresh = Session::new();
            let t = Instant::now();
            let data = std::fs::read(&path).map_err(|e| e.to_string())?;
            vectorcraft_engine::cmd::fileio::open_bytes(&mut fresh, &name, &data, None).map_err(|e| format!("open {name}: {e}"))?;
            let v = ms(t);
            // Dropping the opened document isn't part of opening it.
            drop(fresh);
            Ok(v)
        });
    }
    // Best effort: the files are in the temp folder anyway.
    let _ = std::fs::remove_dir_all(&dir);
}

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let quick = args.iter().any(|a| a == "--quick");
    let n: usize = arg(&args, "--paths").and_then(|v| v.parse().ok()).unwrap_or(if quick { 1_000 } else { 10_000 }).clamp(10, 1_000_000);
    let reps: usize = arg(&args, "--reps").and_then(|v| v.parse().ok()).unwrap_or(if quick { 5 } else { 15 }).clamp(1, 10_000);
    let mut b = Bench {
        renderer: Renderer::new(),
        opts: canvas_opts(),
        sampler: RssSampler::start(),
        reps,
        only: arg(&args, "--only"),
        rows: vec![],
        errors: vec![],
        rendered: 0.0,
    };
    let cores = std::thread::available_parallelism().map_or(1, |c| c.get());
    println!("perf_scenarios: {n} paths, {reps} reps, {cores} cores, viewport {VIEW_W}x{VIEW_H}{}", if quick { " (quick)" } else { "" });
    let t = Instant::now();
    edit_scenarios(&mut b, n);
    anchor_scenarios(&mut b, n);
    text_scenarios(&mut b);
    pathfinder_scenarios(&mut b, quick);
    file_scenarios(&mut b, n);
    println!("done in {:.1} s", t.elapsed().as_secs_f64());

    let errors: Vec<Value> = b.errors.iter().map(|(name, e)| json!({"name": name, "error": e})).collect();
    let report = json!({
        "bench": "perf_scenarios",
        "schema": 1,
        "context": {"paths": n, "reps": reps, "quick": quick, "cores": cores, "viewport": [VIEW_W, VIEW_H], "renderer": "cpu", "errors": errors},
        "rows": b.rows,
        "process_peak_rss_bytes": proc_status_bytes("VmHWM:").or_else(|| b.sampler.peak()),
    });
    if let Some(path) = arg(&args, "--json") {
        let written =
            serde_json::to_string_pretty(&report).map_err(|e| e.to_string()).and_then(|t| std::fs::write(&path, t).map_err(|e| e.to_string()));
        if let Err(e) = written {
            eprintln!("error: writing {path}: {e}");
            return std::process::ExitCode::FAILURE;
        }
    }
    if b.errors.is_empty() {
        std::process::ExitCode::SUCCESS
    } else {
        eprintln!("{} row(s) failed: {}", b.errors.len(), b.errors.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>().join("; "));
        // The rows that ran are in the report; xtask perf marks the failed ones missing.
        std::process::ExitCode::SUCCESS
    }
}
