//! 작품 폴더: gathering the photos of a project into one folder of the library (to edit them in
//! Lightroom and the like) — the dialog, the bar above a project's photos and the background job.
//! The file work itself is in [`photoorg_core::works`].

use crate::app::{PhotoApp, SideSel, WriteJob, WriterTask};
use crate::theme::{bold, font};
use crate::widgets::{self, Icon};
use crossbeam_channel::{unbounded, Receiver};
use eframe::egui::{self, vec2, Frame, Margin, Rect, Sense, Ui};
use photoorg_core::format;
use photoorg_core::works::{self, GatherMode, GatherOptions, GatherPlan, GatherSummary};
use photoorg_core::{Catalog, Library};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorksKind {
    Gather(GatherMode),
    Restore,
    /// The project (or a theme) was renamed: its folder follows.
    Rename,
}

pub struct WorksJob {
    pub kind: WorksKind,
    pub project: u32,
    pub done: Arc<AtomicUsize>,
    pub total: Arc<AtomicUsize>,
    pub cancel: Arc<AtomicBool>,
    rx: Receiver<Result<GatherSummary, String>>,
}

impl WorksJob {
    pub fn label(&self) -> &'static str {
        match self.kind {
            WorksKind::Gather(GatherMode::Move) => "작품 폴더로 옮기는 중",
            WorksKind::Gather(GatherMode::Copy) => "작품 폴더에 복사하는 중",
            WorksKind::Restore => "날짜 폴더로 되돌리는 중",
            WorksKind::Rename => "작품 폴더 이름 바꾸는 중",
        }
    }
}

pub enum WorksDialog {
    Gather {
        project: u32,
        plan: GatherPlan,
        key: (GatherMode, bool),
    },
    /// Move back to the dated folders: every photo moved into the folder, or only `only`.
    Restore {
        project: u32,
        only: Option<Vec<i64>>,
        photos: usize,
    },
}

type Work =
    dyn FnOnce(&Library, &mut Catalog, &AtomicBool, &dyn Fn(usize, usize)) -> anyhow::Result<GatherSummary> + Send;

impl PhotoApp {
    fn works_options(&self) -> GatherOptions {
        GatherOptions {
            mode: self.cfg.works_mode,
            by_theme: self.cfg.works_by_theme,
            write_xmp: self.xmp_enabled.load(Ordering::Relaxed),
        }
    }

    fn works_plan(&self, project: u32) -> Option<GatherPlan> {
        let st = self.lib.as_ref()?;
        match works::plan(&st.lib, &st.catalog, project, &self.works_options()) {
            Ok(p) => Some(p),
            Err(e) => {
                log::warn!("작품 폴더 {project}: {e:#}");
                None
            }
        }
    }

    /// Folder of a project (library-relative), from its current name.
    pub fn works_folder(&self, project: u32) -> Option<String> {
        let st = self.lib.as_ref()?;
        st.index.project(project).map(|p| works::project_folder(&p.name))
    }

    /// Subfolder of a theme in its project's folder (library-relative).
    pub fn works_theme_folder(&self, tag: u32) -> Option<String> {
        let st = self.lib.as_ref()?;
        let t = st.index.tags.get(&tag)?;
        Some(format!("{}/{}", self.works_folder(t.project?)?, works::theme_folder(t.short_name())))
    }

    pub fn works_folder_exists(&self, project: u32) -> bool {
        match (self.lib.as_ref(), self.works_folder(project)) {
            (Some(st), Some(f)) => st.lib.abs(&f).is_dir(),
            _ => false,
        }
    }

    /// Status of the project shown in the bar (computed when needed, cleared when photos or
    /// projects change).
    pub fn works_status(&mut self, project: u32) -> Option<&GatherPlan> {
        if self.works_status.as_ref().is_none_or(|(p, _)| *p != project) {
            let plan = self.works_plan(project)?;
            self.works_status = Some((project, plan));
        }
        self.works_status.as_ref().map(|(_, p)| p)
    }

