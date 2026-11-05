//! Culling assistant glue: runs the engine ([`photoorg_cull`]) on the library's photos and keeps
//! its results in the catalog. Everything about judging photos lives in the engine crate.

pub use photoorg_cull::*;

use crate::catalog::Catalog;
use crate::library::Library;
use crate::media::FileKind;
use crate::metadata::JpegRef;
use crate::preview;
use anyhow::{Context, Result};
use rayon::prelude::*;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;

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
