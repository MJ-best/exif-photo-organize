use super::grid::{contain_rect, framed_size, paint_framed, FULL};
use super::titlebar_pad;
use crate::app::{PhotoApp, PreviewKey};
use crate::theme::{bold, font};
use crate::widgets::{self, Icon};
use chrono::Datelike;
use eframe::egui::{self, pos2, vec2, Align2, Color32, Id, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, UiBuilder};
use photoorg_core::cull::composition::{self, Composition};
use photoorg_core::cull::learn::reason;
use photoorg_core::format;

const FILMSTRIP_H: f32 = 70.0;

impl PhotoApp {
    pub fn viewer_view(&mut self, ui: &mut Ui) {
        let pal = self.pal;
        let ctx = ui.ctx().clone();
        let Some(v) = &self.viewer else { return };
        let (id, vzoom) = (v.id, v.zoom);
        if self.cfg.show_composition {
            self.composition(id);
        }
        let Some(st) = &self.lib else { return };
        let Some(a) = st.index.get(id).cloned() else {
            self.viewer = None;
            return;
        };
        // Videos are shown fitted; the zoom stays for the next photo.
        let zoom = if a.is_video() { None } else { vzoom };
        let pos = self.filtered_pos.get(&id).copied();
        let full = ui.available_rect_before_wrap();
        ui.painter().rect_filled(full, 0.0, pal.viewer_bg);

        // Top bar.
        let bar_h = 52.0 + titlebar_pad();
        let bar = Rect::from_min_size(full.min, vec2(full.width(), bar_h));
        let bresp = ui.interact(bar, Id::unique("viewer-bar"), Sense::click_and_drag());
        if cfg!(target_os = "macos") && bresp.drag_started() {
            ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }
        ui.painter().rect_filled(bar, 0.0, pal.toolbar_bg);
        widgets::separator_h(ui, bar, &pal);
        let inner = Rect::from_min_max(
            pos2(bar.left() + 10.0, bar.top() + titlebar_pad()),
            pos2(bar.right() - 12.0, bar.bottom()),
        );
        // A narrow viewer (window small, sidebar and info panel open): tighter tools, shorter title.
        let narrow = inner.width() < 640.0;
        // The tools first, from the right; the title gets the room that is left.
        let mut rui =
            ui.new_child(UiBuilder::new().max_rect(inner).layout(egui::Layout::right_to_left(egui::Align::Center)));
        rui.spacing_mut().item_spacing.x = if narrow { 2.0 } else { 6.0 };
        if widgets::icon_button(&mut rui, Icon::Info, self.cfg.show_info, &pal, "정보 (⌘I)").clicked() {
            self.cfg.show_info = !self.cfg.show_info;
        }
        let crop_clicked = !a.is_video()
            && widgets::icon_button(&mut rui, Icon::Crop, self.crop.is_some(), &pal, "크롭 · 수평 (R)").clicked();
        if !a.is_video()
            && widgets::icon_button(
                &mut rui,
                Icon::Thirds,
                self.cfg.show_composition,
                &pal,
                "구도 보기 (G) — 3분할 · 피사체 · 헤드룸 · 시선 · 머리 뒤 선 · 수평",
            )
            .clicked()
        {
            self.cfg.show_composition = !self.cfg.show_composition;
        }
        let zr = widgets::icon_button(&mut rui, Icon::Search, zoom.is_some(), &pal, "100% 확대 (Z · 스페이스 두 번)");
        if zr.clicked() {
            if let Some(v) = &mut self.viewer {
                v.zoom = if v.zoom.is_some() { None } else { Some(crate::app::ZOOM_CENTER) };
            }
        }
        let mut flag_to = None;
        if !a.is_video() {
            if widgets::icon_button(&mut rui, Icon::Reject, a.flag < 0, &pal, "거절 — B컷 (X)").clicked() {
                flag_to = Some(if a.flag < 0 { 0 } else { -1 });
            }
            if widgets::icon_button(&mut rui, Icon::Flag, a.flag > 0, &pal, "픽 — A컷 (P)").clicked() {
                flag_to = Some(if a.flag > 0 { 0 } else { 1 });
            }
            rui.add_space(if narrow { 2.0 } else { 6.0 });
        }
        let rate_to = widgets::stars(&mut rui, a.rating, if narrow { 14.0 } else { 18.0 }, &pal);
        let tools_left = rui.min_rect().left();
        let mut bui =
            ui.new_child(UiBuilder::new().max_rect(inner).layout(egui::Layout::left_to_right(egui::Align::Center)));
        let back = bui
            .add(
                egui::Button::new(egui::RichText::new(if narrow { "‹" } else { "‹  돌아가기" }).font(font(13.0)))
                    .frame(false),
            )
            .on_hover_text("돌아가기 (Esc · Space)");
        if back.clicked() {
            self.close_viewer();
            return;
        }
        // Title between the two: on the middle of the bar when it fits there, otherwise in the
        // room that is left — the longest wording that fits, then cut with "…".
        let (left, right) = (back.rect.right() + 14.0, tools_left - 14.0);
        if right - left >= 48.0 {
            let room = right - left;
            let captured = a.captured();
            let place = pos.map(|p| format!("{} / {}", p + 1, self.filtered.len()));
            let titles = [
                format::datetime_ko(captured),
                format!("{} {}", format::date_medium(captured.date()), format::time_ko(captured)),
                format::date_medium(captured.date()),
                format!("{}. {}. {}.", captured.year(), captured.month(), captured.day()),
            ];
            let kind = photoorg_core::media::formats::label(a.formats);
            let mut subs = vec![match &place {
                Some(pl) => format!("{}  ·  {kind}  ·  {pl}", a.name),
                None => format!("{}  ·  {kind}", a.name),
            }];
            if let Some(pl) = &place {
                subs.push(format!("{}  ·  {pl}", a.name));
                subs.push(pl.clone());
            }
            let fit = |texts: &[String], f: egui::FontId, color: Color32| {
                let p = ui.painter();
                let one = |t: &str, wrap: f32| {
                    let mut job = egui::text::LayoutJob::single_section(
                        t.to_owned(),
                        egui::TextFormat { font_id: f.clone(), color, ..Default::default() },
                    );
                    job.wrap = egui::text::TextWrapping::truncate_at_width(wrap);
                    p.layout_job(job)
                };
                texts
                    .iter()
                    .map(|t| one(t, f32::INFINITY))
                    .find(|g| g.size().x <= room)
                    .unwrap_or_else(|| one(texts.last().map(|s| s.as_str()).unwrap_or(""), room))
            };
            let tg = fit(&titles, bold(14.0), pal.text);
            let sg = fit(&subs, font(11.5), pal.text2);
            let w = tg.size().x.max(sg.size().x);
            let mid = inner.center().x;
            let cx = if mid - w / 2.0 >= left && mid + w / 2.0 <= right { mid } else { (left + right) / 2.0 };
            let tp = ui.painter();
            let (ty, sy) = (inner.center().y - 8.0, inner.center().y + 10.0);
            tp.galley(pos2(cx - tg.size().x / 2.0, ty - tg.size().y / 2.0), tg, pal.text);
            tp.galley(pos2(cx - sg.size().x / 2.0, sy - sg.size().y / 2.0), sg, pal.text2);
        }

        // Image area.
        let strip_h = if self.cfg.show_filmstrip { FILMSTRIP_H } else { 0.0 };
        // Under the photo, above the filmstrip: the feedback strip (see `overlays`).
        let band_bottom = full.bottom() - strip_h;
        let band_top = band_bottom - super::STATUS_H;
        self.status_band = Some(Rect::from_min_max(pos2(full.left(), band_top), pos2(full.right(), band_bottom)));
        let area = Rect::from_min_max(pos2(full.left(), bar.bottom()), pos2(full.right(), band_top));
        let img_area = area.shrink(16.0);
        let ppp = ctx.pixels_per_point();
        let max_side = ctx.input(|i| i.max_texture_side) as u32;
        let fit_edge = ((img_area.width().max(img_area.height()) * ppp) as u32).clamp(512, 4096);
        // Bucket so that resizing the window doesn't trigger constant reloads.
        let fit_edge = fit_edge.div_ceil(512) * 512;
        let thumb = self.thumbs.as_mut().and_then(|t| t.get(id));
        let previews = self.previews.as_mut().unwrap();
        let fit_tex = previews.get(PreviewKey { id, edge: fit_edge });
        let full_edge = max_side.min(16384);
        let full_tex = if zoom.is_some() { previews.get(PreviewKey { id, edge: full_edge }) } else { None };
        // Prefetch neighbours for fast culling; zoomed in, the next one at full size too (the zoom
        // stays when stepping).
        if let Some(p) = pos {
            for d in [1i64, -1, 2] {
                let np = p as i64 + d;
                if np >= 0 && (np as usize) < self.filtered.len() {
                    let nid = st.index.assets[self.filtered[np as usize]].id;
                    previews.prefetch(PreviewKey { id: nid, edge: fit_edge });
                    if d == 1 && zoom.is_some() {
                        previews.prefetch(PreviewKey { id: nid, edge: full_edge });
                    }
                }
            }
        }
        if crop_clicked {
            if self.crop.is_some() {
                self.commit_crop(&ctx);
            } else {
                self.start_crop(id);
            }
        }
        // The crop tool takes over the image area.
        if self.crop.as_ref().is_some_and(|c| c.id == id) {
            let tex = fit_tex.clone().or(thumb.clone());
            self.crop_view(ui, area, tex.as_ref());
            if let Some(r) = rate_to {
                self.rate(&ctx, vec![id], r);
            }
            if let Some(f) = flag_to {
                self.set_flag(&ctx, vec![id], f);
            }
            return;
        }
        let resp = ui.interact(area, Id::unique(("viewer-img", id)), Sense::click_and_drag());
        let p = ui.painter_at(area);
        let framing = a.framing.filter(|f| !f.is_identity());
        let fr = framing.as_ref();
        let aspect_tex = fit_tex.as_ref().or(thumb.as_ref()).map(|t| framed_size(t.size(), fr));
        // Where the photo is drawn (for the composition overlay).
        let mut shown: Option<Rect> = None;
        match (zoom, &full_tex) {
            (Some(center), Some(tex)) => {
                // 100%: one texture pixel per physical pixel (of the framed photo).
                let fs = framed_size(tex.size(), fr);
                let size = vec2(fs[0] as f32, fs[1] as f32) / ppp;
                let mut c = center;
                if resp.dragged() {
                    let d = resp.drag_delta();
                    c -= vec2(d.x / size.x, d.y / size.y);
                }
                c = vec2(c.x.clamp(0.0, 1.0), c.y.clamp(0.0, 1.0));
                let min = area.center() - vec2(c.x * size.x, c.y * size.y);
                let mut rect = Rect::from_min_size(min, size);
                // Center when smaller than the area.
                if size.x < area.width() {
                    rect = rect.translate(vec2(area.center().x - rect.center().x, 0.0));
                }
                if size.y < area.height() {
                    rect = rect.translate(vec2(0.0, area.center().y - rect.center().y));
                }
                paint_framed(&p, tex, rect, fr, FULL, 0.0);
                shown = Some(rect);
                if let Some(v) = &mut self.viewer {
                    v.zoom = Some(c);
                }
                if resp.clicked() {
                    if let Some(v) = &mut self.viewer {
                        v.zoom = None;
                    }
                }
                widgets::badge(
                    &p,
                    area.left_top() + vec2(14.0, 12.0),
                    Align2::LEFT_TOP,
                    "100%",
                    10.0,
                    Color32::from_black_alpha(150),
                    Color32::WHITE,
                );
            }
            _ => {
                let tex = fit_tex.as_ref().or(thumb.as_ref());
                if let (Some(tex), Some(sz)) = (tex, aspect_tex) {
                    let r = contain_rect(sz, img_area);
                    paint_framed(&p, tex, r, fr, FULL, 0.0);
                    shown = Some(r);
                    if zoom.is_some() {
                        p.text(
                            area.center(),
                            Align2::CENTER_CENTER,
                            "원본 해상도 불러오는 중…",
                            font(13.0),
                            Color32::WHITE,
                        );
                    }
                    if resp.clicked() && !a.is_video() {
                        if let (Some(pp), Some(v)) = (resp.interact_pointer_pos(), &mut self.viewer) {
                            let n = (pp - r.min) / r.size();
                            v.zoom = Some(vec2(n.x.clamp(0.0, 1.0), n.y.clamp(0.0, 1.0)));
                        }
                    }
                } else if a.is_video() {
                    widgets::paint_icon(
                        &p,
                        Rect::from_center_size(area.center(), egui::Vec2::splat(80.0)),
                        Icon::Video,
                        pal.text3,
                    );
                    p.text(
                        area.center() + vec2(0.0, 64.0),
                        Align2::CENTER_CENTER,
                        "동영상은 미리보기를 지원하지 않습니다 — 파일 위치에서 재생하세요",
                        font(13.0),
                        pal.text2,
                    );
                } else if self.previews.as_ref().is_some_and(|pc| pc.is_missing(PreviewKey { id, edge: fit_edge })) {
                    p.text(
                        area.center(),
                        Align2::CENTER_CENTER,
                        "미리보기를 만들 수 없습니다 (HEIF 등 미지원 형식)",
                        font(13.0),
                        pal.text2,
                    );
                } else {
                    p.text(area.center(), Align2::CENTER_CENTER, "불러오는 중…", font(13.0), pal.text2);
                }
            }
        }
        if resp.double_clicked() {
            self.close_viewer();
            return;
        }
        // Pick / reject / B-cut candidate status.
        let status = if a.flag < 0 {
            Some((Icon::Reject, "거절 (B컷)".to_string(), pal.danger))
        } else if a.flag > 0 {
            Some((Icon::Flag, "픽 (A컷)".to_string(), pal.success))
        } else if self.cfg.cull_badges
            && photoorg_core::filter::CullFilter::Candidates(self.cfg.cull_threshold).matches(&a)
        {
            let why = photoorg_core::cull::learn::reason::labels(a.reasons);
            let mut t = format!("B컷 후보 {:.0}%", a.b_prob * 100.0);
            if !why.is_empty() {
                t.push_str(" · ");
                t.push_str(&why.join(", "));
            }
            Some((Icon::Suspect, t, pal.warning))
        } else if self.cfg.cull_badges && photoorg_core::filter::CullFilter::Keepers(self.cfg.keep_pct).matches(&a) {
            let good = composition::strength::labels(a.strengths);
            let mut t = "A컷 후보".to_string();
            if !good.is_empty() {
                t.push_str(" · ");
                t.push_str(&good.join(", "));
            }
            Some((Icon::Keeper, t, pal.success))
        } else {
            None
        };
        // Status and the active project's themes, as badges in the top left: measured first so
        // the composition labels keep clear of them, painted last so nothing crosses them.
        let mut badge_rows: Vec<(Icon, Color32, std::sync::Arc<egui::Galley>)> = Vec::new();
        if let Some((icon, text, color)) = status {
            badge_rows.push((icon, color, one_line(ui.painter(), text, bold(11.5), area.width() - 70.0)));
        }
        if let (Some(pid), Some(st)) = (self.active_project, &self.lib) {
            let names: Vec<String> = st
                .index
                .themes_of(pid)
                .iter()
                .enumerate()
                .filter(|(_, t)| a.tags.contains(&t.id))
                .map(|(i, t)| format!("{} {}", i + 1, t.short_name()))
                .collect();
            if !names.is_empty() {
                let pname = st.index.project(pid).map(|p| p.name.as_str()).unwrap_or("");
                let text = format!("{pname}: {}", names.join(" · "));
                badge_rows.push((Icon::Tag, pal.accent, one_line(ui.painter(), text, bold(11.5), area.width() - 70.0)));
            }
        }
        let top = if zoom.is_some() { 40.0 } else { 12.0 };
        let badges: Vec<Rect> = badge_rows
            .iter()
            .enumerate()
            .map(|(k, (_, _, g))| {
                Rect::from_min_size(area.left_top() + vec2(14.0, top + 32.0 * k as f32), vec2(g.size().x + 38.0, 26.0))
            })
            .collect();
        if let (true, Some(r), false) = (self.cfg.show_composition, shown, a.is_video()) {
            let p = ui.painter_at(area);
            match self.comp_cache.as_ref().filter(|c| c.0 == id).and_then(|c| c.1.as_ref()) {
                // Judged as framed.
                Some(c) => {
                    let c = framing.map(|f| f.composition(c)).unwrap_or_else(|| c.clone());
                    paint_composition(&p, r, &c, self.cfg.learn.off, &badges)
                }
                None => {
                    let msg = if self.analysis_job.is_some() {
                        "구도: 분석 중…"
                    } else {
                        "구도: 아직 분석되지 않은 사진"
                    };
                    widgets::badge(
                        &p,
                        area.left_bottom() + vec2(14.0, -12.0),
                        Align2::LEFT_BOTTOM,
                        msg,
                        11.0,
                        Color32::from_black_alpha(150),
                        Color32::WHITE,
                    );
                }
            }
        }
        let p = ui.painter_at(area);
        for ((icon, color, g), r) in badge_rows.into_iter().zip(&badges) {
            p.rect_filled(*r, 13.0, Color32::from_black_alpha(160));
            widgets::paint_icon(
                &p,
                Rect::from_center_size(pos2(r.left() + 16.0, r.center().y), vec2(14.0, 14.0)),
                icon,
                color,
            );
            p.galley(pos2(r.left() + 30.0, r.center().y - g.size().y / 2.0), g, Color32::WHITE);
        }
        if let Some(r) = rate_to {
            self.rate(&ctx, vec![id], r);
        }
        if let Some(f) = flag_to {
            self.set_flag(&ctx, vec![id], f);
        }
        // Prev / next hover arrows.
        let hover = resp.hover_pos();
        for (dir, x) in [(-1i64, area.left() + 30.0), (1, area.right() - 30.0)] {
            let r = Rect::from_center_size(pos2(x, area.center().y), vec2(40.0, 64.0));
            let near = hover.is_some_and(|h| (h.x - x).abs() < 90.0);
            let can = pos.is_some_and(|p| if dir < 0 { p > 0 } else { p + 1 < self.filtered.len() });
            if near && can {
                let r2 = ui.interact(r, Id::unique(("nav", dir)), Sense::click());
                ui.painter().rect_filled(r, 10.0, Color32::from_black_alpha(if r2.hovered() { 150 } else { 90 }));
                widgets::paint_icon(
                    ui.painter(),
                    r.shrink2(vec2(10.0, 22.0)),
                    if dir < 0 { Icon::Left } else { Icon::Right },
                    Color32::WHITE,
                );
                if r2.clicked() {
                    self.viewer_step(dir);
                }
            }
        }

        // Film strip.
        if self.cfg.show_filmstrip {
            let strip = Rect::from_min_max(pos2(full.left(), full.bottom() - FILMSTRIP_H), full.max);
            ui.painter().rect_filled(strip, 0.0, pal.toolbar_bg);
            ui.painter().line_segment([strip.left_top(), strip.right_top()], Stroke::new(1.0, pal.separator));
            if let Some(cur) = pos {
                let th = FILMSTRIP_H - 14.0;
                let tw = th;
                let n_side = ((strip.width() / (tw + 4.0)) / 2.0).ceil() as i64 + 1;
                let st = self.lib.as_ref().unwrap();
                let thumbs = self.thumbs.as_mut().unwrap();
                let mut jump = None;
                for d in -n_side..=n_side {
                    let np = cur as i64 + d;
                    if np < 0 || np as usize >= self.filtered.len() {
                        continue;
                    }
                    let a2 = &st.index.assets[self.filtered[np as usize]];
                    let x = strip.center().x + d as f32 * (tw + 4.0);
                    let r = Rect::from_center_size(pos2(x, strip.center().y), vec2(tw, th));
                    if !strip.intersects(r) {
                        continue;
                    }
                    let pp = ui.painter_at(strip);
                    pp.rect_filled(r, 4.0, pal.placeholder);
                    if let Some(t) = thumbs.get(a2.id) {
                        let fr = a2.framing.as_ref();
                        let uv = super::grid::cover_uv(super::grid::framed_size(t.size(), fr), r.size());
                        super::grid::paint_framed(&pp, &t, r, fr, uv, 4.0);
                    }
                    if a2.flag < 0 {
                        pp.rect_filled(r, 4.0, Color32::from_black_alpha(150));
                        pp.circle_filled(r.left_top() + vec2(6.0, 6.0), 3.5, pal.danger);
                    } else if a2.flag > 0 {
                        pp.circle_filled(r.left_top() + vec2(6.0, 6.0), 3.5, pal.success);
                    }
                    if a2.rating > 0 {
                        widgets::paint_stars(&pp, r.left_bottom() + vec2(3.0, -3.0), a2.rating, 7.0, pal.star, true);
                    }
                    if d == 0 {
                        pp.rect_stroke(r.expand(1.5), 5.0, Stroke::new(2.5, pal.accent), StrokeKind::Outside);
                    } else {
                        let rr = ui.interact(r, Id::unique(("strip", a2.id)), Sense::click());
                        if rr.hovered() {
                            pp.rect_filled(r, 4.0, Color32::from_white_alpha(25));
                        }
                        if rr.clicked() {
                            jump = Some(d);
                        }
                    }
                }
                if let Some(d) = jump {
                    self.viewer_step(d);
                }
            }
        }
        // Scroll wheel / swipe to navigate (when not zoomed).
        if zoom.is_none() && ui.rect_contains_pointer(area) {
            let dx = ctx.input(|i| i.smooth_scroll_delta);
            let acc_id = Id::unique("viewer-scroll-acc");
            let mut acc: f32 = ctx.data(|d| d.get_temp(acc_id).unwrap_or(0.0));
            acc += if dx.x.abs() > dx.y.abs() { -dx.x } else { -dx.y };
            if acc.abs() > 120.0 {
                self.viewer_step(acc.signum() as i64);
                acc = 0.0;
            }
            ctx.data_mut(|d| d.insert_temp(acc_id, acc));
        }
    }
}

