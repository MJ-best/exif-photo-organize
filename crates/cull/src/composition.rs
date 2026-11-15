//! Composition (구도): where the subject sits in the frame and the framing rules portrait
//! photographers check — rule of thirds, headroom, room in front of the face (lead room), a pole
//! or a line growing out of the head, a tilted horizon, a face cut by the frame edge — and whether
//! the focus landed on the subject or on the background.
//!
//! [`measure`] keeps raw geometry (normalised to the frame, 0..1 from the top left) in
//! [`Composition`], stored with the analysis. [`judge`] turns it into 0..1 "how much this breaks
//! the rule" values for the learner ([`crate::learn`]) and into notes for the photographer
//! ([`notes`]); thresholds live there, so they can be tuned without analysing again.
//!
//! Subject: the person found by segmentation ([`crate::segment::person`]); without one, the part
//! of the frame that is clearly sharper than the rest (a shallow depth of field isolates the
//! subject). Deep-focus scenes without a person have no subject: only the horizon is judged.

use crate::metrics::{region_stats, Gray};
use crate::segment::Mask;
use crate::vision::DetectedFace;
use serde::{Deserialize, Serialize};

/// What [`Composition::subject`] is.
pub mod subject {
    pub const NONE: u8 = 0;
    /// One or more people (segmentation, or the face when segmentation was not available).
    pub const PERSON: u8 = 1;
    /// The sharp region of a photo with a shallow depth of field.
    pub const FOCUS: u8 = 2;
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Composition {
    /// See [`subject`].
    pub subject: u8,
    /// People found (separate figures).
    pub people: u8,
    /// Faces framed together (about as big as the main one): more than one is a group.
    pub group: u8,
    /// Subject box (normalised).
    pub bx: f32,
    pub by: f32,
    pub bw: f32,
    pub bh: f32,
    /// Share of the frame covered by the subject.
    pub area: f32,
    /// The point the composition hangs on: between the eyes of the main face, else the
    /// subject's centre of mass.
    pub kx: f32,
    pub ky: f32,
    /// Frame width / height.
    pub aspect: f32,
    /// Main (largest) face: height and horizontal centre (0 = no face).
    pub face_h: f32,
    pub face_x: f32,
    /// Share of the main face's box outside the frame, and of it above the top edge.
    pub face_cut: f32,
    pub face_cut_top: f32,
    /// Where the main face looks: -1 towards the left edge … 1 towards the right, 0 at the camera.
    pub facing: f32,
    /// Top of the main person's head (normalised y; 0 = touches the top edge).
    pub head_top: Option<f32>,
    /// A vertical background line right above the head (a pole, a tree, a door frame): how
    /// continuous it is 0..1 and where.
    pub pole: f32,
    pub pole_x: f32,
    /// A horizontal background line through the head or neck (a horizon, a railing) 0..1, where.
    pub neck_line: f32,
    pub neck_y: f32,
    /// Strongest long straight line close to level (or plumb): its angle in degrees (positive =
    /// the right end is lower / the top leans right), how much of the frame it spans 0..1, and its
    /// position (y at the centre for a level line, x for a plumb one).
    pub tilt: f32,
    pub tilt_conf: f32,
    pub tilt_vertical: bool,
    pub tilt_at: f32,
    /// Edge crispness (see [`split_sharpness`]) of the crispest parts of the person and of the
    /// background behind them: which one the focus is on.
    pub subject_sharp: Option<f32>,
    pub bg_sharp: Option<f32>,
    /// Detail (ln(1 + Laplacian variance)) of the person and of the background: a background
    /// much plainer than the person is out of focus (a shallow depth of field).
    pub subject_detail: Option<f32>,
    pub bg_detail: Option<f32>,
}

impl Default for Composition {
    fn default() -> Self {
        Composition {
            subject: subject::NONE,
            people: 0,
            group: 0,
            bx: 0.0,
            by: 0.0,
            bw: 0.0,
            bh: 0.0,
            area: 0.0,
            kx: 0.5,
            ky: 0.5,
            aspect: 1.5,
            face_h: 0.0,
            face_x: 0.5,
            face_cut: 0.0,
            face_cut_top: 0.0,
            facing: 0.0,
            head_top: None,
            pole: 0.0,
            pole_x: 0.5,
            neck_line: 0.0,
            neck_y: 0.5,
            tilt: 0.0,
            tilt_conf: 0.0,
            tilt_vertical: false,
            tilt_at: 0.5,
            subject_sharp: None,
            bg_sharp: None,
            subject_detail: None,
            bg_detail: None,
        }
    }
}

// -------------------------------------------------------------------------------------------
// Measuring

/// Connected regions (4-neighbour) of `mask >= 0.5`: a label per cell (0 = none) and each
/// region's cell count (index = label - 1).
fn components(m: &Mask) -> (Vec<u32>, Vec<usize>) {
    let mut label = vec![0u32; m.p.len()];
    let mut sizes = Vec::new();
    let mut stack = Vec::new();
    for start in 0..m.p.len() {
        if m.p[start] < 0.5 || label[start] != 0 {
            continue;
        }
        let id = sizes.len() as u32 + 1;
        let mut n = 0;
        label[start] = id;
        stack.push(start);
        while let Some(i) = stack.pop() {
            n += 1;
            let (x, y) = (i % m.w, i / m.w);
            let mut visit = |j: usize| {
                if m.p[j] >= 0.5 && label[j] == 0 {
                    label[j] = id;
                    stack.push(j);
                }
            };
            if x > 0 {
                visit(i - 1);
            }
            if x + 1 < m.w {
                visit(i + 1);
            }
            if y > 0 {
                visit(i - m.w);
            }
            if y + 1 < m.h {
                visit(i + m.w);
            }
        }
        sizes.push(n);
    }
    (label, sizes)
}

/// A grayscale copy `f` times smaller (box filter), for line finding.
fn shrink(g: &Gray, f: usize) -> Gray {
    let (w, h) = ((g.w / f).max(1), (g.h / f).max(1));
    let mut px = vec![0f32; w * h];
    for y in 0..h {
        for x in 0..w {
            let mut s = 0.0;
            for dy in 0..f {
                let row = (y * f + dy).min(g.h - 1) * g.w;
                for dx in 0..f {
                    s += g.px[row + (x * f + dx).min(g.w - 1)];
                }
            }
            px[y * w + x] = s / (f * f) as f32;
        }
    }
    Gray { w, h, px }
}

/// Central-difference gradients (zero at the border).
fn gradients(g: &Gray) -> (Vec<f32>, Vec<f32>) {
    let (mut gx, mut gy) = (vec![0f32; g.w * g.h], vec![0f32; g.w * g.h]);
    for y in 1..g.h.saturating_sub(1) {
        for x in 1..g.w.saturating_sub(1) {
            let i = y * g.w + x;
            gx[i] = (g.px[i + 1] - g.px[i - 1]) * 0.5;
            gy[i] = (g.px[i + g.w] - g.px[i - g.w]) * 0.5;
        }
    }
    (gx, gy)
}

/// Edge threshold: a clear fraction of the photo's own strong gradients.
fn edge_threshold(gx: &[f32], gy: &[f32]) -> f32 {
    let mut mags: Vec<f32> = gx.iter().zip(gy).map(|(a, b)| (a * a + b * b).sqrt()).filter(|m| *m > 0.5).collect();
    if mags.is_empty() {
        return f32::MAX;
    }
    let k = (mags.len() * 9 / 10).min(mags.len() - 1);
    let p90 = *mags.select_nth_unstable_by(k, |a, b| a.total_cmp(b)).1;
    (p90 * 0.45).max(3.0)
}

/// Strongest long straight line within ±`MAX_TILT`° of level (or of plumb), by a Hough vote of
/// edge pixels whose gradient is perpendicular to the line. People are left out (they lean).
/// Returns (angle °, share of the frame it spans, position 0..1).
fn straight_line(
    g: &Gray,
    gx: &[f32],
    gy: &[f32],
    t: f32,
    vertical: bool,
    keep: impl Fn(usize, usize) -> bool,
) -> (f32, f32, f32) {
    const MAX_TILT: f32 = 10.0;
    const STEP: f32 = 0.25;
    let n_ang = (2.0 * MAX_TILT / STEP) as usize + 1;
    // Along the line (u) and across it (v).
    let (len_u, len_v) = if vertical { (g.h, g.w) } else { (g.w, g.h) };
    let mut pts: Vec<(f32, f32)> = Vec::new();
    for y in 1..g.h.saturating_sub(1) {
        for x in 1..g.w.saturating_sub(1) {
            let i = y * g.w + x;
            let (across, along) = if vertical { (gx[i].abs(), gy[i].abs()) } else { (gy[i].abs(), gx[i].abs()) };
            if across >= t && across >= 3.0 * along && keep(x, y) {
                pts.push(if vertical { (y as f32, x as f32) } else { (x as f32, y as f32) });
            }
        }
    }
    if pts.len() < len_u / 4 {
        return (0.0, 0.0, 0.5);
    }
    let bins = len_v + 2 * (len_u as f32 * (MAX_TILT.to_radians()).tan()) as usize + 4;
    let off = (bins - len_v) as f32 / 2.0;
    // (angle, bin, votes)
    let mut best = (0.0f32, 0usize, 0u32);
    let mut acc = vec![0u32; bins];
    for a in 0..n_ang {
        let ang = (-MAX_TILT + a as f32 * STEP).to_radians();
        let (s, c) = ang.sin_cos();
        acc.iter_mut().for_each(|v| *v = 0);
        for &(u, v) in &pts {
            // Distance across the line through (u, v) at this angle.
            let r = v * c - u * s + off;
            let b = r as usize;
            if b < bins {
                acc[b] += 1;
            }
        }
        // Lines are a pixel or two thick.
        for b in 1..bins - 1 {
            let votes = acc[b - 1] + acc[b] + acc[b + 1];
            if votes > best.2 {
                best = (ang, b, votes);
            }
        }
    }
    // How much of the frame the winning line spans: positions along it that have an edge.
    let (ang, b, _) = best;
    let (s, c) = ang.sin_cos();
    let mut covered = vec![false; len_u];
    for &(u, v) in &pts {
        if ((v * c - u * s + off) - b as f32 - 0.5).abs() <= 1.5 {
            covered[(u as usize).min(len_u - 1)] = true;
        }
    }
    let support = covered.iter().filter(|c| **c).count() as f32 / len_u as f32;
    // Position of the line at the middle of the frame.
    let mid = (b as f32 + 0.5 - off + (len_u as f32 / 2.0) * s) / c / len_v as f32;
    (ang.to_degrees(), support, mid.clamp(0.0, 1.0))
}

/// Sharpness of the person and of the background, per tile, from the pixels well inside the
/// person and those well outside (`class`: 1, 0, or -1 near the outline, on a grid over the frame).
///
/// Two measures. *Crispness*: on edge pixels (a clear gradient), the RMS Laplacian over the mean
/// gradient. Blur spreads an edge over more pixels, so the ratio falls with blur whatever the
/// contrast and however busy the content — it tells which one the focus is on. *Detail*: the
/// Laplacian variance of all pixels, which also counts how much texture there is — a
/// background with much less detail than the person is out of focus or plain.
///
/// Returns (person crispness, background crispness, person detail, background detail) for the
/// tiles that have enough pixels, as natural logarithms.
fn split_sharpness(g: &Gray, n: usize, class: &[i8], cw: usize, ch: usize) -> [Vec<f32>; 4] {
    const EDGE: f32 = 8.0;
    // Per tile and side: Σ lap², Σ |grad| and count over edge pixels; Σ lap, Σ lap², count.
    let mut acc = vec![[0f64; 12]; n * n];
    for y in 1..g.h - 1 {
        let cy = (y * ch / g.h).min(ch - 1);
        let ty = (y * n / g.h).min(n - 1);
        for x in 1..g.w - 1 {
            let k = class[cy * cw + (x * cw / g.w).min(cw - 1)];
            if k < 0 {
                continue;
            }
            let i = y * g.w + x;
            let (l, r, u, d) = (g.px[i - 1], g.px[i + 1], g.px[i - g.w], g.px[i + g.w]);
            let lap = (l + r + u + d - 4.0 * g.px[i]) as f64;
            let a = &mut acc[ty * n + (x * n / g.w).min(n - 1)];
            let o = if k == 1 { 0 } else { 6 };
            let mag = (((r - l) * 0.5).powi(2) + ((d - u) * 0.5).powi(2)).sqrt();
            if mag >= EDGE {
                a[o] += lap * lap;
                a[o + 1] += mag as f64;
                a[o + 2] += 1.0;
            }
            a[o + 3] += lap;
            a[o + 4] += lap * lap;
            a[o + 5] += 1.0;
        }
    }
    let tile_px = (g.w * g.h) as f64 / (n * n) as f64;
    let mut out: [Vec<f32>; 4] = Default::default();
    for a in &acc {
        for (side, o) in [(0usize, 0usize), (1, 6)] {
            if a[o + 2] >= 40.0 {
                let rms = (a[o] / a[o + 2]).sqrt();
                out[side].push((rms / (a[o + 1] / a[o + 2]).max(1e-6)).max(1e-6).ln() as f32);
            }
            if a[o + 5] >= tile_px * 0.4 {
                let m = a[o + 3] / a[o + 5];
                out[2 + side].push((1.0 + (a[o + 4] / a[o + 5] - m * m).max(0.0)).ln() as f32);
            }
        }
    }
    out
}

/// Sharpness map: ln(1 + Laplacian variance) on a `n`×`n` grid of tiles.
fn tile_sharpness(g: &Gray, n: usize) -> Vec<f32> {
    let mut v = Vec::with_capacity(n * n);
    for ty in 0..n {
        for tx in 0..n {
            let st = region_stats(g, tx * g.w / n, ty * g.h / n, (tx + 1) * g.w / n, (ty + 1) * g.h / n);
            v.push((1.0 + st.lap_var).ln());
        }
    }
    v
}

/// Measure the composition of an analysed image. `g` is the level-normalised grayscale image the
/// faces were found on (pixel coordinates); `person` the segmentation, if it ran.
pub fn measure(g: &Gray, faces: &[DetectedFace], person: Option<&Mask>) -> Composition {
    let (w, h) = (g.w as f32, g.h as f32);
    let mut c = Composition { aspect: w / h.max(1.0), ..Default::default() };
    let main = faces.iter().filter(|f| f.w >= 8.0 && f.h >= 8.0).max_by(|a, b| (a.w * a.h).total_cmp(&(b.w * b.h)));

    // The faces that make the picture: the main one and those about as big (a group).
    let group: Vec<&DetectedFace> = match main {
        Some(m) => faces.iter().filter(|f| f.w * f.h >= 0.4 * m.w * m.h).collect(),
        None => Vec::new(),
    };
    let eyes = |f: &DetectedFace| {
        let (e1, e2) = (f.landmarks[0], f.landmarks[1]);
        if e1.0 <= e2.0 {
            (e1, e2)
        } else {
            (e2, e1)
        }
    };
    let cut = |f: &DetectedFace| {
        let ix = ((f.x + f.w).min(w) - f.x.max(0.0)).max(0.0);
        let iy = ((f.y + f.h).min(h) - f.y.max(0.0)).max(0.0);
        let all = (1.0 - ix * iy / (f.w * f.h).max(1.0)).clamp(0.0, 1.0);
        (all, ((-f.y).max(0.0) / f.h.max(1.0)).clamp(0.0, all))
    };

    // The main face: size, place, how much is outside the frame, where it looks. A group is
    // framed as a whole: its eye line and middle, the worst cut; nobody's gaze.
    if let Some(f) = main {
        c.group = group.len().min(255) as u8;
        c.face_h = f.h / h;
        c.face_x = ((f.x + f.w / 2.0) / w).clamp(0.0, 1.0);
        (c.face_cut, c.face_cut_top) =
            group.iter().map(|f| cut(f)).fold((0.0, 0.0), |a, b| if b.0 > a.0 { b } else { a });
        let (l, r) = eyes(f);
        let (ex, ey) = (r.0 - l.0, r.1 - l.1);
        let d = (ex * ex + ey * ey).sqrt().max(1.0);
        let (mx, my) = ((l.0 + r.0) / 2.0, (l.1 + r.1) / 2.0);
        if group.len() == 1 {
            // The nose moves towards where the face turns; measured along the eye line (head roll).
            let nose = f.landmarks[2];
            let yaw = ((nose.0 - mx) * ex + (nose.1 - my) * ey) / (d * d);
            c.facing = (yaw / 0.35).clamp(-1.0, 1.0);
            c.kx = (mx / w).clamp(0.0, 1.0);
            c.ky = (my / h).clamp(0.0, 1.0);
        } else {
            let n = group.len() as f32;
            let (sx, sy) = group
                .iter()
                .map(|f| eyes(f))
                .fold((0.0, 0.0), |a, (l, r)| (a.0 + (l.0 + r.0) / 2.0, a.1 + (l.1 + r.1) / 2.0));
            c.kx = (sx / n / w).clamp(0.0, 1.0);
            c.ky = (sy / n / h).clamp(0.0, 1.0);
            let (x0, x1) = group.iter().fold((f32::MAX, f32::MIN), |a, f| (a.0.min(f.x), a.1.max(f.x + f.w)));
            c.face_x = (((x0 + x1) / 2.0) / w).clamp(0.0, 1.0);
        }
    }

    // The subject: people from the segmentation.
    let mut person_at: Option<Box<dyn Fn(f32, f32) -> f32 + '_>> = None;
    let mut person_mask: Option<&Mask> = None;
    if let Some(m) = person.filter(|m| !m.p.is_empty()) {
        let (label, sizes) = components(m);
        let biggest = sizes.iter().copied().max().unwrap_or(0);
        let cell = |x: f32, y: f32| -> usize {
            ((y * m.h as f32) as usize).min(m.h - 1) * m.w + ((x * m.w as f32) as usize).min(m.w - 1)
        };
        // The main face's figure: the region under its eyes (or most of its box).
        let main_label = main.and_then(|f| {
            let mut votes = std::collections::HashMap::<u32, usize>::new();
            for j in 0..5 {
                for i in 0..5 {
                    let x = (f.x + f.w * (i as f32 + 0.5) / 5.0) / w;
                    let y = (f.y + f.h * (j as f32 + 0.5) / 5.0) / h;
                    if (0.0..1.0).contains(&x) && (0.0..1.0).contains(&y) {
                        let l = label[cell(x, y)];
                        if l > 0 {
                            *votes.entry(l).or_default() += 1;
                        }
                    }
                }
            }
            votes.into_iter().max_by_key(|v| v.1).map(|v| v.0)
        });
        let significant: Vec<bool> = sizes
            .iter()
            .enumerate()
            .map(|(k, &n)| n * 4 >= biggest && n * 300 >= m.p.len() || main_label == Some(k as u32 + 1))
            .collect();
        let total: usize = sizes.iter().zip(&significant).filter(|(_, s)| **s).map(|(n, _)| n).sum();
        // Without a face, a small figure-like patch is more often something else.
        let min_share = if main.is_some() { 100 } else { 25 };
        if total * min_share >= m.p.len() {
            let (mut x0, mut y0, mut x1, mut y1) = (m.w, m.h, 0, 0);
            let (mut sx, mut sy) = (0usize, 0usize);
            for (i, &l) in label.iter().enumerate() {
                if l > 0 && significant[l as usize - 1] {
                    let (x, y) = (i % m.w, i / m.w);
                    x0 = x0.min(x);
                    y0 = y0.min(y);
                    x1 = x1.max(x);
                    y1 = y1.max(y);
                    sx += x;
                    sy += y;
                }
            }
            c.subject = subject::PERSON;
            c.people = significant.iter().filter(|s| **s).count().min(255) as u8;
            c.bx = x0 as f32 / m.w as f32;
            c.by = y0 as f32 / m.h as f32;
            c.bw = (x1 + 1 - x0) as f32 / m.w as f32;
            c.bh = (y1 + 1 - y0) as f32 / m.h as f32;
            c.area = total as f32 / m.p.len() as f32;
            if main.is_none() {
                c.kx = (sx as f32 / total as f32 + 0.5) / m.w as f32;
                c.ky = (sy as f32 / total as f32 + 0.5) / m.h as f32;
            }
            // Top of the head: the highest cell of the figure above each face of the group (the
            // tallest one counts).
            for f in &group {
                let (fx0, fx1) = (((f.x - 0.1 * f.w) / w).max(0.0), ((f.x + 1.1 * f.w) / w).min(1.0));
                let (cx0, cx1) = ((fx0 * m.w as f32) as usize, ((fx1 * m.w as f32).ceil() as usize).min(m.w));
                let (face_top, fh) = (f.y / h, f.h / h);
                let (fy0, fy1) =
                    ((face_top.max(0.0) * m.h as f32) as usize, (((f.y + f.h) / h).min(1.0) * m.h as f32) as usize);
                // The figure under this face.
                let mine: Vec<u32> = (fy0..fy1.min(m.h))
                    .flat_map(|y| (cx0..cx1).map(move |x| (x, y)))
                    .map(|(x, y)| label[y * m.w + x])
                    .filter(|l| *l > 0)
                    .collect();
                let Some(&fl) = mine.iter().max_by_key(|l| mine.iter().filter(|m| m == l).count()) else { continue };
                if let Some(top) = (0..m.h).find(|&y| (cx0..cx1).any(|x| label[y * m.w + x] == fl)) {
                    // A raised arm or a tall prop is not the head: at most a face height above it.
                    let t = (top as f32 / m.h as f32).max(face_top - fh).min(face_top).max(0.0);
                    c.head_top = Some(c.head_top.map_or(t, |h: f32| h.min(t)));
                }
            }
        }
        person_at = Some(Box::new(move |x: f32, y: f32| m.at(x, y)));
        person_mask = Some(m);
    }
    if let Some(f) = main {
        if c.subject == subject::NONE {
            // No segmentation: the face stands for the person.
            c.subject = subject::PERSON;
            c.people = faces.len().min(255) as u8;
            c.bx = (f.x / w).clamp(0.0, 1.0);
            c.by = (f.y / h).clamp(0.0, 1.0);
            c.bw = (f.w / w).min(1.0);
            c.bh = (f.h / h).min(1.0);
            c.area = c.bw * c.bh;
        }
        if c.head_top.is_none() {
            // Hair above the detector's boxes.
            let top = group.iter().map(|f| f.y - 0.3 * f.h).fold(f.y - 0.3 * f.h, f32::min);
            c.head_top = Some((top / h).max(0.0));
        }
    }

    // Sharpness: of the person against the background, or the sharp region as the subject.
    const TN: usize = 16;
    if c.subject == subject::PERSON {
        if let Some(m) = person_mask {
            // Well inside or well outside: the mask is coarse at the outline.
            let class: Vec<i8> = (0..m.p.len())
                .map(|i| {
                    let (x, y) = ((i % m.w) as i64, (i / m.w) as i64);
                    let v = |dx: i64, dy: i64| {
                        let (xx, yy) = ((x + dx).clamp(0, m.w as i64 - 1), (y + dy).clamp(0, m.h as i64 - 1));
                        m.p[yy as usize * m.w + xx as usize]
                    };
                    let around = [v(0, 0), v(-2, 0), v(2, 0), v(0, -2), v(0, 2)];
                    if around.iter().all(|p| *p >= 0.5) {
                        1
                    } else if around.iter().all(|p| *p < 0.2) {
                        0
                    } else {
                        -1
                    }
                })
                .collect();
            let [crisp_s, crisp_b, detail_s, detail_b] = split_sharpness(g, TN, &class, m.w, m.h);
            let top2 = |v: &[f32], min: usize| -> Option<f32> {
                if v.len() < min {
                    return None;
                }
                let mut v = v.to_vec();
                v.sort_by(|a, b| b.total_cmp(a));
                Some(v.iter().take(2).sum::<f32>() / 2.0)
            };
            if let (Some(s), Some(b)) = (top2(&crisp_s, 2), top2(&crisp_b, 3)) {
                c.subject_sharp = Some(s);
                c.bg_sharp = Some(b);
            }
            if let (Some(s), Some(b)) = (top2(&detail_s, 2), top2(&detail_b, 4)) {
                c.subject_detail = Some(s);
                c.bg_detail = Some(b);
            }
        }
    } else {
        let tiles = tile_sharpness(g, TN);
        let peak = tiles.iter().copied().fold(f32::MIN, f32::max);
        let mut sorted = tiles.clone();
        sorted.sort_by(|a, b| a.total_cmp(b));
        let median = sorted[sorted.len() / 2];
        // Only a shallow depth of field singles a subject out.
        if peak - median >= 1.2 {
            let (mut x0, mut y0, mut x1, mut y1) = (TN, TN, 0, 0);
            let (mut sx, mut sy, mut sw, mut n) = (0f32, 0f32, 0f32, 0usize);
            for ty in 0..TN {
                for tx in 0..TN {
                    let v = tiles[ty * TN + tx];
                    if v >= peak - 0.8 {
                        let wgt = v - (peak - 0.8) + 0.1;
                        x0 = x0.min(tx);
                        y0 = y0.min(ty);
                        x1 = x1.max(tx);
                        y1 = y1.max(ty);
                        sx += (tx as f32 + 0.5) * wgt;
                        sy += (ty as f32 + 0.5) * wgt;
                        sw += wgt;
                        n += 1;
                    }
                }
            }
            // A sharp region scattered over the whole frame is texture, not a subject.
            let box_area = ((x1 + 1 - x0) * (y1 + 1 - y0)) as f32 / (TN * TN) as f32;
            let (bw, bh) = ((x1 + 1 - x0) as f32 / TN as f32, (y1 + 1 - y0) as f32 / TN as f32);
            if n > 0 && box_area <= 0.5 && bw < 0.9 && bh < 0.9 {
                c.subject = subject::FOCUS;
                c.bx = x0 as f32 / TN as f32;
                c.by = y0 as f32 / TN as f32;
                c.bw = (x1 + 1 - x0) as f32 / TN as f32;
                c.bh = (y1 + 1 - y0) as f32 / TN as f32;
                c.area = n as f32 / (TN * TN) as f32;
                c.kx = sx / sw / TN as f32;
                c.ky = sy / sw / TN as f32;
            }
        }
    }

    // Lines: on a ~400 px copy, where soft (out of focus) edges still show.
    let scale = (g.w.max(g.h) / 400).max(1);
    let small = shrink(g, scale);
    let (gx, gy) = gradients(&small);
    let t = edge_threshold(&gx, &gy);
    let (sw_, sh_) = (small.w as f32, small.h as f32);
    let is_person = |x: usize, y: usize| -> bool {
        person_at.as_ref().is_some_and(|at| {
            let (nx, ny) = ((x as f32 + 0.5) / sw_, (y as f32 + 0.5) / sh_);
            // A little margin: the outline of the person is not a background line.
            let d = 3.0 / 192.0;
            at(nx, ny) >= 0.3
                || at(nx - d, ny) >= 0.3
                || at(nx + d, ny) >= 0.3
                || at(nx, ny - d) >= 0.3
                || at(nx, ny + d) >= 0.3
        })
    };
    let bg = |x: usize, y: usize| !is_person(x, y);

    // Tilt: plumb lines near the middle tell how the camera was held (at the edges a building
    // leans because of perspective); without them, a long line close to level — a horizon. Other
    // level-looking lines (a bus, a table) slope in perspective, so they need to be long and
    // there must be no plumb line to go by.
    let (ah, sh, ph) = straight_line(&small, &gx, &gy, t, false, bg);
    let (av, sv, pv) =
        straight_line(&small, &gx, &gy, t, true, |x, y| bg(x, y) && (x as f32) > sw_ * 0.25 && (x as f32) < sw_ * 0.75);
    if sv < 0.35 && (sh >= 0.45 || sh >= sv * 1.5) {
        (c.tilt, c.tilt_conf, c.tilt_vertical, c.tilt_at) = (ah, sh, false, ph);
    } else {
        // Plumb lines span the height; a line leaning right at the top has a negative angle in
        // (y, x) coordinates — report it the same way as a level line (positive = clockwise).
        (c.tilt, c.tilt_conf, c.tilt_vertical, c.tilt_at) = (-av, sv, true, pv);
    }

    // Lines merging with the head.
    if let (Some(f), Some(ht)) = (main, c.head_top) {
        let s = 1.0 / scale as f32;
        let (fx, fy, fw, fh) = (f.x * s, f.y * s, f.w * s, f.h * s);
        let cx = fx + fw / 2.0;
        let top_px = ht * sh_;
        // A pole: right above the head, as wide as the face.
        let (y0, y1) = ((top_px - 1.5 * fh).max(0.0) as usize, (top_px - 0.05 * fh).max(0.0) as usize);
        let (x0, x1) = ((cx - 0.45 * fw).max(1.0) as usize, ((cx + 0.45 * fw) as usize).min(small.w - 2));
        let rows = y1.saturating_sub(y0);
        if rows as f32 >= 0.3 * fh && x1 > x0 {
            let edge = |x: usize, y: usize| {
                let i = y * small.w + x;
                gx[i].abs() >= t && gx[i].abs() >= 2.0 * gy[i].abs() && bg(x, y)
            };
            let (mut best, mut best_x) = (0f32, cx);
            for x in x0..=x1 {
                let n = (y0..y1).filter(|&y| edge(x - 1, y) || edge(x, y) || edge(x + 1, y)).count();
                let support = n as f32 / rows as f32;
                if support > best {
                    (best, best_x) = (support, x as f32);
                }
            }
            c.pole = best;
            c.pole_x = (best_x / sw_).clamp(0.0, 1.0);
        }
        // A line through the head or neck, beside the person on both sides where there is room.
        let (r0, r1) = ((fy + 0.3 * fh).max(1.0) as usize, ((fy + 1.6 * fh) as usize).min(small.h - 2));
        let (mut best, mut best_y) = (0f32, 0.0f32);
        for y in r0..=r1.max(r0) {
            if y + 1 >= small.h {
                break;
            }
            let (mut hit, mut room) = (0usize, 0usize);
            for x in 1..small.w - 1 {
                if !bg(x, y) {
                    continue;
                }
                room += 1;
                let e = |yy: usize| {
                    let i = yy * small.w + x;
                    gy[i].abs() >= t && gy[i].abs() >= 2.0 * gx[i].abs()
                };
                if e(y - 1) || e(y) || e(y + 1) {
                    hit += 1;
                }
            }
            if room as f32 >= 0.25 * sw_ {
                let v = hit as f32 / room as f32;
                if v > best {
                    (best, best_y) = (v, y as f32);
                }
            }
        }
        c.neck_line = best;
        c.neck_y = (best_y / sh_).clamp(0.0, 1.0);
    }
    c
}

// -------------------------------------------------------------------------------------------
// Judging

/// How much a photo breaks each rule, 0 (fine) ..= 1 (clearly), and what it does well.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Judgement {
    pub thirds: f32,
    pub headroom: f32,
    pub lead_room: f32,
    pub merger: f32,
    pub tilt: f32,
    pub cut: f32,
    pub focus_bg: f32,
    /// [`strength`] bits.
    pub good: u32,
}

