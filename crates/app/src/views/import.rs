use super::grid::{contain_rect, cover_uv};
use super::titlebar_pad;
use crate::app::{ImportPhase, PhotoApp};
use crate::theme::{bold, font};
use crate::widgets::{self, Icon};
use chrono::NaiveDate;
use eframe::egui::{self, pos2, vec2, Align2, Color32, Id, Rect, Sense, Stroke, StrokeKind, Ui, UiBuilder};
use photoorg_core::{format, template};

impl PhotoApp {
    pub fn import_view(&mut self, ui: &mut Ui) {
        let pal = self.pal;
        let ctx = ui.ctx().clone();
        if self.import.is_none() {
            self.view = crate::app::MainView::Library;
            return;
        }
        if self.lib.is_none() {
            ui.centered_and_justified(|ui| ui.label("먼저 라이브러리 폴더를 선택하세요."));
            return;
        }
        let full = ui.available_rect_before_wrap();

        // Header.
        let head_h = 64.0 + titlebar_pad();
        let head = Rect::from_min_size(full.min, vec2(full.width(), head_h));
        let hresp = ui.interact(head, Id::unique("import-head"), Sense::click_and_drag());
        if cfg!(target_os = "macos") && hresp.drag_started() {
            ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }
        ui.painter().rect_filled(head, 0.0, pal.toolbar_bg);
        widgets::separator_h(ui, head, &pal);
        let hb = Rect::from_min_max(
            pos2(head.left() + 18.0, head.top() + titlebar_pad()),
            pos2(head.right() - 16.0, head.bottom()),
        );
        let sess = self.import.as_ref().unwrap();
        let p = ui.painter();
        widgets::paint_icon(
            p,
            Rect::from_center_size(pos2(hb.left() + 12.0, hb.center().y), egui::Vec2::splat(22.0)),
            if sess.volume.is_some() { Icon::Card } else { Icon::Folder },
            pal.accent,
        );
        p.text(
            pos2(hb.left() + 34.0, hb.center().y - 9.0),
            Align2::LEFT_CENTER,
            format!("가져오기 · {}", sess.label),
            bold(16.0),
            pal.text,
        );
        let lib_name = self.lib.as_ref().map(|s| s.lib.root.display().to_string()).unwrap_or_default();
        p.text(
            pos2(hb.left() + 34.0, hb.center().y + 11.0),
            Align2::LEFT_CENTER,
            format!("{}  →  {}", sess.source.display(), lib_name),
            font(11.5),
            pal.text2,
        );

        let mut rui = ui.new_child(
            UiBuilder::new()
                .max_rect(Rect::from_min_max(pos2(hb.center().x, hb.top()), hb.max))
                .layout(egui::Layout::right_to_left(egui::Align::Center)),
        );
        rui.spacing_mut().item_spacing.x = 8.0;
        let mut start: Option<bool> = None;
        let mut cancel = false;
        let mut eject = false;
        let mut show_imported = false;
        match &sess.phase {
            ImportPhase::Ready | ImportPhase::Done => {
                let new = sess.summary.as_ref().map(|s| s.new_groups).unwrap_or(0);
                if widgets::primary_button(
                    &mut rui,
                    &format!("새 항목 모두 가져오기 ({})", format::count(new)),
                    &pal,
                    new > 0,
                )
                .clicked()
                {
                    start = Some(false);
                }
                if !sess.selected.is_empty()
                    && widgets::secondary_button(
                        &mut rui,
                        &format!("선택 항목 가져오기 ({})", sess.selected.len()),
                        &pal,
                    )
                    .clicked()
                {
                    start = Some(true);
                }
                if sess.volume.is_some() && widgets::secondary_button(&mut rui, "추출", &pal).clicked() {
                    eject = true;
                }
                if matches!(sess.phase, ImportPhase::Done)
                    && widgets::secondary_button(&mut rui, "가져온 사진 보기", &pal).clicked()
                {
                    show_imported = true;
                }
            }
            ImportPhase::Running if widgets::secondary_button(&mut rui, "중단", &pal).clicked() => {
                cancel = true;
            }
            _ => {}
        }

        let mut y = head.bottom();
        // Options / progress strip.
        let strip_h = match sess.phase {
            ImportPhase::Running => 74.0,
            _ => 96.0,
        };
        let strip = Rect::from_min_size(pos2(full.left(), y), vec2(full.width(), strip_h));
        ui.painter().rect_filled(strip, 0.0, pal.panel_bg);
        widgets::separator_h(ui, strip, &pal);
        y = strip.bottom();
        let inner = strip.shrink2(vec2(20.0, 10.0));
        let mut sui = ui.new_child(UiBuilder::new().max_rect(inner).layout(egui::Layout::top_down(egui::Align::Min)));
        let sess = self.import.as_ref().unwrap();
        match &sess.phase {
            ImportPhase::Running => {
                let pr = sess.progress.clone();
                let label = if pr.files_total == 0 {
                    "준비 중…".to_string()
                } else {
                    format!(
                        "{} / {} 파일  ·  {} / {}  ·  {}/s{}",
                        format::count(pr.files_done),
                        format::count(pr.files_total),
                        format::bytes(pr.bytes_done),
                        format::bytes(pr.bytes_total),
                        format::bytes(pr.speed() as u64),
                        pr.eta().map(|e| format!("  ·  남은 시간 {}", format::duration(e))).unwrap_or_default()
                    )
                };
                sui.label(egui::RichText::new("복사 · 검증 · 정리 · 썸네일 생성 중").font(bold(13.0)));
                sui.label(egui::RichText::new(label).font(font(12.0)).color(pal.text2));
                sui.add_space(6.0);
                let (bar, _) = sui.allocate_exact_size(vec2(inner.width(), 6.0), Sense::hover());
                sui.painter().rect_filled(bar, 3.0, pal.control_bg);
                sui.painter().rect_filled(
                    Rect::from_min_size(bar.min, vec2(bar.width() * pr.fraction(), 6.0)),
                    3.0,
                    pal.accent,
                );
                if !pr.current.is_empty() {
                    sui.label(egui::RichText::new(&pr.current).font(font(10.5)).color(pal.text3));
                }
            }
            _ => {
                let mut opts = self.cfg.import.clone();
                sui.horizontal(|ui| {
                    ui.label(egui::RichText::new("폴더 구조").font(font(12.5)).color(pal.text2));
                    egui::ComboBox::from_id_salt("folder-tpl")
                        .width(250.0)
                        .selected_text(
                            template::FOLDER_PRESETS
                                .iter()
                                .find(|p| p.template == opts.folder_template)
                                .map(|p| p.label)
                                .unwrap_or(&opts.folder_template),
                        )
                        .show_ui(ui, |ui| {
                            for pr in template::FOLDER_PRESETS {
                                ui.selectable_value(&mut opts.folder_template, pr.template.to_string(), pr.label);
                            }
                        });
                    ui.add_space(10.0);
                    ui.label(egui::RichText::new("파일 이름").font(font(12.5)).color(pal.text2));
                    egui::ComboBox::from_id_salt("file-tpl")
                        .width(250.0)
                        .selected_text(
                            template::FILE_PRESETS
                                .iter()
                                .find(|p| p.template == opts.file_template)
                                .map(|p| p.label)
                                .unwrap_or(&opts.file_template),
                        )
                        .show_ui(ui, |ui| {
                            for pr in template::FILE_PRESETS {
                                ui.selectable_value(&mut opts.file_template, pr.template.to_string(), pr.label);
                            }
                        });
                });
                sui.add_space(4.0);
                sui.horizontal(|ui| {
                    let mut raw_sub = !opts.raw_subfolder.is_empty();
                    if widgets::switch(ui, &mut raw_sub, "RAW는 ‘RAW’ 하위 폴더에", &pal).changed() {
                        opts.raw_subfolder = if raw_sub { "RAW".into() } else { String::new() };
                    }
                    ui.add_space(12.0);
                    widgets::switch(ui, &mut opts.include_videos, "동영상 포함", &pal);
                    ui.add_space(12.0);
                    widgets::switch(ui, &mut opts.verify, "복사 후 체크섬 검증", &pal);
                    ui.add_space(12.0);
                    widgets::switch(ui, &mut self.cfg.eject_after_import, "완료 후 카드 추출", &pal);
                });
                sui.add_space(2.0);
                sui.label(
                    egui::RichText::new(format!("예: {}", self.example_destination()))
                        .font(font(11.0))
                        .color(pal.text3),
                );
                if opts != self.cfg.import {
                    let videos_changed = opts.include_videos != self.cfg.import.include_videos;
                    self.cfg.import = opts;
                    self.cfg.save();
                    if videos_changed {
                        if let Some(s) = &self.import {
                            let (src, vol) = (s.source.clone(), s.volume.clone());
                            self.import = None;
                            self.start_scan(src, vol);
                        }
                    }
                }
            }
        }

        // Summary line.
        let sess = self.import.as_ref().unwrap();
        let sum_rect = Rect::from_min_size(pos2(full.left(), y), vec2(full.width(), 40.0));
        y = sum_rect.bottom();
        let sp = ui.painter();
        match &sess.phase {
            ImportPhase::Scanning { done, total } => {
                let t = if *total == 0 {
                    "파일을 찾는 중…".to_string()
                } else {
                    format!("메타데이터 읽는 중… {done} / {total}")
                };
                sp.text(sum_rect.left_center() + vec2(20.0, 0.0), Align2::LEFT_CENTER, t, font(13.0), pal.text2);
            }
            ImportPhase::Failed(e) => {
                sp.text(
                    sum_rect.left_center() + vec2(20.0, 0.0),
                    Align2::LEFT_CENTER,
                    format!("오류: {e}"),
                    font(13.0),
                    pal.danger,
                );
            }
            _ => {
                if let Some(s) = &sess.summary {
                    let mut parts =
                        vec![format!("새 항목 {}개", format::count(s.new_groups)), format::bytes(s.new_bytes)];
                    if s.raw_pairs > 0 {
                        parts.push(format!("RAW+JPEG {}쌍", format::count(s.raw_pairs)));
                    }
                    if let (Some(a), Some(b)) = (s.first, s.last) {
                        parts.push(if a.date() == b.date() {
                            format::date_medium(a.date())
                        } else {
                            format!("{} – {}", format::date_medium(a.date()), format::date_medium(b.date()))
                        });
                    }
                    if s.imported_groups > 0 {
                        parts.push(format!("이미 가져옴 {}개", format::count(s.imported_groups)));
                    }
                    let cams: Vec<String> = s.cameras.iter().take(3).map(|(c, n)| format!("{c} {n}")).collect();
                    if !cams.is_empty() {
                        parts.push(cams.join(", "));
                    }
                    sp.text(
                        sum_rect.left_center() + vec2(20.0, 0.0),
                        Align2::LEFT_CENTER,
                        parts.join("  ·  "),
                        font(12.5),
                        pal.text,
                    );
                }
                if let Some(r) = &sess.result {
                    let txt = format!(
                        "완료: 새 사진 {}개 · 파일 {}개 ({}) · {}{}{}",
                        format::count(r.assets_added),
                        format::count(r.files_copied),
                        format::bytes(r.bytes_copied),
                        format::duration(r.elapsed),
                        if r.duplicates > 0 {
                            format!(" · 중복 {}개 건너뜀", r.duplicates)
                        } else {
                            String::new()
                        },
                        if r.failures.is_empty() {
                            String::new()
                        } else {
                            format!(" · 실패 {}개", r.failures.len())
                        }
                    );
                    sp.text(
                        sum_rect.right_center() - vec2(20.0, 0.0),
                        Align2::RIGHT_CENTER,
                        txt,
                        bold(12.5),
                        if r.failures.is_empty() { pal.success } else { pal.danger },
                    );
                }
            }
        }
        // Hide-imported toggle.
        if matches!(sess.phase, ImportPhase::Ready | ImportPhase::Done)
            && sess.summary.as_ref().is_some_and(|s| s.imported_groups > 0)
            && sess.result.is_none()
        {
            let r = Rect::from_min_size(pos2(sum_rect.right() - 230.0, sum_rect.top() + 9.0), vec2(220.0, 22.0));
            let mut hide = sess.hide_imported;
            let mut c = ui.new_child(UiBuilder::new().max_rect(r));
            if widgets::switch(&mut c, &mut hide, "가져온 항목 숨기기", &pal).changed() {
                self.import.as_mut().unwrap().hide_imported = hide;
            }
        }

        // Grid of card contents.
        let grid_rect = Rect::from_min_max(pos2(full.left(), y), full.max);
        let mut gui = ui.new_child(UiBuilder::new().max_rect(grid_rect));
        self.import_grid(&mut gui);

        if let Some(only) = start {
            self.start_import(&ctx, only);
        }
        if cancel {
            if let Some(s) = &self.import {
                s.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
            }
        }
        if eject {
            if let Some(v) = self.import.as_ref().and_then(|s| s.volume.clone()) {
                self.eject(&v);
            }
        }
        if show_imported {
            let id = self.lib.as_ref().and_then(|s| s.index.imports.first().map(|i| i.id));
            if let Some(id) = id {
                self.select_side(crate::app::SideSel::Import(id));
            }
        }
        let _ = (Color32::WHITE, Stroke::NONE);
    }

