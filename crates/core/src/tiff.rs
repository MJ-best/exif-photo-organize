//! Minimal, seek-based TIFF / EXIF IFD reader.
//!
//! RAW files are TIFF containers that can be 20–100 MB large. Generic EXIF crates read the whole
//! file into memory, which is far too slow when scanning thousands of files on an SD card.
//! This reader only touches the bytes it needs: the header, the IFD tables and the values we ask for.

use anyhow::{bail, Result};
use std::io::{Read, Seek, SeekFrom};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Endian {
    Little,
    Big,
}

impl Endian {
    #[inline]
    pub fn u16(self, b: &[u8]) -> u16 {
        let a = [b[0], b[1]];
        match self {
            Endian::Little => u16::from_le_bytes(a),
            Endian::Big => u16::from_be_bytes(a),
        }
    }
    #[inline]
    pub fn u32(self, b: &[u8]) -> u32 {
        let a = [b[0], b[1], b[2], b[3]];
        match self {
            Endian::Little => u32::from_le_bytes(a),
            Endian::Big => u32::from_be_bytes(a),
        }
    }
    #[inline]
    pub fn u64(self, b: &[u8]) -> u64 {
        let mut a = [0u8; 8];
        a.copy_from_slice(&b[..8]);
        match self {
            Endian::Little => u64::from_le_bytes(a),
            Endian::Big => u64::from_be_bytes(a),
        }
    }
}

/// TIFF field types.
pub mod ty {
    pub const BYTE: u16 = 1;
    pub const ASCII: u16 = 2;
    pub const SHORT: u16 = 3;
    pub const LONG: u16 = 4;
    pub const RATIONAL: u16 = 5;
    pub const SBYTE: u16 = 6;
    pub const UNDEFINED: u16 = 7;
    pub const SSHORT: u16 = 8;
    pub const SLONG: u16 = 9;
    pub const SRATIONAL: u16 = 10;
    pub const FLOAT: u16 = 11;
    pub const DOUBLE: u16 = 12;
    pub const IFD: u16 = 13;
}

fn type_size(t: u16) -> Option<u32> {
    Some(match t {
        1 | 2 | 6 | 7 => 1,
        3 | 8 => 2,
        4 | 9 | 11 | 13 => 4,
        5 | 10 | 12 => 8,
        _ => return None,
    })
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub tag: u16,
    pub typ: u16,
    pub count: u32,
    /// The 4 raw bytes of the value/offset field.
    pub raw: [u8; 4],
}

#[derive(Clone, Debug, Default)]
pub struct Ifd {
    pub entries: Vec<Entry>,
    pub next: u32,
}

impl Ifd {
    pub fn get(&self, tag: u16) -> Option<&Entry> {
        self.entries.iter().find(|e| e.tag == tag)
    }
}

/// Reader for a TIFF structure that starts at `base` within `r`.
pub struct TiffReader<R> {
    r: R,
    base: u64,
    pub endian: Endian,
}

const MAX_VALUE_BYTES: u64 = 4 << 20;

impl<R: Read + Seek> TiffReader<R> {
    /// Parse the TIFF header at `base`. Returns the reader and the offset of IFD0.
    ///
    /// Accepts regular TIFF (42) and the variants used by Panasonic (0x55) and Olympus (ORF).
    pub fn new(mut r: R, base: u64) -> Result<(Self, u32)> {
        let mut h = [0u8; 8];
        r.seek(SeekFrom::Start(base))?;
        r.read_exact(&mut h)?;
        let endian = match &h[0..2] {
            b"II" => Endian::Little,
            b"MM" => Endian::Big,
            _ => bail!("not a TIFF header"),
        };
        let magic = endian.u16(&h[2..4]);
        if !matches!(magic, 42 | 0x55 | 0x4F52 | 0x5352) {
            bail!("unsupported TIFF magic {magic:#x}");
        }
        let first = endian.u32(&h[4..8]);
        Ok((TiffReader { r, base, endian }, first))
    }

