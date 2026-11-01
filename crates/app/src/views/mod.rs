//! Screens and panels.

mod filters;
mod grid;
mod import;
mod info;
mod sidebar;
mod toolbar;
mod viewer;

use crate::app::{MainView, PhotoApp};
use crate::config::ThemePref;
use crate::theme::{bold, font};
use crate::widgets::{self, Icon};
use eframe::egui::{self, pos2, vec2, Align2, Color32, Frame, Margin, Rect, Sense, Ui};
use photoorg_core::format;
use photoorg_core::template;

pub const SIDEBAR_W: f32 = 232.0;
pub const INFO_W: f32 = 300.0;

/// Extra top padding on macOS where the title bar is merged into the window content.
pub fn titlebar_pad() -> f32 {
    if cfg!(target_os = "macos") {
        28.0
    } else {
        0.0
    }
}

impl PhotoApp {
    pub fn draw(&mut self, ui: &mut Ui) {
        let pal = self.pal;
        egui::Panel::left("sidebar")
            .exact_size(SIDEBAR_W)
            .resizable(false)
            .show_separator_line(false)
            .frame(Frame::new().fill(pal.sidebar_bg).inner_margin(Margin { left: 10, right: 10, top: 0, bottom: 8 }))
            .show(ui, |ui| self.sidebar(ui));

        let show_info = self.cfg.show_info && self.view == MainView::Library && self.lib.is_some();
        if show_info {
            egui::Panel::right("info")
                .exact_size(INFO_W)
                .resizable(false)
                .show_separator_line(false)
                .frame(Frame::new().fill(pal.panel_bg).inner_margin(Margin::same(0)))
                .show(ui, |ui| self.info_panel(ui));
        }

        egui::CentralPanel::no_frame().show(ui, |ui| {
            let r = ui.max_rect();
            ui.painter().rect_filled(r, 0.0, pal.content_bg);
            // Hairline between sidebar and content.
            ui.painter().line_segment([r.left_top(), r.left_bottom()], egui::Stroke::new(1.0, pal.separator));
            if show_info {
                ui.painter().line_segment([r.right_top(), r.right_bottom()], egui::Stroke::new(1.0, pal.separator));
            }
            match self.view {
                MainView::Welcome => self.welcome(ui),
                MainView::Import => self.import_view(ui),
                MainView::Library => {
                    if self.viewer.is_some() {
                        self.viewer_view(ui);
                    } else {
                        self.toolbar(ui);
                        if self.show_filters || self.filter_active() {
                            self.filter_bar(ui);
                        }
                        self.browse(ui);
                    }
                }
            }
        });

        self.overlays(ui);
        if self.settings_open {
            self.settings(ui);
        }
    }

    pub fn filter_active(&self) -> bool {
        let mut f = self.filter.clone();
        f.text.clear();
        // Sidebar presets are not shown as filter tokens.
        f.import_id = None;
        f != photoorg_core::Filter::default() && self.filter.active_count() > self.preset_filter_count()
    }

    fn preset_filter_count(&self) -> usize {
        use crate::app::SideSel::*;
        match self.side {
            Rated | Unrated | Videos | Pairs | Camera(_) | Lens(_) => 1,
            _ => 0,
        }
    }

