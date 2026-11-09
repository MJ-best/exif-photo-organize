//! Photo Organize on Android: what the Kotlin app calls, through UniFFI.
//!
//! The library, the import and the culling assistant are the same code as on the desktop
//! (`photoorg-core`, `photoorg-cull`); this crate only gives them an interface the app can call.
//! Cards are read through Android's Storage Access Framework: the app lists the files of the
//! card ([`SourceEntry`]) and opens each one when asked ([`FileOpener`]), handing over a file
//! descriptor. Everything else works on the library's own folder, which the app owns.
//!
//! Long calls (scan, import, analysis, learning) block; the app makes them off the main thread.

use photoorg_core::catalog::{AssetRecord, Catalog, LibraryIndex};
use photoorg_core::cull::{self, learn::reason, LearnSettings, Report, Scene, Score};
use photoorg_core::filter::CullFilter;
use photoorg_core::import::{self, ImportEvent, ImportOptions, ScanResult};
use photoorg_core::media::formats;
use photoorg_core::source::{ListedFile, ListedFs, Source};
use photoorg_core::{format, preview, trash, FileKind, Library};
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

uniffi::setup_scaffolding!();

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum PhotoError {
    // Not `message`: that would clash with Kotlin's Throwable.message.
    #[error("{reason}")]
    Failed { reason: String },
}

impl From<anyhow::Error> for PhotoError {
    fn from(e: anyhow::Error) -> Self {
        PhotoError::Failed { reason: format!("{e:#}") }
    }
}

type Res<T> = Result<T, PhotoError>;

fn fail(message: impl Into<String>) -> PhotoError {
    PhotoError::Failed { reason: message.into() }
}

/// Which photos to show.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum View {
    All,
    /// No stars, not picked or rejected.
    Undecided,
    /// B컷 후보: undecided photos that look like B-cuts.
    Candidates,
    /// A컷 후보: undecided photos among the best of the library.
    Keepers,
    Rejected,
    Picked,
    /// With at least one star.
    Rated,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct Photo {
    pub id: i64,
    pub name: String,
    /// Capture time on the camera's clock, as milliseconds since 1970-01-01 00:00 of that clock
    /// (format it as UTC to show the camera's time).
    pub captured_ms: i64,
    pub rating: i32,
    /// -1 rejected (B컷), 0 undecided, 1 picked (A컷).
    pub flag: i32,
    /// Chance of being a B-cut, in percent (none until analysed).
    pub b_pct: Option<i32>,
    /// A-cut rank: in the best this many percent of the library (none for weaker burst frames
    /// and photos not analysed yet).
    pub top_pct: Option<i32>,
    pub reasons: Vec<String>,
    /// Place in its burst (1 = best) and the burst's size; 0 when not part of a burst.
    pub burst_rank: i32,
    pub burst_size: i32,
    pub scene: String,
    pub video: bool,
    pub raw: bool,
    pub jpeg: bool,
    pub width: i32,
    pub height: i32,
    pub camera: String,
    pub lens: String,
    /// e.g. "35mm · ƒ1.8 · 1/250 · ISO 400".
    pub exposure: String,
}

#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct Counts {
    pub all: i32,
    pub undecided: i32,
    pub candidates: i32,
    pub keepers: i32,
    pub rejected: i32,
    pub picked: i32,
    pub rated: i32,
    /// Photos the culling assistant has looked at.
    pub analysed: i32,
}

/// A file of a card, as the app lists it.
#[derive(Clone, Debug, uniffi::Record)]
pub struct SourceEntry {
    /// Path inside the card or folder, '/' separated (e.g. "DCIM/100CANON/IMG_0001.CR3").
    pub rel: String,
    pub size: i64,
    pub mtime_ms: Option<i64>,
    /// Passed back to [`FileOpener::open`] (the document's URI).
    pub id: String,
}

