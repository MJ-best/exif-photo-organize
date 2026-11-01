//! Fast thumbnail / preview decoding.
//!
//! Strategy: never decode RAW sensor data. Every camera embeds at least one JPEG preview in its RAW
//! files; we pick the smallest one that is still large enough and decode it with DCT scaling
//! (1/2, 1/4, 1/8), which is several times faster than a full decode followed by a resize.

use crate::media::FileKind;
use crate::metadata::{self, JpegRef, PreviewSource};
use anyhow::{bail, Context, Result};
use image::{imageops::FilterType, DynamicImage, RgbImage};
use std::fs::File;
use std::io::{BufReader, Cursor, Read, Seek, SeekFrom};
use std::path::Path;

/// Long edge (pixels) of thumbnails stored in the catalog.
pub const THUMB_EDGE: u32 = 480;
const MAX_JPEG_BYTES: u64 = 96 << 20;

#[derive(Clone, Copy, Debug)]
pub struct Candidate {
    pub jpeg: JpegRef,
    pub width: u32,
    pub height: u32,
}

impl Candidate {
    pub fn long_edge(&self) -> u32 {
        self.width.max(self.height)
    }
}

/// Inspect embedded JPEGs and return the decodable ones with their dimensions.
pub fn candidates(path: &Path, previews: &[JpegRef]) -> Vec<Candidate> {
    let Ok(f) = File::open(path) else { return Vec::new() };
    let len = f.metadata().map(|m| m.len()).unwrap_or(u64::MAX);
    let mut r = BufReader::with_capacity(16 * 1024, f);
    let mut out: Vec<Candidate> = Vec::new();
    for j in previews {
        if j.len < 128 || j.offset + j.len > len.saturating_add(1) {
            continue;
        }
        if out.iter().any(|c| c.jpeg.offset == j.offset) {
            continue;
        }
        if let Some((w, h, lossless)) = jpeg_sof(&mut r, j.offset) {
            if !lossless && w >= 32 && h >= 32 {
                out.push(Candidate { jpeg: *j, width: w, height: h });
            }
        }
    }
    out
}

/// Walk JPEG markers via seeks to find the frame header (SOF).
fn jpeg_sof<R: Read + Seek>(r: &mut R, base: u64) -> Option<(u32, u32, bool)> {
    r.seek(SeekFrom::Start(base)).ok()?;
    let mut m = [0u8; 2];
    r.read_exact(&mut m).ok()?;
    if m != [0xFF, 0xD8] {
        return None;
    }
    let mut pos = base + 2;
    for _ in 0..128 {
        r.seek(SeekFrom::Start(pos)).ok()?;
        let mut h = [0u8; 4];
        r.read_exact(&mut h).ok()?;
        if h[0] != 0xFF {
            return None;
        }
        let marker = h[1];
        if marker == 0xFF {
            pos += 1;
            continue;
        }
        if (0xD0..=0xD7).contains(&marker) || marker == 0x01 {
            pos += 2;
            continue;
        }
        if marker == 0xD9 || marker == 0xDA {
            return None;
        }
        let len = u16::from_be_bytes([h[2], h[3]]) as u64;
        if (0xC0..=0xCF).contains(&marker) && !matches!(marker, 0xC4 | 0xC8 | 0xCC) {
            let mut s = [0u8; 5];
            r.read_exact(&mut s).ok()?;
            let hh = u16::from_be_bytes([s[1], s[2]]) as u32;
            let ww = u16::from_be_bytes([s[3], s[4]]) as u32;
            return Some((ww, hh, matches!(marker, 0xC3 | 0xC7 | 0xCB | 0xCF)));
        }
        pos += 2 + len;
    }
    None
}

