//! macOS integration for LOUPE.
//!
//! The Mac App Store build runs in the App Sandbox: it may only use folders the person chose in a
//! folder panel, and only until it quits. [`remember`] keeps that permission (a security-scoped
//! bookmark in the config folder) and [`restore`] takes it up again at the next start. Opening a
//! folder in Finder and ejecting a card go through `NSWorkspace` rather than helper programs, which
//! the sandbox does not allow.
//!
//! Everything here does nothing outside the sandbox and on other systems, so the website build
//! keeps behaving as before.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Running inside the App Sandbox (the Mac App Store build).
pub fn is_sandboxed() -> bool {
    cfg!(target_os = "macos") && std::env::var_os("APP_SANDBOX_CONTAINER_ID").is_some()
}

/// Keep access to a folder the person has just chosen in a folder panel, so that it can be
/// opened again after a restart (sandbox only).
pub fn remember(path: &Path) {
    #[cfg(target_os = "macos")]
    if is_sandboxed() {
        imp::remember(path);
    }
    #[cfg(not(target_os = "macos"))]
    let _ = path;
}

/// Take up access again to every remembered folder that is reachable now — at start, and when a
/// drive is connected. Returns how many folders became accessible.
pub fn restore() -> usize {
    #[cfg(target_os = "macos")]
    if is_sandboxed() {
        return imp::restore();
    }
    0
}

/// The folder can be listed (in the sandbox: it was chosen or remembered).
pub fn can_read(path: &Path) -> bool {
    std::fs::read_dir(path).is_ok()
}

/// Show a folder in Finder through `NSWorkspace`. `false` when not handled here (other systems,
/// or outside the sandbox where the usual way works).
pub fn open_folder(path: &Path) -> bool {
    #[cfg(target_os = "macos")]
    if is_sandboxed() {
        return imp::open(path);
    }
    let _ = path;
    false
}

/// Eject a volume through `NSWorkspace`. `None` when not handled here.
pub fn eject(path: &Path) -> Option<Result<(), String>> {
    #[cfg(target_os = "macos")]
    if is_sandboxed() {
        return Some(imp::eject(path));
    }
    let _ = path;
    None
}

// ---- the remembered folders (config folder / bookmarks.json) ----

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Entry {
    path: PathBuf,
    /// Bookmark data, hex.
    data: String,
}

/// Remembered folders kept at most (the oldest go first).
const KEEP: usize = 40;

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
mod store {
    use super::{Entry, KEEP};
    use std::path::{Path, PathBuf};

    fn file() -> Option<PathBuf> {
        Some(dirs::config_dir()?.join("photo-organize").join("bookmarks.json"))
    }

    pub fn load() -> Vec<Entry> {
        file().map(|f| load_from(&f)).unwrap_or_default()
    }

    pub fn save(entries: &[Entry]) {
        if let Some(f) = file() {
            save_to(&f, entries);
        }
    }