    pub fn open_works_gather(&mut self, project: u32) {
        if self.works_job.is_some() {
            self.toast("작품 폴더 작업이 아직 진행 중입니다", true);
            return;
        }
        if let Some(plan) = self.works_plan(project) {
            let key = (self.cfg.works_mode, self.cfg.works_by_theme);
            self.works_dialog = Some(WorksDialog::Gather { project, plan, key });
        }
    }

    /// Ask before moving the photos of a project folder back to their dated folders.
    pub fn open_works_restore(&mut self, project: u32, only: Option<Vec<i64>>) {
        if self.works_job.is_some() {
            self.toast("작품 폴더 작업이 아직 진행 중입니다", true);
            return;
        }
        let (Some(st), Some(folder)) = (self.lib.as_ref(), self.works_folder(project)) else { return };
        let mut ids: Vec<i64> = st
            .catalog
            .files_under(&folder)
            .unwrap_or_default()
            .into_iter()
            .filter(|f| f.home_rel.is_some() && only.as_ref().is_none_or(|o| o.contains(&f.asset_id)))
            .map(|f| f.asset_id)
            .collect();
        ids.dedup();
        if ids.is_empty() {
            self.toast("작품 폴더로 옮긴 사진이 없습니다 (복사본은 그대로 둡니다)", false);
            return;
        }
        self.works_dialog = Some(WorksDialog::Restore { project, only, photos: ids.len() });
    }

    /// Run `work` in the background: on the catalog writer when it moves files (so sidecar writes
    /// stay in order with it), on a thread of its own when it copies (started by the writer, so
    /// the sidecar writes queued before it are done first).
    fn spawn_works(&mut self, kind: WorksKind, project: u32, ctx: Option<&egui::Context>, work: Box<Work>) {
        if self.lib.is_none() {
            return;
        }
        let (tx, rx) = unbounded();
        let job = WorksJob {
            kind,
            project,
            done: Arc::new(AtomicUsize::new(0)),
            total: Arc::new(AtomicUsize::new(0)),
            cancel: Arc::new(AtomicBool::new(false)),
            rx,
        };
        let (done, total, cancel) = (job.done.clone(), job.total.clone(), job.cancel.clone());
        let ctx = ctx.cloned();
        let run = move |lib: &Library, cat: &mut Catalog| {
            let progress = |d: usize, t: usize| {
                done.store(d, Ordering::Relaxed);
                total.store(t, Ordering::Relaxed);
            };
            let r = work(lib, cat, &cancel, &progress).map_err(|e| format!("{e:#}"));
            let _ = tx.send(r);
            if let Some(ctx) = ctx {
                ctx.request_repaint();
            }
        };
        let task: WriterTask = if kind == WorksKind::Gather(GatherMode::Copy) {
            Box::new(move |lib: &Library, _: &mut Catalog| {
                let lib = lib.clone();
                std::thread::Builder::new()
                    .name("works-copy".into())
                    .spawn(move || match lib.catalog() {
                        Ok(mut cat) => run(&lib, &mut cat),
                        Err(e) => log::error!("catalog: {e:#}"),
                    })
                    .expect("spawn works");
            })
        } else {
            Box::new(run)
        };
        let Some(w) = &self.writer else { return };
        if w.send(WriteJob::Run(task)).is_err() {
            self.toast("작업을 시작할 수 없습니다", true);
            return;
        }
        self.works_job = Some(job);
    }

    fn start_works_gather(&mut self, ctx: &egui::Context, project: u32, plan: GatherPlan) {
        let mut opts = self.works_options();
        if opts.mode == GatherMode::Copy {
            // Copies are made off the writer: refresh their sidecars on it first.
            if let Some(w) = &self.writer {
                let _ = w.send(WriteJob::Xmp(plan.items.iter().map(|i| i.asset_id).collect()));
            }
            opts.write_xmp = false;
        }
        let work: Box<Work> =
            Box::new(move |lib, cat, cancel, progress| works::gather(lib, cat, &plan, &opts, cancel, progress));
        self.spawn_works(WorksKind::Gather(opts.mode), project, Some(ctx), work);
    }