    pub fn base(&self) -> u64 {
        self.base
    }

    pub fn inner(&mut self) -> &mut R {
        &mut self.r
    }

    pub fn read_ifd(&mut self, offset: u32) -> Result<Ifd> {
        if offset == 0 {
            bail!("null IFD offset");
        }
        self.r.seek(SeekFrom::Start(self.base + offset as u64))?;
        let mut n = [0u8; 2];
        self.r.read_exact(&mut n)?;
        let count = self.endian.u16(&n) as usize;
        if count == 0 || count > 1000 {
            bail!("implausible IFD entry count {count}");
        }
        let mut buf = vec![0u8; count * 12 + 4];
        // Some writers omit the trailing next-IFD pointer at the end of the file.
        let got = read_up_to(&mut self.r, &mut buf)?;
        if got < count * 12 {
            bail!("truncated IFD");
        }
        let mut entries = Vec::with_capacity(count);
        for i in 0..count {
            let e = &buf[i * 12..i * 12 + 12];
            entries.push(Entry {
                tag: self.endian.u16(&e[0..2]),
                typ: self.endian.u16(&e[2..4]),
                count: self.endian.u32(&e[4..8]),
                raw: [e[8], e[9], e[10], e[11]],
            });
        }
        let next = if got >= count * 12 + 4 { self.endian.u32(&buf[count * 12..]) } else { 0 };
        Ok(Ifd { entries, next })
    }

    /// Absolute position (within the underlying reader) of the value of `e`,
    /// or `None` if the value is stored inline in the entry.
    pub fn value_pos(&self, e: &Entry) -> Option<u64> {
        let size = type_size(e.typ)? as u64 * e.count as u64;
        if size <= 4 {
            None
        } else {
            Some(self.base + self.endian.u32(&e.raw) as u64)
        }
    }

    /// Raw value bytes of an entry.
    pub fn bytes(&mut self, e: &Entry) -> Result<Vec<u8>> {
        let Some(sz) = type_size(e.typ) else { bail!("unknown type {}", e.typ) };
        let size = sz as u64 * e.count as u64;
        if size <= 4 {
            return Ok(e.raw[..size as usize].to_vec());
        }
        if size > MAX_VALUE_BYTES {
            bail!("value too large");
        }
        let off = self.endian.u32(&e.raw) as u64;
        self.r.seek(SeekFrom::Start(self.base + off))?;
        let mut v = vec![0u8; size as usize];
        self.r.read_exact(&mut v)?;
        Ok(v)
    }

    /// All values as unsigned integers (BYTE / SHORT / LONG / IFD).
    pub fn uints(&mut self, e: &Entry) -> Vec<u32> {
        let Ok(b) = self.bytes(e) else { return Vec::new() };
        let en = self.endian;
        match e.typ {
            ty::BYTE | ty::UNDEFINED => b.iter().map(|&x| x as u32).collect(),
            ty::SHORT => b.chunks_exact(2).map(|c| en.u16(c) as u32).collect(),
            ty::LONG | ty::IFD => b.chunks_exact(4).map(|c| en.u32(c)).collect(),
            ty::SSHORT => b.chunks_exact(2).map(|c| (en.u16(c) as i16).max(0) as u32).collect(),
            ty::SLONG => b.chunks_exact(4).map(|c| (en.u32(c) as i32).max(0) as u32).collect(),
            _ => Vec::new(),
        }
    }

    pub fn uint(&mut self, e: &Entry) -> Option<u32> {
        self.uints(e).into_iter().next()
    }

