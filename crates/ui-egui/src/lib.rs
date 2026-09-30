//! DrawCraft's egui frontend: an Illustrator-style UI over `drawcraft-engine`.
//!
//! The UI is thin: every action goes through [`DrawcraftApp::run`], which dispatches UI commands
//! (view/window) here and everything else to the engine. The same entry point serves menus,
//! shortcuts, the ⌘K palette and the control channel ([`control`]).
#![forbid(unsafe_code)]

pub mod canvas;
pub mod chrome;
pub mod control;
pub mod dialogs;
pub mod dock;
pub mod icon_data;
pub mod icons;
pub mod io;
pub mod menus;
pub mod palette;
pub mod panels;
pub mod shortcuts;
pub mod state;
pub mod theme;
pub mod toolbar;
pub mod widgets;

use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};

use drawcraft_engine::{Session, ViewInfo};
use serde_json::{Value, json};

pub use control::{ControlRequest, ControlResponse};
pub use state::{UiState, View};

pub type PickSave = Box<dyn FnMut(&str) -> Option<String>>;
pub type ReadFn = Box<dyn Fn(&str) -> Result<Vec<u8>, String>>;
pub type WriteFn = Box<dyn FnMut(&str, &[u8]) -> Result<(), String>>;
pub type Inbox = Arc<Mutex<Vec<(String, Vec<u8>)>>>;
pub type DownloadFn = Box<dyn FnMut(&str, &[u8])>;

/// Platform services injected by the host app (desktop or web).
#[derive(Default)]
pub struct Services {
    /// Show an open dialog; returns a path.
    pub pick_open: Option<Box<dyn FnMut() -> Option<String>>>,
    /// Show a save dialog with a suggested file name; returns a path.
    pub pick_save: Option<PickSave>,
    pub read: Option<ReadFn>,
    pub write: Option<WriteFn>,
    /// Files that arrived asynchronously (web open / drops): (name, bytes).
    pub inbox: Option<Inbox>,
    /// Web: trigger a browser download instead of writing a path.
    pub download: Option<DownloadFn>,
    /// Web: start an async open (bytes arrive via `inbox`).
    pub open_async: Option<Box<dyn FnMut()>>,
}