#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct ScanInfo {
    pub label: String,
    pub photos: i32,
    pub new_photos: i32,
    pub new_files: i32,
    pub new_bytes: i64,
    pub videos: i32,
    pub raw_pairs: i32,
    pub first_ms: Option<i64>,
    pub last_ms: Option<i64>,
    /// "Canon EOS R6 · 120장"
    pub cameras: Vec<String>,
    pub errors: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct ImportStatus {
    pub files_done: i32,
    pub files_total: i32,
    pub bytes_done: i64,
    pub bytes_total: i64,
    pub current: String,
}

#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct ImportResult {
    pub added: i32,
    pub updated: i32,
    pub duplicates: i32,
    pub bytes: i64,
    pub failures: Vec<String>,
    pub cancelled: bool,
}

#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct LearnStatus {
    /// "기본 기준 · 학습까지 A 2 / B 6 (각 6장)" or "내 기준으로 학습됨 · A 40 · B 85".
    pub summary: String,
    pub learned: bool,
    pub a: i32,
    pub b: i32,
    pub analysed: i32,
    pub total: i32,
}

#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct DeleteResult {
    pub deleted: i32,
    pub failed: i32,
    pub bytes: i64,
    pub first_error: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct DeletePlan {
    pub photos: i32,
    pub files: i32,
    pub bytes: i64,
}

/// Opens a listed file for reading.
#[uniffi::export(callback_interface)]
pub trait FileOpener: Send + Sync {
    /// A file descriptor open for reading, which the library then owns and closes; -1 if the
    /// file cannot be opened.
    fn open(&self, id: String) -> i32;
}

#[uniffi::export(callback_interface)]
pub trait ImportListener: Send + Sync {
    fn progress(&self, status: ImportStatus);
}

#[uniffi::export(callback_interface)]
pub trait ProgressListener: Send + Sync {
    fn progress(&self, done: i32, total: i32);
}

struct State {
    catalog: Catalog,
    index: LibraryIndex,
    report: Option<Report>,
    scan: Option<ScanResult>,
}

#[derive(uniffi::Object)]
pub struct PhotoLibrary {
    lib: Library,
    state: Mutex<State>,
    settings: Mutex<Settings>,
    cancel_import: Arc<AtomicBool>,
    cancel_analysis: Arc<AtomicBool>,
}

#[derive(Clone, Copy)]
struct Settings {
    /// B컷 후보 from this B-cut chance (percent).
    threshold: u8,
    /// A컷 후보: the best this many percent.
    keep_pct: u8,
    learn: LearnSettings,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { threshold: 60, keep_pct: 15, learn: LearnSettings::default() }
    }
}

impl Settings {
    fn filter(&self, view: View) -> Option<CullFilter> {
        Some(match view {
            View::All | View::Rated => return None,
            View::Undecided => CullFilter::Undecided,
            View::Candidates => CullFilter::Candidates(self.threshold),
            View::Keepers => CullFilter::Keepers(self.keep_pct),
            View::Rejected => CullFilter::Rejected,
            View::Picked => CullFilter::Picked,
        })
    }

    fn matches(&self, view: View, a: &AssetRecord) -> bool {
        match view {
            View::Rated => a.rating > 0,
            // "아직 고르지 않은 사진": no stars either.
            View::Undecided => a.flag == 0 && a.rating == 0,
            _ => self.filter(view).is_none_or(|f| f.matches(a)),
        }
    }
}

fn pct(v: f32) -> Option<i32> {
    v.is_finite().then(|| (v * 100.0).round().clamp(0.0, 100.0) as i32)
}