/// The composition check drawn over the photo shown in `r`: thirds, the subject and the point the
/// composition hangs on, headroom, where the face looks, background lines merging with the head
/// and the strongest straight line. Rules turned off (`off`, [`reason`] bits) are not drawn.
fn paint_composition(p: &egui::Painter, r: Rect, c: &Composition, off: u32, badges: &[Rect]) {
    use composition::subject;
    let j = composition::judge(c);
    let at = |x: f32, y: f32| pos2(r.left() + x * r.width(), r.top() + y * r.height());
    let bad = Color32::from_rgb(255, 128, 72);
    let good = Color32::from_rgb(110, 225, 140);
    let soft = Color32::from_white_alpha(150);
    let verdict = |v: f32| if v >= 0.5 { bad } else { good };
    let line = |a: Pos2, b: Pos2, col: Color32, w: f32| {
        p.line_segment([a, b], Stroke::new(w + 2.0, Color32::from_black_alpha(80)));
        p.line_segment([a, b], Stroke::new(w, col));
    };
    // Labels: inside the photo, clear of each other and of the status badges.
    let mut placed: Vec<Rect> = badges.to_vec();
    let bounds = r.shrink(4.0).intersect(p.clip_rect().shrink(4.0));
    let mut tag = |pos: Pos2, align: Align2, text: &str, col: Color32| {
        let g = p.layout_no_wrap(text.to_owned(), bold(10.5), col);
        let size = g.size() + vec2(10.0, 4.0);
        let mut b = align.anchor_size(pos, size);
        let keep_in = |b: Rect| {
            let dx = (bounds.left() - b.left()).max(0.0) + (bounds.right() - b.right()).min(0.0);
            let dy = (bounds.top() - b.top()).max(0.0) + (bounds.bottom() - b.bottom()).min(0.0);
            b.translate(vec2(dx, dy))
        };
        b = keep_in(b);
        for _ in 0..8 {
            let Some(o) = placed.iter().find(|o| o.expand(2.0).intersects(b)).copied() else { break };
            // Below what is in the way; above it near the bottom.
            let down = b.translate(vec2(0.0, o.bottom() + 3.0 - b.top()));
            b = if down.bottom() <= bounds.bottom() {
                down
            } else {
                b.translate(vec2(0.0, o.top() - 3.0 - b.bottom()))
            };
            b = keep_in(b);
        }
        p.rect_filled(b, b.height() / 2.0, Color32::from_black_alpha(165));
        p.galley(b.center() - g.size() / 2.0, g, col);
        placed.push(b);
    };
    let on = |rule: u32| off & rule == 0;

    if on(reason::THIRDS) {
        for v in [1.0 / 3.0, 2.0 / 3.0] {
            line(at(v, 0.0), at(v, 1.0), Color32::from_white_alpha(90), 1.0);
            line(at(0.0, v), at(1.0, v), Color32::from_white_alpha(90), 1.0);
        }
    }
    if c.subject != subject::NONE {
        let b = Rect::from_min_max(at(c.bx, c.by), at(c.bx + c.bw, c.by + c.bh));
        p.rect_stroke(b, 3.0, Stroke::new(1.5, soft), StrokeKind::Middle);
        let k = at(c.kx, c.ky);
        let col = if on(reason::THIRDS) { verdict(j.thirds) } else { soft };
        p.circle_stroke(k, 8.0, Stroke::new(3.5, Color32::from_black_alpha(80)));
        p.circle_stroke(k, 8.0, Stroke::new(2.0, col));
        line(k - vec2(13.0, 0.0), k + vec2(13.0, 0.0), col, 1.5);
        line(k - vec2(0.0, 13.0), k + vec2(0.0, 13.0), col, 1.5);
    }
    if c.is_portrait() {
        if let (true, Some(ht)) = (on(reason::HEADROOM), c.head_top) {
            let col = verdict(j.headroom);
            let x = c.face_x;
            line(at(x - 0.07, ht), at(x + 0.07, ht), col, 2.0);
            if ht > 0.01 {
                line(at(x, 0.0), at(x, ht), col, 1.5);
            }
            let text = if c.face_h >= 0.3 {
                format!("클로즈업 · 눈높이 {:.0}%", c.ky * 100.0)
            } else {
                format!("헤드룸 {:.0}%", ht * 100.0)
            };
            tag(at(x + 0.075, ht), Align2::LEFT_CENTER, &text, col);
        }
        if let (true, Some(front)) = (on(reason::LEAD_ROOM), c.front_room()) {
            let col = verdict(j.lead_room);
            let k = at(c.kx, c.ky);
            let dir = c.facing.signum();
            let end = k + vec2(dir * r.width() * 0.12, 0.0);
            line(k + vec2(dir * 10.0, 0.0), end, col, 2.5);
            line(end, end + vec2(-dir * 9.0, -6.0), col, 2.5);
            line(end, end + vec2(-dir * 9.0, 6.0), col, 2.5);
            tag(
                end + vec2(dir * 6.0, 0.0),
                if dir > 0.0 { Align2::LEFT_CENTER } else { Align2::RIGHT_CENTER },
                &format!("시선 앞 {:.0}%", front * 100.0),
                col,
            );
        }
        if on(reason::MERGER) && j.merger >= 0.5 {
            if c.pole >= 0.5 && c.pole >= c.neck_line {
                let top = c.head_top.unwrap_or(c.ky);
                line(at(c.pole_x, (top - 0.35).max(0.0)), at(c.pole_x, top), bad, 3.0);
                tag(at(c.pole_x, (top - 0.35).max(0.0)), Align2::CENTER_BOTTOM, "머리 위로 솟은 선", bad);
            } else {
                line(at(0.0, c.neck_y), at(1.0, c.neck_y), bad, 2.5);
                tag(at(0.02, c.neck_y), Align2::LEFT_BOTTOM, "머리 · 목을 가로지르는 선", bad);
            }
        }
    }
    if on(reason::TILT) && c.tilt_conf >= 0.3 && c.tilt.abs() < 9.5 && (j.tilt > 0.0 || c.tilt.abs() < 0.4) {
        let col = verdict(j.tilt);
        let t = c.tilt.to_radians().tan();
        let (a, b) = if c.tilt_vertical {
            (at(c.tilt_at + 0.5 * t / c.aspect, 0.0), at(c.tilt_at - 0.5 * t / c.aspect, 1.0))
        } else {
            (at(0.0, c.tilt_at - 0.5 * t * c.aspect), at(1.0, c.tilt_at + 0.5 * t * c.aspect))
        };
        line(a, b, col, 1.5);
        let kind = if c.tilt_vertical { "수직" } else { "수평" };
        let text = if c.tilt.abs() < 0.4 {
            format!("{kind} 맞음")
        } else {
            format!("{kind} {:.1}° 기울어짐", c.tilt.abs())
        };
        tag(if c.tilt_vertical { b - vec2(0.0, 30.0) } else { b - vec2(8.0, 0.0) }, Align2::RIGHT_BOTTOM, &text, col);
    }
}

/// One line of white text, cut with "…" to `width`.
fn one_line(p: &egui::Painter, text: String, f: egui::FontId, width: f32) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::single_section(
        text,
        egui::TextFormat { font_id: f, color: Color32::WHITE, ..Default::default() },
    );
    job.wrap = egui::text::TextWrapping::truncate_at_width(width.max(40.0));
    p.layout_job(job)
}