    fn welcome(&mut self, ui: &mut Ui) {
        let pal = self.pal;
        let ctx = ui.ctx().clone();
        let rect = ui.max_rect();
        let n_recent = self.cfg.recent_libraries.len().min(6);
        let h = 330.0
            + if self.lib_error.is_some() { 48.0 } else { 0.0 }
            + if n_recent > 0 { 34.0 + 24.0 * n_recent as f32 } else { 0.0 };
        let card = Rect::from_center_size(rect.center(), vec2(480.0_f32.min(rect.width() - 40.0), h));
        ui.painter().rect_filled(card, 16.0, pal.panel_bg);
        let mut child = ui.new_child(
            egui::UiBuilder::new().max_rect(card.shrink(28.0)).layout(egui::Layout::top_down(egui::Align::Center)),
        );
        let ui = &mut child;
        ui.add_space(6.0);
        let (ir, _) = ui.allocate_exact_size(vec2(64.0, 64.0), Sense::hover());
        ui.painter().rect_filled(ir, 16.0, pal.accent);
        widgets::paint_icon(ui.painter(), ir.shrink(14.0), Icon::Photos, Color32::WHITE);
        ui.add_space(10.0);
        ui.label(egui::RichText::new("Photo Organize").font(bold(24.0)));
        ui.add_space(4.0);
        ui.label(
            egui::RichText::new(
                "SD 카드의 사진을 외장 하드로 옮기면서\nEXIF 기준으로 자동 정리하고, 별점으로 셀렉하세요.",
            )
            .font(font(13.0))
            .color(pal.text2),
        );
        ui.add_space(16.0);
        if let Some(e) = &self.lib_error {
            ui.label(egui::RichText::new(e).color(pal.danger).font(font(12.5)));
            ui.add_space(8.0);
        }
        if widgets::primary_button(ui, "라이브러리 폴더 선택…", &pal, true).clicked() {
            self.choose_library(&ctx);
        }
        ui.add_space(4.0);
        ui.label(
            egui::RichText::new("사진을 보관할 폴더 (예: 외장하드/Photos). 없으면 새로 만듭니다.")
                .font(font(11.5))
                .color(pal.text3),
        );
        if !self.cfg.recent_libraries.is_empty() {
            ui.add_space(14.0);
            ui.label(egui::RichText::new("최근 라이브러리").font(bold(12.0)).color(pal.text2));
            let recent: Vec<_> = self.cfg.recent_libraries.iter().take(6).cloned().collect();
            for p in recent {
                let exists = p.exists();
                let txt = format!("{}{}", short_path(&p), if exists { "" } else { "  (연결 안 됨)" });
                let r = ui
                    .add_enabled(exists, egui::Button::new(egui::RichText::new(txt).font(font(12.5))).frame(false))
                    .on_hover_text(p.display().to_string());
                if r.clicked() {
                    self.open_library(&ctx, p, false);
                }
            }
        }
    }

