//! 작품 폴더 (work folders): the photos of a project gathered into
//! `<library>/작품/<project>/<theme>/`, so the project can be opened as one folder (for example
//! added to Lightroom) and edited there.
//!
//! - **Moving** (the default) keeps a single copy. Files go into the folder together with their
//!   XMP sidecars, the catalog follows them (the app keeps showing them by date) and remembers
//!   where they came from, so the move can be undone with [`restore`].
//! - **Copying** leaves the dated folders as they are; the copies are recorded so gathering again
//!   only adds photos that are not there yet.
//!
//! A photo with several themes goes into the folder of its first theme only (all its themes stay
//! in the XMP keywords), so an editor never sees it twice. Gathering is incremental: photos already
//! in the folder stay where they are, even if their themes changed since.

use crate::catalog::{Catalog, FileRecord, TagKind};
use crate::export::{copy_file, safe_folder, Names};
use crate::library::Library;
use crate::media::FileKind;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

/// Folder (in the library) holding the 작품 folders.
pub const WORKS_DIR: &str = "작품";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum GatherMode {
    #[default]
    Move,
    Copy,
}

#[derive(Clone, Copy, Debug)]
pub struct GatherOptions {
    pub mode: GatherMode,
    /// One subfolder per theme (otherwise everything in the project folder).
    pub by_theme: bool,
    /// Refresh the XMP sidecars first, so editors see current ratings and keywords.
    pub write_xmp: bool,
}

/// Library-relative folder of a project (from its name).
pub fn project_folder(project_name: &str) -> String {
    format!("{WORKS_DIR}/{}", safe_folder(project_name))
}

/// Name of a theme's subfolder.
pub fn theme_folder(theme: &str) -> String {
    safe_folder(theme)
}

/// A photo to gather.
#[derive(Clone, Debug, PartialEq)]
pub struct GatherItem {
    pub asset_id: i64,
    /// Destination folder (library-relative).
    pub folder: String,
    /// Copied rather than moved (copy mode, or the photo was moved into another 작품 folder).
    pub copy: bool,
    pub bytes: u64,
}

/// Where the photos of a project stand with respect to its folder.
#[derive(Clone, Debug, Default)]
pub struct GatherPlan {
    pub project: u32,
    /// The project folder (library-relative), e.g. `작품/바다 사진집`.
    pub folder: String,
    /// Photos still to gather.
    pub items: Vec<GatherItem>,
    /// Photos in the project.
    pub members: usize,
    /// Already moved into the folder.
    pub moved: usize,
    /// Already copied into the folder.
    pub copied: usize,
    /// Of `items`: photos moved into another 작품 folder, which are copied instead.
    pub elsewhere: usize,
    /// Photos moved into the folder that are no longer in the project.
    pub strays: Vec<i64>,
    /// Bytes of `items`.
    pub bytes: u64,
}

impl GatherPlan {
    pub fn gathered(&self) -> usize {
        self.moved + self.copied
    }
    pub fn copies(&self) -> usize {
        self.items.iter().filter(|i| i.copy).count()
    }
}

#[derive(Clone, Debug, Default)]
pub struct GatherSummary {
    /// Photos moved (or returned, for [`restore`]).
    pub moved: usize,
    pub copied: usize,
    /// Files moved or copied (sidecars included).
    pub files: usize,
    pub bytes: u64,
    /// Photos that could not be moved, with the reason.
    pub failed: Vec<(String, String)>,
    pub cancelled: bool,
    /// The project folder.
    pub folder: PathBuf,
}

