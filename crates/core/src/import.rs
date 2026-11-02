//! Import pipeline: scan a card (or folder), plan destinations, copy with hashing and
//! verification, catalog every asset and build thumbnails — all at once.
//!
//! ```text
//!  SD card ──read thread──▶ [4 MB chunks, bounded] ──▶ write+hash ──▶ catalog ──▶ thumbnail pool
//! ```
//! Reading from the card and writing to the external drive overlap, so the import runs at the
//! speed of the slower device rather than the sum of both.

use crate::catalog::{naive_to_ms, Catalog, NewFile, ThumbStore};
use crate::library::{Library, DATA_DIR};
use crate::media::{is_junk_name, FileKind};
use crate::metadata::{self, JpegRef, Metadata};
use crate::{preview, tags, template, xmp};
use anyhow::{bail, Context, Result};
use crossbeam_channel::{bounded, Sender};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct ImportOptions {
    /// Folder template relative to the library root, e.g. `{YYYY}/{YYYY}-{MM}-{DD}`.
    pub folder_template: String,
    /// File name template without extension, e.g. `{orig}`.
    pub file_template: String,
    /// If not empty, RAW files go into this sub folder of the destination folder.
    pub raw_subfolder: String,
    pub include_videos: bool,
    /// fsync and re-read every copied file to verify its checksum.
    pub verify: bool,
    /// Copy existing `.xmp` sidecars along with the photos.
    pub copy_sidecars: bool,
}

impl Default for ImportOptions {
    fn default() -> Self {
        ImportOptions {
            folder_template: template::FOLDER_PRESETS[0].template.to_string(),
            file_template: "{orig}".to_string(),
            raw_subfolder: String::new(),
            include_videos: true,
            verify: true,
            copy_sidecars: true,
        }
    }
}

#[derive(Clone, Debug)]
pub struct SourceFile {
    pub path: PathBuf,
    pub name: String,
    pub kind: FileKind,
    pub size: u64,
    /// Unix seconds.
    pub mtime: Option<i64>,
    /// Asset id when this exact file is already in the catalog.
    pub existing: Option<i64>,
}

/// Files that belong together (e.g. `IMG_0001.CR3` + `IMG_0001.JPG`).
#[derive(Clone, Debug)]
pub struct SourceGroup {
    pub stem: String,
    /// Sorted by preview priority: `files[0]` is the primary file.
    pub files: Vec<SourceFile>,
    pub meta: Metadata,
    /// Embedded JPEGs of `files[0]`.
    pub previews: Vec<JpegRef>,
    pub sidecar: Option<PathBuf>,
}

impl SourceGroup {
    pub fn primary(&self) -> &SourceFile {
        &self.files[0]
    }
    pub fn formats(&self) -> u8 {
        self.files.iter().fold(0, |a, f| a | f.kind.format_flag())
    }
    pub fn bytes(&self) -> u64 {
        self.files.iter().map(|f| f.size).sum()
    }
    pub fn captured_ms(&self) -> i64 {
        naive_to_ms(self.meta.captured_at.unwrap_or_default())
    }
    pub fn is_imported(&self) -> bool {
        self.files.iter().all(|f| f.existing.is_some())
    }
    pub fn is_video(&self) -> bool {
        self.files.iter().all(|f| f.kind.is_video())
    }
    pub fn quick_key(&self, f: &SourceFile) -> String {
        format!("{}|{}|{}", f.name.to_lowercase(), f.size, self.captured_ms())
    }
}

#[derive(Clone, Debug, Default)]
pub struct ScanResult {
    pub source: PathBuf,
    pub label: String,
    pub groups: Vec<SourceGroup>,
    pub errors: Vec<String>,
}

#[derive(Clone, Debug, Default)]
pub struct ScanSummary {
    pub groups: usize,
    pub new_groups: usize,
    pub imported_groups: usize,
    pub new_files: usize,
    pub new_bytes: u64,
    pub videos: usize,
    pub raw_pairs: usize,
    pub first: Option<chrono::NaiveDateTime>,
    pub last: Option<chrono::NaiveDateTime>,
    pub cameras: Vec<(String, usize)>,
}

impl ScanResult {
    pub fn summary(&self, include_videos: bool) -> ScanSummary {
        let mut s = ScanSummary { groups: self.groups.len(), ..Default::default() };
        let mut cams: BTreeMap<String, usize> = BTreeMap::new();
        for g in &self.groups {
            if g.is_video() {
                s.videos += 1;
                if !include_videos {
                    continue;
                }
            }
            if g.is_imported() {
                s.imported_groups += 1;
                continue;
            }
            s.new_groups += 1;
            if g.formats() & crate::media::formats::RAW != 0 && g.files.len() > 1 {
                s.raw_pairs += 1;
            }
            for f in g.files.iter().filter(|f| f.existing.is_none()) {
                s.new_files += 1;
                s.new_bytes += f.size;
            }
            if let Some(t) = g.meta.captured_at {
                s.first = Some(s.first.map_or(t, |x| x.min(t)));
                s.last = Some(s.last.map_or(t, |x| x.max(t)));
            }
            *cams.entry(g.meta.camera().unwrap_or_else(|| "알 수 없음".into())).or_default() += 1;
        }
        s.cameras = cams.into_iter().collect();
        s.cameras.sort_by_key(|c| std::cmp::Reverse(c.1));
        s
    }
}

pub fn source_label(source: &Path) -> String {
    source
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| source.display().to_string())
}

fn walk(source: &Path, include_videos: bool, cancel: &AtomicBool) -> (Vec<SourceFile>, Vec<String>) {
    let mut files = Vec::new();
    let mut errors = Vec::new();
    let mut stack = vec![(source.to_path_buf(), 0usize)];
    while let Some((dir, depth)) = stack.pop() {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        let rd = match std::fs::read_dir(&dir) {
            Ok(rd) => rd,
            Err(e) => {
                errors.push(format!("{}: {e}", dir.display()));
                continue;
            }
        };
        for entry in rd.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if is_junk_name(&name) {
                continue;
            }
            let Ok(ft) = entry.file_type() else { continue };
            let path = entry.path();
            if ft.is_dir() {
                let skip = name == DATA_DIR
                    || name.eq_ignore_ascii_case("$RECYCLE.BIN")
                    || name.eq_ignore_ascii_case("System Volume Information");
                if !skip && depth < 16 {
                    stack.push((path, depth + 1));
                }
                continue;
            }
            let Some(kind) = FileKind::from_path(&path) else { continue };
            if kind.is_video() && !include_videos {
                continue;
            }
            let Ok(md) = entry.metadata() else { continue };
            if md.len() == 0 {
                continue;
            }
            let mtime = md.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_secs() as i64);
            files.push(SourceFile { path, name, kind, size: md.len(), mtime, existing: None });
        }
    }
    (files, errors)
}

/// Folder a file is paired in. RAW files kept in a `RAW` sub folder (as this app does with the
/// "RAW sub folder" option, and many photographers do by hand) pair with the JPEGs one level up.
fn pairing_dir(path: &Path, kind: FileKind) -> PathBuf {
    let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
    if kind == FileKind::Raw {
        let is_raw_dir = dir
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| matches!(n.to_ascii_lowercase().as_str(), "raw" | "raws"));
        if is_raw_dir {
            if let Some(up) = dir.parent() {
                return up.to_path_buf();
            }
        }
    }
    dir
}

