# DrawCraft Roadmap

DrawCraft is a clean-room, open-source, pure-Rust reimplementation of the Adobe Illustrator workflow. It runs on macOS, Windows, Linux and the web (WASM), and agents can drive it fully over a JSON control channel and MCP.

This file tracks **how far we are and what's left**. Estimates are wall-clock hours of continuous agent work (Claude Opus 5.5 lead plus 4–6 parallel agents). They were made by counting the remaining work, not measured, and are updated as work lands.

_Last updated: 2026-09-30._

## Where we are

| Dimension | Status |
|---|---|
| Infrastructure (engine, command registry, history, render, formats, MCP, web, packaging, tests) | **~90%** |
| Look & feel vs Illustrator 2026 default workspace (measured) | **~75–80%** |
| Feature surface vs full Illustrator | **~45–50%** |
| Estimated time to **checklist parity** (every menu item, tool, panel and dialog functional) | **~170–260 h** |
| Estimated time to **1:1 feel** (edge cases, modifier nuances, typographic/colour fidelity) | **~500+ h** |

### Shipped so far
- **Architecture:** 19+ crates with enforced layering (`cargo xtask layers`). Every action is a command (~400 engine + ~50 UI). Undo is unlimited via structural sharing. `command.batch` runs several commands as one transaction.
- **Automation:**
  - Actions panel (record/playback, persisted), generic parameter dialogs for every "…" command.
  - JSON-lines control channel with real egui pointer and keyboard injection.
  - MCP server with 19 tools, which can attach to the running app or run headless.
  - Headless CLI (`drawcraft-cli`).
  - Actions panel that records and plays back commands.
- **UI:** Illustrator 2026 layout restyled to measured values:
  - Medium Dark theme, categorized and Advanced toolbars, 35 pt document tabs, 33 pt panel tabs.
  - Hint bar, contextual task bar, 19 dock panels with ≡ menus.
  - Native macOS menu bar, vector tool cursors, a ⌘K command palette.
  - Four brightness themes, persistent preferences.
- **Tools:**
  - **Selection:** Selection, Direct/Group Selection, Magic Wand, Lasso.
  - **Drawing:** Pen, Curvature, anchor tools, Pencil, Paintbrush, Blob Brush, Smooth, Path Eraser, Join.
  - **Shapes:** all shape tools and the line, arc, spiral and grid tools.
  - **Cutting:** Eraser, Scissors, Knife.
  - **Transform:** Rotate, Reflect, Scale, Shear, Free Transform (distort/perspective).
  - **Other:** Eyedropper, Gradient annotator, Artboard, Measure, Type, Hand, Zoom, Rotate View.
- **Drawing aids:** Smart Guides and snapping, and Draw Normal / Behind / Inside modes.
- **Geometry and effects:**
  - Pathfinder (10 exact curve booleans), Offset, Outline Stroke, Simplify, Clean Up, Split Into Grid, Divide Objects Below.
  - Live effects with previewing dialogs: Distort & Transform, Path, Convert to Shape, 15 Warp styles, Round Corners, Scribble, and raster drop shadow, glows and feather.
- **Advanced art:** live Blends (steps/distance/smooth colour, spine), Envelope Distort (warp/mesh/top object), Gradient Mesh, Shape Builder, Live Paint, Image Trace (12 presets), pattern swatches with pattern editing mode, live Repeat (radial/grid/mirror).
- **Colour, type and file workflows:** Recolor Artwork (dialog with harmonies), Edit Colors, Find & Replace, Change Case, Smart Punctuation, Guides, Lock/Hide Above, Transform Each, Rasterize.
- **Formats:**
  - `.drawcraft` (lossless JSON), SVG import/export, PDF export/import (including PDF-compatible `.ai`).
  - PNG, JPEG and WebP export, Export for Screens, Place.
- **Performance:** 20k shapes + 1k texts render in 27 ms per full-retina frame (7.8 ms zoomed), 7× faster than the first version. The UI thread never blocks. The web build is 7.1 MB gzipped.
- **Tests:** ~700 automated tests: model-based property tests, a junk-parameter sweep over every command, golden renders, and MCP end-to-end tests over stdio.

