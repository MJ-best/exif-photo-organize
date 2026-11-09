//! Where imported photos come from. On a computer that is a folder (a card, a drive); on
//! Android it is the Storage Access Framework, which lists the files of a card and hands out
//! open files rather than paths. The import reads its source only through [`SourceFs`], so both
//! work the same way.

use crate::import::SourceFile;
use crate::library::DATA_DIR;
use crate::media::{is_junk_name, FileKind};
use std::collections::HashMap;
use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::UNIX_EPOCH;

use chrono::{DateTime, Local, NaiveDateTime};

pub trait SourceFs: Send + Sync {
    /// Every media file under `root`, recursively.
    fn walk(&self, root: &Path, include_videos: bool, cancel: &AtomicBool) -> (Vec<SourceFile>, Vec<String>);
    fn open(&self, path: &Path) -> io::Result<File>;
    fn is_file(&self, path: &Path) -> bool;
    /// Modification time of a file (the capture date of photos without EXIF).
    fn file_time(&self, path: &Path) -> Option<NaiveDateTime>;
    /// Real paths on this computer (so a library can adopt files that are already inside it).
    fn is_local(&self) -> bool {
        false
    }
}

/// The source of an import; cheap to clone.
#[derive(Clone)]
pub struct Source(pub Arc<dyn SourceFs>);

impl Source {
    pub fn local() -> Source {
        Source(Arc::new(LocalFs))
    }
}

impl Default for Source {
    fn default() -> Self {
        Source::local()
    }
}

impl std::fmt::Debug for Source {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(if self.0.is_local() { "Source(local)" } else { "Source(listed)" })
    }
}

impl std::ops::Deref for Source {
    type Target = dyn SourceFs;
    fn deref(&self) -> &Self::Target {
        &*self.0
    }
}

/// Folders never looked into: the library's own data and the system folders of cards.
pub fn skip_dir(name: &str) -> bool {
    name == DATA_DIR
        || name.eq_ignore_ascii_case("$RECYCLE.BIN")
        || name.eq_ignore_ascii_case("System Volume Information")
}

/// The file system of this computer.
pub struct LocalFs;

impl SourceFs for LocalFs {
    fn walk(&self, root: &Path, include_videos: bool, cancel: &AtomicBool) -> (Vec<SourceFile>, Vec<String>) {
        let mut files = Vec::new();
        let mut errors = Vec::new();
        let mut stack = vec![(root.to_path_buf(), 0usize)];
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
                    if !skip_dir(&name) && depth < 16 {
                        stack.push((path, depth + 1));
                    }
                    continue;
                }
                let Ok(md) = entry.metadata() else { continue };
                let mtime =
                    md.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_secs() as i64);
                if let Some(f) = source_file(path, name, md.len(), mtime, include_videos) {
                    files.push(f);
                }
            }
        }
        (files, errors)
    }

    fn open(&self, path: &Path) -> io::Result<File> {
        File::open(path)
    }

    fn is_file(&self, path: &Path) -> bool {
        path.is_file()
    }

    fn file_time(&self, path: &Path) -> Option<NaiveDateTime> {
        crate::metadata::file_time(path)
    }

    fn is_local(&self) -> bool {
        true
    }
}

fn source_file(path: PathBuf, name: String, size: u64, mtime: Option<i64>, include_videos: bool) -> Option<SourceFile> {
    let kind = FileKind::from_path(&path)?;
    if (kind.is_video() && !include_videos) || size == 0 {
        return None;
    }
    Some(SourceFile { path, name, kind, size, mtime, existing: None })
}

/// One file of a [`ListedFs`].
#[derive(Clone, Debug)]
pub struct ListedFile {
    /// Path relative to the source, '/' separated (e.g. `DCIM/100CANON/IMG_0001.CR3`).
    pub rel: String,
    pub size: u64,
    /// Unix milliseconds.
    pub mtime_ms: Option<i64>,
    /// What the opener needs to open it (on Android, the document's URI).
    pub id: String,
}

pub type Opener = Box<dyn Fn(&str) -> io::Result<File> + Send + Sync>;

/// Files listed by the app, opened one by one through `opener` (Android's Storage Access
/// Framework). Their paths are made up under `root`, which does not exist on disk.
pub struct ListedFs {
    root: PathBuf,
    files: HashMap<PathBuf, ListedFile>,
    opener: Opener,
}

