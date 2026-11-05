//! "내 셀렉 정확도": how well the culling assistant matches the photographer's own decisions,
//! measured by the engine's cross-validation ([`photoorg_core::cull::eval`]) on photos the model
//! did not learn from. Re-measured in the background after the labels change, kept (with a short
//! history) in the catalog, shown in the candidate bars and in its own dialog.

use crate::app::PhotoApp;
use crate::theme::{bold, font};
use crate::widgets;
use crossbeam_channel::{unbounded, Receiver};
use eframe::egui::{self, pos2, vec2, Frame, Margin, Sense, Stroke};
use photoorg_core::cull::eval::{self, EvalReport, Metrics};
use photoorg_core::{format, Catalog};
use serde::{Deserialize, Serialize};

/// Catalog meta key of the stored measurements.
const META_KEY: &str = "cull_accuracy";
/// Measurements kept for the history line.
const KEEP: usize = 30;
/// Folds of the cross-validation.
const FOLDS: usize = 5;
/// Both A- and B-cuts needed before a measurement means anything.
pub const MIN_EACH: usize = 5;

/// One measurement.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Measurement {
    /// Local time, "2026-11-05 14:03".
    pub at: String,
    pub report: EvalReport,
}

#[derive(Default)]
pub struct Accuracy {
    /// Oldest first.
    pub history: Vec<Measurement>,
    pub job: Option<Receiver<Result<EvalReport, String>>>,
    pub open: bool,
}

impl Accuracy {
    pub fn load(catalog: &Catalog) -> Accuracy {
        let history = catalog
            .meta_get(META_KEY)
            .ok()
            .flatten()
            .and_then(|s| serde_json::from_str::<Vec<Measurement>>(&s).ok())
            .unwrap_or_default();
        Accuracy { history, ..Default::default() }
    }

    pub fn latest(&self) -> Option<&Measurement> {
        self.history.last()
    }

    /// The headline: learned AUC as a percentage, once measured.
    pub fn headline(&self) -> Option<f64> {
        self.latest().and_then(|m| m.report.learned.auc).map(|a| a * 100.0)
    }
}

/// A line of the details table: name, explanation, value, whether higher is better.
type Row = (&'static str, &'static str, fn(&Metrics) -> f64, bool);

impl PhotoApp {
    /// After a rescore: measure again when the judged photos changed enough (or the learning
    /// settings / threshold did), so the numbers follow the photographer's work.
    pub fn maybe_measure_accuracy(&mut self, a: usize, b: usize) {
        if a < MIN_EACH || b < MIN_EACH || self.accuracy.job.is_some() {
            return;
        }
        let threshold = self.cfg.cull_threshold as f64 / 100.0;
        let stale = match self.accuracy.latest() {
            None => true,
            Some(m) => {
                let (pa, pb) = (m.report.a, m.report.b);
                let moved = pa.abs_diff(a) + pb.abs_diff(b);
                moved >= 5.max((pa + pb) / 20) || (m.report.threshold - threshold).abs() > 1e-6
            }
        };
        if stale {
            self.measure_accuracy();
        }
    }

    /// Measure now (in the background).
    pub fn measure_accuracy(&mut self) {
        if self.accuracy.job.is_some() {
            return;
        }
        let Some(st) = &self.lib else { return };
        let lib = st.lib.clone();
        let settings = self.cfg.learn;
        let threshold = self.cfg.cull_threshold as f64 / 100.0;
        let (tx, rx) = unbounded();
        std::thread::Builder::new()
            .name("cull-accuracy".into())
            .spawn(move || {
                let r = (|| -> anyhow::Result<EvalReport> {
                    let items = lib.catalog()?.cull_items()?;
                    Ok(eval::evaluate(&items, &settings, FOLDS, threshold))
                })();
                let _ = tx.send(r.map_err(|e| format!("{e:#}")));
            })
            .expect("spawn accuracy");
        self.accuracy.job = Some(rx);
    }

