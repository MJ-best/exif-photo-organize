//! File classification by extension.

use serde::{Deserialize, Serialize};
use std::path::Path;

/// Kind of a single file on disk.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
pub enum FileKind {
    Raw,
    Jpeg,
    Heif,
    Png,
    Tiff,
    Video,
}

/// Camera RAW extensions we recognise (lower-case).
pub const RAW_EXTS: &[&str] = &[
    "cr2", "cr3", "crw", "nef", "nrw", "arw", "srf", "sr2", "raf", "orf", "rw2", "dng", "pef", "srw", "x3f", "3fr",
    "iiq", "rwl", "erf", "mef", "mos", "mrw", "kdc", "dcr", "gpr", "raw",
];
pub const JPEG_EXTS: &[&str] = &["jpg", "jpeg", "jpe"];
pub const HEIF_EXTS: &[&str] = &["heic", "heif", "hif", "avif"];
pub const VIDEO_EXTS: &[&str] = &["mp4", "mov", "m4v", "avi", "mts", "m2ts", "mxf", "3gp", "insv"];

impl FileKind {
    pub fn from_path(path: &Path) -> Option<FileKind> {
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        Self::from_ext(&ext)
    }

    pub fn from_ext(ext: &str) -> Option<FileKind> {
        let ext = ext.to_ascii_lowercase();
        let e = ext.as_str();
        if JPEG_EXTS.contains(&e) {
            Some(FileKind::Jpeg)
        } else if RAW_EXTS.contains(&e) {
            Some(FileKind::Raw)
        } else if HEIF_EXTS.contains(&e) {
            Some(FileKind::Heif)
        } else if e == "png" {
            Some(FileKind::Png)
        } else if e == "tif" || e == "tiff" {
            Some(FileKind::Tiff)
        } else if VIDEO_EXTS.contains(&e) {
            Some(FileKind::Video)
        } else {
            None
        }
    }

    /// Stable identifier stored in the catalog.
    pub fn role(self) -> &'static str {
        match self {
            FileKind::Raw => "raw",
            FileKind::Jpeg => "jpeg",
            FileKind::Heif => "heif",
            FileKind::Png => "png",
            FileKind::Tiff => "tiff",
            FileKind::Video => "video",
        }
    }

    pub fn from_role(role: &str) -> Option<FileKind> {
        Some(match role {
            "raw" => FileKind::Raw,
            "jpeg" => FileKind::Jpeg,
            "heif" => FileKind::Heif,
            "png" => FileKind::Png,
            "tiff" => FileKind::Tiff,
            "video" => FileKind::Video,
            _ => return None,
        })
    }

    pub fn format_flag(self) -> u8 {
        match self {
            FileKind::Raw => formats::RAW,
            FileKind::Jpeg => formats::JPEG,
            FileKind::Heif => formats::HEIF,
            FileKind::Video => formats::VIDEO,
            FileKind::Png | FileKind::Tiff => formats::OTHER,
        }
    }

    /// Preference when choosing which file of a group to read metadata / previews from.
    /// Lower is better. Camera JPEGs are fastest to decode, RAWs hold the most reliable EXIF.
    pub fn preview_priority(self) -> u8 {
        match self {
            FileKind::Jpeg => 0,
            FileKind::Raw => 1,
            FileKind::Heif => 2,
            FileKind::Png => 3,
            FileKind::Tiff => 4,
            FileKind::Video => 5,
        }
    }

    pub fn is_video(self) -> bool {
        self == FileKind::Video
    }
}

/// Bit flags describing which file formats an asset holds.
pub mod formats {
    pub const RAW: u8 = 1;
    pub const JPEG: u8 = 2;
    pub const HEIF: u8 = 4;
    pub const VIDEO: u8 = 8;
    pub const OTHER: u8 = 16;

    pub fn label(flags: u8) -> &'static str {
        let raw = flags & RAW != 0;
        let jpg = flags & JPEG != 0;
        let heif = flags & HEIF != 0;
        if flags & VIDEO != 0 {
            "동영상"
        } else if raw && jpg {
            "RAW+JPEG"
        } else if raw && heif {
            "RAW+HEIF"
        } else if raw {
            "RAW"
        } else if jpg {
            "JPEG"
        } else if heif {
            "HEIF"
        } else {
            "기타"
        }
    }
}

/// Files the camera / OS leaves on cards that are never interesting.
pub fn is_junk_name(name: &str) -> bool {
    name.starts_with("._") || name.starts_with('.') || name.eq_ignore_ascii_case("Thumbs.db")
}

/// Upper-case extension, e.g. "CR3".
pub fn ext_upper(path: &Path) -> String {
    path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_uppercase()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify() {
        assert_eq!(FileKind::from_path(Path::new("a/IMG_0001.CR3")), Some(FileKind::Raw));
        assert_eq!(FileKind::from_path(Path::new("DSC0001.JPG")), Some(FileKind::Jpeg));
        assert_eq!(FileKind::from_path(Path::new("C0001.MP4")), Some(FileKind::Video));
        assert_eq!(FileKind::from_path(Path::new("x.HIF")), Some(FileKind::Heif));
        assert_eq!(FileKind::from_path(Path::new("readme.txt")), None);
        assert!(is_junk_name("._IMG_0001.JPG"));
        assert_eq!(formats::label(formats::RAW | formats::JPEG), "RAW+JPEG");
    }
}