    /// All values as floating point numbers (handles every numeric type).
    pub fn floats(&mut self, e: &Entry) -> Vec<f64> {
        let Ok(b) = self.bytes(e) else { return Vec::new() };
        let en = self.endian;
        match e.typ {
            ty::RATIONAL => b
                .chunks_exact(8)
                .map(|c| {
                    let n = en.u32(&c[0..4]) as f64;
                    let d = en.u32(&c[4..8]) as f64;
                    if d == 0.0 {
                        f64::NAN
                    } else {
                        n / d
                    }
                })
                .collect(),
            ty::SRATIONAL => b
                .chunks_exact(8)
                .map(|c| {
                    let n = en.u32(&c[0..4]) as i32 as f64;
                    let d = en.u32(&c[4..8]) as i32 as f64;
                    if d == 0.0 {
                        f64::NAN
                    } else {
                        n / d
                    }
                })
                .collect(),
            ty::FLOAT => b.chunks_exact(4).map(|c| f32::from_bits(en.u32(c)) as f64).collect(),
            ty::DOUBLE => b.chunks_exact(8).map(|c| f64::from_bits(en.u64(c))).collect(),
            ty::SBYTE => b.iter().map(|&x| x as i8 as f64).collect(),
            ty::SSHORT => b.chunks_exact(2).map(|c| en.u16(c) as i16 as f64).collect(),
            ty::SLONG => b.chunks_exact(4).map(|c| en.u32(c) as i32 as f64).collect(),
            _ => self.uints(e).into_iter().map(|v| v as f64).collect(),
        }
    }

    pub fn float(&mut self, e: &Entry) -> Option<f64> {
        self.floats(e).into_iter().next().filter(|v| v.is_finite())
    }

    /// ASCII value, trimmed of NULs and whitespace. Empty strings become `None`.
    pub fn ascii(&mut self, e: &Entry) -> Option<String> {
        let b = self.bytes(e).ok()?;
        let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
        let s = String::from_utf8_lossy(&b[..end]).trim().to_string();
        if s.is_empty() {
            None
        } else {
            Some(s)
        }
    }

    pub fn get_ascii(&mut self, ifd: &Ifd, tag: u16) -> Option<String> {
        let e = ifd.get(tag)?.clone();
        self.ascii(&e)
    }
    pub fn get_uint(&mut self, ifd: &Ifd, tag: u16) -> Option<u32> {
        let e = ifd.get(tag)?.clone();
        self.uint(&e)
    }
    pub fn get_uints(&mut self, ifd: &Ifd, tag: u16) -> Vec<u32> {
        match ifd.get(tag).cloned() {
            Some(e) => self.uints(&e),
            None => Vec::new(),
        }
    }
    pub fn get_float(&mut self, ifd: &Ifd, tag: u16) -> Option<f64> {
        let e = ifd.get(tag)?.clone();
        self.float(&e)
    }
    pub fn get_floats(&mut self, ifd: &Ifd, tag: u16) -> Vec<f64> {
        match ifd.get(tag).cloned() {
            Some(e) => self.floats(&e),
            None => Vec::new(),
        }
    }
}

