//! Measuring the engine: how well it separates a photographer's A-cuts from their B-cuts.
//!
//! [`evaluate`] hides the labels of one fold at a time, retrains on the rest exactly as the app
//! does ([`learn::score`]) and scores the hidden photos, so every number is about photos the
//! model has not seen. The prior (no learning) is measured on the same photos, so the report
//! shows what learning adds. This is the yardstick for every change to the engine: run it on a
//! real, labelled library before and after.

use crate::learn::{self, Item, LearnSettings};
use crate::scene::Scene;
use serde::Serialize;

#[derive(Clone, Debug, Default, Serialize, PartialEq)]
pub struct Metrics {
    /// Chance that a random B-cut scores higher than a random A-cut (1 = perfect, 0.5 = guessing;
    /// None without both A- and B-cuts).
    pub auc: Option<f64>,
    /// Of the judged photos at or above the threshold, the share that are B-cuts.
    pub b_precision: f64,
    /// Of the B-cuts, the share at or above the threshold.
    pub b_recall: f64,
    /// Of the best-ranked judged photos (as many as there are A-cuts), the share that are A-cuts.
    pub a_precision: f64,
    /// Share of the photos not judged yet that are at or above the threshold: the size of the
    /// candidate list the photographer would see.
    pub candidate_rate: f64,
}

#[derive(Clone, Debug, Default, Serialize, PartialEq)]
pub struct SceneEval {
    pub scene: Scene,
    pub a: usize,
    pub b: usize,
    pub prior_auc: Option<f64>,
    pub learned_auc: Option<f64>,
}

#[derive(Clone, Debug, Default, Serialize, PartialEq)]
pub struct EvalReport {
    pub photos: usize,
    /// Judged photos: A-cuts and B-cuts.
    pub a: usize,
    pub b: usize,
    pub folds: usize,
    /// B-cut probability from which a photo is a candidate (0..1).
    pub threshold: f64,
    pub prior: Metrics,
    pub learned: Metrics,
    pub scenes: Vec<SceneEval>,
}

/// Area under the ROC curve for `(score, is_b)` (Mann–Whitney, ties counted half).
pub fn auc(points: &[(f64, bool)]) -> Option<f64> {
    let pos = points.iter().filter(|p| p.1).count();
    let neg = points.len() - pos;
    if pos == 0 || neg == 0 {
        return None;
    }
    let mut v: Vec<(f64, bool)> = points.to_vec();
    v.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    // Average ranks over ties.
    let mut rank_sum_pos = 0.0;
    let mut i = 0;
    while i < v.len() {
        let mut j = i;
        while j + 1 < v.len() && v[j + 1].0 == v[i].0 {
            j += 1;
        }
        let avg = (i + j) as f64 / 2.0 + 1.0;
        rank_sum_pos += avg * v[i..=j].iter().filter(|p| p.1).count() as f64;
        i = j + 1;
    }
    let u = rank_sum_pos - (pos * (pos + 1)) as f64 / 2.0;
    Some(u / (pos * neg) as f64)
}

fn metrics(points: &[(f64, bool)], open_probs: &[f64], threshold: f64) -> Metrics {
    let above = points.iter().filter(|p| p.0 >= threshold);
    let above_n = above.clone().count();
    let above_b = above.filter(|p| p.1).count();
    let b = points.iter().filter(|p| p.1).count();
    let a = points.len() - b;
    // The best `a` photos by score (lowest B-cut probability).
    let mut ranked: Vec<&(f64, bool)> = points.iter().collect();
    ranked.sort_by(|x, y| x.0.partial_cmp(&y.0).unwrap_or(std::cmp::Ordering::Equal));
    let a_top = ranked.iter().take(a).filter(|p| !p.1).count();
    let ratio = |n: usize, d: usize| if d == 0 { 0.0 } else { n as f64 / d as f64 };
    Metrics {
        auc: auc(points),
        b_precision: ratio(above_b, above_n),
        b_recall: ratio(above_b, b),
        a_precision: ratio(a_top, a),
        candidate_rate: ratio(open_probs.iter().filter(|p| **p >= threshold).count(), open_probs.len()),
    }
}

