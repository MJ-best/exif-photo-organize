//! 취향 (taste): what a photo shows and how it looks, as a compact embedding, so the engine can
//! learn which *kinds* of photos the photographer keeps or rejects — not only whether they are
//! sharp and well exposed.
//!
//! The embedding is MobileNetV2's 1280 pooled features (the layer the classifier reads), L2
//! normalised and reduced to [`DIM`] dimensions by a fixed random projection (distances are
//! kept, see Johnson–Lindenstrauss), so it is small enough to keep for every photo and needs no
//! fitting. The projection is part of the stored format: changing [`SEED`] or [`DIM`] means
//! analysing again.
//!
//! In learning ([`crate::learn`]), every photo gets two features: its mean similarity to the
//! five most similar photos the photographer rejected, and to the five most similar they kept.

use std::sync::OnceLock;

/// Stored embedding size.
pub const DIM: usize = 128;
/// MobileNetV2 pooled features.
pub const POOLED: usize = 1280;
/// Seed of the projection (part of the stored format).
const SEED: u64 = 0x7A57_E5EE_D001;

/// Gaussian random projection, `DIM` rows of `POOLED`.
fn projection() -> &'static [f32] {
    static P: OnceLock<Vec<f32>> = OnceLock::new();
    P.get_or_init(|| {
        let mut state = SEED;
        let mut uniform = || {
            // xorshift64*
            state ^= state >> 12;
            state ^= state << 25;
            state ^= state >> 27;
            let v = state.wrapping_mul(0x2545_F491_4F6C_DD1D);
            ((v >> 11) as f64 + 0.5) / (1u64 << 53) as f64
        };
        let mut out = Vec::with_capacity(DIM * POOLED);
        while out.len() < DIM * POOLED {
            // Box–Muller.
            let (u1, u2) = (uniform(), uniform());
            let r = (-2.0 * u1.ln()).sqrt();
            let t = std::f64::consts::TAU * u2;
            out.push((r * t.cos()) as f32);
            out.push((r * t.sin()) as f32);
        }
        out.truncate(DIM * POOLED);
        out
    })
}

fn normalize(v: &mut [f32]) -> bool {
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if n > 1e-9 && n.is_finite() {
        v.iter_mut().for_each(|x| *x /= n);
        true
    } else {
        false
    }
}

/// The stored embedding of a photo from MobileNetV2's pooled features (empty if unusable).
pub fn embed(pooled: &[f32]) -> Vec<f32> {
    if pooled.len() != POOLED {
        return Vec::new();
    }
    let mut x = pooled.to_vec();
    if !normalize(&mut x) {
        return Vec::new();
    }
    let p = projection();
    let mut e: Vec<f32> =
        (0..DIM).map(|r| p[r * POOLED..(r + 1) * POOLED].iter().zip(&x).map(|(a, b)| a * b).sum()).collect();
    if !normalize(&mut e) {
        return Vec::new();
    }
    // Four decimals are plenty and keep the catalog small.
    e.iter_mut().for_each(|v| *v = (*v * 10_000.0).round() / 10_000.0);
    e
}

/// Dot product in eight lanes (so the compiler can use SIMD; a plain running sum can't be
/// reordered and stays scalar).
pub fn dot(a: &[f32], b: &[f32]) -> f32 {
    let mut acc = [0f32; 8];
    let (ca, cb) = (a.chunks_exact(8), b.chunks_exact(8));
    let tail: f32 = ca.remainder().iter().zip(cb.remainder()).map(|(x, y)| x * y).sum();
    for (x, y) in ca.zip(cb) {
        for k in 0..8 {
            acc[k] += x[k] * y[k];
        }
    }
    acc.iter().sum::<f32>() + tail
}

/// Embeddings of a whole library centred on their mean and normalised again: the raw ones all
/// share a large common part (every photo is "a photo"), which would hide the differences.
pub fn centred(embeds: &[&[f32]]) -> Vec<Option<Vec<f32>>> {
    let usable = embeds.iter().filter(|e| e.len() == DIM).count();
    if usable == 0 {
        return vec![None; embeds.len()];
    }
    let mut mean = vec![0f32; DIM];
    for e in embeds.iter().filter(|e| e.len() == DIM) {
        for (m, v) in mean.iter_mut().zip(e.iter()) {
            *m += v;
        }
    }
    mean.iter_mut().for_each(|m| *m /= usable as f32);
    embeds
        .iter()
        .map(|e| {
            if e.len() != DIM {
                return None;
            }
            let mut v: Vec<f32> = e.iter().zip(&mean).map(|(a, m)| a - m).collect();
            normalize(&mut v).then_some(v)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projection_is_fixed_and_keeps_similarity() {
        let p = projection();
        assert_eq!(p.len(), DIM * POOLED);
        let mean = p.iter().map(|v| *v as f64).sum::<f64>() / p.len() as f64;
        let var = p.iter().map(|v| (*v as f64 - mean).powi(2)).sum::<f64>() / p.len() as f64;
        assert!(mean.abs() < 0.01 && (var - 1.0).abs() < 0.02, "N(0,1): mean {mean} var {var}");
        // The values never change (stored embeddings depend on them).
        let first: Vec<String> = p[..6].iter().map(|v| format!("{v:.4}")).collect();
        assert_eq!(first, ["-0.0074", "-0.9175", "-0.4084", "0.0145", "0.7795", "-0.5959"]);

        // Two similar feature vectors stay similar, two unrelated ones do not.
        let a: Vec<f32> = (0..POOLED).map(|i| ((i * 7919) % 97) as f32 / 97.0).collect();
        let b: Vec<f32> = a.iter().enumerate().map(|(i, v)| v + if i % 13 == 0 { 0.3 } else { 0.0 }).collect();
        let c: Vec<f32> = (0..POOLED).map(|i| ((i * 104729) % 89) as f32 / 89.0).collect();
        let (ea, eb, ec) = (embed(&a), embed(&b), embed(&c));
        assert_eq!(ea.len(), DIM);
        let cent = centred(&[&ea, &eb, &ec]);
        let s_ab = dot(cent[0].as_ref().unwrap(), cent[1].as_ref().unwrap());
        let s_ac = dot(cent[0].as_ref().unwrap(), cent[2].as_ref().unwrap());
        assert!(s_ab > s_ac + 0.5, "similar {s_ab} vs unrelated {s_ac}");
        assert!(embed(&[0.0; POOLED]).is_empty());
        assert!(embed(&[1.0; 10]).is_empty());
    }
}
