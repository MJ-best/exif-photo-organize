use super::titlebar_pad;
use crate::app::PhotoApp;
use crate::theme::{bold, font};
use crate::widgets::{self, Icon};
use eframe::egui::{self, pos2, vec2, Align2, Color32, Rect, Sense, Stroke, Ui};
use photoorg_core::catalog::TagKind;
use photoorg_core::{format, tags, FileKind};

impl PhotoApp {
    pub fn info_panel(&mut self, ui: &mut Ui) {
        let pal = self.pal;
        let ctx = ui.ctx().clone();
        ui.add_space(titlebar_pad());
        let (hr, _) = ui.allocate_exact_size(vec2(ui.available_width(), 52.0), Sense::hover());
        ui.painter().rect_filled(hr, 0.0, pal.toolbar_bg);
        widgets::separator_h(ui, hr, &pal);
        ui.painter().text(pos2(hr.left() + 18.0, hr.center().y), Align2::LEFT_CENTER, "정보", bold(15.0), pal.text);

        let ids = self.target_ids();
        let focus = self.viewer.as_ref().map(|v| v.id).or(self.sel.focus).or_else(|| ids.first().copied());
        egui::ScrollArea::vertical().id_salt("info").auto_shrink([false, false]).show(ui, |ui| {
            egui::Frame::new().inner_margin(egui::Margin::symmetric(18, 14)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                if ids.len() > 1 {
                    self.multi_info(ui, &ids);
                    return;
                }
                let Some(id) = focus else {
                    ui.label(egui::RichText::new("사진을 선택하세요").color(pal.text2));
                    return;
                };
                let Some(a) = self.lib.as_ref().and_then(|s| s.index.get(id)).cloned() else { return };
                let Some(info) = self.info_for(id) else { return };
                let meta = info.meta.clone();
                let files = info.files.clone();
                let tag_list = info.tags.clone();

                ui.label(egui::RichText::new(&a.name).font(bold(16.0)));
                ui.horizontal(|ui| {
                    let label = photoorg_core::media::formats::label(a.formats);
                    let (r, _) = ui.allocate_exact_size(
                        vec2(ui.painter().layout_no_wrap(label.into(), bold(10.0), pal.text).size().x + 12.0, 18.0),
                        Sense::hover(),
                    );
                    ui.painter().rect_filled(r, 4.0, if a.has_raw() { pal.accent } else { pal.control_bg });
                    ui.painter().text(
                        r.center(),
                        Align2::CENTER_CENTER,
                        label,
                        bold(10.0),
                        if a.has_raw() { pal.accent_text } else { pal.text },
                    );
                    if a.date_from_file {
                        ui.label(
                            egui::RichText::new("EXIF 날짜 없음 · 파일 시간 사용").font(font(11.0)).color(pal.text3),
                        );
                    }
                });
                ui.add_space(4.0);
                ui.label(egui::RichText::new(format::datetime_ko(a.captured())).font(font(12.5)).color(pal.text2));
                ui.add_space(10.0);
                if let Some(r) = widgets::stars(ui, a.rating, 20.0, &pal) {
                    self.rate(&ctx, vec![id], r);
                }
                ui.add_space(14.0);

                // Camera block (like Photos' info window).
                let cam = meta.camera().unwrap_or_else(|| "카메라 정보 없음".into());
                ui.label(egui::RichText::new(cam).font(bold(13.5)));
                if let Some(l) = &meta.lens {
                    ui.label(egui::RichText::new(l).font(font(12.0)).color(pal.text2));
                }
                let mut sizes = Vec::new();
                if let (Some(w), Some(h)) = (meta.width, meta.height) {
                    let (w, h) = if matches!(meta.orientation, 5..=8) { (h, w) } else { (w, h) };
                    sizes.push(format!("{w} × {h}"));
                    sizes.push(format!("{:.1}MP", (w as f64 * h as f64) / 1e6));
                }
                let total: u64 = files.iter().map(|f| f.size).sum();
                if total > 0 {
                    sizes.push(format::bytes(total));
                }
                if !sizes.is_empty() {
                    ui.label(egui::RichText::new(sizes.join("  ·  ")).font(font(11.5)).color(pal.text2));
                }
                ui.add_space(8.0);
                let cells = [
                    meta.iso.map(format::iso).unwrap_or_else(|| "ISO –".into()),
                    meta.focal_length.map(format::focal).unwrap_or_else(|| "– mm".into()),
                    meta.exposure_bias.map(format::ev).unwrap_or_else(|| "– ev".into()),
                    meta.aperture.map(format::aperture).unwrap_or_else(|| "ƒ/–".into()),
                    meta.exposure_time.map(format::shutter).unwrap_or_else(|| "– s".into()),
                ];
                let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::hover());
                let p = ui.painter();
                p.rect_stroke(r, 6.0, Stroke::new(1.0, pal.separator), egui::StrokeKind::Inside);
                let cw = r.width() / cells.len() as f32;
                for (i, c) in cells.iter().enumerate() {
                    let cr = Rect::from_min_size(pos2(r.left() + i as f32 * cw, r.top()), vec2(cw, r.height()));
                    if i > 0 {
                        p.line_segment(
                            [cr.left_top() + vec2(0.0, 6.0), cr.left_bottom() - vec2(0.0, 6.0)],
                            Stroke::new(1.0, pal.separator),
                        );
                    }
                    p.text(cr.center(), Align2::CENTER_CENTER, c, font(11.0), pal.text);
                }
                if let Some(f35) = meta.focal_35mm {
                    ui.label(
                        egui::RichText::new(format!("35mm 환산 {}", format::focal(f35)))
                            .font(font(11.0))
                            .color(pal.text3),
                    );
                }
                let mut extra = Vec::new();
                if meta.flash_fired == Some(true) {
                    extra.push("플래시 사용".to_string());
                }
                if let Some(s) = &meta.serial {
                    extra.push(format!("S/N {s}"));
                }
                if let Some(o) = &meta.offset {
                    extra.push(format!("UTC{o}"));
                }
                if !extra.is_empty() {
                    ui.label(egui::RichText::new(extra.join("  ·  ")).font(font(11.0)).color(pal.text3));
                }
                if let Some(g) = meta.gps {
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(format!("📍 {:.5}, {:.5}", g.lat, g.lon)).font(font(12.0)));
                        if ui
                            .add(egui::Button::new(egui::RichText::new("지도").color(pal.accent)).frame(false))
                            .clicked()
                        {
                            ctx.open_url(egui::OpenUrl::new_tab(format!(
                                "https://maps.google.com/?q={},{}",
                                g.lat, g.lon
                            )));
                        }
                    });
                }

