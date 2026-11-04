//! The catalog: an SQLite database stored inside the library folder (so it travels with the
//! external drive), plus an in-memory index used for instant filtering.

use crate::media::{formats, FileKind};
use crate::metadata::{DateSource, Metadata};
use anyhow::{Context, Result};
use chrono::{DateTime, NaiveDateTime};
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::{BTreeMap, HashMap};
use std::path::Path;

const SCHEMA_VERSION: i64 = 4;

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT);
CREATE TABLE IF NOT EXISTS projects (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    note TEXT NOT NULL DEFAULT '',
    sort INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT ''
);
CREATE TABLE IF NOT EXISTS imports (
    id INTEGER PRIMARY KEY,
    started_at TEXT NOT NULL,
    source TEXT NOT NULL,
    label TEXT NOT NULL,
    asset_count INTEGER NOT NULL DEFAULT 0,
    file_count INTEGER NOT NULL DEFAULT 0,
    bytes INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS assets (
    id INTEGER PRIMARY KEY,
    captured_at TEXT,
    captured_ms INTEGER NOT NULL,
    date_source TEXT NOT NULL,
    utc_offset TEXT,
    make TEXT, model TEXT, camera TEXT, lens TEXT, lens_make TEXT,
    focal_length REAL, focal_35mm REAL, aperture REAL, exposure_time REAL, iso INTEGER,
    exposure_bias REAL, flash INTEGER, exposure_program INTEGER, metering_mode INTEGER, white_balance INTEGER,
    width INTEGER, height INTEGER, orientation INTEGER NOT NULL DEFAULT 1,
    gps_lat REAL, gps_lon REAL, gps_alt REAL,
    serial TEXT, software TEXT, artist TEXT,
    formats INTEGER NOT NULL DEFAULT 0,
    name TEXT NOT NULL,
    rating INTEGER NOT NULL DEFAULT 0,
    import_id INTEGER REFERENCES imports(id),
    imported_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS assets_captured ON assets(captured_ms);
CREATE INDEX IF NOT EXISTS assets_name ON assets(name, captured_ms);
CREATE TABLE IF NOT EXISTS files (
    id INTEGER PRIMARY KEY,
    asset_id INTEGER NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
    role TEXT NOT NULL,
    rel_path TEXT NOT NULL UNIQUE,
    orig_name TEXT NOT NULL,
    orig_path TEXT,
    size INTEGER NOT NULL,
    hash TEXT NOT NULL,
    quick_key TEXT NOT NULL,
    mtime INTEGER
);
CREATE INDEX IF NOT EXISTS files_asset ON files(asset_id);
CREATE INDEX IF NOT EXISTS files_hash ON files(hash);
CREATE INDEX IF NOT EXISTS files_quick ON files(quick_key);
CREATE TABLE IF NOT EXISTS tags (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    kind INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS asset_tags (
    asset_id INTEGER NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
    tag_id INTEGER NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
    PRIMARY KEY (asset_id, tag_id)
) WITHOUT ROWID;
CREATE INDEX IF NOT EXISTS asset_tags_tag ON asset_tags(tag_id);
CREATE TABLE IF NOT EXISTS analysis (
    asset_id INTEGER PRIMARY KEY REFERENCES assets(id) ON DELETE CASCADE,
    version INTEGER NOT NULL,
    scene INTEGER NOT NULL DEFAULT 0,
    data TEXT NOT NULL,
    b_prob REAL,
    reasons INTEGER NOT NULL DEFAULT 0,
    burst INTEGER NOT NULL DEFAULT 0,
    burst_rank INTEGER NOT NULL DEFAULT 0,
    burst_size INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS ml_models (
    key TEXT PRIMARY KEY,
    data TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS work_copies (
    project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    asset_id INTEGER NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
    rel_path TEXT NOT NULL,
    PRIMARY KEY (project_id, rel_path)
);
CREATE INDEX IF NOT EXISTS work_copies_asset ON work_copies(asset_id);
"#;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum TagKind {
    Auto = 0,
    User = 1,
    /// A theme of a project (작품): name "<project>:<theme>", `TagInfo::project` set.
    Theme = 2,
}

impl TagKind {
    pub fn from_i64(v: i64) -> TagKind {
        match v {
            1 => TagKind::User,
            2 => TagKind::Theme,
            _ => TagKind::Auto,
        }
    }
}

#[derive(Clone, Debug)]
pub struct TagInfo {
    pub id: u32,
    pub name: String,
    pub kind: TagKind,
    /// Project of a theme tag.
    pub project: Option<u32>,
    /// Order of a theme within its project.
    pub sort: i64,
}

impl TagInfo {
    /// The part after "<project>:" for themes, the whole name otherwise.
    pub fn short_name(&self) -> &str {
        if self.kind == TagKind::Theme {
            self.name.split_once(':').map(|(_, t)| t).unwrap_or(&self.name)
        } else {
            &self.name
        }
    }
}

/// A photo book / exhibition (작품) whose themes are tags.
#[derive(Clone, Debug, PartialEq)]
pub struct ProjectInfo {
    pub id: u32,
    pub name: String,
    pub note: String,
    pub sort: i64,
}

#[derive(Clone, Debug)]
pub struct ImportInfo {
    pub id: i64,
    pub started_at: String,
    pub source: String,
    pub label: String,
    pub asset_count: i64,
    pub file_count: i64,
    pub bytes: i64,
}

#[derive(Clone, Debug)]
pub struct FileRecord {
    pub id: i64,
    pub asset_id: i64,
    pub kind: FileKind,
    pub rel_path: String,
    pub orig_name: String,
    pub orig_path: Option<String>,
    pub size: u64,
    pub hash: String,
    /// Where the file was before it was moved into a 작품 folder (see [`crate::works`]).
    pub home_rel: Option<String>,
}

/// New file to be recorded.
#[derive(Clone, Debug)]
pub struct NewFile {
    pub kind: FileKind,
    pub rel_path: String,
    pub orig_name: String,
    pub orig_path: Option<String>,
    pub size: u64,
    pub hash: String,
    pub quick_key: String,
    pub mtime: Option<i64>,
}

/// Compact asset record kept in memory for all assets of the library.
#[derive(Clone, Debug)]
pub struct AssetRecord {
    pub id: i64,
    /// Capture time (camera local time) in "naive epoch" milliseconds.
    pub captured_ms: i64,
    pub camera: u32,
    pub lens: u32,
    pub focal: f32,
    pub focal35: f32,
    pub aperture: f32,
    pub shutter: f32,
    pub iso: u32,
    pub bias: f32,
    pub width: u32,
    pub height: u32,
    pub orientation: u8,
    pub rating: u8,
    pub formats: u8,
    pub flash: bool,
    pub has_gps: bool,
    pub date_from_file: bool,
    pub import_id: i64,
    pub tags: Vec<u32>,
    pub name: String,
    /// -1 rejected (B-cut), 0 undecided, 1 picked (A-cut).
    pub flag: i8,
    /// Effective scene (user choice, else analysis), see [`crate::cull::Scene`].
    pub scene: u8,
    /// Probability of being a B-cut (NaN = not analysed yet).
    pub b_prob: f32,
    /// Reason bits, see [`crate::cull::learn::reason`].
    pub reasons: u16,
    /// Burst id (asset id of its first frame), 0 when not part of a burst.
    pub burst: i64,
    pub burst_rank: u16,
    pub burst_size: u16,
    /// A-cut candidate rank: share of the analysed photos (best frame of each burst) that look
    /// better, 0 = the best one (NaN = not analysed, or a weaker frame of a burst). See
    /// [`LibraryIndex::rank_keepers`].
    pub keep_rank: f32,
}

impl AssetRecord {
    pub fn captured(&self) -> NaiveDateTime {
        ms_to_naive(self.captured_ms)
    }
    pub fn is_video(&self) -> bool {
        self.formats & formats::VIDEO != 0
    }
    pub fn has_raw(&self) -> bool {
        self.formats & formats::RAW != 0
    }
    /// Aspect ratio (w/h) after orientation, if known.
    pub fn aspect(&self) -> Option<f32> {
        if self.width == 0 || self.height == 0 {
            return None;
        }
        let (w, h) =
            if matches!(self.orientation, 5..=8) { (self.height, self.width) } else { (self.width, self.height) };
        Some(w as f32 / h as f32)
    }
}

/// Asset row with resolved strings (used to feed new rows into a [`LibraryIndex`]).
#[derive(Clone, Debug)]
pub struct AssetRow {
    pub rec: AssetRecord,
    pub camera: String,
    pub lens: String,
    pub tags: Vec<TagInfo>,
}

pub fn naive_to_ms(t: NaiveDateTime) -> i64 {
    t.and_utc().timestamp_millis()
}

pub fn ms_to_naive(ms: i64) -> NaiveDateTime {
    DateTime::from_timestamp_millis(ms).map(|d| d.naive_utc()).unwrap_or_default()
}

/// String interner; id 0 is reserved for "unknown".
#[derive(Clone, Debug, Default)]
pub struct Interner {
    names: Vec<String>,
    map: HashMap<String, u32>,
}

impl Interner {
    pub fn new() -> Self {
        Interner { names: vec![String::new()], map: HashMap::new() }
    }
    pub fn intern(&mut self, s: &str) -> u32 {
        if s.is_empty() {
            return 0;
        }
        if let Some(&id) = self.map.get(s) {
            return id;
        }
        let id = self.names.len() as u32;
        self.names.push(s.to_string());
        self.map.insert(s.to_string(), id);
        id
    }
    /// Id of an existing name (0 if unknown).
    pub fn intern_lookup(&self, s: &str) -> u32 {
        self.map.get(s).copied().unwrap_or(0)
    }
    pub fn name(&self, id: u32) -> &str {
        self.names.get(id as usize).map(|s| s.as_str()).unwrap_or("")
    }
    pub fn len(&self) -> usize {
        self.names.len()
    }
    pub fn is_empty(&self) -> bool {
        self.names.len() <= 1
    }
}

/// Everything the UI needs to browse a library, in memory.
#[derive(Clone, Debug, Default)]
pub struct LibraryIndex {
    /// Sorted by (captured_ms, id).
    pub assets: Vec<AssetRecord>,
    pub cameras: Interner,
    pub lenses: Interner,
    pub tags: BTreeMap<u32, TagInfo>,
    /// Projects (작품) in display order.
    pub projects: Vec<ProjectInfo>,
    pub imports: Vec<ImportInfo>,
    pos: HashMap<i64, usize>,
}

impl LibraryIndex {
    pub fn new() -> Self {
        LibraryIndex { cameras: Interner::new(), lenses: Interner::new(), ..Default::default() }
    }

    pub fn position(&self, id: i64) -> Option<usize> {
        self.pos.get(&id).copied()
    }
    pub fn get(&self, id: i64) -> Option<&AssetRecord> {
        self.position(id).map(|i| &self.assets[i])
    }
    pub fn get_mut(&mut self, id: i64) -> Option<&mut AssetRecord> {
        let i = self.position(id)?;
        Some(&mut self.assets[i])
    }
    pub fn camera_name(&self, a: &AssetRecord) -> &str {
        self.cameras.name(a.camera)
    }
    pub fn lens_name(&self, a: &AssetRecord) -> &str {
        self.lenses.name(a.lens)
    }
    pub fn tag_name(&self, id: u32) -> &str {
        self.tags.get(&id).map(|t| t.name.as_str()).unwrap_or("")
    }

    pub fn project(&self, id: u32) -> Option<&ProjectInfo> {
        self.projects.iter().find(|p| p.id == id)
    }

    /// Themes of a project in their order.
    pub fn themes_of(&self, project: u32) -> Vec<&TagInfo> {
        let mut v: Vec<&TagInfo> = self.tags.values().filter(|t| t.project == Some(project)).collect();
        v.sort_by_key(|t| (t.sort, t.id));
        v
    }

    /// Whether the asset has any theme of `project`.
    pub fn in_project(&self, a: &AssetRecord, project: u32) -> bool {
        a.tags.iter().any(|t| self.tags.get(t).is_some_and(|ti| ti.project == Some(project)))
    }

    fn reindex(&mut self) {
        self.assets.sort_by_key(|a| (a.captured_ms, a.id));
        self.pos = self.assets.iter().enumerate().map(|(i, a)| (a.id, i)).collect();
        self.rank_keepers();
    }

    /// Rank the analysed photos by how unlikely they are to be B-cuts (the learned model, so
    /// "like the ones I keep"), counting only the best frame of each burst: `keep_rank` 0 is the
    /// best photo, 0.1 means 10 % look better. Call after the scores change.
    pub fn rank_keepers(&mut self) {
        let mut order: Vec<(f32, usize)> = self
            .assets
            .iter()
            .enumerate()
            .filter(|(_, a)| a.b_prob.is_finite() && !a.is_video() && (a.burst_size < 2 || a.burst_rank == 0))
            .map(|(i, a)| (a.b_prob, i))
            .collect();
        order.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal).then(a.1.cmp(&b.1)));
        for a in &mut self.assets {
            a.keep_rank = f32::NAN;
        }
        let n = order.len().max(1) as f32;
        for (rank, (_, i)) in order.into_iter().enumerate() {
            self.assets[i].keep_rank = rank as f32 / n;
        }
    }

    /// Insert or replace rows (e.g. assets that were just imported).
    pub fn upsert(&mut self, rows: Vec<AssetRow>) {
        if rows.is_empty() {
            return;
        }
        for row in rows {
            let mut rec = row.rec;
            rec.camera = self.cameras.intern(&row.camera);
            rec.lens = self.lenses.intern(&row.lens);
            for t in row.tags {
                self.tags.entry(t.id).or_insert(t);
            }
            if let Some(i) = self.pos.get(&rec.id).copied() {
                self.assets[i] = rec;
            } else {
                self.pos.insert(rec.id, self.assets.len());
                self.assets.push(rec);
            }
        }
        self.reindex();
    }

    pub fn remove(&mut self, ids: &[i64]) {
        let set: std::collections::HashSet<i64> = ids.iter().copied().collect();
        self.assets.retain(|a| !set.contains(&a.id));
        self.reindex();
    }
}

pub struct Catalog {
    conn: Connection,
}

fn opt_f32(v: Option<f64>) -> f32 {
    v.map(|x| x as f32).unwrap_or(f32::NAN)
}

impl Catalog {
    pub fn open(path: &Path) -> Result<Catalog> {
        let conn = Connection::open(path).with_context(|| format!("open catalog {}", path.display()))?;
        conn.busy_timeout(std::time::Duration::from_secs(10))?;
        let mode: String = conn.query_row("PRAGMA journal_mode=WAL", [], |r| r.get(0)).unwrap_or_default();
        if !mode.eq_ignore_ascii_case("wal") {
            log::warn!("catalog journal mode is {mode}");
        }
        conn.execute_batch("PRAGMA synchronous=NORMAL; PRAGMA foreign_keys=ON; PRAGMA temp_store=MEMORY;")?;
        conn.execute_batch(SCHEMA)?;
        // v2: pick/reject flag and user-chosen scene.
        let cols: Vec<String> = conn
            .prepare("PRAGMA table_info(assets)")?
            .query_map([], |r| r.get::<_, String>(1))?
            .collect::<rusqlite::Result<_>>()?;
        if !cols.iter().any(|c| c == "flag") {
            conn.execute_batch("ALTER TABLE assets ADD COLUMN flag INTEGER NOT NULL DEFAULT 0;")?;
        }
        if !cols.iter().any(|c| c == "scene_user") {
            conn.execute_batch("ALTER TABLE assets ADD COLUMN scene_user INTEGER;")?;
        }
        // v3: projects (작품) whose themes are tags.
        let tag_cols: Vec<String> = conn
            .prepare("PRAGMA table_info(tags)")?
            .query_map([], |r| r.get::<_, String>(1))?
            .collect::<rusqlite::Result<_>>()?;
        if !tag_cols.iter().any(|c| c == "project_id") {
            conn.execute_batch(
                "ALTER TABLE tags ADD COLUMN project_id INTEGER;
                 ALTER TABLE tags ADD COLUMN sort INTEGER NOT NULL DEFAULT 0;",
            )?;
        }
        // v4: files moved into a 작품 folder remember their place in the dated folders.
        let file_cols: Vec<String> = conn
            .prepare("PRAGMA table_info(files)")?
            .query_map([], |r| r.get::<_, String>(1))?
            .collect::<rusqlite::Result<_>>()?;
        if !file_cols.iter().any(|c| c == "home_rel") {
            conn.execute_batch("ALTER TABLE files ADD COLUMN home_rel TEXT;")?;
        }
        conn.execute("UPDATE meta SET value=?1 WHERE key='schema_version'", [SCHEMA_VERSION.to_string()])?;
        let version: Option<String> =
            conn.query_row("SELECT value FROM meta WHERE key='schema_version'", [], |r| r.get(0)).optional()?;
        if version.is_none() {
            conn.execute("INSERT INTO meta(key, value) VALUES ('schema_version', ?1)", [SCHEMA_VERSION.to_string()])?;
        }
        Ok(Catalog { conn })
    }

    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    pub fn begin_import(&self, source: &str, label: &str) -> Result<i64> {
        let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        self.conn.execute(
            "INSERT INTO imports(started_at, source, label) VALUES (?1, ?2, ?3)",
            params![now, source, label],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn finish_import(&self, id: i64, assets: i64, files: i64, bytes: i64) -> Result<()> {
        if assets == 0 && files == 0 {
            self.conn.execute("DELETE FROM imports WHERE id=?1", [id])?;
        } else {
            self.conn.execute(
                "UPDATE imports SET asset_count=?2, file_count=?3, bytes=?4 WHERE id=?1",
                params![id, assets, files, bytes],
            )?;
        }
        Ok(())
    }

    /// Insert a new asset with its files and auto tags. Returns the asset id.
    pub fn insert_asset(
        &mut self,
        meta: &Metadata,
        name: &str,
        files: &[NewFile],
        import_id: Option<i64>,
        rating: u8,
        tags: &[String],
    ) -> Result<i64> {
        let fmt = files.iter().fold(0u8, |a, f| a | f.kind.format_flag());
        let captured = meta.captured_at.unwrap_or_default();
        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT INTO assets(captured_at, captured_ms, date_source, utc_offset, make, model, camera, lens, lens_make,
                focal_length, focal_35mm, aperture, exposure_time, iso, exposure_bias, flash, exposure_program,
                metering_mode, white_balance, width, height, orientation, gps_lat, gps_lon, gps_alt, serial,
                software, artist, formats, name, rating, import_id, flag, imported_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24,?25,?26,?27,?28,?29,?30,?31,?32,?33,
                     datetime('now','localtime'))",
            params![
                meta.captured_at.map(|t| t.format("%Y-%m-%d %H:%M:%S%.3f").to_string()),
                naive_to_ms(captured),
                if meta.date_source == DateSource::Exif { "exif" } else { "file" },
                meta.offset,
                meta.make,
                meta.model,
                meta.camera(),
                meta.lens,
                meta.lens_make,
                meta.focal_length,
                meta.focal_35mm,
                meta.aperture,
                meta.exposure_time,
                meta.iso,
                meta.exposure_bias,
                meta.flash_fired,
                meta.exposure_program,
                meta.metering_mode,
                meta.white_balance,
                meta.width,
                meta.height,
                meta.orientation,
                meta.gps.map(|g| g.lat),
                meta.gps.map(|g| g.lon),
                meta.gps.and_then(|g| g.alt),
                meta.serial,
                meta.software,
                meta.artist,
                fmt,
                name,
                rating,
                import_id,
                if meta.reject { -1i64 } else { 0 },
            ],
        )?;
        let id = tx.last_insert_rowid();
        insert_files(&tx, id, files)?;
        for t in tags {
            add_tag_tx(&tx, id, t, TagKind::Auto)?;
        }
        tx.commit()?;
        Ok(id)
    }

    /// Attach additional files (e.g. the RAW half of a pair imported separately) to an asset.
    pub fn attach_files(&mut self, asset_id: i64, files: &[NewFile]) -> Result<()> {
        let tx = self.conn.transaction()?;
        insert_files(&tx, asset_id, files)?;
        let fmt = files.iter().fold(0u8, |a, f| a | f.kind.format_flag());
        tx.execute("UPDATE assets SET formats = formats | ?2 WHERE id=?1", params![asset_id, fmt])?;
        // Refresh the format tag.
        let new_fmt: u8 = tx.query_row("SELECT formats FROM assets WHERE id=?1", [asset_id], |r| r.get(0))?;
        tx.execute(
            "DELETE FROM asset_tags WHERE asset_id=?1 AND tag_id IN (SELECT id FROM tags WHERE name LIKE '형식:%')",
            [asset_id],
        )?;
        add_tag_tx(&tx, asset_id, &format!("형식:{}", formats::label(new_fmt)), TagKind::Auto)?;
        tx.commit()?;
        Ok(())
    }

    pub fn find_by_quick_key(&self, key: &str) -> Result<Option<i64>> {
        Ok(self
            .conn
            .query_row("SELECT asset_id FROM files WHERE quick_key=?1 LIMIT 1", [key], |r| r.get(0))
            .optional()?)
    }

    pub fn find_by_hash(&self, hash: &str) -> Result<Option<i64>> {
        Ok(self.conn.query_row("SELECT asset_id FROM files WHERE hash=?1 LIMIT 1", [hash], |r| r.get(0)).optional()?)
    }

    /// Find an existing asset that is the other half of a RAW+JPEG pair (e.g. shot to two cards).
    pub fn find_partner(&self, name: &str, captured_ms: i64, camera: Option<&str>, fmt: u8) -> Result<Option<i64>> {
        let mut stmt = self
            .conn
            .prepare_cached("SELECT id, formats, camera FROM assets WHERE name=?1 AND captured_ms BETWEEN ?2 AND ?3")?;
        let rows = stmt.query_map(params![name, captured_ms - 1000, captured_ms + 1000], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, u8>(1)?, r.get::<_, Option<String>>(2)?))
        })?;
        for row in rows {
            let (id, f, cam) = row?;
            let complementary = (f & formats::RAW != 0) != (fmt & formats::RAW != 0) && f & formats::VIDEO == 0;
            if complementary && cam.as_deref() == camera {
                return Ok(Some(id));
            }
        }
        Ok(None)
    }

    pub fn asset_name(&self, id: i64) -> Result<Option<String>> {
        Ok(self.conn.query_row("SELECT name FROM assets WHERE id=?1", [id], |r| r.get(0)).optional()?)
    }

    pub fn path_exists(&self, rel_path: &str) -> Result<bool> {
        Ok(self.conn.query_row("SELECT 1 FROM files WHERE rel_path=?1", [rel_path], |_| Ok(())).optional()?.is_some())
    }

    /// -1 reject (B-cut), 0 none, 1 pick (A-cut).
    pub fn set_flag(&mut self, ids: &[i64], flag: i8) -> Result<()> {
        let tx = self.conn.transaction()?;
        {
            let mut stmt = tx.prepare_cached("UPDATE assets SET flag=?2 WHERE id=?1")?;
            for id in ids {
                stmt.execute(params![id, flag as i64])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Override (or with `None` reset) the detected scene.
    pub fn set_scene_user(&mut self, ids: &[i64], scene: Option<u8>) -> Result<()> {
        let tx = self.conn.transaction()?;
        {
            let mut stmt = tx.prepare_cached("UPDATE assets SET scene_user=?2 WHERE id=?1")?;
            for id in ids {
                stmt.execute(params![id, scene.map(|s| s as i64)])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Scene set by the user (None = automatic).
    pub fn scene_user(&self, id: i64) -> Result<Option<u8>> {
        Ok(self.conn.query_row("SELECT scene_user FROM assets WHERE id=?1", [id], |r| {
            Ok(r.get::<_, Option<i64>>(0)?.map(|v| v as u8))
        })?)
    }

    pub fn rating_and_flag(&self, id: i64) -> Result<(u8, i8)> {
        Ok(self.conn.query_row("SELECT rating, flag FROM assets WHERE id=?1", [id], |r| {
            Ok((r.get::<_, u8>(0)?, r.get::<_, i64>(1)? as i8))
        })?)
    }

    pub fn put_analysis(&self, id: i64, a: &crate::cull::Analysis) -> Result<()> {
        let json = serde_json::to_string(a)?;
        self.conn.execute(
            "INSERT INTO analysis(asset_id, version, scene, data) VALUES (?1,?2,?3,?4)
             ON CONFLICT(asset_id) DO UPDATE SET version=excluded.version, scene=excluded.scene, data=excluded.data",
            params![id, a.version, a.scene as u8, json],
        )?;
        Ok(())
    }

    pub fn analysis(&self, id: i64) -> Result<Option<crate::cull::Analysis>> {
        let s: Option<String> =
            self.conn.query_row("SELECT data FROM analysis WHERE asset_id=?1", [id], |r| r.get(0)).optional()?;
        Ok(s.and_then(|s| serde_json::from_str(&s).ok()))
    }

    /// Assets analysed with the given analysis version.
    pub fn analyzed_ids(&self, version: u32) -> Result<std::collections::HashSet<i64>> {
        let mut stmt = self.conn.prepare("SELECT asset_id FROM analysis WHERE version=?1")?;
        let v = stmt.query_map([version], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
        Ok(v)
    }

    /// Inputs for training / scoring the culling model.
    pub fn cull_items(&self) -> Result<Vec<crate::cull::learn::Item>> {
        let mut stmt = self.conn.prepare(
            "SELECT a.id, a.captured_ms, COALESCE(a.camera, ''), a.exposure_time, COALESCE(a.focal_35mm, a.focal_length),
                    a.iso, a.rating, a.flag, a.scene_user, an.data
             FROM assets a JOIN analysis an ON an.asset_id = a.id",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Option<f64>>(3)?,
                r.get::<_, Option<f64>>(4)?,
                r.get::<_, Option<u32>>(5)?,
                r.get::<_, u8>(6)?,
                r.get::<_, i64>(7)?,
                r.get::<_, Option<i64>>(8)?,
                r.get::<_, String>(9)?,
            ))
        })?;
        let mut items = Vec::new();
        for row in rows {
            let (id, ms, camera, shutter, focal, iso, rating, flag, scene_user, data) = row?;
            let Ok(a) = serde_json::from_str::<crate::cull::Analysis>(&data) else { continue };
            let scene = scene_user.map(|s| crate::cull::Scene::from_u8(s as u8)).unwrap_or(a.scene);
            items.push(crate::cull::learn::Item {
                id,
                captured_ms: ms,
                camera,
                shutter: shutter.unwrap_or(0.0) as f32,
                focal35: focal.unwrap_or(0.0) as f32,
                iso: iso.unwrap_or(0),
                rating,
                flag: flag as i8,
                scene,
                sharp_peak: a.sharp_peak,
                sharp_global: a.sharp_global,
                motion: a.motion,
                clip_high: a.clip_high,
                clip_low: a.clip_low,
                brightness: a.brightness,
                face_area: a.face_area(),
                face_sharp: a.face_sharp(),
                eyes: a.eyes(),
                dhash: a.dhash,
            });
        }
        Ok(items)
    }

    pub fn put_scores(&mut self, scores: &[crate::cull::Score]) -> Result<()> {
        let tx = self.conn.transaction()?;
        {
            let mut stmt = tx.prepare_cached(
                "UPDATE analysis SET b_prob=?2, reasons=?3, burst=?4, burst_rank=?5, burst_size=?6 WHERE asset_id=?1",
            )?;
            for s in scores {
                stmt.execute(params![s.id, s.b_prob as f64, s.reasons, s.burst, s.burst_rank, s.burst_size])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn put_models(&mut self, models: &[(Option<crate::cull::Scene>, crate::cull::learn::Model)]) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM ml_models", [])?;
        for (scene, m) in models {
            let key = scene.map(|s| s.label().to_string()).unwrap_or_else(|| "전체".into());
            tx.execute(
                "INSERT INTO ml_models(key, data, updated_at) VALUES (?1, ?2, datetime('now','localtime'))",
                params![key, serde_json::to_string(m)?],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn set_rating(&mut self, ids: &[i64], rating: u8) -> Result<()> {
        let tx = self.conn.transaction()?;
        {
            let mut stmt = tx.prepare_cached("UPDATE assets SET rating=?2 WHERE id=?1")?;
            for id in ids {
                stmt.execute(params![id, rating])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn add_tag(&mut self, ids: &[i64], name: &str, kind: TagKind) -> Result<TagInfo> {
        let tx = self.conn.transaction()?;
        let mut tag_id = 0;
        for id in ids {
            tag_id = add_tag_tx(&tx, *id, name, kind)?;
        }
        if ids.is_empty() {
            tag_id = ensure_tag(&tx, name, kind)?;
        }
        tx.commit()?;
        let (project, sort) = self
            .conn
            .query_row("SELECT project_id, sort FROM tags WHERE id=?1", [tag_id], |r| {
                Ok((r.get::<_, Option<i64>>(0)?, r.get::<_, i64>(1)?))
            })
            .unwrap_or((None, 0));
        let kind = self
            .conn
            .query_row("SELECT kind FROM tags WHERE id=?1", [tag_id], |r| r.get::<_, i64>(0))
            .map(TagKind::from_i64)
            .unwrap_or(kind);
        Ok(TagInfo { id: tag_id as u32, name: name.to_string(), kind, project: project.map(|p| p as u32), sort })
    }

    pub fn remove_tag(&mut self, ids: &[i64], tag_id: u32) -> Result<()> {
        let tx = self.conn.transaction()?;
        {
            let mut stmt = tx.prepare_cached("DELETE FROM asset_tags WHERE asset_id=?1 AND tag_id=?2")?;
            for id in ids {
                stmt.execute(params![id, tag_id])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn files_for(&self, asset_id: i64) -> Result<Vec<FileRecord>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, asset_id, role, rel_path, orig_name, orig_path, size, hash, home_rel FROM files
             WHERE asset_id=?1 ORDER BY role",
        )?;
        let rows = stmt.query_map([asset_id], file_from_row)?;
        let mut v: Vec<FileRecord> = rows.collect::<rusqlite::Result<_>>()?;
        v.sort_by_key(|f| f.kind.preview_priority());
        Ok(v)
    }

    /// Files inside the library folder `dir` (library-relative, '/' separated), at any depth.
    pub fn files_under(&self, dir: &str) -> Result<Vec<FileRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, asset_id, role, rel_path, orig_name, orig_path, size, hash, home_rel FROM files
             WHERE substr(rel_path, 1, length(?1) + 1) = ?1 || '/' ORDER BY asset_id, role",
        )?;
        let v = stmt.query_map([dir], file_from_row)?.collect::<rusqlite::Result<_>>()?;
        Ok(v)
    }

    /// Record files moved into a 작품 folder: `(file id, new path)`. The first place a file is
    /// moved from is kept as its home.
    pub fn move_files(&mut self, moves: &[(i64, String)]) -> Result<()> {
        let tx = self.conn.transaction()?;
        {
            let mut stmt =
                tx.prepare_cached("UPDATE files SET home_rel=COALESCE(home_rel, rel_path), rel_path=?2 WHERE id=?1")?;
            for (id, rel) in moves {
                stmt.execute(params![id, rel])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Record files moved back to their dated folders: `(file id, new path)`.
    pub fn return_files(&mut self, moves: &[(i64, String)]) -> Result<()> {
        let tx = self.conn.transaction()?;
        {
            let mut stmt = tx.prepare_cached("UPDATE files SET rel_path=?2, home_rel=NULL WHERE id=?1")?;
            for (id, rel) in moves {
                stmt.execute(params![id, rel])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// A folder was renamed from `old` to `new` (library-relative): update the paths inside it.
    pub fn rename_dir(&mut self, old: &str, new: &str) -> Result<usize> {
        let tx = self.conn.transaction()?;
        let n = tx.execute(
            "UPDATE files SET rel_path = ?2 || substr(rel_path, length(?1) + 1)
             WHERE substr(rel_path, 1, length(?1) + 1) = ?1 || '/'",
            params![old, new],
        )?;
        tx.execute(
            "UPDATE work_copies SET rel_path = ?2 || substr(rel_path, length(?1) + 1)
             WHERE substr(rel_path, 1, length(?1) + 1) = ?1 || '/'",
            params![old, new],
        )?;
        tx.commit()?;
        Ok(n)
    }

    /// Copies made in a project's 작품 folder, per photo (library-relative paths).
    pub fn work_copies(&self, project: u32) -> Result<HashMap<i64, Vec<String>>> {
        let mut stmt = self.conn.prepare("SELECT asset_id, rel_path FROM work_copies WHERE project_id=?1")?;
        let mut out: HashMap<i64, Vec<String>> = HashMap::new();
        for row in stmt.query_map([project], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))? {
            let (id, rel) = row?;
            out.entry(id).or_default().push(rel);
        }
        Ok(out)
    }

    /// Record the copies of one photo in a project's 작품 folder (replacing earlier ones).
    pub fn set_work_copies(&mut self, project: u32, asset_id: i64, rels: &[String]) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM work_copies WHERE project_id=?1 AND asset_id=?2", params![project, asset_id])?;
        for rel in rels {
            tx.execute(
                "INSERT OR REPLACE INTO work_copies(project_id, asset_id, rel_path) VALUES (?1, ?2, ?3)",
                params![project, asset_id, rel],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Photos with tag `tag`.
    pub fn tag_members(&self, tag: u32) -> Result<Vec<i64>> {
        let mut stmt = self.conn.prepare_cached("SELECT asset_id FROM asset_tags WHERE tag_id=?1 ORDER BY asset_id")?;
        let v = stmt.query_map([tag], |r| r.get::<_, i64>(0))?.collect::<rusqlite::Result<_>>()?;
        Ok(v)
    }

    /// Full metadata of an asset (for the info panel).
    pub fn metadata(&self, asset_id: i64) -> Result<Metadata> {
        self.conn
            .query_row(
                "SELECT captured_at, date_source, utc_offset, make, model, lens, lens_make, focal_length, focal_35mm,
                    aperture, exposure_time, iso, exposure_bias, flash, exposure_program, metering_mode, white_balance,
                    width, height, orientation, gps_lat, gps_lon, gps_alt, serial, software, artist
                 FROM assets WHERE id=?1",
                [asset_id],
                |r| {
                    let captured: Option<String> = r.get(0)?;
                    let lat: Option<f64> = r.get(20)?;
                    let lon: Option<f64> = r.get(21)?;
                    Ok(Metadata {
                        captured_at: captured
                            .and_then(|s| NaiveDateTime::parse_from_str(&s, "%Y-%m-%d %H:%M:%S%.f").ok()),
                        date_source: if r.get::<_, String>(1)? == "exif" {
                            DateSource::Exif
                        } else {
                            DateSource::FileTime
                        },
                        offset: r.get(2)?,
                        make: r.get(3)?,
                        model: r.get(4)?,
                        lens: r.get(5)?,
                        lens_make: r.get(6)?,
                        focal_length: r.get::<_, Option<f64>>(7)?.map(|v| v as f32),
                        focal_35mm: r.get::<_, Option<f64>>(8)?.map(|v| v as f32),
                        aperture: r.get::<_, Option<f64>>(9)?.map(|v| v as f32),
                        exposure_time: r.get::<_, Option<f64>>(10)?.map(|v| v as f32),
                        iso: r.get(11)?,
                        exposure_bias: r.get::<_, Option<f64>>(12)?.map(|v| v as f32),
                        flash_fired: r.get(13)?,
                        exposure_program: r.get(14)?,
                        metering_mode: r.get(15)?,
                        white_balance: r.get(16)?,
                        width: r.get(17)?,
                        height: r.get(18)?,
                        orientation: r.get(19)?,
                        gps: match (lat, lon) {
                            (Some(lat), Some(lon)) => Some(crate::metadata::Gps { lat, lon, alt: r.get(22)? }),
                            _ => None,
                        },
                        serial: r.get(23)?,
                        software: r.get(24)?,
                        artist: r.get(25)?,
                        rating: None,
                        reject: false,
                    })
                },
            )
            .context("asset metadata")
    }

    pub fn tag_names_for(&self, asset_id: i64) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT t.name FROM asset_tags at JOIN tags t ON t.id = at.tag_id WHERE at.asset_id=?1 ORDER BY t.kind DESC, t.name",
        )?;
        let v = stmt.query_map([asset_id], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
        Ok(v)
    }

    pub fn imports(&self) -> Result<Vec<ImportInfo>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, started_at, source, label, asset_count, file_count, bytes FROM imports ORDER BY id DESC",
        )?;
        let v = stmt
            .query_map([], |r| {
                Ok(ImportInfo {
                    id: r.get(0)?,
                    started_at: r.get(1)?,
                    source: r.get(2)?,
                    label: r.get(3)?,
                    asset_count: r.get(4)?,
                    file_count: r.get(5)?,
                    bytes: r.get(6)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(v)
    }

    fn row_from(r: &rusqlite::Row) -> rusqlite::Result<(AssetRecord, String, String)> {
        let rec = AssetRecord {
            id: r.get(0)?,
            captured_ms: r.get(1)?,
            camera: 0,
            lens: 0,
            focal: opt_f32(r.get(4)?),
            focal35: opt_f32(r.get(5)?),
            aperture: opt_f32(r.get(6)?),
            shutter: opt_f32(r.get(7)?),
            iso: r.get::<_, Option<u32>>(8)?.unwrap_or(0),
            bias: opt_f32(r.get(9)?),
            width: r.get::<_, Option<u32>>(10)?.unwrap_or(0),
            height: r.get::<_, Option<u32>>(11)?.unwrap_or(0),
            orientation: r.get::<_, Option<u8>>(12)?.unwrap_or(1),
            rating: r.get(13)?,
            formats: r.get(14)?,
            flash: r.get::<_, Option<bool>>(15)?.unwrap_or(false),
            has_gps: r.get::<_, Option<f64>>(16)?.is_some(),
            date_from_file: r.get::<_, String>(17)? != "exif",
            import_id: r.get::<_, Option<i64>>(18)?.unwrap_or(0),
            tags: Vec::new(),
            name: r.get(19)?,
            flag: r.get::<_, i64>(20)? as i8,
            scene: r.get::<_, i64>(21)? as u8,
            b_prob: opt_f32(r.get(22)?),
            reasons: r.get::<_, Option<i64>>(23)?.unwrap_or(0) as u16,
            burst: r.get::<_, Option<i64>>(24)?.unwrap_or(0),
            burst_rank: r.get::<_, Option<i64>>(25)?.unwrap_or(0) as u16,
            burst_size: r.get::<_, Option<i64>>(26)?.unwrap_or(0) as u16,
            keep_rank: f32::NAN,
        };
        Ok((
            rec,
            r.get::<_, Option<String>>(2)?.unwrap_or_default(),
            r.get::<_, Option<String>>(3)?.unwrap_or_default(),
        ))
    }

    const ROW_SQL: &'static str = "SELECT a.id, a.captured_ms, a.camera, a.lens, a.focal_length, a.focal_35mm, a.aperture,
        a.exposure_time, a.iso, a.exposure_bias, a.width, a.height, a.orientation, a.rating, a.formats, a.flash, a.gps_lat,
        a.date_source, a.import_id, a.name, a.flag, COALESCE(a.scene_user, an.scene, 0), an.b_prob, an.reasons, an.burst,
        an.burst_rank, an.burst_size
        FROM assets a LEFT JOIN analysis an ON an.asset_id = a.id";

    /// Load the whole library into memory.
    pub fn load_index(&self) -> Result<LibraryIndex> {
        let mut idx = LibraryIndex::new();
        let mut by_id: HashMap<i64, usize> = HashMap::new();
        {
            let mut stmt = self.conn.prepare(Self::ROW_SQL)?;
            let rows = stmt.query_map([], Self::row_from)?;
            for row in rows {
                let (mut rec, cam, lens) = row?;
                rec.camera = idx.cameras.intern(&cam);
                rec.lens = idx.lenses.intern(&lens);
                by_id.insert(rec.id, idx.assets.len());
                idx.assets.push(rec);
            }
        }
        {
            for t in self.tags()? {
                idx.tags.insert(t.id, t);
            }
            idx.projects = self.projects()?;
        }
        {
            let mut stmt = self.conn.prepare("SELECT asset_id, tag_id FROM asset_tags")?;
            let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)? as u32)))?;
            for row in rows {
                let (a, t) = row?;
                if let Some(&i) = by_id.get(&a) {
                    idx.assets[i].tags.push(t);
                }
            }
        }
        idx.imports = self.imports()?;
        idx.reindex();
        Ok(idx)
    }

    /// Load one asset with resolved names.
    pub fn load_row(&self, id: i64) -> Result<Option<AssetRow>> {
        let sql = format!("{} WHERE a.id=?1", Self::ROW_SQL);
        let Some((mut rec, camera, lens)) = self.conn.query_row(&sql, [id], Self::row_from).optional()? else {
            return Ok(None);
        };
        let mut stmt = self.conn.prepare_cached(
            "SELECT t.id, t.name, t.kind, t.project_id, t.sort FROM asset_tags at JOIN tags t ON t.id=at.tag_id WHERE at.asset_id=?1",
        )?;
        let tags: Vec<TagInfo> = stmt.query_map([id], tag_from_row)?.collect::<rusqlite::Result<_>>()?;
        rec.tags = tags.iter().map(|t| t.id).collect();
        Ok(Some(AssetRow { rec, camera, lens, tags }))
    }

    pub fn all_asset_ids(&self) -> Result<Vec<i64>> {
        let mut stmt = self.conn.prepare("SELECT id FROM assets ORDER BY captured_ms")?;
        let v = stmt.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
        Ok(v)
    }

    /// Remove assets from the catalog (files on disk are not touched).
    pub fn forget(&mut self, ids: &[i64]) -> Result<()> {
        let tx = self.conn.transaction()?;
        {
            let mut stmt = tx.prepare_cached("DELETE FROM assets WHERE id=?1")?;
            for id in ids {
                stmt.execute([id])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Export every asset as CSV (one row per asset).
    pub fn export_csv<W: std::io::Write>(&self, mut w: W) -> Result<usize> {
        fn q(s: &str) -> String {
            if s.contains([',', '"', '\n']) {
                format!("\"{}\"", s.replace('"', "\"\""))
            } else {
                s.to_string()
            }
        }
        writeln!(
            w,
            "id,captured_at,camera,lens,focal_mm,focal_35mm,aperture,shutter_s,iso,ev,width,height,rating,format,files,tags,gps_lat,gps_lon,flag,scene,b_cut_probability"
        )?;
        let mut stmt = self.conn.prepare(
            "SELECT a.id, a.captured_at, a.camera, a.lens, a.focal_length, a.focal_35mm, a.aperture, a.exposure_time,
                    a.iso, a.exposure_bias, a.width, a.height, a.rating, a.formats,
                    (SELECT group_concat(rel_path, ' | ') FROM files f WHERE f.asset_id=a.id),
                    (SELECT group_concat(t.name, ' | ') FROM asset_tags at JOIN tags t ON t.id=at.tag_id WHERE at.asset_id=a.id),
                    a.gps_lat, a.gps_lon, a.flag, COALESCE(a.scene_user, an.scene, 0), an.b_prob
             FROM assets a LEFT JOIN analysis an ON an.asset_id = a.id ORDER BY a.captured_ms",
        )?;
        let mut rows = stmt.query([])?;
        let mut n = 0;
        let num = |v: Option<f64>| v.map(|x| format!("{}", (x * 10000.0).round() / 10000.0)).unwrap_or_default();
        while let Some(r) = rows.next()? {
            let s = |i: usize| -> rusqlite::Result<String> { Ok(r.get::<_, Option<String>>(i)?.unwrap_or_default()) };
            writeln!(
                w,
                "{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
                r.get::<_, i64>(0)?,
                q(&s(1)?),
                q(&s(2)?),
                q(&s(3)?),
                num(r.get(4)?),
                num(r.get(5)?),
                num(r.get(6)?),
                num(r.get(7)?),
                r.get::<_, Option<i64>>(8)?.map(|v| v.to_string()).unwrap_or_default(),
                num(r.get(9)?),
                r.get::<_, Option<i64>>(10)?.map(|v| v.to_string()).unwrap_or_default(),
                r.get::<_, Option<i64>>(11)?.map(|v| v.to_string()).unwrap_or_default(),
                r.get::<_, i64>(12)?,
                formats::label(r.get::<_, u8>(13)?),
                q(&s(14)?),
                q(&s(15)?),
                num(r.get(16)?),
                num(r.get(17)?),
                match r.get::<_, i64>(18)? {
                    x if x < 0 => "reject",
                    x if x > 0 => "pick",
                    _ => "",
                },
                match r.get::<_, i64>(19)? {
                    0 => "",
                    v => crate::cull::Scene::from_u8(v as u8).label(),
                },
                r.get::<_, Option<f64>>(20)?.map(|p| format!("{:.3}", p)).unwrap_or_default(),
            )?;
            n += 1;
        }
        Ok(n)
    }
}

fn insert_files(tx: &rusqlite::Transaction, asset_id: i64, files: &[NewFile]) -> Result<()> {
    let mut stmt = tx.prepare_cached(
        "INSERT INTO files(asset_id, role, rel_path, orig_name, orig_path, size, hash, quick_key, mtime)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
    )?;
    for f in files {
        stmt.execute(params![
            asset_id,
            f.kind.role(),
            f.rel_path,
            f.orig_name,
            f.orig_path,
            f.size as i64,
            f.hash,
            f.quick_key,
            f.mtime
        ])?;
    }
    Ok(())
}

fn tag_from_row(r: &rusqlite::Row) -> rusqlite::Result<TagInfo> {
    Ok(TagInfo {
        id: r.get::<_, i64>(0)? as u32,
        name: r.get(1)?,
        kind: TagKind::from_i64(r.get(2)?),
        project: r.get::<_, Option<i64>>(3)?.map(|p| p as u32),
        sort: r.get(4)?,
    })
}

/// Project names are the first part of their theme tags ("<project>:<theme>").
fn clean_project_name(name: &str) -> Result<String> {
    let n = name.trim().replace(':', "·");
    if n.is_empty() {
        anyhow::bail!("작품 이름이 비어 있습니다");
    }
    Ok(n)
}

fn clean_theme_name(name: &str) -> Result<String> {
    let n = name.trim().to_string();
    if n.is_empty() {
        anyhow::bail!("테마 이름이 비어 있습니다");
    }
    Ok(n)
}

fn ensure_tag(tx: &rusqlite::Transaction, name: &str, kind: TagKind) -> Result<i64> {
    let mut sel = tx.prepare_cached("SELECT id FROM tags WHERE name=?1")?;
    if let Some(id) = sel.query_row([name], |r| r.get::<_, i64>(0)).optional()? {
        return Ok(id);
    }
    tx.execute("INSERT INTO tags(name, kind) VALUES (?1, ?2)", params![name, kind as i64])?;
    Ok(tx.last_insert_rowid())
}

fn file_from_row(r: &rusqlite::Row) -> rusqlite::Result<FileRecord> {
    Ok(FileRecord {
        id: r.get(0)?,
        asset_id: r.get(1)?,
        kind: FileKind::from_role(&r.get::<_, String>(2)?).unwrap_or(FileKind::Jpeg),
        rel_path: r.get(3)?,
        orig_name: r.get(4)?,
        orig_path: r.get(5)?,
        size: r.get::<_, i64>(6)? as u64,
        hash: r.get(7)?,
        home_rel: r.get(8)?,
    })
}

fn add_tag_tx(tx: &rusqlite::Transaction, asset_id: i64, name: &str, kind: TagKind) -> Result<i64> {
    let id = ensure_tag(tx, name, kind)?;
    tx.execute("INSERT OR IGNORE INTO asset_tags(asset_id, tag_id) VALUES (?1, ?2)", params![asset_id, id])?;
    Ok(id)
}

impl Catalog {
    pub fn tags(&self) -> Result<Vec<TagInfo>> {
        let mut stmt = self.conn.prepare("SELECT id, name, kind, project_id, sort FROM tags")?;
        let v = stmt.query_map([], tag_from_row)?.collect::<rusqlite::Result<_>>()?;
        Ok(v)
    }

    pub fn projects(&self) -> Result<Vec<ProjectInfo>> {
        let mut stmt = self.conn.prepare("SELECT id, name, note, sort FROM projects ORDER BY sort, id")?;
        let v = stmt
            .query_map([], |r| {
                Ok(ProjectInfo { id: r.get::<_, i64>(0)? as u32, name: r.get(1)?, note: r.get(2)?, sort: r.get(3)? })
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(v)
    }

    pub fn meta_get(&self, key: &str) -> Result<Option<String>> {
        Ok(self.conn.query_row("SELECT value FROM meta WHERE key=?1", [key], |r| r.get(0)).optional()?)
    }

    pub fn meta_set(&self, key: &str, value: Option<&str>) -> Result<()> {
        match value {
            Some(v) => self.conn.execute(
                "INSERT INTO meta(key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                params![key, v],
            )?,
            None => self.conn.execute("DELETE FROM meta WHERE key=?1", [key])?,
        };
        Ok(())
    }

    /// Create a project (작품) with its themes, in the given order.
    pub fn create_project(&mut self, name: &str, themes: &[String]) -> Result<ProjectInfo> {
        let name = clean_project_name(name)?;
        let tx = self.conn.transaction()?;
        let exists: bool =
            tx.query_row("SELECT COUNT(*) FROM projects WHERE name=?1", [&name], |r| r.get::<_, i64>(0))? > 0;
        if exists {
            anyhow::bail!("'{name}' 작품이 이미 있습니다");
        }
        let sort: i64 = tx.query_row("SELECT COALESCE(MAX(sort), 0) + 1 FROM projects", [], |r| r.get(0))?;
        let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        tx.execute("INSERT INTO projects(name, sort, created_at) VALUES (?1, ?2, ?3)", params![name, sort, now])?;
        let id = tx.last_insert_rowid() as u32;
        let mut seen = std::collections::HashSet::new();
        for t in themes {
            let Ok(t) = clean_theme_name(t) else { continue };
            if seen.insert(t.clone()) {
                add_theme_tx(&tx, id, &name, &t)?;
            }
        }
        tx.commit()?;
        Ok(ProjectInfo { id, name, note: String::new(), sort })
    }

    /// Add a theme to a project (an existing tag of the same name becomes the theme).
    pub fn add_theme(&mut self, project: u32, theme: &str) -> Result<TagInfo> {
        let theme = clean_theme_name(theme)?;
        let tx = self.conn.transaction()?;
        let pname: String =
            tx.query_row("SELECT name FROM projects WHERE id=?1", [project], |r| r.get(0)).context("작품 없음")?;
        let id = add_theme_tx(&tx, project, &pname, &theme)?;
        let t = tx.query_row("SELECT id, name, kind, project_id, sort FROM tags WHERE id=?1", [id], tag_from_row)?;
        tx.commit()?;
        Ok(t)
    }

    /// Asset ids that carry any of the given tags (for refreshing sidecars).
    fn assets_with_tags(tx: &rusqlite::Transaction, tags: &[i64]) -> Result<Vec<i64>> {
        let mut out = Vec::new();
        let mut stmt = tx.prepare_cached("SELECT asset_id FROM asset_tags WHERE tag_id=?1")?;
        for t in tags {
            for id in stmt.query_map([t], |r| r.get::<_, i64>(0))? {
                out.push(id?);
            }
        }
        out.sort_unstable();
        out.dedup();
        Ok(out)
    }

    fn theme_ids(tx: &rusqlite::Transaction, project: u32) -> Result<Vec<i64>> {
        let mut stmt = tx.prepare_cached("SELECT id FROM tags WHERE project_id=?1")?;
        let v = stmt.query_map([project], |r| r.get::<_, i64>(0))?.collect::<rusqlite::Result<_>>()?;
        Ok(v)
    }

    /// Rename a project; returns the photos whose keywords changed.
    pub fn rename_project(&mut self, project: u32, name: &str) -> Result<Vec<i64>> {
        let name = clean_project_name(name)?;
        let tx = self.conn.transaction()?;
        let clash: bool =
            tx.query_row("SELECT COUNT(*) FROM projects WHERE name=?1 AND id<>?2", params![name, project], |r| {
                r.get::<_, i64>(0)
            })? > 0;
        if clash {
            anyhow::bail!("'{name}' 작품이 이미 있습니다");
        }
        tx.execute("UPDATE projects SET name=?2 WHERE id=?1", params![project, name])?;
        let themes = Self::theme_ids(&tx, project)?;
        for t in &themes {
            let old: String = tx.query_row("SELECT name FROM tags WHERE id=?1", [t], |r| r.get(0))?;
            let theme = old.split_once(':').map(|(_, t)| t.to_string()).unwrap_or(old);
            let new = format!("{name}:{theme}");
            let taken: bool =
                tx.query_row("SELECT COUNT(*) FROM tags WHERE name=?1 AND id<>?2", params![new, t], |r| {
                    r.get::<_, i64>(0)
                })? > 0;
            if taken {
                anyhow::bail!("'{new}' 태그가 이미 있습니다");
            }
            tx.execute("UPDATE tags SET name=?2 WHERE id=?1", params![t, new])?;
        }
        let affected = Self::assets_with_tags(&tx, &themes)?;
        tx.commit()?;
        Ok(affected)
    }

    /// Delete a project and its themes (the photos stay); returns the photos that had them.
    pub fn delete_project(&mut self, project: u32) -> Result<Vec<i64>> {
        let tx = self.conn.transaction()?;
        let themes = Self::theme_ids(&tx, project)?;
        let affected = Self::assets_with_tags(&tx, &themes)?;
        for t in &themes {
            tx.execute("DELETE FROM asset_tags WHERE tag_id=?1", [t])?;
            tx.execute("DELETE FROM tags WHERE id=?1", [t])?;
        }
        tx.execute("DELETE FROM projects WHERE id=?1", [project])?;
        tx.commit()?;
        Ok(affected)
    }

    pub fn rename_theme(&mut self, tag: u32, theme: &str) -> Result<Vec<i64>> {
        let theme = clean_theme_name(theme)?;
        let tx = self.conn.transaction()?;
        let project: i64 = tx
            .query_row("SELECT project_id FROM tags WHERE id=?1 AND kind=2", [tag], |r| r.get(0))
            .context("테마 없음")?;
        let pname: String = tx.query_row("SELECT name FROM projects WHERE id=?1", [project], |r| r.get(0))?;
        let new = format!("{pname}:{theme}");
        let taken: bool =
            tx.query_row("SELECT COUNT(*) FROM tags WHERE name=?1 AND id<>?2", params![new, tag], |r| {
                r.get::<_, i64>(0)
            })? > 0;
        if taken {
            anyhow::bail!("'{theme}' 테마가 이미 있습니다");
        }
        tx.execute("UPDATE tags SET name=?2 WHERE id=?1", params![tag, new])?;
        let affected = Self::assets_with_tags(&tx, &[tag as i64])?;
        tx.commit()?;
        Ok(affected)
    }

    pub fn delete_theme(&mut self, tag: u32) -> Result<Vec<i64>> {
        let tx = self.conn.transaction()?;
        let affected = Self::assets_with_tags(&tx, &[tag as i64])?;
        tx.execute("DELETE FROM asset_tags WHERE tag_id=?1", [tag])?;
        tx.execute("DELETE FROM tags WHERE id=?1 AND kind=2", [tag])?;
        tx.commit()?;
        Ok(affected)
    }

    /// Move a theme up (-1) or down (+1) within its project.
    pub fn move_theme(&mut self, tag: u32, delta: i32) -> Result<()> {
        let tx = self.conn.transaction()?;
        let project: i64 = tx.query_row("SELECT project_id FROM tags WHERE id=?1", [tag], |r| r.get(0))?;
        let mut ids: Vec<i64> = {
            let mut stmt = tx.prepare("SELECT id FROM tags WHERE project_id=?1 ORDER BY sort, id")?;
            let v = stmt.query_map([project], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
            v
        };
        if let Some(i) = ids.iter().position(|&t| t == tag as i64) {
            let j = (i as i64 + delta as i64).clamp(0, ids.len() as i64 - 1) as usize;
            let t = ids.remove(i);
            ids.insert(j, t);
        }
        for (k, t) in ids.iter().enumerate() {
            tx.execute("UPDATE tags SET sort=?2 WHERE id=?1", params![t, k as i64 + 1])?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Give the photos a tag by id.
    pub fn assign_tag(&mut self, ids: &[i64], tag: u32) -> Result<()> {
        let tx = self.conn.transaction()?;
        {
            let mut stmt = tx.prepare_cached("INSERT OR IGNORE INTO asset_tags(asset_id, tag_id) VALUES (?1, ?2)")?;
            for id in ids {
                stmt.execute(params![id, tag])?;
            }
        }
        tx.commit()?;
        Ok(())
    }
}

fn add_theme_tx(tx: &rusqlite::Transaction, project: u32, pname: &str, theme: &str) -> Result<i64> {
    let name = format!("{pname}:{theme}");
    let sort: i64 =
        tx.query_row("SELECT COALESCE(MAX(sort), 0) + 1 FROM tags WHERE project_id=?1", [project], |r| r.get(0))?;
    let existing: Option<(i64, Option<i64>)> = tx
        .query_row("SELECT id, project_id FROM tags WHERE name=?1", [&name], |r| Ok((r.get(0)?, r.get(1)?)))
        .optional()?;
    match existing {
        Some((id, Some(p))) if p == project as i64 => Ok(id),
        Some((id, _)) => {
            tx.execute("UPDATE tags SET kind=2, project_id=?2, sort=?3 WHERE id=?1", params![id, project, sort])?;
            Ok(id)
        }
        None => {
            tx.execute(
                "INSERT INTO tags(name, kind, project_id, sort) VALUES (?1, 2, ?2, ?3)",
                params![name, project, sort],
            )?;
            Ok(tx.last_insert_rowid())
        }
    }
}

/// Thumbnail store: a second SQLite file holding small JPEGs keyed by asset id.
/// One file instead of thousands keeps external HDDs (exFAT) fast.
pub struct ThumbStore {
    conn: Connection,
}

impl ThumbStore {
    pub fn open(path: &Path) -> Result<ThumbStore> {
        let conn = Connection::open(path).with_context(|| format!("open thumbnails {}", path.display()))?;
        conn.busy_timeout(std::time::Duration::from_secs(10))?;
        let _: String = conn.query_row("PRAGMA journal_mode=WAL", [], |r| r.get(0)).unwrap_or_default();
        conn.execute_batch(
            "PRAGMA synchronous=NORMAL;
             CREATE TABLE IF NOT EXISTS thumbs (asset_id INTEGER PRIMARY KEY, width INTEGER NOT NULL, height INTEGER NOT NULL, data BLOB NOT NULL);",
        )?;
        Ok(ThumbStore { conn })
    }

    pub fn put(&self, id: i64, w: u32, h: u32, data: &[u8]) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO thumbs(asset_id, width, height, data) VALUES (?1,?2,?3,?4)",
            params![id, w, h, data],
        )?;
        Ok(())
    }

    pub fn get(&self, id: i64) -> Result<Option<Vec<u8>>> {
        let mut stmt = self.conn.prepare_cached("SELECT data FROM thumbs WHERE asset_id=?1")?;
        Ok(stmt.query_row([id], |r| r.get(0)).optional()?)
    }

    pub fn ids(&self) -> Result<std::collections::HashSet<i64>> {
        let mut stmt = self.conn.prepare("SELECT asset_id FROM thumbs")?;
        let v = stmt.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
        Ok(v)
    }

    pub fn delete(&self, ids: &[i64]) -> Result<()> {
        for id in ids {
            self.conn.execute("DELETE FROM thumbs WHERE asset_id=?1", [id])?;
        }
        Ok(())
    }
}