/// Cross-validated evaluation of the engine on `items` (labels from their ratings and flags,
/// as `settings` reads them). `folds` ≥ 2; `threshold` is the candidate threshold (0..1).
pub fn evaluate(items: &[Item], settings: &LearnSettings, folds: usize, threshold: f64) -> EvalReport {
    let folds = folds.max(2);
    let labels: Vec<Option<bool>> = items.iter().map(|it| settings.label(it.rating, it.flag)).collect();
    let judged: Vec<usize> = (0..items.len()).filter(|&i| labels[i].is_some()).collect();
    let open: Vec<usize> = (0..items.len()).filter(|&i| items[i].rating == 0 && items[i].flag == 0).collect();
    let cleared = |keep: &dyn Fn(usize) -> bool| -> Vec<Item> {
        items
            .iter()
            .enumerate()
            .map(|(i, it)| {
                let mut it = it.clone();
                if !keep(i) {
                    it.rating = 0;
                    it.flag = 0;
                }
                it
            })
            .collect()
    };

    // Prior: no labels at all.
    let (prior_scores, _, _) = learn::score(&cleared(&|_| false), settings);
    // Learned: each judged photo scored by a model trained without its fold.
    let mut learned_prob = vec![f64::NAN; items.len()];
    for f in 0..folds {
        let hidden: std::collections::HashSet<usize> =
            judged.iter().enumerate().filter(|(k, _)| k % folds == f).map(|(_, &i)| i).collect();
        if hidden.is_empty() {
            continue;
        }
        let (scores, _, _) = learn::score(&cleared(&|i| !hidden.contains(&i)), settings);
        for &i in &hidden {
            learned_prob[i] = scores[i].b_prob as f64;
        }
    }
    // Candidate list with everything learned (as in the app).
    let (all_scores, _, _) = learn::score(items, settings);

    let points = |probs: &dyn Fn(usize) -> f64, filter: &dyn Fn(usize) -> bool| -> Vec<(f64, bool)> {
        judged.iter().filter(|&&i| filter(i)).map(|&i| (probs(i), labels[i].unwrap())).collect()
    };
    let prior_p = |i: usize| prior_scores[i].b_prob as f64;
    let learned_p = |i: usize| learned_prob[i];
    let open_prior: Vec<f64> = open.iter().map(|&i| prior_p(i)).collect();
    let open_learned: Vec<f64> = open.iter().map(|&i| all_scores[i].b_prob as f64).collect();
    let all = |_: usize| true;

    let b = judged.iter().filter(|&&i| labels[i] == Some(true)).count();
    let mut report = EvalReport {
        photos: items.len(),
        a: judged.len() - b,
        b,
        folds,
        threshold,
        prior: metrics(&points(&prior_p, &all), &open_prior, threshold),
        learned: metrics(&points(&learned_p, &all), &open_learned, threshold),
        scenes: Vec::new(),
    };
    for s in Scene::ALL {
        let in_scene = |i: usize| items[i].scene == s;
        let pts = points(&learned_p, &in_scene);
        let b = pts.iter().filter(|p| p.1).count();
        if pts.is_empty() {
            continue;
        }
        report.scenes.push(SceneEval {
            scene: s,
            a: pts.len() - b,
            b,
            prior_auc: auc(&points(&prior_p, &in_scene)),
            learned_auc: auc(&pts),
        });
    }
    report
}

