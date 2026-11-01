use crate::app::{PhotoApp, ScrollReq};
use crate::layout::{CardLayout, GridLayout, GridMode, GAP, HEADER_H, SIDE_PAD};
use crate::theme::{bold, font};
use crate::widgets::{self, Icon};
use eframe::egui::{
    self, epaint, pos2, vec2, Align2, Color32, Id, Mesh, Pos2, Rect, Sense, Stroke, StrokeKind, TextureHandle, Ui,
};
use photoorg_core::format;

/// UV rectangle that crops a texture to fill `target` (cover) .
pub fn cover_uv(tex: [usize; 2], target: egui::Vec2) -> Rect {
    let ta = tex[0] as f32 / tex[1].max(1) as f32;
    let ra = target.x / target.y.max(1.0);
    if ta > ra {
        let w = ra / ta;
        Rect::from_min_max(pos2((1.0 - w) / 2.0, 0.0), pos2((1.0 + w) / 2.0, 1.0))
    } else {
        let h = ta / ra;
        Rect::from_min_max(pos2(0.0, (1.0 - h) / 2.0), pos2(1.0, (1.0 + h) / 2.0))
    }
}

/// Rect of an image fitted (contain) into `outer`.
pub fn contain_rect(tex: [usize; 2], outer: Rect) -> Rect {
    let ta = tex[0] as f32 / tex[1].max(1) as f32;
    let oa = outer.width() / outer.height().max(1.0);
    let size =
        if ta > oa { vec2(outer.width(), outer.width() / ta) } else { vec2(outer.height() * ta, outer.height()) };
    Rect::from_center_size(outer.center(), size)
}

pub fn paint_tex_rounded(p: &egui::Painter, tex: &TextureHandle, rect: Rect, uv: Rect, radius: f32) {
    p.add(epaint::RectShape::filled(rect, radius, Color32::WHITE).with_texture(tex.id(), uv));
}

/// Vertical gradient (top colour → bottom colour).
pub fn gradient(p: &egui::Painter, rect: Rect, top: Color32, bottom: Color32) {
    let mut m = Mesh::default();
    m.colored_vertex(rect.left_top(), top);
    m.colored_vertex(rect.right_top(), top);
    m.colored_vertex(rect.left_bottom(), bottom);
    m.colored_vertex(rect.right_bottom(), bottom);
    m.add_triangle(0, 1, 2);
    m.add_triangle(1, 3, 2);
    p.add(m);
}

impl PhotoApp {
    pub fn browse(&mut self, ui: &mut Ui) {
        if self.filtered.is_empty() {
            self.empty_state(ui);
            return;
        }
        match self.mode {
            GridMode::Years | GridMode::Months => self.cards(ui),
            GridMode::Days | GridMode::All => self.grid(ui),
        }
    }

    fn empty_state(&mut self, ui: &mut Ui) {
        let pal = self.pal;
        let r = ui.available_rect_before_wrap();
        let c = r.center();
        let p = ui.painter();
        widgets::paint_icon(
            p,
            Rect::from_center_size(c - vec2(0.0, 50.0), egui::Vec2::splat(54.0)),
            Icon::Photos,
            pal.text3,
        );
        let total = self.lib.as_ref().map(|s| s.index.assets.len()).unwrap_or(0);
        let (t, s) = if total == 0 {
            ("사진이 없습니다", "SD 카드를 연결하거나 왼쪽의 ‘폴더에서 가져오기…’를 사용하세요.")
        } else {
            ("일치하는 사진이 없습니다", "필터를 바꾸거나 지워 보세요.")
        };
        p.text(c + vec2(0.0, 6.0), Align2::CENTER_CENTER, t, bold(18.0), pal.text2);
        p.text(c + vec2(0.0, 32.0), Align2::CENTER_CENTER, s, font(13.0), pal.text3);
    }