/// What gathering `project` with `opts` would do (catalog plus a few file checks; no changes).
pub fn plan(lib: &Library, catalog: &Catalog, project: u32, opts: &GatherOptions) -> Result<GatherPlan> {
    let p = catalog.projects()?.into_iter().find(|p| p.id == project).context("작품을 찾을 수 없습니다")?;
    let folder = project_folder(&p.name);
    let mut themes: Vec<_> =
        catalog.tags()?.into_iter().filter(|t| t.kind == TagKind::Theme && t.project == Some(project)).collect();
    themes.sort_by_key(|t| (t.sort, t.id));
    // Every member with the folder of its first theme.
    let mut first: BTreeMap<i64, String> = BTreeMap::new();
    for t in &themes {
        for id in catalog.tag_members(t.id)? {
            first.entry(id).or_insert_with(|| theme_folder(t.short_name()));
        }
    }
    let copies = catalog.work_copies(project)?;
    let here = format!("{folder}/");
    let works = format!("{WORKS_DIR}/");
    let mut plan = GatherPlan { project, folder: folder.clone(), members: first.len(), ..Default::default() };
    for (&id, theme) in &first {
        let files = catalog.files_for(id)?;
        if files.is_empty() {
            continue;
        }
        if files.iter().any(|f| f.rel_path.starts_with(&here)) {
            plan.moved += 1;
            continue;
        }
        if copies.get(&id).is_some_and(|c| !c.is_empty() && c.iter().all(|r| lib.abs(r).exists())) {
            plan.copied += 1;
            continue;
        }
        let elsewhere = files.iter().any(|f| f.rel_path.starts_with(&works));
        let copy = opts.mode == GatherMode::Copy || elsewhere;
        if elsewhere && opts.mode == GatherMode::Move {
            plan.elsewhere += 1;
        }
        let bytes: u64 = files.iter().map(|f| f.size).sum();
        plan.bytes += bytes;
        let dest = if opts.by_theme { format!("{folder}/{theme}") } else { folder.clone() };
        plan.items.push(GatherItem { asset_id: id, folder: dest, copy, bytes });
    }
    let mut strays: Vec<i64> = catalog
        .files_under(&folder)?
        .into_iter()
        .filter(|f| f.home_rel.is_some() && !first.contains_key(&f.asset_id))
        .map(|f| f.asset_id)
        .collect();
    strays.dedup();
    plan.strays = strays;
    Ok(plan)
}

/// The files of a photo to carry along: its files (RAW first) and the XMP sidecar next to them
/// (the RAW's when the RAW and the JPEG each have one: they would get the same name).
fn sources(lib: &Library, files: &[FileRecord]) -> Vec<(PathBuf, Option<i64>)> {
    let mut files: Vec<&FileRecord> = files.iter().collect();
    files.sort_by_key(|f| if f.kind == FileKind::Raw { 0 } else { 1 });
    let mut out: Vec<(PathBuf, Option<i64>)> = files.iter().map(|f| (lib.abs(&f.rel_path), Some(f.id))).collect();
    for f in &files {
        let sc = crate::xmp::sidecar_path(&lib.abs(&f.rel_path));
        let name = name_of(&sc).to_lowercase();
        if sc.is_file() && !out.iter().any(|(p, _)| name_of(p).to_lowercase() == name) {
            out.push((sc, None));
        }
    }
    out
}

