use super::grid::contain_rect;
use super::titlebar_pad;
use crate::app::{PhotoApp, PreviewKey};
use crate::theme::{bold, font};
use crate::widgets::{self, Icon};
use eframe::egui::{self, pos2, vec2, Align2, Color32, Id, Rect, Sense, Stroke, StrokeKind, Ui, UiBuilder};
use photoorg_core::format;

const FILMSTRIP_H: f32 = 70.0;

impl PhotoApp {
    pub fn viewer_view(&mut self, ui: &mut Ui) {
        let pal = self.pal;
        let ctx = ui.ctx().clone();
        let Some(v) = &self.viewer else { return };
        let id = v.id;
        let zoom = v.zoom;
        let Some(st) = &self.lib else { return };
        let Some(a) = st.index.get(id).cloned() else {
            self.viewer = None;
            return;
        };
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
        let mut bui =
            ui.new_child(UiBuilder::new().max_rect(inner).layout(egui::Layout::left_to_right(egui::Align::Center)));
        let back = bui.add(egui::Button::new(egui::RichText::new("‹  돌아가기").font(font(13.0))).frame(false));
        if back.clicked() {
            self.close_viewer();
            return;
        }
        let captured = a.captured();
        let title = format::datetime_ko(captured);
        let sub = format!(
            "{}  ·  {}{}",
            a.name,
            photoorg_core::media::formats::label(a.formats),
            pos.map(|p| format!("  ·  {} / {}", p + 1, self.filtered.len())).unwrap_or_default()
        );
        let tp = ui.painter();
        tp.text(pos2(inner.center().x, inner.center().y - 8.0), Align2::CENTER_CENTER, &title, bold(14.0), pal.text);
        tp.text(pos2(inner.center().x, inner.center().y + 10.0), Align2::CENTER_CENTER, &sub, font(11.5), pal.text2);
        let right = Rect::from_min_max(pos2(inner.center().x + 180.0, inner.top()), inner.max);
        let mut rui =
            ui.new_child(UiBuilder::new().max_rect(right).layout(egui::Layout::right_to_left(egui::Align::Center)));
        rui.spacing_mut().item_spacing.x = 8.0;
        if widgets::icon_button(&mut rui, Icon::Info, self.cfg.show_info, &pal, "정보 (⌘I)").clicked() {
            self.cfg.show_info = !self.cfg.show_info;
        }
        let zr = widgets::icon_button(&mut rui, Icon::Search, zoom.is_some(), &pal, "100% 확대 (Z)");
        if zr.clicked() {
            if let Some(v) = &mut self.viewer {
                v.zoom = if v.zoom.is_some() { None } else { Some(vec2(0.5, 0.5)) };
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
            rui.add_space(4.0);
        }
        let rate_to = widgets::stars(&mut rui, a.rating, 18.0, &pal);

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
        let full_tex = if zoom.is_some() { previews.get(PreviewKey { id, edge: max_side.min(16384) }) } else { None };
        // Prefetch neighbours for fast culling.
        if let Some(p) = pos {
            for d in [1i64, -1, 2] {
                let np = p as i64 + d;
                if np >= 0 && (np as usize) < self.filtered.len() {
                    let nid = st.index.assets[self.filtered[np as usize]].id;
                    previews.prefetch(PreviewKey { id: nid, edge: fit_edge });
                }
            }
        }
        let resp = ui.interact(area, Id::unique(("viewer-img", id)), Sense::click_and_drag());
        let p = ui.painter_at(area);
        let aspect_tex = fit_tex.as_ref().or(thumb.as_ref()).map(|t| t.size());
        match (zoom, &full_tex) {
            (Some(center), Some(tex)) => {
                // 100%: one texture pixel per physical pixel.
                let size = vec2(tex.size()[0] as f32, tex.size()[1] as f32) / ppp;
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
                p.image(tex.id(), rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
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
                    p.image(tex.id(), r, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
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
        } else {
            None
        };
        let has_status = status.is_some();
        if let Some((icon, text, color)) = status {
            let y = if zoom.is_some() { 40.0 } else { 12.0 };
            let g = ui.painter().layout_no_wrap(text, bold(11.5), Color32::WHITE);
            let r = Rect::from_min_size(area.left_top() + vec2(14.0, y), vec2(g.size().x + 38.0, 26.0));
            let p = ui.painter_at(area);
            p.rect_filled(r, 13.0, Color32::from_black_alpha(160));
            widgets::paint_icon(
                &p,
                Rect::from_center_size(pos2(r.left() + 16.0, r.center().y), vec2(14.0, 14.0)),
                icon,
                color,
            );
            p.galley(pos2(r.left() + 30.0, r.center().y - g.size().y / 2.0), g, Color32::WHITE);
        }
        // Themes of the active project on this photo.
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
                let y = if zoom.is_some() { 40.0 } else { 12.0 } + if has_status { 32.0 } else { 0.0 };
                let g = ui.painter().layout_no_wrap(text, bold(11.5), Color32::WHITE);
                let r = Rect::from_min_size(area.left_top() + vec2(14.0, y), vec2(g.size().x + 38.0, 26.0));
                let p = ui.painter_at(area);
                p.rect_filled(r, 13.0, Color32::from_black_alpha(160));
                widgets::paint_icon(
                    &p,
                    Rect::from_center_size(pos2(r.left() + 16.0, r.center().y), vec2(14.0, 14.0)),
                    Icon::Tag,
                    pal.accent,
                );
                p.galley(pos2(r.left() + 30.0, r.center().y - g.size().y / 2.0), g, Color32::WHITE);
            }
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
                        super::grid::paint_tex_rounded(&pp, &t, r, super::grid::cover_uv(t.size(), r.size()), 4.0);
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