    fn grid(&mut self, ui: &mut Ui) {
        let pal = self.pal;
        let width = ui.available_width() - 2.0;
        let target = self.cfg.tile_size;
        let key = (self.filter_gen, width as u32, target as u32, self.mode);
        if self.grid_layout.as_ref().map(|l| l.key) != Some(key) {
            let st = self.lib.as_ref().unwrap();
            self.grid_layout =
                Some(GridLayout::build(&st.index, &self.filtered, width, target, self.mode, self.filter_gen));
        }
        let layout = self.grid_layout.take().unwrap();
        self.last_grid_cols = layout.cols;
        let view_h = ui.available_height();

        let mut sa = egui::ScrollArea::vertical().id_salt(("grid", self.mode as u8)).auto_shrink([false, false]);
        if let Some(req) = self.scroll_req.take() {
            let off = match req {
                ScrollReq::Top => Some(0.0),
                ScrollReq::Bottom => Some((layout.height - view_h).max(0.0)),
                ScrollReq::Item(p) => layout.y_of_item(p).map(|y| {
                    // Keep the current offset if the item is already visible.
                    let cur = self.grid_offset;
                    if y >= cur && y + layout.tile <= cur + view_h {
                        cur
                    } else {
                        (y - view_h / 2.0 + layout.tile / 2.0).max(0.0)
                    }
                }),
                ScrollReq::Date(d) => layout.y_of_date(d).map(|y| y.max(0.0)),
            };
            if let Some(o) = off {
                sa = sa.vertical_scroll_offset(o.min((layout.height - view_h).max(0.0)));
            }
        }

        let modifiers = ui.input(|i| i.modifiers);
        let mut clicked: Option<(usize, i64)> = None;
        let mut double: Option<i64> = None;
        let mut context_rate: Option<(Vec<i64>, u8)> = None;
        let mut reveal: Option<i64> = None;
        let mut top_date: Option<chrono::NaiveDate> = None;
        let out = sa.show_viewport(ui, |ui, viewport| {
            ui.set_height(layout.height);
            ui.set_width(width);
            let origin = ui.max_rect().min;
            let p = ui.painter().clone();
            // Headers (Days).
            for h in &layout.headers {
                if h.y + HEADER_H < viewport.min.y || h.y > viewport.max.y {
                    continue;
                }
                let y = origin.y + h.y;
                p.text(pos2(origin.x + SIDE_PAD + 14.0, y + 24.0), Align2::LEFT_CENTER, &h.title, bold(17.0), pal.text);
                p.text(
                    pos2(origin.x + SIDE_PAD + 14.0, y + 44.0),
                    Align2::LEFT_CENTER,
                    &h.subtitle,
                    font(12.0),
                    pal.text2,
                );
            }
            let st = self.lib.as_ref().unwrap();
            let thumbs = self.thumbs.as_mut().unwrap();
            let range = layout.visible_rows(viewport.min.y, viewport.max.y);
            if let Some(r) = layout.rows.get(range.start) {
                top_date = Some(st.index.assets[self.filtered[r.start]].captured().date());
            }
            for ri in range.clone() {
                let row = &layout.rows[ri];
                for (col, fi) in (row.start..row.end).enumerate() {
                    let a = &st.index.assets[self.filtered[fi]];
                    let rect = Rect::from_min_size(
                        pos2(origin.x + layout.tile_rect_x(col), origin.y + row.y),
                        egui::Vec2::splat(layout.tile),
                    );
                    let resp = ui.interact(rect, Id::unique(("tile", a.id)), Sense::click());
                    p.rect_filled(rect, 0.0, pal.placeholder);
                    if let Some(tex) = thumbs.get(a.id) {
                        if self.cfg.square_tiles {
                            p.image(tex.id(), rect, cover_uv(tex.size(), rect.size()), Color32::WHITE);
                        } else {
                            p.image(
                                tex.id(),
                                contain_rect(tex.size(), rect.shrink(2.0)),
                                Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                                Color32::WHITE,
                            );
                        }
                    } else if a.is_video() {
                        widgets::paint_icon(
                            &p,
                            Rect::from_center_size(rect.center(), egui::Vec2::splat(rect.width() * 0.28)),
                            Icon::Video,
                            pal.text3,
                        );
                    } else if thumbs.is_missing(a.id) && layout.tile > 70.0 {
                        p.text(rect.center(), Align2::CENTER_CENTER, &a.name, font(10.5), pal.text3);
                    }
                    // Overlays.
                    if layout.tile >= 70.0 {
                        if a.is_video() {
                            widgets::paint_icon(
                                &p,
                                Rect::from_min_size(rect.right_bottom() - vec2(22.0, 18.0), vec2(16.0, 14.0)),
                                Icon::Video,
                                Color32::WHITE,
                            );
                        }
                        if a.has_raw() {
                            let label =
                                if a.formats & photoorg_core::media::formats::JPEG != 0 { "RAW+J" } else { "RAW" };
                            widgets::badge(
                                &p,
                                rect.right_top() + vec2(-5.0, 5.0),
                                Align2::RIGHT_TOP,
                                label,
                                8.5,
                                Color32::from_black_alpha(140),
                                Color32::WHITE,
                            );
                        }
                        if a.rating > 0 {
                            gradient(
                                &p,
                                Rect::from_min_max(pos2(rect.left(), rect.bottom() - 24.0), rect.right_bottom()),
                                Color32::TRANSPARENT,
                                Color32::from_black_alpha(90),
                            );
                            widgets::paint_stars(
                                &p,
                                rect.left_bottom() + vec2(5.0, -5.0),
                                a.rating,
                                11.0,
                                pal.star,
                                true,
                            );
                        }
                    } else if a.rating > 0 {
                        p.circle_filled(rect.left_bottom() + vec2(6.0, -6.0), 3.0, pal.star);
                    }
                    let selected = self.sel.ids.contains(&a.id);
                    if selected {
                        p.rect_stroke(rect, 0.0, Stroke::new(3.0, pal.accent), StrokeKind::Inside);
                        p.rect_stroke(
                            rect.shrink(3.0),
                            0.0,
                            Stroke::new(1.0, Color32::from_white_alpha(160)),
                            StrokeKind::Inside,
                        );
                    } else if resp.hovered() {
                        p.rect_filled(rect, 0.0, Color32::from_white_alpha(18));
                    }
                    if self.sel.focus == Some(a.id) && self.sel.ids.len() > 1 {
                        p.rect_stroke(rect.shrink(5.0), 0.0, Stroke::new(1.5, Color32::WHITE), StrokeKind::Inside);
                    }
                    if resp.double_clicked() {
                        double = Some(a.id);
                    } else if resp.clicked() {
                        clicked = Some((fi, a.id));
                    }
                    let id = a.id;
                    let rating = a.rating;
                    let sel_ids: Vec<i64> = if selected { self.sel.ids.iter().copied().collect() } else { vec![id] };
                    resp.context_menu(|ui| {
                        ui.label(
                            egui::RichText::new(if sel_ids.len() > 1 {
                                format!("{}개 항목", sel_ids.len())
                            } else {
                                "별점".into()
                            })
                            .font(bold(12.0)),
                        );
                        for r in (0..=5u8).rev() {
                            let label = if r == 0 {
                                "별점 지우기 (0)".to_string()
                            } else {
                                format!("{}  (F{r})", "★".repeat(r as usize))
                            };
                            if ui.selectable_label(sel_ids.len() == 1 && rating == r && r > 0, label).clicked() {
                                context_rate = Some((sel_ids.clone(), r));
                                ui.close();
                            }
                        }
                        ui.separator();
                        if ui.button("파일 위치 보기").clicked() {
                            reveal = Some(id);
                            ui.close();
                        }
                    });
                }
            }
            // Prefetch one screen above and below.
            let extra = viewport.height();
            for ri in layout
                .visible_rows(viewport.max.y, viewport.max.y + extra)
                .chain(layout.visible_rows((viewport.min.y - extra).max(0.0), viewport.min.y))
            {
                let row = &layout.rows[ri];
                for fi in row.start..row.end {
                    thumbs.prefetch(st.index.assets[self.filtered[fi]].id);
                }
            }
        });

        self.grid_offset = out.state.clamped_offset().y;
        // Floating date in "All Photos".
        if self.mode == GridMode::All {
            if let Some(d) = top_date {
                if out.state.clamped_offset().y > 20.0 {
                    let r = out.inner_rect;
                    let p = ui.painter_at(r);
                    gradient(
                        &p,
                        Rect::from_min_max(r.min, pos2(r.right(), r.top() + 56.0)),
                        Color32::from_black_alpha(150),
                        Color32::TRANSPARENT,
                    );
                    p.text(
                        r.left_top() + vec2(18.0, 22.0),
                        Align2::LEFT_CENTER,
                        format::date_medium(d),
                        bold(17.0),
                        Color32::WHITE,
                    );
                }
            }
        }
        // Ctrl/Cmd + scroll wheel zooms the grid.
        if ui.rect_contains_pointer(out.inner_rect) {
            let (zoom, cmd) = ui.input(|i| (i.zoom_delta(), i.modifiers.command));
            if zoom != 1.0 && cmd {
                self.cfg.tile_size = (self.cfg.tile_size * zoom).clamp(64.0, 420.0);
            }
        }

        self.grid_layout = Some(layout);
        if let Some((fi, id)) = clicked {
            if modifiers.command {
                if !self.sel.ids.remove(&id) {
                    self.sel.ids.insert(id);
                }
                self.sel.focus = Some(id);
                self.sel.anchor = Some(fi);
            } else if modifiers.shift {
                self.set_focus(fi, true);
                self.scroll_req = None;
            } else {
                self.sel.ids = [id].into();
                self.sel.focus = Some(id);
                self.sel.anchor = Some(fi);
            }
        }
        if let Some(id) = double {
            self.open_viewer(id);
        }
        if let Some((ids, r)) = context_rate {
            let ctx = ui.ctx().clone();
            self.rate(&ctx, ids, r);
        }
        if let Some(id) = reveal {
            self.reveal_asset(id);
        }
    }