fn photo(index: &LibraryIndex, a: &AssetRecord) -> Photo {
    let mut exposure = Vec::new();
    if a.focal > 0.0 {
        exposure.push(format::focal(a.focal));
    }
    if a.aperture > 0.0 {
        exposure.push(format::aperture(a.aperture));
    }
    if a.shutter > 0.0 {
        exposure.push(format::shutter(a.shutter));
    }
    if a.iso > 0 {
        exposure.push(format::iso(a.iso));
    }
    Photo {
        id: a.id,
        name: a.name.clone(),
        captured_ms: a.captured_ms,
        rating: a.rating as i32,
        flag: a.flag as i32,
        b_pct: pct(a.b_prob),
        top_pct: pct(a.keep_rank).map(|p| p.max(1)),
        reasons: reason::labels(a.reasons).into_iter().map(String::from).collect(),
        burst_rank: if a.burst_size > 1 { a.burst_rank as i32 + 1 } else { 0 },
        burst_size: if a.burst_size > 1 { a.burst_size as i32 } else { 0 },
        scene: if a.b_prob.is_finite() { Scene::from_u8(a.scene).label().to_string() } else { String::new() },
        video: a.is_video(),
        raw: a.formats & formats::RAW != 0,
        jpeg: a.formats & formats::JPEG != 0,
        width: a.width as i32,
        height: a.height as i32,
        camera: index.cameras.name(a.camera).to_string(),
        lens: index.lenses.name(a.lens).to_string(),
        exposure: exposure.join(" · "),
    }
}

fn learn_status(index: &LibraryIndex, report: Option<&Report>) -> LearnStatus {
    let total = index.assets.iter().filter(|a| !a.is_video()).count() as i32;
    let analysed = index.assets.iter().filter(|a| a.b_prob.is_finite()).count() as i32;
    let Some(r) = report.and_then(|r| r.scenes.first()) else {
        return LearnStatus { summary: "기본 기준".into(), analysed, total, ..Default::default() };
    };
    let scenes = report.map(|r| r.scenes.iter().filter(|s| s.model == "장면별 학습").count()).unwrap_or(0);
    let learned = r.model != "기본값";
    let summary = match r.model {
        "기본값" => format!("기본 기준 · 학습까지 A {} / B {} (각 6장)", r.a.min(6), r.b.min(6)),
        _ if scenes > 0 => format!("내 기준으로 학습됨 · A {} · B {} · 장면별 {scenes}개", r.a, r.b),
        _ => format!("내 기준으로 학습됨 · A {} · B {}", r.a, r.b),
    };
    LearnStatus { summary, learned, a: r.a as i32, b: r.b as i32, analysed, total }
}

#[cfg(unix)]
fn file_from_fd(fd: i32) -> std::io::Result<File> {
    use std::os::fd::FromRawFd;
    if fd < 0 {
        return Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "열 수 없습니다"));
    }
    // SAFETY: the app hands over a descriptor it detached for us (ParcelFileDescriptor.detachFd).
    Ok(unsafe { File::from_raw_fd(fd) })
}

#[cfg(not(unix))]
fn file_from_fd(_fd: i32) -> std::io::Result<File> {
    Err(std::io::Error::other("file descriptors are only supported on Android"))
}

#[uniffi::export]
impl PhotoLibrary {
    /// Open (or create) the library in the folder `root`.
    #[uniffi::constructor]
    pub fn open(root: String) -> Res<Arc<Self>> {
        init_logging();
        let lib = Library::open(Path::new(&root))?;
        let catalog = lib.catalog()?;
        let mut index = catalog.load_index()?;
        index.rank_keepers();
        Ok(Arc::new(PhotoLibrary {
            lib,
            state: Mutex::new(State { catalog, index, report: None, scan: None }),
            settings: Mutex::new(Settings::default()),
            cancel_import: Arc::new(AtomicBool::new(false)),
            cancel_analysis: Arc::new(AtomicBool::new(false)),
        }))
    }

    pub fn root(&self) -> String {
        self.lib.root.display().to_string()
    }

    pub fn photos(&self, view: View) -> Vec<Photo> {
        let settings = *self.settings();
        let st = self.state();
        st.index.assets.iter().filter(|a| settings.matches(view, a)).map(|a| photo(&st.index, a)).collect()
    }

    pub fn photo(&self, id: i64) -> Option<Photo> {
        let st = self.state();
        st.index.get(id).map(|a| photo(&st.index, a))
    }