## Milestones and estimates

| # | Milestone | Status | Est. remaining (h) |
|---|---|---|---|
| M0 | Skeleton + vertical slice | ✅ done | — |
| M1 | Selection, transform, layers, MCP | ✅ mostly done (rotated persistent bbox pending) | 3–5 |
| M2 | Drawing tools + smart guides | ✅ mostly done (Shaper, Pen modifier nuances) | 5–10 |
| M3 | Paint & appearance (swatches, color, gradient, stroke, appearance, transparency, styles) | 🟡 panels done; opacity masks, freeform gradients pending | 10–15 |
| M4 | Files (native, SVG, PDF, raster, Export for Screens, clipboard interop) | 🟡 Export for Screens (PNG/JPG/WebP/SVG/PDF × scales) done; system clipboard SVG/PNG interop, EPS/DXF pending | 8–12 |
| M5 | Performance | 🟡 background render + caches + MT done; dirty-region rendering, GPU backend spike | 10–20 |
| M6 | Path operations (Pathfinder, Shape Builder, offset…) | ✅ mostly done (Shape Builder edge erase, large-offset bug open) | 3–6 |
| M7 | Type (point/area/path, editing, styles, OpenType, threading, glyphs) | 🟡 in progress | 20–30 |
| M8 | Transform & distort (Puppet Warp, Liquify tools, Envelopes, Blends, Perspective Grid) | 🟡 live Blends + Envelopes (warp/mesh/top object) done; Puppet Warp, Liquify, Perspective Grid pending | 20–30 |
| M9 | Live effects (+ 3D & Materials) | 🟡 2D effects done; 3D pending | 25–40 |
| M10 | Brushes, symbols, patterns, Repeat | 🟡 pattern swatches (5 tile types, Pattern Options, editing mode, SVG `<pattern>`/PDF export) and live Repeat (radial/grid/mirror) done; brushes/symbols in progress | 8–15 |
| M11 | Artboards & views (artboard panel/tool done; print tiling, multiple windows, presentation polish) | 🟡 | 10–15 |
| M12 | Advanced color & art (CMYK/ICC, separations, Gradient Mesh, Live Paint, Image Trace, Graphs) | 🟡 Gradient Mesh, Live Paint, Image Trace (12 presets, 18 ms/1k² image), Recolor Artwork done; CMYK/ICC, separations, Graphs pending | 25–40 |
| M13 | Automation (Actions ✅ record/playback, persisted; variables, scripting, batch) | 🟡 | 8–12 |
| M14 | 1.0 polish (preferences, shortcut editor, workspaces, accessibility, packaging for all OSes) | ⬜ | 20–30 |
| — | Interaction fidelity pass (every tool's modifiers, Properties panel per context, isolation, nuance) | ⬜ | 40–60 |
| — | Hardening at scale (big-file corpus, fuzzing, cross-platform + browser QA) | 🟡 | 30–50 |
| | **Total to checklist parity** | | **~170–260** |

## Out of scope (by design or by law)
- **Native `.ai` private data:** it's undocumented. We read the PDF-compatible part, so Illustrator-only live objects arrive as appearance.
- **Adobe cloud services** (Libraries sync, Adobe Fonts, Firefly/generative): these are pluggable provider APIs, not built in.
- **Adobe's bundled assets** (swatch/brush/symbol libraries, presets, icons): ours are original.

## Where we aim to be better than Illustrator
- **Speed:** off-thread multithreaded rendering, instant startup, a responsive UI on huge files.
- **Robustness:** exact curve booleans (no "cannot perform operation"), property-tested undo/redo and file round trips.
- **Openness:** a documented native format, first-class SVG, and the same app in the browser.
- **Automation:** every command, gesture, dialog and widget is drivable by agents (JSON channel + MCP), with headless batch mode.

## How to update this file
After each milestone task lands, update the status column and remaining estimates, and move items into "Shipped so far". Keep estimates honest: re-derive them from what remains, never from wishful velocity.