/// Cached canvas raster.
pub struct CanvasCache {
    pub renderer: drawcraft_render::Renderer,
    pub texture: Option<egui::TextureHandle>,
    pub key: Option<CacheKey>,
    pub last_ms: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CacheKey {
    pub doc: usize,
    pub revision: u64,
    pub zoom: f64,
    pub cx: f64,
    pub cy: f64,
    pub w: u32,
    pub h: u32,
    pub outline: bool,
    pub ppp: f32,
    pub hidden: Vec<u64>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Perf {
    pub frame_ms: f64,
    pub render_ms: f64,
    pub fps: f64,
}

pub struct DrawcraftApp {
    pub session: Session,
    pub ui: UiState,
    pub views: Vec<View>,
    pub services: Services,
    pub canvas: CanvasCache,
    pub perf: Perf,
    /// macOS: draw our own title strip under the traffic lights.
    pub integrated_titlebar: bool,
    control_rx: Option<Receiver<ControlRequest>>,
    pending_screenshots: Vec<(u64, Option<String>, Sender<ControlResponse>)>,
    queued_screenshots: Vec<(u64, f64, u32)>,
    screenshot_token: u64,
    /// Synthetic input events (from the control channel) injected one step per frame.
    pub synthetic: Vec<egui::Event>,
    styled: bool,
    fonts_ready: bool,
    frame: u64,
    last_time: f64,
    /// Canvas rect of the last frame (screen points), for control-channel coordinate mapping.
    pub canvas_rect: Option<egui::Rect>,
    /// Hover position in document coordinates.
    pub hover_doc: Option<drawcraft_geom::Point>,
}

impl DrawcraftApp {
    pub fn new(session: Session, services: Services) -> Self {
        let views = session.documents().iter().map(|_| View::default()).collect();
        Self {
            session,
            ui: UiState::default(),
            views,
            services,
            canvas: CanvasCache { renderer: drawcraft_render::Renderer::new(), texture: None, key: None, last_ms: 0.0 },
            perf: Perf::default(),
            integrated_titlebar: false,
            control_rx: None,
            pending_screenshots: vec![],
            queued_screenshots: vec![],
            screenshot_token: 0,
            synthetic: vec![],
            styled: false,
            fonts_ready: false,
            frame: 0,
            last_time: 0.0,
            canvas_rect: None,
            hover_doc: None,
        }
    }

    pub fn with_control(mut self, rx: Receiver<ControlRequest>) -> Self {
        self.control_rx = Some(rx);
        self
    }

    /// Keep `views` aligned with the session's documents.
    pub fn sync_views(&mut self) {
        let n = self.session.documents().len();
        while self.views.len() < n {
            self.views.push(View::default());
        }
        self.views.truncate(n);
    }

    pub fn view(&self) -> Option<&View> {
        self.session.active_index().and_then(|i| self.views.get(i))
    }
    pub fn view_mut(&mut self) -> Option<&mut View> {
        self.sync_views();
        let i = self.session.active_index()?;
        self.views.get_mut(i)
    }

    pub fn view_info(&self) -> ViewInfo {
        ViewInfo {
            zoom: self.view().map(|v| v.zoom).unwrap_or(1.0),
            outline: self.ui.view.outline,
            smart_guides: self.ui.view.smart_guides,
            snap_to_grid: self.ui.view.snap_to_grid,
            show_bbox: self.ui.view.bounding_box,
        }
    }

    /// Run a UI or engine command by id. The single entry point for every frontend path.
    pub fn run(&mut self, id: &str, params: Value) -> Result<Value, String> {
        if let Some(r) = menus::run_ui_command(self, id, &params) {
            return r;
        }
        let r = self.session.execute(id, &params).map_err(|e| e.to_string());
        self.sync_views();
        match &r {
            Err(e) => self.ui.status = e.clone(),
            Ok(_) => {
                if id == "file.new" {
                    self.ui.status.clear();
                }
            }
        }
        r
    }

    /// Select a tool (also used by the toolbar and shortcuts).
    pub fn select_tool(&mut self, id: &str) {
        let v = self.view_info();
        if let Err(e) = self.session.select_tool(id, v) {
            self.ui.status = e.to_string();
        }
        if let Some(g) = drawcraft_tools::catalog::group_of(id)
            && let Some(slot) = self.ui.group_tool.get_mut(g)
        {
            *slot = id.to_string();
        }
        self.ui.flyout = None;
    }

    fn drain_control(&mut self, ctx: &egui::Context) {
        let Some(rx) = self.control_rx.take() else { return };
        while let Ok(req) = rx.try_recv() {
            let reply = req.reply.clone();
            match control::handle(self, ctx, &req) {
                control::Outcome::Done(v) => {
                    let _ = reply.send(v);
                }
                control::Outcome::Screenshot { path } => {
                    self.screenshot_token += 1;
                    let token = self.screenshot_token;
                    let settle = ctx.global_style().animation_time as f64 * 2000.0 + 80.0;
                    self.queued_screenshots.push((token, now_ms() + settle, 0));
                    self.pending_screenshots.push((token, path, reply));
                }
            }
        }
        self.control_rx = Some(rx);
    }

    fn issue_screenshots(&mut self, ctx: &egui::Context) {
        let now = now_ms();
        self.queued_screenshots.retain_mut(|(token, at, frames)| {
            *frames += 1;
            if now >= *at && *frames >= 3 {
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::new(*token)));
                false
            } else {
                true
            }
        });
        if !self.queued_screenshots.is_empty() || !self.pending_screenshots.is_empty() {
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
        }
    }

    fn collect_screenshots(&mut self, ctx: &egui::Context) {
        if self.pending_screenshots.is_empty() {
            return;
        }
        let events: Vec<_> = ctx.input(|i| {
            i.raw
                .events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::Screenshot { user_data, image, .. } => {
                        let token = user_data.data.as_ref().and_then(|d| d.downcast_ref::<u64>()).copied()?;
                        Some((token, image.clone()))
                    }
                    _ => None,
                })
                .collect()
        });
        for (token, image) in events {
            if let Some(i) = self.pending_screenshots.iter().position(|(t, _, _)| *t == token) {
                let (_, path, reply) = self.pending_screenshots.remove(i);
                let _ = reply.send(control::save_screenshot(self, &image, path.as_deref()));
            }
        }
    }

    /// Show a transient status message.
    pub fn status(&mut self, s: impl Into<String>) {
        self.ui.status = s.into();
    }

    fn drain_inbox(&mut self) {
        let arrived: Vec<(String, Vec<u8>)> = self.services.inbox.as_ref().map(|q| std::mem::take(&mut *q.lock().unwrap_or_else(|e| e.into_inner()))).unwrap_or_default();
        for (name, bytes) in arrived {
            if let Err(e) = io::open_bytes(self, &name, &bytes, None) {
                self.status(format!("Couldn't open {name}: {e}"));
            }
        }
    }
}

