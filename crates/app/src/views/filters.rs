use crate::app::PhotoApp;
use crate::theme::{bold, font};
use crate::widgets;
use eframe::egui::{self, vec2, Popup, PopupCloseBehavior, Rect, Sense, Ui};
use photoorg_core::cull::Scene;
use photoorg_core::filter::{CullFilter, FormatFilter, RatingFilter};
use photoorg_core::format;
use photoorg_core::tags;

#[derive(Clone, Copy)]
enum Num {
    Iso,
    Focal,
    Aperture,
    Shutter,
}

impl PhotoApp {
    pub fn filter_bar(&mut self, ui: &mut Ui) {
        let pal = self.pal;
        if self.lib.is_none() {
            return;
        }
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 42.0), Sense::hover());
        ui.painter().rect_filled(rect, 0.0, pal.toolbar_bg);
        widgets::separator_h(ui, rect, &pal);
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(rect.shrink2(vec2(16.0, 8.0)))
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        let ui = &mut child;
        ui.spacing_mut().item_spacing.x = 6.0;
        let _ = self.facets();
        let mut changed = false;

        // Camera
        let label = self.set_label(
            "카메라",
            &self
                .filter
                .cameras
                .iter()
                .map(|c| self.lib.as_ref().unwrap().index.cameras.name(*c).to_string())
                .collect::<Vec<_>>(),
        );
        let r = widgets::pill(ui, &label, !self.filter.cameras.is_empty(), &pal);
        Popup::from_toggle_button_response(&r).close_behavior(PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
            ui.set_min_width(260.0);
            let st = self.lib.as_ref().unwrap();
            let fc = self.facets.as_ref().unwrap();
            egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                for (c, n) in &fc.cameras {
                    let mut on = self.filter.cameras.contains(c);
                    if ui.checkbox(&mut on, format!("{}  ({})", st.index.cameras.name(*c), format::count(*n))).changed()
                    {
                        if on {
                            self.filter.cameras.insert(*c);
                        } else {
                            self.filter.cameras.remove(c);
                        }
                        changed = true;
                    }
                }
            });
        });

        // Lens
        let label = self.set_label(
            "렌즈",
            &self
                .filter
                .lenses
                .iter()
                .map(|c| self.lib.as_ref().unwrap().index.lenses.name(*c).to_string())
                .collect::<Vec<_>>(),
        );
        let r = widgets::pill(ui, &label, !self.filter.lenses.is_empty(), &pal);
        Popup::from_toggle_button_response(&r).close_behavior(PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
            ui.set_min_width(300.0);
            let st = self.lib.as_ref().unwrap();
            let fc = self.facets.as_ref().unwrap();
            if fc.lenses.is_empty() {
                ui.label(egui::RichText::new("렌즈 정보가 없습니다").color(pal.text2));
            }
            egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                for (c, n) in &fc.lenses {
                    let mut on = self.filter.lenses.contains(c);
                    if ui.checkbox(&mut on, format!("{}  ({})", st.index.lenses.name(*c), format::count(*n))).changed()
                    {
                        if on {
                            self.filter.lenses.insert(*c);
                        } else {
                            self.filter.lenses.remove(c);
                        }
                        changed = true;
                    }
                }
            });
        });

        for num in [Num::Iso, Num::Focal, Num::Aperture, Num::Shutter] {
            let (name, active, summary) = match num {
                Num::Iso => (
                    "ISO",
                    self.filter.iso.is_some(),
                    self.filter.iso.map(|(a, b)| range_txt(a.to_string(), b.to_string())),
                ),
                Num::Focal => (
                    "초점거리",
                    self.filter.focal.is_some(),
                    self.filter.focal.map(|(a, b)| range_txt(format::focal(a), format::focal(b))),
                ),
                Num::Aperture => (
                    "조리개",
                    self.filter.aperture.is_some(),
                    self.filter.aperture.map(|(a, b)| range_txt(format::aperture(a), format::aperture(b))),
                ),
                Num::Shutter => (
                    "셔터",
                    self.filter.shutter.is_some(),
                    self.filter.shutter.map(|(a, b)| range_txt(format::shutter(b), format::shutter(a))),
                ),
            };
            let label = match summary {
                Some(s) => format!("{name} {s}"),
                None => name.to_string(),
            };
            let r = widgets::pill(ui, &label, active, &pal);
            Popup::from_toggle_button_response(&r).close_behavior(PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
                ui.set_min_width(300.0);
                ui.set_max_width(360.0);
                changed |= self.numeric_popup(ui, num);
            });
        }

        // Rating
        let label = match self.filter.rating {
            RatingFilter::Any => "별점".to_string(),
            RatingFilter::AtLeast(n) => format!("★{n} 이상"),
            RatingFilter::Exactly(n) => format!("★{n}"),
            RatingFilter::Unrated => "별점 없음".to_string(),
        };
        let r = widgets::pill(ui, &label, self.filter.rating != RatingFilter::Any, &pal);
        Popup::from_toggle_button_response(&r).show(|ui| {
            ui.set_min_width(200.0);
            let rc = self.facets.as_ref().unwrap().ratings;
            let at_least = |n: usize| rc[n..].iter().sum::<usize>();
            let opts: Vec<(RatingFilter, String)> = vec![
                (RatingFilter::Any, format!("전체  ({})", format::count(rc.iter().sum()))),
                (RatingFilter::AtLeast(1), format!("★ 1개 이상  ({})", at_least(1))),
                (RatingFilter::AtLeast(2), format!("★★ 2개 이상  ({})", at_least(2))),
                (RatingFilter::AtLeast(3), format!("★★★ 3개 이상  ({})", at_least(3))),
                (RatingFilter::AtLeast(4), format!("★★★★ 4개 이상  ({})", at_least(4))),
                (RatingFilter::Exactly(5), format!("★★★★★ 5개  ({})", rc[5])),
                (RatingFilter::Unrated, format!("별점 없음  ({})", rc[0])),
            ];
            for (v, l) in opts {
                if ui.selectable_label(self.filter.rating == v, l).clicked() {
                    self.filter.rating = v;
                    changed = true;
                }
            }
        });

        // Format
        let r = widgets::pill(
            ui,
            if self.filter.format == FormatFilter::Any { "형식" } else { self.filter.format.label() },
            self.filter.format != FormatFilter::Any,
            &pal,
        );
        Popup::from_toggle_button_response(&r).show(|ui| {
            ui.set_min_width(200.0);
            let counts = self.facets.as_ref().unwrap().formats.clone();
            for (ff, n) in counts {
                let l = if ff == FormatFilter::Any {
                    "전체".to_string()
                } else {
                    format!("{}  ({})", ff.label(), format::count(n))
                };
                if ui.selectable_label(self.filter.format == ff, l).clicked() {
                    self.filter.format = ff;
                    changed = true;
                }
            }
        });

        // Culling (pick / reject / B-cut candidates)
        let threshold = self.cfg.cull_threshold;
        let r = widgets::pill(
            ui,
            &if self.filter.cull == CullFilter::Any { "셀렉".to_string() } else { self.filter.cull.label() },
            self.filter.cull != CullFilter::Any,
            &pal,
        );
        Popup::from_toggle_button_response(&r).show(|ui| {
            ui.set_min_width(220.0);
            let c = self.facets.as_ref().unwrap().cull;
            let opts = [
                (CullFilter::Any, "전체".to_string()),
                (CullFilter::Candidates(threshold), format!("B컷 후보 ≥{threshold}%  ({})", format::count(c[0]))),
                (CullFilter::Rejected, format!("거절 (B컷)  ({})", format::count(c[1]))),
                (CullFilter::Picked, format!("픽 (A컷)  ({})", format::count(c[2]))),
                (CullFilter::Undecided, format!("아직 안 고름  ({})", format::count(c[3]))),
                (CullFilter::BurstBest, format!("연사 베스트만  ({})", format::count(c[4]))),
            ];
            for (v, l) in opts {
                let on = std::mem::discriminant(&self.filter.cull) == std::mem::discriminant(&v);
                if ui.selectable_label(on, l).clicked() {
                    self.filter.cull = v;
                    changed = true;
                }
            }
        });

        // Scene
        let scene_names: Vec<String> =
            self.filter.scenes.iter().map(|s| Scene::from_u8(*s).label().to_string()).collect();
        let label = self.set_label("장면", &scene_names);
        let r = widgets::pill(ui, &label, !self.filter.scenes.is_empty(), &pal);
        Popup::from_toggle_button_response(&r).close_behavior(PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
            ui.set_min_width(200.0);
            let fc = self.facets.as_ref().unwrap();
            if fc.scenes.is_empty() {
                ui.label(egui::RichText::new("아직 분석된 사진이 없습니다").color(pal.text2));
            }
            for (s, n) in fc.scenes.clone() {
                let mut on = self.filter.scenes.contains(&s);
                if ui.checkbox(&mut on, format!("{}  ({})", Scene::from_u8(s).label(), format::count(n))).changed() {
                    if on {
                        self.filter.scenes.insert(s);
                    } else {
                        self.filter.scenes.remove(&s);
                    }
                    changed = true;
                }
            }
        });

        // Tags
        let tag_names: Vec<String> =
            self.filter.tags.iter().map(|t| self.lib.as_ref().unwrap().index.tag_name(*t).to_string()).collect();
        let label = self.set_label("태그", &tag_names.iter().map(|t| tags::split(t).1.to_string()).collect::<Vec<_>>());
        let r = widgets::pill(ui, &label, !self.filter.tags.is_empty(), &pal);
        Popup::from_toggle_button_response(&r).close_behavior(PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
            ui.set_min_width(320.0);
            ui.set_max_width(420.0);
            let st = self.lib.as_ref().unwrap();
            let fc = self.facets.as_ref().unwrap();
            ui.label(egui::RichText::new("선택한 태그를 모두 가진 사진만 표시").font(font(11.5)).color(pal.text2));
            egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
                let mut by_cat: std::collections::BTreeMap<String, Vec<(u32, String, usize)>> = Default::default();
                for (t, n) in &fc.tags {
                    let name = st.index.tag_name(*t);
                    let (cat, val) = tags::split(name);
                    let cat = if cat.is_empty() { "내 태그".to_string() } else { cat.to_string() };
                    by_cat.entry(cat).or_default().push((*t, val.to_string(), *n));
                }
                let order = |c: &str| tags::CATEGORIES.iter().position(|x| *x == c).unwrap_or(0);
                let mut cats: Vec<_> = by_cat.into_iter().collect();
                cats.sort_by_key(|(c, _)| if c == "내 태그" { 0 } else { order(c) + 1 });
                for (cat, items) in cats {
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new(&cat).font(bold(12.0)).color(pal.text2));
                    ui.horizontal_wrapped(|ui| {
                        for (t, val, n) in items {
                            let on = self.filter.tags.contains(&t);
                            if ui.selectable_label(on, format!("{val} {n}")).clicked() {
                                if on {
                                    self.filter.tags.remove(&t);
                                } else {
                                    self.filter.tags.insert(t);
                                }
                                changed = true;
                            }
                        }
                    });
                }
            });
        });

        if self.filter_active() {
            ui.add_space(4.0);
            if ui.add(egui::Button::new(egui::RichText::new("필터 지우기").color(pal.accent)).frame(false)).clicked()
            {
                let side = self.side.clone();
                self.select_side(side);
                changed = false;
            }
        }
        if changed {
            self.filter_changed();
        }
    }

    /// Bar shown while browsing B-cut candidates.
    pub fn cull_bar(&mut self, ui: &mut Ui) {
        let pal = self.pal;
        let ctx = ui.ctx().clone();
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 40.0), Sense::hover());
        ui.painter().rect_filled(rect, 0.0, pal.warning.gamma_multiply(0.14));
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
            widgets::Icon::Suspect,
            pal.warning,
        );
        ui.add_space(22.0);
        let n = self.filtered.len();
        ui.label(egui::RichText::new(format!("B컷 후보 {}장", format::count(n))).font(bold(12.5)))
            .on_hover_text("X로 거절, 잘못 걸러진 사진은 P(픽)로 표시하세요. 고를 때마다 내 기준을 다시 학습합니다.");
        if rect.width() > 820.0 {
            ui.label(egui::RichText::new("X 거절 · P 잘못 걸러짐").font(font(11.5)).color(pal.text2));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if n > 0 && widgets::secondary_button(ui, &format!("{}장 모두 거절", format::count(n)), &pal).clicked()
            {
                self.reject_all_candidates(&ctx);
            }
            ui.add_space(6.0);
            let mut t = self.cfg.cull_threshold as f32;
            ui.label(egui::RichText::new(format!("{t:.0}%")).font(bold(12.0)));
            ui.spacing_mut().slider_width = 110.0;
            if ui
                .add(egui::Slider::new(&mut t, 30.0..=90.0).step_by(5.0).show_value(false))
                .on_hover_text("B컷 가능성이 이 값 이상인 사진만 표시 (낮출수록 더 많이 걸러냄)")
                .changed()
            {
                self.set_cull_threshold(t as u8);
            }
            ui.label(egui::RichText::new("기준").font(font(11.5)).color(pal.text2));
        });
    }

    fn set_label(&self, name: &str, values: &[String]) -> String {
        match values.len() {
            0 => name.to_string(),
            1 => truncate(&values[0], 22),
            n => format!("{name} {n}개"),
        }
    }

    fn numeric_popup(&mut self, ui: &mut Ui, num: Num) -> bool {
        let pal = self.pal;
        let mut changed = false;
        let fc = self.facets.clone().unwrap_or_default();
        // Presets.
        let presets: Vec<(&str, f32, f32)> = match num {
            Num::Iso => vec![
                ("저감도 ≤ 400", 0.0, 400.0),
                ("400 – 1600", 400.0, 1600.0),
                ("1600 – 6400", 1600.0, 6400.0),
                ("고감도 ≥ 6400", 6400.0, 1e7),
            ],
            Num::Focal => vec![
                ("초광각 < 21mm", 0.0, 20.9),
                ("광각 21–35", 21.0, 35.0),
                ("표준 35–70", 35.0, 70.0),
                ("준망원 70–135", 70.0, 135.0),
                ("망원 135–300", 135.0, 300.0),
                ("초망원 > 300", 300.0, 5000.0),
            ],
            Num::Aperture => vec![
                ("개방 ≤ ƒ/2", 0.0, 2.0),
                ("ƒ/2 – ƒ/4", 2.0, 4.0),
                ("ƒ/4 – ƒ/8", 4.0, 8.0),
                ("조임 ≥ ƒ/8", 8.0, 64.0),
            ],
            Num::Shutter => vec![
                ("고속 ≤ 1/1000", 0.0, 0.001),
                ("1/1000 – 1/125", 0.001, 0.008),
                ("1/125 – 1/30", 0.008, 0.034),
                ("저속 1/30 – 1s", 0.034, 1.0),
                ("장노출 ≥ 1s", 1.0, 1e5),
            ],
        };
        ui.label(egui::RichText::new("빠른 선택").font(bold(12.0)).color(pal.text2));
        ui.horizontal_wrapped(|ui| {
            for (l, a, b) in &presets {
                let cur = self.get_range(num);
                let on = cur.is_some_and(|(x, y)| (x - a).abs() < 1e-6 && (y - b).abs() < 1e-6);
                if ui.selectable_label(on, *l).clicked() {
                    self.set_range(num, if on { None } else { Some((*a, *b)) });
                    changed = true;
                }
            }
        });
        ui.add_space(6.0);
        ui.label(egui::RichText::new("값 (사진 수)").font(bold(12.0)).color(pal.text2));
        let values: Vec<(f32, String, usize)> = match num {
            Num::Iso => fc.isos.iter().map(|(v, n)| (*v as f32, v.to_string(), *n)).collect(),
            Num::Focal => fc.focals.iter().map(|(v, n)| (*v as f32, format::focal(*v as f32), *n)).collect(),
            Num::Aperture => {
                fc.apertures.iter().map(|(v, n)| (*v as f32 / 10.0, format::aperture(*v as f32 / 10.0), *n)).collect()
            }
            Num::Shutter => {
                fc.shutters.iter().map(|(v, n)| (*v as f32 / 1e6, format::shutter(*v as f32 / 1e6), *n)).collect()
            }
        };
        egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                for (v, l, n) in values {
                    let (lo, hi) = match num {
                        Num::Focal => (v - 0.5, v + 0.49),
                        Num::Aperture => (v - 0.04, v + 0.04),
                        _ => (v, v),
                    };
                    let on = self.get_range(num).is_some_and(|(a, b)| (a - lo).abs() < 1e-4 && (b - hi).abs() < 1e-4);
                    if ui.selectable_label(on, format!("{l} · {n}")).clicked() {
                        self.set_range(num, if on { None } else { Some((lo, hi)) });
                        changed = true;
                    }
                }
            });
        });
        if self.get_range(num).is_some() {
            ui.add_space(4.0);
            if ui.button("이 필터 지우기").clicked() {
                self.set_range(num, None);
                changed = true;
            }
        }
        let _ = Rect::NOTHING;
        changed
    }

    fn get_range(&self, num: Num) -> Option<(f32, f32)> {
        match num {
            Num::Iso => self.filter.iso.map(|(a, b)| (a as f32, b as f32)),
            Num::Focal => self.filter.focal,
            Num::Aperture => self.filter.aperture,
            Num::Shutter => self.filter.shutter,
        }
    }

    fn set_range(&mut self, num: Num, r: Option<(f32, f32)>) {
        match num {
            Num::Iso => self.filter.iso = r.map(|(a, b)| (a.round() as u32, b.min(u32::MAX as f32).round() as u32)),
            Num::Focal => self.filter.focal = r,
            Num::Aperture => self.filter.aperture = r,
            Num::Shutter => self.filter.shutter = r,
        }
    }
}

fn range_txt(a: String, b: String) -> String {
    if a == b {
        a
    } else {
        format!("{a}–{b}")
    }
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(n - 1).collect::<String>())
    }
}
