//! Persistent user settings (JSON in the OS config directory).

use photoorg_core::ImportOptions;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ThemePref {
    #[default]
    Dark,
    Light,
    System,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
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
}

impl Default for Config {
    fn default() -> Self {
        Config {
            library: None,
            recent_libraries: Vec::new(),
            import: ImportOptions::default(),
            theme: ThemePref::Dark,
            write_xmp: true,
            auto_advance: false,
            auto_open_import: true,
            eject_after_import: false,
            tile_size: 150.0,
            square_tiles: true,
            show_info: false,
            show_filmstrip: true,
        }
    }
}

impl Config {
    fn path() -> Option<PathBuf> {
        Some(dirs::config_dir()?.join("photo-organize").join("config.json"))
    }

    pub fn load() -> Config {
        let Some(p) = Self::path() else { return Config::default() };
        std::fs::read(&p).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
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
