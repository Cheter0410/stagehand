//! The window: one card per app with its state and what can be done
//! (Install, Update, Open). Network and installs run on worker threads that
//! report back through a channel.

use crate::catalog::{self, AppDef, Catalog};
use crate::github::{self, Release};
use crate::system::{self, Installed};
use egui::{Color32, CornerRadius, Frame, Margin, RichText, Stroke, Vec2};
use std::collections::HashMap;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::{Duration, Instant};

const BG: Color32 = Color32::from_rgb(13, 15, 18);
const CARD: Color32 = Color32::from_rgb(23, 26, 32);
const LINE: Color32 = Color32::from_rgb(42, 47, 56);
const TEXT: Color32 = Color32::from_rgb(232, 235, 239);
const MUTED: Color32 = Color32::from_rgb(140, 146, 160);
const WARN: Color32 = Color32::from_rgb(245, 158, 11);
const ERROR: Color32 = Color32::from_rgb(239, 68, 68);
const OK: Color32 = Color32::from_rgb(34, 197, 94);

/// This program as an entry of its own releases (self-update).
const SELF_RELEASES: &str = "Cheter0410/stagehand-releases";

enum Msg {
    Catalog(Catalog),
    Detected(String, Option<Installed>, bool),
    Latest(String, Result<Option<Release>, String>),
    Icon(String, Vec<u8>),
    Progress(String, u64, u64),
    Installing(String),
    Finished(String, Result<(), String>),
    SelfLatest(Option<Release>),
}

#[derive(Clone, Copy, PartialEq)]
enum Job {
    Download(u64, u64),
    Install,
}

struct Card {
    def: AppDef,
    installed: Option<Installed>,
    detected: bool,
    running: bool,
    latest: Option<Result<Option<Release>, String>>,
    job: Option<Job>,
    message: Option<(String, bool)>,
    notes_open: bool,
    icon: Option<egui::TextureHandle>,
}

pub struct Launcher {
    cards: Vec<Card>,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    last_detect: Instant,
    self_update: Option<Release>,
    self_job: Option<Job>,
    self_error: Option<String>,
    icons_wanted: HashMap<String, bool>,
}

