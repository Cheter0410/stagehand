//! The window: one card per app with its state and what can be done
//! (Install, Update, Open). Network and installs run on worker threads that
//! report back through a channel.

use crate::catalog::{self, AppDef, Catalog};
use crate::dev::{self, RepoState, Settings, Step};
use crate::github::{self, Release};
use crate::system::{self, Installed};
use egui::{Color32, CornerRadius, Frame, Margin, RichText, Stroke, Vec2};
use std::collections::HashMap;
use std::path::PathBuf;
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
    /// Source checkouts found: (app id, folder, state).
    Repos(Vec<(String, Option<PathBuf>, Option<RepoState>)>),
    /// An app was uninstalled (or not).
    Removed(String, Result<(), String>),
    /// A source checkout was deleted (or not).
    Deleted(String, Result<String, String>),
}

#[derive(Clone, Copy, PartialEq)]
enum Job {
    Download(u64, u64),
    Install,
    Remove,
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
    /// "Remove from this computer?" is being asked.
    confirm_remove: bool,
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
    settings: Settings,
    /// App id -> (checkout, state), for the developer section.
    repos: HashMap<String, (Option<PathBuf>, Option<RepoState>)>,
    last_repo_scan: Option<Instant>,
    dev_message: Option<(String, bool)>,
    /// A source folder being typed in (app id, text).
    path_edit: Option<(String, String)>,
    /// App id whose checkout is asked to be deleted / being deleted.
    delete_ask: Option<String>,
    deleting: Option<String>,
}