/// Choose the smallest candidate whose long edge is at least `min_edge`, or the largest one.
pub fn choose(cands: &[Candidate], min_edge: u32) -> Option<Candidate> {
    let mut sorted: Vec<Candidate> = cands.to_vec();
    sorted.sort_by_key(|c| (c.long_edge(), std::cmp::Reverse(c.jpeg.source)));
    sorted.iter().copied().find(|c| c.long_edge() >= min_edge).or_else(|| sorted.last().copied())
}

pub fn read_range(path: &Path, offset: u64, len: u64) -> Result<Vec<u8>> {
    if len > MAX_JPEG_BYTES {
        bail!("embedded image too large");
    }
    let mut f = File::open(path).with_context(|| format!("open {}", path.display()))?;
    f.seek(SeekFrom::Start(offset))?;
    let mut buf = vec![0u8; len as usize];
    let n = crate::tiff::read_up_to(&mut f, &mut buf)?;
    buf.truncate(n);
    Ok(buf)
}

/// Decode a JPEG, using DCT scaling so that the long edge is still >= `min_long_edge`.
pub fn decode_jpeg(bytes: &[u8], min_long_edge: Option<u32>) -> Result<RgbImage> {
    let mut dec = jpeg_decoder::Decoder::new(Cursor::new(bytes));
    dec.read_info().context("jpeg header")?;
    let info = dec.info().context("jpeg info")?;
    let (w, h) = (info.width as u32, info.height as u32);
    if let Some(min) = min_long_edge {
        let long = w.max(h);
        if long > min && min > 0 {
            let ratio = min as f64 / long as f64;
            let rw = ((w as f64 * ratio).ceil() as u32).clamp(1, u16::MAX as u32) as u16;
            let rh = ((h as f64 * ratio).ceil() as u32).clamp(1, u16::MAX as u32) as u16;
            dec.scale(rw, rh).context("jpeg scale")?;
        }
    }
    let pixels = dec.decode().context("jpeg decode")?;
    let info = dec.info().context("jpeg info")?;
    let (w, h) = (info.width as u32, info.height as u32);
    let rgb = match info.pixel_format {
        jpeg_decoder::PixelFormat::RGB24 => pixels,
        jpeg_decoder::PixelFormat::L8 => pixels.iter().flat_map(|&v| [v, v, v]).collect(),
        jpeg_decoder::PixelFormat::L16 => pixels.chunks_exact(2).flat_map(|c| [c[0], c[0], c[0]]).collect(),
        jpeg_decoder::PixelFormat::CMYK32 => pixels
            .chunks_exact(4)
            .flat_map(|c| {
                let k = c[3] as u32;
                [(c[0] as u32 * k / 255) as u8, (c[1] as u32 * k / 255) as u8, (c[2] as u32 * k / 255) as u8]
            })
            .collect(),
    };
    RgbImage::from_raw(w, h, rgb).context("jpeg buffer size")
}

pub fn orient(img: RgbImage, orientation: u8) -> RgbImage {
    if orientation <= 1 || orientation > 8 {
        return img;
    }
    let mut d = DynamicImage::ImageRgb8(img);
    if let Some(o) = image::metadata::Orientation::from_exif(orientation) {
        d.apply_orientation(o);
    }
    d.into_rgb8()
}

pub fn fit(img: RgbImage, max_edge: u32) -> RgbImage {
    let (w, h) = img.dimensions();
    let long = w.max(h);
    if long <= max_edge {
        return img;
    }
    let s = max_edge as f64 / long as f64;
    let nw = ((w as f64 * s).round() as u32).max(1);
    let nh = ((h as f64 * s).round() as u32).max(1);
    image::imageops::resize(&img, nw, nh, FilterType::Triangle)
}

pub fn encode_jpeg(img: &RgbImage, quality: u8) -> Result<Vec<u8>> {
    let mut out = Vec::with_capacity(64 * 1024);
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality).encode_image(img)?;
    Ok(out)
}

