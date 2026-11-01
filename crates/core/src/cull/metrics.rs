//! Classical image-quality measurements: focus (Laplacian variance), motion blur (gradient
//! anisotropy), exposure clipping and a perceptual hash for near-duplicate (burst) detection.

use image::RgbImage;

/// Grayscale plane, values 0..255.
pub struct Gray {
    pub w: usize,
    pub h: usize,
    pub px: Vec<f32>,
}

impl Gray {
    pub fn from_rgb(img: &RgbImage) -> Gray {
        let (w, h) = img.dimensions();
        let px = img.pixels().map(|p| 0.299 * p[0] as f32 + 0.587 * p[1] as f32 + 0.114 * p[2] as f32).collect();
        Gray { w: w as usize, h: h as usize, px }
    }

    /// Stretch a dark frame so its bright end (99th percentile) reaches ~230, with the gain
    /// capped at `max_gain`. Focus measures scale with contrast, so without this a sharp but
    /// dark photo (night, stage) would look blurred. Returns the gain applied.
    pub fn normalize_levels(&mut self, max_gain: f32) -> f32 {
        let mut hist = [0u32; 256];
        for &v in &self.px {
            hist[(v as usize).min(255)] += 1;
        }
        let target = (self.px.len() as f64 * 0.99) as u64;
        let (mut acc, mut p99) = (0u64, 255usize);
        for (i, &n) in hist.iter().enumerate() {
            acc += n as u64;
            if acc >= target {
                p99 = i;
                break;
            }
        }
        let gain = (230.0 / (p99 as f32).max(1.0)).clamp(1.0, max_gain);
        if gain > 1.01 {
            for v in &mut self.px {
                *v = (*v * gain).min(255.0);
            }
        }
        gain
    }

    #[inline]
    pub fn at(&self, x: usize, y: usize) -> f32 {
        self.px[y * self.w + x]
    }

    /// Bilinear sample (clamped to the image).
    pub fn sample(&self, x: f32, y: f32) -> f32 {
        let x = x.clamp(0.0, (self.w - 1) as f32);
        let y = y.clamp(0.0, (self.h - 1) as f32);
        let (x0, y0) = (x.floor() as usize, y.floor() as usize);
        let (x1, y1) = ((x0 + 1).min(self.w - 1), (y0 + 1).min(self.h - 1));
        let (fx, fy) = (x - x0 as f32, y - y0 as f32);
        let a = self.at(x0, y0) * (1.0 - fx) + self.at(x1, y0) * fx;
        let b = self.at(x0, y1) * (1.0 - fx) + self.at(x1, y1) * fx;
        a * (1.0 - fy) + b * fy
    }
}

/// Statistics of one rectangular region.
#[derive(Clone, Copy, Debug, Default)]
pub struct RegionStats {
    /// Variance of the 4-neighbour Laplacian (focus measure).
    pub lap_var: f32,
    /// Structure tensor sums.
    pub gxx: f64,
    pub gyy: f64,
    pub gxy: f64,
}

/// Laplacian variance and gradient structure tensor of `[x0,x1) × [y0,y1)`.
pub fn region_stats(g: &Gray, x0: usize, y0: usize, x1: usize, y1: usize) -> RegionStats {
    let (x0, y0) = (x0.max(1), y0.max(1));
    let (x1, y1) = (x1.min(g.w - 1), y1.min(g.h - 1));
    if x1 <= x0 || y1 <= y0 {
        return RegionStats::default();
    }
    let (mut s, mut s2, mut n) = (0f64, 0f64, 0f64);
    let (mut gxx, mut gyy, mut gxy) = (0f64, 0f64, 0f64);
    for y in y0..y1 {
        let row = y * g.w;
        for x in x0..x1 {
            let c = g.px[row + x];
            let l = g.px[row + x - 1];
            let r = g.px[row + x + 1];
            let u = g.px[row - g.w + x];
            let d = g.px[row + g.w + x];
            let lap = (l + r + u + d - 4.0 * c) as f64;
            s += lap;
            s2 += lap * lap;
            n += 1.0;
            let gx = ((r - l) * 0.5) as f64;
            let gy = ((d - u) * 0.5) as f64;
            gxx += gx * gx;
            gyy += gy * gy;
            gxy += gx * gy;
        }
    }
    let mean = s / n;
    RegionStats { lap_var: (s2 / n - mean * mean).max(0.0) as f32, gxx, gyy, gxy }
}

