//! Help › Check for Updates… and the automatic check, over `craft_update::Updater`.
//!
//! The desktop app hands over the updater ([`VectorcraftApp::set_updater`], with its GitHub
//! source), so this crate has no network code. Without one (the web build, tests, the CLI) the
//! command opens the releases page.
//!
//! - The command opens the Updates dialog (kind [`KIND`]) and starts a check. The dialog follows
//!   the updater's state: checking, up to date, available (Download and Install, or why not and
//!   Open Release Page; Skip This Version; Later), downloading (progress, Cancel), ready (Restart
//!   Now, Install When I Quit) and failed. Agents read its fields (`state`, `version`, `notes`,
//!   `install`, `reason`, `received`, `total`, `error`) and press a button by setting `action`
//!   (`download`, `skip`, `later`, `cancel`, `restart`, `installOnQuit`, `openPage`, `close`)
//!   and confirming.
//! - The automatic check (Preferences › General › Check for updates automatically, kept in
//!   `UiState::updates`) never runs before the first frame: once the app is up, a due check starts
//!   [`craft_update::AutoCheck::START_DELAY_SECS`] after the first frame, on the updater's thread.
//!   A newer version that wasn't skipped shows a quiet notice in the corner of the window
//!   ([`notice`]); nothing else of a background check shows.
//! - The verified download installs when the app quits ([`VectorcraftApp::apply_update`], after
//!   the unsaved-changes questions); Restart Now quits through the normal close path and reopens.

use serde_json::{Value, json};

use crate::state::Dialog;
use crate::theme::{self, Tokens};
use crate::{VectorcraftApp, widgets};

/// Where every VectorCraft release is listed.
pub const RELEASES_PAGE: &str = "https://github.com/treeforjs/vectorcraft/releases";

/// The Updates dialog's kind.
pub const KIND: &str = "updates";

/// The command (Help › Check for Updates…).
pub const COMMAND: &str = "help.checkForUpdates";

/// The updater and what the frame loop keeps about it.
#[derive(Default)]
pub struct Updates {
    #[cfg(not(target_arch = "wasm32"))]
    updater: Option<craft_update::Updater>,
    /// When the first frame ran (the automatic check waits from then).
    #[cfg(not(target_arch = "wasm32"))]
    started: Option<std::time::Instant>,
    /// The running check was started by the app, not the person: its outcome stays quiet.
    #[cfg(not(target_arch = "wasm32"))]
    background: bool,
    /// The quiet "a new version is available" notice shows.
    pub notice: bool,
    /// Restart Now: reopen the app after installing on quit.
    pub restart: bool,
}

impl VectorcraftApp {
    /// Use `updater` for Help › Check for Updates… and the automatic check (desktop app).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn set_updater(&mut self, updater: craft_update::Updater) {
        self.updates.updater = Some(updater);
    }

    /// The updater, if the host installed one.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn updater(&self) -> Option<&craft_update::Updater> {
        self.updates.updater.as_ref()
    }

    /// As the app quits (after its unsaved documents were handled): install a verified download,
    /// reopening the app if the person chose Restart Now. A failure is logged; the app still quits.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn apply_update(&mut self) {
        let restart = self.updates.restart;
        let Some(u) = self.updates.updater.as_mut() else { return };
        match u.apply(restart) {
            Ok(Some(applied)) => log::info!("update: {applied:?} (restart: {restart})"),
            Ok(None) => {}
            Err(e) => log::error!("update not installed: {e}"),
        }
    }
}

/// Help › Check for Updates…: open the Updates dialog and look for a newer version (or, without an
/// updater, open the releases page).
pub(crate) fn command(app: &mut VectorcraftApp) -> Result<Value, String> {
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(u) = app.updates.updater.as_mut() {
        if matches!(u.state(), craft_update::State::Idle | craft_update::State::UpToDate | craft_update::State::Failed(_)) {
            u.check();
        }
        app.updates.background = false;
        app.updates.notice = false;
        app.ui.dialog = Some(Dialog::new(KIND, json!({})));
        sync_fields(app);
        return Ok(json!({ "pending": KIND }));
    }
    app.open_url(RELEASES_PAGE);
    Ok(json!({ "url": RELEASES_PAGE }))
}

