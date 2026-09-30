DrawCraft
=========

By the artcraft team.

A fast, open-source, cross-platform vector illustration app written in pure Rust — a clean-room
reimplementation of the Adobe Illustrator workflow (tools, panels, menus, shortcuts) that runs on
macOS, Windows, Linux and the web (WASM), and is fully drivable by agents over a control channel
and MCP.

```sh
cargo run --release -p drawcraft                     # desktop app
cargo run --release -p drawcraft -- --control 7979   # + JSON control channel
cargo run --release -p drawcraft-cli -- mcp          # MCP server (stdio)
cargo xtask ci                                       # fmt, clippy, tests, layering, wasm
```

Workspace: `crates/{geom,color,doc,pathops,text,effects,render,svg,pdf,format,tools,engine,ui-egui,mcp,testkit}`,
`apps/{drawcraft,drawcraft-cli,drawcraft-web}`. See `CLAUDE.md` and `docs/`.