impl EvalReport {
    /// A short Korean summary (for the CLI and logs).
    pub fn summary(&self) -> String {
        let pct = |v: f64| format!("{:5.1}%", v * 100.0);
        let row = |name: &str, m: &Metrics| {
            format!(
                "{name:<8} AUC {}   B컷 정밀도 {}  재현율 {}   A컷 정밀도 {}   후보 비율 {}",
                m.auc.map(|v| format!("{v:.3}")).unwrap_or_else(|| "  -  ".into()),
                pct(m.b_precision),
                pct(m.b_recall),
                pct(m.a_precision),
                pct(m.candidate_rate)
            )
        };
        let mut s = format!(
            "사진 {}장 · 판정 A {} / B {} · {}겹 교차검증 · 후보 기준 {:.0}%\n{}\n{}\n",
            self.photos,
            self.a,
            self.b,
            self.folds,
            self.threshold * 100.0,
            row("기본 기준", &self.prior),
            row("학습", &self.learned),
        );
        if self.a == 0 || self.b == 0 {
            s.push_str("  A컷(픽 · ★3+)과 B컷(거절 · ★1)이 모두 있어야 정확도를 잴 수 있습니다\n");
        }
        for sc in &self.scenes {
            let f = |v: Option<f64>| v.map(|v| format!("{v:.3}")).unwrap_or_else(|| "  -  ".into());
            s.push_str(&format!(
                "  {:<8} A {:>4} / B {:>4}   AUC 기본 {}  학습 {}\n",
                sc.scene.label(),
                sc.a,
                sc.b,
                f(sc.prior_auc),
                f(sc.learned_auc)
            ));
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auc_basics() {
        assert_eq!(auc(&[(0.9, true), (0.1, false)]), Some(1.0));
        assert_eq!(auc(&[(0.1, true), (0.9, false)]), Some(0.0));
        assert_eq!(auc(&[(0.5, true), (0.5, false)]), Some(0.5));
        assert_eq!(auc(&[(0.5, true)]), None);
    }

    /// A photographer who rejects closed eyes much more strictly than the prior assumes:
    /// learning must beat the prior on photos it has not seen, and must not empty the list.
    #[test]
    fn learning_beats_the_prior_on_unseen_photos() {
        let mut seed = 7u64;
        let mut rnd = || {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            ((seed >> 33) as f64) / (1u64 << 31) as f64
        };
        let mut items: Vec<Item> = (0..1200i64)
            .map(|i| {
                let peak = 3.0 + rnd() as f32 * 4.0;
                let eyes = 0.3 + rnd() as f32 * 0.7;
                Item {
                    id: i + 1,
                    captured_ms: i * 60_000,
                    camera: "Cam".into(),
                    shutter: 1.0 / 500.0,
                    focal35: 85.0,
                    iso: 400,
                    rating: 0,
                    flag: 0,
                    scene: Scene::Portrait,
                    sharp_peak: peak,
                    sharp_global: peak - 1.0,
                    motion: 0.3,
                    clip_high: 0.0,
                    clip_low: 0.0,
                    brightness: 0.45,
                    face_area: 0.03,
                    face_sharp: Some(peak - 0.3),
                    eyes: Some(eyes),
                    dhash: (rnd() * u64::MAX as f64) as u64,
                }
            })
            .collect();
        // Hidden taste: a B-cut is soft (peak < 4) or has eyes below 0.65 open.
        let truth: Vec<bool> = items.iter().map(|it| it.sharp_peak < 4.0 || it.eyes.unwrap() < 0.65).collect();
        // The photographer has judged every third photo.
        for (i, it) in items.iter_mut().enumerate() {
            if i % 3 == 0 {
                it.flag = if truth[i] { -1 } else { 1 };
            }
        }
        let r = evaluate(&items, &LearnSettings::default(), 5, 0.6);
        println!("{}", r.summary());
        assert_eq!(r.a + r.b, 400);
        let (prior, learned) = (r.prior.auc.unwrap(), r.learned.auc.unwrap());
        assert!(learned > prior + 0.05, "prior {prior:.3} learned {learned:.3}");
        assert!(learned > 0.85, "learned {learned:.3}");
        assert!(r.learned.candidate_rate > 0.1, "the candidate list must not collapse: {:?}", r.learned);
    }
}
