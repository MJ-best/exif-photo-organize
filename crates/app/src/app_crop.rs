//! 크롭 · 수평 (crop and straighten) in the viewer: `R` opens the tool on the photo shown. The
//! framing is stored in the catalog and applied to how the photo is shown, to the culling
//! assistant's composition check and to JPEG exports; the files are never changed.

use crate::app::PhotoApp;
use crate::theme::{bold, font};
use crate::widgets;
use eframe::egui::{self, pos2, vec2, Align2, Color32, Id, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, UiBuilder};
use photoorg_core::framing::{self, Framing, MAX_ANGLE};

/// Shape the crop keeps while it is resized.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CropRatio {
    /// The photo's own.
    Original,
    Free,
    Square,
    R4x5,
    R3x2,
    R16x9,
}

impl CropRatio {
    pub const ALL: [(CropRatio, &'static str); 6] = [
        (CropRatio::Original, "원본 비율"),
        (CropRatio::Free, "자유"),
        (CropRatio::Square, "1:1"),
        (CropRatio::R4x5, "4:5"),
        (CropRatio::R3x2, "3:2"),
        (CropRatio::R16x9, "16:9"),
    ];

    /// Width / height in pixels, oriented like the photo (None = free).
    pub fn value(self, aspect: f32) -> Option<f32> {
        let r = match self {
            CropRatio::Original => return Some(aspect),
            CropRatio::Free => return None,
            CropRatio::Square => 1.0,
            CropRatio::R4x5 => 5.0 / 4.0,
            CropRatio::R3x2 => 3.0 / 2.0,
            CropRatio::R16x9 => 16.0 / 9.0,
        };
        Some(if aspect >= 1.0 { r } else { 1.0 / r })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Grip {
    Move,
    /// Corner: which side of x / y moves (true = right / bottom).
    Corner(bool, bool),
    /// Edge: 0 left, 1 top, 2 right, 3 bottom.
    Edge(u8),
}

pub struct CropSession {
    pub id: i64,
    pub f: Framing,
    pub ratio: CropRatio,
    /// Photo width / height (as displayed).
    pub aspect: f32,
    drag: Option<(Grip, Framing, Pos2)>,
}

const MIN_SIZE: f32 = 0.05;

impl PhotoApp {
    /// Open the crop tool on photo `id` (its current framing, if any).
    pub fn start_crop(&mut self, id: i64) {
        let Some(st) = &self.lib else { return };
        let Some(a) = st.index.get(id) else { return };
        if a.is_video() {
            return;
        }
        let swap = (5..=8).contains(&a.orientation);
        let (w, h) = if swap { (a.height, a.width) } else { (a.width, a.height) };
        let aspect = if w > 0 && h > 0 { w as f32 / h as f32 } else { 1.5 };
        let f = a.framing.unwrap_or_default();
        let ratio = if (f.aspect(aspect) - aspect).abs() < 0.01 { CropRatio::Original } else { CropRatio::Free };
        self.crop = Some(CropSession { id, f, ratio, aspect, drag: None });
        if let Some(v) = &mut self.viewer {
            v.zoom = None;
        }
    }

    /// Keep the framing being edited and close the tool.
    pub fn commit_crop(&mut self, ctx: &egui::Context) {
        let Some(s) = self.crop.take() else { return };
        let f = (!s.f.is_identity()).then_some(s.f);
        let Some(st) = &mut self.lib else { return };
        let before = st.index.get(s.id).and_then(|a| a.framing);
        if before == f {
            return;
        }
        if let Err(e) = st.catalog.set_framing(s.id, f) {
            self.toast(format!("저장 실패: {e:#}"), true);
            return;
        }
        if let Some(a) = st.index.get_mut(s.id) {
            a.framing = f;
        }
        self.info = None;
        // The composition check (and the B-cut reasons from it) follow the new framing.
        self.labels_dirty_at = Some(ctx.input(|i| i.time));
    }

    pub fn cancel_crop(&mut self) {
        self.crop = None;
    }

    /// Straighten automatically (and with `compose`, crop for a better composition).
    pub fn auto_crop(&mut self, compose: bool) {
        let Some(id) = self.crop.as_ref().map(|s| s.id) else { return };
        let off = self.cfg.learn.off;
        let Some(c) = self.composition(id).cloned() else {
            self.toast("아직 분석되지 않은 사진이라 자동으로 맞출 수 없습니다 — 분석이 끝난 뒤 다시 해 보세요", true);
            return;
        };
        let Some(s) = &mut self.crop else { return };
        let mut c = c;
        // Measured on the photo as it is (its aspect from the analysis may be rounded).
        c.aspect = s.aspect;
        let f = framing::suggest(&c, s.ratio.value(s.aspect), compose, off);
        let level = c.tilt_conf >= 0.3 && c.tilt.abs() >= 0.4 && c.tilt.abs() < 9.5;
        s.f = f;
        // Cropped beyond what turning needs: the composition crop.
        let level_only = framing::Framing::straightened(f.angle, s.aspect, s.ratio.value(s.aspect));
        let composed = compose && f.w * f.h < level_only.w * level_only.h - 1e-3;
        let msg = match (level, composed) {
            (true, true) => format!("{:.1}° 돌려 수평을 맞추고 구도에 맞춰 잘랐습니다", f.angle.abs()),
            (true, false) => format!("{:.1}° 돌려 수평을 맞췄습니다", f.angle.abs()),
            (false, true) => "구도에 맞춰 잘랐습니다".to_string(),
            (false, false) if compose => "기울기 · 구도 모두 괜찮아 그대로 둡니다".to_string(),
            (false, false) => "뚜렷한 기울기가 없어 그대로 둡니다".to_string(),
        };
        self.toast(msg, false);
    }

    /// Turn to `angle`, keeping the crop's place and (relative) size.
    fn crop_set_angle(&mut self, angle: f32) {
        let Some(s) = &mut self.crop else { return };
        let f = s.f;
        let (cx, cy) = (f.x + f.w / 2.0, f.y + f.h / 2.0);
        let ratio = s.ratio.value(s.aspect).or(Some(f.aspect(s.aspect)));
        let was = Framing::largest(f.angle, s.aspect, ratio, cx, cy);
        let rel = (f.w / was.w.max(1e-4)).min(1.0);
        let max = Framing::largest(angle, s.aspect, ratio, cx, cy);
        let (w, h) = (max.w * rel, max.h * rel);
        s.f = Framing { angle, x: cx - w / 2.0, y: cy - h / 2.0, w, h }.fit(s.aspect);
    }

    fn crop_set_ratio(&mut self, ratio: CropRatio) {
        let Some(s) = &mut self.crop else { return };
        s.ratio = ratio;
        if let Some(r) = ratio.value(s.aspect) {
            let f = s.f;
            s.f = Framing::largest(f.angle, s.aspect, Some(r), f.x + f.w / 2.0, f.y + f.h / 2.0);
            if s.f.w < 0.2 {
                s.f = Framing::straightened(f.angle, s.aspect, Some(r));
            }
        }
    }

    /// The crop tool, drawn over the image area `area` with the photo's texture `tex`.
    pub fn crop_view(&mut self, ui: &mut Ui, area: Rect, tex: Option<&egui::TextureHandle>) {
        let pal = self.pal;
        let Some(s) = &mut self.crop else { return };
        if let Some(t) = tex {
            // The texture knows the shape better than the catalog (e.g. no size recorded).
            let ta = t.size()[0] as f32 / t.size()[1].max(1) as f32;
            if (ta - s.aspect).abs() > 0.01 {
                s.aspect = ta;
                s.f = s.f.fit(ta);
            }
        }
        let aspect = s.aspect;
        let tools_h = 54.0;
        let canvas_area = Rect::from_min_max(area.min, pos2(area.max.x, area.max.y - tools_h)).shrink(28.0);
        let canvas = crate::views::grid::contain_rect([(aspect * 1000.0) as usize, 1000], canvas_area);
        let p = ui.painter_at(canvas);
        // The photo turned by the angle, inside the frame.
        if let Some(t) = tex {
            let mut m = egui::Mesh::with_texture(t.id());
            for (u, v) in [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)] {
                let (fx, fy) = s.f.photo_to_frame(aspect, u, v);
                let pos = pos2(canvas.left() + fx * canvas.width(), canvas.top() + fy * canvas.height());
                m.vertices.push(egui::epaint::Vertex { pos, uv: pos2(u, v), color: Color32::WHITE });
            }
            m.add_triangle(0, 1, 2);
            m.add_triangle(1, 3, 2);
            p.add(m);
        } else {
            p.text(canvas.center(), Align2::CENTER_CENTER, "불러오는 중…", font(13.0), pal.text2);
        }
        let to_screen = |x: f32, y: f32| pos2(canvas.left() + x * canvas.width(), canvas.top() + y * canvas.height());
        let crop = Rect::from_min_max(to_screen(s.f.x, s.f.y), to_screen(s.f.x + s.f.w, s.f.y + s.f.h));
        // Dim what is cut away.
        let dim = Color32::from_black_alpha(150);
        for r in [
            Rect::from_min_max(canvas.min, pos2(canvas.max.x, crop.min.y)),
            Rect::from_min_max(pos2(canvas.min.x, crop.max.y), canvas.max),
            Rect::from_min_max(pos2(canvas.min.x, crop.min.y), pos2(crop.min.x, crop.max.y)),
            Rect::from_min_max(pos2(crop.max.x, crop.min.y), pos2(canvas.max.x, crop.max.y)),
        ] {
            p.rect_filled(r, 0.0, dim);
        }
        // Thirds and a frame with grips.
        let fine = Stroke::new(1.0, Color32::from_white_alpha(110));
        for k in [1.0, 2.0] {
            let x = crop.left() + crop.width() * k / 3.0;
            let y = crop.top() + crop.height() * k / 3.0;
            p.line_segment([pos2(x, crop.top()), pos2(x, crop.bottom())], fine);
            p.line_segment([pos2(crop.left(), y), pos2(crop.right(), y)], fine);
        }
        let pp = ui.painter();
        pp.rect_stroke(crop, 0.0, Stroke::new(1.5, Color32::WHITE), StrokeKind::Middle);
        let g = 18.0f32.min(crop.width() / 3.0).min(crop.height() / 3.0);
        let thick = Stroke::new(4.0, Color32::WHITE);
        for (c, dx, dy) in [
            (crop.left_top(), 1.0, 1.0),
            (crop.right_top(), -1.0, 1.0),
            (crop.left_bottom(), 1.0, -1.0),
            (crop.right_bottom(), -1.0, -1.0),
        ] {
            pp.line_segment([c, c + vec2(dx * g, 0.0)], thick);
            pp.line_segment([c, c + vec2(0.0, dy * g)], thick);
        }
        if s.ratio == CropRatio::Free {
            for (a, b) in [
                (pos2(crop.center().x - g / 2.0, crop.top()), pos2(crop.center().x + g / 2.0, crop.top())),
                (pos2(crop.center().x - g / 2.0, crop.bottom()), pos2(crop.center().x + g / 2.0, crop.bottom())),
                (pos2(crop.left(), crop.center().y - g / 2.0), pos2(crop.left(), crop.center().y + g / 2.0)),
                (pos2(crop.right(), crop.center().y - g / 2.0), pos2(crop.right(), crop.center().y + g / 2.0)),
            ] {
                pp.line_segment([a, b], thick);
            }
        }
        // Size of the result.
        widgets::badge(
            pp,
            crop.left_top() + vec2(6.0, 6.0),
            Align2::LEFT_TOP,
            &format!("{:.0}% · {:.1}°", s.f.w * s.f.h * 100.0, s.f.angle),
            10.5,
            Color32::from_black_alpha(160),
            Color32::WHITE,
        );

        // Dragging the frame, a corner or an edge.
        let resp = ui.interact(canvas.expand(24.0), Id::unique(("crop", s.id)), Sense::drag());
        if resp.drag_started() {
            if let Some(at) = resp.interact_pointer_pos() {
                let near = |q: Pos2| (q - at).length() <= 22.0;
                let grip = [
                    (crop.left_top(), false, false),
                    (crop.right_top(), true, false),
                    (crop.left_bottom(), false, true),
                    (crop.right_bottom(), true, true),
                ]
                .iter()
                .find(|(q, _, _)| near(*q))
                .map(|(_, r, b)| Grip::Corner(*r, *b))
                .or_else(|| {
                    if s.ratio != CropRatio::Free {
                        return None;
                    }
                    let inside_x = at.x > crop.left() && at.x < crop.right();
                    let inside_y = at.y > crop.top() && at.y < crop.bottom();
                    if inside_y && (at.x - crop.left()).abs() < 12.0 {
                        Some(Grip::Edge(0))
                    } else if inside_x && (at.y - crop.top()).abs() < 12.0 {
                        Some(Grip::Edge(1))
                    } else if inside_y && (at.x - crop.right()).abs() < 12.0 {
                        Some(Grip::Edge(2))
                    } else if inside_x && (at.y - crop.bottom()).abs() < 12.0 {
                        Some(Grip::Edge(3))
                    } else {
                        None
                    }
                })
                .or_else(|| crop.contains(at).then_some(Grip::Move));
                s.drag = grip.map(|g| (g, s.f, at));
            }
        }
        if let (Some((grip, start, at0)), Some(at)) = (s.drag, resp.interact_pointer_pos()) {
            if resp.dragged() {
                let d = (at - at0) / canvas.size();
                let ratio = s.ratio.value(aspect);
                let want = match grip {
                    Grip::Move => Framing { x: start.x + d.x, y: start.y + d.y, ..start },
                    Grip::Corner(right, bottom) => {
                        // The opposite corner stays.
                        let ax = if right { start.x } else { start.x + start.w };
                        let ay = if bottom { start.y } else { start.y + start.h };
                        let cx = if right { start.x + start.w + d.x } else { start.x + d.x };
                        let cy = if bottom { start.y + start.h + d.y } else { start.y + d.y };
                        let mut w = (cx - ax).abs().max(MIN_SIZE);
                        let mut h = (cy - ay).abs().max(MIN_SIZE);
                        if let Some(r) = ratio {
                            // Follow the larger movement.
                            if w * aspect / r >= h {
                                h = w * aspect / r;
                            } else {
                                w = h * r / aspect;
                            }
                        }
                        let x = if right { ax } else { ax - w };
                        let y = if bottom { ay } else { ay - h };
                        Framing { x, y, w, h, ..start }
                    }
                    Grip::Edge(e) => {
                        let mut f = start;
                        match e {
                            0 => {
                                let x = (start.x + d.x).min(start.x + start.w - MIN_SIZE);
                                f.w = start.x + start.w - x;
                                f.x = x;
                            }
                            1 => {
                                let y = (start.y + d.y).min(start.y + start.h - MIN_SIZE);
                                f.h = start.y + start.h - y;
                                f.y = y;
                            }
                            2 => f.w = (start.w + d.x).max(MIN_SIZE),
                            _ => f.h = (start.h + d.y).max(MIN_SIZE),
                        }
                        f
                    }
                };
                // As far towards the wanted framing as the photo allows.
                let lerp = |t: f32| Framing {
                    angle: start.angle,
                    x: start.x + (want.x - start.x) * t,
                    y: start.y + (want.y - start.y) * t,
                    w: start.w + (want.w - start.w) * t,
                    h: start.h + (want.h - start.h) * t,
                };
                s.f = if want.inside(aspect) {
                    want
                } else {
                    let (mut lo, mut hi) = (0.0f32, 1.0f32);
                    for _ in 0..24 {
                        let mid = (lo + hi) / 2.0;
                        if lerp(mid).inside(aspect) {
                            lo = mid;
                        } else {
                            hi = mid;
                        }
                    }
                    lerp(lo)
                };
            }
        }
        if resp.drag_stopped() {
            s.drag = None;
        }

        // Tools under the photo.
        let tools = Rect::from_min_max(
            pos2(area.left() + 12.0, area.bottom() - tools_h),
            pos2(area.right() - 12.0, area.bottom()),
        );
        let mut tui =
            ui.new_child(UiBuilder::new().max_rect(tools).layout(egui::Layout::left_to_right(egui::Align::Center)));
        tui.spacing_mut().item_spacing.x = 8.0;
        tui.label(egui::RichText::new("수평").font(bold(12.0)).color(pal.text2));
        let mut angle = s.f.angle;
        tui.spacing_mut().slider_width = 170.0;
        let slider = tui
            .add(egui::Slider::new(&mut angle, -MAX_ANGLE..=MAX_ANGLE).step_by(0.1).fixed_decimals(1).suffix("°"))
            .on_hover_text("끌어서 돌리기 (두 번 누르면 0°)");
        let reset_angle = slider.double_clicked();
        let mut set_angle = None;
        if slider.changed() {
            set_angle = Some(angle);
        }
        if reset_angle {
            set_angle = Some(0.0);
        }
        let auto_level = widgets::secondary_button(&mut tui, "자동 수평", &pal)
            .on_hover_text("셀렉 도우미가 찾은 세로선 · 수평선으로 기울기를 맞추고, 빈 모서리가 없게 자릅니다");
        let auto_compose = widgets::secondary_button(&mut tui, "자동 구도", &pal).on_hover_text(
            "수평을 맞추고, 3분할 · 헤드룸 · 시선 앞 공간이 나아지도록 조금만 자릅니다 (끈 구도 기준은 빼고)",
        );
        let mut ratio = s.ratio;
        egui::ComboBox::from_id_salt("crop-ratio")
            .selected_text(CropRatio::ALL.iter().find(|r| r.0 == ratio).map(|r| r.1).unwrap_or(""))
            .width(96.0)
            .show_ui(&mut tui, |ui| {
                for (r, label) in CropRatio::ALL {
                    ui.selectable_value(&mut ratio, r, label);
                }
            });
        let ratio_changed = ratio != s.ratio;
        let reset = widgets::secondary_button(&mut tui, "원래대로", &pal);
        let mut done = false;
        let mut cancel = false;
        tui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            done = widgets::primary_button(ui, "완료 (Enter)", &pal, true).clicked();
            cancel = widgets::secondary_button(ui, "취소 (Esc)", &pal).clicked();
        });
        if let Some(a) = set_angle {
            self.crop_set_angle(a);
        }
        if ratio_changed {
            self.crop_set_ratio(ratio);
        }
        if reset.clicked() {
            if let Some(s) = &mut self.crop {
                s.f = Framing::default();
                s.ratio = CropRatio::Original;
            }
        }
        if auto_level.clicked() {
            self.auto_crop(false);
        }
        if auto_compose.clicked() {
            self.auto_crop(true);
        }
        let ctx = ui.ctx().clone();
        if done {
            self.commit_crop(&ctx);
        } else if cancel {
            self.cancel_crop();
        }
    }
}
