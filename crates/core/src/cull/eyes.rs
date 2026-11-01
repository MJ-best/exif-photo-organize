//! Eye openness from a face's eye landmark.
//!
//! An open eye shows the round iris: the patch around the eye centre has strong *vertical*
//! edges (iris borders, eye corners). A closed eye is a lid with a lash line: almost only
//! *horizontal* edges. The ratio of horizontal-gradient energy to total gradient energy
//! separates the two well when the eye is large enough to measure.
//!
//! This is a heuristic, not a trained classifier; the learner and the comparison with burst
//! siblings (same person, same light) make it robust in practice.

use super::metrics::Gray;

/// Minimum distance between the eyes (pixels in the analysed image) to judge eye state.
pub const MIN_EYE_DISTANCE: f32 = 24.0;

/// Fraction of gradient energy that is horizontal (∂/∂x), i.e. vertical edges.
pub fn vertical_edge_ratio(g: &Gray, ex: f32, ey: f32, d: f32) -> f32 {
    let (pw, ph) = (32usize, 20usize);
    let (sw, sh) = (0.55 * d, 0.34 * d);
    let mut p = vec![0f32; pw * ph];
    for j in 0..ph {
        for i in 0..pw {
            let x = ex - sw / 2.0 + (i as f32 + 0.5) * sw / pw as f32;
            let y = ey - sh / 2.0 + (j as f32 + 0.5) * sh / ph as f32;
            p[j * pw + i] = g.sample(x, y);
        }
    }
    let (mut ex_, mut ey_) = (0f32, 0f32);
    for j in 1..ph - 1 {
        for i in 1..pw - 1 {
            ex_ += (p[j * pw + i + 1] - p[j * pw + i - 1]).abs();
            ey_ += (p[(j + 1) * pw + i] - p[(j - 1) * pw + i]).abs();
        }
    }
    ex_ / (ex_ + ey_ + 1e-6)
}

/// Openness 0 (closed) ..= 1 (open), or `None` when the eyes are too small to judge.
pub fn openness(g: &Gray, eye: (f32, f32), other_eye: (f32, f32)) -> Option<f32> {
    let d = ((eye.0 - other_eye.0).powi(2) + (eye.1 - other_eye.1).powi(2)).sqrt();
    if d < MIN_EYE_DISTANCE {
        return None;
    }
    let r = vertical_edge_ratio(g, eye.0, eye.1, d);
    Some(((r - 0.27) / 0.20).clamp(0.0, 1.0))
}