/// What an A-cut candidate does well (shown as its reasons).
pub mod strength {
    pub const EYES_SHARP: u32 = 1;
    pub const THIRDS: u32 = 2;
    pub const HEADROOM: u32 = 4;
    pub const LEAD_ROOM: u32 = 8;
    pub const LEVEL: u32 = 16;
    pub const ISOLATED: u32 = 32;
    pub const BURST_BEST: u32 = 64;
    pub const LIKE_MY_A: u32 = 128;
    pub const MY_COMPOSITION: u32 = 256;

    pub const ALL: [(u32, &str); 9] = [
        (EYES_SHARP, "눈에 초점"),
        (THIRDS, "3분할 구도"),
        (HEADROOM, "헤드룸 알맞음"),
        (LEAD_ROOM, "시선 앞 여백"),
        (LEVEL, "수평이 맞음"),
        (ISOLATED, "배경이 깔끔하게 흐림"),
        (BURST_BEST, "연사 중 베스트"),
        (LIKE_MY_A, "내가 고른 사진과 비슷"),
        (MY_COMPOSITION, "내가 좋아하는 구도"),
    ];

    pub fn labels(mask: u32) -> Vec<&'static str> {
        ALL.iter().filter(|(b, _)| mask & b != 0).map(|(_, l)| *l).collect()
    }
}