/// Each frame, before layout: pick up the updater's news, start a due automatic check once the
/// app has been up for a while, and keep the dialog's fields current.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn frame(app: &mut VectorcraftApp, ctx: &egui::Context) {
    use craft_update::{AutoCheck, State};
    let dialog_open = app.ui.dialog.as_ref().is_some_and(|d| d.kind == KIND);
    let (ui, up) = (&mut app.ui, &mut app.updates);
    let Some(u) = up.updater.as_mut() else { return };
    let started = *up.started.get_or_insert_with(std::time::Instant::now);
    let checking = matches!(u.state(), State::Checking);
    if u.poll() {
        if checking && !matches!(u.state(), State::Checking) {
            ui.updates.last_checked = Some(AutoCheck::now());
            // A background check shows only a newer version that wasn't skipped, and quietly.
            if std::mem::take(&mut up.background) && !dialog_open {
                match u.state() {
                    State::Available(offer) if !ui.updates.is_skipped(&offer.release.version) => up.notice = true,
                    _ => u.dismiss(),
                }
            }
        }
        // A cancelled download is no failure to report.
        if matches!(u.state(), State::Failed(craft_update::Error::Cancelled)) {
            u.dismiss();
        }
    }
    let idle = matches!(u.state(), State::Idle | State::UpToDate | State::Failed(_));
    if idle && u.prepared().is_none() && ui.updates.due(AutoCheck::now()) {
        let delay = std::time::Duration::from_secs(AutoCheck::START_DELAY_SECS);
        match delay.checked_sub(started.elapsed()) {
            // Wake an idle window when the delay is over.
            Some(left) if !left.is_zero() => ctx.request_repaint_after(left),
            _ => {
                u.check();
                up.background = true;
            }
        }
    }
    if dialog_open {
        sync_fields(app);
    }
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn frame(_app: &mut VectorcraftApp, _ctx: &egui::Context) {}

/// The updater's state as the dialog's fields (what agents read).
fn sync_fields(app: &mut VectorcraftApp) {
    let fields = state_json(app);
    if let Some(d) = app.ui.dialog.as_mut().filter(|d| d.kind == KIND)
        && let Some(f) = fields.as_object()
    {
        for (k, v) in f {
            d.fields.insert(k.clone(), v.clone());
        }
    }
}

/// `{state, current, version?, notes?, install?, reason?, page?, received?, total?, error?,
/// notice}`: also in `ui.inspect` as `updates`.
pub fn state_json(app: &VectorcraftApp) -> Value {
    let mut v = json!({ "state": "unavailable", "current": env!("CARGO_PKG_VERSION"), "notice": app.updates.notice });
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(u) = &app.updates.updater {
        use craft_update::State;
        let offer_json = |o: &craft_update::Offer| {
            json!({
                "version": o.release.version.to_string(),
                "notes": short_notes(&o.release.notes),
                "page": o.release.page,
                "install": o.install.is_ok(),
                "reason": o.install.as_ref().err(),
            })
        };
        let (state, extra) = match u.state() {
            State::Idle => ("idle", json!({})),
            State::Checking => ("checking", json!({})),
            State::UpToDate => ("upToDate", json!({})),
            State::Available(o) => ("available", offer_json(o)),
            State::Downloading { offer, received, total } => {
                let mut x = offer_json(offer);
                x["received"] = json!(received);
                x["total"] = json!(total);
                ("downloading", x)
            }
            State::Ready(o) => ("ready", offer_json(o)),
            State::Failed(e) => ("failed", json!({ "error": e.to_string() })),
        };
        v["state"] = json!(state);
        if let (Some(o), Some(x)) = (v.as_object_mut(), extra.as_object()) {
            o.extend(x.iter().map(|(k, val)| (k.clone(), val.clone())));
        }
    }
    v
}

/// The start of the release notes: a dozen lines, at most 800 characters.
fn short_notes(notes: &str) -> String {
    let mut out: String = notes.lines().map(str::trim_end).filter(|l| !l.is_empty()).take(12).collect::<Vec<_>>().join("\n");
    if let Some((cut, _)) = out.char_indices().nth(800) {
        out.truncate(cut);
        out.push('…');
    }
    out
}