/// Scan a folder or card: find media, pair RAW+JPEG and read metadata (in parallel).
/// `progress(done, total)` is called while metadata is read.
pub fn scan_source(
    source: &Path,
    include_videos: bool,
    cancel: &AtomicBool,
    progress: &(dyn Fn(usize, usize) + Sync),
) -> Result<ScanResult> {
    if !source.is_dir() {
        bail!("{}은(는) 폴더가 아닙니다", source.display());
    }
    let source = &std::path::absolute(source).unwrap_or_else(|_| source.to_path_buf());
    let (files, errors) = walk(source, include_videos, cancel);
    let mut map: HashMap<(PathBuf, String, bool), Vec<SourceFile>> = HashMap::new();
    for f in files {
        let dir = pairing_dir(&f.path, f.kind);
        let stem = f.path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        map.entry((dir, stem.to_uppercase(), f.kind.is_video())).or_default().push(f);
    }
    let mut groups: Vec<SourceGroup> = map
        .into_values()
        .map(|mut files| {
            files.sort_by_key(|f| (f.kind.preview_priority(), f.name.clone()));
            let stem = files[0].path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            SourceGroup { stem, files, meta: Metadata::default(), previews: Vec::new(), sidecar: None }
        })
        .collect();
    let total = groups.len();
    let done = AtomicUsize::new(0);
    groups.par_iter_mut().for_each(|g| {
        if cancel.load(Ordering::Relaxed) {
            return;
        }
        let primary = g.files[0].clone();
        match metadata::probe(&primary.path, primary.kind) {
            Ok(p) => {
                g.meta = p.meta;
                g.previews = p.previews;
            }
            Err(_) => g.meta.captured_at = metadata::file_time(&primary.path),
        }
        // RAW files hold the most complete EXIF (e.g. lens); fill gaps from them.
        if g.meta.lens.is_none() || g.meta.make.is_none() {
            for f in g.files.iter().skip(1).filter(|f| f.kind == FileKind::Raw) {
                if let Ok(p) = metadata::probe(&f.path, f.kind) {
                    g.meta.merge_missing(&p.meta);
                }
            }
        }
        // Sidecar next to the RAW (Lightroom convention) or next to any other file of the group.
        let mut dirs: Vec<&Path> =
            g.files.iter().filter(|f| f.kind == FileKind::Raw).filter_map(|f| f.path.parent()).collect();
        dirs.extend(g.files.iter().filter_map(|f| f.path.parent()));
        'find: for dir in dirs {
            for ext in ["xmp", "XMP"] {
                let p = dir.join(format!("{}.{ext}", g.stem));
                if p.is_file() {
                    if g.meta.rating.is_none() {
                        g.meta.apply_xmp_rating(std::fs::read(&p).ok().and_then(|b| metadata::xmp_rating(&b)));
                    }
                    g.sidecar = Some(p);
                    break 'find;
                }
            }
        }
        let n = done.fetch_add(1, Ordering::Relaxed) + 1;
        if n.is_multiple_of(16) || n == total {
            progress(n, total);
        }
    });
    if cancel.load(Ordering::Relaxed) {
        bail!("취소됨");
    }
    groups.sort_by(|a, b| a.captured_ms().cmp(&b.captured_ms()).then_with(|| a.stem.cmp(&b.stem)));
    Ok(ScanResult { source: source.to_path_buf(), label: source_label(source), groups, errors })
}

/// Mark files that are already in the catalog.
pub fn mark_existing(scan: &mut ScanResult, catalog: &Catalog) -> Result<()> {
    for g in &mut scan.groups {
        let keys: Vec<String> = g.files.iter().map(|f| g.quick_key(f)).collect();
        for (f, k) in g.files.iter_mut().zip(keys) {
            f.existing = catalog.find_by_quick_key(&k)?;
        }
    }
    Ok(())
}

#[derive(Clone, Debug, Default)]
pub struct ImportProgress {
    pub files_done: usize,
    pub files_total: usize,
    pub bytes_done: u64,
    pub bytes_total: u64,
    pub assets_done: usize,
    pub assets_total: usize,
    pub current: String,
    pub elapsed: f64,
}

impl ImportProgress {
    pub fn fraction(&self) -> f32 {
        if self.bytes_total == 0 {
            if self.files_total == 0 {
                1.0
            } else {
                self.files_done as f32 / self.files_total as f32
            }
        } else {
            (self.bytes_done as f64 / self.bytes_total as f64) as f32
        }
    }
    /// Bytes per second.
    pub fn speed(&self) -> f64 {
        if self.elapsed > 0.2 {
            self.bytes_done as f64 / self.elapsed
        } else {
            0.0
        }
    }
    pub fn eta(&self) -> Option<f64> {
        let s = self.speed();
        (s > 0.0).then(|| (self.bytes_total.saturating_sub(self.bytes_done)) as f64 / s)
    }
}

#[derive(Clone, Debug, Default)]
pub struct ImportSummary {
    pub import_id: i64,
    pub assets_added: usize,
    pub assets_updated: usize,
    pub files_copied: usize,
    pub bytes_copied: u64,
    pub duplicates: usize,
    pub failures: Vec<String>,
    pub cancelled: bool,
    pub elapsed: f64,
}

#[derive(Clone, Debug)]
pub enum ImportEvent {
    Progress(ImportProgress),
    AssetAdded(i64),
    AssetUpdated(i64),
    ThumbReady(i64),
    Warning(String),
    Finished(Result<ImportSummary, String>),
}

pub type EventSender = Sender<ImportEvent>;
pub type EventReceiver = crossbeam_channel::Receiver<ImportEvent>;

pub fn channel() -> (EventSender, EventReceiver) {
    crossbeam_channel::unbounded()
}

struct PlannedFile {
    src: SourceFile,
    dest: PathBuf,
    rel: String,
}

struct PlannedGroup {
    gi: usize,
    files: Vec<PlannedFile>,
    attach_to: Option<i64>,
    name: String,
}

fn plan(
    lib: &Library,
    catalog: &Catalog,
    scan: &ScanResult,
    opts: &ImportOptions,
    in_place: bool,
) -> Result<(Vec<PlannedGroup>, usize)> {
    let mut out = Vec::new();
    let mut duplicates = 0;
    let mut reserved: HashSet<PathBuf> = HashSet::new();
    let mut stems = DirStems::default();
    for (gi, g) in scan.groups.iter().enumerate() {
        let new: Vec<&SourceFile> =
            g.files.iter().filter(|f| f.existing.is_none() && (opts.include_videos || !f.kind.is_video())).collect();
        if new.is_empty() {
            if g.is_imported() {
                duplicates += 1;
            }
            continue;
        }
        let fmt_new = new.iter().fold(0u8, |a, f| a | f.kind.format_flag());
        let mut attach_to = g.files.iter().find_map(|f| f.existing);
        if attach_to.is_none() && !in_place {
            // Other half of a RAW+JPEG pair imported earlier (e.g. dual card slots)?
            attach_to = catalog.find_partner(&g.stem, g.captured_ms(), g.meta.camera().as_deref(), fmt_new)?;
        }
        if in_place {
            let files = new
                .iter()
                .filter_map(|f| {
                    let rel = lib.rel(&f.path)?;
                    Some(PlannedFile { src: (*f).clone(), dest: f.path.clone(), rel })
                })
                .collect::<Vec<_>>();
            if !files.is_empty() {
                out.push(PlannedGroup { gi, files, attach_to, name: g.stem.clone() });
            }
            continue;
        }
        let dir_rel = template::render(&opts.folder_template, &g.meta, None, &g.stem);
        let base = match attach_to {
            Some(id) => catalog.asset_name(id)?.unwrap_or_else(|| g.stem.clone()),
            None => {
                template::sanitize(&template::render(&opts.file_template, &g.meta, None, &g.stem).replace('/', "_"))
            }
        };
        let raw_sub = template::sanitize(opts.raw_subfolder.trim());
        let mut chosen = None;
        for n in 0..100_000u32 {
            let stem = if n == 0 { base.clone() } else { format!("{base}_{n}") };
            let mut files = Vec::with_capacity(new.len());
            let mut ok = true;
            for f in &new {
                let ext = f.path.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_default();
                let mut rel = dir_rel.clone();
                if f.kind == FileKind::Raw && !opts.raw_subfolder.trim().is_empty() {
                    rel = if rel.is_empty() { raw_sub.clone() } else { format!("{rel}/{raw_sub}") };
                }
                let fname = if ext.is_empty() { stem.clone() } else { format!("{stem}.{ext}") };
                let rel = if rel.is_empty() { fname } else { format!("{rel}/{fname}") };
                let dest = lib.abs(&rel);
                let dir = dest.parent().map(Path::to_path_buf).unwrap_or_default();
                // A new asset must not share its name with any other file in the folder, otherwise
                // other applications would treat unrelated files as a RAW+JPEG pair.
                let taken = if attach_to.is_some() {
                    reserved.contains(&dest) || dest.exists()
                } else {
                    stems.taken(&dir, &stem)
                };
                if taken || catalog.path_exists(&rel)? {
                    ok = false;
                    break;
                }
                files.push(PlannedFile { src: (*f).clone(), dest, rel });
            }
            if ok {
                chosen = Some((stem, files));
                break;
            }
        }
        let Some((stem, files)) = chosen else {
            bail!("파일 이름 충돌을 해결할 수 없습니다: {}", g.stem)
        };
        for f in &files {
            reserved.insert(f.dest.clone());
            if let Some(dir) = f.dest.parent() {
                stems.reserve(dir, &stem);
            }
        }
        out.push(PlannedGroup { gi, files, attach_to, name: stem });
    }
    Ok((out, duplicates))
}