fn ramp(v: f32, from: f32, to: f32) -> f32 {
    ((v - from) / (to - from)).clamp(0.0, 1.0)
}

/// A face big enough for portrait framing rules (not a passer-by in a landscape).
const PORTRAIT_FACE: f32 = 0.06;
/// A close-up: cropping into the top of the head is a choice, not a mistake.
const CLOSE_UP_FACE: f32 = 0.3;

impl Composition {
    pub fn is_portrait(&self) -> bool {
        self.subject == subject::PERSON && self.face_h >= PORTRAIT_FACE
    }

    /// Distance of the key point from the thirds: horizontally to the nearer vertical third,
    /// vertically to the upper third for eyes (to the nearer horizontal third otherwise).
    pub fn thirds_offset(&self) -> (f32, f32) {
        let dx = (self.kx - 1.0 / 3.0).abs().min((self.kx - 2.0 / 3.0).abs());
        let dy = if self.face_h > 0.0 {
            (self.ky - 1.0 / 3.0).abs()
        } else {
            (self.ky - 1.0 / 3.0).abs().min((self.ky - 2.0 / 3.0).abs())
        };
        (dx, dy)
    }

    /// Room in front of the face (towards where it looks), share of the frame width; None when
    /// the face looks at the camera.
    pub fn front_room(&self) -> Option<f32> {
        (self.face_h > 0.0 && self.facing.abs() >= 0.3).then_some(if self.facing > 0.0 {
            1.0 - self.kx
        } else {
            self.kx
        })
    }

