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

pub const N: usize = 24;
/// Index of the taste features ([`taste_features`]): similarity to the photographer's B-cuts,
/// then to their A-cuts.
pub const TASTE_B: usize = 14;
pub const TASTE_A: usize = 15;
/// Composition rules ([`crate::composition::judge`]), 0..1 each.
pub const THIRDS: usize = 16;
pub const HEADROOM: usize = 17;
pub const LEAD_ROOM: usize = 18;
pub const MERGER: usize = 19;
pub const TILT: usize = 20;
pub const CUT: usize = 21;
pub const FOCUS_BG: usize = 22;
/// How far the composition is from the photographer's own A-cuts ([`composition_features`]).
pub const MY_COMPOSITION: usize = 23;

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
    "내 B컷과 닮음",
    "내 A컷과 닮음",
    "3분할에서 벗어남",
    "헤드룸",
    "시선 앞 공간 부족",
    "머리 뒤 기둥 · 선",
    "수평 · 수직 기울어짐",
    "얼굴이 프레임에 잘림",
    "배경에 초점",
    "내 구도와 다름",
];

/// Reason bits shown to the user. They are also the B-cut rules the photographer can turn off
/// ([`LearnSettings::off`]).
pub mod reason {
    pub const BLUR: u32 = 1;
    pub const SHAKE: u32 = 2;
    pub const FACE_BLUR: u32 = 4;
    pub const EYES: u32 = 8;
    pub const OVER: u32 = 16;
    pub const UNDER: u32 = 32;
    pub const BURST: u32 = 64;
    pub const NOISE: u32 = 128;
    pub const TASTE: u32 = 256;
    pub const THIRDS: u32 = 1 << 9;
    pub const HEADROOM: u32 = 1 << 10;
    pub const LEAD_ROOM: u32 = 1 << 11;
    pub const MERGER: u32 = 1 << 12;
    pub const TILT: u32 = 1 << 13;
    pub const CUT: u32 = 1 << 14;
    pub const FOCUS_BG: u32 = 1 << 15;
    pub const MY_COMPOSITION: u32 = 1 << 16;

    pub const ALL: [(u32, &str); 17] = [
        (BLUR, "초점 흐림"),
        (SHAKE, "흔들림"),
        (FACE_BLUR, "얼굴 초점 빗나감"),
        (EYES, "눈 감음 의심"),
        (OVER, "노출 과다"),
        (UNDER, "노출 부족"),
        (BURST, "연사 중 덜 선명"),
        (NOISE, "고감도 노이즈"),
        (TASTE, "내가 거절한 사진과 비슷"),
        (THIRDS, "3분할에서 벗어남"),
        (HEADROOM, "헤드룸이 어색함"),
        (LEAD_ROOM, "시선 앞 공간 부족"),
        (MERGER, "머리 뒤 기둥 · 선"),
        (TILT, "수평 · 수직 기울어짐"),
        (CUT, "얼굴이 프레임에 잘림"),
        (FOCUS_BG, "초점이 배경에"),
        (MY_COMPOSITION, "내 구도와 다름"),
    ];

    /// The composition rules (구도), as opposed to focus, exposure and the rest.
    pub const COMPOSITION: u32 = THIRDS | HEADROOM | LEAD_ROOM | MERGER | TILT | CUT | MY_COMPOSITION;

    pub fn labels(mask: u32) -> Vec<&'static str> {
        ALL.iter().filter(|(b, _)| mask & b != 0).map(|(_, l)| *l).collect()
    }
}

const FEATURE_REASON: [u32; N] = [
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
    reason::TASTE,
    // Similarity to the A-cuts is part of the taste rule.
    reason::TASTE,
    reason::THIRDS,
    reason::HEADROOM,
    reason::LEAD_ROOM,
    reason::MERGER,
    reason::TILT,
    reason::CUT,
    reason::FOCUS_BG,
    reason::MY_COMPOSITION,
];

pub type Features = [f64; N];

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Model {
    pub w: Vec<f64>,
    pub b: f64,
}

impl Model {
    /// Hand-tuned defaults: a typical good photo scores ~10 %, a clearly blurred one ~90 %.
    /// One broken framing rule alone does not make a B-cut (a centred portrait can be the
    /// photographer's style); with a soft focus or another broken rule it does.
    pub fn prior() -> Model {
        // Taste and "my composition" start at zero: only the photographer's own decisions give
        // them weight.
        Model {
            w: vec![
                2.5, 3.5, 2.0, 3.5, 3.5, 2.5, 3.0, 3.0, 2.5, 0.5, 0.8, 0.3, 0.0, 0.8, 0.0, 0.0, // quality
                1.5, 1.8, 1.6, 1.6, 1.6, 2.4, 2.4, 0.0, // composition
            ],
            b: -4.2,
        }
    }

