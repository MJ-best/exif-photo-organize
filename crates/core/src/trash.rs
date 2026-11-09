//! Deleting photos: their files (RAW, JPEG, sidecars, copies in 작품 folders) go to the system
//! trash — never deleted outright — and the photos leave the catalog. What the photographer had
//! decided about them stays for the culling assistant ([`Catalog::forget_assets`]).
//!
//! Android has no trash for an app's own files, so there they go to the library's own trash
//! folder ([`Library::trash_dir`], [`Bin::Library`]), which the app can empty.

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

/// Where deleted files go.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bin {
    /// The system's trash (the library's own on Android, which has none for apps).
    System,
    /// The library's own trash folder.
    Library,
}

fn to_trash(lib: &Library, paths: &[PathBuf], bin: Bin) -> Result<(), String> {
    match bin {
        #[cfg(not(target_os = "android"))]
        Bin::System => to_system_trash(paths),
        _ => to_library_trash(lib, paths),
    }
}

fn to_library_trash(lib: &Library, paths: &[PathBuf]) -> Result<(), String> {
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis();
    let base = lib.trash_dir().join(stamp.to_string());
    let mut moved: Vec<(PathBuf, PathBuf)> = Vec::new();
    for p in paths {
        let dest = match lib.rel(p) {
            Some(rel) => base.join(rel),
            None => base.join(p.file_name().unwrap_or_default()),
        };
        let res = dest.parent().map(std::fs::create_dir_all).unwrap_or(Ok(())).and_then(|_| std::fs::rename(p, &dest));
        if let Err(e) = res {
            // All files of a photo or none.
            for (from, to) in moved.iter().rev() {
                let _ = std::fs::rename(to, from);
            }
            return Err(format!("{}: {e}", p.display()));
        }
        moved.push((p.clone(), dest));
    }
    Ok(())
}

#[cfg(not(target_os = "android"))]
fn to_system_trash(paths: &[PathBuf]) -> Result<(), String> {
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
    trash_assets_in(lib, cat, ids, Bin::System)
}

/// [`trash_assets`] into a chosen trash.
pub fn trash_assets_in(lib: &Library, cat: &mut Catalog, ids: &[i64], bin: Bin) -> Result<TrashReport> {
    let mut report = TrashReport::default();
    for (id, rels) in cat.paths_of(ids)? {
        let files = files_on_disk(lib, &rels);
        let bytes = size(&files);
        match to_trash(lib, &files, bin) {
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