/// Upper-case file stems per destination folder (loaded lazily) plus stems reserved by this import.
#[derive(Default)]
struct DirStems {
    dirs: HashMap<PathBuf, HashSet<String>>,
}

impl DirStems {
    fn entry(&mut self, dir: &Path) -> &mut HashSet<String> {
        self.dirs.entry(dir.to_path_buf()).or_insert_with(|| {
            let mut set = HashSet::new();
            if let Ok(rd) = std::fs::read_dir(dir) {
                for e in rd.flatten() {
                    let name = e.file_name().to_string_lossy().into_owned();
                    if name.starts_with('.') {
                        continue;
                    }
                    let stem =
                        Path::new(&name).file_stem().map(|s| s.to_string_lossy().to_uppercase()).unwrap_or_default();
                    set.insert(stem);
                }
            }
            set
        })
    }
    fn taken(&mut self, dir: &Path, stem: &str) -> bool {
        self.entry(dir).contains(&stem.to_uppercase())
    }
    fn reserve(&mut self, dir: &Path, stem: &str) {
        self.entry(dir).insert(stem.to_uppercase());
    }
}

enum Chunk {
    Begin,
    Data(Vec<u8>),
    End,
    Error(String),
}

const CHUNK: usize = 4 << 20;

fn spawn_reader(paths: Vec<PathBuf>, cancel: Arc<AtomicBool>) -> crossbeam_channel::Receiver<Chunk> {
    let (tx, rx) = bounded::<Chunk>(8);
    std::thread::Builder::new()
        .name("import-reader".into())
        .spawn(move || {
            for p in paths {
                if cancel.load(Ordering::Relaxed) {
                    return;
                }
                let mut f = match File::open(&p) {
                    Ok(f) => f,
                    Err(e) => {
                        if tx.send(Chunk::Error(format!("{}: {e}", p.display()))).is_err() {
                            return;
                        }
                        continue;
                    }
                };
                if tx.send(Chunk::Begin).is_err() {
                    return;
                }
                loop {
                    if cancel.load(Ordering::Relaxed) {
                        return;
                    }
                    let mut buf = vec![0u8; CHUNK];
                    match crate::tiff::read_up_to(&mut f, &mut buf) {
                        Ok(0) => {
                            let _ = tx.send(Chunk::End);
                            break;
                        }
                        Ok(n) => {
                            buf.truncate(n);
                            let last = n < CHUNK;
                            if tx.send(Chunk::Data(buf)).is_err() {
                                return;
                            }
                            if last {
                                let _ = tx.send(Chunk::End);
                                break;
                            }
                        }
                        Err(e) => {
                            let _ = tx.send(Chunk::Error(format!("{}: {e}", p.display())));
                            break;
                        }
                    }
                }
            }
        })
        .expect("spawn reader");
    rx
}

fn hash_file(path: &Path, limit: Option<u64>) -> Result<String> {
    let mut f = File::open(path)?;
    let mut h = blake3::Hasher::new();
    let mut buf = vec![0u8; 1 << 20];
    let mut total = 0u64;
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
        total += n as u64;
        if limit.is_some_and(|l| total >= l) {
            break;
        }
    }
    Ok(h.finalize().to_hex().to_string())
}

/// Copy, catalog and thumbnail everything new in `scan`.
pub fn run_import(
    lib: &Library,
    scan: &ScanResult,
    opts: &ImportOptions,
    events: &Sender<ImportEvent>,
    cancel: Arc<AtomicBool>,
) -> Result<ImportSummary> {
    let start = Instant::now();
    let mut catalog = lib.catalog()?;
    let in_place = lib.contains(&scan.source);
    let (planned, duplicates) = plan(lib, &catalog, scan, opts, in_place)?;
    let mut summary = ImportSummary { duplicates, ..Default::default() };
    let import_id = catalog.begin_import(&scan.source.display().to_string(), &scan.label)?;
    summary.import_id = import_id;

    let mut prog = ImportProgress {
        files_total: planned.iter().map(|p| p.files.len()).sum(),
        bytes_total: planned.iter().flat_map(|p| &p.files).map(|f| f.src.size).sum(),
        assets_total: planned.len(),
        ..Default::default()
    };
    let _ = events.send(ImportEvent::Progress(prog.clone()));

    // Thumbnail workers run concurrently with copying.
    let thumbs = Arc::new(Mutex::new(lib.thumbs()?));
    let workers = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).saturating_sub(1).clamp(1, 8);
    let pool = rayon::ThreadPoolBuilder::new().num_threads(workers).thread_name(|i| format!("thumb-{i}")).build()?;
    let (done_tx, done_rx) = std::sync::mpsc::channel::<()>();
    let mut thumb_jobs = 0usize;

    let reader = if in_place {
        None
    } else {
        Some(spawn_reader(
            planned.iter().flat_map(|p| p.files.iter().map(|f| f.src.path.clone())).collect(),
            cancel.clone(),
        ))
    };
    let mut last_event = Instant::now();

    'groups: for pg in &planned {
        let g = &scan.groups[pg.gi];
        let mut new_files: Vec<NewFile> = Vec::new();
        // Files of a group that is interrupted must not stay on disk without a catalog entry.
        let discard = |files: &[NewFile], summary: &mut ImportSummary| {
            if !in_place {
                for f in files {
                    let _ = std::fs::remove_file(lib.abs(&f.rel_path));
                    summary.files_copied = summary.files_copied.saturating_sub(1);
                    summary.bytes_copied = summary.bytes_copied.saturating_sub(f.size);
                }
            }
        };
        for pf in &pg.files {
            if cancel.load(Ordering::Relaxed) {
                summary.cancelled = true;
                discard(&new_files, &mut summary);
                break 'groups;
            }
            prog.current = pf.src.name.clone();
            let quick_key = g.quick_key(&pf.src);
            let hash = if let Some(rx) = &reader {
                match copy_one(rx, pf, opts.verify, &mut prog, &mut last_event, events, &start) {
                    Ok(h) => h,
                    Err(e) => {
                        if cancel.load(Ordering::Relaxed) {
                            summary.cancelled = true;
                            discard(&new_files, &mut summary);
                            break 'groups;
                        }
                        let msg = format!("{}: {e:#}", pf.src.name);
                        let _ = events.send(ImportEvent::Warning(msg.clone()));
                        summary.failures.push(msg);
                        continue;
                    }
                }
            } else {
                // Adopting files already inside the library: a quick partial hash is enough.
                match hash_file(&pf.src.path, Some(1 << 20)) {
                    Ok(h) => {
                        prog.bytes_done += pf.src.size;
                        prog.files_done += 1;
                        format!("q:{}:{h}", pf.src.size)
                    }
                    Err(e) => {
                        summary.failures.push(format!("{}: {e}", pf.src.name));
                        continue;
                    }
                }
            };
            if !in_place {
                summary.files_copied += 1;
                summary.bytes_copied += pf.src.size;
            }
            // Content already in the library under another name?
            if !in_place && catalog.find_by_hash(&hash)?.is_some() {
                let _ = std::fs::remove_file(&pf.dest);
                summary.files_copied -= 1;
                summary.bytes_copied -= pf.src.size;
                continue;
            }
            new_files.push(NewFile {
                kind: pf.src.kind,
                rel_path: pf.rel.clone(),
                orig_name: pf.src.name.clone(),
                orig_path: Some(pf.src.path.display().to_string()),
                size: pf.src.size,
                hash,
                quick_key,
                mtime: pf.src.mtime,
            });
        }
        if new_files.is_empty() {
            summary.duplicates += 1;
            continue;
        }

        // Sidecar: carry over ratings / edits made in other apps.
        if opts.copy_sidecars && !in_place {
            if let Some(sc) = &g.sidecar {
                let anchor = pg.files.iter().find(|f| f.src.kind == FileKind::Raw).or(pg.files.first());
                if let Some(anchor) = anchor {
                    let dest = xmp::sidecar_path(&anchor.dest);
                    if !dest.exists() {
                        let _ = std::fs::copy(sc, &dest);
                    }
                }
            }
        }

        let fmt = new_files.iter().fold(0u8, |a, f| a | f.kind.format_flag());
        let (asset_id, is_new) = match pg.attach_to {
            Some(id) => {
                catalog.attach_files(id, &new_files)?;
                summary.assets_updated += 1;
                (id, false)
            }
            None => {
                let rating = g.meta.rating.unwrap_or(0);
                let auto = tags::auto_tags(&g.meta, fmt);
                let id = catalog.insert_asset(&g.meta, &pg.name, &new_files, Some(import_id), rating, &auto)?;
                summary.assets_added += 1;
                (id, true)
            }
        };
        prog.assets_done += 1;
        let _ =
            events.send(if is_new { ImportEvent::AssetAdded(asset_id) } else { ImportEvent::AssetUpdated(asset_id) });

        // Thumbnail from the best newly written still image.
        let primary_written = pg.files.first().map(|f| f.src.path == g.files[0].path).unwrap_or(false);
        let still = pg.files.iter().filter(|f| !f.src.kind.is_video()).min_by_key(|f| f.src.kind.preview_priority());
        if let Some(pf) = still {
            let need = is_new || pf.src.kind == FileKind::Jpeg;
            if need {
                let path = pf.dest.clone();
                let kind = pf.src.kind;
                let orientation = g.meta.orientation;
                let previews =
                    if primary_written && pf.src.path == g.files[0].path { g.previews.clone() } else { Vec::new() };
                let thumbs = thumbs.clone();
                let ev = events.clone();
                let done = done_tx.clone();
                thumb_jobs += 1;
                pool.spawn(move || {
                    match preview::make_thumbnail(&path, kind, orientation, Some(&previews)) {
                        Ok((w, h, data)) => {
                            if thumbs.lock().unwrap().put(asset_id, w, h, &data).is_ok() {
                                let _ = ev.send(ImportEvent::ThumbReady(asset_id));
                            }
                        }
                        Err(e) => log::debug!("thumbnail {}: {e:#}", path.display()),
                    }
                    let _ = done.send(());
                });
            }
        }
        if last_event.elapsed() > Duration::from_millis(100) {
            prog.elapsed = start.elapsed().as_secs_f64();
            let _ = events.send(ImportEvent::Progress(prog.clone()));
            last_event = Instant::now();
        }
    }
    drop(reader);
    // Wait for thumbnails.
    for _ in 0..thumb_jobs {
        let _ = done_rx.recv();
    }
    prog.elapsed = start.elapsed().as_secs_f64();
    let _ = events.send(ImportEvent::Progress(prog.clone()));
    catalog.finish_import(
        import_id,
        (summary.assets_added + summary.assets_updated) as i64,
        summary.files_copied as i64,
        summary.bytes_copied as i64,
    )?;
    summary.elapsed = start.elapsed().as_secs_f64();
    Ok(summary)
}

