//! B-cut probability: features, a hand-tuned prior and per-user logistic regression.
//!
//! Every photo becomes a feature vector where larger values mean "more likely a B-cut"
//! (blur, motion, clipping, closed eyes, worse than its burst siblings, ...). Without any
//! labels a hand-tuned prior model scores them. Whenever the user rates / rejects / picks, the
//! labels train a logistic regression that is regularised *towards the prior* — with few
//! labels it stays close to sensible defaults, with many it follows the user's taste. A
//! global model is trained first; per-scene models (portrait, landscape, ...) are trained on
//! top of it when a scene has enough labels of its own.

use super::scene::Scene;
use serde::{Deserialize, Serialize};

pub const N: usize = 14;

/// Feature names (for the settings / debug view).
pub const FEATURE_NAMES: [&str; N] = [
    "선명도(같은 장면 대비)",
    "선명도(절대)",
    "흔들림",
    "노출 과다",
    "노출 부족",
    "얼굴 초점",
    "눈 감음",
    "눈 감음(연사 대비)",
    "연사 중 선명도",
    "연사 베스트 아님",
    "느린 셔터",
    "고감도",
    "얼굴 있음",
    "전체 선명도(같은 장면 대비)",
];

/// Reason bits shown to the user.
pub mod reason {
    pub const BLUR: u16 = 1;
    pub const SHAKE: u16 = 2;
    pub const FACE_BLUR: u16 = 4;
    pub const EYES: u16 = 8;
    pub const OVER: u16 = 16;
    pub const UNDER: u16 = 32;
    pub const BURST: u16 = 64;
    pub const NOISE: u16 = 128;

    pub const ALL: [(u16, &str); 8] = [
        (BLUR, "초점 흐림"),
        (SHAKE, "흔들림"),
        (FACE_BLUR, "얼굴 초점 빗나감"),
        (EYES, "눈 감음 의심"),
        (OVER, "노출 과다"),
        (UNDER, "노출 부족"),
        (BURST, "연사 중 덜 선명"),
        (NOISE, "고감도 노이즈"),
    ];

    pub fn labels(mask: u16) -> Vec<&'static str> {
        ALL.iter().filter(|(b, _)| mask & b != 0).map(|(_, l)| *l).collect()
    }
}

const FEATURE_REASON: [u16; N] = [
    reason::BLUR,
    reason::BLUR,
    reason::SHAKE,
    reason::OVER,
    reason::UNDER,
    reason::FACE_BLUR,
    reason::EYES,
    reason::EYES,
    reason::BURST,
    reason::BURST,
    reason::SHAKE,
    reason::NOISE,
    0,
    reason::BLUR,
];

pub type Features = [f64; N];

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Model {
    pub w: Vec<f64>,
    pub b: f64,
}

impl Model {
    /// Hand-tuned defaults: a typical good photo scores ~10 %, a clearly blurred one ~90 %.
    pub fn prior() -> Model {
        Model { w: vec![2.5, 3.5, 2.0, 3.5, 3.5, 2.5, 3.0, 3.0, 2.5, 0.5, 0.8, 0.3, 0.0, 0.8], b: -4.2 }
    }

    pub fn logit(&self, x: &Features) -> f64 {
        self.b + self.w.iter().zip(x.iter()).map(|(w, x)| w * x).sum::<f64>()
    }

    pub fn prob(&self, x: &Features) -> f64 {
        sigmoid(self.logit(x))
    }

    /// Reason bits for features that push the score up noticeably (none for photos that are
    /// unlikely B-cuts anyway — "the least sharp of many good photos" is not a reason).
    pub fn reasons(&self, x: &Features) -> u16 {
        if self.prob(x) < 0.3 {
            return 0;
        }
        let mut m = 0;
        for i in 0..N {
            if self.w[i] * x[i] >= 1.4 {
                m |= FEATURE_REASON[i];
            }
        }
        m
    }
}

pub fn sigmoid(z: f64) -> f64 {
    1.0 / (1.0 + (-z).exp())
}