    /// The same measurements on the photo turned by `angle` degrees (clockwise) and cropped:
    /// `map` takes a point of the photo to the framed photo (both normalised), `sw`, `sh` are the
    /// crop's size relative to the photo. Sharpness and the background lines' strength stay as
    /// measured; how much of the face the crop cuts off is added to what the frame already cut.
    pub fn reframed(&self, angle: f32, map: impl Fn(f32, f32) -> (f32, f32), sw: f32, sh: f32) -> Composition {
        let mut c = self.clone();
        let (sw, sh) = (sw.max(1e-3), sh.max(1e-3));
        c.aspect = self.aspect * sw / sh;
        (c.kx, c.ky) = map(self.kx, self.ky);
        if self.subject != subject::NONE {
            let corners = [
                map(self.bx, self.by),
                map(self.bx + self.bw, self.by),
                map(self.bx, self.by + self.bh),
                map(self.bx + self.bw, self.by + self.bh),
            ];
            let (x0, x1) = corners.iter().fold((f32::MAX, f32::MIN), |a, p| (a.0.min(p.0), a.1.max(p.0)));
            let (y0, y1) = corners.iter().fold((f32::MAX, f32::MIN), |a, p| (a.0.min(p.1), a.1.max(p.1)));
            let (cx0, cy0, cx1, cy1) = (x0.clamp(0.0, 1.0), y0.clamp(0.0, 1.0), x1.clamp(0.0, 1.0), y1.clamp(0.0, 1.0));
            (c.bx, c.by, c.bw, c.bh) = (cx0, cy0, (cx1 - cx0).max(0.0), (cy1 - cy0).max(0.0));
            c.area = (self.area / (sw * sh)).min(1.0);
        }
        if self.face_h > 0.0 {
            c.face_h = self.face_h / sh;
            c.face_x = map(self.face_x, self.ky).0;
            // The face's box, roughly (eyes 40 % down a box about 0.8 as wide as tall).
            let fw = self.face_h * 0.8 / self.aspect;
            let (fx0, fx1, fy0, fy1) =
                (self.face_x - fw / 2.0, self.face_x + fw / 2.0, self.ky - 0.4 * self.face_h, self.ky + 0.6 * self.face_h);
            let pts = [map(fx0, fy0), map(fx1, fy0), map(fx0, fy1), map(fx1, fy1)];
            let (x0, x1) = pts.iter().fold((f32::MAX, f32::MIN), |a, p| (a.0.min(p.0), a.1.max(p.0)));
            let (y0, y1) = pts.iter().fold((f32::MAX, f32::MIN), |a, p| (a.0.min(p.1), a.1.max(p.1)));
            let area = ((x1 - x0) * (y1 - y0)).max(1e-6);
            let ix = (x1.min(1.0) - x0.max(0.0)).max(0.0);
            let iy = (y1.min(1.0) - y0.max(0.0)).max(0.0);
            let lost = 1.0 - ix * iy / area;
            let lost_top = ((-y0).max(0.0) / (y1 - y0).max(1e-6)).min(lost);
            c.face_cut = (self.face_cut + lost).min(1.0);
            c.face_cut_top = (self.face_cut_top + lost_top).min(c.face_cut);
        }
        if let Some(ht) = self.head_top {
            c.head_top = Some(map(self.face_x, ht).1.max(0.0));
        }
        c.pole_x = map(self.pole_x, self.head_top.unwrap_or(self.ky)).0;
        c.neck_y = map(self.face_x, self.neck_y).1;
        // A line turned with the photo; its place moves with the crop.
        c.tilt = self.tilt + angle;
        c.tilt_at = if self.tilt_vertical { map(self.tilt_at, 0.5).0 } else { map(0.5, self.tilt_at).1 };
        c
    }