impl Launcher {
    pub fn new(ctx: &egui::Context) -> Self {
        style(ctx);
        let (tx, rx) = channel();
        let mut l = Self {
            cards: Vec::new(),
            tx,
            rx,
            last_detect: Instant::now(),
            self_update: None,
            self_job: None,
            self_error: None,
            icons_wanted: HashMap::new(),
        };
        l.set_catalog(Catalog::load(), ctx);
        l.refresh(ctx);
        // The live catalog (new apps without a new Stagehand).
        let (tx, c) = (l.tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            if let Ok(cat) = Catalog::fetch() {
                let _ = tx.send(Msg::Catalog(cat));
                c.request_repaint();
            }
        });
        // A newer Stagehand?
        let (tx, c) = (l.tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            let r = github::latest(SELF_RELEASES)
                .ok()
                .flatten()
                .filter(|r| github::is_newer(&r.version, env!("CARGO_PKG_VERSION")));
            let _ = tx.send(Msg::SelfLatest(r));
            c.request_repaint();
        });
        l
    }

    fn set_catalog(&mut self, cat: Catalog, ctx: &egui::Context) {
        let mut old: HashMap<String, Card> = self
            .cards
            .drain(..)
            .map(|c| (c.def.id.clone(), c))
            .collect();
        for def in cat.apps {
            let card = match old.remove(&def.id) {
                Some(mut c) => {
                    c.def = def;
                    c
                }
                None => {
                    let icon =
                        catalog::built_in_icon(&def.id).and_then(|b| texture(ctx, &def.id, b));
                    if icon.is_none() {
                        if let Some(url) = def.icon_url.clone() {
                            if self.icons_wanted.insert(def.id.clone(), true).is_none() {
                                let (tx, id, c) = (self.tx.clone(), def.id.clone(), ctx.clone());
                                std::thread::spawn(move || {
                                    if let Ok(r) = crate::github::http().get(&url).call() {
                                        let mut v = Vec::new();
                                        if std::io::Read::read_to_end(&mut r.into_reader(), &mut v)
                                            .is_ok()
                                        {
                                            let _ = tx.send(Msg::Icon(id, v));
                                            c.request_repaint();
                                        }
                                    }
                                });
                            }
                        }
                    }
                    Card {
                        def,
                        installed: None,
                        detected: false,
                        running: false,
                        latest: None,
                        job: None,
                        message: None,
                        notes_open: false,
                        icon,
                    }
                }
            };
            self.cards.push(card);
        }
    }

    /// Look for installed copies and the newest releases (all apps).
    fn refresh(&mut self, ctx: &egui::Context) {
        for c in &mut self.cards {
            c.latest = None;
        }
        let defs: Vec<AppDef> = self.cards.iter().map(|c| c.def.clone()).collect();
        self.detect(ctx);
        for def in defs {
            let (tx, c) = (self.tx.clone(), ctx.clone());
            std::thread::spawn(move || {
                let r = github::latest(&def.releases);
                let _ = tx.send(Msg::Latest(def.id, r));
                c.request_repaint();
            });
        }
    }

    /// Which apps are installed and running (cheap, runs every few seconds).
    fn detect(&mut self, ctx: &egui::Context) {
        self.last_detect = Instant::now();
        let defs: Vec<AppDef> = self.cards.iter().map(|c| c.def.clone()).collect();
        let (tx, c) = (self.tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            for def in defs {
                let inst = system::installed(&def);
                let running = inst.as_ref().is_some_and(|i| system::is_running(&def, i));
                let _ = tx.send(Msg::Detected(def.id, inst, running));
            }
            c.request_repaint();
        });
    }

    fn card_mut(&mut self, id: &str) -> Option<&mut Card> {
        self.cards.iter_mut().find(|c| c.def.id == id)
    }

    fn drain(&mut self, ctx: &egui::Context) {
        while let Ok(m) = self.rx.try_recv() {
            match m {
                Msg::Catalog(cat) => {
                    let new: Vec<AppDef> = cat
                        .apps
                        .iter()
                        .filter(|a| !self.cards.iter().any(|c| c.def.id == a.id))
                        .cloned()
                        .collect();
                    self.set_catalog(cat, ctx);
                    if !new.is_empty() {
                        self.refresh(ctx);
                    }
                }
                Msg::Detected(id, inst, running) => {
                    if let Some(c) = self.card_mut(&id) {
                        c.installed = inst;
                        c.running = running;
                        c.detected = true;
                    }
                }
                Msg::Latest(id, r) => {
                    if let Some(c) = self.card_mut(&id) {
                        c.latest = Some(r);
                    }
                }
                Msg::Icon(id, bytes) => {
                    let t = texture(ctx, &id, &bytes);
                    if let Some(c) = self.card_mut(&id) {
                        c.icon = t;
                    }
                }
                Msg::Progress(id, d, t) => {
                    if id == "stagehand" {
                        self.self_job = Some(Job::Download(d, t));
                    } else if let Some(c) = self.card_mut(&id) {
                        c.job = Some(Job::Download(d, t));
                    }
                }
                Msg::Installing(id) => {
                    if id == "stagehand" {
                        self.self_job = Some(Job::Install);
                    } else if let Some(c) = self.card_mut(&id) {
                        c.job = Some(Job::Install);
                    }
                }
                Msg::Finished(id, r) => {
                    if id == "stagehand" {
                        self.self_job = None;
                        self.self_error = r.err();
                        continue;
                    }
                    if let Some(c) = self.card_mut(&id) {
                        c.job = None;
                        c.message = Some(match r {
                            Ok(()) => (format!("{} is installed.", c.def.name), false),
                            Err(e) => (e, true),
                        });
                    }
                    self.detect(ctx);
                }
                Msg::SelfLatest(r) => self.self_update = r,
            }
        }
    }

    fn start_install(&mut self, id: &str, ctx: &egui::Context) {
        let tx = self.tx.clone();
        let Some(c) = self.card_mut(id) else { return };
        let Some(Ok(Some(rel))) = c.latest.clone() else {
            return;
        };
        let Some(asset) = system::pick_asset(&c.def, &rel).cloned() else {
            return;
        };
        c.job = Some(Job::Download(0, asset.size));
        c.message = None;
        let (def, cx) = (c.def.clone(), ctx.clone());
        std::thread::spawn(move || {
            let id = def.id.clone();
            let tx2 = tx.clone();
            let cx2 = cx.clone();
            let progress = move |d, t| {
                let _ = tx2.send(Msg::Progress(id.clone(), d, t));
                cx2.request_repaint();
            };
            let result = github::download(&asset, &crate::paths::download_dir(), &progress)
                .and_then(|file| {
                    let _ = tx.send(Msg::Installing(def.id.clone()));
                    cx.request_repaint();
                    let r = system::install(&def, &file, &rel.version);
                    let _ = std::fs::remove_file(&file);
                    r
                });
            let _ = tx.send(Msg::Finished(def.id.clone(), result));
            cx.request_repaint();
        });
    }

    fn start_self_update(&mut self, ctx: &egui::Context) {
        let Some(rel) = self.self_update.clone() else {
            return;
        };
        let me = self_def();
        let Some(asset) = system::pick_asset(&me, &rel).cloned() else {
            system::open_url(&rel.page);
            return;
        };
        self.self_job = Some(Job::Download(0, asset.size));
        self.self_error = None;
        let (tx, cx) = (self.tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            let tx2 = tx.clone();
            let cx2 = cx.clone();
            let progress = move |d, t| {
                let _ = tx2.send(Msg::Progress("stagehand".into(), d, t));
                cx2.request_repaint();
            };
            let r = github::download(&asset, &crate::paths::download_dir(), &progress).and_then(
                |file| {
                    let _ = tx.send(Msg::Installing("stagehand".into()));
                    cx.request_repaint();
                    self_install(&me, &file, &rel.version)
                },
            );
            match r {
                // The new version takes over: quit (it was started already).
                Ok(()) => std::process::exit(0),
                Err(e) => {
                    let _ = tx.send(Msg::Finished("stagehand".into(), Err(e)));
                    cx.request_repaint();
                }
            }
        });
    }
}