    pub fn reveal_asset(&mut self, id: i64) {
        let Some(st) = &self.lib else { return };
        let files = st.catalog.files_for(id).unwrap_or_default();
        if let Some(f) = files.first() {
            let p = st.lib.abs(&f.rel_path);
            self.reveal(&p);
        }
    }

    fn cards(&mut self, ui: &mut Ui) {
        let pal = self.pal;
        let width = ui.available_width() - 2.0;
        let key = (self.filter_gen, width as u32, self.mode);
        if self.card_layout.as_ref().map(|l| l.key) != Some(key) {
            let st = self.lib.as_ref().unwrap();
            self.card_layout = Some(CardLayout::build(&st.index, &self.filtered, width, self.mode, self.filter_gen));
        }
        let layout = self.card_layout.take().unwrap();
        let view_h = ui.available_height();
        let mut sa = egui::ScrollArea::vertical().id_salt(("cards", self.mode as u8)).auto_shrink([false, false]);
        if let Some(req) = self.scroll_req.take() {
            let off = match req {
                ScrollReq::Bottom => (layout.height - view_h).max(0.0),
                ScrollReq::Date(d) => {
                    let label = format!("{}년", chrono::Datelike::year(&d));
                    layout.headers.iter().find(|h| h.1 == label).map(|h| h.0).unwrap_or(0.0)
                }
                _ => 0.0,
            };
            sa = sa.vertical_scroll_offset(off);
        }
        let mut open: Option<(GridMode, chrono::NaiveDate)> = None;
        sa.show_viewport(ui, |ui, viewport| {
            ui.set_height(layout.height);
            ui.set_width(width);
            let origin = ui.max_rect().min;
            let p = ui.painter().clone();
            for (y, title) in &layout.headers {
                if *y + 44.0 < viewport.min.y || *y > viewport.max.y {
                    continue;
                }
                p.text(pos2(origin.x + 22.0, origin.y + y + 22.0), Align2::LEFT_CENTER, title, bold(22.0), pal.text);
            }
            let st = self.lib.as_ref().unwrap();
            let thumbs = self.thumbs.as_mut().unwrap();
            for c in &layout.cards {
                if c.y + c.h < viewport.min.y - 200.0 || c.y > viewport.max.y + 200.0 {
                    continue;
                }
                let rect = Rect::from_min_size(pos2(origin.x + c.x, origin.y + c.y), vec2(c.w, c.h));
                let resp = ui.interact(rect, Id::unique(("card", self.mode as u8, c.date)), Sense::click());
                let a = &st.index.assets[self.filtered[c.key]];
                p.rect_filled(rect, 12.0, pal.placeholder);
                if let Some(tex) = thumbs.get(a.id) {
                    paint_tex_rounded(&p, &tex, rect, cover_uv(tex.size(), rect.size()), 12.0);
                } else if a.is_video() {
                    widgets::paint_icon(
                        &p,
                        Rect::from_center_size(rect.center(), egui::Vec2::splat(48.0)),
                        Icon::Video,
                        pal.text3,
                    );
                }
                let top = Rect::from_min_max(rect.min, pos2(rect.right(), rect.top() + rect.height().min(110.0)));
                let mut m = Mesh::default();
                m.colored_vertex(top.left_top() + vec2(0.0, 0.0), Color32::from_black_alpha(140));
                m.colored_vertex(top.right_top(), Color32::from_black_alpha(140));
                m.colored_vertex(top.left_bottom(), Color32::TRANSPARENT);
                m.colored_vertex(top.right_bottom(), Color32::TRANSPARENT);
                m.add_triangle(0, 1, 2);
                m.add_triangle(1, 3, 2);
                p.with_clip_rect(rect.shrink(1.0)).add(m);
                let big = if self.mode == GridMode::Years { 30.0 } else { 22.0 };
                p.text(
                    rect.left_top() + vec2(16.0, 14.0 + big / 2.0),
                    Align2::LEFT_CENTER,
                    &c.title,
                    bold(big),
                    Color32::WHITE,
                );
                p.text(
                    rect.left_top() + vec2(17.0, 22.0 + big),
                    Align2::LEFT_CENTER,
                    &c.subtitle,
                    font(12.5),
                    Color32::from_white_alpha(220),
                );
                if resp.hovered() {
                    p.rect_stroke(rect, 12.0, Stroke::new(2.0, Color32::from_white_alpha(70)), StrokeKind::Inside);
                }
                if resp.clicked() {
                    open = Some((if self.mode == GridMode::Years { GridMode::Months } else { GridMode::Days }, c.date));
                }
            }
        });
        self.card_layout = Some(layout);
        if let Some((m, d)) = open {
            self.mode = m;
            self.scroll_req = Some(ScrollReq::Date(d));
        }
        let _ = (GAP, Pos2::ZERO);
    }
}