    pub fn counts(&self) -> Counts {
        let settings = *self.settings();
        let st = self.state();
        let mut c = Counts::default();
        for a in &st.index.assets {
            c.all += 1;
            let views = [
                (View::Undecided, &mut c.undecided),
                (View::Candidates, &mut c.candidates),
                (View::Keepers, &mut c.keepers),
                (View::Rejected, &mut c.rejected),
                (View::Picked, &mut c.picked),
                (View::Rated, &mut c.rated),
            ];
            for (v, n) in views {
                if settings.matches(v, a) {
                    *n += 1;
                }
            }
            if a.b_prob.is_finite() {
                c.analysed += 1;
            }
        }
        c
    }

    /// The thumbnail (JPEG) made when the photo was imported.
    pub fn thumbnail(&self, id: i64) -> Option<Vec<u8>> {
        self.lib.thumbs().ok()?.get(id).ok().flatten()
    }

    /// A JPEG of the photo for viewing, its long edge about `max_edge` (from the preview inside
    /// RAW files; RAW data itself is never developed).
    pub fn preview(&self, id: i64, max_edge: i32) -> Res<Vec<u8>> {
        let (files, orientation) = {
            let st = self.state();
            (st.catalog.files_for(id)?, st.catalog.metadata(id).map(|m| m.orientation).unwrap_or(1))
        };
        let f = files
            .iter()
            .filter(|f| !f.kind.is_video())
            .min_by_key(|f| f.kind.preview_priority())
            .ok_or_else(|| fail("미리보기가 없습니다"))?;
        let img = preview::load_image(
            &self.lib.abs(&f.rel_path),
            f.kind,
            orientation,
            max_edge.clamp(64, 8192) as u32,
            None,
        )?;
        Ok(preview::encode_jpeg(&img, 88)?)
    }

    /// Stars (0 clears them). Written to the catalog and the XMP sidecars.
    pub fn set_rating(&self, ids: Vec<i64>, rating: i32) -> Res<()> {
        let rating = rating.clamp(0, 5) as u8;
        let mut st = self.state();
        st.catalog.set_rating(&ids, rating)?;
        for id in &ids {
            if let Some(a) = st.index.get_mut(*id) {
                a.rating = rating;
            }
        }
        self.write_sidecars(&st.catalog, &ids);
        Ok(())
    }

    /// Pick (1), reject (-1) or clear (0).
    pub fn set_flag(&self, ids: Vec<i64>, flag: i32) -> Res<()> {
        let flag = flag.signum() as i8;
        let mut st = self.state();
        st.catalog.set_flag(&ids, flag)?;
        for id in &ids {
            if let Some(a) = st.index.get_mut(*id) {
                a.flag = flag;
            }
        }
        self.write_sidecars(&st.catalog, &ids);
        Ok(())
    }

    /// B컷 후보 from this chance (percent, 30–90).
    pub fn set_threshold(&self, percent: i32) {
        self.settings().threshold = percent.clamp(30, 90) as u8;
    }

    pub fn threshold(&self) -> i32 {
        self.settings().threshold as i32
    }

    /// Look at the files of a card or folder: what is new, how much, from which cameras.
    /// Keeps the result for [`Self::import_scanned`].
    pub fn scan(
        &self,
        label: String,
        files: Vec<SourceEntry>,
        opener: Box<dyn FileOpener>,
        include_videos: bool,
    ) -> Res<ScanInfo> {
        self.cancel_import.store(false, Ordering::Relaxed);
        let root = PathBuf::from("/source").join(sanitize_label(&label));
        let listed = files
            .into_iter()
            .map(|e| ListedFile { rel: e.rel, size: e.size.max(0) as u64, mtime_ms: e.mtime_ms, id: e.id })
            .collect();
        let opener: Arc<dyn FileOpener> = Arc::from(opener);
        let fs = Source(Arc::new(ListedFs::new(
            &root,
            listed,
            Box::new(move |id| file_from_fd(opener.open(id.to_string()))),
        )));
        let mut scan = import::scan_in(&fs, &root, include_videos, &self.cancel_import, &|_, _| {})?;
        scan.label = label;
        {
            let st = self.state();
            import::mark_existing(&mut scan, &st.catalog)?;
        }
        let s = scan.summary(include_videos);
        let info = ScanInfo {
            label: scan.label.clone(),
            photos: s.groups as i32,
            new_photos: s.new_groups as i32,
            new_files: s.new_files as i32,
            new_bytes: s.new_bytes as i64,
            videos: s.videos as i32,
            raw_pairs: s.raw_pairs as i32,
            first_ms: s.first.map(photoorg_core::catalog::naive_to_ms),
            last_ms: s.last.map(photoorg_core::catalog::naive_to_ms),
            cameras: s.cameras.iter().map(|(c, n)| format!("{c} · {}장", format::count(*n))).collect(),
            errors: scan.errors.clone(),
        };
        self.state().scan = Some(scan);
        Ok(info)
    }

