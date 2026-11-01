//! Exporting photos: the export dialog (원본 전체 / JPEG만 / 작게, as files or ZIP) and the
//! background job with progress.

use crate::app::PhotoApp;
use crate::theme::{bold, font};
use crate::widgets;
use crossbeam_channel::{unbounded, Receiver};
use eframe::egui::{self, Frame, Margin};
use photoorg_core::export::{self, ExportKind, ExportOptions, ExportPlanInfo, ExportSummary, ZipOptions};
use photoorg_core::format;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

pub struct ExportJob {
    pub done: Arc<AtomicUsize>,
    pub total: Arc<AtomicUsize>,
    pub cancel: Arc<AtomicBool>,
    pub dir: PathBuf,
    zip: bool,
    rx: Receiver<Result<ExportSummary, String>>,
}

/// The format chosen in the dialog.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ExportMode {
    Originals,
    JpegOnly,
    #[default]
    Smaller,
}

pub struct ExportDialog {
    pub ids: Vec<i64>,
    pub info: ExportPlanInfo,
    pub mode: ExportMode,
    pub zip_name: String,
}

/// Long-edge choices (0 = keep the size).
pub const EDGES: [(u32, &str); 5] =
    [(1080, "1080px"), (2048, "2048px"), (3000, "3000px"), (4000, "4000px"), (0, "원래 크기")];
pub const QUALITIES: [(u8, &str); 3] = [(80, "보통"), (88, "높음"), (95, "최고")];
/// ZIP split sizes in MB (0 = do not split).
pub const SPLITS: [(u32, &str); 4] = [(0, "나누지 않음"), (25, "25 MB씩"), (100, "100 MB씩"), (1000, "1 GB씩")];

impl PhotoApp {
    /// Open the export dialog for `ids` with `mode` preselected.
    pub fn open_export(&mut self, ids: Vec<i64>, mode: ExportMode) {
        let Some(st) = &self.lib else { return };
        if ids.is_empty() {
            return;
        }
        let info = export::plan_info(&st.catalog, &ids).unwrap_or_default();
        let zip_name = match (info.first, info.last) {
            (Some(a), Some(b)) if a.date() != b.date() => {
                format!("사진 {}–{} ({}장)", a.format("%Y-%m-%d"), b.format("%m-%d"), ids.len())
            }
            (Some(a), _) => format!("사진 {} ({}장)", a.format("%Y-%m-%d"), ids.len()),
            _ => format!("사진 {}장", ids.len()),
        };
        self.export_dialog = Some(ExportDialog { ids, info, mode, zip_name });
    }

    fn export_options(&self, mode: ExportMode, zip_name: &str) -> ExportOptions {
        let kind = match mode {
            ExportMode::Originals => ExportKind::Originals,
            ExportMode::JpegOnly => ExportKind::JpegOnly,
            ExportMode::Smaller => {
                ExportKind::Resized { long_edge: self.cfg.export_edge, quality: self.cfg.export_quality }
            }
        };
        let zip = self.cfg.export_zip.then(|| ZipOptions {
            name: zip_name.trim().to_string(),
            part_bytes: self.cfg.export_split_mb as u64 * 1_000_000,
        });
        ExportOptions { kind, date_names: self.cfg.export_date_names, zip }
    }

