//! Projects (작품) and their themes: creating and editing them, the active project, toggling
//! themes on photos (chips and ⇧1–9) and exporting a project by theme.

use crate::app::{Hud, PhotoApp, SideSel, WriteJob};
use crate::app_export::ExportMode;
use crate::theme::{bold, font};
use crate::widgets;
use eframe::egui::{self, Frame, Margin};
use photoorg_core::filter::ProjectFilter;
use photoorg_core::format;

/// Meta key (in the library's catalog) of the project being worked on.
const ACTIVE_KEY: &str = "active_project";

/// Something chosen in the sidebar's project section.
#[derive(Clone, Copy, Debug)]
pub enum ProjectAction {
    New,
    AddThemes(u32),
    Rename(u32),
    Export(u32),
    Delete(u32),
    RenameTheme(u32),
    MoveTheme(u32, i32),
    DeleteTheme(u32),
    MinRating(u8),
}

pub enum ProjectDialog {
    New { name: String, themes: String },
    Rename { project: u32, text: String },
    AddThemes { project: u32, text: String },
    RenameTheme { tag: u32, text: String },
    DeleteProject { project: u32 },
    DeleteTheme { tag: u32 },
}

/// "새벽, 파도\n빛" -> ["새벽", "파도", "빛"]
pub fn parse_themes(text: &str) -> Vec<String> {
    text.split(['\n', ',', '·']).map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).collect()
}

impl PhotoApp {
    /// Restore the active project of the opened library.
    pub fn load_active_project(&mut self) {
        let Some(st) = &self.lib else { return };
        let saved = st.catalog.meta_get(ACTIVE_KEY).ok().flatten().and_then(|v| v.parse::<u32>().ok());
        self.active_project =
            saved.filter(|p| st.index.project(*p).is_some()).or_else(|| st.index.projects.first().map(|p| p.id));
    }

    pub fn set_active_project(&mut self, project: Option<u32>) {
        self.active_project = project;
        if let Some(st) = &self.lib {
            let _ = st.catalog.meta_set(ACTIVE_KEY, project.map(|p| p.to_string()).as_deref());
        }
    }

    /// Re-read projects and tags after a structural change; `affected` photos get new sidecars.
    pub fn reload_projects(&mut self, affected: Vec<i64>) {
        let Some(st) = &mut self.lib else { return };
        if let (Ok(tags), Ok(projects)) = (st.catalog.tags(), st.catalog.projects()) {
            let known: std::collections::HashSet<u32> = tags.iter().map(|t| t.id).collect();
            st.index.tags = tags.into_iter().map(|t| (t.id, t)).collect();
            st.index.projects = projects;
            // Deleted themes disappear from the photos.
            for id in &affected {
                if let Some(a) = st.index.get_mut(*id) {
                    a.tags.retain(|t| known.contains(t));
                }
            }
        }
        crate::app_cull::refresh_totals(st, self.cfg.cull_threshold);
        if !affected.is_empty() {
            if let Some(w) = &self.writer {
                let _ = w.send(WriteJob::Xmp(affected));
            }
        }
        if self.active_project.is_some_and(|p| self.lib.as_ref().is_some_and(|st| st.index.project(p).is_none())) {
            let first = self.lib.as_ref().and_then(|st| st.index.projects.first().map(|p| p.id));
            self.set_active_project(first);
        }
        // The current view may show a deleted project / theme.
        let gone = match &self.side {
            SideSel::Project(p) | SideSel::ProjectTodo(p) => {
                self.lib.as_ref().is_some_and(|st| st.index.project(*p).is_none())
            }
            SideSel::Theme(t) => self.lib.as_ref().is_some_and(|st| !st.index.tags.contains_key(t)),
            _ => false,
        };
        self.facets = None;
        self.info = None;
        if gone {
            self.select_side(SideSel::All);
        } else if self.filter.project.is_some() || !self.filter.tags.is_empty() {
            self.refilter();
        }
    }

    pub fn create_project(&mut self, name: &str, themes: &str) -> bool {
        let Some(st) = &mut self.lib else { return false };
        match st.catalog.create_project(name, &parse_themes(themes)) {
            Ok(p) => {
                self.reload_projects(Vec::new());
                self.set_active_project(Some(p.id));
                self.select_side(SideSel::ProjectTodo(p.id));
                self.toast(
                    format!(
                        "'{}' 작품을 만들었습니다. 별점 준 사진을 고르고 테마를 누르거나 ⇧1–9 로 지정하세요",
                        p.name
                    ),
                    false,
                );
                true
            }
            Err(e) => {
                self.toast(format!("{e:#}"), true);
                false
            }
        }
    }