    /// The level line is the horizon of a scene without a subject, when it is clear.
    fn horizon(&self) -> Option<f32> {
        (!self.tilt_vertical && self.tilt_conf >= 0.5).then_some(self.tilt_at)
    }
}

/// Rule by rule, how much the photo breaks it.
pub fn judge(c: &Composition) -> Judgement {
    let mut j = Judgement::default();
    let portrait = c.is_portrait();
    let close_up = c.face_h >= CLOSE_UP_FACE;

    // Thirds: the eyes (or the subject) on a third, the eyes on the upper third. A face filling
    // the frame cannot sit on a vertical third.
    if c.subject != subject::NONE && (c.subject != subject::PERSON || c.face_h == 0.0 || portrait) {
        let (dx, dy) = c.thirds_offset();
        // A face filling the frame cannot sit on a vertical third; a group is usually centred.
        let wx = if c.group >= 2 {
            0.2
        } else if c.face_h >= 0.45 {
            0.3
        } else {
            0.6
        };
        j.thirds = ramp(dx, 0.05, 0.15) * wx + ramp(dy, 0.06, 0.16) * (1.0 - wx);
        if j.thirds < 0.15 {
            j.good |= strength::THIRDS;
        }
    } else if let Some(hy) = c.horizon() {
        // A landscape cut in half by its horizon.
        j.thirds = if (hy - 0.5).abs() < 0.06 { 0.6 } else { 0.0 };
        if (hy - 1.0 / 3.0).abs() < 0.06 || (hy - 2.0 / 3.0).abs() < 0.06 {
            j.good |= strength::THIRDS;
        }
    }

    if portrait {
        if let Some(ht) = c.head_top {
            j.headroom = if close_up {
                // Close-ups crop the head; the eyes still belong in the upper half.
                ramp(c.ky, 0.55, 0.7)
            } else {
                // Head touching or cut by the top edge, or the person sunk to the bottom with
                // empty space above.
                let tight = ramp(0.03 - ht, 0.0, 0.03) * 0.5 + if ht <= 0.005 { 0.5 } else { 0.0 };
                let loose = ramp(ht, 0.22, 0.4) * ramp(c.ky, 0.42, 0.6);
                tight.max(loose)
            };
            if j.headroom == 0.0 && (close_up || (0.03..=0.2).contains(&ht)) {
                j.good |= strength::HEADROOM;
            }
        }
        if let Some(front) = c.front_room() {
            if c.face_h < 0.5 {
                j.lead_room = ramp(0.45 - front, 0.0, 0.15) * ramp(c.facing.abs(), 0.3, 0.6);
                if front >= 0.5 {
                    j.good |= strength::LEAD_ROOM;
                }
            }
        }
        j.merger = ramp(c.pole, 0.5, 0.8).max(ramp(c.neck_line, 0.55, 0.85));
        // Close-ups may crop the top of the head (and the forehead), not the chin or a side.
        let cut = if close_up { c.face_cut - c.face_cut_top } else { c.face_cut };
        j.cut = ramp(cut, 0.02, 0.15) * if c.face_h > 0.6 { 0.3 } else { 1.0 };
    }

    // Tilt: a clear line a little off level. Tilts beyond the search range are intended.
    if c.tilt_conf >= 0.3 && c.tilt.abs() < 9.5 {
        j.tilt = ramp(c.tilt.abs(), 0.6, 2.0) * ramp(c.tilt_conf, 0.3, 0.55);
        if c.tilt.abs() < 0.4 && c.tilt_conf >= 0.5 {
            j.good |= strength::LEVEL;
        }
    }

    if let (Some(s), Some(b)) = (c.subject_sharp, c.bg_sharp) {
        j.focus_bg = ramp(b - s, 0.25, 0.7);
    }
    if let (Some(s), Some(b)) = (c.subject_detail, c.bg_detail) {
        if s - b >= 1.0 && j.focus_bg == 0.0 {
            j.good |= strength::ISOLATED;
        }
    }
    j
}