/// A button of the dialog: what it sets `action` to.
fn press(app: &mut VectorcraftApp, action: &str) {
    if let Some(d) = app.ui.dialog.as_mut().filter(|d| d.kind == KIND) {
        d.fields.insert("action".into(), json!(action));
    }
    if let Err(e) = crate::dialogs::confirm(app) {
        app.status(e);
    }
}

/// The dialog's buttons (`action`, then confirm).
pub(crate) fn confirm(app: &mut VectorcraftApp, d: &Dialog) -> Result<Value, String> {
    let action = d.str("action");
    if let Some(open) = app.ui.dialog.as_mut().filter(|d| d.kind == KIND) {
        open.fields.remove("action");
    }
    let page = d.fields.get("page").and_then(Value::as_str).unwrap_or(RELEASES_PAGE).to_string();
    let close = |app: &mut VectorcraftApp| {
        app.ui.dialog = None;
        app.updates.notice = false;
    };
    match action.as_str() {
        "openPage" => {
            close(app);
            app.open_url(&page);
        }
        "later" | "installOnQuit" => close(app),
        "restart" => {
            close(app);
            app.updates.restart = true;
            // The usual way out: modified documents are asked about first.
            return app.run("app.quit", json!({}));
        }
        "" | "close" => {
            close(app);
            #[cfg(not(target_arch = "wasm32"))]
            if let Some(u) = app.updates.updater.as_mut()
                && matches!(u.state(), craft_update::State::UpToDate | craft_update::State::Failed(_))
            {
                u.dismiss();
            }
        }
        #[cfg(not(target_arch = "wasm32"))]
        "download" => {
            let u = app.updates.updater.as_mut().ok_or("no updater")?;
            if !matches!(u.state(), craft_update::State::Available(o) if o.install.is_ok()) {
                return Err("no update to download".into());
            }
            u.download();
            sync_fields(app);
        }
        #[cfg(not(target_arch = "wasm32"))]
        "cancel" => {
            if let Some(u) = app.updates.updater.as_mut() {
                u.cancel();
            }
            close(app);
        }
        #[cfg(not(target_arch = "wasm32"))]
        "skip" => {
            let u = app.updates.updater.as_mut().ok_or("no updater")?;
            let craft_update::State::Available(o) = u.state() else { return Err("no update to skip".into()) };
            app.ui.updates.skipped = Some(o.release.version.to_string());
            u.dismiss();
            close(app);
        }
        other => return Err(format!("unknown action `{other}`")),
    }
    Ok(Value::Null)
}