    /// Called every frame.
    pub fn poll_accuracy(&mut self, ctx: &egui::Context) {
        let Some(rx) = &self.accuracy.job else { return };
        let Ok(r) = rx.try_recv() else {
            ctx.request_repaint_after(std::time::Duration::from_millis(300));
            return;
        };
        self.accuracy.job = None;
        match r {
            Ok(report) => {
                let m = Measurement { at: chrono::Local::now().format("%Y-%m-%d %H:%M").to_string(), report };
                let h = &mut self.accuracy.history;
                // Same judged photos as last time (e.g. only the threshold moved): replace.
                if h.last().is_some_and(|l| l.report.a == m.report.a && l.report.b == m.report.b) {
                    h.pop();
                }
                h.push(m);
                if h.len() > KEEP {
                    h.drain(..h.len() - KEEP);
                }
                if let Some(st) = &self.lib {
                    if let Ok(json) = serde_json::to_string(&self.accuracy.history) {
                        let _ = st.catalog.meta_set(META_KEY, Some(&json));
                    }
                }
                // The threshold slider moved while measuring: measure again for the new one.
                let (a, b) = self.accuracy.latest().map(|m| (m.report.a, m.report.b)).unwrap_or((0, 0));
                self.maybe_measure_accuracy(a, b);
            }
            Err(e) => log::warn!("accuracy: {e}"),
        }
    }