    fn start_works_restore(&mut self, ctx: &egui::Context, project: u32, only: Option<Vec<i64>>) {
        let Some(folder) = self.works_folder(project) else { return };
        let work: Box<Work> = Box::new(move |lib, cat, cancel, progress| {
            works::restore(lib, cat, &folder, only.as_deref(), cancel, progress)
        });
        self.spawn_works(WorksKind::Restore, project, Some(ctx), work);
    }

    pub fn cancel_works(&mut self) {
        if let Some(j) = &self.works_job {
            j.cancel.store(true, Ordering::Relaxed);
        }
    }

    /// Called every frame.
    pub fn poll_works(&mut self, ctx: &egui::Context) {
        let Some(job) = &self.works_job else { return };
        let Ok(r) = job.rx.try_recv() else {
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
            return;
        };
        let kind = job.kind;
        self.works_job = None;
        self.works_status = None;
        self.info = None;
        match r {
            Ok(s) => {
                let mut msg = match kind {
                    _ if s.cancelled => format!("중지했습니다 ({}장 완료)", format::count(s.moved + s.copied)),
                    WorksKind::Gather(_) if s.moved + s.copied == 0 => "모을 사진이 없었습니다".to_string(),
                    WorksKind::Gather(_) => {
                        let mut parts = Vec::new();
                        if s.moved > 0 {
                            parts.push(format!("{}장을 옮겼습니다", format::count(s.moved)));
                        }
                        if s.copied > 0 {
                            parts.push(format!("{}장을 복사했습니다", format::count(s.copied)));
                        }
                        format!("작품 폴더로 사진 {} (파일 {}개, {})", parts.join(", "), s.files, format::bytes(s.bytes))
                    }
                    WorksKind::Restore => format!("사진 {}장을 날짜 폴더로 되돌렸습니다", format::count(s.moved)),
                    WorksKind::Rename if s.moved > 0 => format!(
                        "작품 폴더 이름도 바꿨습니다: {}\n라이트룸에 이미 가져왔다면 라이트룸에서 폴더 위치를 다시 지정하세요",
                        s.folder.display()
                    ),
                    WorksKind::Rename => String::new(),
                };
                if kind == WorksKind::Rename {
                    if let Some((name, why)) = s.failed.first() {
                        self.toast(format!("작품 폴더 이름은 그대로 두었습니다 — {name}: {why}"), true);
                    } else if !msg.is_empty() {
                        self.toast(msg, false);
                    }
                    return;
                }
                if let Some((name, why)) = s.failed.first() {
                    msg.push_str(&format!("\n{}장은 옮기지 못했습니다 — {name}: {why}", format::count(s.failed.len())));
                }
                if matches!(kind, WorksKind::Gather(_)) {
                    msg.push_str(&format!("\n{}", s.folder.display()));
                }
                let error = !s.failed.is_empty() && s.moved + s.copied == 0;
                self.toast(msg, error);
                if matches!(kind, WorksKind::Gather(_)) && s.moved + s.copied > 0 && !s.cancelled {
                    self.open_folder(&s.folder);
                }
            }
            Err(e) => self.toast(format!("작품 폴더 작업 실패: {e}"), true),
        }
    }

    /// Open a folder in the OS file manager.
    pub fn open_folder(&mut self, dir: &std::path::Path) {
        let cmd = if cfg!(target_os = "macos") {
            "open"
        } else if cfg!(target_os = "windows") {
            "explorer"
        } else {
            "xdg-open"
        };
        if let Err(e) = std::process::Command::new(cmd).arg(dir).spawn() {
            self.toast(format!("폴더를 열 수 없습니다: {e}"), true);
        }
    }

    pub fn open_works_folder(&mut self, project: u32) {
        let (Some(st), Some(f)) = (self.lib.as_ref(), self.works_folder(project)) else { return };
        let dir = st.lib.abs(&f);
        if dir.is_dir() {
            self.open_folder(&dir);
        } else {
            self.toast("아직 작품 폴더가 없습니다 — '작품 폴더로 모으기'를 먼저 하세요", false);
        }
    }