/// The Updates dialog.
pub(crate) fn show(app: &mut VectorcraftApp, ctx: &egui::Context) {
    let t = Tokens::get(ctx);
    let fields = state_json(app);
    let s = |k: &str| fields.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    let n = |k: &str| fields.get(k).and_then(Value::as_u64).unwrap_or(0);
    let (state, version, current) = (s("state"), s("version"), s("current"));
    let mut pressed: Option<&'static str> = None;
    let heading = tl!("Software Update");
    crate::dialogs::modal::show(ctx, heading, egui::Id::new("dialog-updates"), -40.0, 22, |ui| {
        ui.set_width(440.0);
        crate::dialogs::modal::heading(ui, heading);
        ui.add_space(12.0);
        let strong = |text: String| egui::RichText::new(text).font(theme::semibold(13.5)).color(t.text_strong);
        let dim = |text: String| egui::RichText::new(text).color(t.text_dim);
        let fmt = |s: &str, args: &[(&str, &str)]| crate::i18n::fmt(s, args);
        match state.as_str() {
            "upToDate" => {
                ui.label(strong(fmt(tl!("VectorCraft {version} is up to date."), &[("version", &current)])));
            }
            "available" | "downloading" | "ready" => {
                let title = match state.as_str() {
                    "downloading" => fmt(tl!("Downloading VectorCraft {version}…"), &[("version", &version)]),
                    "ready" => fmt(tl!("VectorCraft {version} is ready to install."), &[("version", &version)]),
                    _ => fmt(tl!("VectorCraft {version} is available."), &[("version", &version)]),
                };
                ui.label(strong(title));
                ui.label(dim(fmt(tl!("You have version {version}."), &[("version", &current)])));
                let notes = s("notes");
                if state == "available" && !notes.is_empty() {
                    ui.add_space(8.0);
                    egui::Frame::NONE
                        .fill(t.panel_darker)
                        .corner_radius(egui::CornerRadius::same(t.radius))
                        .inner_margin(egui::Margin::same(8))
                        .show(ui, |ui| {
                            egui::ScrollArea::vertical().id_salt("update-notes").max_height(150.0).show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.label(egui::RichText::new(notes).size(11.5).color(t.text));
                            });
                        });
                }
                if state == "available" && fields.get("install") != Some(&json!(true)) {
                    ui.add_space(8.0);
                    ui.label(dim(s("reason")));
                }
                if state == "downloading" {
                    ui.add_space(8.0);
                    let (received, total) = (n("received"), n("total"));
                    let frac = if total > 0 { (received as f64 / total as f64).clamp(0.0, 1.0) as f32 } else { 0.0 };
                    let mb = |b: u64| format!("{:.1}", b as f64 / 1_000_000.0);
                    let text = fmt(tl!("{received} of {total} MB"), &[("received", &mb(received)), ("total", &mb(total))]);
                    ui.add(egui::ProgressBar::new(frac).fill(t.accent).text(text).animate(total == 0));
                }
                if state == "ready" {
                    ui.add_space(4.0);
                    ui.label(dim(tl!("It installs when you quit VectorCraft, or right away if you restart now.").to_string()));
                }
            }
            "failed" => {
                ui.label(strong(tl!("Couldn't update VectorCraft.").to_string()));
                ui.label(dim(s("error")));
            }
            // Idle (the check is about to start) or checking.
            _ => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(tl!("Checking for updates…"));
                });
            }
        }
        ui.add_space(16.0);
        let row = egui::vec2(ui.min_rect().width(), ui.spacing().interact_size.y);
        ui.allocate_ui_with_layout(row, egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let mut button = |ui: &mut egui::Ui, primary: bool, label: &str, action: &'static str| {
                let r = if primary { widgets::primary_button(ui, label) } else { widgets::secondary_button(ui, label) };
                if r.clicked() {
                    pressed = Some(action);
                }
                ui.add_space(8.0);
            };
            match state.as_str() {
                "available" => {
                    if fields.get("install") == Some(&json!(true)) {
                        button(ui, true, tl!("Download and Install"), "download");
                    } else {
                        button(ui, true, tl!("Open Release Page"), "openPage");
                    }
                    button(ui, false, tl!("Later"), "later");
                    button(ui, false, tl!("Skip This Version"), "skip");
                }
                "downloading" => button(ui, false, tl!("Cancel"), "cancel"),
                "ready" => {
                    button(ui, true, tl!("Restart Now"), "restart");
                    button(ui, false, tl!("Install When I Quit"), "installOnQuit");
                }
                "failed" => {
                    button(ui, true, tl!("Close"), "close");
                    button(ui, false, tl!("Open Release Page"), "openPage");
                }
                _ => button(ui, true, tl!("Close"), "close"),
            }
        });
    });
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) && pressed.is_none() {
        pressed = Some(if state == "downloading" { "later" } else { "close" });
    }
    if let Some(action) = pressed {
        press(app, action);
    }
}

