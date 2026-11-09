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
    /// Where deleted photos go on systems without a trash for apps (Android).
    pub fn trash_dir(&self) -> PathBuf {
        self.data_dir().join("trash")
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

    /// Write the XMP sidecar of an asset (next to its RAW file, or its primary file) with its
    /// current rating (rejected photos get -1, the Lightroom convention) and tags.
    pub fn write_xmp(&self, catalog: &Catalog, asset_id: i64) -> Result<()> {
        let (rating, flag) = catalog.rating_and_flag(asset_id)?;
        let rating: i8 = if flag < 0 { -1 } else { rating as i8 };
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

    /// Add a user tag and refresh sidecars.
    pub fn tag_assets(&self, catalog: &mut Catalog, ids: &[i64], name: &str) -> Result<crate::catalog::TagInfo> {
        catalog.add_tag(ids, name, TagKind::User)
    }
}
