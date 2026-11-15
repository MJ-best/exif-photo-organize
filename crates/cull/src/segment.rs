//! Person and background: where the people are in the frame (PP-HumanSeg, run locally with tract
//! on a 192×192 copy of the image). The mask is a probability per cell of a grid over the whole
//! frame. (A general salient-object network was tried for other subjects and was ~20× slower
//! than everything else in the analysis; [`crate::composition`] uses the sharp region instead.)

use anyhow::{Context, Result};
use image::{imageops, RgbImage};
use std::sync::OnceLock;
use tract_onnx::prelude::*;

static HUMANSEG_ONNX: &[u8] = include_bytes!("../models/human_segmentation_pphumanseg_2023mar.onnx");

const HUMANSEG_SIZE: usize = 192;

type Plan = std::sync::Arc<TypedRunnableModel>;

static NET: OnceLock<std::result::Result<Plan, String>> = OnceLock::new();

fn net() -> Result<&'static Plan> {
    NET.get_or_init(|| {
        let load = || -> TractResult<Plan> {
            tract_onnx::onnx()
                .model_for_read(&mut std::io::Cursor::new(HUMANSEG_ONNX))?
                .with_input_fact(0, f32::fact([1, 3, HUMANSEG_SIZE, HUMANSEG_SIZE]).into())?
                .into_optimized()?
                .into_runnable()
        };
        load().map_err(|e| format!("{e:#}"))
    })
    .as_ref()
    .map_err(|e| anyhow::anyhow!("인물 분할 모델을 불러오지 못했습니다: {e}"))
}

/// Load the network ahead of time.
pub fn warm_up() -> Result<()> {
    net().map(|_| ())
}

/// Probabilities 0..1 on a `w`×`h` grid stretched over the whole frame (row-major).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Mask {
    pub w: usize,
    pub h: usize,
    pub p: Vec<f32>,
}

impl Mask {
    pub fn new(w: usize, h: usize, p: Vec<f32>) -> Mask {
        debug_assert_eq!(p.len(), w * h);
        Mask { w, h, p }
    }

    /// Probability at a normalised position (nearest cell; 0 outside the frame).
    pub fn at(&self, x: f32, y: f32) -> f32 {
        if !(0.0..1.0).contains(&x) || !(0.0..1.0).contains(&y) || self.p.is_empty() {
            return 0.0;
        }
        let (i, j) = ((x * self.w as f32) as usize, (y * self.h as f32) as usize);
        self.p[j.min(self.h - 1) * self.w + i.min(self.w - 1)]
    }

    /// Share of the frame covered (sum of probabilities above 0.5).
    pub fn area(&self) -> f32 {
        if self.p.is_empty() {
            return 0.0;
        }
        self.p.iter().filter(|&&v| v >= 0.5).count() as f32 / self.p.len() as f32
    }
}

/// Image → NCHW tensor of `size`×`size` (stretched), `f(channel, value 0..1)` normalising.
fn input(img: &RgbImage, size: usize, f: impl Fn(usize, f32) -> f32) -> Tensor {
    let small = imageops::resize(img, size as u32, size as u32, imageops::FilterType::Triangle);
    tract_ndarray::Array4::from_shape_fn((1, 3, size, size), |(_, c, y, x)| {
        f(c, small.get_pixel(x as u32, y as u32)[c] as f32 / 255.0)
    })
    .into()
}

/// Where a person is: probability per cell of a 192×192 grid over the frame.
pub fn person(img: &RgbImage) -> Result<Mask> {
    // RGB, (v - 0.5) / 0.5 (OpenCV Zoo demo).
    let x = input(img, HUMANSEG_SIZE, |_, v| (v - 0.5) / 0.5);
    let out = net()?.run(tvec!(x.into())).context("PP-HumanSeg")?;
    let o = out[0].to_plain_array_view::<f32>()?;
    let o = o.as_slice().context("humanseg output")?;
    let n = HUMANSEG_SIZE * HUMANSEG_SIZE;
    anyhow::ensure!(o.len() >= 2 * n, "humanseg output size {}", o.len());
    // Two channels (background, person): already a softmax in the exported model, but take one
    // again so that logits would work too.
    let p = (0..n)
        .map(|i| {
            let (bg, fg) = (o[i], o[n + i]);
            if (0.0..=1.0).contains(&bg) && (0.0..=1.0).contains(&fg) && (bg + fg - 1.0).abs() < 0.05 {
                fg
            } else {
                1.0 / (1.0 + (bg - fg).exp())
            }
        })
        .collect();
    Ok(Mask::new(HUMANSEG_SIZE, HUMANSEG_SIZE, p))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn models_load_and_run() {
        let img = crate::metrics::tests::scene(640, 427);
        let p = person(&img).unwrap();
        assert_eq!(p.p.len(), 192 * 192);
        assert!(p.p.iter().all(|v| (0.0..=1.0).contains(v)));
        // Rings and checkers are not much of a person (no face either: see composition).
        assert!(p.area() < 0.2, "{}", p.area());
    }

    /// Masks of real photos as PNGs next to them, with timings:
    /// `PHOTOORG_TEST_IMAGES=dir cargo test -p photoorg-cull masks_of_samples -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn masks_of_samples() {
        let dir = std::env::var("PHOTOORG_TEST_IMAGES").expect("set PHOTOORG_TEST_IMAGES");
        let out = std::path::Path::new(&dir).join("masks");
        std::fs::create_dir_all(&out).unwrap();
        warm_up().unwrap();
        for e in std::fs::read_dir(&dir).unwrap().flatten() {
            let path = e.path();
            if !matches!(path.extension().and_then(|e| e.to_str()), Some("jpg" | "png")) {
                continue;
            }
            let img = image::open(&path).unwrap().to_rgb8();
            let t = std::time::Instant::now();
            let p = person(&img).unwrap();
            let tp = t.elapsed();
            println!("{}: person {:.0}% in {tp:?}", path.file_name().unwrap().to_string_lossy(), p.area() * 100.0);
            let g = image::GrayImage::from_fn(p.w as u32, p.h as u32, |x, y| {
                image::Luma([(p.p[y as usize * p.w + x as usize] * 255.0) as u8])
            });
            g.save(out.join(format!("{}-person.png", path.file_stem().unwrap().to_string_lossy()))).unwrap();
        }
    }
}