/// Solve `a·x = b` (Gaussian elimination with partial pivoting). `a` is n×n row-major.
fn solve(mut a: Vec<f64>, mut b: Vec<f64>, n: usize) -> Option<Vec<f64>> {
    for col in 0..n {
        let piv = (col..n).max_by(|&i, &j| a[i * n + col].abs().partial_cmp(&a[j * n + col].abs()).unwrap())?;
        if a[piv * n + col].abs() < 1e-12 {
            return None;
        }
        if piv != col {
            for k in 0..n {
                a.swap(col * n + k, piv * n + k);
            }
            b.swap(col, piv);
        }
        for row in col + 1..n {
            let f = a[row * n + col] / a[col * n + col];
            for k in col..n {
                a[row * n + k] -= f * a[col * n + k];
            }
            b[row] -= f * b[col];
        }
    }
    let mut x = vec![0.0; n];
    for row in (0..n).rev() {
        let mut s = b[row];
        for k in row + 1..n {
            s -= a[row * n + k] * x[k];
        }
        x[row] = s / a[row * n + row];
    }
    Some(x)
}

/// Class-balanced logistic regression, L2-regularised towards `prior` (Newton's method).
pub fn fit(xs: &[Features], ys: &[bool], prior: &Model, lambda: f64) -> Model {
    let d = N + 1;
    let n_pos = ys.iter().filter(|y| **y).count().max(1) as f64;
    let n_neg = ys.iter().filter(|y| !**y).count().max(1) as f64;
    let n = ys.len() as f64;
    let (c_pos, c_neg) = (n / (2.0 * n_pos), n / (2.0 * n_neg));
    let theta0: Vec<f64> = prior.w.iter().copied().chain(std::iter::once(prior.b)).collect();
    let mut theta = theta0.clone();
    // The intercept is only weakly tied to the prior.
    let reg = |j: usize| if j == N { lambda * 0.05 } else { lambda };
    for _ in 0..25 {
        let mut grad = vec![0.0; d];
        let mut hess = vec![0.0; d * d];
        for (x, &y) in xs.iter().zip(ys) {
            let z = theta[N] + (0..N).map(|j| theta[j] * x[j]).sum::<f64>();
            let p = sigmoid(z);
            let c = if y { c_pos } else { c_neg };
            let r = c * (p - if y { 1.0 } else { 0.0 });
            let s = c * p * (1.0 - p);
            for j in 0..d {
                let xj = if j == N { 1.0 } else { x[j] };
                grad[j] += r * xj;
                for k in 0..d {
                    let xk = if k == N { 1.0 } else { x[k] };
                    hess[j * d + k] += s * xj * xk;
                }
            }
        }
        for j in 0..d {
            grad[j] += reg(j) * (theta[j] - theta0[j]);
            hess[j * d + j] += reg(j);
        }
        let Some(step) = solve(hess, grad.clone(), d) else { break };
        let mut max_step = 0f64;
        for j in 0..d {
            theta[j] -= step[j];
            max_step = max_step.max(step[j].abs());
        }
        if max_step < 1e-6 {
            break;
        }
    }
    Model { w: theta[..N].to_vec(), b: theta[N] }
}

/// k-fold cross-validated balanced accuracy of `fit` (None if too few labels).
pub fn cross_validate(xs: &[Features], ys: &[bool], prior: &Model, lambda: f64, k: usize) -> Option<f64> {
    let pos = ys.iter().filter(|y| **y).count();
    let neg = ys.len() - pos;
    if pos < k || neg < k {
        return None;
    }
    let (mut tp, mut tn) = (0usize, 0usize);
    for fold in 0..k {
        let (mut tx, mut ty) = (Vec::new(), Vec::new());
        for i in 0..xs.len() {
            if i % k != fold {
                tx.push(xs[i]);
                ty.push(ys[i]);
            }
        }
        let m = fit(&tx, &ty, prior, lambda);
        for i in (fold..xs.len()).step_by(k) {
            let pred = m.prob(&xs[i]) >= 0.5;
            if pred && ys[i] {
                tp += 1;
            }
            if !pred && !ys[i] {
                tn += 1;
            }
        }
    }
    Some(0.5 * (tp as f64 / pos as f64 + tn as f64 / neg as f64))
}

