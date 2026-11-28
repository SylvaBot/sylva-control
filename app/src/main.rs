#![allow(clippy::too_many_arguments)]
// Never open a console window on Windows, in debug or release builds.
#![windows_subsystem = "windows"]

//! Sylva Control: a native window (drawn on the GPU with egui) over pine-core's Sylva and
//! Twitch clients and the song request player.

mod activity;
mod app;
mod backend;
mod drag;
mod format;
mod secrets;
mod updater;

fn main() -> eframe::Result<()> {
    updater::clean_up_old_build();
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("tokio runtime");
    let _guard = rt.enter();
    let icon = image::load_from_memory(include_bytes!("../icons/128x128.png")).ok().map(|i| {
        let i = i.to_rgba8();
        eframe::egui::IconData { width: i.width(), height: i.height(), rgba: i.into_raw() }
    });
    let mut viewport = eframe::egui::ViewportBuilder::default()
        .with_title("Sylva Control")
        .with_app_id("app.pine.desktop")
        .with_inner_size([1180.0, 820.0])
        .with_min_inner_size([360.0, 420.0])
        .with_decorations(false);
    if let Some(icon) = icon {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions { viewport, persist_window: true, ..Default::default() };
    let handle = rt.handle().clone();
    eframe::run_native("Sylva Control", options, Box::new(move |cc| Ok(Box::new(app::App::new(cc, handle)))))
}
