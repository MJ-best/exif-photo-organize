//! Detection of camera memory cards (any mounted volume with a `DCIM` folder).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Volume {
    /// Display name, e.g. "EOS_DIGITAL" or "SD 카드 (E:)".
    pub name: String,
    /// Mount point.
    pub path: PathBuf,
}

fn candidate_mounts() -> Vec<PathBuf> {
    let mut v = Vec::new();
    #[cfg(target_os = "macos")]
    {
        if let Ok(rd) = std::fs::read_dir("/Volumes") {
            for e in rd.flatten() {
                let p = e.path();
                // Skip the symlink to the boot volume.
                if std::fs::read_link(&p).map(|t| t == Path::new("/")).unwrap_or(false) {
                    continue;
                }
                v.push(p);
            }
        }
    }
    #[cfg(target_os = "windows")]
    {
        for letter in b'D'..=b'Z' {
            v.push(PathBuf::from(format!("{}:\\", letter as char)));
        }
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let user = std::env::var("USER").unwrap_or_default();
        let mut roots = vec![PathBuf::from("/media"), PathBuf::from("/mnt"), PathBuf::from("/run/media")];
        if !user.is_empty() {
            roots.insert(0, PathBuf::from(format!("/media/{user}")));
            roots.insert(0, PathBuf::from(format!("/run/media/{user}")));
        }
        for root in roots {
            if let Ok(rd) = std::fs::read_dir(&root) {
                for e in rd.flatten() {
                    v.push(e.path());
                }
            }
        }
    }
    v
}

fn has_dcim(p: &Path) -> bool {
    ["DCIM", "dcim"].iter().any(|d| p.join(d).is_dir())
}

/// All mounted volumes that look like camera cards.
pub fn camera_volumes() -> Vec<Volume> {
    let mut out: Vec<Volume> = candidate_mounts()
        .into_iter()
        .filter(|p| has_dcim(p))
        .map(|p| {
            let name = if cfg!(windows) {
                format!("SD 카드 ({})", p.display().to_string().trim_end_matches('\\'))
            } else {
                p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| p.display().to_string())
            };
            Volume { name, path: p }
        })
        .collect();
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out.dedup();
    out
}

/// Poll for card changes on a background thread; `on_change` is called with the new list.
pub fn spawn_watcher(interval: Duration, on_change: impl Fn(Vec<Volume>) + Send + 'static) -> Arc<AtomicBool> {
    let stop = Arc::new(AtomicBool::new(false));
    let stop2 = stop.clone();
    std::thread::Builder::new()
        .name("volume-watcher".into())
        .spawn(move || {
            let mut last: Option<Vec<Volume>> = None;
            while !stop2.load(Ordering::Relaxed) {
                let now = camera_volumes();
                if last.as_ref() != Some(&now) {
                    on_change(now.clone());
                    last = Some(now);
                }
                std::thread::sleep(interval);
            }
        })
        .expect("spawn volume watcher");
    stop
}

/// Best-effort eject / unmount.
pub fn eject(path: &Path) -> anyhow::Result<()> {
    use std::process::Command;
    #[cfg(target_os = "macos")]
    let status = Command::new("diskutil").arg("eject").arg(path).status()?;
    #[cfg(target_os = "windows")]
    let status = {
        let drive = path.display().to_string().trim_end_matches('\\').to_string();
        Command::new("powershell")
            .args([
                "-NoProfile",
                "-Command",
                &format!(
                    "(New-Object -comObject Shell.Application).Namespace(17).ParseName('{drive}').InvokeVerb('Eject')"
                ),
            ])
            .status()?
    };
    #[cfg(all(unix, not(target_os = "macos")))]
    let status = {
        let gio = Command::new("gio").arg("mount").arg("-e").arg(path).status();
        match gio {
            Ok(s) if s.success() => s,
            _ => Command::new("umount").arg(path).status()?,
        }
    };
    if !status.success() {
        anyhow::bail!("추출 실패 ({status})");
    }
    Ok(())
}