    /// Theme `tag` on/off for `ids`: on for all unless all already have it.
    pub fn toggle_theme(&mut self, ctx: &egui::Context, ids: Vec<i64>, tag: u32) {
        if ids.is_empty() {
            return;
        }
        let Some(st) = &mut self.lib else { return };
        let all = ids.iter().all(|id| st.index.get(*id).is_some_and(|a| a.tags.contains(&tag)));
        let r = if all { st.catalog.remove_tag(&ids, tag) } else { st.catalog.assign_tag(&ids, tag) };
        if let Err(e) = r {
            self.toast(format!("저장 실패: {e:#}"), true);
            return;
        }
        for id in &ids {
            if let Some(a) = st.index.get_mut(*id) {
                if all {
                    a.tags.retain(|t| *t != tag);
                } else if !a.tags.contains(&tag) {
                    a.tags.push(tag);
                }
            }
        }
        crate::app_cull::refresh_totals(st, self.cfg.cull_threshold);
        if let Some(w) = &self.writer {
            let _ = w.send(WriteJob::Xmp(ids.clone()));
        }
        self.hud = Some((Hud::Theme(tag, !all), ctx.input(|i| i.time) + 0.9));
        self.facets = None;
        self.info = None;
        // Unlike ratings, themes never move on to the next photo: one photo can take several.
        if matches!(self.filter.project, Some(ProjectFilter::Missing(_))) {
            // "고를 사진": the photo stays until the user moves on, so more themes can be added.
            if self.viewer.is_some() {
                self.refilter_needed = true;
            } else {
                self.refilter_on_move = self.sel.focus.or(ids.first().copied());
            }
        } else if self.filter.project.is_some() || !self.filter.tags.is_empty() {
            self.refilter_in_place();
        }
    }

    /// ⇧1–9: toggle the n-th theme (1-based) of the active project on the target photos.
    pub fn theme_key(&mut self, ctx: &egui::Context, n: usize) {
        let Some(p) = self.active_project else {
            self.toast("먼저 작품을 만드세요 (사이드바 '작품' → 새 작품)", true);
            return;
        };
        let tag = self.lib.as_ref().and_then(|st| st.index.themes_of(p).get(n - 1).map(|t| t.id));
        match tag {
            Some(t) => {
                let ids = self.target_ids();
                self.toggle_theme(ctx, ids, t);
            }
            None => self.toast(format!("이 작품에는 {n}번째 테마가 없습니다"), true),
        }
    }

    /// Export a project, one folder per theme.
    pub fn export_project(&mut self, project: u32) {
        let Some(st) = &self.lib else { return };
        let groups: Vec<(String, Vec<i64>)> = st
            .index
            .themes_of(project)
            .iter()
            .map(|t| {
                let ids: Vec<i64> = st.index.assets.iter().filter(|a| a.tags.contains(&t.id)).map(|a| a.id).collect();
                (t.short_name().to_string(), ids)
            })
            .filter(|g| !g.1.is_empty())
            .collect();
        if groups.is_empty() {
            self.toast("이 작품에 아직 사진이 없습니다", true);
            return;
        }
        let name = st.index.project(project).map(|p| p.name.clone()).unwrap_or_default();
        let mut ids: Vec<i64> = groups.iter().flat_map(|g| g.1.iter().copied()).collect();
        ids.sort_unstable();
        ids.dedup();
        let all: Vec<i64> = groups.iter().flat_map(|g| g.1.iter().copied()).collect();
        let group_info = photoorg_core::export::plan_info(&st.catalog, &all).ok();
        self.open_export(ids, ExportMode::JpegOnly);
        if let Some(d) = &mut self.export_dialog {
            d.zip_name = name;
            d.groups = Some(groups);
            d.by_theme = true;
            d.group_info = group_info;
        }
    }

