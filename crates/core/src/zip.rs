//! A minimal ZIP writer (stored entries, no compression) for handing exported photos over as one
//! file. JPEG and RAW data hardly compress, so storing is as small as deflating and much faster.
//! Names are flagged as UTF-8 (Korean file names open correctly on macOS, Windows and Linux).
//! Archives stay below 4 GiB, so no ZIP64 is needed: callers split before [`MAX_BYTES`].

use anyhow::{bail, Context, Result};
use chrono::{Datelike, NaiveDateTime, Timelike};
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

/// Largest archive we write (well below the 4 GiB limit of plain ZIP).
pub const MAX_BYTES: u64 = 3_500_000_000;

const LOCAL_HEADER: u64 = 30;
const CENTRAL_HEADER: u64 = 46;
const END_RECORD: u64 = 22;

struct Entry {
    name: Vec<u8>,
    crc: u32,
    size: u32,
    offset: u32,
    time: u16,
    date: u16,
}

pub struct ZipWriter {
    out: BufWriter<File>,
    tmp: PathBuf,
    path: PathBuf,
    entries: Vec<Entry>,
    offset: u64,
}

impl ZipWriter {
    /// Start writing `path` (via a temporary file that becomes `path` on [`finish`]).
    pub fn create(path: &Path) -> Result<ZipWriter> {
        let tmp = path.with_file_name(format!(".{}.part", path.file_name().unwrap_or_default().to_string_lossy()));
        let f = File::create(&tmp).with_context(|| format!("ZIP 을 만들 수 없습니다: {}", tmp.display()))?;
        Ok(ZipWriter {
            out: BufWriter::with_capacity(1 << 20, f),
            tmp,
            path: path.to_path_buf(),
            entries: Vec::new(),
            offset: 0,
        })
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Size of the finished archive if nothing more were added.
    pub fn size(&self) -> u64 {
        self.offset + self.entries.iter().map(|e| CENTRAL_HEADER + e.name.len() as u64).sum::<u64>() + END_RECORD
    }

    /// Bytes an entry adds to the archive.
    pub fn entry_cost(name: &str, len: u64) -> u64 {
        LOCAL_HEADER + CENTRAL_HEADER + 2 * name.len() as u64 + len
    }

    pub fn add(&mut self, name: &str, data: &[u8], modified: Option<NaiveDateTime>) -> Result<()> {
        if self.size() + Self::entry_cost(name, data.len() as u64) > MAX_BYTES {
            bail!("ZIP 이 너무 큽니다");
        }
        if self.entries.len() >= u16::MAX as usize {
            bail!("ZIP 항목이 너무 많습니다");
        }
        let (time, date) = dos_time(modified);
        let crc = crc32fast::hash(data);
        let e = Entry {
            name: name.as_bytes().to_vec(),
            crc,
            size: data.len() as u32,
            offset: self.offset as u32,
            time,
            date,
        };
        let mut h = Vec::with_capacity(LOCAL_HEADER as usize + e.name.len());
        h.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        h.extend_from_slice(&10u16.to_le_bytes()); // version needed (stored)
        h.extend_from_slice(&0x0800u16.to_le_bytes()); // UTF-8 names
        h.extend_from_slice(&0u16.to_le_bytes()); // stored
        h.extend_from_slice(&e.time.to_le_bytes());
        h.extend_from_slice(&e.date.to_le_bytes());
        h.extend_from_slice(&e.crc.to_le_bytes());
        h.extend_from_slice(&e.size.to_le_bytes());
        h.extend_from_slice(&e.size.to_le_bytes());
        h.extend_from_slice(&(e.name.len() as u16).to_le_bytes());
        h.extend_from_slice(&0u16.to_le_bytes());
        h.extend_from_slice(&e.name);
        self.out.write_all(&h)?;
        self.out.write_all(data)?;
        self.offset += h.len() as u64 + data.len() as u64;
        self.entries.push(e);
        Ok(())
    }

    pub fn finish(mut self) -> Result<PathBuf> {
        let cd_start = self.offset;
        let mut cd = Vec::new();
        for e in &self.entries {
            cd.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
            cd.extend_from_slice(&(0x0300u16 | 20).to_le_bytes()); // made by: Unix, 2.0
            cd.extend_from_slice(&10u16.to_le_bytes());
            cd.extend_from_slice(&0x0800u16.to_le_bytes());
            cd.extend_from_slice(&0u16.to_le_bytes());
            cd.extend_from_slice(&e.time.to_le_bytes());
            cd.extend_from_slice(&e.date.to_le_bytes());
            cd.extend_from_slice(&e.crc.to_le_bytes());
            cd.extend_from_slice(&e.size.to_le_bytes());
            cd.extend_from_slice(&e.size.to_le_bytes());
            cd.extend_from_slice(&(e.name.len() as u16).to_le_bytes());
            cd.extend_from_slice(&0u16.to_le_bytes()); // extra
            cd.extend_from_slice(&0u16.to_le_bytes()); // comment
            cd.extend_from_slice(&0u16.to_le_bytes()); // disk
            cd.extend_from_slice(&0u16.to_le_bytes()); // internal attributes
            cd.extend_from_slice(&((0o100644u32) << 16).to_le_bytes()); // regular file, rw-r--r--
            cd.extend_from_slice(&e.offset.to_le_bytes());
            cd.extend_from_slice(&e.name);
        }
        let n = self.entries.len() as u16;
        let mut end = Vec::with_capacity(END_RECORD as usize);
        end.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
        end.extend_from_slice(&0u16.to_le_bytes());
        end.extend_from_slice(&0u16.to_le_bytes());
        end.extend_from_slice(&n.to_le_bytes());
        end.extend_from_slice(&n.to_le_bytes());
        end.extend_from_slice(&(cd.len() as u32).to_le_bytes());
        end.extend_from_slice(&(cd_start as u32).to_le_bytes());
        end.extend_from_slice(&0u16.to_le_bytes());
        self.out.write_all(&cd)?;
        self.out.write_all(&end)?;
        let f = self.out.into_inner().map_err(|e| e.into_error())?;
        f.sync_all().ok();
        drop(f);
        std::fs::rename(&self.tmp, &self.path).context("ZIP 이름 바꾸기 실패")?;
        Ok(self.path)
    }

    /// Give up: remove the partial archive.
    pub fn abandon(self) {
        let ZipWriter { out, tmp, .. } = self;
        drop(out);
        let _ = std::fs::remove_file(tmp);
    }
}

fn dos_time(t: Option<NaiveDateTime>) -> (u16, u16) {
    let t = t.unwrap_or_else(|| chrono::Local::now().naive_local());
    let year = t.year().clamp(1980, 2107) as u16;
    let time = ((t.hour() as u16) << 11) | ((t.minute() as u16) << 5) | (t.second() as u16 / 2);
    let date = ((year - 1980) << 9) | ((t.month() as u16) << 5) | t.day() as u16;
    (time, date)
}

/// Read back a ZIP written by [`ZipWriter`] (names and contents), checking every CRC. For tests
/// and for verifying an export.
pub fn read_stored(bytes: &[u8]) -> Result<Vec<(String, Vec<u8>)>> {
    let end = bytes.len().checked_sub(END_RECORD as usize).context("too short")?;
    if bytes[end..end + 4] != 0x0605_4b50u32.to_le_bytes() {
        bail!("no end record");
    }
    let n = u16::from_le_bytes([bytes[end + 10], bytes[end + 11]]) as usize;
    let mut p = u32::from_le_bytes(bytes[end + 16..end + 20].try_into()?) as usize;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        if bytes[p..p + 4] != 0x0201_4b50u32.to_le_bytes() {
            bail!("bad central header");
        }
        let crc = u32::from_le_bytes(bytes[p + 16..p + 20].try_into()?);
        let size = u32::from_le_bytes(bytes[p + 24..p + 28].try_into()?) as usize;
        let name_len = u16::from_le_bytes([bytes[p + 28], bytes[p + 29]]) as usize;
        let local = u32::from_le_bytes(bytes[p + 42..p + 46].try_into()?) as usize;
        let name = String::from_utf8(bytes[p + 46..p + 46 + name_len].to_vec())?;
        let lname = u16::from_le_bytes([bytes[local + 26], bytes[local + 27]]) as usize;
        let data_start = local + LOCAL_HEADER as usize + lname;
        let data = bytes[data_start..data_start + size].to_vec();
        if crc32fast::hash(&data) != crc {
            bail!("CRC mismatch in {name}");
        }
        out.push((name, data));
        p += CENTRAL_HEADER as usize + name_len;
    }
    Ok(out)
}
