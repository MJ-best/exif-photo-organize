//! LOUPE — a fast, EXIF-driven photo importer, organiser and culling tool.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod app_access;
mod app_accuracy;
mod app_crop;
mod app_cull;
mod app_delete;
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
        .with_title("LOUPE")
        .with_app_id("photo-organize")
        .with_inner_size([1360.0, 860.0])
        .with_min_inner_size([960.0, 600.0])
        .with_icon(std::sync::Arc::new(icon::icon()));
    if cfg!(target_os = "macos") {
        // Unified title bar like Photos: traffic lights float over the sidebar.
        viewport = viewport.with_fullsize_content_view(true).with_titlebar_shown(false).with_title_shown(false);
    }
    let options = eframe::NativeOptions { viewport, ..Default::default() };
    eframe::run_native("LOUPE", options, Box::new(|cc| Ok(Box::new(app::PhotoApp::new(cc)))))
}

// `winit` 0.30's macOS backend has an unused `Window::set_blur` path that references the private
// CoreGraphics functions `CGSMainConnectionID` and `CGSSetWindowBackgroundBlurRadius`. LOUPE never
// turns window background blur on, but `WindowDelegate::new` branches on `attrs.blur` at runtime,
// leaving both symbols in the Mach-O import table and triggering App Store Connect's automated
// private-API rejection (Guideline 2.5.1). Defining them here as `.private_extern` no-ops resolves
// the references locally (instead of binding to CoreGraphics.framework) and lets `strip -x` remove
// their symbol names from the binary completely.
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
std::arch::global_asm!(
    ".section __TEXT,__text,regular,pure_instructions",
    ".p2align 2",
    ".globl _CGSMainConnectionID",
    ".private_extern _CGSMainConnectionID",
    "_CGSMainConnectionID:",
    "mov x0, #0",
    "ret",
    ".p2align 2",
    ".globl _CGSSetWindowBackgroundBlurRadius",
    ".private_extern _CGSSetWindowBackgroundBlurRadius",
    "_CGSSetWindowBackgroundBlurRadius:",
    "mov w0, #0",
    "ret",
);

#[cfg(all(target_os = "macos", target_arch = "x86_64"))]
std::arch::global_asm!(
    ".section __TEXT,__text,regular,pure_instructions",
    ".p2align 4",
    ".globl _CGSMainConnectionID",
    ".private_extern _CGSMainConnectionID",
    "_CGSMainConnectionID:",
    "xor eax, eax",
    "ret",
    ".p2align 4",
    ".globl _CGSSetWindowBackgroundBlurRadius",
    ".private_extern _CGSSetWindowBackgroundBlurRadius",
    "_CGSSetWindowBackgroundBlurRadius:",
    "xor eax, eax",
    "ret",
);
