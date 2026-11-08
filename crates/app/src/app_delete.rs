//! 지우기: photos go to the system trash — never deleted outright — after a confirmation. The work
//! runs on the writer thread (after any pending sidecar writes); the photos then leave the
//! library, and what the photographer decided about them stays for the culling assistant
//! (`photoorg_core::trash`).

use crate::app::{PhotoApp, WriteJob};
use crate::theme::{bold, font};
use crate::widgets::{self, Icon};
use crossbeam_channel::{unbounded, Receiver};
use eframe::egui::{self, vec2, Frame, Margin, Rect, Sense, Stroke, Ui};
use photoorg_core::filter::CullFilter;
use photoorg_core::format;
use photoorg_core::trash::{self, TrashPlan, TrashReport};
use std::collections::HashSet;

pub struct DeleteDialog {
    pub ids: Vec<i64>,
    pub plan: TrashPlan,
}

#[derive(Default)]
pub struct DeleteState {
    pub dialog: Option<DeleteDialog>,
    job: Option<Receiver<Result<TrashReport, String>>>,
}

impl DeleteState {
    pub fn busy(&self) -> bool {
        self.job.is_some()
    }
}

impl PhotoApp {
    /// Ask to move these photos to the trash.
    pub fn ask_delete(&mut self, ids: Vec<i64>) {
        if ids.is_empty() || self.delete.busy() {
            return;
        }
        let Some(st) = &self.lib else { return };
        let plan =
            trash::plan(&st.lib, &st.catalog, &ids).unwrap_or(TrashPlan { photos: ids.len(), ..Default::default() });
        self.delete.dialog = Some(DeleteDialog { ids, plan });
    }

    fn start_delete(&mut self, ctx: &egui::Context, ids: Vec<i64>) {
        let Some(w) = &self.writer else { return };
        let (tx, rx) = unbounded();
        let ctx2 = ctx.clone();
        let _ = w.send(WriteJob::Run(Box::new(move |lib, cat| {
            let _ = tx.send(trash::trash_assets(lib, cat, &ids).map_err(|e| format!("{e:#}")));
            ctx2.request_repaint();
        })));
        self.delete.job = Some(rx);
    }

    /// Called every frame from `poll`.
    pub fn poll_delete(&mut self, ctx: &egui::Context) {
        let Some(rx) = &self.delete.job else { return };
        let Ok(result) = rx.try_recv() else { return };
        self.delete.job = None;
        match result {
            Ok(report) => self.apply_deleted(ctx, report),
            Err(e) => self.toast(format!("휴지통으로 옮기지 못했습니다: {e}"), true),
        }
    }

    fn apply_deleted(&mut self, ctx: &egui::Context, report: TrashReport) {
        let gone: HashSet<i64> = report.trashed.iter().copied().collect();
        // In the photo view, go on to the next photo that stays (or the one before).
        let mut next_view = None;
        if let Some(v) = &self.viewer {
            if gone.contains(&v.id) {
                if let (Some(&p), Some(st)) = (self.filtered_pos.get(&v.id), &self.lib) {
                    let id_at = |i: usize| st.index.assets[self.filtered[i]].id;
                    next_view = (p + 1..self.filtered.len())
                        .map(id_at)
                        .find(|id| !gone.contains(id))
                        .or_else(|| (0..p).rev().map(id_at).find(|id| !gone.contains(id)));
                }
                if next_view.is_none() {
                    self.viewer = None;
                }
            }
        }
        if let Some(st) = &mut self.lib {
            st.index.remove(&report.trashed);
            crate::app_cull::refresh_totals(st, self.cfg.cull_threshold);
        }
        self.sel.ids.retain(|id| !gone.contains(id));
        if self.sel.focus.is_some_and(|id| gone.contains(&id)) {
            self.sel.focus = None;
        }
        self.info = None;
        self.facets = None;
        self.refilter();
        if let Some(id) = next_view {
            self.open_viewer(id);
        }
        if !report.trashed.is_empty() {
            self.toast(
                format!(
                    "{}장을 휴지통으로 옮겼습니다 ({}). 휴지통에서 되살려 다시 가져올 수 있어요.",
                    format::count(report.trashed.len()),
                    format::bytes(report.bytes)
                ),
                false,
            );
        }
        if let Some((_, why)) = report.failed.first() {
            self.toast(format!("{}장은 옮기지 못했습니다: {why}", format::count(report.failed.len())), true);
        }
        // The library changed: score again (the decisions themselves are kept).
        self.labels_dirty_at = Some(ctx.input(|i| i.time));
    }