/// What the developer section asks for.
enum DevAction {
    Release(String),
    Install(String),
    Clone(String),
    ReleaseAll,
    InstallAll,
    EditPath(String),
    SetPath(String, String),
    CancelPath,
    /// Ask before deleting the checkout.
    Delete(String),
    DeleteConfirm(String),
    DeleteCancel,
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
            settings: Settings::load(),
            repos: HashMap::new(),
            last_repo_scan: None,
            dev_message: None,
            path_edit: None,
            delete_ask: None,
            deleting: None,
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
            let me = self_def();
            let r = github::latest(SELF_RELEASES, |r| system::pick_asset(&me, r).is_some())
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
                        confirm_remove: false,
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
                let r = github::latest(&def.releases, |r| system::pick_asset(&def, r).is_some());
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
                Msg::Repos(list) => {
                    for (id, dir, state) in list {
                        self.repos.insert(id, (dir, state));
                    }
                }
                Msg::Removed(id, r) => {
                    if let Some(c) = self.card_mut(&id) {
                        c.job = None;
                        c.message = Some(match r {
                            Ok(()) => (format!("{} was removed.", c.def.name), false),
                            Err(e) => (e, true),
                        });
                    }
                    self.detect(ctx);
                }
                Msg::Deleted(id, r) => {
                    self.deleting = None;
                    if r.is_ok() {
                        self.settings.repos.remove(&id);
                        self.settings.save();
                        self.repos.remove(&id);
                    }
                    self.dev_message = Some(match r {
                        Ok(m) => (m, false),
                        Err(e) => (e, true),
                    });
                    self.scan_repos(ctx);
                }
            }
        }
    }

    /// Apps with a source repository: the catalog's, then Stagehand itself.
    fn dev_apps(&self) -> Vec<AppDef> {
        let mut v: Vec<AppDef> = self
            .cards
            .iter()
            .filter(|c| c.def.dev.is_some())
            .map(|c| c.def.clone())
            .collect();
        v.push(self_def());
        v
    }

    /// Find the source checkouts and their git state (off the UI thread).
    fn scan_repos(&mut self, ctx: &egui::Context) {
        self.last_repo_scan = Some(Instant::now());
        let (apps, settings, tx, c) = (
            self.dev_apps(),
            self.settings.clone(),
            self.tx.clone(),
            ctx.clone(),
        );
        std::thread::spawn(move || {
            let list = apps
                .iter()
                .map(|a| {
                    let dir = dev::find_checkout(a, &settings);
                    let state = dir.as_deref().and_then(dev::repo_state);
                    (a.id.clone(), dir, state)
                })
                .collect();
            let _ = tx.send(Msg::Repos(list));
            c.request_repaint();
        });
    }

    /// The checkout of an app, if it is a usable git checkout.
    fn checkout(&self, id: &str) -> Option<PathBuf> {
        self.repos
            .get(id)
            .filter(|(_, s)| s.is_some())
            .and_then(|(d, _)| d.clone())
    }

    fn dev_action(&mut self, action: DevAction, ctx: &egui::Context) {
        let apps = self.dev_apps();
        let release_step = |a: &AppDef, dir: PathBuf| {
            Some(Step {
                name: format!("Release {}", a.name),
                dir,
                command: a.dev.as_ref()?.release.clone()?,
            })
        };
        let install_step = |a: &AppDef, dir: PathBuf| {
            Some(Step {
                name: format!("Install {}", a.name),
                dir,
                command: dev::install_command(a)?,
            })
        };
        let one = |id: &str, make: &dyn Fn(&AppDef, PathBuf) -> Option<Step>| -> Vec<Step> {
            apps.iter()
                .find(|a| a.id == id)
                .and_then(|a| make(a, self.checkout(id)?))
                .into_iter()
                .collect()
        };
        let all = |make: &dyn Fn(&AppDef, PathBuf) -> Option<Step>| -> Vec<Step> {
            apps.iter()
                .filter_map(|a| make(a, self.checkout(&a.id)?))
                .collect()
        };
        let run = match action {
            DevAction::Release(id) => Some(one(&id, &release_step)),
            DevAction::Install(id) => Some(one(&id, &install_step)),
            DevAction::ReleaseAll => Some(all(&release_step)),
            DevAction::InstallAll => Some(all(&install_step)),
            DevAction::Clone(id) => {
                let others: Vec<PathBuf> =
                    self.repos.values().filter_map(|(d, _)| d.clone()).collect();
                let app = apps.iter().find(|a| a.id == id);
                match (app, app.and_then(|a| dev::default_dir(a, &others))) {
                    (Some(app), Some(target)) => {
                        let parent = target.parent().map(PathBuf::from).unwrap_or_default();
                        let _ = std::fs::create_dir_all(&parent);
                        let repo = app.dev.as_ref().map(|d| d.repo.clone()).unwrap_or_default();
                        self.settings.repos.insert(id, target.display().to_string());
                        self.settings.save();
                        Some(vec![Step {
                            name: format!("Clone {} into {}", app.name, target.display()),
                            dir: parent,
                            command: format!(
                                "git clone https://github.com/{repo}.git {}",
                                dev::quote(&target.display().to_string())
                            ),
                        }])
                    }
                    _ => None,
                }
            }
            DevAction::EditPath(id) => {
                let cur = self
                    .repos
                    .get(&id)
                    .and_then(|(d, _)| d.as_ref())
                    .map(|p| p.display().to_string())
                    .unwrap_or_default();
                self.path_edit = Some((id, cur));
                None
            }
            DevAction::SetPath(id, text) => {
                let t = text.trim();
                if t.is_empty() {
                    self.settings.repos.remove(&id);
                } else {
                    let p = match (t.strip_prefix("~/"), dirs::home_dir()) {
                        (Some(rest), Some(home)) => home.join(rest),
                        _ => PathBuf::from(t),
                    };
                    self.settings.repos.insert(id, p.display().to_string());
                }
                self.settings.save();
                self.path_edit = None;
                None
            }
            DevAction::CancelPath => {
                self.path_edit = None;
                None
            }
            DevAction::Delete(id) => {
                self.delete_ask = Some(id);
                None
            }
            DevAction::DeleteCancel => {
                self.delete_ask = None;
                None
            }
            DevAction::DeleteConfirm(id) => {
                self.delete_ask = None;
                if let Some(dir) = self.repos.get(&id).and_then(|(d, _)| d.clone()) {
                    self.deleting = Some(id.clone());
                    let (tx, c) = (self.tx.clone(), ctx.clone());
                    std::thread::spawn(move || {
                        let r = dev::delete_checkout(&dir)
                            .map(|()| format!("Deleted {}.", short_path(&dir)));
                        let _ = tx.send(Msg::Deleted(id, r));
                        c.request_repaint();
                    });
                }
                None
            }
        };
        if let Some(steps) = run {
            self.dev_message = Some(if steps.is_empty() {
                ("Nothing to do: no source folder found.".into(), true)
            } else {
                let title = if steps.len() == 1 {
                    steps[0].name.clone()
                } else if steps[0].name.starts_with("Release") {
                    "Release all apps".into()
                } else {
                    "Install all apps from source".into()
                };
                match dev::open_terminal(&title, &steps) {
                    Ok(()) => (format!("Opened a terminal: {title}"), false),
                    Err(e) => (e, true),
                }
            });
        }
        self.scan_repos(ctx);
    }

    fn start_remove(&mut self, id: &str, ctx: &egui::Context) {
        let tx = self.tx.clone();
        let Some(c) = self.card_mut(id) else { return };
        let Some(inst) = c.installed.clone() else {
            return;
        };
        c.confirm_remove = false;
        c.job = Some(Job::Remove);
        let (def, cx) = (c.def.clone(), ctx.clone());
        std::thread::spawn(move || {
            let r = system::uninstall(&def, &inst);
            let _ = tx.send(Msg::Removed(def.id.clone(), r));
            cx.request_repaint();
        });
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
        dev: Some(catalog::DevDef {
            repo: "Cheter0410/stagehand".into(),
            folder: "stagehand".into(),
            release: Some("./release.sh".into()),
            install_macos: Some("bash install-mac.sh".into()),
            install_linux: Some("./install.sh".into()),
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
    /// Show / hide the "remove?" question.
    AskRemove(bool),
    Remove,
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
                        if dev::available()
                            && ui
                                .selectable_label(self.settings.dev_mode, "Developer")
                                .on_hover_text("Release apps and install them from source")
                                .clicked()
                        {
                            self.settings.dev_mode = !self.settings.dev_mode;
                            self.settings.save();
                        }
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
                                    Some(Job::Install | Job::Remove) => {
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
                let mut dev_actions: Vec<DevAction> = Vec::new();
                let dev_on = self.settings.dev_mode && dev::available();
                if dev_on && self.last_repo_scan.is_none_or(|t| t.elapsed() > Duration::from_secs(10)) {
                    self.scan_repos(&ctx);
                }
                let dev_apps = if dev_on { self.dev_apps() } else { Vec::new() };
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    for c in &self.cards {
                        if let Some(a) = card(ui, c) {
                            actions.push((c.def.id.clone(), a));
                        }
                        ui.add_space(10.0);
                    }
                    if dev_on {
                        let del = DeleteState { ask: self.delete_ask.as_deref(), busy: self.deleting.as_deref() };
                        dev_section(ui, &dev_apps, &self.repos, &mut self.path_edit, del, &self.dev_message, &mut dev_actions);
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
                for a in dev_actions {
                    self.dev_action(a, &ctx);
                }
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
                        Action::AskRemove(on) => {
                            if let Some(c) = self.card_mut(&id) {
                                c.confirm_remove = on;
                                c.message = None;
                            }
                        }
                        Action::Remove => self.start_remove(&id, &ctx),
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
                        Some(Job::Remove) => {
                            ui.label("Removing…");
                            ui.spinner();
                        }
                        None if c.confirm_remove => {
                            if ui.button("Cancel").clicked() {
                                action = Some(Action::AskRemove(false));
                            }
                            if ui
                                .add(egui::Button::new(RichText::new("Remove").strong().color(Color32::WHITE)).fill(ERROR))
                                .clicked()
                            {
                                action = Some(Action::Remove);
                            }
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
                            if let Some(inst) = &c.installed {
                                let removable = system::removable(&c.def, inst);
                                let b = ui
                                    .add_enabled(
                                        removable && !c.running,
                                        egui::Button::new(RichText::new("Remove…").small().color(MUTED))
                                            .frame(false),
                                    )
                                    .on_hover_text("Uninstall it from this computer")
                                    .on_disabled_hover_text(if c.running {
                                        format!("Quit {} first - it is running.", c.def.name)
                                    } else {
                                        format!(
                                            "Installed by the system ({}) - remove it with the package manager.",
                                            inst.path.display()
                                        )
                                    });
                                if b.clicked() {
                                    action = Some(Action::AskRemove(true));
                                }
                            }
                        }
                    },
                );
            });
            if c.confirm_remove && c.job.is_none() {
                ui.add_space(4.0);
                ui.label(
                    RichText::new(format!(
                        "Remove {} from this computer? Its settings and your files (shows, presentations) stay.",
                        c.def.name
                    ))
                    .color(WARN),
                );
            }
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

/// The developer section: per app its source folder and git state, with
/// Release / Install (or Clone), and both for all apps at once.
/// Deleting a checkout: which app is asked about / being deleted.
#[derive(Clone, Copy)]
struct DeleteState<'a> {
    ask: Option<&'a str>,
    busy: Option<&'a str>,
}