/// One line of the composition check, for the photographer.
#[derive(Clone, Debug, PartialEq)]
pub struct Note {
    /// The rule ([`crate::learn::reason`] bit), to dim the rules turned off.
    pub rule: u32,
    /// Some(false) = breaks the rule, Some(true) = follows it, None = measured, no verdict.
    pub ok: Option<bool>,
    pub text: String,
}

fn pct(v: f32) -> String {
    format!("{:.0}%", v * 100.0)
}

/// What the composition check found, in words.
pub fn notes(c: &Composition) -> Vec<Note> {
    use crate::learn::reason;
    let j = judge(c);
    let verdict = |bad: f32, good: bool| {
        if bad >= 0.5 {
            Some(false)
        } else if good {
            Some(true)
        } else {
            None
        }
    };
    let mut out = Vec::new();
    let what = match c.subject {
        subject::PERSON if c.people > 1 => format!("인물 {}명", c.people),
        subject::PERSON => "인물".to_string(),
        subject::FOCUS => "초점이 맞은 부분".to_string(),
        _ => "뚜렷한 피사체 없음".to_string(),
    };
    let size = if c.subject != subject::NONE { format!(" · 화면의 {}", pct(c.area)) } else { String::new() };
    out.push(Note { rule: 0, ok: None, text: format!("피사체: {what}{size}") });

    if c.subject != subject::NONE && (c.subject != subject::PERSON || c.face_h == 0.0 || c.is_portrait()) {
        let (dx, dy) = c.thirds_offset();
        let at = if c.face_h > 0.0 { "눈높이" } else { "피사체" };
        let side = if (c.kx - 0.5).abs() < 0.05 {
            "가운데".to_string()
        } else {
            format!("세로 3분할 선에서 {}", pct(dx))
        };
        out.push(Note {
            rule: reason::THIRDS,
            ok: verdict(j.thirds, j.good & strength::THIRDS != 0),
            text: format!(
                "3분할: {side} · {at}가 {}선에서 {}",
                if c.face_h > 0.0 { "위 1/3 " } else { "3분할 " },
                pct(dy)
            ),
        });
    } else if let Some(hy) = c.horizon() {
        out.push(Note {
            rule: reason::THIRDS,
            ok: verdict(j.thirds, j.good & strength::THIRDS != 0),
            text: format!("수평선 높이: 위에서 {}", pct(hy)),
        });
    }
    if c.is_portrait() {
        if let Some(ht) = c.head_top {
            let text = if c.face_h >= CLOSE_UP_FACE {
                format!("헤드룸: 클로즈업 · 눈높이 위에서 {}", pct(c.ky))
            } else if ht <= 0.005 {
                "헤드룸: 머리가 위쪽 프레임에 닿거나 잘림".to_string()
            } else {
                format!("헤드룸: 머리 위 여백 {}", pct(ht))
            };
            out.push(Note { rule: reason::HEADROOM, ok: verdict(j.headroom, j.good & strength::HEADROOM != 0), text });
        }
        match c.front_room() {
            Some(front) if c.face_h < 0.5 => out.push(Note {
                rule: reason::LEAD_ROOM,
                ok: verdict(j.lead_room, j.good & strength::LEAD_ROOM != 0),
                text: format!(
                    "시선 앞 공간: {}쪽을 봄 · 앞 {} / 뒤 {}",
                    if c.facing > 0.0 { "오른" } else { "왼" },
                    pct(front),
                    pct(1.0 - front)
                ),
            }),
            _ => out.push(Note { rule: reason::LEAD_ROOM, ok: None, text: "시선 앞 공간: 정면을 봄".into() }),
        }
        let merger = if ramp(c.pole, 0.5, 0.8) >= 0.5 {
            "머리 뒤 배경: 머리 위로 세로선(기둥 · 나무 · 모서리)이 솟음"
        } else if ramp(c.neck_line, 0.55, 0.85) >= 0.5 {
            "머리 뒤 배경: 머리 · 목을 가로지르는 선"
        } else {
            "머리 뒤 배경: 겹치는 선 없음"
        }
        .to_string();
        out.push(Note { rule: reason::MERGER, ok: Some(j.merger < 0.5), text: merger });
        let cut = if c.face_h >= CLOSE_UP_FACE { c.face_cut - c.face_cut_top } else { c.face_cut };
        if cut >= 0.02 {
            out.push(Note {
                rule: reason::CUT,
                ok: verdict(j.cut, false),
                text: format!("얼굴의 {}가 프레임 밖", pct(cut)),
            });
        }
    }
    if c.tilt_conf >= 0.3 && c.tilt.abs() < 9.5 {
        let kind = if c.tilt_vertical { "수직(세로선)" } else { "수평" };
        out.push(Note {
            rule: reason::TILT,
            ok: verdict(j.tilt, j.good & strength::LEVEL != 0),
            text: if c.tilt.abs() < 0.4 {
                format!("{kind}: 맞음")
            } else {
                format!("{kind}: {:.1}° 기울어짐", c.tilt.abs())
            },
        });
    }
    if c.subject_sharp.is_some() || c.subject_detail.is_some() {
        let text = match (c.subject_sharp, c.bg_sharp) {
            (Some(s), Some(b)) if j.focus_bg >= 0.5 => {
                format!("초점: 배경이 인물보다 선명 (가장자리 {:.1}배)", (b - s).exp())
            }
            _ if j.good & strength::ISOLATED != 0 => "초점: 인물 · 배경은 흐려 인물이 돋보임".to_string(),
            _ => "초점: 인물에 맞음".to_string(),
        };
        out.push(Note { rule: reason::FOCUS_BG, ok: verdict(j.focus_bg, j.good & strength::ISOLATED != 0), text });
    }
    out
}