/// Receive one file from the reader thread, write it to a temp file, verify, and move it in place.
fn copy_one(
    rx: &crossbeam_channel::Receiver<Chunk>,
    pf: &PlannedFile,
    verify: bool,
    prog: &mut ImportProgress,
    last_event: &mut Instant,
    events: &Sender<ImportEvent>,
    start: &Instant,
) -> Result<String> {
    match rx.recv() {
        Ok(Chunk::Begin) => {}
        Ok(Chunk::Error(e)) => bail!("{e}"),
        Ok(_) => bail!("internal: unexpected chunk"),
        Err(_) => bail!("취소됨"),
    }
    let dir = pf.dest.parent().context("destination has no parent")?;
    let fname = pf.dest.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let tmp = dir.join(format!(".{fname}.part"));
    // Set once the reader has nothing more to send for this file (End, Error or disconnect).
    let mut ended = false;
    let result = (|| -> Result<String> {
        std::fs::create_dir_all(dir).with_context(|| format!("폴더 생성 실패 {}", dir.display()))?;
        let mut out = std::io::BufWriter::with_capacity(
            CHUNK,
            File::create(&tmp).with_context(|| format!("쓰기 실패 {}", tmp.display()))?,
        );
        let mut hasher = blake3::Hasher::new();
        let mut written = 0u64;
        loop {
            match rx.recv() {
                Ok(Chunk::Data(buf)) => {
                    hasher.update(&buf);
                    out.write_all(&buf).context("쓰기 실패 (디스크 공간을 확인하세요)")?;
                    written += buf.len() as u64;
                    prog.bytes_done += buf.len() as u64;
                    if last_event.elapsed() > Duration::from_millis(100) {
                        prog.elapsed = start.elapsed().as_secs_f64();
                        let _ = events.send(ImportEvent::Progress(prog.clone()));
                        *last_event = Instant::now();
                    }
                }
                Ok(Chunk::End) => {
                    ended = true;
                    break;
                }
                Ok(Chunk::Error(e)) => {
                    ended = true;
                    bail!("{e}")
                }
                Ok(Chunk::Begin) => bail!("internal: unexpected begin"),
                Err(_) => {
                    ended = true;
                    bail!("취소됨")
                }
            }
        }
        let file = out.into_inner().map_err(|e| e.into_error())?;
        if let Some(t) = pf.src.mtime {
            let _ = file.set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(t.max(0) as u64));
        }
        if verify {
            file.sync_all().context("동기화 실패")?;
        }
        drop(file);
        let hash = hasher.finalize().to_hex().to_string();
        if written != pf.src.size {
            bail!("크기 불일치: {written} / {}", pf.src.size);
        }
        if verify {
            let check = hash_file(&tmp, None)?;
            if check != hash {
                bail!("검증 실패: 체크섬이 다릅니다");
            }
        }
        if pf.dest.exists() {
            bail!("대상 파일이 이미 존재합니다: {}", pf.dest.display());
        }
        std::fs::rename(&tmp, &pf.dest).with_context(|| format!("이름 변경 실패 {}", pf.dest.display()))?;
        Ok(hash)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
        // The write side failed (e.g. disk full): skip the rest of this file so the next file
        // starts at its own `Begin`.
        while !ended {
            match rx.recv() {
                Ok(Chunk::Data(buf)) => prog.bytes_done += buf.len() as u64,
                Ok(Chunk::Begin) => {}
                Ok(Chunk::End) | Ok(Chunk::Error(_)) | Err(_) => ended = true,
            }
        }
        prog.files_done += 1;
        return result;
    }
    prog.files_done += 1;
    result
}