impl ListedFs {
    pub fn new(root: &Path, files: Vec<ListedFile>, opener: Opener) -> ListedFs {
        let files = files.into_iter().map(|f| (join_rel(root, &f.rel), f)).collect();
        ListedFs { root: root.to_path_buf(), files, opener }
    }
}

fn join_rel(root: &Path, rel: &str) -> PathBuf {
    let mut p = root.to_path_buf();
    p.extend(rel.split('/').filter(|c| !c.is_empty() && *c != "." && *c != ".."));
    p
}

impl SourceFs for ListedFs {
    fn walk(&self, root: &Path, include_videos: bool, cancel: &AtomicBool) -> (Vec<SourceFile>, Vec<String>) {
        let mut out = Vec::new();
        for (path, f) in &self.files {
            if cancel.load(Ordering::Relaxed) {
                break;
            }
            if !path.starts_with(root) {
                continue;
            }
            let rel = path.strip_prefix(&self.root).unwrap_or(path);
            let parts: Vec<String> = rel.iter().map(|c| c.to_string_lossy().into_owned()).collect();
            let Some((name, dirs)) = parts.split_last() else { continue };
            if is_junk_name(name) || dirs.iter().any(|d| skip_dir(d) || is_junk_name(d)) || dirs.len() > 16 {
                continue;
            }
            let mtime = f.mtime_ms.map(|ms| ms.div_euclid(1000));
            if let Some(sf) = source_file(path.clone(), name.clone(), f.size, mtime, include_videos) {
                out.push(sf);
            }
        }
        out.sort_by(|a, b| a.path.cmp(&b.path));
        (out, Vec::new())
    }

    fn open(&self, path: &Path) -> io::Result<File> {
        let f = self
            .files
            .get(path)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, format!("{}", path.display())))?;
        (self.opener)(&f.id)
    }

    fn is_file(&self, path: &Path) -> bool {
        self.files.contains_key(path)
    }

    fn file_time(&self, path: &Path) -> Option<NaiveDateTime> {
        let dt: DateTime<Local> = DateTime::from_timestamp_millis(self.files.get(path)?.mtime_ms?)?.into();
        Some(dt.naive_local())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn listed_files_walk_like_a_folder() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.jpg"), b"x").unwrap();
        let real = dir.path().join("a.jpg");
        let files = vec![
            ListedFile { rel: "DCIM/100CANON/IMG_0001.JPG".into(), size: 10, mtime_ms: Some(5_250), id: "1".into() },
            ListedFile { rel: "DCIM/100CANON/IMG_0001.CR3".into(), size: 20, mtime_ms: None, id: "2".into() },
            ListedFile { rel: "DCIM/100CANON/._IMG_0001.JPG".into(), size: 1, mtime_ms: None, id: "3".into() },
            ListedFile { rel: "DCIM/100CANON/notes.txt".into(), size: 3, mtime_ms: None, id: "4".into() },
            ListedFile { rel: "DCIM/100CANON/MVI_0002.MP4".into(), size: 30, mtime_ms: None, id: "5".into() },
            ListedFile { rel: ".photo-organize/thumbs/x.jpg".into(), size: 3, mtime_ms: None, id: "6".into() },
            ListedFile { rel: "DCIM/empty.jpg".into(), size: 0, mtime_ms: None, id: "7".into() },
        ];
        let fs = ListedFs::new(
            Path::new("/card"),
            files,
            Box::new(move |id| {
                assert_eq!(id, "1");
                File::open(&real)
            }),
        );
        let cancel = AtomicBool::new(false);
        let (found, errors) = fs.walk(Path::new("/card"), false, &cancel);
        assert!(errors.is_empty());
        let names: Vec<&str> = found.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["IMG_0001.CR3", "IMG_0001.JPG"]);
        assert_eq!(found[1].path, Path::new("/card/DCIM/100CANON/IMG_0001.JPG"));
        assert_eq!(found[1].mtime, Some(5));
        let t = fs.file_time(&found[1].path).unwrap();
        assert_eq!(t.and_utc().timestamp_subsec_millis(), 250, "milliseconds kept");
        assert_eq!(fs.walk(Path::new("/card"), true, &cancel).0.len(), 3, "the video too");
        assert!(fs.is_file(Path::new("/card/DCIM/100CANON/IMG_0001.JPG")));
        assert!(!fs.is_file(Path::new("/card/DCIM/100CANON/IMG_0009.JPG")));
        assert!(fs.open(Path::new("/card/DCIM/100CANON/IMG_0001.JPG")).is_ok());
        assert!(fs.open(Path::new("/card/nope.jpg")).is_err());
    }
}