/// The quiet notice a background check leaves when it finds a newer version: a small card in the
/// window's lower right corner, over nothing the person is working on, with View (opens the
/// Updates dialog) and a close button. Never while a dialog is open.
pub(crate) fn notice(app: &mut VectorcraftApp, ctx: &egui::Context) {
    if !app.updates.notice || app.ui.dialog.is_some() {
        return;
    }
    let fields = state_json(app);
    let Some(version) = fields.get("version").and_then(Value::as_str).map(str::to_string) else {
        app.updates.notice = false;
        return;
    };
    let t = Tokens::get(ctx);
    let (mut view, mut dismiss) = (false, false);
    egui::Area::new(egui::Id::new("update-notice")).order(egui::Order::Middle).anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-16.0, -44.0)).show(
        ctx,
        |ui| {
            egui::Frame::NONE
                .fill(t.panel)
                .stroke(egui::Stroke::new(1.0, t.border))
                .corner_radius(egui::CornerRadius::same(t.radius.saturating_mul(2)))
                .inner_margin(egui::Margin::symmetric(12, 10))
                .shadow(ctx.global_style().visuals.popup_shadow)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        crate::icons::icon(ui, "info", 16.0, t.accent);
                        ui.label(
                            egui::RichText::new(crate::i18n::fmt(tl!("VectorCraft {version} is available."), &[("version", &version)])).color(t.text),
                        );
                        ui.add_space(8.0);
                        view = widgets::primary_button(ui, "View").clicked();
                        dismiss = widgets::icon_button(ui, "x", "Close", false, 20.0).clicked();
                    });
                });
        },
    );
    if dismiss {
        app.updates.notice = false;
    }
    if view && let Err(e) = app.run(COMMAND, json!({})) {
        app.status(e);
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use craft_update::{AppInfo, Arch, Error, InstallKind, Source, State, Updater, Version};

    use super::*;
    use crate::Services;
    use vectorcraft_engine::Session;

    /// A fake GitHub: one release, `latest`, with an AppImage, the checksum list and its signature.
    struct Fake {
        latest: String,
        asked: Mutex<u32>,
    }

    const REPO: &str = "treeforjs/vectorcraft";

    fn info() -> AppInfo {
        AppInfo {
            id: "vectorcraft".into(),
            display_name: "VectorCraft".into(),
            repo: REPO.into(),
            current: "0.8.0".into(),
            // Any 32 bytes: nothing is downloaded in these tests.
            public_key: Some(format!("{}=", "A".repeat(43))),
            prereleases: false,
        }
    }

    impl Source for Fake {
        fn get(&self, url: &str, _limit: u64) -> Result<Vec<u8>, Error> {
            *self.asked.lock().unwrap_or_else(std::sync::PoisonError::into_inner) += 1;
            assert!(url.starts_with(&format!("https://api.github.com/repos/{REPO}/releases")), "{url}");
            let tag = format!("v{}", self.latest);
            let dl = |name: &str| {
                format!(r#"{{"name":"{name}","size":10,"browser_download_url":"https://github.com/{REPO}/releases/download/{tag}/{name}"}}"#)
            };
            let version = Version::parse(&self.latest).ok_or_else(|| Error::Invalid("bad test version".into()))?;
            let appimage = InstallKind::AppImage("/tmp/VectorCraft.AppImage".into())
                .asset_name(&info(), &version, Arch::current())
                .unwrap_or_else(|| "no-appimage-for-this-processor".into());
            let assets = [dl(&appimage), dl("SHA256SUMS.txt"), dl("SHA256SUMS.txt.sig")].join(",");
            Ok(format!(
                r#"[{{"tag_name":"{tag}","html_url":"https://github.com/{REPO}/releases/tag/{tag}","body":"Faster Pathfinder.\n\nNew brushes.","assets":[{assets}]}}]"#
            )
            .into_bytes())
        }

        fn download(&self, _url: &str, _limit: u64, _sink: &mut dyn FnMut(&[u8]) -> Result<(), Error>) -> Result<(), Error> {
            Err(Error::Network("offline".into()))
        }
    }

    fn app_with(latest: &str) -> (VectorcraftApp, Arc<Fake>) {
        let fake = Arc::new(Fake { latest: latest.into(), asked: Mutex::new(0) });
        let mut app = VectorcraftApp::new(Session::new(), Services::default());
        let updater = Updater::new(info(), fake.clone()).with_install_kind(InstallKind::AppImage("/tmp/VectorCraft.AppImage".into()));
        app.set_updater(updater);
        (app, fake)
    }

    /// Frames until the updater is done checking.
    fn settle(app: &mut VectorcraftApp, ctx: &egui::Context) {
        let until = Instant::now() + Duration::from_secs(10);
        loop {
            frame(app, ctx);
            if !app.updater().is_some_and(Updater::busy) || Instant::now() > until {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        frame(app, ctx);
    }

    /// Pretend the app has been up longer than the automatic check's delay.
    fn up_for_a_while(app: &mut VectorcraftApp) {
        app.updates.started = Instant::now().checked_sub(Duration::from_secs(craft_update::AutoCheck::START_DELAY_SECS + 1));
    }

    fn texts(out: &egui::FullOutput) -> Vec<String> {
        fn walk(s: &egui::Shape, out: &mut Vec<String>) {
            match s {
                egui::Shape::Text(t) => out.push(t.galley.text().to_string()),
                egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
                _ => {}
            }
        }
        let mut v = vec![];
        out.shapes.iter().for_each(|c| walk(&c.shape, &mut v));
        v
    }

    /// Two frames of the whole window's dialogs and notice (a window measures itself first).
    fn draw(app: &mut VectorcraftApp, ctx: &egui::Context) -> Vec<String> {
        let mut shown = vec![];
        for _ in 0..2 {
            let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
                crate::dialogs::show(app, ui.ctx());
                notice(app, ui.ctx());
            });
            out.textures_delta.clear();
            shown = texts(&out);
        }
        shown
    }

    fn ctx() -> egui::Context {
        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx);
        ctx
    }

    #[test]
    fn the_dialog_shows_the_available_version_and_a_download_button() {
        let (mut app, _) = app_with("99.0.0");
        let ctx = ctx();
        app.run(COMMAND, json!({})).unwrap();
        assert_eq!(app.ui.dialog.as_ref().unwrap().kind, KIND);
        settle(&mut app, &ctx);
        let d = app.ui.dialog.as_ref().unwrap();
        assert_eq!(d.str("state"), "available");
        assert_eq!(d.str("version"), "99.0.0");
        assert_eq!(d.fields["install"], json!(true));
        let shown = draw(&mut app, &ctx);
        assert!(shown.iter().any(|t| t == "VectorCraft 99.0.0 is available."), "{shown:?}");
        assert!(shown.iter().any(|t| t.contains("Faster Pathfinder.")), "{shown:?}");
        for b in ["Download and Install", "Skip This Version", "Later"] {
            assert!(shown.iter().any(|t| t == b), "{b}: {shown:?}");
        }
        // The check is recorded for the automatic one.
        assert!(app.ui.updates.last_checked.is_some());
        // Download: the fake is offline, so the dialog ends up reporting it, never panicking.
        app.ui.dialog.as_mut().unwrap().fields.insert("action".into(), json!("download"));
        crate::dialogs::confirm(&mut app).unwrap();
        settle(&mut app, &ctx);
        assert_eq!(app.ui.dialog.as_ref().unwrap().str("state"), "failed");
        let shown = draw(&mut app, &ctx);
        assert!(shown.iter().any(|t| t == "Open Release Page"), "{shown:?}");
    }

    #[test]
    fn without_an_updater_the_command_opens_the_releases_page() {
        let opened = Arc::new(Mutex::new(vec![]));
        let o = opened.clone();
        let services = Services { open_url: Some(Box::new(move |u: &str| o.lock().unwrap().push(u.to_string()))), ..Default::default() };
        let mut app = VectorcraftApp::new(Session::new(), services);
        assert_eq!(app.run(COMMAND, json!({})).unwrap()["url"], RELEASES_PAGE);
        assert_eq!(*opened.lock().unwrap(), [RELEASES_PAGE]);
        assert!(app.ui.dialog.is_none());
    }

    #[test]
    fn no_automatic_check_before_the_start_delay() {
        let (mut app, fake) = app_with("99.0.0");
        let ctx = ctx();
        for _ in 0..5 {
            frame(&mut app, &ctx);
        }
        assert!(matches!(app.updater().unwrap().state(), State::Idle));
        assert_eq!(*fake.asked.lock().unwrap(), 0);
    }

    #[test]
    fn a_background_check_shows_a_notice_not_a_modal() {
        let (mut app, fake) = app_with("99.0.0");
        let ctx = ctx();
        frame(&mut app, &ctx);
        up_for_a_while(&mut app);
        settle(&mut app, &ctx);
        assert_eq!(*fake.asked.lock().unwrap(), 1);
        assert!(app.updates.notice);
        assert!(app.ui.dialog.is_none(), "no dialog over the person's work");
        let shown = draw(&mut app, &ctx);
        assert!(shown.iter().any(|t| t == "VectorCraft 99.0.0 is available."), "{shown:?}");
        assert!(shown.iter().any(|t| t == "View"), "{shown:?}");
        assert!(ctx.memory(|m| m.area_rect(crate::dialogs::modal::backdrop().id).is_none()), "no modal backdrop");
        // Checked once: not again the same day.
        settle(&mut app, &ctx);
        assert_eq!(*fake.asked.lock().unwrap(), 1);
        // View opens the dialog on the offer it found.
        app.run(COMMAND, json!({})).unwrap();
        frame(&mut app, &ctx);
        assert!(!app.updates.notice);
        assert_eq!(app.ui.dialog.as_ref().unwrap().str("state"), "available");
        assert_eq!(*fake.asked.lock().unwrap(), 1, "no second check");
    }

    #[test]
    fn an_up_to_date_background_check_shows_nothing() {
        let (mut app, fake) = app_with("0.8.0");
        let ctx = ctx();
        frame(&mut app, &ctx);
        up_for_a_while(&mut app);
        settle(&mut app, &ctx);
        assert_eq!(*fake.asked.lock().unwrap(), 1);
        assert!(!app.updates.notice);
        assert!(app.ui.dialog.is_none());
        assert!(matches!(app.updater().unwrap().state(), State::Idle));
        assert!(app.ui.updates.last_checked.is_some());
        let shown = draw(&mut app, &ctx);
        assert!(!shown.iter().any(|t| t.contains("VectorCraft")), "{shown:?}");
    }

    #[test]
    fn a_skipped_version_is_not_offered_again() {
        let (mut app, _) = app_with("99.0.0");
        let ctx = ctx();
        app.run(COMMAND, json!({})).unwrap();
        settle(&mut app, &ctx);
        app.ui.dialog.as_mut().unwrap().fields.insert("action".into(), json!("skip"));
        crate::dialogs::confirm(&mut app).unwrap();
        assert!(app.ui.dialog.is_none());
        assert_eq!(app.ui.updates.skipped.as_deref(), Some("99.0.0"));
        // The next day's background check finds it again and stays quiet.
        app.ui.updates.last_checked = None;
        up_for_a_while(&mut app);
        settle(&mut app, &ctx);
        assert!(!app.updates.notice);
        assert!(app.ui.dialog.is_none());
        assert!(matches!(app.updater().unwrap().state(), State::Idle));
    }

    #[test]
    fn the_automatic_check_preference_round_trips_and_turns_it_off() {
        let (mut app, fake) = app_with("99.0.0");
        crate::prefs_dialog::open(&mut app, Some("General"));
        assert_eq!(app.ui.dialog.as_ref().unwrap().fields["__checkUpdates"], json!(true), "on by default");
        app.ui.dialog.as_mut().unwrap().fields.insert("__checkUpdates".into(), json!(false));
        crate::prefs_dialog::confirm(&mut app).unwrap();
        assert!(!app.ui.updates.enabled);
        app.ui.updates.skipped = Some("98.0.0".into());
        let saved = serde_json::to_vec(&app.ui).unwrap();
        let back = serde_json::from_slice::<crate::UiState>(&saved).unwrap().sanitized();
        assert_eq!(back.updates, app.ui.updates);
        // Older preference files turn it on.
        let old: crate::UiState = serde_json::from_str("{}").unwrap();
        assert!(old.updates.enabled);
        // Off: no background check.
        let ctx = ctx();
        frame(&mut app, &ctx);
        up_for_a_while(&mut app);
        settle(&mut app, &ctx);
        assert_eq!(*fake.asked.lock().unwrap(), 0);
    }

    #[test]
    fn restart_now_quits_through_the_unsaved_changes_question() {
        let (mut app, _) = app_with("99.0.0");
        app.run("file.new", json!({})).unwrap();
        app.run("shape.rectangle", json!({"x": 0, "y": 0, "width": 10, "height": 10})).unwrap();
        app.ui.dialog = Some(Dialog::new(KIND, json!({"action": "restart"})));
        let r = crate::dialogs::confirm(&mut app).unwrap();
        assert_eq!(r["pending"], crate::unsaved::KIND);
        assert!(app.updates.restart);
        // Cancel keeps the app open and forgets the restart.
        crate::dialogs::cancel(&mut app);
        assert!(!app.updates.restart);
        assert_ne!(app.ui.status, "quit");
        // Nothing to install: applying is a no-op.
        app.apply_update();
    }

    #[test]
    fn release_notes_are_cut_short() {
        assert_eq!(short_notes("a\n\n\nb"), "a\nb");
        let long = "é".repeat(2000);
        let s = short_notes(&long);
        assert_eq!(s.chars().count(), 801);
        assert!(s.ends_with('…'));
        assert_eq!(short_notes(&"x\n".repeat(50)).lines().count(), 12);
    }
}