/// Everything about one photo needed to compute its features.
#[derive(Clone, Debug)]
pub struct Item {
    pub id: i64,
    pub captured_ms: i64,
    pub camera: String,
    pub shutter: f32,
    pub focal35: f32,
    pub iso: u32,
    pub rating: u8,
    pub flag: i8,
    pub scene: Scene,
    pub sharp_peak: f32,
    pub sharp_global: f32,
    pub motion: f32,
    pub clip_high: f32,
    pub clip_low: f32,
    pub brightness: f32,
    pub face_area: f32,
    pub face_sharp: Option<f32>,
    pub eyes: Option<f32>,
    pub dhash: u64,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub struct LearnSettings {
    /// Count ★1 as a B-cut label.
    pub one_star_is_b: bool,
    /// Ratings >= this count as A-cut labels (0 disables).
    pub a_min_rating: u8,
}

impl Default for LearnSettings {
    fn default() -> Self {
        LearnSettings { one_star_is_b: true, a_min_rating: 3 }
    }
}

impl LearnSettings {
    pub fn label(&self, rating: u8, flag: i8) -> Option<bool> {
        if flag < 0 {
            return Some(true);
        }
        if flag > 0 {
            return Some(false);
        }
        if self.one_star_is_b && rating == 1 {
            return Some(true);
        }
        if self.a_min_rating > 0 && rating >= self.a_min_rating {
            return Some(false);
        }
        None
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Score {
    pub id: i64,
    pub b_prob: f32,
    pub reasons: u16,
    /// Asset id of the first frame of the burst (0 = not part of a burst).
    pub burst: i64,
    /// 0 = best frame of its burst.
    pub burst_rank: u16,
    pub burst_size: u16,
    /// Effective scene used for scoring.
    pub scene: u8,
}

#[derive(Clone, Debug, Default)]
pub struct SceneReport {
    pub scene: Option<Scene>,
    pub a: usize,
    pub b: usize,
    /// "기본값" / "전체 학습" / "장면별 학습"
    pub model: &'static str,
    pub accuracy: Option<f64>,
}

#[derive(Clone, Debug, Default)]
pub struct Report {
    pub scenes: Vec<SceneReport>,
    pub analysed: usize,
    pub candidates_at_60: usize,
}

pub const BURST_GAP_MS: i64 = 3000;
pub const BURST_HAMMING: u32 = 18;
const LAMBDA: f64 = 3.0;
const MIN_GLOBAL: usize = 6;
const MIN_SCENE: usize = 10;

fn percentiles(values: &[(usize, f32)]) -> Vec<(usize, f64)> {
    let mut v: Vec<(usize, f32)> = values.to_vec();
    v.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
    let n = v.len().max(2) as f64 - 1.0;
    v.iter().enumerate().map(|(rank, (i, _))| (*i, rank as f64 / n)).collect()
}

/// How much the motion-blur measurement can be trusted at this peak sharpness (0..1).
pub fn motion_reliability(sharp_peak: f32) -> f64 {
    ((sharp_peak as f64 - 1.5) / 1.5).clamp(0.0, 1.0)
}

/// Compute features for all items (needs the whole set for percentiles and bursts).
pub fn features(items: &[Item]) -> (Vec<Features>, Vec<Score>) {
    let n = items.len();
    let mut peak_pct = vec![0.5f64; n];
    let mut global_pct = vec![0.5f64; n];
    // Percentiles within each scene (whole library when a scene is small).
    let mut by_scene: std::collections::HashMap<Scene, Vec<usize>> = Default::default();
    for (i, it) in items.iter().enumerate() {
        by_scene.entry(it.scene).or_default().push(i);
    }
    let all: Vec<usize> = (0..n).collect();
    for idxs in by_scene.values() {
        let pool = if idxs.len() >= 20 { idxs } else { &all };
        let peaks = percentiles(&pool.iter().map(|&i| (i, items[i].sharp_peak)).collect::<Vec<_>>());
        let globals = percentiles(&pool.iter().map(|&i| (i, items[i].sharp_global)).collect::<Vec<_>>());
        let set: std::collections::HashSet<usize> = idxs.iter().copied().collect();
        for (i, p) in peaks {
            if set.contains(&i) {
                peak_pct[i] = p;
            }
        }
        for (i, p) in globals {
            if set.contains(&i) {
                global_pct[i] = p;
            }
        }
    }
    // Bursts: consecutive frames of one camera, close in time and visually similar.
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&a, &b| items[a].camera.cmp(&items[b].camera).then(items[a].captured_ms.cmp(&items[b].captured_ms)));
    let mut group_of = vec![usize::MAX; n];
    let mut groups: Vec<Vec<usize>> = Vec::new();
    for (k, &i) in order.iter().enumerate() {
        let joined = k > 0 && {
            let p = order[k - 1];
            items[p].camera == items[i].camera
                && (items[i].captured_ms - items[p].captured_ms).abs() <= BURST_GAP_MS
                && super::metrics::hamming(items[p].dhash, items[i].dhash) <= BURST_HAMMING
        };
        if joined {
            let g = group_of[order[k - 1]];
            groups[g].push(i);
            group_of[i] = g;
        } else {
            group_of[i] = groups.len();
            groups.push(vec![i]);
        }
    }
    let mut scores = vec![Score::default(); n];
    let mut burst_rel = vec![0f64; n];
    let mut eyes_rel = vec![0f64; n];
    let mut not_best = vec![0f64; n];
    for g in &groups {
        for &i in g {
            scores[i].id = items[i].id;
            scores[i].scene = items[i].scene as u8;
        }
        if g.len() < 2 {
            continue;
        }
        let quality = |i: usize| items[i].sharp_peak as f64 + items[i].eyes.map(|e| e as f64 * 1.5).unwrap_or(0.0);
        let mut ranked = g.clone();
        ranked.sort_by(|&a, &b| quality(b).partial_cmp(&quality(a)).unwrap_or(std::cmp::Ordering::Equal));
        let best_peak = g.iter().map(|&i| items[i].sharp_peak).fold(f32::MIN, f32::max);
        let best_eyes =
            g.iter().filter_map(|&i| items[i].eyes).fold(None, |a: Option<f32>, e| Some(a.map_or(e, |a| a.max(e))));
        let first = g.iter().map(|&i| (items[i].captured_ms, items[i].id)).min().map(|x| x.1).unwrap_or(0);
        for (rank, &i) in ranked.iter().enumerate() {
            scores[i].burst = first;
            scores[i].burst_rank = rank as u16;
            scores[i].burst_size = g.len() as u16;
            // Laplacian variance ratio to the sharpest sibling.
            burst_rel[i] = (1.0 - ((items[i].sharp_peak - best_peak) as f64).exp()).clamp(0.0, 1.0);
            if let (Some(best), Some(e)) = (best_eyes, items[i].eyes) {
                eyes_rel[i] = ((best - e) as f64).clamp(0.0, 1.0);
            }
            not_best[i] = if rank > 0 { 1.0 } else { 0.0 };
        }
    }
    let feats = items
        .iter()
        .enumerate()
        .map(|(i, it)| {
            let mut x = [0f64; N];
            x[0] = 1.0 - peak_pct[i];
            // ln(1+var): ~6+ crisp detail, ~3 soft, <2 very blurred (at 1600 px).
            x[1] = ((4.0 - it.sharp_peak as f64) / 2.5).clamp(0.0, 1.0);
            // The direction of the remaining gradients is meaningless once everything is blurred
            // (that case is covered by the focus features).
            x[2] = ((it.motion as f64 - 0.45) / 0.4).clamp(0.0, 1.0) * motion_reliability(it.sharp_peak);
            // A little clipping (a bright sky) is normal; a quarter of the frame blown out is not.
            x[3] = ((it.clip_high as f64 - 0.03).max(0.0) * 3.5 + (it.brightness as f64 - 0.7).max(0.0) * 3.0)
                .clamp(0.0, 1.5);
            // Night scenes are legitimately dark (~0.1–0.2); a frame averaging 5 % is not.
            x[4] = (it.clip_low as f64 * 2.0 + (0.16 - it.brightness as f64).max(0.0) * 12.0).clamp(0.0, 1.5);
            x[5] = match it.face_sharp {
                // Face clearly softer than the sharpest part of the frame → focus missed the face.
                Some(fs) if it.face_area >= 0.004 => ((it.sharp_peak as f64 - fs as f64 - 0.7) / 1.5).clamp(0.0, 1.0),
                _ => 0.0,
            };
            x[6] = it.eyes.map(|e| (1.0 - e as f64).clamp(0.0, 1.0)).unwrap_or(0.0);
            x[7] = eyes_rel[i];
            x[8] = burst_rel[i];
            x[9] = not_best[i];
            x[10] = if it.shutter > 0.0 && it.focal35 > 0.0 {
                ((it.shutter as f64 * it.focal35 as f64).log2() / 3.0).clamp(0.0, 1.0)
            } else {
                0.0
            };
            x[11] = if it.iso > 0 { ((it.iso as f64 / 800.0).log2() / 4.0).clamp(0.0, 1.0) } else { 0.0 };
            x[12] = if it.face_area >= 0.004 { 1.0 } else { 0.0 };
            x[13] = 1.0 - global_pct[i];
            x
        })
        .collect();
    (feats, scores)
}

/// Trained models: `None` = the library-wide model, `Some(scene)` = a per-scene model.
pub type Models = Vec<(Option<Scene>, Model)>;

/// Train models from labelled items and score everything.
pub fn score(items: &[Item], settings: &LearnSettings) -> (Vec<Score>, Report, Models) {
    let (feats, mut scores) = features(items);
    let labels: Vec<Option<bool>> = items.iter().map(|it| settings.label(it.rating, it.flag)).collect();
    let prior = Model::prior();
    let (gx, gy): (Vec<Features>, Vec<bool>) =
        feats.iter().zip(&labels).filter_map(|(x, y)| y.map(|y| (*x, y))).unzip();
    let g_b = gy.iter().filter(|y| **y).count();
    let g_a = gy.len() - g_b;
    let mut report = Report { analysed: items.len(), ..Default::default() };
    let global = if g_a >= MIN_GLOBAL && g_b >= MIN_GLOBAL { fit(&gx, &gy, &prior, LAMBDA) } else { prior.clone() };
    report.scenes.push(SceneReport {
        scene: None,
        a: g_a,
        b: g_b,
        model: if global == prior { "기본값" } else { "전체 학습" },
        accuracy: cross_validate(&gx, &gy, &prior, LAMBDA, 5),
    });
    let mut models: Models = vec![(None, global.clone())];
    let mut scene_model: std::collections::HashMap<Scene, Model> = Default::default();
    for s in Scene::ALL {
        let (sx, sy): (Vec<Features>, Vec<bool>) = feats
            .iter()
            .zip(&labels)
            .zip(items)
            .filter(|(_, it)| it.scene == s)
            .filter_map(|((x, y), _)| y.map(|y| (*x, y)))
            .unzip();
        let b = sy.iter().filter(|y| **y).count();
        let a = sy.len() - b;
        let count = items.iter().filter(|it| it.scene == s).count();
        if count == 0 {
            continue;
        }
        let (model, name) = if a >= MIN_SCENE && b >= MIN_SCENE {
            (fit(&sx, &sy, &global, LAMBDA), "장면별 학습")
        } else {
            (global.clone(), if global == prior { "기본값" } else { "전체 학습" })
        };
        report.scenes.push(SceneReport {
            scene: Some(s),
            a,
            b,
            model: name,
            accuracy: cross_validate(&sx, &sy, &global, LAMBDA, 5),
        });
        if name == "장면별 학습" {
            models.push((Some(s), model.clone()));
        }
        scene_model.insert(s, model);
    }
    for (i, it) in items.iter().enumerate() {
        let m = scene_model.get(&it.scene).unwrap_or(&global);
        scores[i].b_prob = m.prob(&feats[i]) as f32;
        scores[i].reasons = m.reasons(&feats[i]);
    }
    report.candidates_at_60 = scores.iter().zip(items).filter(|(s, it)| s.b_prob >= 0.6 && it.flag == 0).count();
    (scores, report, models)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: i64, t: i64, peak: f32) -> Item {
        Item {
            id,
            captured_ms: t,
            camera: "Cam".into(),
            shutter: 1.0 / 500.0,
            focal35: 50.0,
            iso: 200,
            rating: 0,
            flag: 0,
            scene: Scene::Landscape,
            sharp_peak: peak,
            sharp_global: peak - 1.0,
            motion: 0.3,
            clip_high: 0.0,
            clip_low: 0.0,
            brightness: 0.45,
            face_area: 0.0,
            face_sharp: None,
            eyes: None,
            dhash: (id as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15),
        }
    }