    pub fn project_dialog_ui(&mut self, ui: &mut egui::Ui) {
        let Some(dlg) = self.project_dialog.take() else { return };
        let pal = self.pal;
        let ctx = ui.ctx().clone();
        let (mut keep, mut ok) = (true, false);
        let mut dlg = dlg;
        let project_name = |app: &PhotoApp, p: u32| {
            app.lib.as_ref().and_then(|st| st.index.project(p).map(|p| p.name.clone())).unwrap_or_default()
        };
        let theme_name = |app: &PhotoApp, t: u32| {
            app.lib
                .as_ref()
                .and_then(|st| st.index.tags.get(&t).map(|t| t.short_name().to_string()))
                .unwrap_or_default()
        };
        let title = match &dlg {
            ProjectDialog::New { .. } => "새 작품".to_string(),
            ProjectDialog::Rename { project, .. } => format!("'{}' 이름 바꾸기", project_name(self, *project)),
            ProjectDialog::AddThemes { project, .. } => format!("'{}'에 테마 추가", project_name(self, *project)),
            ProjectDialog::RenameTheme { tag, .. } => format!("'{}' 테마 이름 바꾸기", theme_name(self, *tag)),
            ProjectDialog::DeleteProject { project } => format!("'{}' 작품 삭제", project_name(self, *project)),
            ProjectDialog::DeleteTheme { tag } => format!("'{}' 테마 삭제", theme_name(self, *tag)),
        };
        let counts = |app: &PhotoApp, tags: Vec<u32>| -> usize {
            app.lib
                .as_ref()
                .map(|st| st.index.assets.iter().filter(|a| a.tags.iter().any(|t| tags.contains(t))).count())
                .unwrap_or(0)
        };
        let modal = egui::Modal::new(egui::Id::unique("project-dialog")).frame(
            Frame::new()
                .fill(pal.panel_bg)
                .corner_radius(14.0)
                .inner_margin(Margin::same(22))
                .stroke(egui::Stroke::new(1.0, pal.separator)),
        );
        let resp = modal.show(&ctx, |ui| {
            ui.set_width(420.0);
            ui.label(egui::RichText::new(&title).font(bold(18.0)));
            ui.add_space(8.0);
            let enter = |ui: &egui::Ui, r: &egui::Response| r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            match &mut dlg {
                ProjectDialog::New { name, themes } => {
                    ui.label(egui::RichText::new("작품 이름 (사진집 · 전시)").font(font(12.5)));
                    let r = ui.add(egui::TextEdit::singleline(name).hint_text("예: 바다 사진집").desired_width(f32::INFINITY));
                    if !r.has_focus() && name.is_empty() && themes.is_empty() {
                        r.request_focus();
                    }
                    ui.add_space(6.0);
                    ui.label(egui::RichText::new("테마 (한 줄에 하나, 또는 쉼표로)").font(font(12.5)));
                    ui.add(
                        egui::TextEdit::multiline(themes)
                            .hint_text("새벽\n파도\n사람\n빛")
                            .desired_rows(5)
                            .desired_width(f32::INFINITY),
                    );
                    let n = parse_themes(themes).len();
                    ui.label(
                        egui::RichText::new(format!(
                            "테마 {n}개 · 순서대로 ⇧1–9 단축키가 붙습니다. 나중에 더하거나 바꿀 수 있습니다."
                        ))
                        .font(font(11.0))
                        .color(pal.text3),
                    );
                }
                ProjectDialog::Rename { text, .. } | ProjectDialog::RenameTheme { text, .. } => {
                    let r = ui.add(egui::TextEdit::singleline(text).desired_width(f32::INFINITY));
                    // Check Enter before focusing again: refocusing first would hide `lost_focus`.
                    if enter(ui, &r) {
                        ok = true;
                    } else if !r.has_focus() {
                        r.request_focus();
                    }
                    ui.label(
                        egui::RichText::new("사진의 XMP 키워드도 새 이름으로 바뀝니다.").font(font(11.0)).color(pal.text3),
                    );
                }
                ProjectDialog::AddThemes { text, .. } => {
                    ui.add(
                        egui::TextEdit::multiline(text).hint_text("한 줄에 하나, 또는 쉼표로").desired_rows(4).desired_width(f32::INFINITY),
                    );
                }
                ProjectDialog::DeleteProject { project } => {
                    let tags: Vec<u32> = self
                        .lib
                        .as_ref()
                        .map(|st| st.index.themes_of(*project).iter().map(|t| t.id).collect())
                        .unwrap_or_default();
                    let n = counts(self, tags.clone());
                    ui.label(
                        egui::RichText::new(format!(
                            "작품과 테마 {}개를 지웁니다. 사진 {}장에서 이 작품의 테마만 빠지고, 사진 · 별점은 그대로입니다.",
                            tags.len(),
                            format::count(n)
                        ))
                        .font(font(12.5)),
                    );
                }
                ProjectDialog::DeleteTheme { tag } => {
                    let n = counts(self, vec![*tag]);
                    ui.label(
                        egui::RichText::new(format!(
                            "테마를 지웁니다. 사진 {}장에서 이 테마만 빠지고, 사진 · 별점은 그대로입니다.",
                            format::count(n)
                        ))
                        .font(font(12.5)),
                    );
                }
            }
            ui.add_space(14.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let (label, enabled) = match &dlg {
                    ProjectDialog::New { name, .. } => ("만들기", !name.trim().is_empty()),
                    ProjectDialog::Rename { text, .. } | ProjectDialog::RenameTheme { text, .. } => {
                        ("바꾸기", !text.trim().is_empty())
                    }
                    ProjectDialog::AddThemes { text, .. } => ("추가", !parse_themes(text).is_empty()),
                    ProjectDialog::DeleteProject { .. } | ProjectDialog::DeleteTheme { .. } => ("삭제", true),
                };
                if widgets::primary_button(ui, label, &pal, enabled).clicked() && enabled {
                    ok = true;
                }
                if widgets::secondary_button(ui, "취소", &pal).clicked() {
                    keep = false;
                }
            });
        });
        if resp.should_close() {
            keep = false;
        }
        if ok {
            let done = self.apply_project_dialog(&dlg);
            keep = !done;
        }
        if keep {
            self.project_dialog = Some(dlg);
        }
    }

    /// Returns true when the dialog can close.
    fn apply_project_dialog(&mut self, dlg: &ProjectDialog) -> bool {
        let Some(st) = &mut self.lib else { return true };
        let r: anyhow::Result<Vec<i64>> = match dlg {
            ProjectDialog::New { name, themes } => return self.create_project(name, themes),
            ProjectDialog::Rename { project, text } => st.catalog.rename_project(*project, text),
            ProjectDialog::AddThemes { project, text } => {
                let mut r = Ok(Vec::new());
                for t in parse_themes(text) {
                    if let Err(e) = st.catalog.add_theme(*project, &t) {
                        r = Err(e);
                        break;
                    }
                }
                r
            }
            ProjectDialog::RenameTheme { tag, text } => st.catalog.rename_theme(*tag, text),
            ProjectDialog::DeleteProject { project } => st.catalog.delete_project(*project),
            ProjectDialog::DeleteTheme { tag } => st.catalog.delete_theme(*tag),
        };
        match r {
            Ok(affected) => {
                self.reload_projects(affected);
                true
            }
            Err(e) => {
                self.toast(format!("{e:#}"), true);
                false
            }
        }
    }

    pub fn project_action(&mut self, a: ProjectAction) {
        let name = |app: &PhotoApp, p: u32| {
            app.lib.as_ref().and_then(|st| st.index.project(p).map(|p| p.name.clone())).unwrap_or_default()
        };
        let theme = |app: &PhotoApp, t: u32| {
            app.lib
                .as_ref()
                .and_then(|st| st.index.tags.get(&t).map(|t| t.short_name().to_string()))
                .unwrap_or_default()
        };
        match a {
            ProjectAction::New => {
                self.project_dialog = Some(ProjectDialog::New { name: String::new(), themes: String::new() })
            }
            ProjectAction::AddThemes(p) => {
                self.project_dialog = Some(ProjectDialog::AddThemes { project: p, text: String::new() })
            }
            ProjectAction::Rename(p) => {
                self.project_dialog = Some(ProjectDialog::Rename { project: p, text: name(self, p) })
            }
            ProjectAction::Export(p) => self.export_project(p),
            ProjectAction::Delete(p) => self.project_dialog = Some(ProjectDialog::DeleteProject { project: p }),
            ProjectAction::RenameTheme(t) => {
                self.project_dialog = Some(ProjectDialog::RenameTheme { tag: t, text: theme(self, t) })
            }
            ProjectAction::DeleteTheme(t) => self.project_dialog = Some(ProjectDialog::DeleteTheme { tag: t }),
            ProjectAction::MoveTheme(t, d) => {
                if let Some(st) = &mut self.lib {
                    if let Err(e) = st.catalog.move_theme(t, d) {
                        self.toast(format!("{e:#}"), true);
                        return;
                    }
                }
                self.reload_projects(Vec::new());
            }
            ProjectAction::MinRating(k) => {
                self.cfg.project_min_rating = k;
                self.cfg.save();
                if let Some(st) = &mut self.lib {
                    st.project_min_rating = k;
                    crate::app_cull::refresh_totals(st, self.cfg.cull_threshold);
                }
                if let SideSel::ProjectTodo(p) = self.side {
                    self.select_side(SideSel::ProjectTodo(p));
                }
            }
        }
    }

    /// The "작품" section of the info panel: active project and its themes as toggles.
    pub fn project_chips(&mut self, ui: &mut egui::Ui, ids: &[i64]) {
        let pal = self.pal;
        let ctx = ui.ctx().clone();
        let Some(st) = &self.lib else { return };
        if st.index.projects.is_empty() {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("작품").font(bold(12.0)).color(pal.text2));
                if ui
                    .add(egui::Button::new(egui::RichText::new("+ 작품 만들기…").color(pal.accent)).frame(false))
                    .clicked()
                {
                    self.project_dialog = Some(ProjectDialog::New { name: String::new(), themes: String::new() });
                }
            });
            return;
        }
        let projects: Vec<(u32, String)> = st.index.projects.iter().map(|p| (p.id, p.name.clone())).collect();
        let active = self.active_project.filter(|p| projects.iter().any(|x| x.0 == *p)).unwrap_or(projects[0].0);
        let themes: Vec<(u32, String, usize)> = st
            .index
            .themes_of(active)
            .iter()
            .map(|t| {
                let n = ids.iter().filter(|id| st.index.get(**id).is_some_and(|a| a.tags.contains(&t.id))).count();
                (t.id, t.short_name().to_string(), n)
            })
            .collect();
        let mut switch_to = None;
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("작품").font(bold(12.0)).color(pal.text2));
            let current = projects.iter().find(|p| p.0 == active).map(|p| p.1.clone()).unwrap_or_default();
            egui::ComboBox::from_id_salt("active-project").selected_text(current).width(170.0).show_ui(ui, |ui| {
                for (id, name) in &projects {
                    if ui.selectable_label(*id == active, name).clicked() {
                        switch_to = Some(*id);
                    }
                }
            });
        });
        if let Some(p) = switch_to {
            self.set_active_project(Some(p));
        }
        let mut toggle = None;
        let mut add = false;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
            for (i, (tag, name, n)) in themes.iter().enumerate() {
                let label = if i < 9 { format!("{} {name}", i + 1) } else { name.clone() };
                let state = if *n == 0 {
                    0
                } else if *n == ids.len() {
                    2
                } else {
                    1
                };
                if theme_chip(ui, &label, state, &pal)
                    .on_hover_text(if i < 9 { format!("⇧{} 로도 켜고 끕니다", i + 1) } else { String::new() })
                    .clicked()
                {
                    toggle = Some(*tag);
                }
            }
            if ui.add(egui::Button::new(egui::RichText::new("+ 테마").color(pal.accent)).frame(false)).clicked() {
                add = true;
            }
        });
        if themes.is_empty() {
            ui.label(
                egui::RichText::new("테마를 추가하면 여기서 바로 지정할 수 있습니다").font(font(11.0)).color(pal.text3),
            );
        }
        if let Some(t) = toggle {
            self.toggle_theme(&ctx, ids.to_vec(), t);
        }
        if add {
            self.project_dialog = Some(ProjectDialog::AddThemes { project: active, text: String::new() });
        }
    }
}

/// A toggle chip: 0 = off, 1 = some of the selection, 2 = on.
fn theme_chip(ui: &mut egui::Ui, text: &str, state: u8, pal: &crate::theme::Palette) -> egui::Response {
    let g = ui.painter().layout_no_wrap(text.to_owned(), font(11.5), pal.text);
    let (r, resp) = ui.allocate_exact_size(egui::vec2(g.size().x + 16.0, 22.0), egui::Sense::click());
    let (bg, fg, stroke) = match state {
        2 => (pal.accent, pal.accent_text, egui::Stroke::NONE),
        1 => (pal.accent.gamma_multiply(0.25), pal.text, egui::Stroke::new(1.0, pal.accent)),
        _ if resp.hovered() => (pal.hover, pal.text, egui::Stroke::new(1.0, pal.separator)),
        _ => (pal.control_bg, pal.text, egui::Stroke::NONE),
    };
    ui.painter().rect(r, 11.0, bg, stroke, egui::StrokeKind::Inside);
    let g = ui.painter().layout_no_wrap(text.to_owned(), font(11.5), fg);
    ui.painter().galley(egui::pos2(r.left() + 8.0, r.center().y - g.size().y / 2.0), g, fg);
    resp
}
