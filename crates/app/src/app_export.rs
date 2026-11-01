//! Exporting photos: background job with progress, and the "작게 내보내기" dialog.

use crate::app::PhotoApp;
use crate::theme::{bold, font};
use crate::widgets;
use crossbeam_channel::{unbounded, Receiver};
use eframe::egui::{self, Frame, Margin};
use photoorg_core::export::{self, ExportKind, ExportSummary};
use photoorg_core::format;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

pub struct ExportJob {
    pub done: Arc<AtomicUsize>,
    pub total: Arc<AtomicUsize>,
    pub cancel: Arc<AtomicBool>,
    pub dir: PathBuf,
    rx: Receiver<Result<ExportSummary, String>>,
}

/// What a menu entry asks for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ExportAction {
    Run(ExportKind),
    /// Open the "작게 내보내기" dialog.
    Smaller,
}

/// Long-edge choices of the dialog (0 = keep the size).
pub const EDGES: [(u32, &str); 5] =
    [(1080, "1080px"), (2048, "2048px"), (3000, "3000px"), (4000, "4000px"), (0, "원래 크기")];
pub const QUALITIES: [(u8, &str); 3] = [(80, "보통"), (88, "높음"), (95, "최고")];

impl PhotoApp {
    pub fn export_action(&mut self, ids: Vec<i64>, action: ExportAction) {
        if ids.is_empty() {
            return;
        }
        match action {
            ExportAction::Run(kind) => self.export_assets(ids, kind),
            ExportAction::Smaller => self.export_dialog = Some(ids),
        }
    }

    /// Ask for a folder and export `ids` there in the background.
    pub fn export_assets(&mut self, ids: Vec<i64>, kind: ExportKind) {
        let Some(st) = &self.lib else { return };
        if ids.is_empty() {
            return;
        }
        if self.export_job.is_some() {
            self.toast("이전 내보내기가 아직 진행 중입니다", true);
            return;
        }
        let title = match kind {
            ExportKind::Originals => format!("{}장의 원본을 내보낼 폴더", ids.len()),
            ExportKind::JpegOnly => format!("{}장을 JPEG로 내보낼 폴더", ids.len()),
            ExportKind::Resized { .. } => format!("{}장을 작게 내보낼 폴더", ids.len()),
        };
        let Some(dir) = rfd::FileDialog::new().set_title(title).pick_folder() else { return };
        let lib = st.lib.clone();
        let (tx, rx) = unbounded();
        let job = ExportJob {
            done: Arc::new(AtomicUsize::new(0)),
            total: Arc::new(AtomicUsize::new(ids.len())),
            cancel: Arc::new(AtomicBool::new(false)),
            dir: dir.clone(),
            rx,
        };
        let (done, total, cancel) = (job.done.clone(), job.total.clone(), job.cancel.clone());
        std::thread::Builder::new()
            .name("export".into())
            .spawn(move || {
                let r = lib
                    .catalog()
                    .and_then(|c| {
                        export::export(&lib, &c, &ids, &dir, kind, &cancel, &|d, t| {
                            done.store(d, Ordering::Relaxed);
                            total.store(t, Ordering::Relaxed);
                        })
                    })
                    .map_err(|e| format!("{e:#}"));
                let _ = tx.send(r);
            })
            .expect("spawn export");
        self.export_job = Some(job);
    }

    pub fn cancel_export(&mut self) {
        if let Some(j) = &self.export_job {
            j.cancel.store(true, Ordering::Relaxed);
        }
    }

    /// Called every frame.
    pub fn poll_export(&mut self, ctx: &egui::Context) {
        let Some(job) = &self.export_job else { return };
        let Ok(r) = job.rx.try_recv() else {
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
            return;
        };
        let dir = job.dir.clone();
        self.export_job = None;
        match r {
            Ok(s) => {
                let mut msg = if s.cancelled {
                    format!("내보내기를 중지했습니다 ({}개 완료)", format::count(s.files))
                } else {
                    format!("파일 {}개를 내보냈습니다 ({})", format::count(s.files), format::bytes(s.bytes))
                };
                if s.from_raw > 0 {
                    msg.push_str(&format!(
                        "\nJPEG가 없던 {}장은 RAW 안의 JPEG로 만들었습니다",
                        format::count(s.from_raw)
                    ));
                }
                if !s.skipped.is_empty() {
                    let first = &s.skipped[0];
                    msg.push_str(&format!(
                        "\n{}개는 건너뜀 — {}: {}",
                        format::count(s.skipped.len()),
                        first.0,
                        first.1
                    ));
                }
                msg.push_str(&format!("\n{}", dir.display()));
                let error = s.files == 0 && !s.skipped.is_empty();
                self.toast(msg, error);
                if s.files > 0 && !s.cancelled {
                    self.reveal(&dir);
                }
            }
            Err(e) => self.toast(format!("내보내기 실패: {e}"), true),
        }
    }