fn name_of(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// Move one file; across drives by copying, checking the size and removing the original.
fn move_file(src: &Path, dst: &Path) -> Result<u64> {
    if dst.exists() {
        bail!("이미 있습니다: {}", dst.display());
    }
    let len = std::fs::metadata(src).with_context(|| format!("파일이 없습니다: {}", src.display()))?.len();
    if std::fs::rename(src, dst).is_err() {
        copy_file(src, dst)?;
        if std::fs::metadata(dst).map(|m| m.len()).unwrap_or(0) != len {
            let _ = std::fs::remove_file(dst);
            bail!("복사한 크기가 다릅니다: {}", src.display());
        }
        std::fs::remove_file(src).with_context(|| format!("원래 파일을 지울 수 없습니다: {}", src.display()))?;
    }
    Ok(len)
}

/// Move the files of one photo (`(source, file id, destination)`) and record the new paths, all
/// or nothing: on any error the files already moved are put back.
fn move_photo(
    lib: &Library,
    catalog: &mut Catalog,
    moves: &[(PathBuf, Option<i64>, PathBuf)],
    returning: bool,
) -> Result<(usize, u64)> {
    let mut done: Vec<(&Path, &Path)> = Vec::new();
    let undo = |done: &[(&Path, &Path)]| {
        for (src, dst) in done.iter().rev() {
            let _ = move_file(dst, src);
        }
    };
    let mut bytes = 0;
    let mut updates = Vec::new();
    for (src, id, dst) in moves {
        match move_file(src, dst) {
            Ok(n) => {
                bytes += n;
                done.push((src, dst));
                if let Some(id) = id {
                    match lib.rel(dst) {
                        Some(rel) => updates.push((*id, rel)),
                        None => {
                            undo(&done);
                            bail!("라이브러리 밖입니다: {}", dst.display());
                        }
                    }
                }
            }
            Err(e) => {
                undo(&done);
                return Err(e);
            }
        }
    }
    let r = if returning { catalog.return_files(&updates) } else { catalog.move_files(&updates) };
    if let Err(e) = r {
        undo(&done);
        return Err(e);
    }
    Ok((done.len(), bytes))
}

/// Gather the photos of `plan` into the project folder.
pub fn gather(
    lib: &Library,
    catalog: &mut Catalog,
    plan: &GatherPlan,
    opts: &GatherOptions,
    cancel: &AtomicBool,
    progress: &dyn Fn(usize, usize),
) -> Result<GatherSummary> {
    let mut sum = GatherSummary { folder: lib.abs(&plan.folder), ..Default::default() };
    let mut names: HashMap<String, Names> = HashMap::new();
    let mut left: HashSet<PathBuf> = HashSet::new();
    let total = plan.items.len();
    progress(0, total);
    for (i, item) in plan.items.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            sum.cancelled = true;
            break;
        }
        if opts.write_xmp {
            if let Err(e) = lib.write_xmp(catalog, item.asset_id) {
                log::warn!("xmp {}: {e:#}", item.asset_id);
            }
        }
        let files = catalog.files_for(item.asset_id)?;
        let label =
            files.first().map(|f| name_of(Path::new(&f.rel_path))).unwrap_or_else(|| format!("#{}", item.asset_id));
        let dir = lib.abs(&item.folder);
        std::fs::create_dir_all(&dir).with_context(|| format!("폴더를 만들 수 없습니다: {}", dir.display()))?;
        let srcs = sources(lib, &files);
        let given: Vec<String> = srcs.iter().map(|(p, _)| name_of(p)).collect();
        let chosen =
            names.entry(item.folder.clone()).or_insert_with(|| Names::new(Some(dir.clone()))).take_group(&given);
        let moves: Vec<(PathBuf, Option<i64>, PathBuf)> =
            srcs.into_iter().zip(chosen).map(|((src, id), name)| (src, id, dir.join(name))).collect();
        let r = if item.copy {
            copy_photo(lib, catalog, plan.project, item.asset_id, &moves)
        } else {
            let r = move_photo(lib, catalog, &moves, false);
            if r.is_ok() {
                left.extend(moves.iter().filter_map(|(src, _, _)| src.parent().map(Path::to_path_buf)));
            }
            r
        };
        match r {
            Ok((n, bytes)) => {
                if item.copy {
                    sum.copied += 1;
                } else {
                    sum.moved += 1;
                }
                sum.files += n;
                sum.bytes += bytes;
            }
            Err(e) => sum.failed.push((label, format!("{e:#}"))),
        }
        progress(i + 1, total);
    }
    // Dated folders emptied by moving.
    for dir in left {
        prune_empty(lib, &dir);
    }
    Ok(sum)
}

fn copy_photo(
    lib: &Library,
    catalog: &mut Catalog,
    project: u32,
    asset_id: i64,
    copies: &[(PathBuf, Option<i64>, PathBuf)],
) -> Result<(usize, u64)> {
    let mut made: Vec<&Path> = Vec::new();
    let mut bytes = 0;
    let mut rels = Vec::new();
    for (src, _, dst) in copies {
        match copy_file(src, dst) {
            Ok(n) => {
                bytes += n;
                made.push(dst);
                rels.extend(lib.rel(dst));
            }
            Err(e) => {
                for p in made {
                    let _ = std::fs::remove_file(p);
                }
                return Err(e);
            }
        }
    }
    catalog.set_work_copies(project, asset_id, &rels)?;
    Ok((made.len(), bytes))
}

