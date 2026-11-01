//! Culling assistant: finds likely B-cuts (out of focus, shaken, closed eyes, badly exposed,
//! weaker frame of a burst) and learns the user's own A/B decisions per scene.
//!
//! Pipeline per photo: decode the embedded preview at [`ANALYSIS_EDGE`] px → classical
//! measurements ([`metrics`]) → face detection + eye state ([`vision`], [`eyes`]) → scene
//! ([`scene`]). Library-wide: percentiles, burst grouping, model training and scoring
//! ([`learn`]).

pub mod eyes;
pub mod learn;
pub mod metrics;
pub mod scene;
pub mod vision;

use crate::catalog::Catalog;
use crate::library::Library;
use crate::media::FileKind;
use crate::metadata::JpegRef;
use crate::preview;
use anyhow::{Context, Result};
use image::RgbImage;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;

pub use learn::{LearnSettings, Report, Score};
pub use scene::Scene;

/// Bump when the analysis changes so old results are recomputed.
pub const ANALYSIS_VERSION: u32 = 1;
/// Long edge of the image the analysis runs on.
pub const ANALYSIS_EDGE: u32 = 1600;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct FaceInfo {
    /// Box normalised to 0..1.
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub score: f32,
    /// ln(1 + Laplacian variance) inside the face.
    pub sharp: f32,
    /// Eye openness 0..1 of the less open eye (None if too small to judge).
    pub eyes: Option<f32>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Analysis {
    pub version: u32,
    pub sharp_peak: f32,
    pub sharp_global: f32,
    pub motion: f32,
    pub clip_high: f32,
    pub clip_low: f32,
    pub brightness: f32,
    pub faces: Vec<FaceInfo>,
    pub scene: Scene,
    pub scene_conf: f32,
    /// Top ImageNet classes (index, probability).
    pub classes: Vec<(u16, f32)>,
    pub dhash: u64,
    /// False when the neural networks could not run (classical metrics only).
    pub nets: bool,
}

impl Analysis {
    /// Largest face as a share of the image area.
    pub fn face_area(&self) -> f32 {
        self.faces.iter().map(|f| f.w * f.h).fold(0.0, f32::max)
    }

    /// Faces large enough to matter for the photo.
    pub fn main_faces(&self) -> impl Iterator<Item = &FaceInfo> {
        let biggest = self.face_area();
        self.faces.iter().filter(move |f| f.w * f.h >= (biggest * 0.25).max(0.002))
    }

    /// Sharpness of the main faces (the least sharp one matters most).
    pub fn face_sharp(&self) -> Option<f32> {
        self.main_faces().map(|f| f.sharp).fold(None, |a: Option<f32>, s| Some(a.map_or(s, |a| a.min(s))))
    }

    /// Openness of the least open eye among the main faces.
    pub fn eyes(&self) -> Option<f32> {
        self.main_faces().filter_map(|f| f.eyes).fold(None, |a: Option<f32>, e| Some(a.map_or(e, |a| a.min(e))))
    }
}

/// Analyse an already decoded, correctly oriented image.
pub fn analyze_image(img: &RgbImage) -> Analysis {
    let mut gray = metrics::Gray::from_rgb(img);
    let dhash = metrics::dhash(&gray);
    gray.normalize_levels(3.0);
    let sharp = metrics::sharpness(&gray);
    let exp = metrics::exposure(img);
    let (w, h) = (img.width() as f32, img.height() as f32);
    let mut a = Analysis {
        version: ANALYSIS_VERSION,
        sharp_peak: sharp.peak,
        sharp_global: sharp.global,
        motion: sharp.motion,
        clip_high: exp.clip_high,
        clip_low: exp.clip_low,
        brightness: exp.brightness,
        dhash,
        nets: true,
        ..Default::default()
    };
    match vision::detect_faces(img, 0.7) {
        Ok(faces) => {
            for f in faces {
                if f.w < 8.0 || f.h < 8.0 {
                    continue;
                }
                // Inner part of the box: skin, eyes, mouth (not background).
                let (cx, cy) = (f.x + f.w / 2.0, f.y + f.h / 2.0);
                let (iw, ih) = (f.w * 0.35, f.h * 0.35);
                let st = metrics::region_stats(
                    &gray,
                    (cx - iw).max(0.0) as usize,
                    (cy - ih).max(0.0) as usize,
                    (cx + iw).min(w) as usize,
                    (cy + ih).min(h) as usize,
                );
                let re = eyes::openness(&gray, f.landmarks[0], f.landmarks[1]);
                let le = eyes::openness(&gray, f.landmarks[1], f.landmarks[0]);
                let eyes = match (re, le) {
                    (Some(a), Some(b)) => Some(a.min(b)),
                    _ => None,
                };
                a.faces.push(FaceInfo {
                    x: (f.x / w).clamp(0.0, 1.0),
                    y: (f.y / h).clamp(0.0, 1.0),
                    w: (f.w / w).clamp(0.0, 1.0),
                    h: (f.h / h).clamp(0.0, 1.0),
                    score: f.score,
                    sharp: (1.0 + st.lap_var).ln(),
                    eyes,
                });
            }
        }
        Err(e) => {
            log::warn!("face detection: {e:#}");
            a.nets = false;
        }
    }
    let probs = match vision::classify(img) {
        Ok(p) => Some(p),
        Err(e) => {
            log::warn!("classification: {e:#}");
            a.nets = false;
            None
        }
    };
    if let Some(p) = &probs {
        let mut idx: Vec<(usize, f32)> = p.iter().copied().enumerate().collect();
        idx.sort_by(|x, y| y.1.partial_cmp(&x.1).unwrap_or(std::cmp::Ordering::Equal));
        a.classes = idx.iter().take(3).map(|(i, v)| (*i as u16, *v)).collect();
    }
    let big_faces = a.faces.iter().filter(|f| f.w * f.h >= 0.002).count();
    let (scene, conf) = scene::decide(a.face_area(), big_faces, probs.as_deref());
    a.scene = scene;
    a.scene_conf = conf;
    a
}

/// Decode a file's preview and analyse it.
pub fn analyze_file(path: &Path, kind: FileKind, orientation: u8, previews: Option<&[JpegRef]>) -> Result<Analysis> {
    let img = preview::load_image(path, kind, orientation, ANALYSIS_EDGE, previews)?;
    Ok(analyze_image(&img))
}

/// Analyse `ids` in parallel and store the results. `on_done(id, count)` reports progress.
pub fn analyze_assets(
    lib: &Library,
    ids: &[i64],
    cancel: &AtomicBool,
    on_done: &(dyn Fn(i64, usize) + Sync),
) -> Result<usize> {
    let catalog = Mutex::new(lib.catalog()?);
    let count = AtomicUsize::new(0);
    let _ = vision::warm_up();
    ids.par_iter().for_each(|&id| {
        if cancel.load(Ordering::Relaxed) {
            return;
        }
        let (files, orientation) = {
            let c = catalog.lock().unwrap();
            (c.files_for(id).unwrap_or_default(), c.metadata(id).map(|m| m.orientation).unwrap_or(1))
        };
        let Some(f) = files.iter().filter(|f| !f.kind.is_video()).min_by_key(|f| f.kind.preview_priority()) else {
            return;
        };
        match analyze_file(&lib.abs(&f.rel_path), f.kind, orientation, None) {
            Ok(a) => {
                if catalog.lock().unwrap().put_analysis(id, &a).is_ok() {
                    let n = count.fetch_add(1, Ordering::Relaxed) + 1;
                    on_done(id, n);
                }
            }
            Err(e) => log::debug!("analyse {}: {e:#}", f.rel_path),
        }
    });
    Ok(count.load(Ordering::Relaxed))
}

/// Retrain from the current labels and rescore the whole library (fast; no image decoding).
pub fn rescore(catalog: &mut Catalog, settings: &LearnSettings) -> Result<(Vec<Score>, Report)> {
    let items = catalog.cull_items().context("load analysis")?;
    let (scores, report, models) = learn::score(&items, settings);
    catalog.put_scores(&scores)?;
    catalog.put_models(&models)?;
    Ok((scores, report))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn analyses_synthetic_image() {
        let img = metrics::tests::scene(800, 533);
        let a = analyze_image(&img);
        assert!(a.nets, "networks should load from the embedded models");
        assert!(a.sharp_peak > 4.0);
        assert!(a.faces.is_empty());
        assert_eq!(a.classes.len(), 3);
        let blurred = analyze_image(&image::imageops::blur(&img, 4.0));
        assert!(blurred.sharp_peak < a.sharp_peak - 1.0);
    }

    /// Faces/eyes on real photos: `PHOTOORG_TEST_IMAGES=dir cargo test -- --ignored faces`
    /// (expects lena.jpg and messi5.jpg from the OpenCV samples).
    #[test]
    #[ignore]
    fn faces_on_sample_photos() {
        let dir = std::env::var("PHOTOORG_TEST_IMAGES").expect("set PHOTOORG_TEST_IMAGES");
        for (name, faces, scene) in
            [("lena.jpg", 1, Scene::Portrait), ("messi5.jpg", 1, Scene::Portrait), ("fruits.jpg", 0, Scene::Food)]
        {
            let img = image::open(format!("{dir}/{name}")).unwrap().to_rgb8();
            let a = analyze_image(&img);
            println!("{name}: scene {:?} faces {:?} classes {:?}", a.scene, a.faces, a.classes);
            assert_eq!(a.faces.len(), faces, "{name}");
            assert_eq!(a.scene, scene, "{name}");
        }
    }
}
