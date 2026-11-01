//! Exporting photos out of the library: the original files, only JPEGs, or smaller JPEGs for
//! sharing.
//!
//! Originals are copied byte for byte. "JPEG only" copies the camera JPEG and, for a photo that
//! has no JPEG (RAW only), writes one from the full-size JPEG the camera embeds in the RAW — it
//! never copies RAW files, sidecars or videos. "Smaller" re-encodes a JPEG with the long edge
//! limited and a chosen quality, keeping the shooting information (date, camera, lens, exposure)
//! but not the location, with the orientation baked in.

use crate::catalog::{Catalog, FileRecord};
use crate::library::Library;
use crate::media::FileKind;
use crate::metadata::Metadata;
use crate::preview;
use crate::tiff::{tag, writer};
use anyhow::{bail, Context, Result};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;

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

#[derive(Clone, Debug, Default)]
pub struct ExportSummary {
    /// Files written.
    pub files: usize,
    /// JPEGs made from a RAW (no camera JPEG existed).
    pub from_raw: usize,
    /// Photos left out, with the reason ("동영상", "HEIF" or an error).
    pub skipped: Vec<(String, String)>,
    pub bytes: u64,
    pub cancelled: bool,
}

/// One unit of work.
enum Job {
    Copy { src: PathBuf, dst: PathBuf },
    Render { src: PathBuf, kind: FileKind, meta: Box<Metadata>, edge: u32, quality: u8, dst: PathBuf, from_raw: bool },
}

impl Job {
    fn dst(&self) -> &Path {
        match self {
            Job::Copy { dst, .. } | Job::Render { dst, .. } => dst,
        }
    }
}

/// Export `ids` into `dest`. Existing files are never overwritten (`_1`, `_2`, … are appended).
/// `progress(done, total)` is called after each written file.
pub fn export(
    lib: &Library,
    catalog: &Catalog,
    ids: &[i64],
    dest: &Path,
    kind: ExportKind,
    cancel: &AtomicBool,
    progress: &(dyn Fn(usize, usize) + Sync),
) -> Result<ExportSummary> {
    std::fs::create_dir_all(dest).with_context(|| format!("폴더를 만들 수 없습니다: {}", dest.display()))?;
    let mut sum = ExportSummary::default();
    let mut names = Names::new(dest);
    let mut jobs = Vec::new();
    for &id in ids {
        let files = catalog.files_for(id)?;
        let label = files.first().map(|f| file_name(&f.rel_path)).unwrap_or_else(|| format!("#{id}"));
        match kind {
            ExportKind::Originals => {
                for f in &files {
                    let src = lib.abs(&f.rel_path);
                    let dst = names.take(&file_name(&f.rel_path));
                    jobs.push(Job::Copy { src, dst });
                }
                // Keep ratings / keywords with the RAW.
                if let Some(raw) = files.iter().find(|f| f.kind == FileKind::Raw) {
                    let sc = crate::xmp::sidecar_path(&lib.abs(&raw.rel_path));
                    if sc.exists() {
                        let dst = names.take(&sc.file_name().unwrap_or_default().to_string_lossy());
                        jobs.push(Job::Copy { src: sc, dst });
                    }
                }
            }
            ExportKind::JpegOnly => {
                let jpegs: Vec<&FileRecord> = files.iter().filter(|f| f.kind == FileKind::Jpeg).collect();
                if !jpegs.is_empty() {
                    for f in jpegs {
                        jobs.push(Job::Copy { src: lib.abs(&f.rel_path), dst: names.take(&file_name(&f.rel_path)) });
                    }
                } else {
                    match render_source(&files) {
                        Ok(f) => {
                            let meta = Box::new(catalog.metadata(id).unwrap_or_default());
                            jobs.push(Job::Render {
                                src: lib.abs(&f.rel_path),
                                kind: f.kind,
                                meta,
                                edge: u32::MAX,
                                quality: FROM_RAW_QUALITY,
                                dst: names.take(&jpeg_name(&f.rel_path)),
                                from_raw: f.kind == FileKind::Raw,
                            });
                        }
                        Err(why) => sum.skipped.push((label, why)),
                    }
                }
            }
            ExportKind::Resized { long_edge, quality } => match render_source(&files) {
                Ok(f) => {
                    let meta = Box::new(catalog.metadata(id).unwrap_or_default());
                    jobs.push(Job::Render {
                        src: lib.abs(&f.rel_path),
                        kind: f.kind,
                        meta,
                        edge: if long_edge == 0 { u32::MAX } else { long_edge },
                        quality: quality.clamp(30, 100),
                        dst: names.take(&jpeg_name(&f.rel_path)),
                        from_raw: false,
                    });
                }
                Err(why) => sum.skipped.push((label, why)),
            },
        }
    }

    let total = jobs.len();
    let done = AtomicUsize::new(0);
    let shared = Mutex::new(&mut sum);
    let run = |job: &Job| {
        if cancel.load(Ordering::Relaxed) {
            return;
        }
        let r = match job {
            Job::Copy { src, dst } => copy_file(src, dst),
            Job::Render { src, kind, meta, edge, quality, dst, .. } => render(src, *kind, meta, *edge, *quality, dst),
        };
        let mut s = shared.lock().unwrap();
        match r {
            Ok(bytes) => {
                s.files += 1;
                s.bytes += bytes;
                if matches!(job, Job::Render { from_raw: true, .. }) {
                    s.from_raw += 1;
                }
            }
            Err(e) => s.skipped.push((file_name(&job.dst().to_string_lossy()), format!("{e:#}"))),
        }
        drop(s);
        let n = done.fetch_add(1, Ordering::Relaxed) + 1;
        progress(n, total);
    };
    // Copies are disk bound (one at a time is fastest on external drives); encoding is CPU bound.
    let (copies, renders): (Vec<&Job>, Vec<&Job>) = jobs.iter().partition(|j| matches!(j, Job::Copy { .. }));
    copies.iter().for_each(|j| run(j));
    renders.par_iter().for_each(|j| run(j));
    sum.cancelled = cancel.load(Ordering::Relaxed);
    Ok(sum)
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

/// Unique destination names (existing files and names already handed out in this export).
struct Names<'a> {
    dest: &'a Path,
    taken: HashSet<String>,
}

impl<'a> Names<'a> {
    fn new(dest: &'a Path) -> Self {
        Names { dest, taken: HashSet::new() }
    }

    fn take(&mut self, name: &str) -> PathBuf {
        let p = Path::new(name);
        let stem = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let ext = p.extension().map(|s| format!(".{}", s.to_string_lossy())).unwrap_or_default();
        let mut candidate = name.to_string();
        let mut k = 1;
        while self.taken.contains(&candidate.to_lowercase()) || self.dest.join(&candidate).exists() {
            candidate = format!("{stem}_{k}{ext}");
            k += 1;
        }
        self.taken.insert(candidate.to_lowercase());
        self.dest.join(candidate)
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

fn render(src: &Path, kind: FileKind, meta: &Metadata, edge: u32, quality: u8, dst: &Path) -> Result<u64> {
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
    write_atomic(dst, |tmp| std::fs::write(tmp, &out).context("쓰기 실패"))
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