/// Move the photos in a project folder back to the dated folders they came from (all of them, or
/// only `only`). Copies and files the app did not put there are left alone; folders left empty
/// are removed.
pub fn restore(
    lib: &Library,
    catalog: &mut Catalog,
    folder: &str,
    only: Option<&[i64]>,
    cancel: &AtomicBool,
    progress: &dyn Fn(usize, usize),
) -> Result<GatherSummary> {
    let mut sum = GatherSummary { folder: lib.abs(folder), ..Default::default() };
    let mut by_asset: BTreeMap<i64, Vec<FileRecord>> = BTreeMap::new();
    for f in catalog.files_under(folder)? {
        if f.home_rel.is_some() && only.is_none_or(|o| o.contains(&f.asset_id)) {
            by_asset.entry(f.asset_id).or_default().push(f);
        }
    }
    let mut names: HashMap<PathBuf, Names> = HashMap::new();
    let mut left: HashSet<PathBuf> = HashSet::new();
    let total = by_asset.len();
    progress(0, total);
    for (i, mut files) in by_asset.into_values().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            sum.cancelled = true;
            break;
        }
        files.sort_by_key(|f| if f.kind == FileKind::Raw { 0 } else { 1 });
        let label = name_of(Path::new(&files[0].rel_path));
        // Per home folder: (source, file id, name at home). The sidecar goes home with the first
        // file of the photo that has one (the RAW when there is one).
        let mut groups: BTreeMap<PathBuf, Vec<(PathBuf, Option<i64>, String)>> = BTreeMap::new();
        let mut taken_sidecars: HashSet<PathBuf> = HashSet::new();
        for f in &files {
            let src = lib.abs(&f.rel_path);
            let home = lib.abs(f.home_rel.as_deref().unwrap_or_default());
            let Some(dir) = home.parent().map(Path::to_path_buf) else { continue };
            let group = groups.entry(dir).or_default();
            group.push((src.clone(), Some(f.id), name_of(&home)));
            let sc = crate::xmp::sidecar_path(&src);
            if sc.is_file() && taken_sidecars.insert(sc.clone()) {
                group.push((sc, None, name_of(&crate::xmp::sidecar_path(&home))));
            }
        }
        let mut moves: Vec<(PathBuf, Option<i64>, PathBuf)> = Vec::new();
        let mut ok = true;
        for (dir, group) in groups {
            if let Err(e) = std::fs::create_dir_all(&dir) {
                sum.failed.push((label.clone(), format!("{e:#}")));
                ok = false;
                break;
            }
            let given: Vec<String> = group.iter().map(|(_, _, n)| n.clone()).collect();
            let chosen = names.entry(dir.clone()).or_insert_with(|| Names::new(Some(dir.clone()))).take_group(&given);
            for ((src, id, _), name) in group.into_iter().zip(chosen) {
                moves.push((src, id, dir.join(name)));
            }
        }
        if ok {
            match move_photo(lib, catalog, &moves, true) {
                Ok((n, bytes)) => {
                    sum.moved += 1;
                    sum.files += n;
                    sum.bytes += bytes;
                    left.extend(moves.iter().filter_map(|(src, _, _)| src.parent().map(Path::to_path_buf)));
                }
                Err(e) => sum.failed.push((label, format!("{e:#}"))),
            }
        }
        progress(i + 1, total);
    }
    for dir in left {
        prune_empty(lib, &dir);
    }
    Ok(sum)
}

/// Rename a folder of the library (e.g. a project folder when the project is renamed) and update
/// the paths inside it. Returns false when there is nothing to rename or the new name is taken.
pub fn rename_folder(lib: &Library, catalog: &mut Catalog, old: &str, new: &str) -> Result<bool> {
    let (from, to) = (lib.abs(old), lib.abs(new));
    if old == new || !from.is_dir() || to.exists() {
        return Ok(false);
    }
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::rename(&from, &to).with_context(|| format!("폴더 이름을 바꿀 수 없습니다: {}", from.display()))?;
    if let Err(e) = catalog.rename_dir(old, new) {
        let _ = std::fs::rename(&to, &from);
        return Err(e);
    }
    Ok(true)
}

/// Files the system leaves in folders (they do not keep a folder alive).
fn is_junk(name: &str) -> bool {
    matches!(name, ".DS_Store" | "Thumbs.db" | "desktop.ini") || name.starts_with("._")
}

/// Remove `dir` and its parents while they are empty, up to (not including) the library root.
fn prune_empty(lib: &Library, dir: &Path) {
    let mut dir = dir.to_path_buf();
    while dir.starts_with(&lib.root) && dir != lib.root && dir != lib.data_dir() {
        let Ok(rd) = std::fs::read_dir(&dir) else { return };
        let entries: Vec<_> = rd.flatten().collect();
        if entries.iter().any(|e| !is_junk(&e.file_name().to_string_lossy())) {
            return;
        }
        for e in &entries {
            let _ = std::fs::remove_file(e.path());
        }
        if std::fs::remove_dir(&dir).is_err() {
            return;
        }
        match dir.parent() {
            Some(p) => dir = p.to_path_buf(),
            None => return,
        }
    }
}
