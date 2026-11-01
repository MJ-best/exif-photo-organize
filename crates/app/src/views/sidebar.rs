use super::titlebar_pad;
use crate::app::{ImportPhase, PhotoApp, SideSel};
use crate::theme::{bold, font};
use crate::widgets::{self, Icon};
use eframe::egui::{self, pos2, vec2, Align2, Rect, Sense, Stroke, Ui, Vec2};
use photoorg_core::format;

impl PhotoApp {
    pub fn sidebar(&mut self, ui: &mut Ui) {
        let pal = self.pal;
        let ctx = ui.ctx().clone();
        ui.add_space(titlebar_pad() + 12.0);
        let bottom_h = 58.0
            + if self.import_running() { 46.0 } else { 0.0 }
            + if self.maintenance.is_some() { 34.0 } else { 0.0 }
            + if self.analysis_job.is_some() { 34.0 } else { 0.0 };
        let avail = ui.available_height() - bottom_h;
        egui::ScrollArea::vertical().id_salt("sidebar").max_height(avail.max(50.0)).auto_shrink([false, false]).show(
            ui,
            |ui| {
                ui.spacing_mut().item_spacing.y = 1.0;
                let mut clicked: Option<SideSel> = None;
                if let Some(st) = &self.lib {
                    let t = &st.totals;
                    let rated: usize = t.ratings[1..].iter().sum();
                    let pairs = t
                        .formats
                        .iter()
                        .find(|f| f.0 == photoorg_core::filter::FormatFilter::Pair)
                        .map(|f| f.1)
                        .unwrap_or(0);
                    let videos = t
                        .formats
                        .iter()
                        .find(|f| f.0 == photoorg_core::filter::FormatFilter::Video)
                        .map(|f| f.1)
                        .unwrap_or(0);
                    let recent = st.index.imports.first().map(|i| i.asset_count as usize);
                    widgets::section_title(ui, "사진", &pal);
                    let items: Vec<(SideSel, Icon, &str, Option<usize>)> = vec![
                        (SideSel::All, Icon::Photos, "라이브러리", Some(t.total)),
                        (SideSel::Recent, Icon::Clock, "최근 가져온 항목", recent),
                        (SideSel::Rated, Icon::Star, "별점 있는 사진", Some(rated)),
                        (SideSel::Unrated, Icon::Check, "아직 고르지 않은 사진", Some(t.ratings[0])),
                        (SideSel::Pairs, Icon::Raw, "RAW+JPEG", Some(pairs)),
                        (SideSel::Videos, Icon::Video, "동영상", Some(videos)),
                    ];
                    for (sel, icon, label, count) in items {
                        if matches!(sel, SideSel::Videos | SideSel::Pairs) && count == Some(0) {
                            continue;
                        }
                        if widgets::sidebar_item(ui, icon, label, count, self.side == sel, &pal).clicked() {
                            clicked = Some(sel);
                        }
                    }

                    // Culling assistant.
                    let c = st.cull_counts;
                    if st.analysed > 0 || c[1] > 0 || c[2] > 0 {
                        widgets::section_title(ui, "셀렉 도우미", &pal);
                        let items: Vec<(SideSel, Icon, &str, usize, &str)> = vec![
                            (
                                SideSel::Candidates,
                                Icon::Suspect,
                                "B컷 후보",
                                c[0],
                                "초점·흔들림·눈 감음·노출·연사 비교로 골라낸, 아직 고르지 않은 사진",
                            ),
                            (SideSel::Rejected, Icon::Reject, "거절 (B컷)", c[1], "X 키로 거절한 사진"),
                            (SideSel::Picked, Icon::Flag, "픽 (A컷)", c[2], "P 키로 고른 사진"),
                            (
                                SideSel::BurstBest,
                                Icon::Stack,
                                "연사 베스트",
                                c[4],
                                "연사·비슷한 장면 묶음마다 가장 좋은 한 장 (단독 사진 포함)",
                            ),
                        ];
                        for (sel, icon, label, count, tip) in items {
                            if widgets::sidebar_item(ui, icon, label, Some(count), self.side == sel, &pal)
                                .on_hover_text(tip)
                                .clicked()
                            {
                                clicked = Some(sel);
                            }
                        }
                    }
                }

                widgets::section_title(ui, "가져오기", &pal);
                let vols = self.volumes.clone();
                for v in &vols {
                    let sel = matches!(&self.side, SideSel::Device(p) if p == &v.path)
                        && self.view == crate::app::MainView::Import;
                    let running_here = self
                        .import
                        .as_ref()
                        .is_some_and(|s| s.source == v.path && matches!(s.phase, ImportPhase::Running));
                    let resp = widgets::sidebar_item(ui, Icon::Card, &v.name, None, sel, &pal);
                    let er =
                        Rect::from_center_size(pos2(resp.rect.right() - 16.0, resp.rect.center().y), Vec2::splat(18.0));
                    if running_here {
                        let p = self.import.as_ref().map(|s| s.progress.fraction()).unwrap_or(0.0);
                        paint_ring(ui, er.center(), 7.0, p, &pal);
                    } else {
                        let eresp = ui
                            .interact(er, ui.scope_id().with(("eject", &v.path)), Sense::click())
                            .on_hover_text("추출");
                        widgets::paint_icon(
                            ui.painter(),
                            er.shrink(3.0),
                            Icon::Eject,
                            if eresp.hovered() { pal.text } else { pal.text2 },
                        );
                        if eresp.clicked() {
                            self.eject(v);
                            continue;
                        }
                    }
                    if resp.clicked() {
                        clicked = Some(SideSel::Device(v.path.clone()));
                    }
                }
                // A folder import session (not a card).
                if let Some(sess) = &self.import {
                    if sess.volume.is_none() {
                        let sel = self.view == crate::app::MainView::Import;
                        if widgets::sidebar_item(ui, Icon::Folder, &sess.label, None, sel, &pal).clicked() {
                            self.view = crate::app::MainView::Import;
                            self.viewer = None;
                        }
                    }
                }
                if widgets::sidebar_item(ui, Icon::Import, "폴더에서 가져오기…", None, false, &pal).clicked()
                {
                    if self.lib.is_some() {
                        self.pick_import_folder();
                    } else {
                        self.toast("먼저 라이브러리 폴더를 선택하세요", true);
                    }
                }
                if vols.is_empty() {
                    ui.add_space(2.0);
                    ui.horizontal(|ui| {
                        ui.add_space(34.0);
                        ui.label(
                            egui::RichText::new("SD 카드를 꽂으면 여기에 나타납니다").font(font(10.5)).color(pal.text3),
                        );
                    });
                }

                if let Some(st) = &self.lib {
                    let scenes: Vec<(u8, usize)> = st.totals.scenes.iter().copied().filter(|s| s.0 != 0).collect();
                    if !scenes.is_empty() {
                        widgets::section_title(ui, "장면", &pal);
                        for (s, n) in scenes {
                            let label = photoorg_core::cull::Scene::from_u8(s).label();
                            if widgets::sidebar_item(
                                ui,
                                Icon::Scene,
                                label,
                                Some(n),
                                self.side == SideSel::Scene(s),
                                &pal,
                            )
                            .clicked()
                            {
                                clicked = Some(SideSel::Scene(s));
                            }
                        }
                    }
                    let cams: Vec<(u32, usize)> = st.totals.cameras.clone();
                    if !cams.is_empty() {
                        widgets::section_title(ui, "카메라", &pal);
                        for (c, n) in cams.iter().take(12) {
                            let name = st.index.cameras.name(*c).to_string();
                            if widgets::sidebar_item(
                                ui,
                                Icon::Camera,
                                &name,
                                Some(*n),
                                self.side == SideSel::Camera(*c),
                                &pal,
                            )
                            .clicked()
                            {
                                clicked = Some(SideSel::Camera(*c));
                            }
                        }
                    }
                    let lenses: Vec<(u32, usize)> = st.totals.lenses.clone();
                    if !lenses.is_empty() {
                        widgets::section_title(ui, "렌즈", &pal);
                        for (l, n) in lenses.iter().take(12) {
                            let name = st.index.lenses.name(*l).to_string();
                            if widgets::sidebar_item(
                                ui,
                                Icon::Lens,
                                &name,
                                Some(*n),
                                self.side == SideSel::Lens(*l),
                                &pal,
                            )
                            .clicked()
                            {
                                clicked = Some(SideSel::Lens(*l));
                            }
                        }
                    }
                    if !st.index.imports.is_empty() {
                        widgets::section_title(ui, "가져오기 기록", &pal);
                        for imp in st.index.imports.iter().take(10) {
                            // "2026-11-01 05:06:07" -> "11.01 05:06"
                            let date = imp.started_at.get(5..16).unwrap_or(&imp.started_at).replace('-', ".");
                            let label = format!("{date}  {}", imp.label);
                            if widgets::sidebar_item(
                                ui,
                                Icon::Library,
                                &label,
                                Some(imp.asset_count as usize),
                                self.side == SideSel::Import(imp.id),
                                &pal,
                            )
                            .on_hover_text(format!(
                                "{}\n{}개 파일 · {}",
                                imp.source,
                                imp.file_count,
                                format::bytes(imp.bytes as u64)
                            ))
                            .clicked()
                            {
                                clicked = Some(SideSel::Import(imp.id));
                            }
                        }
                    }
                }
                if let Some(s) = clicked {
                    self.select_side(s);
                }
            },
        );

        // Bottom area: progress + library + settings.
        let rest = ui.available_rect_before_wrap();
        let mut y = rest.bottom() - bottom_h + 6.0;
        let p = ui.painter().clone();
        if let Some(sess) = &self.import {
            if matches!(sess.phase, ImportPhase::Running) {
                let r = Rect::from_min_size(pos2(rest.left(), y), vec2(rest.width(), 40.0));
                let resp = ui.interact(r, ui.scope_id().with("import-progress"), Sense::click());
                p.rect_filled(r, 8.0, if resp.hovered() { pal.sidebar_sel } else { pal.hover });
                let pr = &sess.progress;
                p.text(
                    pos2(r.left() + 10.0, r.top() + 12.0),
                    Align2::LEFT_CENTER,
                    format!("{} 가져오는 중", sess.label),
                    bold(11.5),
                    pal.text,
                );
                p.text(
                    pos2(r.right() - 10.0, r.top() + 12.0),
                    Align2::RIGHT_CENTER,
                    format!("{}/{}", pr.files_done, pr.files_total),
                    font(11.0),
                    pal.text2,
                );
                let bar = Rect::from_min_size(pos2(r.left() + 10.0, r.top() + 25.0), vec2(r.width() - 20.0, 5.0));
                p.rect_filled(bar, 2.5, pal.control_bg);
                p.rect_filled(
                    Rect::from_min_size(bar.min, vec2(bar.width() * pr.fraction(), bar.height())),
                    2.5,
                    pal.accent,
                );
                if resp.clicked() {
                    self.view = crate::app::MainView::Import;
                    self.viewer = None;
                }
                y += 46.0;
            }
        }
        if let Some(crate::app::Maintenance::Thumbs { done, total, .. }) = &self.maintenance {
            let d = done.load(std::sync::atomic::Ordering::Relaxed);
            let r = Rect::from_min_size(pos2(rest.left(), y), vec2(rest.width(), 28.0));
            p.text(
                pos2(r.left() + 10.0, r.top() + 8.0),
                Align2::LEFT_CENTER,
                format!("썸네일 만드는 중 {d}/{total}"),
                font(11.0),
                pal.text2,
            );
            let bar = Rect::from_min_size(pos2(r.left() + 10.0, r.top() + 18.0), vec2(r.width() - 20.0, 4.0));
            p.rect_filled(bar, 2.0, pal.control_bg);
            p.rect_filled(
                Rect::from_min_size(bar.min, vec2(bar.width() * (d as f32 / (*total).max(1) as f32), 4.0)),
                2.0,
                pal.accent,
            );
            y += 34.0;
        }
        if let Some(job) = &self.analysis_job {
            let d = job.done.load(std::sync::atomic::Ordering::Relaxed);
            let total = job.total;
            let r = Rect::from_min_size(pos2(rest.left(), y), vec2(rest.width(), 28.0));
            ui.interact(r, ui.scope_id().with("analysis-progress"), Sense::hover())
                .on_hover_text("B컷 후보를 찾기 위해 초점·흔들림·노출·얼굴·눈·장면을 분석합니다.\n사진은 바뀌지 않으며, 분석은 한 번만 합니다.");
            p.text(
                pos2(r.left() + 10.0, r.top() + 8.0),
                Align2::LEFT_CENTER,
                format!("사진 분석 중 {}/{}", format::count(d), format::count(total)),
                font(11.0),
                pal.text2,
            );
            let bar = Rect::from_min_size(pos2(r.left() + 10.0, r.top() + 18.0), vec2(r.width() - 20.0, 4.0));
            p.rect_filled(bar, 2.0, pal.control_bg);
            p.rect_filled(
                Rect::from_min_size(bar.min, vec2(bar.width() * (d as f32 / total.max(1) as f32).min(1.0), 4.0)),
                2.0,
                pal.accent,
            );
            y += 34.0;
        }
        p.line_segment([pos2(rest.left(), y), pos2(rest.right(), y)], Stroke::new(1.0, pal.separator));
        let row = Rect::from_min_size(pos2(rest.left(), y + 8.0), vec2(rest.width(), 40.0));
        let gear = Rect::from_center_size(pos2(row.right() - 16.0, row.center().y), Vec2::splat(28.0));
        let gresp = ui.interact(gear, ui.scope_id().with("gear"), Sense::click()).on_hover_text("설정");
        if gresp.hovered() {
            p.rect_filled(gear, 6.0, pal.hover);
        }
        widgets::paint_icon(&p, gear.shrink(6.0), Icon::Gear, pal.text2);
        if gresp.clicked() {
            self.settings_open = true;
        }
        let lresp = ui.interact(
            Rect::from_min_max(row.min, pos2(gear.left() - 4.0, row.max.y)),
            ui.scope_id().with("libname"),
            Sense::click(),
        );
        let (name, path) = match &self.lib {
            Some(st) => (st.lib.name(), st.lib.root.display().to_string()),
            None => ("라이브러리 없음".to_string(), "클릭하여 선택".to_string()),
        };
        p.text(pos2(row.left() + 8.0, row.top() + 12.0), Align2::LEFT_CENTER, &name, bold(12.0), pal.text);
        let g = p.layout_no_wrap(path.clone(), font(10.5), pal.text3);
        p.with_clip_rect(Rect::from_min_max(row.min, pos2(gear.left() - 6.0, row.max.y))).galley(
            pos2(row.left() + 8.0, row.top() + 22.0),
            g,
            pal.text3,
        );
        if lresp.on_hover_text(format!("{path}\n클릭하여 라이브러리 변경")).clicked() {
            self.choose_library(&ctx);
        }
    }

    pub fn import_running(&self) -> bool {
        self.import.as_ref().is_some_and(|s| matches!(s.phase, ImportPhase::Running))
    }
}

fn paint_ring(ui: &Ui, c: egui::Pos2, r: f32, frac: f32, pal: &crate::theme::Palette) {
    let p = ui.painter();
    p.circle_stroke(c, r, Stroke::new(2.0, pal.control_bg));
    let n = 32;
    let end = (frac.clamp(0.0, 1.0) * n as f32) as usize;
    let pts: Vec<egui::Pos2> = (0..=end)
        .map(|i| {
            let a = -std::f32::consts::FRAC_PI_2 + i as f32 / n as f32 * std::f32::consts::TAU;
            pos2(c.x + a.cos() * r, c.y + a.sin() * r)
        })
        .collect();
    if pts.len() > 1 {
        p.add(egui::Shape::line(pts, Stroke::new(2.0, pal.accent)));
    }
}