/// Coherence of the summed structure tensor: 0 = gradients in all directions, 1 = one direction.
pub fn anisotropy(gxx: f64, gyy: f64, gxy: f64) -> f32 {
    let tr = gxx + gyy;
    if tr <= 1e-9 {
        return 0.0;
    }
    let det_term = ((gxx - gyy).powi(2) + 4.0 * gxy * gxy).sqrt();
    (det_term / tr).clamp(0.0, 1.0) as f32
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Sharpness {
    /// ln(1 + Laplacian variance) of the sharpest tiles — "is anything in focus?"
    pub peak: f32,
    /// ln(1 + mean Laplacian variance) over the frame.
    pub global: f32,
    /// Gradient anisotropy in the most detailed tiles (motion blur indicator), 0..1.
    pub motion: f32,
}

pub const TILES: usize = 8;

pub fn sharpness(g: &Gray) -> Sharpness {
    let mut tiles: Vec<RegionStats> = Vec::with_capacity(TILES * TILES);
    for ty in 0..TILES {
        for tx in 0..TILES {
            let (x0, x1) = (tx * g.w / TILES, (tx + 1) * g.w / TILES);
            let (y0, y1) = (ty * g.h / TILES, (ty + 1) * g.h / TILES);
            tiles.push(region_stats(g, x0, y0, x1, y1));
        }
    }
    let mut vars: Vec<f32> = tiles.iter().map(|t| t.lap_var).collect();
    vars.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
    let top = &vars[..3.min(vars.len())];
    let peak = (1.0 + top.iter().sum::<f32>() / top.len() as f32).ln();
    let global = (1.0 + vars.iter().sum::<f32>() / vars.len() as f32).ln();
    // Motion: structure tensor of the 6 tiles with the most gradient energy.
    let mut by_energy: Vec<&RegionStats> = tiles.iter().collect();
    by_energy.sort_by(|a, b| (b.gxx + b.gyy).partial_cmp(&(a.gxx + a.gyy)).unwrap_or(std::cmp::Ordering::Equal));
    let (mut a, mut b, mut c) = (0f64, 0f64, 0f64);
    let mut per_tile = Vec::new();
    for t in by_energy.iter().take(6) {
        a += t.gxx;
        b += t.gyy;
        c += t.gxy;
        per_tile.push(anisotropy(t.gxx, t.gyy, t.gxy));
    }
    // Motion blur makes *every* detailed tile anisotropic in the same direction; a single
    // strongly oriented subject (a fence, a building edge) only affects some tiles.
    let combined = anisotropy(a, b, c);
    per_tile.sort_by(|x, y| x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal));
    let lowest = per_tile.first().copied().unwrap_or(0.0);
    Sharpness { peak, global, motion: combined.min(lowest + 0.25) }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Exposure {
    /// Fraction of blown (near-white) pixels. A single saturated channel (a red flower, an
    /// orange sunset) is not counted: that is colour, not overexposure.
    pub clip_high: f32,
    /// Fraction of pixels that are (almost) black.
    pub clip_low: f32,
    /// Mean luminance 0..1.
    pub brightness: f32,
}

pub fn exposure(img: &RgbImage) -> Exposure {
    let (mut hi, mut lo, mut sum) = (0u64, 0u64, 0f64);
    let n = (img.width() as u64 * img.height() as u64).max(1);
    for p in img.pixels() {
        let l = 0.299 * p[0] as f32 + 0.587 * p[1] as f32 + 0.114 * p[2] as f32;
        let (mx, mn) = (p[0].max(p[1]).max(p[2]), p[0].min(p[1]).min(p[2]));
        if mx >= 250 && mn >= 235 {
            hi += 1;
        }
        if l <= 6.0 {
            lo += 1;
        }
        sum += l as f64;
    }
    Exposure {
        clip_high: hi as f32 / n as f32,
        clip_low: lo as f32 / n as f32,
        brightness: (sum / n as f64 / 255.0) as f32,
    }
}

/// 64-bit difference hash (9×8 area-averaged grayscale, left < right).
pub fn dhash(g: &Gray) -> u64 {
    let (cw, ch) = (9usize, 8usize);
    let mut cells = [0f32; 72];
    for cy in 0..ch {
        for cx in 0..cw {
            let (x0, x1) = (cx * g.w / cw, ((cx + 1) * g.w / cw).max(cx * g.w / cw + 1));
            let (y0, y1) = (cy * g.h / ch, ((cy + 1) * g.h / ch).max(cy * g.h / ch + 1));
            let (mut s, mut n) = (0f32, 0f32);
            // Subsample for speed.
            let step = ((x1 - x0) / 16).max(1);
            for y in (y0..y1).step_by(step) {
                for x in (x0..x1).step_by(step) {
                    s += g.at(x.min(g.w - 1), y.min(g.h - 1));
                    n += 1.0;
                }
            }
            cells[cy * cw + cx] = s / n.max(1.0);
        }
    }
    let mut h = 0u64;
    for cy in 0..ch {
        for cx in 0..8 {
            if cells[cy * cw + cx] < cells[cy * cw + cx + 1] {
                h |= 1 << (cy * 8 + cx);
            }
        }
    }
    h
}

pub fn hamming(a: u64, b: u64) -> u32 {
    (a ^ b).count_ones()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use image::{imageops, Rgb};

    /// A detailed synthetic scene (checker + circles + gradient).
    pub fn scene(w: u32, h: u32) -> RgbImage {
        RgbImage::from_fn(w, h, |x, y| {
            let checker = ((x / 7 + y / 7) % 2) as u8 * 90;
            let r = ((x as f32 - w as f32 * 0.6).powi(2) + (y as f32 - h as f32 * 0.4).powi(2)).sqrt();
            let ring = if (r as u32 / 9).is_multiple_of(2) { 120 } else { 0 };
            let v = (checker as u32 + ring + (x * 40 / w)) as u8;
            Rgb([v, v.saturating_add(20), v / 2 + 40])
        })
    }

    pub fn motion_blur(img: &RgbImage, len: i32) -> RgbImage {
        let (w, h) = img.dimensions();
        RgbImage::from_fn(w, h, |x, y| {
            let mut acc = [0u32; 3];
            for d in -len..=len {
                let xx = (x as i32 + d).clamp(0, w as i32 - 1) as u32;
                let p = img.get_pixel(xx, y);
                for c in 0..3 {
                    acc[c] += p[c] as u32;
                }
            }
            let n = (2 * len + 1) as u32;
            Rgb([(acc[0] / n) as u8, (acc[1] / n) as u8, (acc[2] / n) as u8])
        })
    }

    #[test]
    fn focus_and_motion() {
        let sharp = scene(640, 420);
        let soft = imageops::blur(&sharp, 3.0);
        let shaken = motion_blur(&sharp, 6);
        let s = sharpness(&Gray::from_rgb(&sharp));
        let b = sharpness(&Gray::from_rgb(&soft));
        let m = sharpness(&Gray::from_rgb(&shaken));
        assert!(s.peak > b.peak + 1.5, "sharp {s:?} vs blurred {b:?}");
        assert!(s.peak > m.peak, "sharp {s:?} vs shaken {m:?}");
        assert!(m.motion > s.motion + 0.2, "motion blur should be anisotropic: {m:?} vs {s:?}");
        assert!(b.motion < 0.5, "gaussian blur is isotropic: {b:?}");
    }

    #[test]
    fn dark_but_sharp_is_not_blurred() {
        let sharp = scene(640, 420);
        let dark = RgbImage::from_fn(640, 420, |x, y| {
            let p = sharp.get_pixel(x, y);
            Rgb([p[0] / 4, p[1] / 4, p[2] / 4])
        });
        let s = sharpness(&Gray::from_rgb(&sharp));
        let raw = sharpness(&Gray::from_rgb(&dark));
        let mut g = Gray::from_rgb(&dark);
        assert!(g.normalize_levels(3.0) > 2.5);
        let d = sharpness(&g);
        assert!(raw.peak < s.peak - 2.0, "contrast drives the raw measure: {raw:?} vs {s:?}");
        assert!(d.peak > s.peak - 0.8, "normalised dark frame should read sharp: {d:?} vs {s:?}");
    }

    #[test]
    fn exposure_and_hash() {
        let white = RgbImage::from_pixel(100, 80, Rgb([255, 255, 255]));
        let e = exposure(&white);
        assert!(e.clip_high > 0.99 && e.brightness > 0.99);
        // A saturated yellow sky is colour, not blown highlights.
        let yellow = RgbImage::from_pixel(100, 80, Rgb([255, 214, 90]));
        assert_eq!(exposure(&yellow).clip_high, 0.0);
        let img = scene(320, 200);
        let shifted = imageops::crop_imm(&scene(330, 200), 4, 0, 320, 200).to_image();
        let other = imageops::rotate180(&img);
        let (a, b, c) =
            (dhash(&Gray::from_rgb(&img)), dhash(&Gray::from_rgb(&shifted)), dhash(&Gray::from_rgb(&other)));
        assert!(hamming(a, b) < 12, "near duplicate {}", hamming(a, b));
        assert!(hamming(a, c) > 20, "different {}", hamming(a, c));
    }
}