fn dev_section(
    ui: &mut egui::Ui,
    apps: &[AppDef],
    repos: &HashMap<String, (Option<PathBuf>, Option<RepoState>)>,
    path_edit: &mut Option<(String, String)>,
    del: DeleteState,
    message: &Option<(String, bool)>,
    out: &mut Vec<DevAction>,
) {
    Frame::new()
        .fill(CARD)
        .stroke(Stroke::new(1.0, LINE))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::same(14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(RichText::new("Developer").size(17.0).strong());
                ui.label(
                    RichText::new("opens a terminal and runs the project's own scripts")
                        .small()
                        .color(MUTED),
                );
            });
            for a in apps {
                let (dir, state) = repos.get(&a.id).cloned().unwrap_or((None, None));
                let accent = catalog::parse_color(&a.color)
                    .map(|[r, g, b]| Color32::from_rgb(r, g, b))
                    .unwrap_or(OK);
                ui.add_space(4.0);
                ui.separator();
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(Vec2::new(4.0, 34.0), egui::Sense::hover());
                    ui.painter().rect_filled(r, 2.0, accent);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let d = a.dev.as_ref();
                        if del.busy == Some(a.id.as_str()) {
                            ui.label(RichText::new("Deleting…").color(MUTED));
                            ui.spinner();
                        } else if del.ask == Some(a.id.as_str()) {
                            if ui.button("Cancel").clicked() {
                                out.push(DevAction::DeleteCancel);
                            }
                            if ui
                                .add(egui::Button::new(RichText::new("Delete").strong().color(Color32::WHITE)).fill(ERROR))
                                .clicked()
                            {
                                out.push(DevAction::DeleteConfirm(a.id.clone()));
                            }
                        } else if dir.is_some() && state.is_some() {
                            if dev::install_command(a).is_some()
                                && ui
                                    .button("Install")
                                    .on_hover_text("Get the latest source, build it and install it on this computer")
                                    .clicked()
                            {
                                out.push(DevAction::Install(a.id.clone()));
                            }
                            if d.and_then(|d| d.release.as_ref()).is_some()
                                && ui
                                    .add(egui::Button::new(RichText::new("Release").strong().color(Color32::BLACK)).fill(accent))
                                    .on_hover_text("Build a new version and upload it (release.sh)")
                                    .clicked()
                            {
                                out.push(DevAction::Release(a.id.clone()));
                            }
                            if ui
                                .add(egui::Button::new(RichText::new("Delete…").small().color(MUTED)).frame(false))
                                .on_hover_text("Delete this source folder (with its build files) from this computer")
                                .clicked()
                            {
                                out.push(DevAction::Delete(a.id.clone()));
                            }
                        } else if d.is_some()
                            && ui.button("Clone").on_hover_text("Get the source from GitHub").clicked()
                        {
                            out.push(DevAction::Clone(a.id.clone()));
                        }
                        ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                            ui.label(RichText::new(&a.name).strong());
                            match path_edit.as_mut().filter(|(id, _)| id == &a.id) {
                                Some((_, text)) => {
                                    ui.horizontal(|ui| {
                                        let r = ui.add(
                                            egui::TextEdit::singleline(text)
                                                .hint_text("~/dev/…  (empty = search automatically)")
                                                .desired_width((ui.available_width() - 120.0).max(120.0)),
                                        );
                                        let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                                        if ui.small_button("OK").clicked() || enter {
                                            out.push(DevAction::SetPath(a.id.clone(), text.clone()));
                                        } else if ui.small_button("Cancel").clicked() {
                                            out.push(DevAction::CancelPath);
                                        }
                                    });
                                }
                                None if del.ask == Some(a.id.as_str()) => {
                                    let place = dir.as_deref().map(short_path).unwrap_or_default();
                                    let (text, color) = match state.as_ref().and_then(RepoState::would_lose) {
                                        Some(lost) => (format!("Delete {place}? Lost for good: {lost}."), ERROR),
                                        None => (
                                            format!("Delete {place}? Everything in it is on GitHub; Clone gets it back."),
                                            WARN,
                                        ),
                                    };
                                    ui.add(egui::Label::new(RichText::new(text).small().color(color)).wrap());
                                }
                                None => {
                                    ui.horizontal(|ui| {
                                        if ui
                                            .add(egui::Button::new(RichText::new("Folder…").small().color(MUTED)).small())
                                            .on_hover_text("Use another source folder")
                                            .clicked()
                                        {
                                            out.push(DevAction::EditPath(a.id.clone()));
                                        }
                                        let (text, color) = match (&dir, &state) {
                                            (Some(d), Some(s)) => {
                                                let mut t = format!("{} · {}", short_path(d), s.branch);
                                                if s.changes > 0 {
                                                    t.push_str(&format!(" · {} changed file(s)", s.changes));
                                                }
                                                if s.behind > 0 {
                                                    t.push_str(&format!(" · {} new commit(s) on GitHub", s.behind));
                                                }
                                                (t, if s.changes > 0 { WARN } else { MUTED })
                                            }
                                            (Some(d), None) => (format!("{} · not a git checkout", short_path(d)), ERROR),
                                            (None, _) => ("source not on this computer".into(), MUTED),
                                        };
                                        ui.add(egui::Label::new(RichText::new(text).small().color(color)).truncate());
                                    });
                                }
                            }
                        });
                    });
                });
            }
            ui.add_space(4.0);
            ui.separator();
            ui.horizontal(|ui| {
                let any = apps
                    .iter()
                    .any(|a| repos.get(&a.id).is_some_and(|(d, s)| d.is_some() && s.is_some()));
                if ui
                    .add_enabled(any, egui::Button::new("Release all"))
                    .on_hover_text("One terminal that releases every app in turn (Stagehand last)")
                    .clicked()
                {
                    out.push(DevAction::ReleaseAll);
                }
                if ui
                    .add_enabled(any, egui::Button::new("Install all"))
                    .on_hover_text("One terminal that builds and installs every app from source (Stagehand last)")
                    .clicked()
                {
                    out.push(DevAction::InstallAll);
                }
                if let Some((m, err)) = message {
                    ui.label(RichText::new(m).small().color(if *err { ERROR } else { OK }));
                }
            });
        });
}

/// A folder with the home folder shown as `~`.
fn short_path(p: &std::path::Path) -> String {
    match dirs::home_dir().and_then(|h| p.strip_prefix(h).ok().map(PathBuf::from)) {
        Some(rest) => format!("~/{}", rest.display()),
        None => p.display().to_string(),
    }
}
