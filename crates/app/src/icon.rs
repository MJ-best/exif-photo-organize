//! App icon / mascot artwork (embedded PNGs generated from the same artwork as the
//! macOS `.icns` and the Windows `.ico`, see `crates/app/assets/README.md`).

use eframe::egui::{self, IconData};

/// 256 px, used for the window / taskbar icon.
const ICON_256: &[u8] = include_bytes!("../assets/icon-256.png");
/// 512 px, drawn inside the app (welcome screen, empty states).
const ICON_512: &[u8] = include_bytes!("../assets/icon-512.png");

fn decode(bytes: &[u8]) -> photoorg_core::image::RgbaImage {
    photoorg_core::image::load_from_memory(bytes).expect("embedded icon is a valid PNG").to_rgba8()
}

pub fn icon() -> IconData {
    let img = decode(ICON_256);
    let (width, height) = img.dimensions();
    IconData { rgba: img.into_raw(), width, height }
}

/// The mascot as a texture (loaded once per context).
pub fn mascot(ctx: &egui::Context) -> egui::TextureHandle {
    let id = egui::Id::unique("photoorg-mascot");
    if let Some(t) = ctx.data(|d| d.get_temp::<egui::TextureHandle>(id)) {
        return t;
    }
    let img = decode(ICON_512);
    let size = [img.width() as usize, img.height() as usize];
    let color = egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw());
    let tex = ctx.load_texture("mascot", color, egui::TextureOptions::LINEAR);
    ctx.data_mut(|d| d.insert_temp(id, tex.clone()));
    tex
}

/// Paint the mascot centred in `rect`.
pub fn paint_mascot(ctx: &egui::Context, painter: &egui::Painter, rect: egui::Rect) {
    let tex = mascot(ctx);
    painter.image(
        tex.id(),
        rect,
        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        egui::Color32::WHITE,
    );
}

#[cfg(test)]
mod tests {
    #[test]
    fn icon_decodes() {
        let i = super::icon();
        assert_eq!((i.width, i.height), (256, 256));
        // Corners transparent (the rounded square is cut out), centre opaque.
        assert_eq!(i.rgba[3], 0);
        assert_eq!(i.rgba[(128 * 256 + 128) * 4 + 3], 255);
        let m = super::decode(super::ICON_512);
        assert_eq!(m.dimensions(), (512, 512));
    }
}
