# DrawCraft — instructions for agents

DrawCraft is a clean-room, open-source, Rust-native vector illustration app targeting Adobe Illustrator parity (and beyond). It runs natively on macOS, Windows and Linux, and on the web via WASM. Siblings: `../photocraft` (Photoshop-class) and `../printcraft` (Acrobat-class); same conventions.

## Start every session here
1. Read `plan/STATUS.md` (current milestone, next task), then the task in `plan/execution-plan.md` §3 and the relevant `plan/architecture.md` section. Behaviour reference: `plan/illustrator/*.md`.
2. Follow the autonomous operation protocol (`plan/execution-plan.md` §7): orient → plan → implement + test → verify → record → commit. Don't stop to ask unless §7 lists the decision as the user's.

`plan/` is gitignored (local only).

## Non-negotiables
- **Clean-room.** Never read, disassemble or copy anything inside the Illustrator bundle (names/listings only). Never copy Adobe icons, artwork, presets or wording beyond feature names. Behaviour comes from public docs and black-box observation of the running app with synthetic documents only (screenshots by window id, stored under `plan/illustrator/screenshots/`, never committed). Never copy GPL/AGPL code (Inkscape, lib2geom…).
- **Everything is a command.** User-visible behaviour = a command in `crates/engine/src/cmd/*` (id, label, menu path, shortcut, params doc, `enabled`, `run`) + tests. Tools emit commands (Begin/Preview/Commit). UI-only commands live in `crates/ui-egui/src/menus.rs` (`UI_COMMANDS`). The control channel and MCP reach all of them.
- **Layering** is enforced by `cargo xtask layers`. Nothing below L6 depends on egui/eframe/winit/rfd.
- **The UI is thin**: panels read engine state and act through `app.run(id, params)`. Colours come from `theme::Tokens`.
- **Rust only** (no handwritten JS/TS). **Never break wasm** (`cargo xtask wasm`).
- **Quality gates** before every commit: `cargo xtask ci` (fmt, clippy -D warnings, tests, layers, wasm). One task id per commit (`M2.1: pen tool`).

## Running and looking at the app
- `cargo run --release -p drawcraft -- --control 7979 [file.svg|file.drawcraft]`
- Drive it: JSON lines on `127.0.0.1:7979`, e.g. `{"id":1,"method":"engine.execute","params":{"command":"shape.rectangle","params":{"x":10,"y":10,"width":100,"height":50}}}` then `{"id":2,"method":"ui.screenshot","params":{"path":"/tmp/shot.png"}}`. Methods: `crates/ui-egui/src/control.rs`.
- **For UI work, look at the result** (take `ui.screenshot`, read the PNG) and compare with `plan/illustrator/02-ui-ux.md` / `10-observed-ui.md`.
- MCP: `drawcraft-cli mcp` (see `docs/mcp.md`).
- Shell gotcha: `mv`/`cp` are aliased interactive here — use `/bin/mv -f` / `/bin/cp -f`.
- Parallel agents: separate `CARGO_TARGET_DIR` per agent; edit only the crates you own; write manifests atomically.

## Roadmap
`ROADMAP.md` (committed) tracks status, milestones and time-to-parity estimates. Update it whenever a milestone task lands.