/// Stagehand in its own releases (for self-update).
fn self_def() -> AppDef {
    AppDef {
        id: "stagehand".into(),
        name: "Stagehand".into(),
        tagline: String::new(),
        color: String::new(),
        releases: SELF_RELEASES.into(),
        icon_url: None,
        macos: Some(catalog::MacDef {
            asset_suffix: ".dmg".into(),
            app: "Stagehand.app".into(),
        }),
        windows: Some(catalog::WinDef {
            asset_prefix: "Stagehand-Setup-".into(),
            asset_suffix: ".exe".into(),
            inno_app_id: "{3E5A9C21-7D4B-4F6E-9A1C-5B8D2E0F7A63}".into(),
            exe: "stagehand.exe".into(),
        }),
        linux: Some(catalog::LinuxDef {
            asset_suffix: "-linux-x86_64.tar.gz".into(),
            bin: "stagehand".into(),
        }),
    }
}

/// Install a new Stagehand over this one and start it.
fn self_install(me: &AppDef, file: &std::path::Path, version: &str) -> Result<(), String> {
    if cfg!(target_os = "windows") {
        // The installer replaces the running program; it closes this one.
        std::process::Command::new(file)
            .args([
                "/SILENT",
                "/SUPPRESSMSGBOXES",
                "/NORESTART",
                "/CURRENTUSER",
                "/CLOSEAPPLICATIONS",
            ])
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("Could not run the installer: {e}"))
    } else {
        system::install_files(me, file, version)?;
        if let Some(inst) = system::installed(me) {
            // Started after this process has quit.
            let path = inst.path.display().to_string();
            let cmd = if cfg!(target_os = "macos") {
                format!("sleep 1; open -n \"{path}\"")
            } else {
                format!("sleep 1; \"{path}\" &")
            };
            let _ = std::process::Command::new("sh").args(["-c", &cmd]).spawn();
        }
        Ok(())
    }
}

