//! Exporting photos out of the library: the original files, only JPEGs, or smaller JPEGs for
//! sharing — as loose files or packed into ZIP archives for sending many photos at once.
//!
//! Originals are copied byte for byte. "JPEG only" copies the camera JPEG and, for a photo that
//! has no JPEG (RAW only), writes one from the full-size JPEG the camera embeds in the RAW — it
//! never copies RAW files, sidecars or videos. "Smaller" re-encodes a JPEG with the long edge
//! limited and a chosen quality, keeping the shooting information (date, camera, lens, exposure)
//! but not the location, with the orientation baked in.
//!
//! Photos are exported in shooting order. Optionally every name starts with the capture time
//! ("2026-10-03_091500_IMG_0001.jpg") so a recipient sees them in that order and files from
//! different cameras never collide; RAW, JPEG and sidecar of one photo keep one name.

use crate::catalog::{Catalog, FileRecord};
use crate::library::Library;
use crate::media::FileKind;
use crate::metadata::Metadata;
use crate::preview;
use crate::tiff::{tag, writer};
use crate::zip::ZipWriter;
use anyhow::{bail, Context, Result};
use chrono::NaiveDateTime;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// Quality used when "JPEG only" has to make a JPEG from a RAW's embedded preview.
pub const FROM_RAW_QUALITY: u8 = 95;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExportKind {
    /// Every file of the photo (RAW, JPEG, XMP sidecar).
    Originals,
    /// One JPEG per photo: the camera JPEG, or one made from the RAW's embedded JPEG.
    JpegOnly,
    /// A new JPEG with the long edge at most `long_edge` pixels (0 = keep the size).
    Resized { long_edge: u32, quality: u8 },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ZipOptions {
    /// Archive name without ".zip". Split archives are "name (1).zip", "name (2).zip", …
    pub name: String,
    /// Start a new archive before it would exceed this many bytes (0 = as few as possible).
    /// Each archive opens on its own (no multi-volume ZIP).
    pub part_bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportOptions {
    pub kind: ExportKind,
    /// Prefix names with the capture time: "2026-10-03_091500_IMG_0001.jpg".
    pub date_names: bool,
    /// Pack into ZIP archive(s) instead of writing loose files.
    pub zip: Option<ZipOptions>,
    /// Export into one sub folder per group, e.g. one per theme of a project (a photo in two
    /// themes goes into both). Empty = all `ids` directly into the destination.
    #[serde(default)]
    pub groups: Vec<(String, Vec<i64>)>,
}

impl ExportOptions {
    pub fn files(kind: ExportKind) -> ExportOptions {
        ExportOptions { kind, date_names: false, zip: None, groups: Vec::new() }
    }
}

/// A folder name that is valid on macOS, Windows and Linux.
fn safe_folder(name: &str) -> String {
    let n: String = name
        .chars()
        .map(|c| if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') { '-' } else { c })
        .collect();
    let n = n.trim().trim_matches('.').to_string();
    if n.is_empty() {
        "사진".to_string()
    } else {
        n
    }
}

#[derive(Clone, Debug, Default)]
pub struct ExportSummary {
    /// Files written (inside the archives when zipping).
    pub files: usize,
    /// JPEGs made from a RAW (no camera JPEG existed).
    pub from_raw: usize,
    /// Photos left out, with the reason ("동영상", "HEIF" or an error).
    pub skipped: Vec<(String, String)>,
    /// Bytes of the exported files (of the archives when zipping).
    pub bytes: u64,
    /// ZIP archives written.
    pub archives: Vec<PathBuf>,
    pub cancelled: bool,
}

/// What an export of `ids` would contain, for the dialog (cheap: catalog only).
#[derive(Clone, Debug, Default)]
pub struct ExportPlanInfo {
    pub photos: usize,
    pub originals_bytes: u64,
    /// Bytes of the camera JPEGs (photos with a JPEG).
    pub jpeg_bytes: u64,
    /// Photos without a camera JPEG that have a RAW (a JPEG is made from it).
    pub raw_only: usize,
    pub videos: usize,
    pub heif_only: usize,
    pub first: Option<NaiveDateTime>,
    pub last: Option<NaiveDateTime>,
}

pub fn plan_info(catalog: &Catalog, ids: &[i64]) -> Result<ExportPlanInfo> {
    let mut i = ExportPlanInfo { photos: ids.len(), ..Default::default() };
    for &id in ids {
        let files = catalog.files_for(id)?;
        i.originals_bytes += files.iter().map(|f| f.size).sum::<u64>();
        let jpeg: u64 = files.iter().filter(|f| f.kind == FileKind::Jpeg).map(|f| f.size).sum();
        i.jpeg_bytes += jpeg;
        let has = |k| files.iter().any(|f| f.kind == k);
        if jpeg == 0 && has(FileKind::Raw) {
            i.raw_only += 1;
        }
        if has(FileKind::Video) && !has(FileKind::Jpeg) && !has(FileKind::Raw) {
            i.videos += 1;
        }
        if has(FileKind::Heif) && !has(FileKind::Jpeg) && !has(FileKind::Raw) {
            i.heif_only += 1;
        }
        if let Some(t) = catalog.metadata(id).ok().and_then(|m| m.captured_at) {
            i.first = Some(i.first.map_or(t, |f| f.min(t)));
            i.last = Some(i.last.map_or(t, |l| l.max(t)));
        }
    }
    Ok(i)
}

enum Source {
    Copy(PathBuf),
    Render { src: PathBuf, kind: FileKind, meta: Box<Metadata>, edge: u32, quality: u8, from_raw: bool },
}

struct Job {
    /// File name in the destination folder / archive.
    name: String,
    source: Source,
    modified: Option<NaiveDateTime>,
}

/// Export `ids` into `dest` (a folder; archives are created inside it). Existing files are never
/// overwritten (`_1`, `_2`, … are appended). `progress(done, total)` follows the written files.
pub fn export(
    lib: &Library,
    catalog: &Catalog,
    ids: &[i64],
    dest: &Path,
    opts: &ExportOptions,
    cancel: &AtomicBool,
    progress: &(dyn Fn(usize, usize) + Sync),
) -> Result<ExportSummary> {
    std::fs::create_dir_all(dest).with_context(|| format!("폴더를 만들 수 없습니다: {}", dest.display()))?;
    let mut sum = ExportSummary::default();

    // One folder per group (e.g. per theme of a project), or everything in `dest`.
    let groups: Vec<(String, Vec<i64>)> = if opts.groups.is_empty() {
        vec![(String::new(), ids.to_vec())]
    } else {
        opts.groups.iter().map(|(f, ids)| (safe_folder(f), ids.clone())).collect()
    };
    let mut jobs = Vec::new();
    for (folder, ids) in &groups {
        let base = if folder.is_empty() { dest.to_path_buf() } else { dest.join(folder) };
        if opts.zip.is_none() {
            std::fs::create_dir_all(&base).with_context(|| format!("폴더를 만들 수 없습니다: {}", base.display()))?;
        }
        let sub = if folder.is_empty() { String::new() } else { format!("{folder}/") };
        // Shooting order.
        let mut photos: Vec<(Option<NaiveDateTime>, i64, Metadata)> = ids
            .iter()
            .map(|&id| (catalog.metadata(id).ok(), id))
            .map(|(m, id)| {
                let m = m.unwrap_or_default();
                (m.captured_at, id, m)
            })
            .collect();
        photos.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
        let mut names = Names { dest: if opts.zip.is_some() { None } else { Some(&base) }, taken: HashSet::new() };
        for (when, id, meta) in photos {
            let files = catalog.files_for(id)?;
            let label = files.first().map(|f| file_name(&f.rel_path)).unwrap_or_else(|| format!("#{id}"));
            let prefix = match (opts.date_names, when) {
                (true, Some(t)) => t.format("%Y-%m-%d_%H%M%S_").to_string(),
                _ => String::new(),
            };
            let render = |f: &FileRecord, edge: u32, quality: u8, from_raw: bool| Source::Render {
                src: lib.abs(&f.rel_path),
                kind: f.kind,
                meta: Box::new(meta.clone()),
                edge,
                quality,
                from_raw,
            };
            match opts.kind {
                ExportKind::Originals => {
                    let mut items: Vec<(String, PathBuf)> =
                        files.iter().map(|f| (file_name(&f.rel_path), lib.abs(&f.rel_path))).collect();
                    // Keep ratings / keywords with the RAW.
                    if let Some(raw) = files.iter().find(|f| f.kind == FileKind::Raw) {
                        let sc = crate::xmp::sidecar_path(&lib.abs(&raw.rel_path));
                        if sc.exists() {
                            items.push((sc.file_name().unwrap_or_default().to_string_lossy().into_owned(), sc));
                        }
                    }
                    let given: Vec<String> = items.iter().map(|(n, _)| format!("{prefix}{n}")).collect();
                    for ((_, src), name) in items.into_iter().zip(names.take_group(&given)) {
                        jobs.push(Job { name: format!("{sub}{name}"), source: Source::Copy(src), modified: when });
                    }
                }
                ExportKind::JpegOnly => {
                    let jpegs: Vec<&FileRecord> = files.iter().filter(|f| f.kind == FileKind::Jpeg).collect();
                    if !jpegs.is_empty() {
                        let given: Vec<String> =
                            jpegs.iter().map(|f| format!("{prefix}{}", file_name(&f.rel_path))).collect();
                        for (f, name) in jpegs.into_iter().zip(names.take_group(&given)) {
                            jobs.push(Job {
                                name: format!("{sub}{name}"),
                                source: Source::Copy(lib.abs(&f.rel_path)),
                                modified: when,
                            });
                        }
                    } else {
                        match render_source(&files) {
                            Ok(f) => {
                                let name = names.take_group(&[format!("{prefix}{}", jpeg_name(&f.rel_path))]).remove(0);
                                let source = render(f, u32::MAX, FROM_RAW_QUALITY, f.kind == FileKind::Raw);
                                jobs.push(Job { name: format!("{sub}{name}"), source, modified: when });
                            }
                            Err(why) => sum.skipped.push((label, why)),
                        }
                    }
                }
                ExportKind::Resized { long_edge, quality } => match render_source(&files) {
                    Ok(f) => {
                        let name = names.take_group(&[format!("{prefix}{}", jpeg_name(&f.rel_path))]).remove(0);
                        let edge = if long_edge == 0 { u32::MAX } else { long_edge };
                        jobs.push(Job {
                            name: format!("{sub}{name}"),
                            source: render(f, edge, quality.clamp(30, 100), false),
                            modified: when,
                        });
                    }
                    Err(why) => sum.skipped.push((label, why)),
                },
            }
        }
    }

    // Full-size images need a lot of memory each: fewer at a time.
    let full_size = jobs.iter().any(|j| matches!(&j.source, Source::Render { edge, .. } if *edge > 4000));
    let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
    let threads = if full_size { 2.min(cores) } else { cores.min(8) };
    let pool = rayon::ThreadPoolBuilder::new().num_threads(threads).build().context("thread pool")?;

    let total = jobs.len();
    let done = AtomicUsize::new(0);
    let tick = || progress(done.fetch_add(1, Ordering::Relaxed) + 1, total);
    match &opts.zip {
        None => {
            // Copies are disk bound (one at a time is fastest on external drives); encoding is CPU
            // bound.
            let results: Vec<(usize, Result<u64>)> = {
                let mut r = Vec::with_capacity(total);
                for (i, j) in jobs.iter().enumerate() {
                    if let Source::Copy(src) = &j.source {
                        if cancel.load(Ordering::Relaxed) {
                            break;
                        }
                        r.push((i, copy_file(src, &dest.join(&j.name))));
                        tick();
                    }
                }
                let renders: Vec<(usize, &Job)> =
                    jobs.iter().enumerate().filter(|(_, j)| matches!(j.source, Source::Render { .. })).collect();
                r.extend(pool.install(|| {
                    renders
                        .par_iter()
                        .filter_map(|(i, j)| {
                            if cancel.load(Ordering::Relaxed) {
                                return None;
                            }
                            let res = produce(&j.source).and_then(|bytes| {
                                write_atomic(&dest.join(&j.name), |tmp| Ok(std::fs::write(tmp, &bytes)?))
                            });
                            tick();
                            Some((*i, res))
                        })
                        .collect::<Vec<_>>()
                }));
                r
            };
            for (i, res) in results {
                tally(&mut sum, &jobs[i], res);
            }
        }
        Some(z) => {
            let mut parts = ZipParts::new(
                dest,
                &z.name,
                if z.part_bytes == 0 { crate::zip::MAX_BYTES } else { z.part_bytes.min(crate::zip::MAX_BYTES) },
            )?;
            // Produce a few entries in parallel, then append them in order (bounded memory).
            for chunk in jobs.chunks(threads * 2) {
                if cancel.load(Ordering::Relaxed) {
                    break;
                }
                let made: Vec<Result<Vec<u8>>> =
                    pool.install(|| chunk.par_iter().map(|j| produce(&j.source)).collect());
                for (j, bytes) in chunk.iter().zip(made) {
                    let res = bytes.and_then(|b| {
                        parts.add(&j.name, &b, j.modified)?;
                        Ok(b.len() as u64)
                    });
                    tally(&mut sum, j, res);
                    tick();
                }
            }
            if cancel.load(Ordering::Relaxed) {
                parts.abandon();
            } else {
                sum.archives = parts.finish()?;
                sum.bytes = sum.archives.iter().filter_map(|p| std::fs::metadata(p).ok()).map(|m| m.len()).sum();
            }
        }
    }
    sum.cancelled = cancel.load(Ordering::Relaxed);
    Ok(sum)
}

fn tally(sum: &mut ExportSummary, job: &Job, res: Result<u64>) {
    match res {
        Ok(bytes) => {
            sum.files += 1;
            sum.bytes += bytes;
            if matches!(job.source, Source::Render { from_raw: true, .. }) {
                sum.from_raw += 1;
            }
        }
        Err(e) => sum.skipped.push((job.name.clone(), format!("{e:#}"))),
    }
}

/// Contents of one exported file.
fn produce(source: &Source) -> Result<Vec<u8>> {
    match source {
        Source::Copy(src) => std::fs::read(src).with_context(|| format!("읽을 수 없습니다 {}", src.display())),
        Source::Render { src, kind, meta, edge, quality, .. } => render(src, *kind, meta, *edge, *quality),
    }
}

/// Archives "name.zip" or "name (1).zip", "name (2).zip", … filled in order.
struct ZipParts {
    dest: PathBuf,
    base: String,
    limit: u64,
    current: Option<ZipWriter>,
    done: Vec<PathBuf>,
}

impl ZipParts {
    fn new(dest: &Path, name: &str, limit: u64) -> Result<ZipParts> {
        let clean: String = name.chars().map(|c| if matches!(c, '/' | '\\' | ':') { '-' } else { c }).collect();
        let clean = if clean.trim().is_empty() { "사진".to_string() } else { clean.trim().to_string() };
        // A base whose archives do not exist yet.
        let mut base = clean.clone();
        let mut k = 2;
        while dest.join(format!("{base}.zip")).exists() || dest.join(format!("{base} (1).zip")).exists() {
            base = format!("{clean} {k}");
            k += 1;
        }
        Ok(ZipParts { dest: dest.to_path_buf(), base, limit, current: None, done: Vec::new() })
    }

    fn add(&mut self, name: &str, data: &[u8], modified: Option<NaiveDateTime>) -> Result<()> {
        let cost = ZipWriter::entry_cost(name, data.len() as u64);
        if let Some(z) = &self.current {
            if !z.is_empty() && z.size() + cost > self.limit {
                let z = self.current.take().unwrap();
                self.done.push(z.finish()?);
            }
        }
        if self.current.is_none() {
            let n = self.done.len() + 1;
            self.current = Some(ZipWriter::create(&self.dest.join(format!("{} ({n}).zip", self.base)))?);
        }
        self.current.as_mut().unwrap().add(name, data, modified)
    }

    fn finish(mut self) -> Result<Vec<PathBuf>> {
        if let Some(z) = self.current.take() {
            self.done.push(z.finish()?);
        }
        // A single archive does not need a number.
        if self.done.len() == 1 {
            let plain = self.dest.join(format!("{}.zip", self.base));
            std::fs::rename(&self.done[0], &plain)?;
            self.done[0] = plain;
        }
        Ok(self.done)
    }

    fn abandon(mut self) {
        if let Some(z) = self.current.take() {
            z.abandon();
        }
        for p in &self.done {
            let _ = std::fs::remove_file(p);
        }
    }
}

/// The file a new JPEG is made from: the camera JPEG, else the RAW (its embedded JPEG), else a
/// PNG / TIFF. HEIF and videos cannot be converted.
fn render_source(files: &[FileRecord]) -> std::result::Result<&FileRecord, String> {
    for k in [FileKind::Jpeg, FileKind::Raw, FileKind::Tiff, FileKind::Png] {
        if let Some(f) = files.iter().find(|f| f.kind == k) {
            return Ok(f);
        }
    }
    if files.iter().any(|f| f.kind == FileKind::Heif) {
        return Err("HEIF 는 JPEG 로 바꿀 수 없습니다".into());
    }
    if files.iter().any(|f| f.kind == FileKind::Video) {
        return Err("동영상".into());
    }
    Err("파일 없음".into())
}

fn file_name(rel: &str) -> String {
    Path::new(rel).file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| rel.to_string())
}

fn jpeg_name(rel: &str) -> String {
    let stem = Path::new(rel).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    format!("{stem}.jpg")
}

fn split_name(name: &str) -> (String, String) {
    let p = Path::new(name);
    let stem = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let ext = p.extension().map(|s| format!(".{}", s.to_string_lossy())).unwrap_or_default();
    (stem, ext)
}

/// Unique destination names: not taken earlier in this export and (for loose files) not present
/// in the folder.
struct Names<'a> {
    dest: Option<&'a Path>,
    taken: HashSet<String>,
}

impl Names<'_> {
    fn free(&self, name: &str) -> bool {
        !self.taken.contains(&name.to_lowercase()) && self.dest.is_none_or(|d| !d.join(name).exists())
    }

    /// Names for the files of one photo. Files sharing a stem (IMG_0001.CR3 / .JPG / .xmp) get
    /// the same suffix so they stay together.
    fn take_group(&mut self, given: &[String]) -> Vec<String> {
        let parts: Vec<(String, String)> = given.iter().map(|n| split_name(n)).collect();
        let mut out = Vec::with_capacity(given.len());
        let mut by_stem: Vec<(String, String)> = Vec::new(); // original stem -> chosen stem
        for (stem, ext) in &parts {
            let chosen = if let Some((_, c)) = by_stem.iter().find(|(s, _)| s == stem) {
                c.clone()
            } else {
                let exts: Vec<&String> = parts.iter().filter(|(s, _)| s == stem).map(|(_, e)| e).collect();
                let mut cand = stem.clone();
                let mut k = 1;
                while !exts.iter().all(|e| self.free(&format!("{cand}{e}"))) {
                    cand = format!("{stem}_{k}");
                    k += 1;
                }
                by_stem.push((stem.clone(), cand.clone()));
                cand
            };
            let name = format!("{chosen}{ext}");
            self.taken.insert(name.to_lowercase());
            out.push(name);
        }
        out
    }
}