    pub fn load_from(file: &Path) -> Vec<Entry> {
        std::fs::read(file).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    pub fn save_to(file: &Path, entries: &[Entry]) {
        if let Some(dir) = file.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let tmp = file.with_extension("json.tmp");
        let ok = serde_json::to_vec_pretty(entries)
            .ok()
            .is_some_and(|b| std::fs::write(&tmp, b).is_ok() && std::fs::rename(&tmp, file).is_ok());
        if !ok {
            log::warn!("could not save {}", file.display());
        }
    }

    /// Add or replace the entry for `path` (newest last).
    pub fn put(entries: &mut Vec<Entry>, path: &Path, data: &[u8]) {
        entries.retain(|e| e.path != path);
        entries.push(Entry { path: path.to_path_buf(), data: hex(data) });
        if entries.len() > KEEP {
            let extra = entries.len() - KEEP;
            entries.drain(..extra);
        }
    }

    pub fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    pub fn unhex(s: &str) -> Option<Vec<u8>> {
        if !s.len().is_multiple_of(2) {
            return None;
        }
        (0..s.len()).step_by(2).map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok()).collect()
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use super::store;
    use objc2::rc::Retained;
    use objc2::runtime::Bool;
    use objc2_app_kit::NSWorkspace;
    use objc2_foundation::{NSData, NSString, NSURLBookmarkCreationOptions, NSURLBookmarkResolutionOptions, NSURL};
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;

    /// A URL whose security-scoped access is in use: kept for the whole run (NSURL is immutable
    /// and safe to hand between threads).
    struct Held {
        // Only kept alive: access lasts while the URL does.
        _url: Retained<NSURL>,
    }
    unsafe impl Send for Held {}

    static ACTIVE: Mutex<Vec<(PathBuf, Held)>> = Mutex::new(Vec::new());

    fn url(path: &Path) -> Retained<NSURL> {
        NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()))
    }

    fn bookmark(url: &NSURL) -> Option<Vec<u8>> {
        url.bookmarkDataWithOptions_includingResourceValuesForKeys_relativeToURL_error(
            NSURLBookmarkCreationOptions::WithSecurityScope,
            None,
            None,
        )
        .map(|d| d.to_vec())
        .map_err(|e| log::warn!("bookmark: {}", e.localizedDescription()))
        .ok()
    }

    pub fn remember(path: &Path) {
        // The folder panel granted access for this run; the bookmark keeps it for the next ones.
        let Some(data) = bookmark(&url(path)) else { return };
        let mut entries = store::load();
        store::put(&mut entries, path, &data);
        store::save(&entries);
    }

    pub fn restore() -> usize {
        let mut entries = store::load();
        let mut active = ACTIVE.lock().unwrap_or_else(|e| e.into_inner());
        let mut changed = false;
        let mut count = 0;
        for e in entries.iter_mut() {
            if active.iter().any(|(p, _)| p == &e.path) {
                continue;
            }
            let Some(bytes) = store::unhex(&e.data) else { continue };
            let data = NSData::with_bytes(&bytes);
            let mut stale = Bool::NO;
            let options = NSURLBookmarkResolutionOptions::WithSecurityScope
                | NSURLBookmarkResolutionOptions::WithoutUI
                | NSURLBookmarkResolutionOptions::WithoutMounting;
            // Fails while the drive is not connected: try again when it is.
            let Ok(u) = (unsafe {
                NSURL::URLByResolvingBookmarkData_options_relativeToURL_bookmarkDataIsStale_error(
                    &data, options, None, &mut stale,
                )
            }) else {
                continue;
            };
            if !unsafe { u.startAccessingSecurityScopedResource() } {
                continue;
            }
            let path = u.path().map(|s| PathBuf::from(s.to_string())).unwrap_or_else(|| e.path.clone());
            if stale.as_bool() || path != e.path {
                if let Some(fresh) = bookmark(&u) {
                    e.path = path.clone();
                    e.data = store::hex(&fresh);
                    changed = true;
                }
            }
            active.push((path, Held { _url: u }));
            count += 1;
        }
        if changed {
            store::save(&entries);
        }
        count
    }

    pub fn open(path: &Path) -> bool {
        NSWorkspace::sharedWorkspace().openURL(&url(path))
    }

    pub fn eject(path: &Path) -> Result<(), String> {
        NSWorkspace::sharedWorkspace()
            .unmountAndEjectDeviceAtURL_error(&url(path))
            .map_err(|e| e.localizedDescription().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trip() {
        let bytes: Vec<u8> = (0..=255).collect();
        assert_eq!(store::unhex(&store::hex(&bytes)), Some(bytes));
        assert_eq!(store::unhex("abc"), None);
        assert_eq!(store::unhex("zz"), None);
    }

    #[test]
    fn store_keeps_the_newest_and_replaces_by_path() {
        let dir = std::env::temp_dir().join(format!("photoorg-macos-test-{}", std::process::id()));
        let file = dir.join("bookmarks.json");
        let mut v = Vec::new();
        for i in 0..KEEP + 5 {
            store::put(&mut v, Path::new(&format!("/Volumes/CARD{i}")), &[i as u8]);
        }
        assert_eq!(v.len(), KEEP);
        assert_eq!(v[0].path, PathBuf::from("/Volumes/CARD5"));
        store::put(&mut v, Path::new("/Volumes/CARD5"), &[1, 2, 3]);
        assert_eq!(v.len(), KEEP);
        assert_eq!(v.last().unwrap().data, "010203");
        store::save_to(&file, &v);
        assert_eq!(store::load_from(&file), v);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn nothing_happens_outside_the_sandbox() {
        // Tests never run sandboxed.
        assert!(!is_sandboxed());
        assert_eq!(restore(), 0);
        assert!(!open_folder(Path::new("/")));
        assert!(eject(Path::new("/nonexistent")).is_none());
        assert!(can_read(Path::new(".")));
        assert!(!can_read(Path::new("/definitely/not/here")));
    }
}
