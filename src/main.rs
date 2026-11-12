//! Stagehand: installs, updates and starts the show tools (Show Master,
//! Beamweave and whatever comes next).

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod catalog;
mod dev;
mod github;
mod paths;
mod system;
mod ui;

fn main() -> eframe::Result<()> {
    if std::env::args().any(|a| a == "--version" || a == "-V") {
        println!("stagehand {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("Stagehand")
        .with_app_id("stagehand")
        .with_inner_size([820.0, 600.0])
        .with_min_inner_size([520.0, 420.0]);
    if let Ok(img) = image::load_from_memory(include_bytes!("../assets/icon.png")) {
        let img = img.to_rgba8();
        let (w, h) = img.dimensions();
        viewport = viewport.with_icon(egui::IconData {
            rgba: img.into_raw(),
            width: w,
            height: h,
        });
    }
    eframe::run_native(
        "Stagehand",
        eframe::NativeOptions {
            viewport,
            ..Default::default()
        },
        Box::new(|cc| Ok(Box::new(ui::Launcher::new(&cc.egui_ctx)))),
    )
}