    /// The "내 셀렉 정확도" dialog.
    pub fn accuracy_dialog_ui(&mut self, ui: &mut egui::Ui) {
        if !self.accuracy.open {
            return;
        }
        let pal = self.pal;
        let ctx = ui.ctx().clone();
        // Judged photos right now (from the last training).
        let (a_now, b_now) =
            self.cull_report.as_ref().and_then(|r| r.scenes.first()).map(|s| (s.a, s.b)).unwrap_or((0, 0));
        let threshold = self.cfg.cull_threshold;
        let running = self.accuracy.job.is_some();
        let mut close = false;
        let mut again = false;
        let modal = egui::Modal::new(egui::Id::unique("accuracy-dialog")).frame(
            Frame::new()
                .fill(pal.panel_bg)
                .corner_radius(14.0)
                .inner_margin(Margin::same(22))
                .stroke(Stroke::new(1.0, pal.separator)),
        );
        let resp = modal.show(&ctx, |ui| {
            ui.set_width(520.0);
            ui.spacing_mut().item_spacing.y = 6.0;
            ui.label(egui::RichText::new("내 셀렉 정확도").font(bold(18.0)));
            let latest = self.accuracy.latest().cloned();
            match &latest {
                None if a_now < MIN_EACH || b_now < MIN_EACH => {
                    ui.label(
                        egui::RichText::new(format!(
                            "A컷(픽 · ★3 이상)과 B컷(거절 · ★1)을 각각 {MIN_EACH}장 이상 고르면 잴 수 있습니다. \
                             지금 A {a_now} · B {b_now}"
                        ))
                        .font(font(12.5)),
                    );
                }
                None => {
                    ui.label(egui::RichText::new("재는 중…").font(font(12.5)).color(pal.text2));
                }
                Some(m) => self.accuracy_body(ui, m),
            }
            ui.add_space(10.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if widgets::primary_button(ui, "닫기", &pal, true).clicked() {
                    close = true;
                }
                let can = a_now >= MIN_EACH && b_now >= MIN_EACH && !running;
                let label = if running { "재는 중…" } else { "지금 다시 재기" };
                if widgets::secondary_button(ui, label, &pal).clicked() && can {
                    again = true;
                }
                if let Some(m) = &latest {
                    if (m.report.threshold * 100.0 - threshold as f64).abs() > 0.5 {
                        ui.label(
                            egui::RichText::new(format!("기준이 {threshold}%로 바뀌었습니다"))
                                .font(font(11.0))
                                .color(pal.text3),
                        );
                    }
                }
            });
        });
        if resp.should_close() || close {
            self.accuracy.open = false;
        }
        if again {
            self.measure_accuracy();
        }
    }

    fn accuracy_body(&self, ui: &mut egui::Ui, m: &Measurement) {
        let pal = self.pal;
        let r = &m.report;
        ui.label(
            egui::RichText::new(format!(
                "내가 판정한 사진 A {} · B {}장으로 확인 · {}",
                format::count(r.a),
                format::count(r.b),
                m.at
            ))
            .font(font(12.0))
            .color(pal.text2),
        );
        ui.label(
            egui::RichText::new(format!(
                "판정한 사진을 {}묶음으로 나눠, 한 묶음씩 가린 채 나머지로 학습하고 가린 사진을 맞혀 봤습니다 \
                 — 학습에 쓰지 않은 사진으로 잰 값입니다.",
                r.folds
            ))
            .font(font(11.0))
            .color(pal.text3),
        );
        ui.add_space(6.0);

        // Headline: how well A and B are told apart, before and after learning.
        if let (Some(prior), Some(learned)) = (r.prior.auc, r.learned.auc) {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(format!("{:.0}%", learned * 100.0)).font(bold(34.0)).color(pal.accent));
                ui.vertical(|ui| {
                    ui.add_space(6.0);
                    ui.label(egui::RichText::new("A컷 · B컷 구분 정확도").font(bold(13.0)));
                    let d = (learned - prior) * 100.0;
                    let (txt, c) = if d >= 0.5 {
                        (format!("기본 기준 {:.0}% → 내 기준 학습으로 +{d:.0}%p", prior * 100.0), pal.success)
                    } else if d <= -0.5 {
                        (format!("기본 기준 {:.0}% 보다 {:.0}%p 낮음", prior * 100.0, -d), pal.warning)
                    } else {
                        (format!("기본 기준 {:.0}%와 비슷", prior * 100.0), pal.text2)
                    };
                    ui.label(egui::RichText::new(txt).font(font(12.0)).color(c));
                });
            })
            .response
            .on_hover_text(
                "B컷 하나와 A컷 하나를 무작위로 골랐을 때 B컷 쪽을 더 B컷답다고 본 비율입니다. \
                 50%는 찍기, 100%는 완벽.",
            );
        }
        ui.add_space(6.0);

        // Details: before / after learning, at the current threshold.
        let pct = |v: f64| format!("{:.0}%", v * 100.0);
        let rows: [Row; 5] = [
            ("거절할 사진을 미리 잡아냄", "내가 B컷으로 판정한 사진 중 B컷 후보로 잡은 비율", |m| m.b_recall, true),
            ("B컷 후보가 맞음", "B컷 후보로 올린 사진 중 실제로 B컷인 비율", |m| m.b_precision, true),
            (
                "A컷을 B컷 후보로 잘못 올림",
                "내가 A컷으로 고른 사진 중 B컷 후보로 잘못 올린 비율 — 낮을수록 좋음",
                |m| m.a_false_alarm,
                false,
            ),
            ("A컷 후보 적중", "가장 잘 나왔다고 본 사진(A컷 수만큼) 중 실제 A컷인 비율", |m| m.a_precision, true),
            (
                "아직 안 고른 사진 중 B컷 후보",
                "지금 B컷 후보 목록의 크기 (기본 기준 vs 학습한 기준)",
                |m| m.candidate_rate,
                true,
            ),
        ];
        egui::Grid::new("accuracy-rows").num_columns(3).spacing(vec2(18.0, 4.0)).striped(true).show(ui, |ui| {
            ui.label(
                egui::RichText::new(format!("B컷 후보 기준 {:.0}%", r.threshold * 100.0))
                    .font(font(11.0))
                    .color(pal.text3),
            );
            ui.label(egui::RichText::new("기본 기준").font(font(11.0)).color(pal.text3));
            ui.label(egui::RichText::new("내 기준 (학습)").font(font(11.0)).color(pal.text3));
            ui.end_row();
            for (i, (name, tip, get, higher_better)) in rows.iter().enumerate() {
                ui.label(egui::RichText::new(*name).font(font(12.0))).on_hover_text(*tip);
                let (p, l) = (get(&r.prior), get(&r.learned));
                ui.label(egui::RichText::new(pct(p)).font(font(12.0)).color(pal.text2));
                // The candidate share is neither better nor worse; colour only the scores.
                let c = if i == 4 || (l - p).abs() < 0.005 {
                    pal.text
                } else if (l > p) == *higher_better {
                    pal.success
                } else {
                    pal.warning
                };
                ui.label(egui::RichText::new(pct(l)).font(bold(12.0)).color(c));
                ui.end_row();
            }
        });

        // Per scene: only scenes with both kinds of decisions can be measured.
        let judged_scenes = r.scenes.iter().filter(|s| s.a + s.b > 0).count();
        let scenes: Vec<_> = r.scenes.iter().filter(|s| s.a > 0 && s.b > 0).collect();
        let one_sided: Vec<&str> =
            r.scenes.iter().filter(|s| s.a + s.b > 0 && (s.a == 0 || s.b == 0)).map(|s| s.scene.label()).collect();
        if !scenes.is_empty() && judged_scenes > 1 {
            ui.add_space(6.0);
            ui.label(egui::RichText::new("장면별 구분 정확도").font(bold(12.0)));
            egui::Grid::new("accuracy-scenes").num_columns(5).spacing(vec2(18.0, 3.0)).show(ui, |ui| {
                for h in ["장면", "A컷", "B컷", "기본 기준", "내 기준"] {
                    ui.label(egui::RichText::new(h).font(font(11.0)).color(pal.text3));
                }
                ui.end_row();
                let f = |v: Option<f64>| v.map(|v| format!("{:.0}%", v * 100.0)).unwrap_or_else(|| "–".into());
                for s in scenes {
                    ui.label(egui::RichText::new(s.scene.label()).font(font(11.5)));
                    ui.label(egui::RichText::new(s.a.to_string()).font(font(11.5)));
                    ui.label(egui::RichText::new(s.b.to_string()).font(font(11.5)));
                    ui.label(egui::RichText::new(f(s.prior_auc)).font(font(11.5)).color(pal.text2));
                    ui.label(egui::RichText::new(f(s.learned_auc)).font(bold(11.5)));
                    ui.end_row();
                }
            });
            let mut note = "한 장면의 판정이 적으면(A · B 각 10장 미만) 숫자가 크게 흔들립니다.".to_string();
            if !one_sided.is_empty() {
                note.push_str(&format!(" A컷이나 B컷 한쪽만 있는 장면은 잴 수 없습니다: {}.", one_sided.join(" · ")));
            }
            ui.label(egui::RichText::new(note).font(font(11.0)).color(pal.text3));
        }

        // History: the accuracy as the judged photos grew.
        let points: Vec<(f64, usize)> = self
            .accuracy
            .history
            .iter()
            .filter_map(|m| m.report.learned.auc.map(|a| (a, m.report.a + m.report.b)))
            .collect();
        let prior_points: Vec<f64> =
            self.accuracy.history.iter().filter_map(|m| m.report.learned.auc.and(m.report.prior.auc)).collect();
        if points.len() >= 2 {
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("판정이 쌓이며 바뀐 정확도").font(bold(12.0)));
                ui.label(egui::RichText::new("━ 내 기준").font(font(11.0)).color(pal.accent));
                ui.label(egui::RichText::new("━ 기본 기준").font(font(11.0)).color(pal.text3));
            });
            let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 64.0), Sense::hover());
            let p = ui.painter();
            p.rect_filled(rect, 6.0, pal.control_bg);
            let inner = rect.shrink2(vec2(36.0, 10.0));
            let (lo, hi) = (0.5f64, 1.0f64);
            let at = |i: usize, v: f64| {
                let x = inner.left() + inner.width() * i as f32 / (points.len() - 1) as f32;
                let y = inner.bottom() - inner.height() * ((v.clamp(lo, hi) - lo) / (hi - lo)) as f32;
                pos2(x, y)
            };
            let line: Vec<egui::Pos2> = points.iter().enumerate().map(|(i, (v, _))| at(i, *v)).collect();
            if prior_points.len() == points.len() {
                let base: Vec<egui::Pos2> = prior_points.iter().enumerate().map(|(i, v)| at(i, *v)).collect();
                p.add(egui::Shape::line(base, Stroke::new(1.2, pal.text3)));
            }
            p.add(egui::Shape::line(line.clone(), Stroke::new(2.0, pal.accent)));
            for q in &line {
                p.circle_filled(*q, 2.5, pal.accent);
            }
            let first = points.first().unwrap();
            let last = points.last().unwrap();
            p.text(
                pos2(rect.left() + 6.0, line[0].y),
                egui::Align2::LEFT_CENTER,
                format!("{:.0}%", first.0 * 100.0),
                font(10.5),
                pal.text2,
            );
            p.text(
                pos2(rect.right() - 6.0, line[line.len() - 1].y),
                egui::Align2::RIGHT_CENTER,
                format!("{:.0}%", last.0 * 100.0),
                font(10.5),
                pal.text,
            );
            p.text(
                pos2(inner.left(), rect.bottom() - 2.0),
                egui::Align2::LEFT_BOTTOM,
                format!("판정 {}장", format::count(first.1)),
                font(9.5),
                pal.text3,
            );
            p.text(
                pos2(inner.right(), rect.bottom() - 2.0),
                egui::Align2::RIGHT_BOTTOM,
                format!("{}장", format::count(last.1)),
                font(9.5),
                pal.text3,
            );
        }
        ui.add_space(4.0);
        ui.label(
            egui::RichText::new(
                "후보 목록에서만 판정하면 쉬운 사진 위주로 재게 됩니다. 라이브러리 전체에서 고르게 판정할수록 숫자가 정확해집니다.",
            )
            .font(font(11.0))
            .color(pal.text3),
        );
    }
}