    #[test]
    fn prior_flags_blur() {
        let mut items: Vec<Item> = (0..40).map(|i| item(i + 1, i * 60_000, 6.0 + (i % 5) as f32 * 0.2)).collect();
        items.push(item(100, 9_000_000, 1.5)); // very blurred
        let (scores, _, _) = score(&items, &LearnSettings::default());
        let blurred = scores.iter().find(|s| s.id == 100).unwrap();
        let typical: f32 = scores.iter().filter(|s| s.id < 100).map(|s| s.b_prob).sum::<f32>() / 40.0;
        assert!(blurred.b_prob > 0.8, "blurred {blurred:?}");
        assert!(typical < 0.25, "typical {typical}");
        assert!(blurred.reasons & reason::BLUR != 0);
    }

    #[test]
    fn bursts_rank_by_sharpness() {
        let mut items = vec![item(1, 0, 6.5), item(2, 400, 5.0), item(3, 800, 6.8), item(4, 60_000, 6.0)];
        for it in &mut items[..3] {
            it.dhash = 0xFFFF_0000_FFFF_0000;
        }
        let (scores, _, _) = score(&items, &LearnSettings::default());
        let s = |id: i64| scores.iter().find(|s| s.id == id).unwrap();
        assert_eq!(s(1).burst_size, 3);
        assert_eq!(s(3).burst_rank, 0, "sharpest frame is best");
        assert_eq!(s(2).burst_rank, 2);
        assert_eq!(s(4).burst_size, 0);
        assert!(s(2).b_prob > s(3).b_prob);
        assert!(s(2).reasons & reason::BURST != 0);
    }

