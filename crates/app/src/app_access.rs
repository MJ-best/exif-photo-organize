//! The Mac App Store build runs in the App Sandbox: a card or folder the app has not been given
//! yet must be chosen once in the folder panel. This dialog explains why and opens the panel at
//! the right place; the choice is remembered ([`photoorg_macos::remember`]), so the same card opens
//! straight away next time. Outside the sandbox the dialog never appears.

use crate::app::PhotoApp;
use crate::theme::{bold, font};
use crate::widgets;
use eframe::egui::{self, Frame, Margin, Stroke, Ui};
use std::path::Path;

impl PhotoApp {
    pub fn access_dialog_ui(&mut self, ui: &mut Ui) {
        let Some((path, volume)) = self.access_request.clone() else { return };
        let pal = self.pal;
        let ctx = ui.ctx().clone();
        let name = volume
            .as_ref()
            .map(|v| v.name.clone())
            .or_else(|| path.file_name().map(|n| n.to_string_lossy().into_owned()))
            .unwrap_or_else(|| path.display().to_string());
        let modal = egui::Modal::new(egui::Id::unique("sandbox-access")).frame(
            Frame::new()
                .fill(pal.panel_bg)
                .corner_radius(14.0)
                .inner_margin(Margin::same(22))
                .stroke(Stroke::new(1.0, pal.separator)),
        );
        let (mut choose, mut cancel) = (false, false);
        let resp = modal.show(&ctx, |ui| {
            ui.set_width(440.0);
            ui.label(egui::RichText::new(format!("‘{name}’ 사용 허락")).font(bold(18.0)));
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new(
                    "앱스토어판은 macOS 보안 규칙에 따라, 카드나 폴더를 처음 쓸 때 한 번 허락이 필요해요. \
                     다음 창에서 이 카드를 고르고 ‘열기’를 누르세요. 같은 카드는 다음부터 바로 열립니다.",
                )
                .font(font(13.0))
                .color(pal.text2),
            );
            ui.add_space(16.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if widgets::primary_button(ui, "카드 고르기…", &pal, true).clicked() {
                    choose = true;
                }
                if widgets::secondary_button(ui, "취소", &pal).clicked() {
                    cancel = true;
                }
            });
        });
        if cancel || resp.should_close() {
            self.access_request = None;
            return;
        }
        if choose {
            self.access_request = None;
            // Open the panel next to the card so it is one click away.
            let start = path.parent().map(Path::to_path_buf).unwrap_or_else(|| path.clone());
            if let Some(dir) = rfd::FileDialog::new()
                .set_title(format!("‘{name}’을(를) 고르고 ‘열기’를 누르세요"))
                .set_directory(&start)
                .pick_folder()
            {
                photoorg_macos::remember(&dir);
                let volume = if dir == path { volume } else { None };
                self.start_scan(dir, volume);
            }
        }
    }
}