/// Read as many bytes as available into `buf` (stops at EOF).
pub fn read_up_to<R: Read>(r: &mut R, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut total = 0;
    while total < buf.len() {
        match r.read(&mut buf[total..]) {
            Ok(0) => break,
            Ok(n) => total += n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
    Ok(total)
}

/// Well-known tag numbers.
pub mod tag {
    // IFD0 / image
    pub const NEW_SUBFILE_TYPE: u16 = 0x00FE;
    pub const IMAGE_WIDTH: u16 = 0x0100;
    pub const IMAGE_LENGTH: u16 = 0x0101;
    pub const BITS_PER_SAMPLE: u16 = 0x0102;
    pub const COMPRESSION: u16 = 0x0103;
    pub const PHOTOMETRIC: u16 = 0x0106;
    pub const MAKE: u16 = 0x010F;
    pub const MODEL: u16 = 0x0110;
    pub const STRIP_OFFSETS: u16 = 0x0111;
    pub const ORIENTATION: u16 = 0x0112;
    pub const STRIP_BYTE_COUNTS: u16 = 0x0117;
    pub const SOFTWARE: u16 = 0x0131;
    pub const DATE_TIME: u16 = 0x0132;
    pub const ARTIST: u16 = 0x013B;
    pub const SUB_IFDS: u16 = 0x014A;
    pub const JPEG_IF_OFFSET: u16 = 0x0201;
    pub const JPEG_IF_LENGTH: u16 = 0x0202;
    pub const RATING: u16 = 0x4746;
    pub const RATING_PERCENT: u16 = 0x4749;
    pub const EXIF_IFD: u16 = 0x8769;
    pub const GPS_IFD: u16 = 0x8825;
    /// Panasonic RW2: embedded full-size JPEG.
    pub const PANASONIC_JPG_FROM_RAW: u16 = 0x002E;
    /// Panasonic RW2: ISO in IFD0.
    pub const PANASONIC_ISO: u16 = 0x0017;
    pub const DNG_VERSION: u16 = 0xC612;
    pub const UNIQUE_CAMERA_MODEL: u16 = 0xC614;

    // EXIF IFD
    pub const EXPOSURE_TIME: u16 = 0x829A;
    pub const F_NUMBER: u16 = 0x829D;
    pub const EXPOSURE_PROGRAM: u16 = 0x8822;
    pub const ISO: u16 = 0x8827;
    pub const RECOMMENDED_EXPOSURE_INDEX: u16 = 0x8832;
    pub const DATE_TIME_ORIGINAL: u16 = 0x9003;
    pub const DATE_TIME_DIGITIZED: u16 = 0x9004;
    pub const OFFSET_TIME_ORIGINAL: u16 = 0x9011;
    pub const OFFSET_TIME: u16 = 0x9010;
    pub const EXPOSURE_BIAS: u16 = 0x9204;
    pub const METERING_MODE: u16 = 0x9207;
    pub const FLASH: u16 = 0x9209;
    pub const FOCAL_LENGTH: u16 = 0x920A;
    pub const SUBSEC_TIME_ORIGINAL: u16 = 0x9291;
    pub const PIXEL_X: u16 = 0xA002;
    pub const PIXEL_Y: u16 = 0xA003;
    pub const WHITE_BALANCE: u16 = 0xA403;
    pub const FOCAL_35: u16 = 0xA405;
    pub const BODY_SERIAL: u16 = 0xA431;
    pub const LENS_SPEC: u16 = 0xA432;
    pub const LENS_MAKE: u16 = 0xA433;
    pub const LENS_MODEL: u16 = 0xA434;

    // GPS IFD
    pub const GPS_LAT_REF: u16 = 1;
    pub const GPS_LAT: u16 = 2;
    pub const GPS_LON_REF: u16 = 3;
    pub const GPS_LON: u16 = 4;
    pub const GPS_ALT_REF: u16 = 5;
    pub const GPS_ALT: u16 = 6;
}

#[cfg(test)]
pub(crate) mod testutil {
    //! A tiny TIFF writer used to build fixtures in tests.

    pub enum Val {
        Ascii(&'static str),
        Short(Vec<u16>),
        Long(Vec<u32>),
        Rational(Vec<(u32, u32)>),
        SRational(Vec<(i32, i32)>),
        /// Pointer to a nested IFD (written after the parent).
        Ifd(Vec<(u16, Val)>),
        #[allow(dead_code)]
        Undefined(Vec<u8>),
    }

    /// Build a little-endian TIFF with a single IFD0 (and nested IFDs). `extra` is appended after
    /// the IFD data and its offset is returned so tests can point tags at it.
    pub fn build_tiff(ifd0: Vec<(u16, Val)>) -> Vec<u8> {
        let mut out = b"II\x2a\x00\x08\x00\x00\x00".to_vec();
        write_ifd(&mut out, ifd0);
        out
    }

    fn write_ifd(out: &mut Vec<u8>, mut entries: Vec<(u16, Val)>) -> u32 {
        entries.sort_by_key(|e| e.0);
        let start = out.len() as u32;
        let n = entries.len();
        out.extend_from_slice(&(n as u16).to_le_bytes());
        let table_pos = out.len();
        out.resize(table_pos + n * 12 + 4, 0);
        let mut nested = Vec::new();
        for (i, (tag, val)) in entries.into_iter().enumerate() {
            let (typ, count, data): (u16, u32, Vec<u8>) = match val {
                Val::Ascii(s) => {
                    let mut d = s.as_bytes().to_vec();
                    d.push(0);
                    (2, d.len() as u32, d)
                }
                Val::Short(v) => (3, v.len() as u32, v.iter().flat_map(|x| x.to_le_bytes()).collect()),
                Val::Long(v) => (4, v.len() as u32, v.iter().flat_map(|x| x.to_le_bytes()).collect()),
                Val::Rational(v) => (
                    5,
                    v.len() as u32,
                    v.iter().flat_map(|(a, b)| a.to_le_bytes().into_iter().chain(b.to_le_bytes())).collect(),
                ),
                Val::SRational(v) => (
                    10,
                    v.len() as u32,
                    v.iter().flat_map(|(a, b)| a.to_le_bytes().into_iter().chain(b.to_le_bytes())).collect(),
                ),
                Val::Undefined(v) => (7, v.len() as u32, v),
                Val::Ifd(sub) => {
                    nested.push((i, sub));
                    (4, 1, vec![0, 0, 0, 0])
                }
            };
            let p = table_pos + i * 12;
            out[p..p + 2].copy_from_slice(&tag.to_le_bytes());
            out[p + 2..p + 4].copy_from_slice(&typ.to_le_bytes());
            out[p + 4..p + 8].copy_from_slice(&count.to_le_bytes());
            if data.len() <= 4 {
                out[p + 8..p + 8 + data.len()].copy_from_slice(&data);
            } else {
                let off = out.len() as u32;
                out.extend_from_slice(&data);
                if out.len() % 2 == 1 {
                    out.push(0);
                }
                out[p + 8..p + 12].copy_from_slice(&off.to_le_bytes());
            }
        }
        for (i, sub) in nested {
            let off = write_ifd(out, sub);
            let p = table_pos + i * 12;
            out[p + 8..p + 12].copy_from_slice(&off.to_le_bytes());
        }
        start
    }
}

#[cfg(test)]
mod tests {
    use super::testutil::*;
    use super::*;
    use std::io::Cursor;

    #[test]
    fn parse_basic() {
        let data = build_tiff(vec![
            (tag::MAKE, Val::Ascii("Canon")),
            (tag::MODEL, Val::Ascii("Canon EOS R5")),
            (tag::ORIENTATION, Val::Short(vec![6])),
            (
                tag::EXIF_IFD,
                Val::Ifd(vec![
                    (tag::F_NUMBER, Val::Rational(vec![(28, 10)])),
                    (tag::EXPOSURE_BIAS, Val::SRational(vec![(-2, 3)])),
                ]),
            ),
        ]);
        let (mut t, first) = TiffReader::new(Cursor::new(data), 0).unwrap();
        let ifd0 = t.read_ifd(first).unwrap();
        assert_eq!(t.get_ascii(&ifd0, tag::MAKE).as_deref(), Some("Canon"));
        assert_eq!(t.get_uint(&ifd0, tag::ORIENTATION), Some(6));
        let exif_off = t.get_uint(&ifd0, tag::EXIF_IFD).unwrap();
        let exif = t.read_ifd(exif_off).unwrap();
        assert!((t.get_float(&exif, tag::F_NUMBER).unwrap() - 2.8).abs() < 1e-6);
        assert!((t.get_float(&exif, tag::EXPOSURE_BIAS).unwrap() + 0.6667).abs() < 1e-3);
    }
}