/// Load an image of a file with its long edge close to `edge` (never smaller unless the source
/// is smaller). `previews` may be passed when they are already known (e.g. from an import scan);
/// otherwise the file is probed. The result is rotated according to `orientation`.
pub fn load_image(
    path: &Path,
    kind: FileKind,
    orientation: u8,
    edge: u32,
    previews: Option<&[JpegRef]>,
) -> Result<RgbImage> {
    let img = match kind {
        FileKind::Png | FileKind::Tiff => {
            let img = image::open(path).with_context(|| format!("decode {}", path.display()))?;
            let rgb = img.to_rgb8();
            return Ok(orient(fit(rgb, edge), orientation));
        }
        FileKind::Video | FileKind::Heif => bail!("no decoder for {:?}", kind),
        FileKind::Jpeg | FileKind::Raw => {
            let owned;
            let previews = match previews {
                Some(p) if !p.is_empty() => p,
                _ => {
                    owned = metadata::probe(path, kind)?.previews;
                    &owned[..]
                }
            };
            let cands = candidates(path, previews);
            let c = choose(&cands, edge).context("no embedded preview found")?;
            let bytes = read_range(path, c.jpeg.offset, c.jpeg.len)?;
            decode_jpeg(&bytes, Some(edge))?
        }
    };
    Ok(orient(fit(img, edge), orientation))
}

/// Generate a catalog thumbnail (JPEG bytes) plus its dimensions.
pub fn make_thumbnail(
    path: &Path,
    kind: FileKind,
    orientation: u8,
    previews: Option<&[JpegRef]>,
) -> Result<(u32, u32, Vec<u8>)> {
    let img = load_image(path, kind, orientation, THUMB_EDGE, previews)?;
    let (w, h) = img.dimensions();
    Ok((w, h, encode_jpeg(&img, 80)?))
}

/// Very fast tiny preview, used for the import screen (EXIF thumbnails where available).
pub fn quick_thumbnail(path: &Path, kind: FileKind, orientation: u8, previews: &[JpegRef]) -> Result<RgbImage> {
    let cands = candidates(path, previews);
    let small: Vec<Candidate> =
        cands.iter().copied().filter(|c| c.jpeg.source == PreviewSource::Thumbnail || c.long_edge() <= 2048).collect();
    let pick = if small.is_empty() { choose(&cands, 160) } else { choose(&small, 160) };
    match pick {
        Some(c) => {
            let bytes = read_range(path, c.jpeg.offset, c.jpeg.len)?;
            let img = decode_jpeg(&bytes, Some(256))?;
            Ok(orient(fit(img, 256), orientation))
        }
        None => load_image(path, kind, orientation, 256, Some(previews)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_scaled_and_orient() {
        let img = RgbImage::from_fn(800, 400, |x, _| image::Rgb([(x % 256) as u8, 0, 0]));
        let jpg = encode_jpeg(&img, 90).unwrap();
        let small = decode_jpeg(&jpg, Some(100)).unwrap();
        // 1/8 scaling of 800x400 gives 100x50.
        assert_eq!(small.dimensions(), (100, 50));
        let rotated = orient(small, 6);
        assert_eq!(rotated.dimensions(), (50, 100));
        let full = decode_jpeg(&jpg, None).unwrap();
        assert_eq!(full.dimensions(), (800, 400));
    }

    #[test]
    fn choose_candidate() {
        let mk = |w, h, s| Candidate { jpeg: JpegRef { offset: w as u64, len: 1000, source: s }, width: w, height: h };
        let c = vec![
            mk(160, 120, PreviewSource::Thumbnail),
            mk(1620, 1080, PreviewSource::Embedded),
            mk(6000, 4000, PreviewSource::Embedded),
        ];
        assert_eq!(choose(&c, 480).unwrap().width, 1620);
        assert_eq!(choose(&c, 3000).unwrap().width, 6000);
        assert_eq!(choose(&c, 9000).unwrap().width, 6000);
        assert_eq!(choose(&c, 100).unwrap().width, 160);
    }
}
