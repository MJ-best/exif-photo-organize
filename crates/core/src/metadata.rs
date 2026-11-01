//! Metadata extraction (EXIF) and discovery of embedded JPEG previews.
//!
//! Supported containers:
//! * JPEG (APP1 Exif + XMP)
//! * TIFF based RAW: CR2, NEF, NRW, ARW, SR2, DNG, ORF, RW2, PEF, SRW, 3FR, IIQ, ERF, ...
//! * Canon CR3 (ISO-BMFF with CMT1..4 boxes)
//! * Fujifilm RAF (embedded JPEG)
//! * HEIF / HEIC, PNG (via `kamadak-exif`)

use crate::media::FileKind;
use crate::tiff::{read_up_to, tag, Ifd, TiffReader};
use anyhow::{bail, Context, Result};
use chrono::{DateTime, Local, NaiveDateTime};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{BufReader, Cursor, Read, Seek, SeekFrom};
use std::path::Path;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum DateSource {
    Exif,
    #[default]
    FileTime,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Gps {
    pub lat: f64,
    pub lon: f64,
    pub alt: Option<f64>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Metadata {
    /// Local capture time as recorded by the camera (no time zone).
    pub captured_at: Option<NaiveDateTime>,
    pub date_source: DateSource,
    /// e.g. "+09:00"
    pub offset: Option<String>,
    pub make: Option<String>,
    pub model: Option<String>,
    pub lens: Option<String>,
    pub lens_make: Option<String>,
    pub focal_length: Option<f32>,
    pub focal_35mm: Option<f32>,
    pub aperture: Option<f32>,
    /// Seconds.
    pub exposure_time: Option<f32>,
    pub iso: Option<u32>,
    pub exposure_bias: Option<f32>,
    pub flash_fired: Option<bool>,
    pub exposure_program: Option<u16>,
    pub metering_mode: Option<u16>,
    pub white_balance: Option<u16>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    /// EXIF orientation 1..=8.
    pub orientation: u8,
    pub gps: Option<Gps>,
    pub serial: Option<String>,
    pub software: Option<String>,
    pub artist: Option<String>,
    /// Rating set in camera (EXIF 0x4746 or XMP), 0..=5.
    pub rating: Option<u8>,
    /// Marked as rejected (XMP rating -1, Lightroom convention).
    #[serde(default)]
    pub reject: bool,
}

impl Metadata {
    /// Human friendly camera name, e.g. "Nikon Z 6", "Sony ILCE-7M3".
    pub fn camera(&self) -> Option<String> {
        camera_display(self.make.as_deref(), self.model.as_deref())
    }

    /// Fill in fields that are missing in `self` from `other`.
    pub fn merge_missing(&mut self, other: &Metadata) {
        macro_rules! fill {
            ($($f:ident),*) => { $( if self.$f.is_none() { self.$f = other.$f.clone(); } )* };
        }
        if self.captured_at.is_none() && other.captured_at.is_some() {
            self.captured_at = other.captured_at;
            self.date_source = other.date_source;
        }
        fill!(
            offset,
            make,
            model,
            lens,
            lens_make,
            focal_length,
            focal_35mm,
            aperture,
            exposure_time,
            iso,
            exposure_bias,
            flash_fired,
            exposure_program,
            metering_mode,
            white_balance,
            width,
            height,
            gps,
            serial,
            software,
            artist,
            rating
        );
        self.reject |= other.reject;
        if self.orientation <= 1 && other.orientation > 1 {
            self.orientation = other.orientation;
        }
    }

    /// Apply an XMP rating: -1 marks the photo as rejected.
    pub fn apply_xmp_rating(&mut self, r: Option<i8>) {
        match r {
            Some(r) if r < 0 => {
                self.reject = true;
                self.rating = Some(0);
            }
            Some(r) => self.rating = Some(r as u8),
            None => {}
        }
    }

    pub fn is_landscape(&self) -> Option<bool> {
        let (w, h) = (self.width?, self.height?);
        let rotated = matches!(self.orientation, 5..=8);
        let (w, h) = if rotated { (h, w) } else { (w, h) };
        Some(w >= h)
    }
}

/// Where an embedded JPEG lives inside a file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JpegRef {
    pub offset: u64,
    pub len: u64,
    pub source: PreviewSource,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum PreviewSource {
    /// Small EXIF / container thumbnail (~160px).
    Thumbnail,
    /// Embedded preview of unknown size.
    Embedded,
    /// The file itself is a JPEG.
    WholeFile,
}

#[derive(Clone, Debug, Default)]
pub struct Probe {
    pub meta: Metadata,
    pub previews: Vec<JpegRef>,
}

/// Read metadata and preview locations of a file. Falls back to the file modification time when
/// no capture date can be found.
pub fn probe(path: &Path, kind: FileKind) -> Result<Probe> {
    let file = File::open(path).with_context(|| format!("open {}", path.display()))?;
    let file_len = file.metadata().map(|m| m.len()).unwrap_or(0);
    let mut r = BufReader::with_capacity(64 * 1024, file);
    let mut p = Probe { meta: Metadata { orientation: 1, ..Default::default() }, previews: Vec::new() };
    let res = match kind {
        FileKind::Jpeg => {
            p.previews.push(JpegRef { offset: 0, len: file_len, source: PreviewSource::WholeFile });
            parse_jpeg(&mut r, 0, &mut p, true)
        }
        FileKind::Raw | FileKind::Tiff => probe_raw(&mut r, file_len, &mut p),
        FileKind::Heif | FileKind::Png => probe_kamadak(&mut r, &mut p),
        FileKind::Video => Ok(()),
    };
    if let Err(e) = res {
        log::debug!("metadata of {}: {e:#}", path.display());
    }
    if p.meta.orientation == 0 || p.meta.orientation > 8 {
        p.meta.orientation = 1;
    }
    if p.meta.captured_at.is_none() {
        if let Some(t) = file_time(path) {
            p.meta.captured_at = Some(t);
            p.meta.date_source = DateSource::FileTime;
        }
    }
    Ok(p)
}

pub fn read_metadata(path: &Path, kind: FileKind) -> Metadata {
    probe(path, kind).map(|p| p.meta).unwrap_or_else(|_| {
        let mut m = Metadata { orientation: 1, ..Default::default() };
        m.captured_at = file_time(path);
        m
    })
}

/// File modification time as local naive date-time.
pub fn file_time(path: &Path) -> Option<NaiveDateTime> {
    let t = std::fs::metadata(path).ok()?.modified().ok()?;
    let dt: DateTime<Local> = t.into();
    Some(dt.naive_local())
}

// ---------------------------------------------------------------------------------------------
// JPEG

/// Walk JPEG segments starting at `base` (SOI). Reads EXIF/XMP from APP1 and dimensions from SOF.
fn parse_jpeg<R: Read + Seek>(r: &mut R, base: u64, p: &mut Probe, collect_thumb: bool) -> Result<()> {
    r.seek(SeekFrom::Start(base))?;
    let mut soi = [0u8; 2];
    r.read_exact(&mut soi)?;
    if soi != [0xFF, 0xD8] {
        bail!("not a JPEG");
    }
    let mut pos = base + 2;
    let mut segments = 0;
    loop {
        segments += 1;
        if segments > 256 {
            break;
        }
        r.seek(SeekFrom::Start(pos))?;
        let mut m = [0u8; 2];
        r.read_exact(&mut m)?;
        if m[0] != 0xFF {
            bail!("bad JPEG marker");
        }
        let mut marker = m[1];
        pos += 2;
        // Skip fill bytes.
        while marker == 0xFF {
            let mut b = [0u8; 1];
            r.read_exact(&mut b)?;
            marker = b[0];
            pos += 1;
        }
        if marker == 0xD9 || marker == 0xDA {
            break;
        }
        if (0xD0..=0xD7).contains(&marker) || marker == 0x01 {
            continue;
        }
        let mut lb = [0u8; 2];
        r.read_exact(&mut lb)?;
        let len = u16::from_be_bytes(lb) as u64;
        if len < 2 {
            bail!("bad segment length");
        }
        let payload_pos = pos + 2;
        let payload_len = len - 2;
        match marker {
            0xE1 => {
                let mut payload = vec![0u8; payload_len as usize];
                let n = read_up_to(r, &mut payload)?;
                payload.truncate(n);
                if payload.starts_with(b"Exif\0\0") {
                    let tiff_abs = payload_pos + 6;
                    let tiff = payload[6..].to_vec();
                    let mut sub = Probe::default();
                    if let Ok((mut t, first)) = TiffReader::new(Cursor::new(tiff), 0) {
                        read_tiff_meta(&mut t, first, &mut sub, tiff_abs, collect_thumb, false);
                    }
                    p.meta.merge_missing(&sub.meta);
                    p.previews.extend(sub.previews);
                } else if payload.starts_with(b"http://ns.adobe.com/xap/1.0/\0") && p.meta.rating.is_none() {
                    p.meta.apply_xmp_rating(xmp_rating(&payload));
                }
            }
            0xE2 if collect_thumb => {
                // Multi-Picture Format: many cameras store a ~1600px preview after the main image.
                let mut payload = vec![0u8; payload_len as usize];
                let n = read_up_to(r, &mut payload)?;
                payload.truncate(n);
                if payload.starts_with(b"MPF\0") {
                    p.previews.extend(mpf_previews(&payload[4..], payload_pos + 4));
                }
            }
            0xC0..=0xCF if !matches!(marker, 0xC4 | 0xC8 | 0xCC) => {
                let mut sof = [0u8; 5];
                r.read_exact(&mut sof)?;
                let h = u16::from_be_bytes([sof[1], sof[2]]) as u32;
                let w = u16::from_be_bytes([sof[3], sof[4]]) as u32;
                if p.meta.width.is_none() && w > 0 && h > 0 {
                    p.meta.width = Some(w);
                    p.meta.height = Some(h);
                }
                break;
            }
            _ => {}
        }
        pos = payload_pos + payload_len;
    }
    Ok(())
}

/// Secondary images listed in an MPF (CIPA DC-007) index. `abs` is the file offset of the MPF
/// TIFF header, which all MP entry offsets are relative to.
fn mpf_previews(tiff: &[u8], abs: u64) -> Vec<JpegRef> {
    let mut out = Vec::new();
    let Ok((mut t, first)) = TiffReader::new(Cursor::new(tiff.to_vec()), 0) else { return out };
    let Ok(ifd) = t.read_ifd(first) else { return out };
    let Some(e) = ifd.get(0xB002).cloned() else { return out };
    let Ok(entries) = t.bytes(&e) else { return out };
    let en = t.endian;
    for (i, ent) in entries.chunks_exact(16).enumerate() {
        let size = en.u32(&ent[4..8]) as u64;
        let off = en.u32(&ent[8..12]) as u64;
        // Entry 0 is the primary image itself (offset 0).
        if i > 0 && off > 0 && size > 0 {
            out.push(JpegRef { offset: abs + off, len: size, source: PreviewSource::Embedded });
        }
    }
    out
}

/// Dimensions and coding of a JPEG stream: `(width, height, is_lossless)`.
pub fn jpeg_info<R: Read + Seek>(r: &mut R, base: u64) -> Option<(u32, u32, bool)> {
    r.seek(SeekFrom::Start(base)).ok()?;
    let mut buf = vec![0u8; 256 * 1024];
    let n = read_up_to(r, &mut buf).ok()?;
    jpeg_info_bytes(&buf[..n])
}

pub fn jpeg_info_bytes(b: &[u8]) -> Option<(u32, u32, bool)> {
    if b.len() < 4 || b[0] != 0xFF || b[1] != 0xD8 {
        return None;
    }
    let mut i = 2;
    while i + 4 <= b.len() {
        if b[i] != 0xFF {
            return None;
        }
        let mut m = b[i + 1];
        i += 2;
        while m == 0xFF && i < b.len() {
            m = b[i];
            i += 1;
        }
        if m == 0xD9 || m == 0xDA {
            return None;
        }
        if (0xD0..=0xD7).contains(&m) || m == 0x01 {
            continue;
        }
        if i + 2 > b.len() {
            return None;
        }
        let len = u16::from_be_bytes([b[i], b[i + 1]]) as usize;
        if (0xC0..=0xCF).contains(&m) && !matches!(m, 0xC4 | 0xC8 | 0xCC) {
            if i + 7 > b.len() {
                return None;
            }
            let h = u16::from_be_bytes([b[i + 3], b[i + 4]]) as u32;
            let w = u16::from_be_bytes([b[i + 5], b[i + 6]]) as u32;
            let lossless = matches!(m, 0xC3 | 0xC7 | 0xCB | 0xCF);
            return Some((w, h, lossless));
        }
        i += len;
    }
    None
}

/// Extract `xmp:Rating` (-1 = rejected, 0..=5) from an XMP packet.
pub fn xmp_rating(xmp: &[u8]) -> Option<i8> {
    let s = String::from_utf8_lossy(xmp);
    let idx = s.find("xmp:Rating")?;
    let rest = &s[idx + "xmp:Rating".len()..];
    let rest = rest.trim_start_matches(['=', '"', '\'', '>', ' ']);
    let num: String = rest.chars().take_while(|c| c.is_ascii_digit() || *c == '-').collect();
    let v: i32 = num.parse().ok()?;
    Some(v.clamp(-1, 5) as i8)
}

// ---------------------------------------------------------------------------------------------
// TIFF metadata

/// Read IFD0, EXIF, GPS IFDs and collect embedded JPEG previews.
/// `abs` is the absolute file offset that corresponds to position 0 of the reader.
fn read_tiff_meta<R: Read + Seek>(
    t: &mut TiffReader<R>,
    first: u32,
    p: &mut Probe,
    abs: u64,
    collect_previews: bool,
    is_raw: bool,
) {
    let Ok(ifd0) = t.read_ifd(first) else { return };
    apply_ifd0(t, &ifd0, &mut p.meta);
    if collect_previews {
        collect_jpegs(t, &ifd0, abs, &mut p.previews, is_raw);
    }
    if let Some(off) = t.get_uint(&ifd0, tag::EXIF_IFD) {
        if let Ok(exif) = t.read_ifd(off) {
            apply_exif(t, &exif, &mut p.meta);
        }
    }
    if let Some(off) = t.get_uint(&ifd0, tag::GPS_IFD) {
        if let Ok(gps) = t.read_ifd(off) {
            apply_gps(t, &gps, &mut p.meta);
        }
    }
    if collect_previews {
        let mut raw_dims: Option<(u32, u32)> = None;
        for off in t.get_uints(&ifd0, tag::SUB_IFDS).into_iter().take(8) {
            if let Ok(sub) = t.read_ifd(off) {
                collect_jpegs(t, &sub, abs, &mut p.previews, is_raw);
                if t.get_uint(&sub, tag::NEW_SUBFILE_TYPE) == Some(0) {
                    if let (Some(w), Some(h)) =
                        (t.get_uint(&sub, tag::IMAGE_WIDTH), t.get_uint(&sub, tag::IMAGE_LENGTH))
                    {
                        raw_dims = Some((w, h));
                    }
                }
            }
        }
        let mut next = ifd0.next;
        for _ in 0..4 {
            if next == 0 {
                break;
            }
            match t.read_ifd(next) {
                Ok(ifd) => {
                    collect_jpegs(t, &ifd, abs, &mut p.previews, is_raw);
                    next = ifd.next;
                }
                Err(_) => break,
            }
        }
        if is_raw && p.meta.width.is_none() {
            if let Some((w, h)) = raw_dims {
                p.meta.width = Some(w);
                p.meta.height = Some(h);
            }
        }
    }
}

fn apply_ifd0<R: Read + Seek>(t: &mut TiffReader<R>, ifd: &Ifd, m: &mut Metadata) {
    if m.make.is_none() {
        m.make = t.get_ascii(ifd, tag::MAKE);
    }
    if m.model.is_none() {
        m.model = t.get_ascii(ifd, tag::MODEL).or_else(|| t.get_ascii(ifd, tag::UNIQUE_CAMERA_MODEL));
    }
    if let Some(o) = t.get_uint(ifd, tag::ORIENTATION) {
        if (1..=8).contains(&o) && m.orientation <= 1 {
            m.orientation = o as u8;
        }
    }
    if m.software.is_none() {
        m.software = t.get_ascii(ifd, tag::SOFTWARE);
    }
    if m.artist.is_none() {
        m.artist = t.get_ascii(ifd, tag::ARTIST);
    }
    if m.rating.is_none() {
        if let Some(r) = t.get_uint(ifd, tag::RATING) {
            if r <= 5 {
                m.rating = Some(r as u8);
            }
        }
    }
    if m.iso.is_none() {
        // Panasonic RW2 stores ISO in IFD0.
        if let Some(entry) = ifd.get(tag::PANASONIC_ISO) {
            if entry.typ == crate::tiff::ty::SHORT {
                if let Some(v) = t.uint(&entry.clone()) {
                    if v > 0 && v < 1_000_000 {
                        m.iso = Some(v);
                    }
                }
            }
        }
    }
    if m.captured_at.is_none() {
        if let Some(dt) = t.get_ascii(ifd, tag::DATE_TIME).and_then(|s| parse_exif_datetime(&s)) {
            // DateTime (modification) is only a weak hint; EXIF DateTimeOriginal overrides it.
            m.captured_at = Some(dt);
            m.date_source = DateSource::Exif;
        }
    }
}

fn apply_exif<R: Read + Seek>(t: &mut TiffReader<R>, ifd: &Ifd, m: &mut Metadata) {
    let original = t
        .get_ascii(ifd, tag::DATE_TIME_ORIGINAL)
        .and_then(|s| parse_exif_datetime(&s))
        .or_else(|| t.get_ascii(ifd, tag::DATE_TIME_DIGITIZED).and_then(|s| parse_exif_datetime(&s)));
    if let Some(mut dt) = original {
        if let Some(ss) = t.get_ascii(ifd, tag::SUBSEC_TIME_ORIGINAL) {
            let digits: String = ss.chars().filter(|c| c.is_ascii_digit()).take(3).collect();
            if !digits.is_empty() {
                let ms: u32 = format!("{digits:0<3}").parse().unwrap_or(0);
                dt += chrono::Duration::milliseconds(ms as i64);
            }
        }
        m.captured_at = Some(dt);
        m.date_source = DateSource::Exif;
    }
    if m.offset.is_none() {
        m.offset = t.get_ascii(ifd, tag::OFFSET_TIME_ORIGINAL).or_else(|| t.get_ascii(ifd, tag::OFFSET_TIME));
    }
    if let Some(v) = t.get_float(ifd, tag::EXPOSURE_TIME) {
        if v > 0.0 {
            m.exposure_time = Some(v as f32);
        }
    }
    if let Some(v) = t.get_float(ifd, tag::F_NUMBER) {
        if v > 0.0 {
            m.aperture = Some(v as f32);
        }
    }
    if let Some(v) = t.get_uint(ifd, tag::EXPOSURE_PROGRAM) {
        m.exposure_program = Some(v as u16);
    }
    let iso = t.get_uint(ifd, tag::ISO);
    let rei = t.get_uint(ifd, tag::RECOMMENDED_EXPOSURE_INDEX);
    let iso = match (iso, rei) {
        (Some(i), Some(r)) if (i == 65535 || i == 0) && r > 0 => Some(r),
        (Some(i), _) if i > 0 => Some(i),
        (_, Some(r)) if r > 0 => Some(r),
        _ => None,
    };
    if iso.is_some() {
        m.iso = iso;
    }
    if let Some(v) = t.get_float(ifd, tag::EXPOSURE_BIAS) {
        m.exposure_bias = Some((v * 100.0).round() as f32 / 100.0);
    }
    if let Some(v) = t.get_uint(ifd, tag::METERING_MODE) {
        m.metering_mode = Some(v as u16);
    }
    if let Some(v) = t.get_uint(ifd, tag::FLASH) {
        m.flash_fired = Some(v & 1 == 1);
    }
    if let Some(v) = t.get_float(ifd, tag::FOCAL_LENGTH) {
        if v > 0.0 {
            m.focal_length = Some(v as f32);
        }
    }
    if let Some(v) = t.get_uint(ifd, tag::FOCAL_35) {
        if v > 0 {
            m.focal_35mm = Some(v as f32);
        }
    }
    if let Some(v) = t.get_uint(ifd, tag::WHITE_BALANCE) {
        m.white_balance = Some(v as u16);
    }
    if let (Some(w), Some(h)) = (t.get_uint(ifd, tag::PIXEL_X), t.get_uint(ifd, tag::PIXEL_Y)) {
        if w > 0 && h > 0 {
            m.width = Some(w);
            m.height = Some(h);
        }
    }
    if m.serial.is_none() {
        m.serial = t.get_ascii(ifd, tag::BODY_SERIAL);
    }
    if m.lens_make.is_none() {
        m.lens_make = t.get_ascii(ifd, tag::LENS_MAKE);
    }
    if m.lens.is_none() {
        m.lens = t.get_ascii(ifd, tag::LENS_MODEL).filter(|s| is_meaningful_lens(s));
    }
    if m.lens.is_none() {
        let spec = t.get_floats(ifd, tag::LENS_SPEC);
        m.lens = lens_from_spec(&spec);
    }
}

fn apply_gps<R: Read + Seek>(t: &mut TiffReader<R>, ifd: &Ifd, m: &mut Metadata) {
    let lat = dms(&t.get_floats(ifd, tag::GPS_LAT));
    let lon = dms(&t.get_floats(ifd, tag::GPS_LON));
    let (Some(mut lat), Some(mut lon)) = (lat, lon) else { return };
    if t.get_ascii(ifd, tag::GPS_LAT_REF).is_some_and(|s| s.starts_with('S')) {
        lat = -lat;
    }
    if t.get_ascii(ifd, tag::GPS_LON_REF).is_some_and(|s| s.starts_with('W')) {
        lon = -lon;
    }
    if (lat == 0.0 && lon == 0.0) || lat.abs() > 90.0 || lon.abs() > 180.0 {
        return;
    }
    let mut alt = t.get_float(ifd, tag::GPS_ALT);
    if let Some(a) = alt.as_mut() {
        if t.get_uint(ifd, tag::GPS_ALT_REF) == Some(1) {
            *a = -*a;
        }
    }
    m.gps = Some(Gps { lat, lon, alt });
}

fn dms(v: &[f64]) -> Option<f64> {
    if v.len() < 3 || v.iter().any(|x| !x.is_finite()) {
        return None;
    }
    Some(v[0] + v[1] / 60.0 + v[2] / 3600.0)
}

/// Record embedded JPEG streams referenced by an IFD.
fn collect_jpegs<R: Read + Seek>(t: &mut TiffReader<R>, ifd: &Ifd, abs: u64, out: &mut Vec<JpegRef>, is_raw: bool) {
    let base = t.base();
    let small = |len: u64| len < 64 * 1024;
    // JPEGInterchangeFormat (thumbnails in IFD1, previews in RAWs).
    if let (Some(off), Some(len)) = (t.get_uint(ifd, tag::JPEG_IF_OFFSET), t.get_uint(ifd, tag::JPEG_IF_LENGTH)) {
        if off > 0 && len > 0 {
            let len = len as u64;
            out.push(JpegRef {
                offset: abs + base + off as u64,
                len,
                source: if small(len) { PreviewSource::Thumbnail } else { PreviewSource::Embedded },
            });
        }
    }
    // Strip-stored JPEG (CR2 IFD0, DNG previews). Skip CFA / linear raw data.
    let compression = t.get_uint(ifd, tag::COMPRESSION);
    if is_raw && matches!(compression, Some(6) | Some(7)) {
        let photometric = t.get_uint(ifd, tag::PHOTOMETRIC);
        let subfile = t.get_uint(ifd, tag::NEW_SUBFILE_TYPE);
        let is_raw_data =
            matches!(photometric, Some(32803) | Some(34892)) || (compression == Some(7) && subfile == Some(0));
        if !is_raw_data {
            let offs = t.get_uints(ifd, tag::STRIP_OFFSETS);
            let lens = t.get_uints(ifd, tag::STRIP_BYTE_COUNTS);
            if offs.len() == 1 && lens.len() == 1 && lens[0] > 0 {
                let len = lens[0] as u64;
                out.push(JpegRef {
                    offset: abs + base + offs[0] as u64,
                    len,
                    source: if small(len) { PreviewSource::Thumbnail } else { PreviewSource::Embedded },
                });
            }
        }
    }
    // Panasonic RW2 JpgFromRaw: stored as UNDEFINED value of the tag itself.
    if let Some(e) = ifd.get(tag::PANASONIC_JPG_FROM_RAW).cloned() {
        if let Some(pos) = t.value_pos(&e) {
            out.push(JpegRef { offset: abs + pos, len: e.count as u64, source: PreviewSource::Embedded });
        }
    }
}

fn probe_raw<R: Read + Seek>(r: &mut R, file_len: u64, p: &mut Probe) -> Result<()> {
    let mut head = [0u8; 16];
    r.seek(SeekFrom::Start(0))?;
    let n = read_up_to(r, &mut head)?;
    if n < 12 {
        bail!("file too small");
    }
    if head.starts_with(b"FUJIFILMCCD-RAW") {
        return probe_raf(r, p);
    }
    if &head[4..8] == b"ftyp" {
        return probe_cr3(r, file_len, p);
    }
    if head.starts_with(&[0xFF, 0xD8]) {
        // Some "RAW" files are plain JPEG.
        p.previews.push(JpegRef { offset: 0, len: file_len, source: PreviewSource::WholeFile });
        return parse_jpeg(r, 0, p, true);
    }
    let (mut t, first) = TiffReader::new(&mut *r, 0)?;
    read_tiff_meta(&mut t, first, p, 0, true, true);
    // Panasonic & others keep the full EXIF only inside the embedded JPEG.
    if p.meta.date_source != DateSource::Exif || p.meta.exposure_time.is_none() {
        if let Some(jr) =
            p.previews.iter().copied().filter(|j| j.source == PreviewSource::Embedded).max_by_key(|j| j.len)
        {
            let mut sub = Probe::default();
            if parse_jpeg(r, jr.offset, &mut sub, false).is_ok() {
                p.meta.merge_missing(&sub.meta);
                if sub.meta.date_source == DateSource::Exif && p.meta.date_source != DateSource::Exif {
                    p.meta.captured_at = sub.meta.captured_at;
                    p.meta.date_source = DateSource::Exif;
                }
            }
        }
    }
    Ok(())
}

fn probe_raf<R: Read + Seek>(r: &mut R, p: &mut Probe) -> Result<()> {
    let mut h = [0u8; 100];
    r.seek(SeekFrom::Start(0))?;
    r.read_exact(&mut h)?;
    let off = u32::from_be_bytes([h[84], h[85], h[86], h[87]]) as u64;
    let len = u32::from_be_bytes([h[88], h[89], h[90], h[91]]) as u64;
    if off == 0 || len == 0 {
        bail!("RAF without embedded JPEG");
    }
    // The whole embedded JPEG is the preview; its own IFD1 thumbnail is collected too.
    let before = p.previews.len();
    parse_jpeg(r, off, p, true)?;
    for jr in &mut p.previews[before..] {
        jr.source = PreviewSource::Thumbnail;
    }
    p.previews.push(JpegRef { offset: off, len, source: PreviewSource::Embedded });
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// ISO-BMFF (Canon CR3)

const CANON_UUID: [u8; 16] =
    [0x85, 0xc0, 0xb6, 0x87, 0x82, 0x0f, 0x11, 0xe0, 0x81, 0x11, 0xf4, 0xce, 0x46, 0x2b, 0x6a, 0x48];
const CANON_PREVIEW_UUID: [u8; 16] =
    [0xea, 0xf4, 0x2b, 0x5e, 0x1c, 0x98, 0x4b, 0x88, 0xb9, 0xfb, 0xb7, 0xdc, 0x40, 0x6e, 0x4d, 0x16];

struct BoxHdr {
    typ: [u8; 4],
    payload: u64,
    end: u64,
}

fn read_box<R: Read + Seek>(r: &mut R, pos: u64, limit: u64) -> Option<BoxHdr> {
    if pos + 8 > limit {
        return None;
    }
    r.seek(SeekFrom::Start(pos)).ok()?;
    let mut h = [0u8; 16];
    let n = read_up_to(r, &mut h).ok()?;
    if n < 8 {
        return None;
    }
    let size32 = u32::from_be_bytes([h[0], h[1], h[2], h[3]]) as u64;
    let typ = [h[4], h[5], h[6], h[7]];
    let (size, hdr) = match size32 {
        0 => (limit - pos, 8),
        1 => {
            if n < 16 {
                return None;
            }
            (u64::from_be_bytes(h[8..16].try_into().unwrap()), 16)
        }
        s => (s, 8),
    };
    if size < hdr || pos + size > limit {
        return None;
    }
    Some(BoxHdr { typ, payload: pos + hdr, end: pos + size })
}

fn children<R: Read + Seek>(r: &mut R, from: u64, to: u64) -> Vec<BoxHdr> {
    let mut v = Vec::new();
    let mut pos = from;
    while let Some(b) = read_box(r, pos, to) {
        pos = b.end;
        v.push(b);
        if v.len() > 512 {
            break;
        }
    }
    v
}

fn read_uuid<R: Read + Seek>(r: &mut R, b: &BoxHdr) -> Option<[u8; 16]> {
    r.seek(SeekFrom::Start(b.payload)).ok()?;
    let mut u = [0u8; 16];
    r.read_exact(&mut u).ok()?;
    Some(u)
}

/// Find the first `FF D8 FF` within the first bytes of a range.
fn find_soi<R: Read + Seek>(r: &mut R, from: u64, to: u64) -> Option<u64> {
    r.seek(SeekFrom::Start(from)).ok()?;
    let mut buf = [0u8; 64];
    let n = read_up_to(r, &mut buf).ok()?.min((to.saturating_sub(from)) as usize);
    buf[..n].windows(3).position(|w| w == [0xFF, 0xD8, 0xFF]).map(|i| from + i as u64)
}

fn probe_cr3<R: Read + Seek>(r: &mut R, file_len: u64, p: &mut Probe) -> Result<()> {
    let top = children(r, 0, file_len);
    let mut found_meta = false;
    for b in &top {
        match &b.typ {
            b"moov" => {
                let mut first_trak = true;
                for c in children(r, b.payload, b.end) {
                    match &c.typ {
                        b"uuid" if read_uuid(r, &c) == Some(CANON_UUID) => {
                            for m in children(r, c.payload + 16, c.end) {
                                match &m.typ {
                                    b"CMT1" => {
                                        if let Ok((mut t, first)) = TiffReader::new(&mut *r, m.payload) {
                                            if let Ok(ifd) = t.read_ifd(first) {
                                                apply_ifd0(&mut t, &ifd, &mut p.meta);
                                                found_meta = true;
                                            }
                                        }
                                    }
                                    b"CMT2" => {
                                        if let Ok((mut t, first)) = TiffReader::new(&mut *r, m.payload) {
                                            if let Ok(ifd) = t.read_ifd(first) {
                                                apply_exif(&mut t, &ifd, &mut p.meta);
                                            }
                                        }
                                    }
                                    b"CMT4" => {
                                        if let Ok((mut t, first)) = TiffReader::new(&mut *r, m.payload) {
                                            if let Ok(ifd) = t.read_ifd(first) {
                                                apply_gps(&mut t, &ifd, &mut p.meta);
                                            }
                                        }
                                    }
                                    b"THMB" => {
                                        if let Some(soi) = find_soi(r, m.payload, m.end) {
                                            p.previews.push(JpegRef {
                                                offset: soi,
                                                len: m.end - soi,
                                                source: PreviewSource::Thumbnail,
                                            });
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }
                        b"trak" if first_trak => {
                            first_trak = false;
                            if let Some(j) = cr3_first_track_jpeg(r, &c) {
                                p.previews.push(j);
                            }
                        }
                        _ => {}
                    }
                }
            }
            b"uuid" if read_uuid(r, b) == Some(CANON_PREVIEW_UUID) => {
                // uuid(16) + 8 bytes, then a PRVW box.
                for c in children(r, b.payload + 16 + 8, b.end) {
                    if &c.typ == b"PRVW" {
                        if let Some(soi) = find_soi(r, c.payload, c.end) {
                            p.previews.push(JpegRef { offset: soi, len: c.end - soi, source: PreviewSource::Embedded });
                        }
                    }
                }
            }
            _ => {}
        }
    }
    if !found_meta {
        bail!("CR3 without Canon metadata");
    }
    Ok(())
}

/// The first track of a CR3 holds a full resolution JPEG.
fn cr3_first_track_jpeg<R: Read + Seek>(r: &mut R, trak: &BoxHdr) -> Option<JpegRef> {
    let find = |r: &mut R, parent: &BoxHdr, typ: &[u8; 4]| -> Option<BoxHdr> {
        children(r, parent.payload, parent.end).into_iter().find(|b| &b.typ == typ)
    };
    let mdia = find(r, trak, b"mdia")?;
    let minf = find(r, &mdia, b"minf")?;
    let stbl = find(r, &minf, b"stbl")?;
    let kids = children(r, stbl.payload, stbl.end);
    let stsz = kids.iter().find(|b| &b.typ == b"stsz")?;
    r.seek(SeekFrom::Start(stsz.payload)).ok()?;
    let mut s = [0u8; 16];
    r.read_exact(&mut s).ok()?;
    let mut size = u32::from_be_bytes([s[4], s[5], s[6], s[7]]) as u64;
    if size == 0 {
        size = u32::from_be_bytes([s[12], s[13], s[14], s[15]]) as u64;
    }
    let offset = if let Some(co64) = kids.iter().find(|b| &b.typ == b"co64") {
        r.seek(SeekFrom::Start(co64.payload + 8)).ok()?;
        let mut o = [0u8; 8];
        r.read_exact(&mut o).ok()?;
        u64::from_be_bytes(o)
    } else {
        let stco = kids.iter().find(|b| &b.typ == b"stco")?;
        r.seek(SeekFrom::Start(stco.payload + 8)).ok()?;
        let mut o = [0u8; 4];
        r.read_exact(&mut o).ok()?;
        u32::from_be_bytes(o) as u64
    };
    r.seek(SeekFrom::Start(offset)).ok()?;
    let mut m = [0u8; 2];
    r.read_exact(&mut m).ok()?;
    (m == [0xFF, 0xD8] && size > 0).then_some(JpegRef { offset, len: size, source: PreviewSource::Embedded })
}

// ---------------------------------------------------------------------------------------------
// HEIF / PNG via kamadak-exif

fn probe_kamadak<R: Read + Seek + std::io::BufRead>(r: &mut R, p: &mut Probe) -> Result<()> {
    r.seek(SeekFrom::Start(0))?;
    let exif = exif::Reader::new().read_from_container(r)?;
    let buf = exif.buf().to_vec();
    let (mut t, first) = TiffReader::new(Cursor::new(buf), 0)?;
    read_tiff_meta(&mut t, first, p, 0, false, false);
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Helpers

pub fn parse_exif_datetime(s: &str) -> Option<NaiveDateTime> {
    let s = s.trim();
    if s.len() < 19 || s.starts_with("0000") {
        return None;
    }
    let mut b: Vec<u8> = s.as_bytes()[..19].to_vec();
    // Normalise "2024-05-01T10:00:00" and "2024:05:01 10:00:00".
    if b[4] == b'-' {
        b[4] = b':';
    }
    if b[7] == b'-' {
        b[7] = b':';
    }
    if b[10] == b'T' {
        b[10] = b' ';
    }
    let s = std::str::from_utf8(&b).ok()?;
    let dt = NaiveDateTime::parse_from_str(s, "%Y:%m:%d %H:%M:%S").ok()?;
    use chrono::Datelike;
    (dt.year() >= 1980 && dt.year() <= 2200).then_some(dt)
}

fn is_meaningful_lens(s: &str) -> bool {
    let t = s.trim();
    !(t.is_empty()
        || t.chars().all(|c| c == '-' || c == ' ' || c == '0' || c == '.')
        || t == "0.0 mm f/0.0"
        || t.eq_ignore_ascii_case("unknown"))
}

fn lens_from_spec(spec: &[f64]) -> Option<String> {
    if spec.len() < 4 {
        return None;
    }
    let (fmin, fmax, amin, amax) = (spec[0], spec[1], spec[2], spec[3]);
    if !fmin.is_finite() || fmin <= 0.0 {
        return None;
    }
    let focal = if !fmax.is_finite() || (fmax - fmin).abs() < 0.5 {
        format!("{}mm", trim_num(fmin))
    } else {
        format!("{}-{}mm", trim_num(fmin), trim_num(fmax))
    };
    let ap = if amin.is_finite() && amin > 0.0 {
        if amax.is_finite() && (amax - amin).abs() > 0.05 {
            format!(" f/{}-{}", trim_num(amin), trim_num(amax))
        } else {
            format!(" f/{}", trim_num(amin))
        }
    } else {
        String::new()
    };
    Some(format!("{focal}{ap}"))
}

fn trim_num(v: f64) -> String {
    if (v - v.round()).abs() < 0.05 {
        format!("{}", v.round() as i64)
    } else {
        format!("{:.1}", v)
    }
}

/// Normalised manufacturer name.
pub fn clean_make(make: &str) -> String {
    let m = make.trim();
    let u = m.to_ascii_uppercase();
    let known: &[(&str, &str)] = &[
        ("NIKON", "Nikon"),
        ("CANON", "Canon"),
        ("SONY", "Sony"),
        ("FUJIFILM", "Fujifilm"),
        ("FUJI PHOTO", "Fujifilm"),
        ("OLYMPUS", "Olympus"),
        ("OM DIGITAL", "OM System"),
        ("PANASONIC", "Panasonic"),
        ("RICOH", "Ricoh"),
        ("PENTAX", "Pentax"),
        ("LEICA", "Leica"),
        ("SAMSUNG", "Samsung"),
        ("HASSELBLAD", "Hasselblad"),
        ("SIGMA", "Sigma"),
        ("APPLE", "Apple"),
        ("GOOGLE", "Google"),
        ("DJI", "DJI"),
        ("GOPRO", "GoPro"),
        ("KODAK", "Kodak"),
        ("EASTMAN KODAK", "Kodak"),
        ("MINOLTA", "Minolta"),
        ("KONICA MINOLTA", "Konica Minolta"),
        ("PHASE ONE", "Phase One"),
        ("XIAOMI", "Xiaomi"),
        ("HUAWEI", "Huawei"),
    ];
    for (k, v) in known {
        if u.starts_with(k) {
            return v.to_string();
        }
    }
    m.to_string()
}

pub fn camera_display(make: Option<&str>, model: Option<&str>) -> Option<String> {
    let model = model.map(str::trim).filter(|s| !s.is_empty());
    let make = make.map(str::trim).filter(|s| !s.is_empty());
    match (make, model) {
        (None, None) => None,
        (Some(mk), None) => Some(clean_make(mk)),
        (None, Some(md)) => Some(md.to_string()),
        (Some(mk), Some(md)) => {
            let clean = clean_make(mk);
            let first_word = mk.split_whitespace().next().unwrap_or(mk);
            let md_upper = md.to_ascii_uppercase();
            let rest = if md_upper.starts_with(&first_word.to_ascii_uppercase()) {
                md[first_word.len()..].trim()
            } else if md_upper.starts_with(&clean.to_ascii_uppercase()) {
                md[clean.len()..].trim()
            } else {
                md
            };
            if rest.is_empty() {
                Some(clean)
            } else {
                Some(format!("{clean} {rest}"))
            }
        }
    }
}

#[cfg(test)]
pub(crate) mod tests_support {
    /// Build a JPEG with an APP1 Exif segment around a real (tiny) image.
    pub fn jpeg_with_exif(tiff: &[u8], w: u32, h: u32) -> Vec<u8> {
        let img = image::RgbImage::from_fn(w, h, |x, y| image::Rgb([(x * 7) as u8, (y * 5) as u8, 128]));
        let mut plain = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut plain, 85).encode_image(&img).unwrap();
        let mut out = vec![0xFF, 0xD8];
        let len = (tiff.len() + 6 + 2) as u16;
        out.extend_from_slice(&[0xFF, 0xE1]);
        out.extend_from_slice(&len.to_be_bytes());
        out.extend_from_slice(b"Exif\0\0");
        out.extend_from_slice(tiff);
        out.extend_from_slice(&plain[2..]);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::tests_support::jpeg_with_exif;
    use super::*;
    use crate::tiff::testutil::*;

    #[test]
    fn camera_names() {
        assert_eq!(camera_display(Some("NIKON CORPORATION"), Some("NIKON Z 6")).unwrap(), "Nikon Z 6");
        assert_eq!(camera_display(Some("Canon"), Some("Canon EOS R5")).unwrap(), "Canon EOS R5");
        assert_eq!(camera_display(Some("SONY"), Some("ILCE-7M3")).unwrap(), "Sony ILCE-7M3");
        assert_eq!(camera_display(Some("FUJIFILM"), Some("X-T4")).unwrap(), "Fujifilm X-T4");
        assert_eq!(camera_display(Some("OLYMPUS IMAGING CORP."), Some("E-M1MarkII")).unwrap(), "Olympus E-M1MarkII");
    }

    #[test]
    fn datetime() {
        let d = parse_exif_datetime("2024:05:01 10:20:30").unwrap();
        assert_eq!(d.to_string(), "2024-05-01 10:20:30");
        assert!(parse_exif_datetime("0000:00:00 00:00:00").is_none());
        assert!(parse_exif_datetime("    :  :     :  :  ").is_none());
    }

    #[test]
    fn lens_spec() {
        assert_eq!(lens_from_spec(&[24.0, 70.0, 2.8, 2.8]).unwrap(), "24-70mm f/2.8");
        assert_eq!(lens_from_spec(&[50.0, 50.0, 1.8, 1.8]).unwrap(), "50mm f/1.8");
        assert_eq!(lens_from_spec(&[18.0, 55.0, 3.5, 5.6]).unwrap(), "18-55mm f/3.5-5.6");
    }

    #[test]
    fn xmp() {
        assert_eq!(xmp_rating(b"<x xmp:Rating=\"4\"/>"), Some(4));
        assert_eq!(xmp_rating(b"<xmp:Rating>3</xmp:Rating>"), Some(3));
        assert_eq!(xmp_rating(b"<x xmp:Rating=\"-1\"/>"), Some(-1));
    }

    #[test]
    fn mpf_preview_is_found() {
        use crate::preview;
        // Primary 2400x1600 JPEG followed by a 1200x800 MPF preview.
        let big = image::RgbImage::from_fn(2400, 1600, |x, _| image::Rgb([(x % 251) as u8, 90, 160]));
        let small = image::RgbImage::from_fn(1200, 800, |_, y| image::Rgb([30, (y % 251) as u8, 60]));
        let mut prim = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut prim, 80).encode_image(&big).unwrap();
        let mut sec = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut sec, 80).encode_image(&small).unwrap();
        // APP2 MPF segment: "MPF\0" + little-endian TIFF with MP Index IFD (one 0xB002 entry).
        let build = |prim_len: u32, sec_off: u32| {
            let mut t = b"II*\0\x08\0\0\0".to_vec();
            t.extend_from_slice(&1u16.to_le_bytes());
            t.extend_from_slice(&0xB002u16.to_le_bytes());
            t.extend_from_slice(&7u16.to_le_bytes());
            t.extend_from_slice(&32u32.to_le_bytes());
            t.extend_from_slice(&26u32.to_le_bytes()); // value offset
            t.extend_from_slice(&0u32.to_le_bytes()); // next IFD
            for (size, off) in [(prim_len, 0u32), (sec.len() as u32, sec_off)] {
                t.extend_from_slice(&0u32.to_le_bytes());
                t.extend_from_slice(&size.to_le_bytes());
                t.extend_from_slice(&off.to_le_bytes());
                t.extend_from_slice(&0u32.to_le_bytes());
            }
            let mut seg = vec![0xFF, 0xE2];
            seg.extend_from_slice(&((t.len() + 6) as u16).to_be_bytes());
            seg.extend_from_slice(b"MPF\0");
            seg.extend_from_slice(&t);
            seg
        };
        let seg_len = build(0, 0).len();
        let mpf_tiff_abs = 2 + 4 + 4; // SOI + marker/len + "MPF\0"
        let total_prim = 2 + seg_len + prim.len() - 2;
        let seg = build(total_prim as u32, (total_prim - mpf_tiff_abs) as u32);
        let mut file = vec![0xFF, 0xD8];
        file.extend_from_slice(&seg);
        file.extend_from_slice(&prim[2..]);
        file.extend_from_slice(&sec);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("MPF.JPG");
        std::fs::write(&path, &file).unwrap();
        let p = probe(&path, FileKind::Jpeg).unwrap();
        let cands = preview::candidates(&path, &p.previews);
        assert!(cands.iter().any(|c| c.width == 1200 && c.height == 800), "{cands:?}");
        assert_eq!(preview::choose(&cands, 480).unwrap().width, 1200);
    }

    #[test]
    fn jpeg_probe() {
        let tiff = build_tiff(vec![
            (tag::MAKE, Val::Ascii("SONY")),
            (tag::MODEL, Val::Ascii("ILCE-7M4")),
            (tag::ORIENTATION, Val::Short(vec![8])),
            (
                tag::EXIF_IFD,
                Val::Ifd(vec![
                    (tag::DATE_TIME_ORIGINAL, Val::Ascii("2025:03:04 05:06:07")),
                    (tag::SUBSEC_TIME_ORIGINAL, Val::Ascii("45")),
                    (tag::EXPOSURE_TIME, Val::Rational(vec![(1, 250)])),
                    (tag::F_NUMBER, Val::Rational(vec![(18, 10)])),
                    (tag::ISO, Val::Short(vec![400])),
                    (tag::FOCAL_LENGTH, Val::Rational(vec![(35, 1)])),
                    (tag::LENS_MODEL, Val::Ascii("FE 35mm F1.8")),
                ]),
            ),
        ]);
        let jpg = jpeg_with_exif(&tiff, 64, 48);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("DSC00001.JPG");
        std::fs::write(&path, &jpg).unwrap();
        let p = probe(&path, FileKind::Jpeg).unwrap();
        let m = &p.meta;
        assert_eq!(m.camera().unwrap(), "Sony ILCE-7M4");
        assert_eq!(m.orientation, 8);
        assert_eq!(m.iso, Some(400));
        assert_eq!(m.lens.as_deref(), Some("FE 35mm F1.8"));
        assert_eq!(m.date_source, DateSource::Exif);
        assert_eq!(m.captured_at.unwrap().to_string(), "2025-03-04 05:06:07.450");
        assert!((m.exposure_time.unwrap() - 0.004).abs() < 1e-6);
        assert_eq!((m.width, m.height), (Some(64), Some(48)));
        assert_eq!(p.previews[0].source, PreviewSource::WholeFile);
    }
}
