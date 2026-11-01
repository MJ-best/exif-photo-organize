use super::titlebar_pad;
use crate::app::{PhotoApp, SideSel};
use crate::layout::GridMode;
use crate::theme::{bold, font};
use crate::widgets::{self, Icon};
use eframe::egui::{self, pos2, vec2, Align2, Rect, Sense, Ui, UiBuilder};
use photoorg_core::format;

pub const TOOLBAR_H: f32 = 52.0;

impl PhotoApp {
    pub fn title(&self) -> String {
        let Some(st) = &self.lib else { return String::new() };
        match &self.side {
            SideSel::All | SideSel::None | SideSel::Device(_) => "라이브러리".into(),
            SideSel::Rated => "별점 있는 사진".into(),
            SideSel::Unrated => "아직 고르지 않은 사진".into(),
            SideSel::Recent => "최근 가져온 항목".into(),
            SideSel::Videos => "동영상".into(),
            SideSel::Pairs => "RAW+JPEG".into(),
            SideSel::Camera(c) => st.index.cameras.name(*c).to_string(),
            SideSel::Lens(l) => st.index.lenses.name(*l).to_string(),
            SideSel::Candidates => "B컷 후보".into(),
            SideSel::Rejected => "거절 (B컷)".into(),
            SideSel::Picked => "픽 (A컷)".into(),
            SideSel::BurstBest => "연사 베스트".into(),
            SideSel::Scene(s) => photoorg_core::cull::Scene::from_u8(*s).label().into(),
            SideSel::Import(id) => st
                .index
                .imports
                .iter()
                .find(|i| i.id == *id)
                .map(|i| format!("{} 가져오기", i.label))
                .unwrap_or_else(|| "가져오기".into()),
        }
    }

    pub fn subtitle(&self) -> String {
        let Some(st) = &self.lib else { return String::new() };
        if self.filtered.is_empty() {
            return "항목 없음".into();
        }
        let first = st.index.assets[self.filtered[0]].captured().date();
        let last = st.index.assets[*self.filtered.last().unwrap()].captured().date();
        let range = if first == last {
            format::date_medium(first)
        } else {
            format!("{} – {}", format::date_medium(first), format::date_medium(last))
        };
        let sel = if self.sel.ids.len() > 1 {
            format!(" · {}개 선택됨", format::count(self.sel.ids.len()))
        } else {
            String::new()
        };
        format!("{range} · {}장{sel}", format::count(self.filtered.len()))
    }

    pub fn toolbar(&mut self, ui: &mut Ui) {
        let pal = self.pal;
        let ctx = ui.ctx().clone();
        let h = TOOLBAR_H + titlebar_pad();
        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::click_and_drag());
        if cfg!(target_os = "macos") && resp.drag_started() {
            ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }
        let p = ui.painter();
        p.rect_filled(rect, 0.0, pal.toolbar_bg);
        widgets::separator_h(ui, rect, &pal);
        let bar = Rect::from_min_max(pos2(rect.left(), rect.top() + titlebar_pad()), rect.max);

        // Title.
        let title = self.title();
        let subtitle = self.subtitle();
        p.text(pos2(bar.left() + 18.0, bar.center().y - 8.0), Align2::LEFT_CENTER, &title, bold(15.0), pal.text);
        p.text(pos2(bar.left() + 18.0, bar.center().y + 10.0), Align2::LEFT_CENTER, &subtitle, font(11.5), pal.text2);

        // Right controls (fewer when the window is narrow).
        let wide = bar.width() > 1040.0;
        let search_w = if bar.width() > 860.0 { 180.0 } else { 120.0 };
        let right = Rect::from_min_max(pos2(bar.center().x + 130.0, bar.top()), pos2(bar.right() - 12.0, bar.bottom()));
        let mut rui =
            ui.new_child(UiBuilder::new().max_rect(right).layout(egui::Layout::right_to_left(egui::Align::Center)));
        rui.spacing_mut().item_spacing.x = 6.0;
        if widgets::icon_button(&mut rui, Icon::Info, self.cfg.show_info, &pal, "정보 (⌘I)").clicked() {
            self.cfg.show_info = !self.cfg.show_info;
        }
        let before = self.search.clone();
        let sresp = widgets::search_field(&mut rui, &mut self.search, search_w, &pal, "search");
        if self.focus_search {
            sresp.request_focus();
            self.focus_search = false;
        }
        if self.search != before {
            self.filter_changed();
        }
        let n = self.filter.active_count().saturating_sub(if self.search.is_empty() { 0 } else { 1 });
        let fresp = widgets::icon_button(&mut rui, Icon::Filter, self.show_filters || n > 0, &pal, "필터");
        if n > 0 {
            widgets::badge(
                rui.painter(),
                fresp.rect.right_top() + vec2(-2.0, 4.0),
                Align2::CENTER_CENTER,
                &n.to_string(),
                9.0,
                pal.accent,
                pal.accent_text,
            );
        }
        if fresp.clicked() {
            self.show_filters = !self.show_filters;
        }
        if wide && matches!(self.mode, GridMode::Days | GridMode::All) {
            rui.add_space(6.0);
            if widgets::icon_button(&mut rui, Icon::Plus, false, &pal, "크게 (⌘+)").clicked() {
                self.zoom_tiles(1.2);
            }
            let mut size = self.cfg.tile_size;
            rui.spacing_mut().slider_width = 90.0;
            if rui.add(egui::Slider::new(&mut size, 64.0..=420.0).show_value(false).logarithmic(true)).changed() {
                self.cfg.tile_size = size;
            }
            if widgets::icon_button(&mut rui, Icon::Minus, false, &pal, "작게 (⌘−)").clicked() {
                self.zoom_tiles(1.0 / 1.2);
            }
        }

        // Centre: segmented control.
        let seg_w = 250.0;
        let cx = if wide { bar.center().x } else { bar.center().x - 40.0 };
        let seg_rect = Rect::from_center_size(pos2(cx, bar.center().y), vec2(seg_w, 26.0));
        let mut cui =
            ui.new_child(UiBuilder::new().max_rect(seg_rect).layout(egui::Layout::left_to_right(egui::Align::Center)));
        let mut mode = self.mode;
        if widgets::segmented(
            &mut cui,
            &mut mode,
            &[(GridMode::Years, "연"), (GridMode::Months, "월"), (GridMode::Days, "일"), (GridMode::All, "모든 사진")],
            &pal,
        ) {
            self.set_mode(mode);
        }
    }
}