    /// Copy what [`Self::scan`] found into the library (checksummed), catalog it and make
    /// thumbnails. Blocks until done; [`Self::cancel_import`] stops it (photos already copied
    /// stay).
    pub fn import_scanned(&self, listener: Box<dyn ImportListener>) -> Res<ImportResult> {
        let scan = self.state().scan.take().ok_or_else(|| fail("먼저 카드를 살펴보세요"))?;
        self.cancel_import.store(false, Ordering::Relaxed);
        let opts = ImportOptions { include_videos: true, ..Default::default() };
        let (tx, rx) = import::channel();
        let summary = std::thread::scope(|s| {
            let forward = s.spawn(move || {
                for ev in rx {
                    if let ImportEvent::Progress(p) = ev {
                        listener.progress(ImportStatus {
                            files_done: p.files_done as i32,
                            files_total: p.files_total as i32,
                            bytes_done: p.bytes_done as i64,
                            bytes_total: p.bytes_total as i64,
                            current: p.current,
                        });
                    }
                }
            });
            let res = import::run_import(&self.lib, &scan, &opts, &tx, self.cancel_import.clone());
            drop(tx);
            let _ = forward.join();
            res
        })?;
        self.reload()?;
        Ok(ImportResult {
            added: summary.assets_added as i32,
            updated: summary.assets_updated as i32,
            duplicates: summary.duplicates as i32,
            bytes: summary.bytes_copied as i64,
            failures: summary.failures,
            cancelled: summary.cancelled,
        })
    }

    pub fn cancel_import(&self) {
        self.cancel_import.store(true, Ordering::Relaxed);
    }

    /// Photos the culling assistant has not looked at yet.
    pub fn pending_analysis(&self) -> i32 {
        self.pending().map(|v| v.len() as i32).unwrap_or(0)
    }

    /// Let the culling assistant look at every photo it has not seen yet, then learn from the
    /// decisions and score the library. Blocks; [`Self::cancel_analysis`] stops it.
    pub fn analyze(&self, listener: Box<dyn ProgressListener>) -> Res<i32> {
        self.cancel_analysis.store(false, Ordering::Relaxed);
        let ids = self.pending()?;
        let total = ids.len() as i32;
        listener.progress(0, total);
        let done = cull::analyze_assets(&self.lib, &ids, &self.cancel_analysis, &|_, n| {
            if n % 8 == 0 || n as i32 == total {
                listener.progress(n as i32, total);
            }
        })?;
        self.reload()?;
        self.learn()?;
        Ok(done as i32)
    }

    pub fn cancel_analysis(&self) {
        self.cancel_analysis.store(true, Ordering::Relaxed);
    }

    /// Learn from the current picks, rejects and stars, and score every photo again (fast; no
    /// image decoding). Call it after decisions change.
    pub fn learn(&self) -> Res<LearnStatus> {
        let settings = self.settings().learn;
        let mut cat = self.lib.catalog()?;
        let (scores, report) = cull::rescore(&mut cat, &settings)?;
        let mut st = self.state();
        apply_scores(&mut st.index, &scores);
        let status = learn_status(&st.index, Some(&report));
        st.report = Some(report);
        Ok(status)
    }

    pub fn learn_status(&self) -> LearnStatus {
        let st = self.state();
        learn_status(&st.index, st.report.as_ref())
    }

