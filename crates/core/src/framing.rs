//! Straighten and crop (크롭 · 수평), non-destructively: the original files are never changed.
//! A [`Framing`] is kept in the catalog and applied when photos are shown and when JPEGs are
//! exported.
//!
//! Geometry: the photo (as displayed, after its EXIF orientation) is rotated by `angle` around its
//! centre inside a frame of the same size, then the crop rectangle — normalised to that frame — is
//! cut out. Pixel aspect matters (a rotation is not a rotation in normalised coordinates), so most
//! functions take the photo's aspect (width / height).

use crate::cull::composition::{self, Composition};
use image::RgbImage;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Framing {
    /// Degrees; positive turns the photo clockwise.
    pub angle: f32,
    /// Crop in the rotated frame, normalised (0..1, from the top left).
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Default for Framing {
    fn default() -> Self {
        Framing { angle: 0.0, x: 0.0, y: 0.0, w: 1.0, h: 1.0 }
    }
}

/// Rotate `v` by `deg` degrees, clockwise on screen (y grows downwards).
fn rot(v: (f32, f32), deg: f32) -> (f32, f32) {
    let (s, c) = deg.to_radians().sin_cos();
    (v.0 * c - v.1 * s, v.0 * s + v.1 * c)
}

/// The most a photo is turned (larger tilts are intended, see [`composition::judge`]).
pub const MAX_ANGLE: f32 = 15.0;
const EPS: f32 = 1e-4;

impl Framing {
    pub fn is_identity(&self) -> bool {
        self.angle.abs() < 0.01 && self.x <= EPS && self.y <= EPS && self.w >= 1.0 - EPS && self.h >= 1.0 - EPS
    }

    /// Width / height of the framed photo, for a photo of `aspect`.
    pub fn aspect(&self, aspect: f32) -> f32 {
        aspect * self.w / self.h.max(EPS)
    }

    /// Point of the rotated frame → point of the photo (both normalised).
    pub fn frame_to_photo(&self, aspect: f32, fx: f32, fy: f32) -> (f32, f32) {
        let (dx, dy) = rot(((fx - 0.5) * aspect, fy - 0.5), -self.angle);
        (dx / aspect + 0.5, dy + 0.5)
    }

    /// Point of the photo → point of the rotated frame.
    pub fn photo_to_frame(&self, aspect: f32, px: f32, py: f32) -> (f32, f32) {
        let (dx, dy) = rot(((px - 0.5) * aspect, py - 0.5), self.angle);
        (dx / aspect + 0.5, dy + 0.5)
    }

    /// Point of the framed photo (0..1 over the crop) → point of the photo: where to sample.
    pub fn to_photo(&self, aspect: f32, u: f32, v: f32) -> (f32, f32) {
        self.frame_to_photo(aspect, self.x + u * self.w, self.y + v * self.h)
    }

    /// Point of the photo → point of the framed photo (outside 0..1 when cropped away).
    pub fn to_framed(&self, aspect: f32, px: f32, py: f32) -> (f32, f32) {
        let (fx, fy) = self.photo_to_frame(aspect, px, py);
        ((fx - self.x) / self.w.max(EPS), (fy - self.y) / self.h.max(EPS))
    }

    /// The crop is inside the frame and shows only the photo (no empty corners).
    pub fn inside(&self, aspect: f32) -> bool {
        if self.x < -EPS || self.y < -EPS || self.x + self.w > 1.0 + EPS || self.y + self.h > 1.0 + EPS {
            return false;
        }
        [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)].iter().all(|&(u, v)| {
            let (px, py) = self.to_photo(aspect, u, v);
            (-EPS..=1.0 + EPS).contains(&px) && (-EPS..=1.0 + EPS).contains(&py)
        })
    }

    /// The largest crop of pixel aspect `ratio` (width / height; None = the photo's) centred on
    /// (`cx`, `cy`) of the frame that shows only the photo turned by `angle`.
    pub fn largest(angle: f32, aspect: f32, ratio: Option<f32>, cx: f32, cy: f32) -> Framing {
        let angle = angle.clamp(-MAX_ANGLE, MAX_ANGLE);
        let r = ratio.unwrap_or(aspect).max(0.05);
        // Normalised size of a crop with half-height `b` (in frame heights).
        let make = |b: f32| {
            let (w, h) = (2.0 * b * r / aspect, 2.0 * b);
            Framing { angle, x: cx - w / 2.0, y: cy - h / 2.0, w, h }
        };
        let (mut lo, mut hi) = (0.0f32, 1.0f32);
        for _ in 0..40 {
            let mid = (lo + hi) / 2.0;
            if make(mid).inside(aspect) {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        make(lo)
    }

    /// Straightened by `angle`: the largest centred crop of the photo's own shape (or `ratio`).
    pub fn straightened(angle: f32, aspect: f32, ratio: Option<f32>) -> Framing {
        Framing::largest(angle, aspect, ratio, 0.5, 0.5)
    }

    /// The closest framing to `self` that shows only the photo: shrunk around its centre (keeping
    /// its shape) and moved into the frame as needed.
    pub fn fit(&self, aspect: f32) -> Framing {
        let mut f = *self;
        f.angle = f.angle.clamp(-MAX_ANGLE, MAX_ANGLE);
        f.w = f.w.clamp(0.02, 1.0);
        f.h = f.h.clamp(0.02, 1.0);
        f.x = f.x.clamp(0.0, 1.0 - f.w);
        f.y = f.y.clamp(0.0, 1.0 - f.h);
        if f.inside(aspect) {
            return f;
        }
        // Towards the middle first (keeps the size), then smaller.
        let toward = |f: &Framing, t: f32, s: f32| {
            let (cx, cy) = (f.x + f.w / 2.0, f.y + f.h / 2.0);
            let (cx, cy) = (cx + (0.5 - cx) * t, cy + (0.5 - cy) * t);
            let (w, h) = (f.w * s, f.h * s);
            Framing { angle: f.angle, x: cx - w / 2.0, y: cy - h / 2.0, w, h }
        };
        let search = |ok: &dyn Fn(f32) -> bool| {
            let (mut lo, mut hi) = (0.0f32, 1.0f32);
            for _ in 0..30 {
                let mid = (lo + hi) / 2.0;
                if ok(mid) {
                    hi = mid;
                } else {
                    lo = mid;
                }
            }
            hi
        };
        if toward(&f, 1.0, 1.0).inside(aspect) {
            let t = search(&|t| toward(&f, t, 1.0).inside(aspect));
            return toward(&f, t, 1.0);
        }
        let centred = toward(&f, 1.0, 1.0);
        let s = search(&|s| toward(&centred, 0.0, 1.0 - s).inside(aspect));
        toward(&centred, 0.0, 1.0 - s)
    }

    /// Size in pixels of the framed photo, for a photo of `w`×`h` pixels.
    pub fn pixel_size(&self, w: u32, h: u32) -> (u32, u32) {
        (((self.w * w as f32).round() as u32).max(1), ((self.h * h as f32).round() as u32).max(1))
    }

    /// The framed photo (bilinear resampling where turned).
    pub fn apply(&self, img: &RgbImage) -> RgbImage {
        let (w, h) = img.dimensions();
        if self.is_identity() {
            return img.clone();
        }
        let aspect = w as f32 / h as f32;
        let (ow, oh) = self.pixel_size(w, h);
        if self.angle.abs() < 0.01 {
            let (x0, y0) = ((self.x * w as f32).round() as u32, (self.y * h as f32).round() as u32);
            return image::imageops::crop_imm(img, x0.min(w - 1), y0.min(h - 1), ow.min(w - x0), oh.min(h - y0))
                .to_image();
        }
        let sample = |px: f32, py: f32| -> [u8; 3] {
            let x = (px * w as f32 - 0.5).clamp(0.0, (w - 1) as f32);
            let y = (py * h as f32 - 0.5).clamp(0.0, (h - 1) as f32);
            let (x0, y0) = (x.floor() as u32, y.floor() as u32);
            let (x1, y1) = ((x0 + 1).min(w - 1), (y0 + 1).min(h - 1));
            let (fx, fy) = (x - x0 as f32, y - y0 as f32);
            let (a, b, c, d) =
                (img.get_pixel(x0, y0), img.get_pixel(x1, y0), img.get_pixel(x0, y1), img.get_pixel(x1, y1));
            let mut out = [0u8; 3];
            for k in 0..3 {
                let top = a[k] as f32 * (1.0 - fx) + b[k] as f32 * fx;
                let bottom = c[k] as f32 * (1.0 - fx) + d[k] as f32 * fx;
                out[k] = (top * (1.0 - fy) + bottom * fy).round() as u8;
            }
            out
        };
        use rayon::prelude::*;
        let rows: Vec<Vec<u8>> = (0..oh)
            .into_par_iter()
            .map(|j| {
                let mut row = Vec::with_capacity(ow as usize * 3);
                for i in 0..ow {
                    let (px, py) = self.to_photo(aspect, (i as f32 + 0.5) / ow as f32, (j as f32 + 0.5) / oh as f32);
                    row.extend_from_slice(&sample(px, py));
                }
                row
            })
            .collect();
        RgbImage::from_raw(ow, oh, rows.concat()).expect("framed image size")
    }

    /// The composition measured on the photo, as it would be on the framed photo.
    pub fn composition(&self, c: &Composition) -> Composition {
        if self.is_identity() {
            return c.clone();
        }
        let a = c.aspect;
        c.reframed(self.angle, |x, y| self.to_framed(a, x, y), self.w, self.h)
    }
}

/// How a photo could be framed better, from its composition: turned level (when a clear line is
/// off), and — with `compose` — cropped so the framing rules hold better (eyes on the thirds,
/// room above the head and in front of the face), cropping as little as possible and never more
/// than [`MAX_COMPOSE_CROP`] of each side's length (a half-length portrait stays one). Rules in `off`
/// ([`crate::cull::learn::reason`] bits) are not considered. `ratio` keeps a shape (None = the
/// photo's own).
/// The smallest crop "자동 구도" makes, as a share of the straightened photo's width and height.
pub const MAX_COMPOSE_CROP: f32 = 0.8;

pub fn suggest(c: &Composition, ratio: Option<f32>, compose: bool, off: u32) -> Framing {
    use crate::cull::learn::reason;
    let aspect = c.aspect;
    let angle = if c.tilt_conf >= 0.3 && c.tilt.abs() >= 0.4 && c.tilt.abs() < 9.5 { -c.tilt } else { 0.0 };
    let base = Framing::straightened(angle, aspect, ratio);
    if !compose || c.subject == composition::subject::NONE {
        return base;
    }
    let cost = |f: &Framing| -> f32 {
        let j = composition::judge(&f.composition(c));
        let on = |r: u32| if off & r == 0 { 1.0 } else { 0.0 };
        on(reason::THIRDS) * j.thirds
            + 1.5 * on(reason::HEADROOM) * j.headroom
            + 1.5 * on(reason::LEAD_ROOM) * j.lead_room
            + 4.0 * j.cut
    };
    let mut best = (cost(&base), base);
    let start = best.0;
    for k in 0..=8 {
        let s = 1.0 - k as f32 * (1.0 - MAX_COMPOSE_CROP) / 8.0;
        let (w, h) = (base.w * s, base.h * s);
        for iy in 0..=8 {
            for ix in 0..=8 {
                let cx = base.x + w / 2.0 + (base.w - w) * ix as f32 / 8.0;
                let cy = base.y + h / 2.0 + (base.h - h) * iy as f32 / 8.0;
                let f = Framing { angle, x: cx - w / 2.0, y: cy - h / 2.0, w, h };
                if !f.inside(aspect) {
                    continue;
                }
                // Cropping costs: as little as needed.
                let v = cost(&f) + 2.5 * (1.0 - s);
                if v < best.0 - 1e-4 {
                    best = (v, f);
                }
            }
        }
    }
    // Not worth a crop for a small gain.
    if start - best.0 < 0.15 {
        base
    } else {
        best.1
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cull::composition::subject;

    #[test]
    fn straightened_crop_shows_only_the_photo() {
        for aspect in [1.5f32, 0.667, 1.0] {
            for angle in [0.0f32, 2.0, -5.0, 12.0] {
                let f = Framing::straightened(angle, aspect, None);
                assert!(f.inside(aspect), "{aspect} {angle}: {f:?}");
                assert!((f.aspect(aspect) - aspect).abs() < 0.01, "same shape");
                // As large as possible: a bit bigger would show a corner.
                let bigger = Framing { w: f.w * 1.02, h: f.h * 1.02, x: f.x - f.w * 0.01, y: f.y - f.h * 0.01, ..f };
                assert!(!bigger.inside(aspect) || angle == 0.0);
                // Round trip of points.
                let (u, v) = f.to_framed(aspect, 0.3, 0.7);
                let (x, y) = f.to_photo(aspect, u, v);
                assert!((x - 0.3).abs() < 1e-4 && (y - 0.7).abs() < 1e-4);
            }
        }
        // Level: the whole photo.
        let f = Framing::straightened(0.0, 1.5, None);
        assert!(f.is_identity(), "{f:?}");
        // 2° of a 3:2 photo costs a few percent per side.
        let f = Framing::straightened(2.0, 1.5, None);
        assert!(f.w > 0.9 && f.w < 0.97, "{f:?}");
        // A square from a 3:2 photo.
        let sq = Framing::straightened(0.0, 1.5, Some(1.0));
        assert!((sq.aspect(1.5) - 1.0).abs() < 0.01 && (sq.h - 1.0).abs() < 1e-3);
    }

    #[test]
    fn fit_moves_and_shrinks_into_the_photo() {
        let f = Framing { angle: 5.0, x: 0.6, y: 0.1, w: 0.5, h: 0.5 }.fit(1.5);
        assert!(f.inside(1.5), "{f:?}");
        assert!((f.w - 0.5).abs() < 1e-3, "moving is enough: {f:?}");
        let f = Framing { angle: 10.0, x: 0.0, y: 0.0, w: 1.0, h: 1.0 }.fit(1.5);
        assert!(f.inside(1.5) && f.w < 1.0 && (f.aspect(1.5) - 1.5).abs() < 0.01, "{f:?}");
    }

    #[test]
    fn apply_turns_and_crops() {
        // A white vertical line in the middle of a black 300×200 photo, leaning by 3°.
        let (w, h) = (300u32, 200u32);
        let lean = 3f32.to_radians().tan();
        let img = RgbImage::from_fn(w, h, |x, y| {
            let line = w as f32 / 2.0 + (y as f32 - h as f32 / 2.0) * lean;
            if (x as f32 - line).abs() < 2.0 {
                image::Rgb([255, 255, 255])
            } else {
                image::Rgb([0, 0, 0])
            }
        });
        // The bottom of the line is to the right of its top: the composition check reports such a
        // plumb line as -3° (counter-clockwise), and turning the photo 3° clockwise makes it plumb.
        let f = Framing::straightened(3.0, 1.5, None);
        let out = f.apply(&img);
        let (ow, oh) = out.dimensions();
        assert_eq!((ow, oh), f.pixel_size(w, h));
        let col = |y: u32| (0..ow).filter(|&x| out.get_pixel(x, y)[0] > 128).map(|x| x as f32).sum::<f32>() / 3.0;
        let (top, bottom) = (col(oh / 8), col(oh * 7 / 8));
        assert!((top - bottom).abs() < 1.5, "plumb: top {top} bottom {bottom}");
        // A plain crop.
        let c = Framing { angle: 0.0, x: 0.5, y: 0.0, w: 0.5, h: 0.5 }.apply(&img);
        assert_eq!(c.dimensions(), (150, 100));
    }

    /// Measured, suggested, applied, measured again: level afterwards, whichever way it leaned.
    #[test]
    fn auto_level_end_to_end() {
        use crate::cull::metrics::Gray;
        let (w, h) = (1200u32, 800u32);
        for (lean, plumb) in [(2.5f32, true), (-4.0, true), (1.5, false), (-2.0, false)] {
            let t = lean.to_radians().tan();
            // A wall edge (plumb) or a horizon (level), off by `lean` degrees.
            let img = RgbImage::from_fn(w, h, |x, y| {
                let (x, y) = (x as f32, y as f32);
                let bright = if plumb {
                    x < w as f32 / 2.0 + (y - h as f32 / 2.0) * t
                } else {
                    y < h as f32 / 2.0 + (x - w as f32 / 2.0) * t
                };
                let v = if bright { 200 } else { 60 };
                image::Rgb([v, v, v])
            });
            let c = composition::measure(&Gray::from_rgb(&img), &[], None);
            assert!(c.tilt.abs() > 1.0, "{lean} {plumb}: measured {}", c.tilt);
            let f = suggest(&c, None, false, 0);
            assert!(composition::judge(&f.composition(&c)).tilt == 0.0);
            let fixed = f.apply(&img);
            let again = composition::measure(&Gray::from_rgb(&fixed), &[], None);
            assert!(again.tilt.abs() < 0.4, "{lean} {plumb}: {} after turning by {}", again.tilt, f.angle);
        }
    }

    fn portrait(kx: f32, ky: f32, head_top: f32, facing: f32) -> Composition {
        Composition {
            subject: subject::PERSON,
            people: 1,
            group: 1,
            bx: kx - 0.1,
            by: head_top,
            bw: 0.2,
            bh: 1.0 - head_top,
            area: 0.15,
            kx,
            ky,
            aspect: 1.5,
            face_h: 0.15,
            face_x: kx,
            facing,
            head_top: Some(head_top),
            ..Default::default()
        }
    }

    #[test]
    fn suggestion_levels_and_reframes() {
        // A clear line 2° off: turned back, nothing else to fix in the middle of the frame.
        let mut c = portrait(1.0 / 3.0, 0.36, 0.12, 0.0);
        c.tilt = 2.0;
        c.tilt_conf = 0.8;
        let f = suggest(&c, None, false, 0);
        assert!((f.angle + 2.0).abs() < 1e-4 && f.inside(1.5));
        let after = composition::judge(&f.composition(&c));
        assert_eq!(after.tilt, 0.0, "level after turning");

        // Centred, looking right, eyes low with empty space above: the crop puts the eyes near a
        // third with room in front.
        let c = portrait(0.5, 0.5, 0.3, 0.9);
        let before = composition::judge(&c);
        let f = suggest(&c, None, true, 0);
        let after = composition::judge(&f.composition(&c));
        assert!(f.w < 1.0, "cropped: {f:?}");
        assert!(f.w >= MAX_COMPOSE_CROP - 1e-3 && f.h >= MAX_COMPOSE_CROP - 1e-3, "not too much: {f:?}");
        let score = |j: &composition::Judgement| j.thirds + j.headroom + j.lead_room;
        assert!(score(&after) < score(&before) - 0.3, "before {before:?} after {after:?}");
        // With the thirds rule off, thirds do not drive the crop.
        let f2 = suggest(&c, None, true, crate::cull::learn::reason::THIRDS);
        assert!(f2.inside(1.5));
    }
}