/// Write via a temporary name so an interrupted export never leaves a half file behind.
fn write_atomic(dst: &Path, write: impl FnOnce(&Path) -> Result<()>) -> Result<u64> {
    let tmp = dst.with_file_name(format!(".{}.part", dst.file_name().unwrap_or_default().to_string_lossy()));
    let r = write(&tmp).and_then(|_| std::fs::rename(&tmp, dst).context("rename"));
    if r.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    r?;
    Ok(std::fs::metadata(dst).map(|m| m.len()).unwrap_or(0))
}

fn copy_file(src: &Path, dst: &Path) -> Result<u64> {
    write_atomic(dst, |tmp| {
        std::fs::copy(src, tmp).with_context(|| format!("복사 실패 {}", src.display()))?;
        Ok(())
    })
}

fn render(src: &Path, kind: FileKind, meta: &Metadata, edge: u32, quality: u8) -> Result<Vec<u8>> {
    let img = preview::load_image(src, kind, meta.orientation, edge, None)
        .with_context(|| format!("읽을 수 없습니다 {}", src.display()))?;
    let jpeg = preview::encode_jpeg(&img, quality)?;
    let mut out = with_exif(&jpeg, &exif_block(meta, Some((img.width(), img.height())), 1))?;
    // Not downscaled and not smaller than the camera JPEG (it was already compressed harder):
    // keep its image data as is and only replace the metadata, so the export never grows.
    let downscaled = edge != u32::MAX && img.width().max(img.height()) >= edge;
    if kind == FileKind::Jpeg && !downscaled {
        let original = std::fs::read(src).with_context(|| format!("읽을 수 없습니다 {}", src.display()))?;
        if out.len() >= original.len() {
            if let Ok(b) = rewrap(&original, &exif_block(meta, None, meta.orientation.max(1))) {
                out = b;
            }
        }
    }
    Ok(out)
}