    #[test]
    fn learns_user_taste() {
        // This user rejects anything shot above ISO 3200 even when sharp — the prior barely
        // cares about ISO, the trained model must pick it up.
        let mut items = Vec::new();
        for i in 0..120i64 {
            let mut it = item(i + 1, i * 60_000, 6.0 + (i % 7) as f32 * 0.1);
            it.iso = if i % 2 == 0 { 12800 } else { 200 };
            it.flag = if i % 2 == 0 { -1 } else { 1 };
            items.push(it);
        }
        let mut test_hi = item(500, 99_000_000, 6.3);
        test_hi.iso = 12800;
        let mut test_lo = item(501, 99_900_000, 6.3);
        test_lo.iso = 200;
        items.push(test_hi);
        items.push(test_lo);
        let (scores, report, _) = score(&items, &LearnSettings::default());
        let p = |id: i64| scores.iter().find(|s| s.id == id).unwrap().b_prob;
        assert!(p(500) > 0.7 && p(501) < 0.3, "hi {} lo {}", p(500), p(501));
        assert_eq!(report.scenes[0].model, "전체 학습");
        assert!(report.scenes[0].accuracy.unwrap() > 0.9);
        let land = report.scenes.iter().find(|r| r.scene == Some(Scene::Landscape)).unwrap();
        assert_eq!(land.model, "장면별 학습");
    }

    #[test]
    fn labels_from_ratings() {
        let s = LearnSettings::default();
        assert_eq!(s.label(1, 0), Some(true));
        assert_eq!(s.label(4, 0), Some(false));
        assert_eq!(s.label(2, 0), None);
        assert_eq!(s.label(5, -1), Some(true));
        assert_eq!(s.label(0, 1), Some(false));
    }

    /// `cargo test --release -p photoorg-core bench_score -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn bench_score_20k() {
        let scenes = [Scene::Portrait, Scene::Landscape, Scene::City, Scene::Food];
        let items: Vec<Item> = (0..20_000i64)
            .map(|i| {
                let mut it = item(i, i * 1500, 2.0 + (i % 50) as f32 / 10.0);
                it.scene = scenes[(i % 4) as usize];
                it.camera = format!("Cam{}", i % 3);
                if i % 7 == 0 {
                    it.flag = if it.sharp_peak < 4.0 { -1 } else { 1 };
                }
                it
            })
            .collect();
        let t = std::time::Instant::now();
        let (scores, report, _) = score(&items, &LearnSettings::default());
        println!("score 20k: {:?} ({} scores, {} scene reports)", t.elapsed(), scores.len(), report.scenes.len());
    }
}
