# Development

## Web build

`apps/drawcraft-web` runs the same `DrawcraftApp` in the browser through eframe's web runner. The renderer is wgpu: WebGPU where the browser has it, WebGL2 otherwise. It is Rust only; the only JavaScript is the glue wasm-bindgen generates.

```sh
brew install trunk                 # or: cargo install trunk --locked
rustup target add wasm32-unknown-unknown
cd apps/drawcraft-web
trunk build --release              # writes ../../dist/web (index.html, .js glue, .wasm)
trunk serve --release              # dev server on http://127.0.0.1:8766
```

Any static file server works for `dist/web`, for example `python3 -m http.server 8766` inside that directory. The release `.wasm` is about 17.5 MB, or 7.1 MB gzipped, so serve it with compression.

URL flag: `?webgl` forces the WebGL2 backend.

How the web shell (`apps/drawcraft-web/src/web.rs`) differs from desktop:

- **Open** sets `Services::open_async`, which shows `rfd::AsyncFileDialog`. The bytes arrive in `Services::inbox`, which the app drains every frame.
- **Save / Save As / Export** go through `Services::download`: a Blob, an object URL and a temporary `<a download>`, all created from Rust. There is no save dialog, so the suggested name becomes the download name.
- **Drag-and-drop:** `WebShell` takes the frame's `dropped_files` before the app sees them, reads each with `DroppedFile::bytes_async` and pushes the bytes into the inbox. (The app's synchronous drop path is compiled out on wasm32.)
- **No control server:** browsers can't listen on TCP. To automate the web build, drive headless Chrome with `--remote-debugging-port`.
- Quick smoke test: `"/Applications/Google Chrome.app/Contents/MacOS/Google Chrome" --headless=new --enable-unsafe-webgpu --screenshot=web.png --window-size=1440,900 --virtual-time-budget=15000 http://127.0.0.1:8766/` (headless Chrome on macOS gets a real WebGPU adapter).