    fn import_grid(&mut self, ui: &mut Ui) {
        let pal = self.pal;
        let Some(sess) = &mut self.import else { return };
        let Some(scan) = sess.scan.clone() else { return };
        let modifiers = ui.input(|i| i.modifiers);
        // Visible items grouped by day.
        sess.visible =
            (0..scan.groups.len()).filter(|&i| !(sess.hide_imported && scan.groups[i].is_imported())).collect();
        let width = ui.available_width() - 4.0;
        let tile = 118.0;
        let gap = 6.0;
        let pad = 18.0;
        let cols = (((width - pad * 2.0) + gap) / (tile + gap)).floor().max(1.0) as usize;
        // Build rows: (y, kind) kind = header(date) or tiles(range in visible)
        let mut rows: Vec<(f32, Option<NaiveDate>, usize, usize)> = Vec::new();
        let mut y = 8.0;
        let mut s = 0;
        while s < sess.visible.len() {
            let d = scan.groups[sess.visible[s]].meta.captured_at.map(|t| t.date()).unwrap_or_default();
            let mut e = s + 1;
            while e < sess.visible.len()
                && scan.groups[sess.visible[e]].meta.captured_at.map(|t| t.date()).unwrap_or_default() == d
            {
                e += 1;
            }
            rows.push((y, Some(d), s, e));
            y += 34.0;
            let mut k = s;
            while k < e {
                let ke = (k + cols).min(e);
                rows.push((y, None, k, ke));
                y += tile + gap;
                k = ke;
            }
            y += 6.0;
            s = e;
        }
        let height = y + 20.0;
        let thumbs = sess.thumbs.as_mut();
        let Some(thumbs) = thumbs else { return };
        let mut toggle: Vec<usize> = Vec::new();
        let mut select_day: Option<(usize, usize)> = None;
        egui::ScrollArea::vertical()
            .id_salt(("import-grid", sess.source.clone()))
            .auto_shrink([false, false])
            .show_viewport(ui, |ui, vp| {
                ui.set_height(height);
                ui.set_width(width);
                let o = ui.max_rect().min;
                let p = ui.painter().clone();
                for &(ry, date, a, b) in &rows {
                    let h = if date.is_some() { 34.0 } else { tile };
                    if ry + h < vp.min.y || ry > vp.max.y {
                        continue;
                    }
                    if let Some(d) = date {
                        p.text(
                            pos2(o.x + pad, o.y + ry + 18.0),
                            Align2::LEFT_CENTER,
                            format::date_long(d),
                            bold(14.0),
                            pal.text,
                        );
                        let n_new = (a..b).filter(|&i| !scan.groups[sess.visible[i]].is_imported()).count();
                        let txt = format!("{}개 · 새 항목 {}개", b - a, n_new);
                        let g = p.layout_no_wrap(format::date_long(d), bold(14.0), pal.text);
                        p.text(
                            pos2(o.x + pad + g.size().x + 10.0, o.y + ry + 18.0),
                            Align2::LEFT_CENTER,
                            txt,
                            font(11.5),
                            pal.text2,
                        );
                        let sr = Rect::from_min_size(pos2(o.x + width - pad - 70.0, o.y + ry + 6.0), vec2(70.0, 24.0));
                        let resp = ui.interact(sr, Id::unique(("selday", d)), Sense::click());
                        p.text(
                            sr.right_center(),
                            Align2::RIGHT_CENTER,
                            "모두 선택",
                            font(12.0),
                            if resp.hovered() { pal.text } else { pal.accent },
                        );
                        if resp.clicked() {
                            select_day = Some((a, b));
                        }
                        continue;
                    }
                    for (col, vi) in (a..b).enumerate() {
                        let gi = sess.visible[vi];
                        let g = &scan.groups[gi];
                        let rect = Rect::from_min_size(
                            pos2(o.x + pad + col as f32 * (tile + gap), o.y + ry),
                            egui::Vec2::splat(tile),
                        );
                        let resp = ui.interact(rect, Id::unique(("src", gi)), Sense::click());
                        p.rect_filled(rect, 6.0, pal.placeholder);
                        if let Some(t) = thumbs.get(gi) {
                            let r = contain_rect(t.size(), rect.shrink(4.0));
                            let _ = cover_uv;
                            p.image(t.id(), r, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                        } else if g.is_video() {
                            widgets::paint_icon(
                                &p,
                                Rect::from_center_size(rect.center(), egui::Vec2::splat(34.0)),
                                Icon::Video,
                                pal.text3,
                            );
                        } else {
                            p.text(rect.center(), Align2::CENTER_CENTER, &g.stem, font(10.5), pal.text3);
                        }
                        let fmt = g.formats();
                        let label = photoorg_core::media::formats::label(fmt);
                        widgets::badge(
                            &p,
                            rect.right_top() + vec2(-5.0, 5.0),
                            Align2::RIGHT_TOP,
                            label,
                            8.0,
                            Color32::from_black_alpha(150),
                            Color32::WHITE,
                        );
                        if let Some(r) = g.meta.rating.filter(|r| *r > 0) {
                            widgets::paint_stars(&p, rect.left_bottom() + vec2(5.0, -5.0), r, 9.0, pal.star, true);
                        }
                        if g.is_imported() {
                            p.rect_filled(rect, 6.0, Color32::from_black_alpha(130));
                            widgets::badge(
                                &p,
                                rect.center(),
                                Align2::CENTER_CENTER,
                                "가져옴",
                                10.0,
                                pal.success.gamma_multiply(0.9),
                                Color32::WHITE,
                            );
                        }
                        let sel = sess.selected.contains(&gi);
                        if sel {
                            p.rect_stroke(rect, 6.0, Stroke::new(3.0, pal.accent), StrokeKind::Inside);
                            let c = rect.right_bottom() - vec2(14.0, 14.0);
                            p.circle_filled(c, 9.0, pal.accent);
                            widgets::paint_icon(
                                &p,
                                Rect::from_center_size(c, egui::Vec2::splat(12.0)),
                                Icon::Check,
                                Color32::WHITE,
                            );
                        } else if resp.hovered() {
                            p.rect_stroke(rect, 6.0, Stroke::new(1.0, pal.text3), StrokeKind::Inside);
                        }
                        let resp = resp.on_hover_text(format!(
                            "{}\n{}\n{}",
                            g.files.iter().map(|f| f.name.clone()).collect::<Vec<_>>().join(" + "),
                            g.meta.captured_at.map(format::datetime_ko).unwrap_or_default(),
                            g.meta.camera().unwrap_or_default()
                        ));
                        if resp.clicked() && !g.is_imported() {
                            toggle.push(gi);
                        }
                    }
                }
                // prefetch
                for &(ry, date, a, b) in &rows {
                    if date.is_none() && ry > vp.max.y && ry < vp.max.y + vp.height() {
                        for vi in a..b {
                            thumbs.prefetch(sess.visible[vi]);
                        }
                    }
                }
            });
        for gi in toggle {
            if modifiers.command || modifiers.shift || !sess.selected.is_empty() {
                if !sess.selected.remove(&gi) {
                    sess.selected.insert(gi);
                }
            } else {
                sess.selected.insert(gi);
            }
        }
        if let Some((a, b)) = select_day {
            let ids: Vec<usize> =
                (a..b).map(|vi| sess.visible[vi]).filter(|&gi| !scan.groups[gi].is_imported()).collect();
            let all = ids.iter().all(|g| sess.selected.contains(g));
            for g in ids {
                if all {
                    sess.selected.remove(&g);
                } else {
                    sess.selected.insert(g);
                }
            }
        }
    }
}
