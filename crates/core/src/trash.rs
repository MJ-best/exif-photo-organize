//! Deleting photos: their files (RAW, JPEG, sidecars, copies in 작품 folders) go to the system
//! trash — never deleted outright — and the photos leave the catalog. What the photographer had
//! decided about them stays for the culling assistant ([`Catalog::forget_assets`]).

use crate::catalog::Catalog;
use crate::library::Library;
use crate::xmp;
use anyhow::Result;
use std::path::PathBuf;

/// What deleting would touch (for the confirmation).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TrashPlan {
    pub photos: usize,
    pub files: usize,
    pub bytes: u64,
}

#[derive(Clone, Debug, Default)]
pub struct TrashReport {
    /// Photos whose files are in the trash and that left the catalog.
    pub trashed: Vec<i64>,
    /// Photos that could not be moved to the trash (they stay): id and reason.
    pub failed: Vec<(i64, String)>,
    pub files: usize,
    pub bytes: u64,
    /// Decisions kept for learning.
    pub remembered: usize,
}

/// The files of one photo that exist on disk, sidecars included, without repeats.
fn files_on_disk(lib: &Library, rels: &[String]) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    for rel in rels {
        let p = lib.abs(rel);
        for f in [xmp::sidecar_path(&p), p] {
            if f.is_file() && !out.contains(&f) {
                out.push(f);
            }
        }
    }
    out
}

fn size(paths: &[PathBuf]) -> u64 {
    paths.iter().filter_map(|p| std::fs::metadata(p).ok()).map(|m| m.len()).sum()
}

pub fn plan(lib: &Library, cat: &Catalog, ids: &[i64]) -> Result<TrashPlan> {
    let mut plan = TrashPlan { photos: ids.len(), ..Default::default() };
    for (_, rels) in cat.paths_of(ids)? {
        let files = files_on_disk(lib, &rels);
        plan.files += files.len();
        plan.bytes += size(&files);
    }
    Ok(plan)
}

fn to_trash(paths: &[PathBuf]) -> Result<(), String> {
    if paths.is_empty() {
        return Ok(());
    }
    #[allow(unused_mut)]
    let mut ctx = trash::TrashContext::default();
    #[cfg(target_os = "macos")]
    {
        // NSFileManager rather than asking Finder: no permission prompt, and it works in the
        // App Sandbox (the Mac App Store build).
        use trash::macos::{DeleteMethod, TrashContextExtMacos};
        ctx.set_delete_method(DeleteMethod::NsFileManager);
    }
    ctx.delete_all(paths).map_err(|e| e.to_string())
}

/// Move the photos' files to the trash and take the photos out of the catalog (and their
/// thumbnails). A photo whose files cannot be moved stays where it is.
pub fn trash_assets(lib: &Library, cat: &mut Catalog, ids: &[i64]) -> Result<TrashReport> {
    let mut report = TrashReport::default();
    for (id, rels) in cat.paths_of(ids)? {
        let files = files_on_disk(lib, &rels);
        let bytes = size(&files);
        match to_trash(&files) {
            Ok(()) => {
                report.trashed.push(id);
                report.files += files.len();
                report.bytes += bytes;
            }
            Err(e) => {
                log::warn!("trash {id}: {e}");
                report.failed.push((id, e));
            }
        }
    }
    report.remembered = cat.forget_assets(&report.trashed)?;
    if let Ok(thumbs) = lib.thumbs() {
        let _ = thumbs.delete(&report.trashed);
    }
    Ok(report)
}
