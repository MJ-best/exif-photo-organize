//! A library is a folder (typically on an external drive) holding the organised photos plus a
//! hidden `.photo-organize` folder with the catalog and thumbnails.

use crate::catalog::{Catalog, TagKind, ThumbStore};
use crate::media::FileKind;
use anyhow::{Context, Result};
use std::path::{Component, Path, PathBuf};

pub const DATA_DIR: &str = ".photo-organize";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Library {
    pub root: PathBuf,
}

impl Library {
    /// Open (and create if needed) a library at `root`.
    pub fn open(root: &Path) -> Result<Library> {
        let root = std::path::absolute(root).unwrap_or_else(|_| root.to_path_buf());
        std::fs::create_dir_all(root.join(DATA_DIR)).with_context(|| format!("create {}", root.display()))?;
        let lib = Library { root };
        lib.catalog()?;
        lib.thumbs()?;
        Ok(lib)
    }

    pub fn is_library(root: &Path) -> bool {
        root.join(DATA_DIR).join("catalog.db").exists()
    }

    pub fn name(&self) -> String {
        self.root
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.root.display().to_string())
    }

    pub fn data_dir(&self) -> PathBuf {
        self.root.join(DATA_DIR)
    }
    pub fn catalog_path(&self) -> PathBuf {
        self.data_dir().join("catalog.db")
    }
    pub fn thumbs_path(&self) -> PathBuf {
        self.data_dir().join("thumbs.db")
    }
    pub fn catalog(&self) -> Result<Catalog> {
        Catalog::open(&self.catalog_path())
    }
    pub fn thumbs(&self) -> Result<ThumbStore> {
        ThumbStore::open(&self.thumbs_path())
    }

    /// Absolute path of a library-relative path ('/' separated).
    pub fn abs(&self, rel: &str) -> PathBuf {
        let mut p = self.root.clone();
        for c in rel.split('/') {
            if !c.is_empty() {
                p.push(c);
            }
        }
        p
    }

    /// Library-relative, '/' separated path.
    pub fn rel(&self, p: &Path) -> Option<String> {
        let r = p.strip_prefix(&self.root).ok()?;
        let parts: Vec<String> = r
            .components()
            .filter_map(|c| match c {
                Component::Normal(s) => Some(s.to_string_lossy().into_owned()),
                _ => None,
            })
            .collect();
        Some(parts.join("/"))
    }

    pub fn contains(&self, p: &Path) -> bool {
        let p = std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf());
        p.starts_with(&self.root)
    }

    /// Write the XMP sidecar of an asset (next to its RAW file, or its primary file).
    pub fn write_xmp(&self, catalog: &Catalog, asset_id: i64, rating: u8) -> Result<()> {
        let files = catalog.files_for(asset_id)?;
        let target = files
            .iter()
            .find(|f| f.kind == FileKind::Raw)
            .or_else(|| files.iter().find(|f| f.kind != FileKind::Video))
            .or(files.first());
        let Some(target) = target else { return Ok(()) };
        let keywords = catalog.tag_names_for(asset_id)?;
        crate::xmp::write_sidecar(&self.abs(&target.rel_path), rating, &keywords)
    }

    /// Copy the files of `ids` into `dest` (e.g. to hand over picks). With `jpeg_only`, RAW files
    /// are skipped when the asset also has a JPEG/HEIF. Existing files are never overwritten.
    /// Returns the number of files copied.
    pub fn export(&self, catalog: &Catalog, ids: &[i64], dest: &Path, jpeg_only: bool) -> Result<usize> {
        std::fs::create_dir_all(dest)?;
        let mut n = 0;
        for id in ids {
            let files = catalog.files_for(*id)?;
            let has_dev = files.iter().any(|f| matches!(f.kind, FileKind::Jpeg | FileKind::Heif));
            for f in &files {
                if jpeg_only && has_dev && f.kind == FileKind::Raw {
                    continue;
                }
                let src = self.abs(&f.rel_path);
                let name = src.file_name().map(|s| s.to_os_string()).unwrap_or_default();
                let mut target = dest.join(&name);
                let mut k = 1;
                while target.exists() {
                    let stem =
                        Path::new(&name).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                    let ext =
                        Path::new(&name).extension().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                    target = dest.join(format!("{stem}_{k}.{ext}"));
                    k += 1;
                }
                std::fs::copy(&src, &target).with_context(|| format!("복사 실패 {}", src.display()))?;
                n += 1;
            }
            // Keep ratings/keywords with the exported files.
            if let Some(raw) = files.iter().find(|f| f.kind == FileKind::Raw) {
                let sc = crate::xmp::sidecar_path(&self.abs(&raw.rel_path));
                if sc.exists() && !(jpeg_only && has_dev) {
                    let _ = std::fs::copy(&sc, dest.join(sc.file_name().unwrap_or_default()));
                }
            }
        }
        Ok(n)
    }

    /// Add a user tag and refresh sidecars.
    pub fn tag_assets(&self, catalog: &mut Catalog, ids: &[i64], name: &str) -> Result<crate::catalog::TagInfo> {
        catalog.add_tag(ids, name, TagKind::User)
    }
}