    pub fn delete_dialog_ui(&mut self, ui: &mut Ui) {
        let Some(dlg) = &self.delete.dialog else { return };
        let (n, files, bytes) = (dlg.plan.photos, dlg.plan.files, dlg.plan.bytes);
        let pal = self.pal;
        let ctx = ui.ctx().clone();
        let modal = egui::Modal::new(egui::Id::unique("delete-photos")).frame(
            Frame::new()
                .fill(pal.panel_bg)
                .corner_radius(14.0)
                .inner_margin(Margin::same(22))
                .stroke(Stroke::new(1.0, pal.separator)),
        );
        let (mut go, mut cancel) = (false, false);
        let resp = modal.show(&ctx, |ui| {
            ui.set_width(440.0);
            ui.label(
                egui::RichText::new(format!("사진 {}장을 휴지통으로 옮길까요?", format::count(n))).font(bold(18.0)),
            );
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new(format!(
                    "파일 {}개({})가 휴지통으로 가고 라이브러리에서 빠집니다 — RAW · JPEG · XMP와 작품 폴더의 \
                     복사본까지. 휴지통에서 되살린 뒤 다시 가져오면 돌아와요.",
                    format::count(files),
                    format::bytes(bytes)
                ))
                .font(font(13.0))
                .color(pal.text2),
            );
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new("거절 · 픽 · 별점은 셀렉 도우미의 학습에 그대로 남습니다.")
                    .font(font(12.0))
                    .color(pal.text3),
            );
            ui.add_space(16.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if widgets::danger_button(ui, "휴지통으로 옮기기", &pal).clicked() {
                    go = true;
                }
                if widgets::secondary_button(ui, "취소", &pal).clicked() {
                    cancel = true;
                }
            });
            if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                go = true;
            }
        });
        if go {
            if let Some(dlg) = self.delete.dialog.take() {
                self.start_delete(&ctx, dlg.ids);
            }
        } else if cancel || resp.should_close() {
            self.delete.dialog = None;
        }
    }

    /// Bar above the rejected photos: clear them out once culling is done.
    pub fn rejected_bar(&mut self, ui: &mut Ui) {
        let pal = self.pal;
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 40.0), Sense::hover());
        ui.painter().rect_filled(rect, 0.0, pal.danger.gamma_multiply(0.10));
        widgets::separator_h(ui, rect, &pal);
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(rect.shrink2(vec2(16.0, 6.0)))
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        let ui = &mut child;
        ui.spacing_mut().item_spacing.x = 8.0;
        widgets::paint_icon(
            ui.painter(),
            Rect::from_center_size(ui.cursor().min + vec2(8.0, rect.height() / 2.0 - 6.0), vec2(16.0, 16.0)),
            Icon::Reject,
            pal.danger,
        );
        ui.add_space(22.0);
        let Some(st) = &self.lib else { return };
        let ids: Vec<i64> = self
            .filtered
            .iter()
            .map(|&i| &st.index.assets[i])
            .filter(|a| CullFilter::Rejected.matches(a))
            .map(|a| a.id)
            .collect();
        ui.label(egui::RichText::new(format!("거절한 사진 {}장", format::count(ids.len()))).font(bold(12.5)));
        if rect.width() > 700.0 {
            ui.label(
                egui::RichText::new("· U 거절 해제 · Delete 휴지통으로 · 학습 기록은 남아요")
                    .font(font(11.5))
                    .color(pal.text3),
            );
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let busy = self.delete.busy();
            let label = if busy {
                "옮기는 중…".to_string()
            } else {
                format!("{}장 휴지통으로…", format::count(ids.len()))
            };
            if !ids.is_empty() && widgets::secondary_button(ui, &label, &pal).clicked() && !busy {
                self.ask_delete(ids);
            }
        });
    }
}