    /// After a project or theme was renamed: rename its folder too, if there is one. Queued on the
    /// catalog writer before the sidecar rewrites the rename causes, so those go to the new place.
    pub fn rename_works_folder(&mut self, project: u32, old: &str, new: &str) {
        let Some(st) = &self.lib else { return };
        if old == new || !st.lib.abs(old).is_dir() {
            return;
        }
        if self.works_job.is_some() {
            self.toast("작품 폴더 작업 중이라 폴더 이름은 그대로 두었습니다", true);
            return;
        }
        let (old, new) = (old.to_string(), new.to_string());
        let work: Box<Work> = Box::new(move |lib, cat, _, _| {
            let renamed = works::rename_folder(lib, cat, &old, &new)?;
            let failed = if renamed {
                Vec::new()
            } else {
                vec![(new.clone(), "같은 이름의 폴더가 이미 있습니다".into())]
            };
            Ok(GatherSummary { moved: renamed as usize, folder: lib.abs(&new), failed, ..Default::default() })
        });
        self.spawn_works(WorksKind::Rename, project, None, work);
    }

    /// The project whose bar is shown above the photos (its own view or one of its themes).
    pub fn works_bar_project(&self) -> Option<u32> {
        match self.side {
            SideSel::Project(p) => Some(p),
            SideSel::Theme(t) => self.lib.as_ref()?.index.tags.get(&t)?.project,
            _ => None,
        }
    }