pub fn now_ms() -> f64 {
    #[cfg(not(target_arch = "wasm32"))]
    {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64() * 1000.0).unwrap_or(0.0)
    }
    #[cfg(target_arch = "wasm32")]
    {
        0.0
    }
}

/// eframe isn't a dependency of this crate (the host owns the event loop); these entry points are
/// called from the host's `eframe::App` impl.
impl DrawcraftApp {
    /// Per-frame logic before layout (control channel, shortcuts, inbox).
    pub fn logic(&mut self, ctx: &egui::Context) {
        if !self.styled {
            theme::install_fonts(ctx);
            theme::apply(ctx, self.ui.brightness);
            egui_extras::install_image_loaders(ctx);
            self.styled = true;
        } else {
            self.fonts_ready = true;
        }
        self.frame += 1;
        let now = ctx.input(|i| i.time);
        let dt = now - self.last_time;
        if dt > 0.0 {
            self.perf.fps = self.perf.fps * 0.9 + (1.0 / dt).min(240.0) * 0.1;
        }
        self.last_time = now;
        self.sync_views();
        self.drain_control(ctx);
        self.collect_screenshots(ctx);
        self.issue_screenshots(ctx);
        self.drain_inbox();
        if self.fonts_ready {
            shortcuts::handle(self, ctx);
        }
        // Native only: the web host reads dropped files asynchronously and feeds `Services::inbox`.
        #[cfg(not(target_arch = "wasm32"))]
        for f in ctx.input(|i| i.raw.dropped_files.clone()) {
            let path = f.path().to_path_buf();
            let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "dropped".into());
            match f.bytes() {
                Ok(b) => {
                    let p = Some(path.to_string_lossy().to_string()).filter(|s| !s.is_empty());
                    if let Err(e) = io::open_bytes(self, &name, &b, p) {
                        self.status(format!("Couldn't open {name}: {e}"));
                    }
                }
                Err(e) => self.status(format!("Couldn't read {name}: {e}")),
            }
        }
    }

    /// Inject synthetic events (one press/release step per frame).
    pub fn raw_input_hook(&mut self, raw: &mut egui::RawInput) {
        if !self.synthetic.is_empty() {
            let n = self.synthetic.iter().position(|e| matches!(e, egui::Event::PointerButton { pressed: false, .. } | egui::Event::Key { pressed: false, .. })).map_or(self.synthetic.len(), |i| i + 1);
            raw.events.extend(self.synthetic.drain(..n));
        }
    }

    /// Lay out the whole window.
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        if !self.fonts_ready {
            ctx.request_repaint();
            return;
        }
        let t0 = now_ms();
        let t = theme::Tokens::get(&ctx);
        if self.ui.screen_mode < 2 {
            chrome::app_bar(self, ui);
            if self.ui.control_bar {
                chrome::control_bar(self, ui);
            }
        }
        if self.ui.status_bar && self.ui.screen_mode < 3 {
            chrome::status_bar(self, ui);
        }
        if self.ui.toolbar && self.ui.screen_mode < 3 {
            toolbar::show(self, ui);
        }
        if self.ui.dock && self.ui.screen_mode < 3 {
            dock::show(self, ui);
        }
        egui::CentralPanel::default().frame(egui::Frame::NONE.fill(t.pasteboard)).show(ui, |ui| {
            if self.ui.screen_mode < 3 {
                chrome::doc_tabs(self, ui);
            }
            canvas::show(self, ui);
        });
        dock::floating_panel(self, &ctx);
        dialogs::show(self, &ctx);
        palette::show(self, &ctx);
        self.perf.frame_ms = now_ms() - t0;
        let _ = json!(null);
    }
}

