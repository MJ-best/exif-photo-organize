//! Photo Organize — a fast, EXIF-driven photo importer, organiser and culling tool.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod app_access;
mod app_accuracy;
mod app_cull;
mod app_export;
#[cfg(feature = "license")]
mod app_license;
mod app_projects;
mod app_works;
mod config;
mod fonts;
mod icon;
mod layout;
#[cfg(feature = "license")]
mod license;
mod theme;
mod thumbs;
mod views;
mod widgets;

use eframe::egui;

fn main() -> eframe::Result {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("Photo Organize")
        .with_app_id("photo-organize")
        .with_inner_size([1360.0, 860.0])
        .with_min_inner_size([960.0, 600.0])
        .with_icon(std::sync::Arc::new(icon::icon()));
    if cfg!(target_os = "macos") {
        // Unified title bar like Photos: traffic lights float over the sidebar.
        viewport = viewport.with_fullsize_content_view(true).with_titlebar_shown(false).with_title_shown(false);
    }
    let options = eframe::NativeOptions { viewport, ..Default::default() };
    eframe::run_native("Photo Organize", options, Box::new(|cc| Ok(Box::new(app::PhotoApp::new(cc)))))
}