/// What the composition-similarity learner compares (see [`crate::learn`]): where the key point
/// is, how big the subject is, how much room above the head, the frame orientation.
pub fn vector(c: &Composition) -> Option<[f32; 6]> {
    if c.subject == subject::NONE {
        return None;
    }
    Some([
        c.kx,
        c.ky,
        c.area.sqrt(),
        c.head_top.unwrap_or(c.by).min(0.5),
        (c.face_h * 2.0).min(1.0),
        if c.aspect >= 1.0 { 0.5 } else { 0.0 },
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn face(x: f32, y: f32, w: f32, h: f32, nose_dx: f32) -> DetectedFace {
        let (ey, ex0, ex1) = (y + h * 0.4, x + w * 0.3, x + w * 0.7);
        let nose = (x + w * 0.5 + nose_dx * w, y + h * 0.6);
        DetectedFace {
            x,
            y,
            w,
            h,
            score: 0.9,
            landmarks: [(ex0, ey), (ex1, ey), nose, (x + w * 0.35, y + h * 0.8), (x + w * 0.65, y + h * 0.8)],
        }
    }

    fn flat(w: usize, h: usize) -> Gray {
        Gray { w, h, px: vec![128.0; w * h] }
    }

    /// A person mask: a head (circle) above shoulders (box), in normalised coordinates.
    fn person_mask(head_cx: f32, head_top: f32, head_r: f32) -> Mask {
        let n = 192;
        let p = (0..n * n)
            .map(|i| {
                let (x, y) = ((i % n) as f32 / n as f32, (i / n) as f32 / n as f32);
                let (cx, cy) = (head_cx, head_top + head_r);
                let head = ((x - cx) / (head_r * 0.75)).powi(2) + ((y - cy) / head_r).powi(2) <= 1.0;
                let body = y >= cy + head_r * 0.8 && (x - cx).abs() <= head_r * 2.2;
                if head || body {
                    0.95
                } else {
                    0.02
                }
            })
            .collect();
        Mask::new(n, n, p)
    }

    #[test]
    fn thirds_headroom_and_facing() {
        let g = flat(1500, 1000);
        // Face on the left third, eyes on the upper third, looking right, room above the head.
        let f = face(1500.0 / 3.0 - 75.0, 1000.0 / 3.0 - 60.0, 150.0, 150.0, 0.12);
        let m = person_mask(1.0 / 3.0, 0.2, 0.12);
        let c = measure(&g, &[f], Some(&m));
        assert_eq!(c.subject, subject::PERSON);
        assert!(c.facing > 0.5, "looks right: {}", c.facing);
        let ht = c.head_top.unwrap();
        assert!((ht - 0.2).abs() < 0.03, "head top {ht}");
        let j = judge(&c);
        assert!(j.thirds < 0.15, "{j:?}");
        assert_eq!(j.headroom, 0.0);
        assert_eq!(j.lead_room, 0.0, "room in front");
        assert!(j.good & strength::LEAD_ROOM != 0 && j.good & strength::THIRDS != 0);

        // The same face on the right third, still looking right: no room in front.
        let f = face(1000.0 - 75.0, 1000.0 / 3.0 - 60.0, 150.0, 150.0, 0.12);
        let m = person_mask(2.0 / 3.0, 0.2, 0.12);
        let j = judge(&measure(&g, &[f], Some(&m)));
        assert!(j.lead_room > 0.5, "{j:?}");

        // Head touching the top edge.
        let f = face(700.0, 30.0, 150.0, 150.0, 0.0);
        let m = person_mask(0.5, 0.0, 0.12);
        let c = measure(&g, &[f], Some(&m));
        assert!(c.head_top.unwrap() <= 0.01);
        assert!(judge(&c).headroom >= 0.9);

        // Centred: off the thirds.
        assert!(judge(&c).thirds > 0.5);
    }

    #[test]
    fn close_up_may_crop_the_head() {
        let g = flat(1000, 1500);
        // A face filling half the height, its top cropped, eyes in the upper part.
        let f = face(250.0, -100.0, 500.0, 750.0, 0.0);
        let c = measure(&g, &[f], None);
        let j = judge(&c);
        assert_eq!(j.headroom, 0.0, "{c:?}");
        assert!(c.face_cut > 0.1);
        assert!(j.cut < 0.5, "close-ups crop the face on purpose: {j:?}");
    }

    #[test]
    fn pole_behind_the_head() {
        let (w, h) = (1500usize, 1000usize);
        let mut g = flat(w, h);
        // A dark vertical pole from the top of the frame down behind the head.
        for y in 0..h {
            for x in 735..765 {
                g.px[y * w + x] = 30.0;
            }
        }
        let f = face(675.0, 400.0, 150.0, 150.0, 0.0);
        let m = person_mask(0.5, 0.33, 0.1);
        let c = measure(&g, std::slice::from_ref(&f), Some(&m));
        assert!(c.pole > 0.8, "pole {}", c.pole);
        assert!((c.pole_x - 0.5).abs() < 0.03);
        assert!(judge(&c).merger > 0.9);
        // Without the pole: nothing.
        let c = measure(&flat(w, h), &[f], Some(&m));
        assert_eq!(c.pole, 0.0);
        assert_eq!(judge(&c).merger, 0.0);
    }

    #[test]
    fn tilted_horizon() {
        let (w, h) = (1600usize, 1000usize);
        for deg in [0.0f32, 1.5, -3.0] {
            let mut g = flat(w, h);
            let t = deg.to_radians().tan();
            for y in 0..h {
                for x in 0..w {
                    // Sky above a line through the centre, ground below.
                    let line = h as f32 * 0.55 + (x as f32 - w as f32 / 2.0) * t;
                    g.px[y * w + x] = if (y as f32) < line { 200.0 } else { 60.0 };
                }
            }
            let c = measure(&g, &[], None);
            assert!(!c.tilt_vertical);
            assert!((c.tilt - deg).abs() < 0.4, "{deg}: {}", c.tilt);
            assert!(c.tilt_conf > 0.8, "{deg}: conf {}", c.tilt_conf);
            assert!((c.tilt_at - 0.55).abs() < 0.03, "{deg}: at {}", c.tilt_at);
            let j = judge(&c);
            if deg == 0.0 {
                assert_eq!(j.tilt, 0.0);
                assert!(j.good & strength::LEVEL != 0);
            } else {
                assert!(j.tilt > 0.6, "{deg}: {j:?}");
            }
        }
    }

    /// Blocks of 12 px in two tones, box-blurred by `blur` px where `soft(x, y)`.
    fn blocks(w: usize, h: usize, blur: usize, soft: impl Fn(usize, usize) -> bool) -> Gray {
        let sharp: Vec<f32> =
            (0..w * h).map(|i| if ((i % w) / 12 + (i / w) / 12).is_multiple_of(2) { 60.0 } else { 190.0 }).collect();
        let r = blur as i64;
        let px = (0..w * h)
            .map(|i| {
                let (x, y) = (i % w, i / w);
                if !soft(x, y) || r == 0 {
                    return sharp[i];
                }
                let (mut s, mut n) = (0.0, 0.0);
                for dy in -r..=r {
                    for dx in -r..=r {
                        let (xx, yy) = ((x as i64 + dx).clamp(0, w as i64 - 1), (y as i64 + dy).clamp(0, h as i64 - 1));
                        s += sharp[yy as usize * w + xx as usize];
                        n += 1.0;
                    }
                }
                s / n
            })
            .collect();
        Gray { w, h, px }
    }

    #[test]
    fn focus_on_the_background() {
        let (w, h) = (900usize, 600usize);
        let m = person_mask(0.5, 0.25, 0.15);
        let on_person = |x: usize, y: usize| m.at(x as f32 / w as f32, y as f32 / h as f32) > 0.5;
        let f = face(390.0, 180.0, 120.0, 120.0, 0.0);
        // The same texture everywhere: blurred on the person (focus behind), then on the background.
        let behind = measure(&blocks(w, h, 3, on_person), std::slice::from_ref(&f), Some(&m));
        let (s, b) = (behind.subject_sharp.unwrap(), behind.bg_sharp.unwrap());
        assert!(b > s + 0.7, "subject {s} background {b}");
        assert!(judge(&behind).focus_bg > 0.9);
        let on_subject = measure(&blocks(w, h, 3, |x, y| !on_person(x, y)), std::slice::from_ref(&f), Some(&m));
        assert_eq!(judge(&on_subject).focus_bg, 0.0, "{on_subject:?}");
        // All sharp: no verdict either way.
        let all = measure(&blocks(w, h, 0, |_, _| false), &[f], Some(&m));
        assert_eq!(judge(&all).focus_bg, 0.0, "{all:?}");
    }

    /// Composition of real photos, printed, with the measurements drawn on a copy:
    /// `PHOTOORG_TEST_IMAGES=dir cargo test -p photoorg-cull composition_of_samples -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn composition_of_samples() {
        use image::Rgb;
        let dir = std::env::var("PHOTOORG_TEST_IMAGES").expect("set PHOTOORG_TEST_IMAGES");
        let out = std::path::Path::new(&dir).join("composition");
        std::fs::create_dir_all(&out).unwrap();
        let mut paths: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .filter(|p| matches!(p.extension().and_then(|e| e.to_str()), Some("jpg" | "png")))
            .collect();
        paths.sort();
        for path in paths {
            let img = image::open(&path).unwrap().to_rgb8();
            let s = crate::ANALYSIS_EDGE as f32 / img.width().max(img.height()) as f32;
            let img = image::imageops::resize(
                &img,
                (img.width() as f32 * s) as u32,
                (img.height() as f32 * s) as u32,
                image::imageops::FilterType::Triangle,
            );
            let t = std::time::Instant::now();
            let a = crate::analyze_image(&img);
            let c = &a.comp;
            println!("== {} ({:?})", path.file_name().unwrap().to_string_lossy(), t.elapsed());
            println!("   {:?}", judge(c));
            for n in notes(c) {
                println!(
                    "   {} {}",
                    match n.ok {
                        Some(true) => "✓",
                        Some(false) => "✗",
                        None => "·",
                    },
                    n.text
                );
            }
            // Thirds (grey), subject box (green), key point (red), head top (blue), pole / neck
            // line (magenta), strongest straight line (yellow).
            let mut o = img.clone();
            let (w, h) = (o.width() as f32, o.height() as f32);
            let line = |o: &mut image::RgbImage, x0: f32, y0: f32, x1: f32, y1: f32, col: Rgb<u8>| {
                let n = ((x1 - x0).abs().max((y1 - y0).abs()) * w.max(h)) as usize + 1;
                for k in 0..=n {
                    let t = k as f32 / n as f32;
                    let (x, y) = ((x0 + (x1 - x0) * t) * w, (y0 + (y1 - y0) * t) * h);
                    for d in -1i32..=1 {
                        let (px, py) = (x as i32 + d, y as i32 + d);
                        if px >= 0 && py >= 0 && (px as u32) < o.width() && (py as u32) < o.height() {
                            o.put_pixel(px as u32, py as u32, col);
                        }
                    }
                }
            };
            for v in [1.0 / 3.0, 2.0 / 3.0] {
                line(&mut o, v, 0.0, v, 1.0, Rgb([200, 200, 200]));
                line(&mut o, 0.0, v, 1.0, v, Rgb([200, 200, 200]));
            }
            if c.subject != subject::NONE {
                let g = Rgb([0, 220, 0]);
                line(&mut o, c.bx, c.by, c.bx + c.bw, c.by, g);
                line(&mut o, c.bx, c.by + c.bh, c.bx + c.bw, c.by + c.bh, g);
                line(&mut o, c.bx, c.by, c.bx, c.by + c.bh, g);
                line(&mut o, c.bx + c.bw, c.by, c.bx + c.bw, c.by + c.bh, g);
                line(&mut o, c.kx - 0.02, c.ky, c.kx + 0.02, c.ky, Rgb([255, 0, 0]));
                line(&mut o, c.kx, c.ky - 0.02, c.kx, c.ky + 0.02, Rgb([255, 0, 0]));
                if c.facing.abs() >= 0.3 {
                    line(&mut o, c.kx, c.ky, c.kx + 0.12 * c.facing.signum(), c.ky, Rgb([255, 0, 0]));
                }
            }
            if let Some(ht) = c.head_top {
                line(&mut o, c.face_x - 0.08, ht, c.face_x + 0.08, ht, Rgb([0, 120, 255]));
            }
            if c.pole >= 0.3 {
                line(&mut o, c.pole_x, 0.0, c.pole_x, c.head_top.unwrap_or(0.5), Rgb([255, 0, 255]));
            }
            if c.neck_line >= 0.3 {
                line(&mut o, 0.0, c.neck_y, 1.0, c.neck_y, Rgb([255, 0, 255]));
            }
            if c.tilt_conf >= 0.3 {
                let t = c.tilt.to_radians().tan();
                if c.tilt_vertical {
                    line(
                        &mut o,
                        c.tilt_at + 0.5 * t / c.aspect,
                        0.0,
                        c.tilt_at - 0.5 * t / c.aspect,
                        1.0,
                        Rgb([255, 220, 0]),
                    );
                } else {
                    line(
                        &mut o,
                        0.0,
                        c.tilt_at - 0.5 * t * c.aspect,
                        1.0,
                        c.tilt_at + 0.5 * t * c.aspect,
                        Rgb([255, 220, 0]),
                    );
                }
            }
            o.save(out.join(path.file_name().unwrap()).with_extension("jpg")).unwrap();
            // The same photo with the focus behind the person: blur the person only.
            if let Ok(m) = crate::segment::person(&img) {
                let soft = image::imageops::blur(&img, 2.5);
                let mut missed = img.clone();
                for (x, y, p) in missed.enumerate_pixels_mut() {
                    if m.at(x as f32 / w, y as f32 / h) >= 0.5 {
                        *p = *soft.get_pixel(x, y);
                    }
                }
                let b = crate::analyze_image(&missed).comp;
                println!(
                    "   focus missed: subject {:?} bg {:?} → focus_bg {:.2} (sharp photo {:.2})",
                    b.subject_sharp,
                    b.bg_sharp,
                    judge(&b).focus_bg,
                    judge(c).focus_bg
                );
            }
        }
    }

    #[test]
    fn notes_explain() {
        let g = flat(1500, 1000);
        let f = face(1000.0 - 75.0, 1000.0 / 3.0 - 60.0, 150.0, 150.0, 0.12);
        let m = person_mask(2.0 / 3.0, 0.2, 0.12);
        let notes = notes(&measure(&g, &[f], Some(&m)));
        let lead = notes.iter().find(|n| n.rule == crate::learn::reason::LEAD_ROOM).unwrap();
        assert_eq!(lead.ok, Some(false));
        assert!(lead.text.contains("오른쪽을 봄"), "{}", lead.text);
    }
}