/// The original JPEG with its metadata segments (EXIF, XMP, IPTC — any of which may hold the
/// location) replaced by `tiff`. Image data, ICC profile and MPF previews are kept as they are.
pub fn rewrap(jpeg: &[u8], tiff: &[u8]) -> Result<Vec<u8>> {
    if jpeg.len() < 4 || jpeg[0..2] != [0xFF, 0xD8] {
        bail!("not a JPEG");
    }
    let mut kept: Vec<&[u8]> = Vec::new();
    let mut pos = 2;
    loop {
        if pos + 4 > jpeg.len() || jpeg[pos] != 0xFF {
            bail!("unexpected JPEG structure");
        }
        let marker = jpeg[pos + 1];
        if marker == 0xFF {
            pos += 1;
            continue;
        }
        // Image data starts: keep everything from here on.
        if !(0xE0..=0xEF).contains(&marker) && marker != 0xFE {
            break;
        }
        let len = u16::from_be_bytes([jpeg[pos + 2], jpeg[pos + 3]]) as usize;
        if pos + 2 + len > jpeg.len() {
            bail!("truncated JPEG segment");
        }
        // APP1 = EXIF / XMP, APP13 = IPTC, COM = comments.
        if !matches!(marker, 0xE1 | 0xED | 0xFE) {
            kept.push(&jpeg[pos..pos + 2 + len]);
        }
        pos += 2 + len;
    }
    let mut out = with_exif(&[0xFF, 0xD8, 0xFF, 0xD9], tiff)?;
    out.truncate(out.len() - 2); // drop the placeholder EOI
    for seg in kept {
        out.extend_from_slice(seg);
    }
    out.extend_from_slice(&jpeg[pos..]);
    Ok(out)
}