    /// What deleting these photos would move to the library's trash.
    pub fn delete_plan(&self, ids: Vec<i64>) -> DeletePlan {
        let st = self.state();
        let p = trash::plan(&self.lib, &st.catalog, &ids).unwrap_or_default();
        DeletePlan { photos: p.photos as i32, files: p.files as i32, bytes: p.bytes as i64 }
    }

    /// Move the photos' files to the library's trash folder and take them out of the library.
    /// Decisions about them stay for the culling assistant.
    pub fn delete(&self, ids: Vec<i64>) -> Res<DeleteResult> {
        let mut st = self.state();
        let report = trash::trash_assets_in(&self.lib, &mut st.catalog, &ids, trash::Bin::Library)?;
        st.index.remove(&report.trashed);
        Ok(DeleteResult {
            deleted: report.trashed.len() as i32,
            failed: report.failed.len() as i32,
            bytes: report.bytes as i64,
            first_error: report.failed.first().map(|(_, e)| e.clone()),
        })
    }

    /// Bytes in the library's trash.
    pub fn trash_bytes(&self) -> i64 {
        dir_size(&self.lib.trash_dir()) as i64
    }

    /// Delete the trash for good.
    pub fn empty_trash(&self) -> Res<()> {
        let dir = self.lib.trash_dir();
        if dir.exists() {
            std::fs::remove_dir_all(&dir).map_err(|e| fail(format!("휴지통을 비우지 못했습니다: {e}")))?;
        }
        Ok(())
    }
}

impl PhotoLibrary {
    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn settings(&self) -> MutexGuard<'_, Settings> {
        self.settings.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn pending(&self) -> Res<Vec<i64>> {
        let st = self.state();
        let done = st.catalog.analyzed_ids(cull::ANALYSIS_VERSION)?;
        Ok(st.index.assets.iter().filter(|a| !a.is_video() && !done.contains(&a.id)).map(|a| a.id).collect())
    }

    /// Load the index again (after an import or analysis), keeping the scores.
    fn reload(&self) -> Res<()> {
        let cat = self.lib.catalog()?;
        let mut index = cat.load_index()?;
        index.rank_keepers();
        self.state().index = index;
        Ok(())
    }

    fn write_sidecars(&self, catalog: &Catalog, ids: &[i64]) {
        for id in ids {
            if let Err(e) = self.lib.write_xmp(catalog, *id) {
                log::warn!("xmp {id}: {e:#}");
            }
        }
    }
}

fn apply_scores(index: &mut LibraryIndex, scores: &[Score]) {
    for s in scores {
        if let Some(a) = index.get_mut(s.id) {
            a.b_prob = s.b_prob;
            a.reasons = s.reasons;
            a.burst = s.burst;
            a.burst_rank = s.burst_rank;
            a.burst_size = s.burst_size;
            a.scene = s.scene;
        }
    }
    index.rank_keepers();
}

fn sanitize_label(label: &str) -> String {
    let s: String = label.chars().map(|c| if c == '/' || c == '\\' { '_' } else { c }).collect();
    if s.trim().is_empty() {
        "card".into()
    } else {
        s
    }
}

fn dir_size(dir: &Path) -> u64 {
    let Ok(rd) = std::fs::read_dir(dir) else { return 0 };
    rd.flatten()
        .map(|e| match e.file_type() {
            Ok(t) if t.is_dir() => dir_size(&e.path()),
            Ok(_) => e.metadata().map(|m| m.len()).unwrap_or(0),
            Err(_) => 0,
        })
        .sum()
}

fn init_logging() {
    #[cfg(target_os = "android")]
    android_logger::init_once(
        android_logger::Config::default().with_max_level(log::LevelFilter::Info).with_tag("photoorg"),
    );
}

/// Kinds of files the app offers to import (for the folder picker's hint).
#[uniffi::export]
pub fn is_media_name(name: String) -> bool {
    FileKind::from_path(Path::new(&name)).is_some()
}

// The tests hand files over as Unix file descriptors, like Android does.
#[cfg(all(test, unix))]
mod tests;