    /// The "작게 내보내기" dialog.
    pub fn export_dialog_ui(&mut self, ui: &mut egui::Ui) {
        let Some(ids) = self.export_dialog.clone() else { return };
        let pal = self.pal;
        let ctx = ui.ctx().clone();
        let mut close = false;
        let mut go = false;
        let modal = egui::Modal::new(egui::Id::unique("export-dialog")).frame(
            Frame::new()
                .fill(pal.panel_bg)
                .corner_radius(14.0)
                .inner_margin(Margin::same(22))
                .stroke(egui::Stroke::new(1.0, pal.separator)),
        );
        let resp = modal.show(&ctx, |ui| {
            ui.set_width(460.0);
            ui.label(egui::RichText::new(format!("작게 내보내기 — {}장", format::count(ids.len()))).font(bold(18.0)));
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new(
                    "공유·메신저·웹에 올리기 좋게 크기와 용량을 줄인 JPEG를 새로 만듭니다. 원본은 그대로입니다.",
                )
                .font(font(12.0))
                .color(pal.text2),
            );
            ui.add_space(12.0);
            ui.label(egui::RichText::new("크기 (긴 변)").font(bold(12.5)));
            ui.horizontal(|ui| {
                let mut e = self.cfg.export_edge;
                if widgets::segmented(ui, &mut e, &EDGES, &pal) {
                    self.cfg.export_edge = e;
                }
            });
            ui.label(
                egui::RichText::new("1080 메신저·SNS  ·  2048 블로그·공유  ·  3000–4000 큰 화면·인화")
                    .font(font(11.0))
                    .color(pal.text3),
            );
            ui.add_space(10.0);
            ui.label(egui::RichText::new("화질").font(bold(12.5)));
            ui.horizontal(|ui| {
                let mut q = self.cfg.export_quality;
                if widgets::segmented(ui, &mut q, &QUALITIES, &pal) {
                    self.cfg.export_quality = q;
                }
            });
            ui.add_space(10.0);
            let estimate = if self.cfg.export_edge == 0 {
                "해상도는 그대로 두고 화질만 낮춥니다. 이미 더 작은 사진은 원본 화질 그대로 두고 위치 정보만 뺍니다"
                    .to_string()
            } else {
                let per = export::estimate_bytes(self.cfg.export_edge, self.cfg.export_quality);
                format!(
                    "장당 약 {} · 모두 약 {} (사진에 따라 다름)",
                    format::bytes(per),
                    format::bytes(per * ids.len() as u64)
                )
            };
            ui.label(egui::RichText::new(estimate).font(font(12.5)));
            ui.add_space(6.0);
            ui.label(
                egui::RichText::new(
                    "날짜 · 카메라 · 렌즈 · 노출 정보는 남기고 위치(GPS)는 넣지 않습니다. \
                     세로 사진은 바로 세워 저장합니다. RAW만 있는 사진은 RAW 안의 JPEG에서 만듭니다.",
                )
                .font(font(11.0))
                .color(pal.text3),
            );
            ui.add_space(16.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if widgets::primary_button(ui, "폴더 선택 후 내보내기…", &pal, true).clicked() {
                    go = true;
                }
                if widgets::secondary_button(ui, "취소", &pal).clicked() {
                    close = true;
                }
            });
        });
        if resp.should_close() {
            close = true;
        }
        if go {
            self.export_dialog = None;
            self.cfg.save();
            let kind = ExportKind::Resized { long_edge: self.cfg.export_edge, quality: self.cfg.export_quality };
            self.export_assets(ids, kind);
        } else if close {
            self.export_dialog = None;
        }
    }
}