/// EXIF for an exported JPEG: shooting information but no GPS. `orientation` is 1 when the pixels
/// were rotated upright, otherwise the original value.
pub fn exif_block(m: &Metadata, dims: Option<(u32, u32)>, orientation: u8) -> Vec<u8> {
    use writer::Val;
    const EXIF_VERSION: u16 = 0x9000;
    let rational = |v: f32, den: u32| Val::Rational(vec![((v as f64 * den as f64).round().max(0.0) as u32, den)]);
    let date = m.captured_at.map(|d| d.format("%Y:%m:%d %H:%M:%S").to_string());

    let mut ifd0: Vec<(u16, Val)> = vec![
        (tag::ORIENTATION, Val::Short(vec![orientation as u16])),
        (tag::SOFTWARE, Val::Text(format!("Photo Organize {}", env!("CARGO_PKG_VERSION")))),
    ];
    if let Some(v) = &m.make {
        ifd0.push((tag::MAKE, Val::Text(v.clone())));
    }
    if let Some(v) = &m.model {
        ifd0.push((tag::MODEL, Val::Text(v.clone())));
    }
    if let Some(v) = &m.artist {
        ifd0.push((tag::ARTIST, Val::Text(v.clone())));
    }
    if let Some(d) = &date {
        ifd0.push((tag::DATE_TIME, Val::Text(d.clone())));
    }

    let mut exif: Vec<(u16, Val)> = vec![(EXIF_VERSION, Val::Undefined(b"0232".to_vec()))];
    if let Some((w, h)) = dims {
        exif.push((tag::PIXEL_X, Val::Long(vec![w])));
        exif.push((tag::PIXEL_Y, Val::Long(vec![h])));
    }
    if let Some(d) = date {
        exif.push((tag::DATE_TIME_ORIGINAL, Val::Text(d)));
    }
    if let Some(o) = &m.offset {
        exif.push((tag::OFFSET_TIME_ORIGINAL, Val::Text(o.clone())));
    }
    if let Some(t) = m.exposure_time.filter(|t| *t > 0.0) {
        // 1/250 s -> 1/250, 2.5 s -> 25/10.
        if t < 1.0 {
            exif.push((tag::EXPOSURE_TIME, Val::Rational(vec![(1, (1.0 / t as f64).round().max(1.0) as u32)])));
        } else {
            exif.push((tag::EXPOSURE_TIME, rational(t, 10)));
        }
    }
    if let Some(f) = m.aperture.filter(|f| *f > 0.0) {
        exif.push((tag::F_NUMBER, rational(f, 10)));
    }
    if let Some(iso) = m.iso {
        exif.push((tag::ISO, Val::Short(vec![iso.min(u16::MAX as u32) as u16])));
    }
    if let Some(b) = m.exposure_bias {
        exif.push((tag::EXPOSURE_BIAS, Val::SRational(vec![((b * 3.0).round() as i32, 3)])));
    }
    if let Some(f) = m.focal_length.filter(|f| *f > 0.0) {
        exif.push((tag::FOCAL_LENGTH, rational(f, 10)));
    }
    if let Some(f) = m.focal_35mm.filter(|f| *f > 0.0) {
        exif.push((tag::FOCAL_35, Val::Short(vec![f.round().min(u16::MAX as f32) as u16])));
    }
    if let Some(v) = &m.lens_make {
        exif.push((tag::LENS_MAKE, Val::Text(v.clone())));
    }
    if let Some(v) = &m.lens {
        exif.push((tag::LENS_MODEL, Val::Text(v.clone())));
    }
    ifd0.push((tag::EXIF_IFD, Val::Ifd(exif)));
    writer::build_tiff(ifd0)
}