                // Files.
                ui.add_space(16.0);
                ui.label(egui::RichText::new("파일").font(bold(12.0)).color(pal.text2));
                for f in &files {
                    ui.add_space(4.0);
                    let role = match f.kind {
                        FileKind::Raw => "RAW 원본",
                        FileKind::Jpeg => {
                            if a.has_raw() {
                                "JPEG (카메라 현상)"
                            } else {
                                "JPEG 원본"
                            }
                        }
                        FileKind::Heif => "HEIF",
                        FileKind::Video => "동영상",
                        _ => "이미지",
                    };
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(role).font(bold(11.5)).color(if f.kind == FileKind::Raw {
                            pal.accent
                        } else {
                            pal.text
                        }));
                        ui.label(egui::RichText::new(format::bytes(f.size)).font(font(11.0)).color(pal.text3));
                    });
                    let path = self.lib.as_ref().map(|s| s.lib.abs(&f.rel_path)).unwrap_or_default();
                    let r = ui.add(
                        egui::Label::new(egui::RichText::new(&f.rel_path).font(font(11.0)).color(pal.text2))
                            .wrap()
                            .sense(Sense::click()),
                    );
                    let r = r.on_hover_text(format!(
                        "{}\n원래 위치: {}\n원래 이름: {}\nBLAKE3: {}…\n클릭하면 파일 위치를 엽니다",
                        path.display(),
                        f.orig_path.clone().unwrap_or_default(),
                        f.orig_name,
                        f.hash.get(..16).unwrap_or(&f.hash)
                    ));
                    if r.clicked() {
                        self.reveal(&path);
                    }
                }

                // Tags.
                ui.add_space(16.0);
                ui.label(egui::RichText::new("태그").font(bold(12.0)).color(pal.text2));
                ui.add_space(4.0);
                let mut remove = None;
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
                    for (tid, name, kind) in &tag_list {
                        let (cat, val) = tags::split(name);
                        let txt = if cat.is_empty() { val.to_string() } else { format!("{cat} · {val}") };
                        let user = *kind == TagKind::User;
                        let g = ui.painter().layout_no_wrap(txt.clone(), font(11.0), pal.text);
                        let w = g.size().x + if user { 26.0 } else { 14.0 };
                        let (r, resp) = ui.allocate_exact_size(vec2(w, 20.0), Sense::click());
                        let bg = if user { pal.accent.gamma_multiply(0.35) } else { pal.control_bg };
                        ui.painter().rect_filled(r, 10.0, bg);
                        ui.painter().galley(pos2(r.left() + 7.0, r.center().y - g.size().y / 2.0), g, pal.text);
                        if user {
                            let xr =
                                Rect::from_center_size(pos2(r.right() - 10.0, r.center().y), egui::Vec2::splat(10.0));
                            widgets::paint_icon(
                                ui.painter(),
                                xr,
                                Icon::Close,
                                if resp.hovered() { pal.text } else { pal.text2 },
                            );
                            if resp.clicked() {
                                remove = Some(*tid);
                            }
                        } else if resp.clicked() {
                            // Filter by this tag.
                            self.filter.tags.insert(*tid);
                            self.show_filters = true;
                        }
                    }
                });
                if let Some(t) = remove {
                    self.remove_tag(vec![id], t);
                }
                if !self.filter.tags.is_empty() && self.viewer.is_none() {
                    self.refilter_needed = true;
                }
                ui.add_space(6.0);
                let resp = ui.add(
                    egui::TextEdit::singleline(&mut self.tag_input)
                        .hint_text("태그 추가 후 Enter")
                        .desired_width(f32::INFINITY),
                );
                if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    let t = std::mem::take(&mut self.tag_input);
                    self.add_tag(vec![id], &t);
                }
                ui.add_space(14.0);
                if widgets::secondary_button(ui, "파일 위치 보기", &pal).clicked() {
                    self.reveal_asset(id);
                }
            });
        });
        let _ = Color32::WHITE;
    }

    fn multi_info(&mut self, ui: &mut Ui, ids: &[i64]) {
        let pal = self.pal;
        let ctx = ui.ctx().clone();
        let Some(st) = &self.lib else { return };
        let recs: Vec<_> = ids.iter().filter_map(|i| st.index.get(*i)).collect();
        ui.label(egui::RichText::new(format!("{}개 항목", format::count(recs.len()))).font(bold(16.0)));
        if let (Some(a), Some(b)) = (recs.iter().map(|r| r.captured_ms).min(), recs.iter().map(|r| r.captured_ms).max())
        {
            let a = photoorg_core::catalog::ms_to_naive(a);
            let b = photoorg_core::catalog::ms_to_naive(b);
            ui.label(
                egui::RichText::new(format!("{} – {}", format::datetime_ko(a), format::time_ko(b)))
                    .font(font(12.0))
                    .color(pal.text2),
            );
        }
        let raws = recs.iter().filter(|r| r.has_raw()).count();
        ui.label(egui::RichText::new(format!("RAW 포함 {raws}개")).font(font(12.0)).color(pal.text2));
        let common = recs.first().map(|r| r.rating).filter(|r| recs.iter().all(|x| x.rating == *r)).unwrap_or(0);
        ui.add_space(10.0);
        ui.label(egui::RichText::new("모두에 별점 적용").font(bold(12.0)).color(pal.text2));
        if let Some(r) = widgets::stars(ui, common, 20.0, &pal) {
            self.rate(&ctx, ids.to_vec(), r);
        }
        ui.add_space(14.0);
        ui.label(egui::RichText::new("모두에 태그 추가").font(bold(12.0)).color(pal.text2));
        let resp = ui.add(
            egui::TextEdit::singleline(&mut self.tag_input)
                .hint_text("태그 입력 후 Enter")
                .desired_width(f32::INFINITY),
        );
        if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            let t = std::mem::take(&mut self.tag_input);
            self.add_tag(ids.to_vec(), &t);
        }
    }
}