fn texture(ctx: &egui::Context, id: &str, bytes: &[u8]) -> Option<egui::TextureHandle> {
    let img = image::load_from_memory(bytes).ok()?.to_rgba8();
    let size = [img.width() as usize, img.height() as usize];
    let color = egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw());
    Some(ctx.load_texture(format!("app-{id}"), color, egui::TextureOptions::LINEAR))
}

fn style(ctx: &egui::Context) {
    let mut v = egui::Visuals::dark();
    v.panel_fill = BG;
    v.window_fill = CARD;
    v.override_text_color = Some(TEXT);
    v.widgets.inactive.weak_bg_fill = Color32::from_rgb(36, 41, 50);
    v.widgets.hovered.weak_bg_fill = Color32::from_rgb(48, 54, 65);
    v.widgets.active.weak_bg_fill = Color32::from_rgb(58, 65, 78);
    ctx.set_visuals(v);
    let mut s = (*ctx.global_style()).clone();
    s.spacing.button_padding = Vec2::new(14.0, 7.0);
    s.spacing.item_spacing = Vec2::new(10.0, 8.0);
    ctx.set_global_style(s);
}

fn os_name() -> &'static str {
    if cfg!(target_os = "macos") {
        "macOS"
    } else if cfg!(target_os = "windows") {
        "Windows"
    } else {
        "Linux"
    }
}

fn fmt_mb(b: u64) -> String {
    format!("{:.1} MB", b as f64 / 1_048_576.0)
}

/// What a card's buttons offer.
enum Action {
    Install,
    Open,
    Notes,
    Page(String),
}

impl eframe::App for Launcher {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.drain(&ctx);
        // Running / installed state changes outside Stagehand: look again.
        if self.last_detect.elapsed() > Duration::from_secs(4)
            && self.cards.iter().all(|c| c.job.is_none())
        {
            self.detect(&ctx);
        }
        ctx.request_repaint_after(Duration::from_secs(2));

        egui::Frame::new()
            .fill(BG)
            .inner_margin(Margin::symmetric(24, 18))
            .show(ui, |ui| {
                let mut refresh = false;
                ui.horizontal(|ui| {
                    if let Some(t) = texture_logo(&ctx) {
                        ui.add(egui::Image::new(&t).fit_to_exact_size(Vec2::splat(30.0)));
                    }
                    ui.label(RichText::new("Stagehand").size(22.0).strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new(format!("v{}", env!("CARGO_PKG_VERSION")))
                                .small()
                                .color(MUTED),
                        );
                        refresh = ui
                            .button("Check for updates")
                            .on_hover_text("Look for new versions of every app")
                            .clicked();
                    });
                });
                if refresh {
                    self.refresh(&ctx);
                }
                ui.add_space(6.0);

