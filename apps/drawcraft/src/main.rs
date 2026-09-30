//! DrawCraft desktop app.
//!
//! Usage: `drawcraft [--control <port>] [files…]`
//!
//! `--control <port>` (or `DRAWCRAFT_CONTROL_PORT`) starts a localhost JSON-lines control server:
//! `{"id":1,"method":"ui.inspect","params":{}}` → `{"id":1,"ok":true,"result":…}`.
//! See `drawcraft_ui_egui::control` for the methods.

mod control_server;

use drawcraft_engine::Session;
use drawcraft_ui_egui::{DrawcraftApp, Services};

struct App(DrawcraftApp);

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.0.logic(ctx);
        if self.0.ui.status == "quit" {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw: &mut egui::RawInput) {
        self.0.raw_input_hook(raw);
    }
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.0.ui(ui);
    }
}

fn services() -> Services {
    Services {
        pick_open: Some(Box::new(|| rfd::FileDialog::new().add_filter("All supported", &["drawcraft", "svg", "png", "jpg", "jpeg", "gif", "webp"]).pick_file().map(|p| p.to_string_lossy().to_string()))),
        pick_save: Some(Box::new(|name: &str| rfd::FileDialog::new().set_file_name(name).save_file().map(|p| p.to_string_lossy().to_string()))),
        read: Some(Box::new(|p: &str| std::fs::read(p).map_err(|e| e.to_string()))),
        write: Some(Box::new(|p: &str, b: &[u8]| std::fs::write(p, b).map_err(|e| e.to_string()))),
        ..Default::default()
    }
}

fn main() -> eframe::Result {
    let mut control_port: Option<u16> = std::env::var("DRAWCRAFT_CONTROL_PORT").ok().and_then(|p| p.parse().ok());
    let mut files = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--control" => control_port = args.next().and_then(|p| p.parse().ok()),
            "--version" => {
                println!("drawcraft {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            _ => files.push(a),
        }
    }
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("DrawCraft")
            .with_inner_size([1440.0, 900.0])
            .with_min_inner_size([800.0, 500.0])
            .with_drag_and_drop(true)
            .with_fullsize_content_view(true)
            .with_titlebar_shown(false)
            .with_title_shown(false),
        ..Default::default()
    };
    eframe::run_native(
        "DrawCraft",
        options,
        Box::new(move |cc| {
            let mut app = DrawcraftApp::new(Session::new(), services());
            app.integrated_titlebar = cfg!(target_os = "macos");
            if let Some(port) = control_port {
                let rx = control_server::start(port, cc.egui_ctx.clone());
                app = app.with_control(rx);
            }
            for f in files {
                if let Err(e) = drawcraft_ui_egui::io::open_path(&mut app, &f) {
                    eprintln!("drawcraft: {f}: {e}");
                }
            }
            Ok(Box::new(App(app)))
        }),
    )
}