    /// Ask for a folder and export `ids` there in the background.
    pub fn export_assets(&mut self, ids: Vec<i64>, opts: ExportOptions) {
        let Some(st) = &self.lib else { return };
        if ids.is_empty() {
            return;
        }
        if self.export_job.is_some() {
            self.toast("이전 내보내기가 아직 진행 중입니다", true);
            return;
        }
        let what = if opts.zip.is_some() { "ZIP 을 저장할 폴더" } else { "내보낼 폴더" };
        let Some(dir) = rfd::FileDialog::new().set_title(format!("{}장 — {what}", ids.len())).pick_folder() else {
            return;
        };
        let lib = st.lib.clone();
        let (tx, rx) = unbounded();
        let job = ExportJob {
            done: Arc::new(AtomicUsize::new(0)),
            total: Arc::new(AtomicUsize::new(ids.len())),
            cancel: Arc::new(AtomicBool::new(false)),
            dir: dir.clone(),
            zip: opts.zip.is_some(),
            rx,
        };
        let (done, total, cancel) = (job.done.clone(), job.total.clone(), job.cancel.clone());
        std::thread::Builder::new()
            .name("export".into())
            .spawn(move || {
                let r = lib
                    .catalog()
                    .and_then(|c| {
                        export::export(&lib, &c, &ids, &dir, &opts, &cancel, &|d, t| {
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
        let (dir, zipped) = (job.dir.clone(), job.zip);
        self.export_job = None;
        match r {
            Ok(s) => {
                let mut msg = if s.cancelled {
                    if zipped {
                        "내보내기를 중지했습니다 (만들던 ZIP 은 지웠습니다)".to_string()
                    } else {
                        format!("내보내기를 중지했습니다 ({}개 완료)", format::count(s.files))
                    }
                } else if s.archives.len() > 1 {
                    format!(
                        "사진 {}장을 ZIP {}개로 내보냈습니다 (모두 {})",
                        format::count(s.files),
                        s.archives.len(),
                        format::bytes(s.bytes)
                    )
                } else if s.archives.len() == 1 {
                    format!("사진 {}장을 ZIP 하나로 내보냈습니다 ({})", format::count(s.files), format::bytes(s.bytes))
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
                    match s.archives.first() {
                        Some(zip) => self.reveal(zip),
                        None => self.reveal(&dir),
                    }
                }
            }
            Err(e) => self.toast(format!("내보내기 실패: {e}"), true),
        }
    }

    /// The export dialog.
    pub fn export_dialog_ui(&mut self, ui: &mut egui::Ui) {
        let Some(d) = &self.export_dialog else { return };
        let (n, info, mut mode, mut zip_name) = (d.ids.len(), d.info.clone(), d.mode, d.zip_name.clone());
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
            ui.set_width(500.0);
            ui.spacing_mut().item_spacing.y = 6.0;
            ui.label(egui::RichText::new(format!("내보내기 — {}장", format::count(n))).font(bold(18.0)));
            if let (Some(a), Some(b)) = (info.first, info.last) {
                let range = if a.date() == b.date() {
                    format::date_medium(a.date())
                } else {
                    format!("{} – {}", format::date_medium(a.date()), format::date_medium(b.date()))
                };
                ui.label(egui::RichText::new(range).font(font(12.0)).color(pal.text2));
            }
            ui.add_space(8.0);

            // Format.
            ui.label(egui::RichText::new("형식").font(bold(12.5)));
            ui.horizontal(|ui| {
                widgets::segmented(
                    ui,
                    &mut mode,
                    &[
                        (ExportMode::Originals, "원본 전체"),
                        (ExportMode::JpegOnly, "JPEG만"),
                        (ExportMode::Smaller, "작게 (용량 줄이기)"),
                    ],
                    &pal,
                );
            });
            let hint = match mode {
                ExportMode::Originals => "RAW · JPEG · 동영상 · 별점(.xmp)을 그대로 — 보정 · 백업용".to_string(),
                ExportMode::JpegOnly => {
                    let mut t = "사진마다 JPEG 하나. RAW 는 내보내지 않습니다".to_string();
                    if info.raw_only > 0 {
                        t.push_str(&format!(" — RAW만 있는 {}장은 RAW 안의 JPEG로 만듭니다", info.raw_only));
                    }
                    t
                }
                ExportMode::Smaller => "공유 · 메신저 · 웹용으로 크기와 용량을 줄인 JPEG를 새로 만듭니다".to_string(),
            };
            ui.label(egui::RichText::new(hint).font(font(11.5)).color(pal.text2));

            if mode == ExportMode::Smaller {
                ui.add_space(6.0);
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
                ui.label(egui::RichText::new("화질").font(bold(12.5)));
                ui.horizontal(|ui| {
                    let mut q = self.cfg.export_quality;
                    if widgets::segmented(ui, &mut q, &QUALITIES, &pal) {
                        self.cfg.export_quality = q;
                    }
                });
            }

            // Expected size.
            let unconvertible = info.videos + info.heif_only;
            let photos_out = match mode {
                ExportMode::Originals => n,
                _ => n.saturating_sub(unconvertible),
            };
            let (total, approx) = match mode {
                ExportMode::Originals => (Some(info.originals_bytes), false),
                // RAW-only photos become a full-size JPEG of a few MB.
                ExportMode::JpegOnly => (Some(info.jpeg_bytes + info.raw_only as u64 * 6_000_000), info.raw_only > 0),
                ExportMode::Smaller if self.cfg.export_edge == 0 => (None, true),
                ExportMode::Smaller => (
                    Some(export::estimate_bytes(self.cfg.export_edge, self.cfg.export_quality) * photos_out as u64),
                    true,
                ),
            };
            ui.add_space(4.0);
            let size_line = match total {
                Some(t) => format!(
                    "{}{} · {}장{}",
                    if approx { "약 " } else { "" },
                    format::bytes(t),
                    format::count(photos_out),
                    if approx { " (사진에 따라 다름)" } else { "" }
                ),
                None => {
                    "해상도는 그대로, 화질만 낮춥니다. 이미 더 작은 사진은 원본 화질 그대로 두고 위치 정보만 뺍니다"
                        .to_string()
                }
            };
            ui.label(egui::RichText::new(size_line).font(font(12.5)));
            if mode != ExportMode::Originals && unconvertible > 0 {
                let mut parts = Vec::new();
                if info.videos > 0 {
                    parts.push(format!("동영상 {}개", info.videos));
                }
                if info.heif_only > 0 {
                    parts.push(format!("HEIF {}장", info.heif_only));
                }
                ui.label(
                    egui::RichText::new(format!("{}는 JPEG로 바꿀 수 없어 빠집니다", parts.join(" · ")))
                        .font(font(11.5))
                        .color(pal.warning),
                );
            }

            // Packaging.
            ui.add_space(8.0);
            ui.label(egui::RichText::new("보내기 좋게").font(bold(12.5)));
            ui.horizontal(|ui| {
                let mut z = self.cfg.export_zip;
                if widgets::segmented(ui, &mut z, &[(false, "폴더에 파일로"), (true, "ZIP 으로 묶기")], &pal)
                {
                    self.cfg.export_zip = z;
                }
            });
            if self.cfg.export_zip {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("이름").font(font(12.0)).color(pal.text2));
                    ui.add(egui::TextEdit::singleline(&mut zip_name).desired_width(300.0));
                    ui.label(egui::RichText::new(".zip").font(font(12.0)).color(pal.text2));
                });
                ui.horizontal(|ui| {
                    let mut s = self.cfg.export_split_mb;
                    if widgets::segmented(ui, &mut s, &SPLITS, &pal) {
                        self.cfg.export_split_mb = s;
                    }
                });
                let parts = match (total, self.cfg.export_split_mb) {
                    (Some(t), mb) if mb > 0 => Some(t.div_ceil(mb as u64 * 1_000_000).max(1)),
                    _ => None,
                };
                let note = match parts {
                    Some(k) if k > 1 => {
                        format!("약 {k}개의 ZIP 으로 나눕니다. 각각 따로 열리므로 메일 · 메신저에 하나씩 보내면 됩니다")
                    }
                    _ => "메일 첨부는 보통 25 MB 이하로 나누면 편합니다. 나눈 ZIP 은 각각 따로 열립니다".to_string(),
                };
                ui.label(egui::RichText::new(note).font(font(11.0)).color(pal.text3));
            }
            ui.add_space(2.0);
            widgets::switch(
                ui,
                &mut self.cfg.export_date_names,
                "파일 이름 앞에 촬영 일시 붙이기 (2026-10-03_091500_IMG_0001.jpg)",
                &pal,
            );
            ui.label(
                egui::RichText::new(
                    "받는 쪽에서도 찍은 순서대로 보이고, 여러 카메라의 같은 번호도 겹치지 않습니다. \
                     켜지 않아도 같은 이름은 _1, _2 로 구분되고, 사진은 항상 찍은 순서대로 내보냅니다.",
                )
                .font(font(11.0))
                .color(pal.text3),
            );
            if mode != ExportMode::Originals {
                ui.label(
                    egui::RichText::new(
                        "새로 만드는 JPEG 에는 날짜 · 카메라 · 렌즈 · 노출 정보를 남기고 위치(GPS)는 넣지 않습니다.",
                    )
                    .font(font(11.0))
                    .color(pal.text3),
                );
            }

            ui.add_space(14.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let enabled = photos_out > 0 && (!self.cfg.export_zip || !zip_name.trim().is_empty());
                if widgets::primary_button(ui, "폴더 선택 후 내보내기…", &pal, enabled).clicked() && enabled
                {
                    go = true;
                }
                if widgets::secondary_button(ui, "취소", &pal).clicked() {
                    close = true;
                }
            });
        });
        if let Some(d) = &mut self.export_dialog {
            d.mode = mode;
            d.zip_name = zip_name.clone();
        }
        self.cfg.export_mode = mode;
        if resp.should_close() {
            close = true;
        }
        if go {
            let ids = self.export_dialog.take().map(|d| d.ids).unwrap_or_default();
            self.cfg.save();
            let opts = self.export_options(mode, &zip_name);
            self.export_assets(ids, opts);
        } else if close {
            self.export_dialog = None;
            self.cfg.save();
        }
    }
}