                // A newer Stagehand.
                if let Some(r) = self.self_update.clone() {
                    let mut go = false;
                    Frame::new()
                        .fill(Color32::from_rgb(20, 46, 30))
                        .stroke(Stroke::new(1.0, OK))
                        .corner_radius(CornerRadius::same(8))
                        .inner_margin(Margin::symmetric(14, 10))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(format!("Stagehand {} is available.", r.version)).strong());
                                match self.self_job {
                                    Some(Job::Download(d, t)) => {
                                        ui.add(egui::ProgressBar::new(if t > 0 { d as f32 / t as f32 } else { 0.0 }).desired_width(160.0));
                                    }
                                    Some(Job::Install) => {
                                        ui.spinner();
                                        ui.label("Installing…");
                                    }
                                    None => {
                                        go = ui.button("Update and restart").clicked();
                                    }
                                }
                                if let Some(e) = &self.self_error {
                                    ui.label(RichText::new(e).color(ERROR));
                                }
                            });
                        });
                    if go {
                        self.start_self_update(&ctx);
                    }
                    ui.add_space(6.0);
                }

                let mut actions: Vec<(String, Action)> = Vec::new();
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    for c in &self.cards {
                        if let Some(a) = card(ui, c) {
                            actions.push((c.def.id.clone(), a));
                        }
                        ui.add_space(10.0);
                    }
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(
                            "Apps are installed from their GitHub releases. A running app is never replaced.",
                        )
                        .small()
                        .color(MUTED),
                    );
                });
                for (id, a) in actions {
                    match a {
                        Action::Install => self.start_install(&id, &ctx),
                        Action::Open => {
                            if let Some(c) = self.card_mut(&id) {
                                if let Some(inst) = c.installed.clone() {
                                    c.message = system::launch(&inst).err().map(|e| (e, true));
                                }
                            }
                        }
                        Action::Notes => {
                            if let Some(c) = self.card_mut(&id) {
                                c.notes_open = !c.notes_open;
                            }
                        }
                        Action::Page(url) => system::open_url(&url),
                    }
                }
            });
    }
}

fn texture_logo(ctx: &egui::Context) -> Option<egui::TextureHandle> {
    let id = egui::Id::unique("stagehand-logo");
    if let Some(t) = ctx.data(|d| d.get_temp::<egui::TextureHandle>(id)) {
        return Some(t);
    }
    let t = texture(ctx, "stagehand", include_bytes!("../assets/icon.png"))?;
    ctx.data_mut(|d| d.insert_temp(id, t.clone()));
    Some(t)
}