/// (Re)generate thumbnails for the given assets. Returns the number generated.
pub fn generate_thumbnails(
    lib: &Library,
    ids: &[i64],
    cancel: &AtomicBool,
    on_done: &(dyn Fn(i64, usize) + Sync),
) -> Result<usize> {
    let catalog = Mutex::new(lib.catalog()?);
    let store = Mutex::new(ThumbStore::open(&lib.thumbs_path())?);
    let count = AtomicUsize::new(0);
    ids.par_iter().for_each(|&id| {
        if cancel.load(Ordering::Relaxed) {
            return;
        }
        let (files, orientation) = {
            let c = catalog.lock().unwrap();
            let files = c.files_for(id).unwrap_or_default();
            let o = c.metadata(id).map(|m| m.orientation).unwrap_or(1);
            (files, o)
        };
        let Some(f) = files.iter().filter(|f| !f.kind.is_video()).min_by_key(|f| f.kind.preview_priority()) else {
            return;
        };
        let path = lib.abs(&f.rel_path);
        if let Ok((w, h, data)) = preview::make_thumbnail(&path, f.kind, orientation, None) {
            if store.lock().unwrap().put(id, w, h, &data).is_ok() {
                let n = count.fetch_add(1, Ordering::Relaxed) + 1;
                on_done(id, n);
            }
        }
    });
    Ok(count.load(Ordering::Relaxed))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tiff::tag;
    use crate::tiff::writer::*;

    fn jpeg(model: &'static str, date: &'static str) -> Vec<u8> {
        let tiff = build_tiff(vec![
            (tag::MAKE, Val::Ascii("Canon")),
            (tag::MODEL, Val::Ascii(model)),
            (
                tag::EXIF_IFD,
                Val::Ifd(vec![
                    (tag::DATE_TIME_ORIGINAL, Val::Ascii(date)),
                    (tag::ISO, Val::Short(vec![200])),
                    (tag::LENS_MODEL, Val::Ascii("RF50mm F1.8 STM")),
                ]),
            ),
        ]);
        crate::metadata::tests_support::jpeg_with_exif(&tiff, 640, 480)
    }

    /// A fake TIFF-based RAW with an embedded JPEG preview.
    fn raw(model: &'static str, date: &'static str) -> Vec<u8> {
        let preview = jpeg(model, date);
        let mut t = build_tiff(vec![
            (tag::MAKE, Val::Ascii("Canon")),
            (tag::MODEL, Val::Ascii(model)),
            (tag::JPEG_IF_OFFSET, Val::Long(vec![0])),
            (tag::JPEG_IF_LENGTH, Val::Long(vec![preview.len() as u32])),
            (tag::EXIF_IFD, Val::Ifd(vec![(tag::DATE_TIME_ORIGINAL, Val::Ascii(date))])),
        ]);
        // Patch the JPEG offset (entries are sorted by tag; find 0x0201).
        let off = t.len() as u32;
        let n = u16::from_le_bytes([t[8], t[9]]) as usize;
        for i in 0..n {
            let p = 10 + i * 12;
            if u16::from_le_bytes([t[p], t[p + 1]]) == tag::JPEG_IF_OFFSET {
                t[p + 8..p + 12].copy_from_slice(&off.to_le_bytes());
            }
        }
        t.extend_from_slice(&preview);
        t
    }

    #[test]
    fn end_to_end_import() {
        let card = tempfile::tempdir().unwrap();
        let dcim = card.path().join("DCIM/100CANON");
        std::fs::create_dir_all(&dcim).unwrap();
        std::fs::write(dcim.join("IMG_0001.JPG"), jpeg("Canon EOS R6", "2026:05:01 10:00:00")).unwrap();
        std::fs::write(dcim.join("IMG_0001.CR2"), raw("Canon EOS R6", "2026:05:01 10:00:00")).unwrap();
        std::fs::write(dcim.join("IMG_0002.CR2"), raw("Canon EOS R6", "2026:05:02 11:00:00")).unwrap();
        std::fs::write(dcim.join("._IMG_0002.CR2"), b"junk").unwrap();
        std::fs::write(dcim.join("MVI_0003.MP4"), b"not really a video").unwrap();

        let libdir = tempfile::tempdir().unwrap();
        let lib = Library::open(libdir.path()).unwrap();
        let cancel = Arc::new(AtomicBool::new(false));
        let mut scan = scan_source(card.path(), true, &cancel, &|_, _| {}).unwrap();
        assert_eq!(scan.groups.len(), 3);
        mark_existing(&mut scan, &lib.catalog().unwrap()).unwrap();
        let s = scan.summary(true);
        assert_eq!(s.new_groups, 3);
        assert_eq!(s.raw_pairs, 1);

        let (tx, rx) = crossbeam_channel::unbounded();
        let opts = ImportOptions { raw_subfolder: "RAW".into(), ..Default::default() };
        let sum = run_import(&lib, &scan, &opts, &tx, cancel.clone()).unwrap();
        assert_eq!(sum.assets_added, 3, "{sum:?}");
        assert_eq!(sum.files_copied, 4);
        assert!(sum.failures.is_empty(), "{:?}", sum.failures);
        assert!(lib.root.join("2026/2026-05-01/IMG_0001.JPG").is_file());
        assert!(lib.root.join("2026/2026-05-01/RAW/IMG_0001.CR2").is_file());
        assert!(lib.root.join("2026/2026-05-02/RAW/IMG_0002.CR2").is_file());
        let thumbs_ready = rx.try_iter().filter(|e| matches!(e, ImportEvent::ThumbReady(_))).count();
        assert_eq!(thumbs_ready, 2, "JPEG + RAW preview thumbnails");

        let cat = lib.catalog().unwrap();
        let idx = cat.load_index().unwrap();
        assert_eq!(idx.assets.len(), 3);
        let pair = idx.assets.iter().find(|a| a.name == "IMG_0001").unwrap();
        assert_eq!(pair.formats, crate::media::formats::RAW | crate::media::formats::JPEG);
        assert_eq!(idx.camera_name(pair), "Canon EOS R6");
        assert_eq!(idx.lens_name(pair), "RF50mm F1.8 STM");
        assert!(pair.tags.iter().any(|t| idx.tag_name(*t) == "형식:RAW+JPEG"));

        // Re-importing the same card finds nothing new.
        let mut scan2 = scan_source(card.path(), true, &cancel, &|_, _| {}).unwrap();
        mark_existing(&mut scan2, &cat).unwrap();
        assert_eq!(scan2.summary(true).new_groups, 0);
        let sum2 = run_import(&lib, &scan2, &opts, &tx, cancel.clone()).unwrap();
        assert_eq!(sum2.assets_added, 0);
        assert_eq!(sum2.duplicates, 3);

        // A different card with a colliding file name but different content gets a suffix.
        let card2 = tempfile::tempdir().unwrap();
        std::fs::write(card2.path().join("IMG_0001.JPG"), jpeg("Canon EOS R5", "2026:05:01 12:00:00")).unwrap();
        let mut scan3 = scan_source(card2.path(), true, &cancel, &|_, _| {}).unwrap();
        mark_existing(&mut scan3, &cat).unwrap();
        let sum3 = run_import(&lib, &scan3, &opts, &tx, cancel).unwrap();
        assert_eq!(sum3.assets_added, 1);
        assert!(lib.root.join("2026/2026-05-01/IMG_0001_1.JPG").is_file());
    }

    #[test]
    fn write_failure_does_not_cascade() {
        let card = tempfile::tempdir().unwrap();
        for (i, d) in ["2026:07:01 10:00:00", "2026:07:01 10:00:01", "2026:07:01 10:00:02"].iter().enumerate() {
            std::fs::write(card.path().join(format!("IMG_000{i}.JPG")), jpeg("Canon EOS R6", d)).unwrap();
        }
        let libdir = tempfile::tempdir().unwrap();
        let lib = Library::open(libdir.path()).unwrap();
        // Make the temp file of the middle photo impossible to create.
        let blocker = lib.root.join("2026/2026-07-01/.IMG_0001.JPG.part");
        std::fs::create_dir_all(&blocker).unwrap();
        let cancel = Arc::new(AtomicBool::new(false));
        let mut scan = scan_source(card.path(), true, &cancel, &|_, _| {}).unwrap();
        mark_existing(&mut scan, &lib.catalog().unwrap()).unwrap();
        let (tx, _rx) = crossbeam_channel::unbounded();
        let sum = run_import(&lib, &scan, &ImportOptions::default(), &tx, cancel).unwrap();
        assert_eq!(sum.failures.len(), 1, "{:?}", sum.failures);
        assert_eq!(sum.assets_added, 2);
        assert!(lib.root.join("2026/2026-07-01/IMG_0000.JPG").is_file());
        assert!(lib.root.join("2026/2026-07-01/IMG_0002.JPG").is_file());
    }

    #[test]
    fn cancelled_import_leaves_no_orphans() {
        let card = tempfile::tempdir().unwrap();
        std::fs::write(card.path().join("IMG_0001.JPG"), jpeg("Canon EOS R6", "2026:07:02 10:00:00")).unwrap();
        let libdir = tempfile::tempdir().unwrap();
        let lib = Library::open(libdir.path()).unwrap();
        let cancel = Arc::new(AtomicBool::new(false));
        let scan = scan_source(card.path(), true, &cancel, &|_, _| {}).unwrap();
        cancel.store(true, Ordering::Relaxed);
        let (tx, _rx) = crossbeam_channel::unbounded();
        let sum = run_import(&lib, &scan, &ImportOptions::default(), &tx, cancel).unwrap();
        assert!(sum.cancelled);
        assert_eq!(sum.assets_added, 0);
        assert!(!lib.root.join("2026").exists());
        assert!(lib.catalog().unwrap().load_index().unwrap().assets.is_empty());
    }

    fn import_files(files: &[(&str, Vec<u8>)]) -> (tempfile::TempDir, Library) {
        let card = tempfile::tempdir().unwrap();
        for (name, data) in files {
            std::fs::write(card.path().join(name), data).unwrap();
        }
        let libdir = tempfile::tempdir().unwrap();
        let lib = Library::open(libdir.path()).unwrap();
        let cancel = Arc::new(AtomicBool::new(false));
        let scan = scan_source(card.path(), true, &cancel, &|_, _| {}).unwrap();
        let (tx, _rx) = crossbeam_channel::unbounded();
        run_import(&lib, &scan, &ImportOptions::default(), &tx, cancel).unwrap();
        (libdir, lib)
    }

    fn export_to(lib: &Library, kind: crate::export::ExportKind) -> (tempfile::TempDir, crate::export::ExportSummary) {
        let cat = lib.catalog().unwrap();
        let ids = cat.all_asset_ids().unwrap();
        let out = tempfile::tempdir().unwrap();
        let opts = crate::export::ExportOptions::files(kind);
        let sum =
            crate::export::export(lib, &cat, &ids, out.path(), &opts, &AtomicBool::new(false), &|_, _| {}).unwrap();
        (out, sum)
    }

    fn names_in(dir: &Path) -> Vec<String> {
        let mut v: Vec<String> =
            std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        v.sort();
        v
    }

    #[test]
    fn export_originals_and_jpeg_only() {
        use crate::export::ExportKind;
        let (_d, lib) = import_files(&[
            ("IMG_0001.JPG", jpeg("Canon EOS R6", "2026:08:01 10:00:00")),
            ("IMG_0001.CR2", raw("Canon EOS R6", "2026:08:01 10:00:00")),
            // A RAW-only photo (no camera JPEG): this one used to be exported as the RAW.
            ("IMG_0002.CR2", raw("Canon EOS R6", "2026:08:01 10:05:00")),
        ]);
        let cat = lib.catalog().unwrap();
        let ids = cat.all_asset_ids().unwrap();
        assert_eq!(ids.len(), 2);
        let mut cat = cat;
        cat.set_rating(&ids, 4).unwrap();
        for id in &ids {
            lib.write_xmp(&cat, *id).unwrap();
        }

        let (out, sum) = export_to(&lib, ExportKind::JpegOnly);
        assert_eq!(names_in(out.path()), vec!["IMG_0001.JPG", "IMG_0002.jpg"], "no RAW, no XMP in JPEG-only");
        assert_eq!((sum.files, sum.from_raw), (2, 1));
        let made = crate::metadata::probe(&out.path().join("IMG_0002.jpg"), FileKind::Jpeg).unwrap().meta;
        assert_eq!(made.model.as_deref(), Some("Canon EOS R6"));
        assert_eq!(made.captured_at.unwrap().to_string(), "2026-08-01 10:05:00");

        let (out, sum) = export_to(&lib, ExportKind::Originals);
        assert_eq!(
            names_in(out.path()),
            vec!["IMG_0001.CR2", "IMG_0001.JPG", "IMG_0001.xmp", "IMG_0002.CR2", "IMG_0002.xmp"]
        );
        assert_eq!(sum.files, 5);
    }

    #[test]
    fn export_never_overwrites() {
        use crate::export::ExportKind;
        let (_d, lib) = import_files(&[("IMG_0001.JPG", jpeg("Canon EOS R6", "2026:08:01 10:00:00"))]);
        let cat = lib.catalog().unwrap();
        let ids = cat.all_asset_ids().unwrap();
        let out = tempfile::tempdir().unwrap();
        let never = AtomicBool::new(false);
        let opts = crate::export::ExportOptions::files(ExportKind::JpegOnly);
        for _ in 0..2 {
            crate::export::export(&lib, &cat, &ids, out.path(), &opts, &never, &|_, _| {}).unwrap();
        }
        assert_eq!(names_in(out.path()), vec!["IMG_0001.JPG", "IMG_0001_1.JPG"]);
    }

    #[test]
    fn export_smaller_keeps_exif_without_gps() {
        use crate::export::ExportKind;
        // 1200x800 photo, rotated 90° (orientation 6), with a GPS position.
        let big = image::RgbImage::from_fn(1200, 800, |x, y| image::Rgb([(x % 251) as u8, (y % 241) as u8, 90]));
        let mut plain = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut plain, 90).encode_image(&big).unwrap();
        let tiff = build_tiff(vec![
            (tag::MAKE, Val::Ascii("SONY")),
            (tag::MODEL, Val::Ascii("ILCE-7M4")),
            (tag::ORIENTATION, Val::Short(vec![6])),
            (
                tag::EXIF_IFD,
                Val::Ifd(vec![
                    (tag::DATE_TIME_ORIGINAL, Val::Ascii("2026:10:03 09:15:00")),
                    (tag::ISO, Val::Short(vec![400])),
                    (tag::EXPOSURE_TIME, Val::Rational(vec![(1, 250)])),
                    (tag::F_NUMBER, Val::Rational(vec![(28, 10)])),
                    (tag::FOCAL_LENGTH, Val::Rational(vec![(85, 1)])),
                    (tag::LENS_MODEL, Val::Ascii("FE 85mm F1.8")),
                ]),
            ),
            (
                tag::GPS_IFD,
                Val::Ifd(vec![
                    (tag::GPS_LAT_REF, Val::Ascii("N")),
                    (tag::GPS_LAT, Val::Rational(vec![(37, 1), (33, 1), (0, 1)])),
                    (tag::GPS_LON_REF, Val::Ascii("E")),
                    (tag::GPS_LON, Val::Rational(vec![(126, 1), (58, 1), (0, 1)])),
                ]),
            ),
        ]);
        let mut src = vec![0xFF, 0xD8, 0xFF, 0xE1];
        src.extend_from_slice(&((tiff.len() + 8) as u16).to_be_bytes());
        src.extend_from_slice(b"Exif\0\0");
        src.extend_from_slice(&tiff);
        src.extend_from_slice(&plain[2..]);
        let (_d, lib) = import_files(&[("DSC01234.JPG", src)]);
        let original = lib.catalog().unwrap().metadata(1).unwrap();
        assert!(original.gps.is_some(), "fixture must carry GPS");

        let (out, sum) = export_to(&lib, ExportKind::Resized { long_edge: 600, quality: 85 });
        assert_eq!(sum.files, 1);
        let path = out.path().join("DSC01234.jpg");
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(&bytes[0..4], &[0xFF, 0xD8, 0xFF, 0xE1], "EXIF right after SOI");
        let img = image::load_from_memory(&bytes).unwrap();
        assert_eq!((img.width(), img.height()), (400, 600), "upright (rotated) and long edge 600");
        let m = crate::metadata::probe(&path, FileKind::Jpeg).unwrap().meta;
        assert_eq!(m.orientation, 1);
        assert!(m.gps.is_none(), "location must not be exported");
        assert_eq!(m.model.as_deref(), Some("ILCE-7M4"));
        assert_eq!(m.lens.as_deref(), Some("FE 85mm F1.8"));
        assert_eq!(m.iso, Some(400));
        assert_eq!(m.captured_at.unwrap().to_string(), "2026-10-03 09:15:00");
        assert!((m.exposure_time.unwrap() - 1.0 / 250.0).abs() < 1e-6);
        assert!((m.aperture.unwrap() - 2.8).abs() < 1e-4);
        assert!((m.focal_length.unwrap() - 85.0).abs() < 1e-4);
        assert!(
            bytes.len()
                < std::fs::metadata(lib.abs(&lib.catalog().unwrap().files_for(1).unwrap()[0].rel_path)).unwrap().len()
                    as usize
        );

        // 0 = keep the size, and a long edge above the photo's never upscales. Either the pixels
        // are rotated upright (orientation 1) or, when re-encoding would not have been smaller,
        // the original pixels are kept with their orientation tag — upright when displayed.
        for long_edge in [0, 4000] {
            let (out, _) = export_to(&lib, ExportKind::Resized { long_edge, quality: 90 });
            let path = out.path().join("DSC01234.jpg");
            let img = image::open(&path).unwrap();
            let m = crate::metadata::probe(&path, FileKind::Jpeg).unwrap().meta;
            let shown =
                if matches!(m.orientation, 5..=8) { (img.height(), img.width()) } else { (img.width(), img.height()) };
            assert_eq!(shown, (800, 1200), "long edge {long_edge}");
            assert!(m.gps.is_none());
        }
    }

    #[test]
    fn export_many_as_zip_parts_in_shooting_order() {
        use crate::export::{ExportKind, ExportOptions, ZipOptions};
        // Two cameras that both produced IMG_0001 (on different days, so the library keeps both
        // names), plus a RAW-only shot, imported out of order.
        let card = tempfile::tempdir().unwrap();
        let a = card.path().join("A");
        let b = card.path().join("B");
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        std::fs::write(a.join("IMG_0001.JPG"), jpeg("Canon EOS R6", "2026:08:02 12:00:00")).unwrap();
        std::fs::write(a.join("IMG_0001.CR2"), raw("Canon EOS R6", "2026:08:02 12:00:00")).unwrap();
        std::fs::write(b.join("IMG_0001.JPG"), jpeg("Canon EOS R5", "2026:08:01 09:30:00")).unwrap();
        std::fs::write(b.join("IMG_0002.CR2"), raw("Canon EOS R5", "2026:08:01 10:15:00")).unwrap();
        let libdir = tempfile::tempdir().unwrap();
        let lib = Library::open(libdir.path()).unwrap();
        let cancel = Arc::new(AtomicBool::new(false));
        let scan = scan_source(card.path(), true, &cancel, &|_, _| {}).unwrap();
        let (tx, _rx) = crossbeam_channel::unbounded();
        run_import(&lib, &scan, &ImportOptions::default(), &tx, cancel).unwrap();
        let cat = lib.catalog().unwrap();
        let ids = cat.all_asset_ids().unwrap();
        assert_eq!(ids.len(), 3);
        let never = AtomicBool::new(false);

        // Loose files without date names, in shooting order: the two IMG_0001 must not collide,
        // and the later pair keeps one stem for its RAW and JPEG.
        let out = tempfile::tempdir().unwrap();
        let opts = ExportOptions::files(ExportKind::Originals);
        crate::export::export(&lib, &cat, &ids, out.path(), &opts, &never, &|_, _| {}).unwrap();
        assert_eq!(names_in(out.path()), vec!["IMG_0001.JPG", "IMG_0001_1.CR2", "IMG_0001_1.JPG", "IMG_0002.CR2"]);

        // ZIP split into small parts, names prefixed with the capture time, shooting order.
        let out = tempfile::tempdir().unwrap();
        let opts = ExportOptions {
            kind: ExportKind::JpegOnly,
            date_names: true,
            zip: Some(ZipOptions { name: "여행 사진".into(), part_bytes: 1 }),
            groups: Vec::new(),
        };
        let seen = std::sync::Mutex::new(Vec::new());
        let sum = crate::export::export(&lib, &cat, &ids, out.path(), &opts, &never, &|d, t| {
            seen.lock().unwrap().push((d, t))
        })
        .unwrap_or_else(|e| panic!("{e:#}"));
        assert_eq!((sum.files, sum.from_raw), (3, 1));
        assert_eq!(sum.archives.len(), 3, "a 1-byte limit puts every photo in its own archive");
        assert_eq!(names_in(out.path()), vec!["여행 사진 (1).zip", "여행 사진 (2).zip", "여행 사진 (3).zip"]);
        let entries: Vec<String> = sum
            .archives
            .iter()
            .flat_map(|p| crate::zip::read_stored(&std::fs::read(p).unwrap()).unwrap())
            .map(|(n, data)| {
                assert_eq!(&data[0..2], &[0xFF, 0xD8], "{n} is a JPEG");
                n
            })
            .collect();
        assert_eq!(
            entries,
            vec!["2026-08-01_093000_IMG_0001.JPG", "2026-08-01_101500_IMG_0002.jpg", "2026-08-02_120000_IMG_0001.JPG"]
        );
        assert_eq!(seen.lock().unwrap().len(), 3);

        // Without a limit everything goes into one archive named without a number.
        let out = tempfile::tempdir().unwrap();
        let opts = ExportOptions {
            kind: ExportKind::Resized { long_edge: 320, quality: 80 },
            date_names: false,
            zip: Some(ZipOptions { name: "공유".into(), part_bytes: 0 }),
            groups: Vec::new(),
        };
        let sum = crate::export::export(&lib, &cat, &ids, out.path(), &opts, &never, &|_, _| {}).unwrap();
        assert_eq!(names_in(out.path()), vec!["공유.zip"]);
        let entries = crate::zip::read_stored(&std::fs::read(&sum.archives[0]).unwrap()).unwrap();
        let names: Vec<&str> = entries.iter().map(|e| e.0.as_str()).collect();
        assert_eq!(names, vec!["IMG_0001.jpg", "IMG_0002.jpg", "IMG_0001_1.jpg"]);
        for (n, data) in &entries {
            let img = image::load_from_memory(data).unwrap();
            assert!(img.width().max(img.height()) <= 320, "{n}");
        }
        assert_eq!(sum.bytes, std::fs::metadata(&sum.archives[0]).unwrap().len());
    }

    #[test]
    fn projects_themes_filters_and_export_by_theme() {
        use crate::export::{ExportKind, ExportOptions};
        use crate::filter::{self, Filter, ProjectFilter, RatingFilter};
        let (_d, lib) = import_files(&[
            ("IMG_0001.JPG", jpeg("Canon EOS R6", "2026:08:01 05:30:00")),
            ("IMG_0002.JPG", jpeg("Canon EOS R6", "2026:08:01 06:00:00")),
            ("IMG_0003.JPG", jpeg("Canon EOS R6", "2026:08:01 18:00:00")),
        ]);
        let mut cat = lib.catalog().unwrap();
        let ids = cat.all_asset_ids().unwrap();
        let (a, b, c) = (ids[0], ids[1], ids[2]);
        // A user tag that will become a theme of the project.
        cat.add_tag(&[c], "바다 사진집:빛", crate::catalog::TagKind::User).unwrap();

        let p = cat
            .create_project("바다 사진집", &["새벽".into(), "파도".into(), "새벽".into(), " ".into(), "빛".into()])
            .unwrap();
        assert!(cat.create_project("바다 사진집", &[]).is_err(), "names are unique");
        assert!(cat.create_project("  ", &[]).is_err());
        let idx = cat.load_index().unwrap();
        let themes: Vec<(u32, &str)> = idx.themes_of(p.id).iter().map(|t| (t.id, t.short_name())).collect();
        assert_eq!(themes.iter().map(|t| t.1).collect::<Vec<_>>(), vec!["새벽", "파도", "빛"]);
        let (dawn, wave, light) = (themes[0].0, themes[1].0, themes[2].0);
        assert_eq!(idx.tags[&light].kind, crate::catalog::TagKind::Theme, "existing tag adopted");

        cat.assign_tag(&[a, b], dawn).unwrap();
        cat.assign_tag(&[b], wave).unwrap();
        cat.set_rating(&[a, b, c], 4).unwrap();
        cat.remove_tag(&[c], light).unwrap();
        let idx = cat.load_index().unwrap();
        let ids_of =
            |f: &Filter| -> Vec<i64> { filter::apply(&idx, f).into_iter().map(|i| idx.assets[i].id).collect() };
        let f = Filter { project: Some(ProjectFilter::In(p.id)), ..Default::default() };
        assert_eq!(ids_of(&f), vec![a, b]);
        let todo = Filter {
            project: Some(ProjectFilter::Missing(p.id)),
            rating: RatingFilter::AtLeast(1),
            ..Default::default()
        };
        assert_eq!(ids_of(&todo), vec![c], "rated but not sorted into the project yet");
        let mut by_theme = Filter::default();
        by_theme.tags.insert(wave);
        assert_eq!(ids_of(&by_theme), vec![b]);

        // Lightroom sees "바다 사진집 > 새벽" as a hierarchical keyword.
        lib.write_xmp(&cat, b).unwrap();
        let jpg = lib.abs(&cat.files_for(b).unwrap()[0].rel_path);
        let xmp = std::fs::read_to_string(crate::xmp::sidecar_path(&jpg)).unwrap();
        assert!(
            xmp.contains("<rdf:li>바다 사진집|새벽</rdf:li>") && xmp.contains("<rdf:li>바다 사진집|파도</rdf:li>"),
            "{xmp}"
        );

        // Export one folder per theme (b is in both).
        let groups: Vec<(String, Vec<i64>)> = idx
            .themes_of(p.id)
            .iter()
            .map(|t| {
                let members = idx.assets.iter().filter(|x| x.tags.contains(&t.id)).map(|x| x.id).collect();
                (t.short_name().to_string(), members)
            })
            .filter(|g: &(String, Vec<i64>)| !g.1.is_empty())
            .collect();
        let out = tempfile::tempdir().unwrap();
        let opts = ExportOptions { groups, ..ExportOptions::files(ExportKind::JpegOnly) };
        let sum =
            crate::export::export(&lib, &cat, &[], out.path(), &opts, &AtomicBool::new(false), &|_, _| {}).unwrap();
        assert_eq!(sum.files, 3);
        assert_eq!(names_in(&out.path().join("새벽")), vec!["IMG_0001.JPG", "IMG_0002.JPG"]);
        assert_eq!(names_in(&out.path().join("파도")), vec!["IMG_0002.JPG"]);

        // Rename (':' is reserved), reorder, delete.
        let affected = cat.rename_project(p.id, "바다:여름").unwrap();
        assert_eq!(affected, vec![a, b]);
        let idx = cat.load_index().unwrap();
        assert_eq!(idx.project(p.id).unwrap().name, "바다·여름");
        assert_eq!(idx.tags[&dawn].name, "바다·여름:새벽");
        assert_eq!(cat.rename_theme(wave, "큰 파도").unwrap(), vec![b]);
        assert!(cat.rename_theme(wave, "새벽").is_err(), "theme names are unique per project");
        cat.move_theme(light, -2).unwrap();
        let idx = cat.load_index().unwrap();
        let order: Vec<&str> = idx.themes_of(p.id).iter().map(|t| t.short_name()).collect();
        assert_eq!(order, vec!["빛", "새벽", "큰 파도"]);
        assert_eq!(cat.delete_theme(wave).unwrap(), vec![b]);
        assert_eq!(cat.delete_project(p.id).unwrap(), vec![a, b]);
        let idx = cat.load_index().unwrap();
        assert!(idx.projects.is_empty());
        assert!(idx.tags.values().all(|t| t.project.is_none()));
        assert_eq!(idx.assets.len(), 3, "photos stay");
        assert!(idx.assets.iter().all(|x| x.rating == 4));
    }

    #[test]
    fn export_keep_size_never_grows() {
        use crate::export::ExportKind;
        // A strongly compressed (q50) photo with GPS and orientation 6: re-encoding it at full
        // size and quality 95 would make it bigger, so the original image data is kept and only
        // the metadata is replaced.
        let img = image::RgbImage::from_fn(900, 600, |x, y| {
            image::Rgb([((x * 13 + y * 7) % 256) as u8, ((x ^ y) % 256) as u8, ((x * y) % 253) as u8])
        });
        let mut plain = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut plain, 50).encode_image(&img).unwrap();
        let tiff = build_tiff(vec![
            (tag::MAKE, Val::Ascii("FUJIFILM")),
            (tag::MODEL, Val::Ascii("X-T5")),
            (tag::ORIENTATION, Val::Short(vec![6])),
            (tag::EXIF_IFD, Val::Ifd(vec![(tag::DATE_TIME_ORIGINAL, Val::Ascii("2026:10:04 12:00:00"))])),
            (
                tag::GPS_IFD,
                Val::Ifd(vec![
                    (tag::GPS_LAT_REF, Val::Ascii("N")),
                    (tag::GPS_LAT, Val::Rational(vec![(35, 1), (0, 1), (0, 1)])),
                ]),
            ),
        ]);
        let src = crate::export::with_exif(&plain, &tiff).unwrap();
        let src_len = src.len();
        let (_d, lib) = import_files(&[("DSCF0001.JPG", src)]);
        let (out, sum) = export_to(&lib, ExportKind::Resized { long_edge: 0, quality: 95 });
        assert_eq!(sum.files, 1);
        let path = out.path().join("DSCF0001.jpg");
        let bytes = std::fs::read(&path).unwrap();
        // Same image data; only the metadata block differs by a few bytes (q95 would be ~2x).
        assert!(bytes.len() <= src_len + 512, "export grew: {} > {src_len}", bytes.len());
        let m = crate::metadata::probe(&path, FileKind::Jpeg).unwrap().meta;
        assert!(m.gps.is_none(), "location must not be exported");
        assert_eq!(m.orientation, 6, "pixels untouched, so the orientation tag stays");
        assert_eq!(m.model.as_deref(), Some("X-T5"));
        let decoded = image::load_from_memory(&bytes).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (900, 600));
    }

    #[test]
    fn recatalog_raw_subfolder_library() {
        // A library organised with RAW sub folders, re-catalogued in place (catalog lost):
        // pairs must stay pairs and ratings come back from the XMP sidecars.
        let card = tempfile::tempdir().unwrap();
        std::fs::write(card.path().join("IMG_0001.JPG"), jpeg("Canon EOS R6", "2026:09:01 10:00:00")).unwrap();
        std::fs::write(card.path().join("IMG_0001.CR2"), raw("Canon EOS R6", "2026:09:01 10:00:00")).unwrap();
        let libdir = tempfile::tempdir().unwrap();
        let lib = Library::open(libdir.path()).unwrap();
        let cancel = Arc::new(AtomicBool::new(false));
        let scan = scan_source(card.path(), true, &cancel, &|_, _| {}).unwrap();
        let (tx, _rx) = crossbeam_channel::unbounded();
        let opts = ImportOptions { raw_subfolder: "RAW".into(), ..Default::default() };
        run_import(&lib, &scan, &opts, &tx, cancel.clone()).unwrap();
        let mut cat = lib.catalog().unwrap();
        let id = cat.all_asset_ids().unwrap()[0];
        cat.set_rating(&[id], 4).unwrap();
        lib.write_xmp(&cat, id).unwrap();
        assert!(lib.root.join("2026/2026-09-01/RAW/IMG_0001.xmp").is_file());

        // Lose the catalog, then adopt the folder again.
        drop(cat);
        std::fs::remove_dir_all(lib.data_dir()).unwrap();
        let lib = Library::open(libdir.path()).unwrap();
        let scan = scan_source(&lib.root, true, &cancel, &|_, _| {}).unwrap();
        assert_eq!(scan.groups.len(), 1, "RAW in RAW/ must pair with the JPEG one level up");
        let sum = run_import(&lib, &scan, &opts, &tx, cancel).unwrap();
        assert_eq!(sum.assets_added, 1);
        assert_eq!(sum.files_copied, 0, "in-place: nothing is copied");
        let idx = lib.catalog().unwrap().load_index().unwrap();
        assert_eq!(idx.assets.len(), 1);
        assert_eq!(idx.assets[0].formats, crate::media::formats::RAW | crate::media::formats::JPEG);
        assert_eq!(idx.assets[0].rating, 4);
    }

    #[test]
    fn dual_slot_pairing() {
        // RAW on card 1, JPEG on card 2 → one asset.
        let c1 = tempfile::tempdir().unwrap();
        let c2 = tempfile::tempdir().unwrap();
        std::fs::write(c1.path().join("IMG_0100.CR2"), raw("Canon EOS R5", "2026:06:01 09:00:00")).unwrap();
        std::fs::write(c2.path().join("IMG_0100.JPG"), jpeg("Canon EOS R5", "2026:06:01 09:00:00")).unwrap();
        let libdir = tempfile::tempdir().unwrap();
        let lib = Library::open(libdir.path()).unwrap();
        let cancel = Arc::new(AtomicBool::new(false));
        let (tx, _rx) = crossbeam_channel::unbounded();
        for c in [&c1, &c2] {
            let mut s = scan_source(c.path(), true, &cancel, &|_, _| {}).unwrap();
            mark_existing(&mut s, &lib.catalog().unwrap()).unwrap();
            run_import(&lib, &s, &ImportOptions::default(), &tx, cancel.clone()).unwrap();
        }
        let idx = lib.catalog().unwrap().load_index().unwrap();
        assert_eq!(idx.assets.len(), 1);
        assert_eq!(idx.assets[0].formats, crate::media::formats::RAW | crate::media::formats::JPEG);
    }
}
