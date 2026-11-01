//! Neural networks (run locally with tract): YuNet face detector and MobileNetV2 classifier.

use anyhow::{Context, Result};
use image::{imageops, RgbImage};
use std::sync::OnceLock;
use tract_onnx::prelude::*;

static YUNET_ONNX: &[u8] = include_bytes!("../../models/face_detection_yunet_2023mar.onnx");
static MOBILENET_ONNX: &[u8] = include_bytes!("../../models/image_classification_mobilenetv2_2022apr.onnx");

type Plan = std::sync::Arc<TypedRunnableModel>;

struct Nets {
    yunet: Plan,
    mobilenet: Plan,
}

static NETS: OnceLock<std::result::Result<Nets, String>> = OnceLock::new();

fn nets() -> Result<&'static Nets> {
    NETS.get_or_init(|| {
        let load = || -> TractResult<Nets> {
            let yunet = tract_onnx::onnx()
                .model_for_read(&mut std::io::Cursor::new(YUNET_ONNX))?
                .into_optimized()?
                .into_runnable()?;
            let mobilenet = tract_onnx::onnx()
                .model_for_read(&mut std::io::Cursor::new(MOBILENET_ONNX))?
                .with_input_fact(0, f32::fact([1, 3, 224, 224]).into())?
                .into_optimized()?
                .into_runnable()?;
            Ok(Nets { yunet, mobilenet })
        };
        load().map_err(|e| format!("{e:#}"))
    })
    .as_ref()
    .map_err(|e| anyhow::anyhow!("모델을 불러오지 못했습니다: {e}"))
}

#[derive(Clone, Debug)]
pub struct DetectedFace {
    /// Box in image pixels.
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub score: f32,
    /// Right eye, left eye, nose tip, right / left mouth corner (image pixels).
    pub landmarks: [(f32, f32); 5],
}

fn iou(a: &DetectedFace, b: &DetectedFace) -> f32 {
    let x1 = a.x.max(b.x);
    let y1 = a.y.max(b.y);
    let x2 = (a.x + a.w).min(b.x + b.w);
    let y2 = (a.y + a.h).min(b.y + b.h);
    let inter = (x2 - x1).max(0.0) * (y2 - y1).max(0.0);
    inter / (a.w * a.h + b.w * b.h - inter).max(1e-6)
}

const YUNET_SIZE: usize = 640;

/// Detect faces (YuNet, 640×640 letterboxed input).
pub fn detect_faces(img: &RgbImage, threshold: f32) -> Result<Vec<DetectedFace>> {
    let nets = nets()?;
    let (w0, h0) = img.dimensions();
    let s = YUNET_SIZE as f32 / w0.max(h0) as f32;
    let (rw, rh) = (((w0 as f32 * s).round() as u32).max(1), ((h0 as f32 * s).round() as u32).max(1));
    let small = imageops::resize(img, rw, rh, imageops::FilterType::Triangle);
    let mut input = tract_ndarray::Array4::<f32>::zeros((1, 3, YUNET_SIZE, YUNET_SIZE));
    for (x, y, p) in small.enumerate_pixels() {
        let (x, y) = (x as usize, y as usize);
        // BGR, 0..255 (OpenCV blob without scaling).
        input[[0, 0, y, x]] = p[2] as f32;
        input[[0, 1, y, x]] = p[1] as f32;
        input[[0, 2, y, x]] = p[0] as f32;
    }
    let out = nets.yunet.run(tvec!(Tensor::from(input).into())).context("YuNet")?;
    let mut faces = Vec::new();
    for (k, stride) in [8usize, 16, 32].iter().enumerate() {
        let cls = out[k].to_plain_array_view::<f32>()?;
        let obj = out[3 + k].to_plain_array_view::<f32>()?;
        let bbox = out[6 + k].to_plain_array_view::<f32>()?;
        let kps = out[9 + k].to_plain_array_view::<f32>()?;
        let (cls, obj, bbox, kps) = (
            cls.as_slice().context("cls")?,
            obj.as_slice().context("obj")?,
            bbox.as_slice().context("bbox")?,
            kps.as_slice().context("kps")?,
        );
        let cols = YUNET_SIZE / stride;
        let st = *stride as f32;
        for i in 0..cls.len() {
            let score = (cls[i].clamp(0.0, 1.0) * obj[i].clamp(0.0, 1.0)).sqrt();
            if score < threshold {
                continue;
            }
            let (r, c) = ((i / cols) as f32, (i % cols) as f32);
            let cx = (c + bbox[i * 4]) * st;
            let cy = (r + bbox[i * 4 + 1]) * st;
            let bw = bbox[i * 4 + 2].exp() * st;
            let bh = bbox[i * 4 + 3].exp() * st;
            let mut lm = [(0f32, 0f32); 5];
            for (j, l) in lm.iter_mut().enumerate() {
                *l = ((kps[i * 10 + 2 * j] + c) * st / s, (kps[i * 10 + 2 * j + 1] + r) * st / s);
            }
            faces.push(DetectedFace {
                x: (cx - bw / 2.0) / s,
                y: (cy - bh / 2.0) / s,
                w: bw / s,
                h: bh / s,
                score,
                landmarks: lm,
            });
        }
    }
    faces.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    let mut keep: Vec<DetectedFace> = Vec::new();
    for f in faces {
        if keep.iter().all(|k| iou(k, &f) < 0.3) {
            keep.push(f);
        }
        if keep.len() >= 50 {
            break;
        }
    }
    Ok(keep)
}

/// ImageNet-1k class probabilities (MobileNetV2, 224×224 centre crop).
pub fn classify(img: &RgbImage) -> Result<Vec<f32>> {
    let nets = nets()?;
    let (w, h) = img.dimensions();
    let s = 256.0 / w.min(h) as f32;
    let (rw, rh) = (((w as f32 * s).round() as u32).max(224), ((h as f32 * s).round() as u32).max(224));
    let resized = imageops::resize(img, rw, rh, imageops::FilterType::Triangle);
    let crop = imageops::crop_imm(&resized, (rw - 224) / 2, (rh - 224) / 2, 224, 224).to_image();
    let mean = [0.485f32, 0.456, 0.406];
    let std = [0.229f32, 0.224, 0.225];
    let input = tract_ndarray::Array4::from_shape_fn((1, 3, 224, 224), |(_, c, y, x)| {
        (crop.get_pixel(x as u32, y as u32)[c] as f32 / 255.0 - mean[c]) / std[c]
    });
    let out = nets.mobilenet.run(tvec!(Tensor::from(input).into())).context("MobileNet")?;
    let logits = out[0].to_plain_array_view::<f32>()?;
    let max = logits.iter().copied().fold(f32::MIN, f32::max);
    let exps: Vec<f32> = logits.iter().map(|v| (v - max).exp()).collect();
    let sum: f32 = exps.iter().sum();
    Ok(exps.into_iter().map(|e| e / sum).collect())
}

/// Load the networks ahead of time (e.g. on a background thread).
pub fn warm_up() -> Result<()> {
    nets().map(|_| ())
}