/// One app's card. Returns what the user asked for.
fn card(ui: &mut egui::Ui, c: &Card) -> Option<Action> {
    let accent = catalog::parse_color(&c.def.color)
        .map(|[r, g, b]| Color32::from_rgb(r, g, b))
        .unwrap_or(OK);
    let supported = system::supported(&c.def);
    let release = match &c.latest {
        Some(Ok(Some(r))) => Some(r),
        _ => None,
    };
    let asset = release.and_then(|r| system::pick_asset(&c.def, r));
    let installed_version = c.installed.as_ref().and_then(|i| i.version.clone());
    let update = match (&installed_version, release) {
        (Some(v), Some(r)) => github::is_newer(&r.version, v),
        (None, Some(_)) => false,
        _ => false,
    };
    let mut action = None;
    Frame::new()
        .fill(CARD)
        .stroke(Stroke::new(1.0, LINE))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::same(14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                match &c.icon {
                    Some(t) => {
                        ui.add(egui::Image::new(t).fit_to_exact_size(Vec2::splat(64.0)));
                    }
                    None => {
                        let (r, _) =
                            ui.allocate_exact_size(Vec2::splat(64.0), egui::Sense::hover());
                        ui.painter().rect_filled(r, 14.0, accent);
                    }
                }
                ui.add_space(6.0);
                // Text column.
                let right_w = 250.0f32.min(ui.available_width() * 0.45);
                ui.allocate_ui_with_layout(
                    Vec2::new(ui.available_width() - right_w, 64.0),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(&c.def.name).size(19.0).strong());
                            if c.running {
                                pill(ui, "RUNNING", accent);
                            }
                        });
                        if !c.def.tagline.is_empty() {
                            ui.add(
                                egui::Label::new(RichText::new(&c.def.tagline).color(MUTED)).wrap(),
                            );
                        }
                        let (status, color) =
                            status_line(c, supported, release, update, &installed_version);
                        ui.label(RichText::new(status).small().color(color));
                    },
                );
                // Buttons.
                ui.with_layout(
                    egui::Layout::right_to_left(egui::Align::Center),
                    |ui| match c.job {
                        Some(Job::Download(d, t)) => {
                            let f = if t > 0 { d as f32 / t as f32 } else { 0.0 };
                            ui.add(egui::ProgressBar::new(f).desired_width(180.0).text(format!(
                                "{} / {}",
                                fmt_mb(d),
                                fmt_mb(t)
                            )));
                        }
                        Some(Job::Install) => {
                            ui.label("Installing…");
                            ui.spinner();
                        }
                        None => {
                            let primary = |text: &str| {
                                egui::Button::new(
                                    RichText::new(text).strong().color(Color32::BLACK),
                                )
                                .fill(accent)
                            };
                            if c.installed.is_some() {
                                if update && asset.is_some() {
                                    let b = ui.add_enabled(
                                        !c.running,
                                        primary(&format!(
                                            "Update to {}",
                                            release.map_or("", |r| r.version.as_str())
                                        )),
                                    );
                                    let b = b.on_disabled_hover_text(format!(
                                        "Quit {} first - it is running.",
                                        c.def.name
                                    ));
                                    if b.clicked() {
                                        action = Some(Action::Install);
                                    }
                                    if ui.button("Open").clicked() {
                                        action = Some(Action::Open);
                                    }
                                } else if ui.add(primary("Open")).clicked() {
                                    action = Some(Action::Open);
                                }
                            } else if supported
                                && asset.is_some()
                                && ui.add(primary("Install")).clicked()
                            {
                                action = Some(Action::Install);
                            }
                            if release.is_some_and(|r| !r.notes.is_empty())
                                && ui.selectable_label(c.notes_open, "What's new").clicked()
                            {
                                action = Some(Action::Notes);
                            }
                        }
                    },
                );
            });
            if let Some((m, err)) = &c.message {
                ui.label(RichText::new(m).color(if *err { ERROR } else { OK }));
            }
            if c.notes_open {
                if let Some(r) = release {
                    ui.separator();
                    ui.label(RichText::new(format!("Version {}", r.version)).strong());
                    ui.add(egui::Label::new(RichText::new(&r.notes).color(MUTED)).wrap());
                    if !r.page.is_empty() && ui.link("Release page").clicked() {
                        action = Some(Action::Page(r.page.clone()));
                    }
                }
            }
        });
    action
}

fn status_line(
    c: &Card,
    supported: bool,
    release: Option<&Release>,
    update: bool,
    installed_version: &Option<String>,
) -> (String, Color32) {
    let latest = match &c.latest {
        None => "checking…".to_string(),
        Some(Ok(Some(r))) => format!("latest {}", r.version),
        Some(Ok(None)) => "no release published yet".to_string(),
        Some(Err(e)) => format!("can't check: {e}"),
    };
    let asset_missing = release.is_some()
        && release
            .and_then(|r| system::pick_asset(&c.def, r))
            .is_none();
    match (&c.installed, supported) {
        _ if !c.detected => ("looking…".into(), MUTED),
        (Some(_), _) if update => (
            format!(
                "Installed {} · update to {} available",
                installed_version.as_deref().unwrap_or("?"),
                release.map_or("", |r| r.version.as_str())
            ),
            WARN,
        ),
        (Some(_), _) => (
            match (installed_version, release) {
                (Some(v), Some(_)) => format!("Installed {v} · up to date"),
                (Some(v), None) => format!("Installed {v} · {latest}"),
                (None, _) => format!("Installed · {latest}"),
            },
            OK,
        ),
        (None, false) => (format!("Not available for {} yet", os_name()), MUTED),
        (None, true) if asset_missing => (
            format!("{latest} · no download for {} in this release", os_name()),
            MUTED,
        ),
        (None, true) => (format!("Not installed · {latest}"), MUTED),
    }
}

fn pill(ui: &mut egui::Ui, text: &str, color: Color32) {
    ui.label(
        RichText::new(format!(" {text} "))
            .small()
            .strong()
            .color(Color32::BLACK)
            .background_color(color),
    );
}