    /// Bar above a project's photos: where its 작품 폴더 stands, with the actions.
    pub fn works_bar(&mut self, ui: &mut Ui, project: u32) {
        let pal = self.pal;
        let running = self
            .works_job
            .as_ref()
            .filter(|j| j.project == project)
            .map(|j| (j.label(), j.done.load(Ordering::Relaxed), j.total.load(Ordering::Relaxed)));
        let exists = self.works_folder_exists(project);
        let Some(plan) = self.works_status(project).cloned() else { return };
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 40.0), Sense::hover());
        ui.painter().rect_filled(rect, 0.0, pal.accent.gamma_multiply(0.08));
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
            Icon::Folder,
            pal.accent,
        );
        ui.add_space(22.0);
        ui.label(egui::RichText::new("작품 폴더").font(bold(12.5))).on_hover_text(format!(
            "라이브러리의 {} — 라이트룸 등에서 이 폴더를 그대로 가져와 편집하세요",
            plan.folder
        ));
        let pending = plan.items.len();
        let status = match (running, plan.gathered(), pending) {
            (Some((label, d, t)), _, _) => format!("{label} {}/{}", format::count(d), format::count(t)),
            (None, 0, 0) => "아직 사진이 없습니다".to_string(),
            (None, 0, p) => format!("아직 모으지 않았습니다 · 사진 {}장", format::count(p)),
            (None, g, 0) => format!("모두 모았습니다 · {}장", format::count(g)),
            (None, g, p) => format!("{}장 모음 · 새로 모을 사진 {}장", format::count(g), format::count(p)),
        };
        ui.label(egui::RichText::new(status).font(font(12.0)).color(pal.text2));
        let mut act: Option<u8> = None;
        if !plan.strays.is_empty() && running.is_none() {
            let r = ui.add(
                egui::Label::new(
                    egui::RichText::new(format!("· 작품에서 빠진 {}장이 폴더에 있음", plan.strays.len()))
                        .font(font(12.0))
                        .color(pal.warning),
                )
                .sense(Sense::click()),
            );
            if r.on_hover_text("누르면 이 사진들을 원래 날짜 폴더로 되돌립니다").clicked() {
                act = Some(3);
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if running.is_some() {
                return;
            }
            let label = if plan.gathered() == 0 { "작품 폴더로 모으기…" } else { "새 사진 모으기…" };
            if pending > 0 && widgets::primary_button(ui, label, &pal, true).clicked() {
                act = Some(1);
            }
            if exists && widgets::secondary_button(ui, "폴더 열기", &pal).clicked() {
                act = Some(2);
            }
        });
        match act {
            Some(1) => self.open_works_gather(project),
            Some(2) => self.open_works_folder(project),
            Some(3) => self.open_works_restore(project, Some(plan.strays.clone())),
            _ => {}
        }
    }

    /// The gather / restore dialogs.
    pub fn works_dialog_ui(&mut self, ui: &mut egui::Ui) {
        let Some(dlg) = self.works_dialog.take() else { return };
        let pal = self.pal;
        let ctx = ui.ctx().clone();
        let (mut keep, mut go) = (true, false);
        // One id per kind of dialog: a modal keeps the size it had last time.
        let id = match &dlg {
            WorksDialog::Gather { .. } => "works-gather",
            WorksDialog::Restore { .. } => "works-restore",
        };
        let modal = egui::Modal::new(egui::Id::unique(id)).frame(
            Frame::new()
                .fill(pal.panel_bg)
                .corner_radius(14.0)
                .inner_margin(Margin::same(22))
                .stroke(egui::Stroke::new(1.0, pal.separator)),
        );
        let project = match &dlg {
            WorksDialog::Gather { project, .. } | WorksDialog::Restore { project, .. } => *project,
        };
        let name =
            self.lib.as_ref().and_then(|st| st.index.project(project).map(|p| p.name.clone())).unwrap_or_default();
        let folder_abs = match (self.lib.as_ref(), self.works_folder(project)) {
            (Some(st), Some(f)) => st.lib.abs(&f).display().to_string(),
            _ => String::new(),
        };
        let themes: Vec<String> = self
            .lib
            .as_ref()
            .map(|st| st.index.themes_of(project).iter().map(|t| t.short_name().to_string()).collect())
            .unwrap_or_default();
        let exists = self.works_folder_exists(project);
        let mut open = false;
        let resp = modal.show(&ctx, |ui| {
            ui.set_width(500.0);
            ui.spacing_mut().item_spacing.y = 6.0;
            match &dlg {
                WorksDialog::Gather { plan, .. } => {
                    ui.label(egui::RichText::new(format!("작품 폴더로 모으기 — {name}")).font(bold(18.0)));
                    ui.label(egui::RichText::new(&folder_abs).font(font(11.5)).color(pal.text2));
                    ui.label(
                        egui::RichText::new(
                            "작품의 사진을 이 폴더에 모읍니다. 라이트룸 · 캡처원 등에서 폴더를 그대로 가져와 편집하세요. \
                             별점과 테마는 XMP(키워드 '작품|테마')로 함께 갑니다.",
                        )
                        .font(font(12.0)),
                    );
                    ui.add_space(6.0);
                    ui.label(egui::RichText::new("방식").font(bold(12.5)));
                    ui.horizontal(|ui| {
                        let mut m = self.cfg.works_mode;
                        if widgets::segmented(ui, &mut m, &[(GatherMode::Move, "옮기기"), (GatherMode::Copy, "복사")], &pal) {
                            self.cfg.works_mode = m;
                        }
                    });
                    let hint = match self.cfg.works_mode {
                        GatherMode::Move => "날짜 폴더에서 작품 폴더로 옮깁니다. 사진은 한 곳에만 있어 용량이 늘지 않고, \
                                             이 앱에서는 날짜별로 그대로 보입니다. 언제든 날짜 폴더로 되돌릴 수 있습니다."
                            .to_string(),
                        GatherMode::Copy => format!(
                            "날짜 폴더의 원본은 보관용으로 그대로 두고 작품 폴더에 복사본을 만듭니다 (용량 {} 더 필요).",
                            format::bytes(plan.bytes)
                        ),
                    };
                    ui.label(egui::RichText::new(hint).font(font(11.5)).color(pal.text2));
                    ui.add_space(4.0);
                    let preview: Vec<String> = themes.iter().take(3).map(|t| format!("{t}/")).collect();
                    let more = if themes.len() > 3 { " …" } else { "" };
                    widgets::switch(
                        ui,
                        &mut self.cfg.works_by_theme,
                        &format!("테마별 하위 폴더 ({}{more})", preview.join(" ")),
                        &pal,
                    );
                    ui.label(
                        egui::RichText::new(
                            "테마가 여러 개인 사진은 첫 번째 테마 폴더에만 들어갑니다 (모든 테마는 키워드로 남음). \
                             이미 모은 사진은 테마가 바뀌어도 그 자리에 둡니다.",
                        )
                        .font(font(11.0))
                        .color(pal.text3),
                    );
                    ui.add_space(8.0);
                    let n = plan.items.len();
                    let verb = if self.cfg.works_mode == GatherMode::Move { "옮길" } else { "복사할" };
                    let line = if n == 0 {
                        "새로 모을 사진이 없습니다".to_string()
                    } else {
                        format!("{verb} 사진 {}장 · {}", format::count(n), format::bytes(plan.bytes))
                    };
                    ui.label(egui::RichText::new(line).font(bold(13.0)));
                    if plan.gathered() > 0 {
                        ui.label(
                            egui::RichText::new(format!("이미 모은 {}장은 그대로 둡니다", format::count(plan.gathered())))
                                .font(font(11.5))
                                .color(pal.text2),
                        );
                    }
                    if plan.elsewhere > 0 && self.cfg.works_mode == GatherMode::Move {
                        ui.label(
                            egui::RichText::new(format!(
                                "다른 작품 폴더로 이미 옮긴 {}장은 옮기지 않고 복사합니다",
                                format::count(plan.elsewhere)
                            ))
                            .font(font(11.5))
                            .color(pal.text2),
                        );
                    }
                    ui.add_space(12.0);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let label = format!(
                            "{}장 {}",
                            format::count(n),
                            if self.cfg.works_mode == GatherMode::Move { "옮기기" } else { "복사" }
                        );
                        if widgets::primary_button(ui, &label, &pal, n > 0).clicked() && n > 0 {
                            go = true;
                        }
                        if widgets::secondary_button(ui, "취소", &pal).clicked() {
                            keep = false;
                        }
                        if exists && widgets::secondary_button(ui, "폴더 열기", &pal).clicked() {
                            open = true;
                        }
                    });
                }
                WorksDialog::Restore { only, photos, .. } => {
                    let what = if only.is_some() { "작품에서 빠진 사진" } else { "작품 폴더로 옮긴 사진" };
                    ui.label(egui::RichText::new(format!("날짜 폴더로 되돌리기 — {name}")).font(bold(18.0)));
                    ui.label(
                        egui::RichText::new(format!(
                            "{what} {}장을 원래 날짜 폴더로 되돌립니다. 복사본과 다른 프로그램이 만든 파일은 그대로 둡니다.",
                            format::count(*photos)
                        ))
                        .font(font(12.5)),
                    );
                    ui.label(
                        egui::RichText::new("라이트룸에 이 폴더를 가져왔다면, 라이트룸에서는 이 사진들의 위치를 다시 찾아야 합니다.")
                            .font(font(11.5))
                            .color(pal.warning),
                    );
                    ui.add_space(12.0);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if widgets::primary_button(ui, "되돌리기", &pal, true).clicked() {
                            go = true;
                        }
                        if widgets::secondary_button(ui, "취소", &pal).clicked() {
                            keep = false;
                        }
                    });
                }
            }
        });
        if resp.should_close() {
            keep = false;
        }
        if open {
            self.open_works_folder(project);
        }
        if go {
            self.cfg.save();
            match dlg {
                WorksDialog::Gather { project, .. } => {
                    // The options may have changed since the plan was made.
                    if let Some(plan) = self.works_plan(project) {
                        self.start_works_gather(&ctx, project, plan);
                    }
                }
                WorksDialog::Restore { project, only, .. } => self.start_works_restore(&ctx, project, only),
            }
            return;
        }
        if !keep {
            self.cfg.save();
            return;
        }
        // Options changed: plan again.
        let dlg = match dlg {
            WorksDialog::Gather { project, plan, key } if key != (self.cfg.works_mode, self.cfg.works_by_theme) => {
                let key = (self.cfg.works_mode, self.cfg.works_by_theme);
                WorksDialog::Gather { project, plan: self.works_plan(project).unwrap_or(plan), key }
            }
            d => d,
        };
        self.works_dialog = Some(dlg);
    }
}