    fn overlays(&mut self, ui: &mut Ui) {
        let pal = self.pal;
        let ctx = ui.ctx().clone();
        let screen = ctx.content_rect();
        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::unique("overlays")));
        // Rating HUD.
        if let Some((r, _)) = self.hud {
            let rect = Rect::from_center_size(screen.center() + vec2(SIDEBAR_W / 2.0, 0.0), vec2(230.0, 70.0));
            painter.rect_filled(rect, 14.0, Color32::from_black_alpha(190));
            if r == 0 {
                painter.text(rect.center(), Align2::CENTER_CENTER, "별점 지움", bold(18.0), Color32::WHITE);
            } else {
                for i in 0..5u8 {
                    let c = pos2(rect.center().x + (i as f32 - 2.0) * 38.0, rect.center().y);
                    if i < r {
                        widgets::star_shape(&painter, c, 15.0, Some(pal.star), None);
                    } else {
                        widgets::star_shape(
                            &painter,
                            c,
                            15.0,
                            None,
                            Some(egui::Stroke::new(1.2, Color32::from_white_alpha(120))),
                        );
                    }
                }
            }
        }
        // Toasts.
        let mut y = screen.bottom() - 24.0;
        for t in self.toasts.iter().rev() {
            let g = painter.layout(t.text.clone(), font(13.0), Color32::WHITE, 520.0);
            let rect = Rect::from_center_size(
                pos2(screen.center().x + SIDEBAR_W / 2.0, y - g.size().y / 2.0 - 10.0),
                g.size() + vec2(32.0, 20.0),
            );
            painter.rect_filled(
                rect,
                10.0,
                if t.error {
                    pal.danger.gamma_multiply(0.95)
                } else {
                    Color32::from_rgba_unmultiplied(40, 40, 44, 235)
                },
            );
            painter.galley(rect.center() - g.size() / 2.0, g, Color32::WHITE);
            y = rect.top() - 8.0;
        }
    }

    fn settings(&mut self, ui: &mut Ui) {
        let pal = self.pal;
        let ctx = ui.ctx().clone();
        let modal = egui::Modal::new(egui::Id::unique("settings")).frame(
            Frame::new()
                .fill(pal.panel_bg)
                .corner_radius(14.0)
                .inner_margin(Margin::same(22))
                .stroke(egui::Stroke::new(1.0, pal.separator)),
        );
        let max_h = (ctx.content_rect().height() - 140.0).max(300.0);
        let resp = modal.show(&ctx, |ui| {
            ui.set_width(540.0);
            ui.label(egui::RichText::new("설정").font(bold(20.0)));
            ui.add_space(10.0);
            egui::ScrollArea::vertical().max_height(max_h).auto_shrink([false, true]).show(ui, |ui| {
                ui.label(egui::RichText::new("라이브러리").font(bold(13.0)).color(pal.text2));
                ui.horizontal(|ui| {
                    let p = self
                        .lib
                        .as_ref()
                        .map(|l| l.lib.root.display().to_string())
                        .unwrap_or_else(|| "선택 안 됨".into());
                    ui.label(egui::RichText::new(p).font(font(12.5)));
                    if widgets::secondary_button(ui, "변경…", &pal).clicked() {
                        self.settings_open = false;
                        self.choose_library(&ctx);
                    }
                });
                ui.add_space(12.0);

                ui.label(egui::RichText::new("화면").font(bold(13.0)).color(pal.text2));
                ui.horizontal(|ui| {
                    let mut t = self.cfg.theme;
                    if widgets::segmented(
                        ui,
                        &mut t,
                        &[(ThemePref::Dark, "다크"), (ThemePref::Light, "라이트"), (ThemePref::System, "시스템")],
                        &pal,
                    ) {
                        self.set_theme(&ctx, t);
                    }
                });
                ui.add_space(6.0);
                widgets::switch(ui, &mut self.cfg.square_tiles, "썸네일을 정사각형으로 자르기", &pal);
                widgets::switch(ui, &mut self.cfg.show_filmstrip, "사진 보기에서 필름 스트립 표시", &pal);
                ui.add_space(12.0);

                ui.label(egui::RichText::new("셀렉 (별점)").font(bold(13.0)).color(pal.text2));
                widgets::switch(ui, &mut self.cfg.auto_advance, "별점을 매기면 다음 사진으로 이동", &pal);
                if widgets::switch(
                    ui,
                    &mut self.cfg.write_xmp,
                    "별점·태그를 XMP 사이드카(.xmp)에 기록 — Lightroom/Capture One 호환",
                    &pal,
                )
                .changed()
                {
                    self.xmp_enabled.store(self.cfg.write_xmp, std::sync::atomic::Ordering::Relaxed);
                }
                ui.add_space(12.0);

                ui.label(egui::RichText::new("가져오기").font(bold(13.0)).color(pal.text2));
                widgets::switch(ui, &mut self.cfg.auto_open_import, "SD 카드를 연결하면 가져오기 화면 열기", &pal);
                widgets::switch(ui, &mut self.cfg.eject_after_import, "가져오기가 끝나면 카드 추출", &pal);
                ui.add_space(12.0);

                ui.label(egui::RichText::new("데이터").font(bold(13.0)).color(pal.text2));
                ui.horizontal(|ui| {
                    if widgets::secondary_button(ui, "카탈로그 CSV로 내보내기…", &pal).clicked() {
                        self.export_csv();
                    }
                    if widgets::secondary_button(ui, "썸네일 모두 다시 만들기", &pal).clicked() {
                        if let Some(st) = &self.lib {
                            let ids: Vec<i64> =
                                st.index.assets.iter().filter(|a| !a.is_video()).map(|a| a.id).collect();
                            self.rebuild_thumbnails(&ctx, ids);
                            if let Some(t) = &mut self.thumbs {
                                t.clear();
                            }
                        }
                    }
                });
                ui.add_space(14.0);
                ui.label(egui::RichText::new("단축키").font(bold(13.0)).color(pal.text2));
                for (k, d) in [
                    ("F1–F5  /  1–5", "별점 1–5개"),
                    ("0  /  F6", "별점 지우기"),
                    ("Space / Enter", "사진 크게 보기 / 돌아가기"),
                    ("← → ↑ ↓", "이동 (Shift: 범위 선택)"),
                    ("Z", "100% 확대 (사진 보기)"),
                    ("⌘/Ctrl + 1·2·3·4", "연 · 월 · 일 · 모든 사진"),
                    ("⌘/Ctrl + I", "정보 패널"),
                    ("⌘/Ctrl + F", "검색"),
                    ("⌘/Ctrl + A", "모두 선택"),
                    ("⌘/Ctrl + + / −", "썸네일 크기"),
                ] {
                    ui.horizontal(|ui| {
                        ui.add_sized(vec2(150.0, 18.0), egui::Label::new(egui::RichText::new(k).font(bold(12.0))));
                        ui.label(egui::RichText::new(d).font(font(12.0)).color(pal.text2));
                    });
                }
            });
            ui.add_space(14.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if widgets::primary_button(ui, "완료", &pal, true).clicked() {
                    self.settings_open = false;
                }
            });
        });
        if resp.should_close() {
            self.settings_open = false;
        }
        if !self.settings_open {
            self.cfg.save();
        }
    }

    pub fn export_csv(&mut self) {
        let Some(st) = &self.lib else { return };
        let Some(path) = rfd::FileDialog::new()
            .set_title("카탈로그 내보내기")
            .set_file_name(format!("{}-catalog.csv", st.lib.name()))
            .add_filter("CSV", &["csv"])
            .save_file()
        else {
            return;
        };
        let res = (|| -> anyhow::Result<usize> {
            use std::io::Write;
            let mut f = std::fs::File::create(&path)?;
            f.write_all("\u{feff}".as_bytes())?;
            st.catalog.export_csv(std::io::BufWriter::new(f))
        })();
        match res {
            Ok(n) => self.toast(format!("{}개 항목을 내보냈습니다: {}", format::count(n), path.display()), false),
            Err(e) => self.toast(format!("내보내기 실패: {e:#}"), true),
        }
    }

    /// Show a file in the OS file manager.
    pub fn reveal(&mut self, path: &std::path::Path) {
        let res = if cfg!(target_os = "macos") {
            std::process::Command::new("open").arg("-R").arg(path).spawn().map(|_| ())
        } else if cfg!(target_os = "windows") {
            std::process::Command::new("explorer").arg(format!("/select,{}", path.display())).spawn().map(|_| ())
        } else {
            let dir = path.parent().unwrap_or(path);
            std::process::Command::new("xdg-open").arg(dir).spawn().map(|_| ())
        };
        if let Err(e) = res {
            self.toast(format!("파일 위치를 열 수 없습니다: {e}"), true);
        }
    }

    /// Example destination path for the import options.
    pub fn example_destination(&self) -> String {
        let o = &self.cfg.import;
        let mut m = photoorg_core::Metadata {
            make: Some("Canon".into()),
            model: Some("Canon EOS R5".into()),
            captured_at: Some(chrono::Local::now().naive_local()),
            ..Default::default()
        };
        let mut stem = "IMG_0001".to_string();
        if let Some(sess) = &self.import {
            if let Some(g) =
                sess.scan.as_ref().and_then(|s| s.groups.iter().find(|g| !g.is_imported() && !g.is_video()))
            {
                m = g.meta.clone();
                stem = g.stem.clone();
            }
        }
        let dir = template::render(&o.folder_template, &m, None, &stem);
        let name = template::render(&o.file_template, &m, None, &stem);
        let root = self.lib.as_ref().map(|l| l.lib.name()).unwrap_or_default();
        let raw =
            if o.raw_subfolder.trim().is_empty() { String::new() } else { format!("{}/", o.raw_subfolder.trim()) };
        format!("{root}/{dir}/{name}.JPG   ·   {root}/{dir}/{raw}{name}.RAW")
    }
}

/// "…/외장하드/Photos" style shortening for long paths.
pub fn short_path(p: &std::path::Path) -> String {
    let comps: Vec<String> = p.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
    if comps.len() <= 4 {
        return p.display().to_string();
    }
    format!("…/{}", comps[comps.len() - 3..].join("/"))
}
