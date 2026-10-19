# Performance budgets

`cargo xtask perf` times everyday editing on a dense document and checks each scenario against a
budget in [`perf/budgets.toml`](../perf/budgets.toml). It runs every night (and on demand) in
`.github/workflows/perf-nightly.yml`, never on pull requests. The harness is PhotoCraft's, so
reports and baselines read the same way in both apps.

```sh
cargo xtask perf                        # build the benches in release, run them, check the budgets
cargo xtask perf --quick                # 1,000-path document: a smoke run, budgets not applied
cargo xtask perf --reuse                # re-judge the last reports in target/perf/ without running
cargo xtask perf --update-baseline      # also record this run in perf/baseline.json
cargo xtask perf --threshold 25         # regression threshold in % (default 15)
```

Run it on an idle machine. The summary warns when the load average was above ¾ of the core
count at the start: the timings are wall-clock, and a busy machine makes them noise.

## What is measured

The bench is `crates/engine/examples/perf_scenarios.rs`. It builds a dense document of 10,000
paths (stroked ellipses, translucent rectangles, stroked stars) and 200 point-type labels on a
1600×1200 pt page, and times what the app does for each interaction:

- **Drags** (Selection tool on 1 and on 1,000 objects, a Direct Selection handle on a
  5,000-anchor path): every sample is one pointer move through `Session::pointer` (the tool event
  and the preview it applies) plus the canvas refresh.
- **Canvas refreshes** at fit, after a pan and at 400 %: `vectorcraft_render::Renderer` at a
  1920×1080 viewport. The desktop canvas renders on the CPU and re-renders the viewport on every
  document revision (`crates/ui-egui/src/canvas.rs`), so this is the cost of a frame after any
  change. No GPU is needed.
- **Commands** through `Session::execute` (the path the UI, CLI and MCP share), then a refresh:
  Move, fill and stroke changes on 1,000 objects, undo/redo, Select All, a Drop Shadow and a
  Gaussian Blur radius change on 200 objects, typing into and resizing a 5,000-word area text,
  Pathfinder Unite and Minus Front.
- **Open and save** of the dense document as SVG, PDF-compatible `.ai` and `.vectorcraft`.

Every row reports p50, p95 and max over its samples, and the peak resident memory sampled while
it ran (Linux). A row that fails or panics is reported as missing, with its error, and fails the
run; the other rows still run.

`vectorcraft-cli perf` is the quick local check of the same kind (render, hit test, save/open,
Pathfinder and anchor drags on a 50,000-path document) and `cargo run --release -p
vectorcraft-render --example bench` times the renderer alone.

## Budgets

| Class | Budget | Why |
|---|---|---|
| A drag frame or a canvas refresh | ≤ 16–25 ms p95 | one display frame at 60 Hz, with headroom |
| A command and the refresh after it | ≤ 50–100 ms p95 | feels immediate |
| Opening or saving a large file | ≤ 1–3 s p95 | a short, visible wait |

A scenario with `enforce = false` misses its budget today: the report shows it as "over budget"
and only a regression fails the run. The pull request that brings it within budget flips it to
`enforce = true`. Never raise a budget to make a run pass; a scenario the code can't measure yet
gets `not_measurable = "why"` instead of a number.

## Results and baselines

Each run writes `target/perf/results.json` (every scenario, the machine, the load average, the
commit), `target/perf/summary.md` (the Markdown table the nightly job posts as its summary) and
one `target/perf/<bench>.json` per bench with the raw samples.

A scenario regresses when its p50 grows more than 15 % (and more than 1 ms) over
`perf/baseline.json`. Baselines are compared only within one machine class (OS, architecture,
CPU model and core count); on another machine the regression check is skipped and only the
budgets apply. To record a baseline for the nightly runner, run the workflow by hand with
**update_baseline**, download the `perf-baseline` artifact and commit it as
`perf/baseline.json`.

## Adding a scenario

1. Add a row to `perf_scenarios.rs` (or a new example that writes the same `--json` report and
   a `[[bench]]` for it). Row names are the key: keep them stable.
2. Add a `[[scenario]]` with the next id, the bench and row, and a budget from the classes above.
3. Run `cargo xtask perf --bench perf_scenarios` and set `enforce = false` if it misses today.