/// Put an APP1 Exif segment right after SOI (dropping the encoder's JFIF APP0).
pub fn with_exif(jpeg: &[u8], tiff: &[u8]) -> Result<Vec<u8>> {
    if jpeg.len() < 4 || jpeg[0..2] != [0xFF, 0xD8] {
        bail!("not a JPEG");
    }
    let seg_len = tiff.len() + 6 + 2;
    if seg_len > u16::MAX as usize {
        bail!("EXIF too large");
    }
    let mut rest = &jpeg[2..];
    if rest.len() >= 4 && rest[0] == 0xFF && rest[1] == 0xE0 {
        let l = u16::from_be_bytes([rest[2], rest[3]]) as usize;
        if l + 2 <= rest.len() {
            rest = &rest[2 + l..];
        }
    }
    let mut out = Vec::with_capacity(jpeg.len() + seg_len + 2);
    out.extend_from_slice(&[0xFF, 0xD8, 0xFF, 0xE1]);
    out.extend_from_slice(&(seg_len as u16).to_be_bytes());
    out.extend_from_slice(b"Exif\0\0");
    out.extend_from_slice(tiff);
    out.extend_from_slice(rest);
    Ok(out)
}

/// Rough output size per photo for the export dialog (typical photos, 3:2).
pub fn estimate_bytes(long_edge: u32, quality: u8) -> u64 {
    let short = long_edge as f64 * 2.0 / 3.0;
    let pixels = long_edge as f64 * short;
    // Bytes per pixel of typical photographs at a given JPEG quality.
    let bpp = match quality {
        0..=75 => 0.16,
        76..=82 => 0.2,
        83..=90 => 0.28,
        91..=95 => 0.45,
        _ => 0.7,
    };
    (pixels * bpp) as u64
}
