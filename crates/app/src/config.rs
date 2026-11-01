//! Persistent user settings (JSON in the OS config directory).

use photoorg_core::ImportOptions;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ThemePref {
    /// Warm paper look of the app icon.
    #[default]
    Cream,
    /// Dark warm look.
    Cocoa,
    /// Follow the OS: Cream when light, Cocoa when dark.
    System,
    /// Classic macOS Photos look.
    Dark,
    Light,
}

impl ThemePref {
    pub fn look(self, system_dark: bool) -> crate::theme::Look {
        use crate::theme::Look;
        match self {
            ThemePref::Cream => Look::Cream,
            ThemePref::Cocoa => Look::Cocoa,
            ThemePref::System => {
                if system_dark {
                    Look::Cocoa
                } else {
                    Look::Cream
                }
            }
            ThemePref::Dark => Look::Dark,
            ThemePref::Light => Look::Light,
        }
    }
}

/// Bumped when a setting's meaning changes; see [`Config::load`].
const CONFIG_VERSION: u32 = 2;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// 0 for configs written before versioning (v0.1.x).
    #[serde(default)]
    pub config_version: u32,
    pub library: Option<PathBuf>,
    pub recent_libraries: Vec<PathBuf>,
    pub import: ImportOptions,
    pub theme: ThemePref,
    /// Write ratings / tags to XMP sidecars (Lightroom, Capture One, darktable ...).
    pub write_xmp: bool,
    /// Jump to the next photo after rating in the viewer.
    pub auto_advance: bool,
    /// Open the import screen when a camera card is inserted.
    pub auto_open_import: bool,
    pub eject_after_import: bool,
    /// Grid tile size in points.
    pub tile_size: f32,
    /// Crop thumbnails to squares (otherwise show the whole photo).
    pub square_tiles: bool,
    pub show_info: bool,
    pub show_filmstrip: bool,
    /// Analyse photos for the culling assistant (B-cut detection) automatically.
    pub cull_auto: bool,
    /// B-cut probability (%) from which an undecided photo is a candidate.
    pub cull_threshold: u8,
    /// Show B-cut badges on thumbnails.
    pub cull_badges: bool,
    /// Which ratings count as A / B labels for learning.
    pub learn: photoorg_core::cull::LearnSettings,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            config_version: CONFIG_VERSION,
            library: None,
            recent_libraries: Vec::new(),
            import: ImportOptions::default(),
            theme: ThemePref::Cream,
            write_xmp: true,
            auto_advance: false,
            auto_open_import: true,
            eject_after_import: false,
            tile_size: 150.0,
            square_tiles: true,
            show_info: false,
            show_filmstrip: true,
            cull_auto: true,
            cull_threshold: 60,
            cull_badges: true,
            learn: Default::default(),
        }
    }
}

impl Config {
    fn path() -> Option<PathBuf> {
        Some(dirs::config_dir()?.join("photo-organize").join("config.json"))
    }

    pub fn load() -> Config {
        let Some(p) = Self::path() else { return Config::default() };
        let mut cfg: Config = std::fs::read(&p).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        cfg.migrate();
        cfg
    }

    fn migrate(&mut self) {
        if self.config_version < 2 {
            // v0.1 saved its default (classic dark) explicitly; move those users to the new
            // default look once. An explicit Light / System choice is kept.
            if self.theme == ThemePref::Dark {
                self.theme = ThemePref::Cream;
            }
        }
        self.config_version = CONFIG_VERSION;
    }

    pub fn save(&self) {
        let Some(p) = Self::path() else { return };
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(json) = serde_json::to_vec_pretty(self) {
            let tmp = p.with_extension("json.tmp");
            if std::fs::write(&tmp, json).is_ok() {
                let _ = std::fs::rename(&tmp, &p);
            }
        }
    }

    pub fn remember_library(&mut self, p: PathBuf) {
        self.recent_libraries.retain(|x| x != &p);
        self.recent_libraries.insert(0, p.clone());
        self.recent_libraries.truncate(8);
        self.library = Some(p);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_default_theme_moves_to_cream_once() {
        let mut old: Config = serde_json::from_str(r#"{"theme":"Dark","tile_size":150.0}"#).unwrap();
        assert_eq!(old.config_version, 0);
        old.migrate();
        assert_eq!(old.theme, ThemePref::Cream);
        assert_eq!(old.config_version, CONFIG_VERSION);
        // A later explicit choice of the classic look is respected.
        let mut new: Config =
            serde_json::from_str(&format!(r#"{{"theme":"Dark","config_version":{CONFIG_VERSION}}}"#)).unwrap();
        new.migrate();
        assert_eq!(new.theme, ThemePref::Dark);
        let mut light: Config = serde_json::from_str(r#"{"theme":"Light"}"#).unwrap();
        light.migrate();
        assert_eq!(light.theme, ThemePref::Light);
        assert_eq!(Config::default().theme, ThemePref::Cream);
    }
}