    pub fn logit(&self, x: &Features) -> f64 {
        self.b + self.w.iter().zip(x.iter()).map(|(w, x)| w * x).sum::<f64>()
    }

    pub fn prob(&self, x: &Features) -> f64 {
        sigmoid(self.logit(x))
    }

    /// Reason bits for features that push the score up noticeably (none for photos that are
    /// unlikely B-cuts anyway — "the least sharp of many good photos" is not a reason).
    pub fn reasons(&self, x: &Features) -> u32 {
        if self.prob(x) < 0.3 {
            return 0;
        }
        let mut m = 0;
        for i in 0..N.min(self.w.len()) {
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
    for _ in 0..60 {
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
        let Some(mut step) = solve(hess, grad.clone(), d) else { break };
        // Damped Newton: a full step from far away can overshoot and diverge.
        let max_step = step.iter().fold(0f64, |m, s| m.max(s.abs()));
        if max_step > 2.0 {
            step.iter_mut().for_each(|s| *s *= 2.0 / max_step);
        }
        for j in 0..d {
            theta[j] -= step[j];
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
    /// Taste embedding ([`crate::taste::embed`]; empty when unknown).
    pub embed: Vec<f32>,
    /// The photo itself was deleted; only the photographer's decision and its measurements are
    /// kept, so the learned standard does not forget it. Never part of a burst.
    pub gone: bool,
    /// Framing ([`crate::composition::measure`]; default = not measured).
    pub comp: crate::composition::Composition,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub struct LearnSettings {
    /// Count ★1 as a B-cut label.
    pub one_star_is_b: bool,
    /// Ratings >= this count as A-cut labels (0 disables).
    pub a_min_rating: u8,
    /// B-cut rules the photographer turned off ([`reason`] bits): the measurements are ignored,
    /// neither the defaults nor the learned standard use them, and they are never given as a
    /// reason (nor as an A-cut strength).
    #[serde(default)]
    pub off: u32,
}

impl Default for LearnSettings {
    fn default() -> Self {
        LearnSettings { one_star_is_b: true, a_min_rating: 3, off: 0 }
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
    pub reasons: u32,
    /// What the photo does well ([`crate::composition::strength`] bits), for A-cut candidates.
    pub strengths: u32,
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
    // Bursts: consecutive frames of one camera, close in time and visually similar. Deleted
    // photos (kept for learning) stand alone: they are not there to compare with.
    let mut order: Vec<usize> = (0..n).filter(|&i| !items[i].gone).collect();
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
    for i in (0..n).filter(|&i| items[i].gone) {
        group_of[i] = groups.len();
        groups.push(vec![i]);
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
            // Detail is not focus: a busy background (a flag, foliage) has more of it than a face
            // even when the face is sharp. When the person's edges are at least as crisp as the
            // background's, the focus is on the person.
            if let (Some(s), Some(b)) = (it.comp.subject_sharp, it.comp.bg_sharp) {
                if s >= b - 0.1 {
                    x[5] *= 0.3;
                }
            }
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
            let j = crate::composition::judge(&it.comp);
            x[THIRDS] = j.thirds as f64;
            x[HEADROOM] = j.headroom as f64;
            x[LEAD_ROOM] = j.lead_room as f64;
            x[MERGER] = j.merger as f64;
            x[TILT] = j.tilt as f64;
            x[CUT] = j.cut as f64;
            x[FOCUS_BG] = j.focus_bg as f64;
            x
        })
        .collect();
    (feats, scores)
}

/// Same moment: one burst, or one camera within a minute. Such photos look alike because they
/// show the same scene; choosing among them is what the burst features are for.
fn same_moment(items: &[Item], scores: &[Score], cam: &[usize], i: usize, j: usize) -> bool {
    const MOMENT_MS: i64 = 60_000;
    (scores[i].burst != 0 && scores[i].burst == scores[j].burst)
        || (cam[i] == cam[j] && (items[i].captured_ms - items[j].captured_ms).abs() <= MOMENT_MS)
}

/// Camera names as numbers (compared in inner loops).
fn camera_ids(items: &[Item]) -> Vec<usize> {
    let mut cams: Vec<&str> = items.iter().map(|it| it.camera.as_str()).collect();
    cams.sort_unstable();
    cams.dedup();
    items.iter().map(|it| cams.binary_search(&it.camera.as_str()).unwrap_or(0)).collect()
}

/// Standard scores over the photos that have a value (0 = typical), clamped to ±3; 0 for the rest.
fn standardize(v: Vec<Option<f64>>) -> Vec<f64> {
    let known: Vec<f64> = v.iter().flatten().copied().collect();
    if known.is_empty() {
        return vec![0.0; v.len()];
    }
    let m = known.iter().sum::<f64>() / known.len() as f64;
    let sd = (known.iter().map(|x| (x - m).powi(2)).sum::<f64>() / known.len() as f64).sqrt().max(0.02);
    v.into_iter().map(|x| x.map_or(0.0, |x| ((x - m) / sd).clamp(-3.0, 3.0))).collect()
}

/// My compositions (내 구도): photographers come back to a handful of framings — a tight
/// portrait with the eyes on the upper third, a half-length figure on the left third, a full
/// figure small in a wide frame. Every A-cut is one example; a photo framed like none of them is
/// "not how I frame". For each photo: the mean distance of its composition ([`vector`]) to the
/// five closest A-cuts of the same scene (of any scene while a scene has fewer than
/// [`MIN_COMPOSITIONS`]), other moments only, as a standard score (positive = farther than usual).
/// Several framings are several clusters of A-cuts; being close to any of them is enough.
///
/// [`vector`]: crate::composition::vector
///
/// Returns each photo's mean distance (None without enough A-cuts or without a subject).
fn composition_features(
    items: &[Item],
    labels: &[Option<bool>],
    scores: &[Score],
    feats: &mut [Features],
) -> Vec<Option<f64>> {
    const K: usize = 5;
    const MIN_COMPOSITIONS: usize = 8;
    let n = items.len();
    let vecs: Vec<Option<[f32; 6]>> = items.iter().map(|it| crate::composition::vector(&it.comp)).collect();
    let a_cuts: Vec<usize> = (0..n).filter(|&i| labels[i] == Some(false) && vecs[i].is_some()).collect();
    if a_cuts.len() < MIN_COMPOSITIONS {
        return vec![None; n];
    }
    let mut by_scene: std::collections::HashMap<Scene, Vec<usize>> = Default::default();
    for &i in &a_cuts {
        by_scene.entry(items[i].scene).or_default().push(i);
    }
    let cam = camera_ids(items);
    use rayon::prelude::*;
    let dist: Vec<Option<f64>> = (0..n)
        .into_par_iter()
        .map(|i| {
            let v = vecs[i]?;
            let pool = by_scene.get(&items[i].scene).filter(|p| p.len() >= MIN_COMPOSITIONS).unwrap_or(&a_cuts);
            let mut d: Vec<f32> = pool
                .iter()
                .filter(|&&j| j != i && !same_moment(items, scores, &cam, i, j))
                .map(|&j| {
                    let u = vecs[j].unwrap();
                    v.iter().zip(&u).map(|(a, b)| (a - b) * (a - b)).sum::<f32>().sqrt()
                })
                .collect();
            if d.is_empty() {
                return None;
            }
            let k = K.min(d.len());
            d.select_nth_unstable_by(k - 1, |a, b| a.total_cmp(b));
            Some(d[..k].iter().map(|x| *x as f64).sum::<f64>() / k as f64)
        })
        .collect();
    for (x, z) in feats.iter_mut().zip(standardize(dist.clone())) {
        x[MY_COMPOSITION] = z;
    }
    dist
}

/// Taste ([`crate::taste`]): for every photo, the mean similarity of the five most similar
/// photos the photographer rejected (`x[TASTE_B]`) and kept (`x[TASTE_A]`). Photos of the same
/// moment (one burst, or one camera within a minute) are left out — they look alike because they
/// show the same scene, and choosing among them is what the burst features are for. A photo is
/// never compared with itself. Both are standard scores over the library (0 = as similar as a
/// typical photo), so that small differences can be learned and only a photo noticeably closer to
/// the B-cuts than most gets that reason. Photos without an embedding, or without judged photos of
/// a kind, get 0 (no push either way).
fn taste_features(items: &[Item], labels: &[Option<bool>], scores: &[Score], feats: &mut [Features]) {
    const K: usize = 5;
    let dim = crate::taste::DIM;
    let n = items.len();
    let embeds: Vec<&[f32]> = items.iter().map(|it| it.embed.as_slice()).collect();
    let unit = crate::taste::centred(&embeds);
    // The judged photos of each kind, their vectors side by side (fast to scan).
    struct Set {
        idx: Vec<usize>,
        vecs: Vec<f32>,
    }
    let set = |want: bool| -> Set {
        let idx: Vec<usize> = (0..n).filter(|&i| labels[i] == Some(want) && unit[i].is_some()).collect();
        let vecs = idx.iter().flat_map(|&i| unit[i].as_ref().unwrap().iter().copied()).collect();
        Set { idx, vecs }
    };
    let (b_set, a_set) = (set(true), set(false));
    if b_set.idx.is_empty() && a_set.idx.is_empty() {
        return;
    }
    let cam = camera_ids(items);
    let same_moment = |i: usize, j: usize| same_moment(items, scores, &cam, i, j);
    let nearest = |i: usize, s: &Set| -> Option<f64> {
        let u = unit[i].as_ref()?;
        let mut top = [f32::MIN; K];
        let mut found = 0;
        for (k, &j) in s.idx.iter().enumerate() {
            if j == i || same_moment(i, j) {
                continue;
            }
            let sim = crate::taste::dot(u, &s.vecs[k * dim..(k + 1) * dim]);
            found += 1;
            // Keep the K largest (top is sorted descending).
            if sim > top[K - 1] {
                let mut p = K - 1;
                while p > 0 && top[p - 1] < sim {
                    top[p] = top[p - 1];
                    p -= 1;
                }
                top[p] = sim;
            }
        }
        let k = found.min(K);
        (k > 0).then(|| top[..k].iter().map(|v| *v as f64).sum::<f64>() / k as f64)
    };
    use rayon::prelude::*;
    let values: Vec<(Option<f64>, Option<f64>)> =
        (0..n).into_par_iter().map(|i| (nearest(i, &b_set), nearest(i, &a_set))).collect();
    let (b, a): (Vec<_>, Vec<_>) = values.into_iter().unzip();
    for (x, (b, a)) in feats.iter_mut().zip(standardize(b).into_iter().zip(standardize(a))) {
        x[TASTE_B] = b;
        x[TASTE_A] = a;
    }
}

/// Guard a trained model against worst-first selection collapse without inflating B-cut scores.
///
/// Class-balanced [`fit`] already places the 50 % boundary midway between the photographer's A-
/// and B-cuts regardless of how many more `X` or `P` labels exist. The one case where that
/// boundary drifts too low is when the photographer rejects only the most extreme candidates
/// first: `fit` then thinks only extreme blur is a B-cut, and the candidate list collapses far
/// below what the prior expects on `open`.
///
/// We therefore only lift the intercept when `fit`'s mean on `open` falls below a floor based on
/// the prior's mean on `open` (scaled down — never above 1.0 — when the user keeps photos the
/// prior flagged). Never amplifying `strictness` above 1.0 and never pulling a healthy learned
/// model down prevents imbalanced `B >> A` labels (or deleted burst frames with `gone = true`)
/// from pushing the whole library — including top A-cuts — toward 95 % B-cut, and prevents
/// `A >> B` star-rating workflows from wiping out the candidate list.
pub fn anchor(model: &Model, prior: &Model, open: &[&Features], judged: &[(&Features, bool)]) -> Model {
    if open.is_empty() || model == prior {
        return model.clone();
    }
    let prior_open = open.iter().map(|x| prior.prob(x)).sum::<f64>() / open.len() as f64;
    let fit_open = open.iter().map(|x| model.prob(x)).sum::<f64>() / open.len() as f64;
    let expected: f64 = judged.iter().map(|(x, _)| prior.prob(x)).sum();
    let actual = judged.iter().filter(|(_, b)| *b).count() as f64;
    let strictness = ((actual + 3.0) / (expected + 3.0)).clamp(0.2, 1.0);
    let floor = (prior_open * strictness).clamp(0.001, 0.65);
    if fit_open >= floor {
        return model.clone();
    }
    let mean = |shift: f64| open.iter().map(|x| sigmoid(model.logit(x) + shift)).sum::<f64>() / open.len() as f64;
    let (mut lo, mut hi) = (0.0, 30.0);
    for _ in 0..60 {
        let mid = (lo + hi) / 2.0;
        if mean(mid) < floor {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    Model { w: model.w.clone(), b: model.b + (lo + hi) / 2.0 }
}

/// What a photo does well (A-cut reasons), leaving out the rules turned off.
fn strengths(it: &Item, x: &Features, s: &Score, comp_dist: Option<f64>, off: u32) -> u32 {
    use crate::composition::strength;
    let mut good = crate::composition::judge(&it.comp).good;
    let rule = [
        (strength::THIRDS, reason::THIRDS),
        (strength::HEADROOM, reason::HEADROOM),
        (strength::LEAD_ROOM, reason::LEAD_ROOM),
        (strength::LEVEL, reason::TILT),
        (strength::ISOLATED, reason::FOCUS_BG),
    ];
    for (g, r) in rule {
        if off & r != 0 {
            good &= !g;
        }
    }
    // The eyes in focus and open: the face as sharp as the sharpest part of the frame.
    if off & (reason::FACE_BLUR | reason::EYES) == 0
        && it.face_area >= 0.004
        && it.face_sharp.is_some_and(|fs| fs >= it.sharp_peak - 0.5 && fs >= 3.5)
        && it.eyes.is_some_and(|e| e >= 0.6)
    {
        good |= strength::EYES_SHARP;
    }
    if off & reason::BURST == 0 && s.burst_size >= 2 && s.burst_rank == 0 {
        good |= strength::BURST_BEST;
    }
    if x[TASTE_A] >= 1.0 {
        good |= strength::LIKE_MY_A;
    }
    // Framed (nearly) like several of my A-cuts: the key point within ~5 % of the frame, the
    // subject about as big.
    if off & reason::MY_COMPOSITION == 0 && comp_dist.is_some_and(|d| d <= 0.08) {
        good |= strength::MY_COMPOSITION;
    }
    good
}

/// Trained models: `None` = the library-wide model, `Some(scene)` = a per-scene model.
pub type Models = Vec<(Option<Scene>, Model)>;

/// Train models from labelled items and score everything.
pub fn score(items: &[Item], settings: &LearnSettings) -> (Vec<Score>, Report, Models) {
    let (mut feats, mut scores) = features(items);
    let labels: Vec<Option<bool>> = items.iter().map(|it| settings.label(it.rating, it.flag)).collect();
    taste_features(items, &labels, &scores, &mut feats);
    let comp_dist = composition_features(items, &labels, &scores, &mut feats);
    // Rules the photographer turned off are not measured at all, as far as the models know.
    for x in feats.iter_mut() {
        for (i, r) in FEATURE_REASON.iter().enumerate() {
            if settings.off & r != 0 {
                x[i] = 0.0;
            }
        }
    }
    let prior = Model::prior();
    let (gx, gy): (Vec<Features>, Vec<bool>) =
        feats.iter().zip(&labels).filter_map(|(x, y)| y.map(|y| (*x, y))).unzip();
    let g_b = gy.iter().filter(|y| **y).count();
    let g_a = gy.len() - g_b;
    let mut report = Report { analysed: items.len(), ..Default::default() };
    // Scene models learn on top of the trained global model; both are anchored to the
    // library's base rate only for scoring (see `anchor`).
    let open: Vec<&Features> = feats.iter().zip(&labels).filter(|(_, y)| y.is_none()).map(|(x, _)| x).collect();
    let judged: Vec<(&Features, bool)> = feats.iter().zip(&labels).filter_map(|(x, y)| y.map(|y| (x, y))).collect();
    let global_fit = if g_a >= MIN_GLOBAL && g_b >= MIN_GLOBAL { fit(&gx, &gy, &prior, LAMBDA) } else { prior.clone() };
    let global = anchor(&global_fit, &prior, &open, &judged);
    report.scenes.push(SceneReport {
        scene: None,
        a: g_a,
        b: g_b,
        model: if global_fit == prior { "기본값" } else { "전체 학습" },
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
            let open: Vec<&Features> = feats
                .iter()
                .zip(&labels)
                .zip(items)
                .filter(|((_, y), it)| it.scene == s && y.is_none())
                .map(|((x, _), _)| x)
                .collect();
            let judged: Vec<(&Features, bool)> = feats
                .iter()
                .zip(&labels)
                .zip(items)
                .filter(|(_, it)| it.scene == s)
                .filter_map(|((x, y), _)| y.map(|y| (x, y)))
                .collect();
            (anchor(&fit(&sx, &sy, &global_fit, LAMBDA), &prior, &open, &judged), "장면별 학습")
        } else {
            (global.clone(), if global_fit == prior { "기본값" } else { "전체 학습" })
        };
        report.scenes.push(SceneReport {
            scene: Some(s),
            a,
            b,
            model: name,
            accuracy: cross_validate(&sx, &sy, &global_fit, LAMBDA, 5),
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
        scores[i].strengths = strengths(it, &feats[i], &scores[i], comp_dist[i], settings.off);
    }
    report.candidates_at_60 =
        scores.iter().zip(items).filter(|(s, it)| s.b_prob >= 0.6 && it.flag == 0 && it.rating == 0).count();
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
            embed: Vec::new(),
            gone: false,
            comp: Default::default(),
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
        // Learned which photos are B-cuts: the ISO-12800 shot clears the 60% candidate threshold
        // while the ISO-200 shot stays firmly on the A-cut side.
        assert!(p(500) > 0.7 && p(501) < 0.15, "hi {} lo {}", p(500), p(501));
        assert_eq!(report.scenes[0].model, "전체 학습");
        assert!(report.scenes[0].accuracy.unwrap() > 0.9);
        let land = report.scenes.iter().find(|r| r.scene == Some(Scene::Landscape)).unwrap();
        assert_eq!(land.model, "장면별 학습");
    }

    #[test]
    fn deleted_photos_still_teach_but_leave_bursts() {
        // The same photographer, who deleted every photo they rejected: the decisions are kept
        // (gone items) and the standard is still learned.
        let mut items = Vec::new();
        for i in 0..120i64 {
            let mut it = item(-(i + 1), i * 60_000, 6.0 + (i % 7) as f32 * 0.1);
            it.iso = if i % 2 == 0 { 12800 } else { 200 };
            it.flag = if i % 2 == 0 { -1 } else { 1 };
            it.gone = it.flag == -1;
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
        assert!(p(500) > 0.7 && p(501) < 0.15, "hi {} lo {}", p(500), p(501));
        assert_eq!(report.scenes[0].b, 60);

        // A deleted frame is not counted in its burst.
        let mut items = vec![item(1, 0, 6.5), item(2, 400, 5.0), item(3, 800, 6.8)];
        for it in &mut items {
            it.dhash = 0xFFFF_0000_FFFF_0000;
        }
        items[2].gone = true;
        let (scores, _, _) = score(&items, &LearnSettings::default());
        let s = |id: i64| scores.iter().find(|s| s.id == id).unwrap();
        assert_eq!(s(1).burst_size, 2);
        assert_eq!(s(1).burst_rank, 0);
        assert_eq!(s(3).burst, 0);
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

    #[test]
    fn candidates_survive_rejecting_the_worst() {
        // Portraits shot wide open, sharpness all over the place. The user rejects the 150
        // worst candidates and keeps 60 photos, some of which the prior thought soft. The
        // trained model used to learn "only the blurriest frames are B-cuts" and the candidate
        // list collapsed (1202 → 41 in a 4000-photo run); anchored, the rest stay candidates.
        let mut seed = 12345u64;
        let mut rnd = || {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            ((seed >> 33) as f64) / (1u64 << 31) as f64
        };
        let mut items: Vec<Item> = (0..1500i64)
            .map(|i| {
                let mut it = item(i + 1, i * 5000, (2.0 + rnd() * 5.0) as f32);
                it.scene = Scene::Portrait;
                it.face_area = 0.02;
                it.face_sharp = Some(it.sharp_peak - (rnd() * 1.5) as f32);
                it.eyes = Some((0.5 + rnd() * 0.5) as f32);
                it.iso = 3200;
                it.motion = (0.2 + rnd() * 0.5) as f32;
                it.dhash = rnd().to_bits();
                it
            })
            .collect();
        let settings = LearnSettings::default();
        let candidates = |sc: &[Score], items: &[Item]| {
            sc.iter().zip(items).filter(|(s, it)| s.b_prob >= 0.6 && it.flag == 0 && it.rating == 0).count()
        };
        let (before, _, _) = score(&items, &settings);
        let n_before = candidates(&before, &items);
        let mut order: Vec<usize> = (0..items.len()).collect();
        order.sort_by(|&a, &b| before[b].b_prob.partial_cmp(&before[a].b_prob).unwrap());
        for &i in &order[..150] {
            items[i].flag = -1;
        }
        for &i in order[150..].iter().step_by(15).take(30) {
            items[i].flag = 1;
        }
        for &i in order.iter().rev().step_by(11).take(30) {
            items[i].rating = 4;
        }
        let (after, report, _) = score(&items, &settings);
        assert_eq!(report.scenes[0].model, "전체 학습");
        let n_after = candidates(&after, &items);
        let left = n_before - 150;
        assert!(n_after * 10 >= left * 6 && n_after <= left * 3 / 2, "before {n_before}, left {left}, after {n_after}");
        // Every probability is finite (the scene model used to diverge).
        assert!(after.iter().all(|s| s.b_prob.is_finite()));
    }

    /// A portrait framed at `(kx, ky)` with the head top at `head_top`, looking `facing`.
    fn portrait(id: i64, t: i64, kx: f32, ky: f32, head_top: f32, facing: f32) -> Item {
        use crate::composition::{subject, Composition};
        let mut it = item(id, t, 6.2);
        it.scene = Scene::Portrait;
        it.face_area = 0.03;
        it.face_sharp = Some(6.0);
        it.eyes = Some(0.9);
        it.comp = Composition {
            subject: subject::PERSON,
            people: 1,
            group: 1,
            bx: kx - 0.15,
            by: head_top,
            bw: 0.3,
            bh: 1.0 - head_top,
            area: 0.25,
            kx,
            ky,
            face_h: 0.18,
            face_x: kx,
            facing,
            head_top: Some(head_top),
            ..Default::default()
        };
        it
    }

    #[test]
    fn composition_rules_give_reasons_and_can_be_turned_off() {
        // Well framed portraits, and one with the head cut by the top edge, looking out of the
        // frame on the right, centred.
        let mut items: Vec<Item> = (0..40)
            .map(|i| {
                let mut it = portrait(i + 1, i * 60_000, 1.0 / 3.0, 0.34, 0.1, 0.8);
                it.sharp_peak = 6.0 + (i % 5) as f32 * 0.2;
                it
            })
            .collect();
        let mut odd = portrait(100, 9_000_000, 0.82, 0.12, 0.0, 0.8);
        odd.sharp_peak = 6.4;
        items.push(odd);
        let p = |sc: &[Score], id: i64| *sc.iter().find(|s| s.id == id).unwrap();
        let typical = |sc: &[Score]| sc.iter().filter(|s| s.id < 100).map(|s| s.b_prob).sum::<f32>() / 40.0;
        let (sc, _, _) = score(&items, &LearnSettings::default());
        let bad = p(&sc, 100);
        let good = p(&sc, 3);
        assert!(bad.b_prob > 0.6 && typical(&sc) < 0.2, "bad {bad:?} typical {}", typical(&sc));
        assert!(bad.reasons & reason::HEADROOM != 0 && bad.reasons & reason::LEAD_ROOM != 0, "{bad:?}");
        use crate::composition::strength;
        assert!(good.strengths & strength::THIRDS != 0 && good.strengths & strength::LEAD_ROOM != 0, "{good:?}");
        // This photographer does not care about headroom or lead room.
        let off = LearnSettings { off: reason::HEADROOM | reason::LEAD_ROOM, ..Default::default() };
        let (sc, _, _) = score(&items, &off);
        let bad = p(&sc, 100);
        assert!(bad.reasons & (reason::HEADROOM | reason::LEAD_ROOM) == 0, "{bad:?}");
        assert!(bad.b_prob < 0.3, "{bad:?}");
        assert!(p(&sc, 1).strengths & strength::LEAD_ROOM == 0);
    }

    #[test]
    fn learns_my_compositions() {
        // Two framings this photographer keeps: a tight portrait on the left third and a full
        // figure in the middle. Frames placed elsewhere they reject — the rules alone would not.
        let mut items = Vec::new();
        let mut t = 0;
        let mut add = |items: &mut Vec<Item>, kx: f32, ky: f32, ht: f32, area: f32, flag: i8| {
            t += 120_000;
            let mut it = portrait(items.len() as i64 + 1, t, kx, ky, ht, 0.0);
            it.comp.area = area;
            it.comp.face_h = if area > 0.2 { 0.25 } else { 0.07 };
            it.flag = flag;
            items.push(it);
        };
        for k in 0..30 {
            let j = (k % 5) as f32 * 0.01;
            add(&mut items, 0.33 + j, 0.33 + j, 0.08, 0.35, 1);
            add(&mut items, 0.5 + j, 0.25 + j, 0.12, 0.08, 1);
            // Rejected: the figure small and pushed into a corner (no rule is broken badly).
            add(&mut items, 0.8 + j, 0.62 + j, 0.45, 0.04, -1);
        }
        let first_new = items.len() as i64 + 1;
        add(&mut items, 0.34, 0.34, 0.08, 0.35, 0); // like the first framing
        add(&mut items, 0.51, 0.26, 0.12, 0.08, 0); // like the second
        add(&mut items, 0.81, 0.63, 0.45, 0.04, 0); // like the rejected ones
        let (sc, report, _) = score(&items, &LearnSettings::default());
        assert_eq!(report.scenes[0].model, "전체 학습");
        let p = |id: i64| *sc.iter().find(|s| s.id == id).unwrap();
        let (a, b, odd) = (p(first_new), p(first_new + 1), p(first_new + 2));
        assert!(odd.b_prob > a.b_prob + 0.3 && odd.b_prob > b.b_prob + 0.3, "{a:?} {b:?} {odd:?}");
        assert!(odd.reasons & reason::MY_COMPOSITION != 0, "{odd:?}");
        use crate::composition::strength;
        assert!(a.strengths & strength::MY_COMPOSITION != 0 && b.strengths & strength::MY_COMPOSITION != 0);
        // Turned off, the framing is not learned at all.
        let off = LearnSettings { off: reason::MY_COMPOSITION, ..Default::default() };
        let (sc, _, _) = score(&items, &off);
        let odd = *sc.iter().find(|s| s.id == first_new + 2).unwrap();
        assert!(odd.reasons & reason::MY_COMPOSITION == 0);
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
                // A taste embedding, as every analysed photo has.
                let mut e: Vec<f32> =
                    (0..crate::taste::DIM).map(|d| ((i as usize * 31 + d * 17) % 97) as f32 - 48.0).collect();
                let n = e.iter().map(|v| v * v).sum::<f32>().sqrt();
                e.iter_mut().for_each(|v| *v /= n);
                it.embed = e;
                it
            })
            .collect();
        let t = std::time::Instant::now();
        let (scores, report, _) = score(&items, &LearnSettings::default());
        println!("score 20k: {:?} ({} scores, {} scene reports)", t.elapsed(), scores.len(), report.scenes.len());
    }

    #[test]
    fn imbalanced_b_labels_and_deleted_bursts_do_not_skew_scores() {
        // Burst-heavy library where the photographer deleted many B-cuts (gone = true, losing
        // their burst features) and picked far fewer A-cuts (6:1 B:A). `anchor` used to amplify
        // `actual / expected` up to 5x, pushing >95 % of undecided photos — even the top 4 %
        // best frames — into B-cut territory.
        let mut seed = 42u64;
        let mut rnd = || -> f64 {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            ((seed >> 33) as f64) / (1u64 << 31) as f64
        };
        let mut items = Vec::new();
        let mut id = 1i64;
        let mut t = 0i64;
        for b in 0..400 {
            t += 120_000;
            let base_hash = (b as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
            for f in 0..3 {
                let peak = (4.5 + rnd() * 2.5) as f32 - f as f32 * 0.7;
                let mut it = item(id, t + f * 400, peak);
                it.scene = Scene::Landscape;
                it.dhash = base_hash;
                it.comp.tilt = ((rnd() - 0.5) * 7.0) as f32;
                items.push(it);
                id += 1;
            }
        }
        let (mut nb, mut na) = (0, 0);
        for it in &mut items {
            let p_rej = if it.sharp_peak < 4.5 { 0.65 } else { 0.25 };
            let r = rnd();
            if r < p_rej && nb < 340 {
                it.flag = -1;
                it.gone = true;
                nb += 1;
            } else if it.sharp_peak > 5.0 && r > 0.80 && na < 55 {
                it.flag = 1;
                na += 1;
            }
        }
        let settings = LearnSettings::default();
        let (scores, _, _) = score(&items, &settings);
        let mut open_p: Vec<f32> =
            scores.iter().zip(&items).filter(|(_, it)| it.flag == 0 && it.rating == 0).map(|(s, _)| s.b_prob).collect();
        open_p.sort_by(|a, b| a.total_cmp(b));
        let top4 = open_p[(open_p.len() - 1) * 4 / 100];
        let cand_rate = open_p.iter().filter(|&&p| p >= 0.6).count() as f64 / open_p.len() as f64;
        let ev = crate::eval::evaluate(&items, &settings, 5, 0.6);
        assert!(top4 < 0.25, "top 4% photo must stay on A-cut side, got b_prob={top4:.2}");
        assert!(cand_rate < 0.5, "candidate list must not explode, got {cand_rate:.2}");
        assert!(ev.learned.a_false_alarm < 0.25, "A-cut false alarm too high: {:?}", ev.learned);
    }

    #[test]
    fn star_heavy_a_labels_do_not_collapse_candidates() {
        // Photographer who rates many keepers (A >> B) and only rejects a handful of B-cuts:
        // the candidate list must stay usable instead of collapsing towards 0 %.
        let mut seed = 101u64;
        let mut rnd = || -> f64 {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            ((seed >> 33) as f64) / (1u64 << 31) as f64
        };
        let mut items: Vec<Item> = (0..1200i64)
            .map(|i| {
                let peak = (2.2 + rnd() * 5.0) as f32;
                let mut it = item(i + 1, i * 60_000, peak);
                it.scene = Scene::Portrait;
                it.face_area = 0.025;
                it.face_sharp = Some(peak - (rnd() * 1.4) as f32);
                it.eyes = Some((0.45 + rnd() * 0.55) as f32);
                it.dhash = rnd().to_bits();
                it
            })
            .collect();
        let (mut nb, mut na) = (0, 0);
        for it in &mut items {
            let is_b = it.sharp_peak < 3.5 || it.eyes.unwrap() < 0.58;
            if is_b && nb < 25 {
                it.flag = -1;
                nb += 1;
            } else if !is_b && na < 250 {
                it.rating = 4;
                na += 1;
            }
        }
        let ev = crate::eval::evaluate(&items, &LearnSettings::default(), 5, 0.6);
        assert!(ev.learned.b_recall > 0.5, "B-cut recall collapsed: {:?}", ev.learned);
        assert!(ev.learned.candidate_rate > 0.15, "candidate list collapsed: {:?}", ev.learned);
    }
}
