//! LOUPE on iOS / iPadOS: lifecycle-safe, zero-reencode UniFFI bridge for the SwiftUI app.
//!
//! Design highlights driven by iOS/iPadOS system constraints:
//! - **No `0xdead10cc` background crashes**: `Catalog` and `ThumbStore` connections are held in
//!   separate locks and explicitly checkpointed (`PRAGMA wal_checkpoint(TRUNCATE)`) + dropped in
//!   [`PhotoLibrary::suspend`], then reopened cleanly in [`PhotoLibrary::resume`].
//! - **Zero-reencode previews**: [`PhotoLibrary::preview_descriptor`] locates the embedded JPEG
//!   byte slice inside RAW/JPEG files so Apple's hardware `ImageIO` decodes directly via mmap
//!   without software re-encoding or FFI byte copies (falling back to rendered JPEG only when
//!   non-destructive crop/straighten framing is active or for PNG/TIFF).
//! - **No per-thumbnail DB opens & separated locks**: `RwLock<IndexState>` serves concurrent grid
//!   and viewer reads without blocking on catalog writes or thumbnail fetches; `ThumbStore` keeps a
//!   persistent SQLite connection.
//! - **Atomic snapshots & windowed paging**: [`PhotoLibrary::snapshot`] returns `ids`, `counts`,
//!   `learn`, and `revision` from a single read lock (no torn reads), and [`PhotoLibrary::photos_batch`]
//!   transfers only the visible grid window across FFI.
//! - **Thermal & Jetsam protection**: [`PhotoLibrary::analyze`] takes a thread budget computed from
//!   `ProcessInfo.thermalState`, and [`release_ml_models`] drops cached ONNX runtime graphs (~100MB+)
//!   on memory pressure or backgrounding.
//! - **Structured domain errors**: [`PhotoError`] distinguishes recoverable conditions (permission
//!   denied, insufficient storage, suspended library, not analyzed, cancelled) so the UI can show
//!   concrete recovery actions instead of opaque error strings.

use photoorg_core::catalog::{AssetRecord, Catalog, LibraryIndex, ThumbStore};
use photoorg_core::cull::{
    self,
    composition::{self, strength, Composition},
    learn::reason,
    LearnSettings, Report, Scene, Score,
};
use photoorg_core::export::{self, ExportKind, ExportOptions};
use photoorg_core::filter::CullFilter;
use photoorg_core::framing::{self, Framing};
use photoorg_core::import::{self, ImportEvent, ImportOptions, ScanResult};
use photoorg_core::media::formats;
use photoorg_core::source::Source;
use photoorg_core::{format, preview, trash, Library};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::time::{Duration, Instant};

uniffi::setup_scaffolding!();

/// Structured domain errors so iOS/iPadOS UI can present targeted recovery actions.
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum PhotoError {
    #[error("라이브러리가 일시 중지 상태입니다")]
    LibrarySuspended,
    #[error("찾을 수 없습니다: {detail}")]
    NotFound { detail: String },
    #[error("폴더 접근 권한이 없거나 만료되었습니다: {detail}")]
    PermissionDenied { detail: String },
    #[error("저장 공간이 부족합니다 (필요: {required_bytes}B, 여유: {available_bytes}B)")]
    InsufficientStorage { required_bytes: i64, available_bytes: i64 },
    #[error("아직 분석되지 않은 사진입니다")]
    NotAnalyzed,
    #[error("동영상에는 적용할 수 없습니다")]
    UnsupportedForVideo,
    #[error("작업이 취소되었습니다")]
    Cancelled,
    #[error("데이터베이스 오류: {detail}")]
    Database { detail: String },
    #[error("파일 처리 오류: {detail}")]
    Io { detail: String },
    #[error("{detail}")]
    Other { detail: String },
}

type Res<T> = Result<T, PhotoError>;

fn classify_anyhow(err: anyhow::Error) -> PhotoError {
    if let Some(io_err) = err.downcast_ref::<std::io::Error>() {
        return match io_err.kind() {
            ErrorKind::PermissionDenied => PhotoError::PermissionDenied { detail: format!("{err:#}") },
            ErrorKind::NotFound => PhotoError::NotFound { detail: format!("{err:#}") },
            _ => PhotoError::Io { detail: format!("{err:#}") },
        };
    }
    let msg = format!("{err:#}");
    let lower = msg.to_ascii_lowercase();
    if lower.contains("permission denied") || lower.contains("operation not permitted") {
        PhotoError::PermissionDenied { detail: msg }
    } else if lower.contains("sqlite") || lower.contains("database") {
        PhotoError::Database { detail: msg }
    } else {
        PhotoError::Other { detail: msg }
    }
}

impl From<anyhow::Error> for PhotoError {
    fn from(err: anyhow::Error) -> Self {
        classify_anyhow(err)
    }
}

/// Drop cached ONNX neural network runtime graphs (YuNet, MobileNetV2, PP-HumanSeg) to reclaim
/// heap memory under iOS memory warning or background transition. Next analysis reloads cleanly.
#[uniffi::export]
pub fn release_ml_models() {
    cull::release_models();
}

/// Which photos to show (named `LibraryView` so UniFFI Swift bindings never shadow `SwiftUI.View`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum LibraryView {
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum ExportMode {
    /// Original files (RAW, JPEG, XMP sidecars).
    Originals,
    /// One JPEG per photo (camera JPEG or full-size RAW-embedded JPEG).
    JpegOnly,
    /// Re-encoded smaller JPEG with GPS stripped and orientation baked in.
    Resized,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct CompNote {
    pub text: String,
    /// Some(true) = good, Some(false) = B-cut reason, None = neutral info.
    pub ok: Option<bool>,
    /// True when the user turned off this rule in Settings.
    pub ignored: bool,
}

/// Normalised composition geometry (0..1 from top-left) for the toggleable `G` overlay in Viewer.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct CompositionOverlay {
    pub has_subject: bool,
    pub subject_x: f32,
    pub subject_y: f32,
    pub subject_w: f32,
    pub subject_h: f32,
    pub key_x: f32,
    pub key_y: f32,
    pub head_top: Option<f32>,
    /// -1 looking left .. +1 looking right.
    pub facing: f32,
    pub pole: f32,
    pub pole_x: f32,
    pub neck_line: f32,
    pub neck_y: f32,
    pub tilt: f32,
    pub tilt_conf: f32,
    pub tilt_vertical: bool,
    pub tilt_at: f32,
    pub show_thirds: bool,
    pub show_headroom: bool,
    pub show_lead_room: bool,
    pub show_merger: bool,
    pub show_tilt: bool,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct RuleItem {
    pub bit: i64,
    pub group: String,
    pub label: String,
    pub enabled: bool,
}

/// Compact grid representation transferred in small visible-window batches.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct PhotoSummary {
    pub id: i64,
    pub name: String,
    pub captured_ms: i64,
    pub rating: i32,
    /// -1 rejected (B컷), 0 undecided, 1 picked (A컷).
    pub flag: i32,
    pub b_pct: Option<i32>,
    pub top_pct: Option<i32>,
    pub burst_rank: i32,
    pub burst_size: i32,
    pub has_framing: bool,
    pub video: bool,
    pub raw: bool,
    pub jpeg: bool,
    pub width: i32,
    pub height: i32,
}

/// Full metadata + culling reasons + composition details fetched only for the active photo.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct PhotoDetail {
    pub summary: PhotoSummary,
    pub reasons: Vec<String>,
    pub strengths: Vec<String>,
    pub scene: String,
    pub framing: Option<String>,
    pub comp_notes: Vec<CompNote>,
    pub overlay: Option<CompositionOverlay>,
    pub camera: String,
    pub lens: String,
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
    pub analysed: i32,
}

#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct LearnStatus {
    pub summary: String,
    pub learned: bool,
    pub a: i32,
    pub b: i32,
    pub analysed: i32,
    pub total: i32,
}

/// Atomic view snapshot read under a single lock (prevents torn reads between ids and counts).
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct LibrarySnapshot {
    pub view: LibraryView,
    pub ids: Vec<i64>,
    pub counts: Counts,
    pub learn: LearnStatus,
    pub revision: i64,
}

/// Zero-reencode preview descriptor for Apple ImageIO hardware decoding.
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum PreviewDescriptor {
    /// Direct byte range of a JPEG inside a local/security-scoped file; Swift maps & decodes once
    /// with hardware `CGImageSourceCreateThumbnailAtIndex` using EXIF `orientation` (1..8).
    EmbeddedSlice { path: String, offset: u64, length: u64, orientation: i32 },
    /// Pre-rendered JPEG bytes (used when non-destructive crop/straighten framing is active or for
    /// non-JPEG formats like PNG/TIFF).
    RenderedJpeg { bytes: Vec<u8> },
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
pub struct ExportPlan {
    pub photos: i32,
    pub originals_bytes: i64,
    pub jpeg_bytes: i64,
    pub raw_only: i32,
    pub videos: i32,
    pub framed_count: i32,
}

#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct ExportResult {
    pub files: i32,
    pub from_raw: i32,
    pub bytes: i64,
    pub skipped: Vec<String>,
    pub cancelled: bool,
}

#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct DeletePlan {
    pub photos: i32,
    pub files: i32,
    pub bytes: i64,
}

#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct DeleteResult {
    pub deleted: i32,
    pub failed: i32,
    pub bytes: i64,
    pub first_error: Option<String>,
}

/// Snapshot of a photo's prior rating/flag state used for atomic batch Undo.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct DecisionRestore {
    pub id: i64,
    pub rating: i32,
    pub flag: i32,
}

#[uniffi::export(callback_interface)]
pub trait ImportListener: Send + Sync {
    fn progress(&self, status: ImportStatus);
}

#[uniffi::export(callback_interface)]
pub trait ProgressListener: Send + Sync {
    fn progress(&self, done: i32, total: i32);
}

/// Tracks active FFI operations so [`PhotoLibrary::suspend`] can drain all running workers before
/// truncating WAL files and closing SQLite handles (zero lingering file locks on iOS suspend).
struct OpBarrier {
    active: Mutex<usize>,
    cv: Condvar,
}

impl OpBarrier {
    fn new() -> Self {
        OpBarrier { active: Mutex::new(0), cv: Condvar::new() }
    }

    fn try_enter(&self, suspended: &AtomicBool) -> Res<OpGuard<'_>> {
        let mut count = self.active.lock().unwrap_or_else(|e| e.into_inner());
        if suspended.load(Ordering::Acquire) {
            return Err(PhotoError::LibrarySuspended);
        }
        *count += 1;
        Ok(OpGuard { barrier: self })
    }

    fn wait_drained(&self, timeout: Duration) {
        let deadline = Instant::now() + timeout;
        let mut count = self.active.lock().unwrap_or_else(|e| e.into_inner());
        while *count > 0 {
            let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                break;
            };
            let (next, res) = self.cv.wait_timeout(count, remaining).unwrap_or_else(|e| e.into_inner());
            count = next;
            if res.timed_out() {
                break;
            }
        }
    }

    #[cfg(test)]
    fn active_count(&self) -> usize {
        *self.active.lock().unwrap_or_else(|e| e.into_inner())
    }
}

struct OpGuard<'a> {
    barrier: &'a OpBarrier,
}

impl Drop for OpGuard<'_> {
    fn drop(&mut self) {
        let mut count = self.barrier.active.lock().unwrap_or_else(|e| e.into_inner());
        *count = count.saturating_sub(1);
        if *count == 0 {
            self.barrier.cv.notify_all();
        }
    }
}

struct IndexState {
    index: LibraryIndex,
    report: Option<Report>,
}

#[derive(Clone, Copy)]
struct Settings {
    threshold: u8,
    keep_pct: u8,
    learn: LearnSettings,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { threshold: 60, keep_pct: 15, learn: LearnSettings::default() }
    }
}

impl Settings {
    fn filter(&self, view: LibraryView) -> Option<CullFilter> {
        Some(match view {
            LibraryView::All | LibraryView::Rated => return None,
            LibraryView::Undecided => CullFilter::Undecided,
            LibraryView::Candidates => CullFilter::Candidates(self.threshold),
            LibraryView::Keepers => CullFilter::Keepers(self.keep_pct),
            LibraryView::Rejected => CullFilter::Rejected,
            LibraryView::Picked => CullFilter::Picked,
        })
    }

    fn matches(&self, view: LibraryView, a: &AssetRecord) -> bool {
        match view {
            LibraryView::Rated => a.rating > 0,
            LibraryView::Undecided => a.flag == 0 && a.rating == 0,
            _ => self.filter(view).is_none_or(|f| f.matches(a)),
        }
    }
}

#[derive(uniffi::Object)]
pub struct PhotoLibrary {
    lib: Library,
    index: RwLock<IndexState>,
    catalog: Mutex<Option<Catalog>>,
    thumbs: Mutex<Option<ThumbStore>>,
    scan: Mutex<Option<ScanResult>>,
    settings: RwLock<Settings>,
    ops: OpBarrier,
    revision: AtomicI64,
    suspended: AtomicBool,
    cancel_import: Arc<AtomicBool>,
    cancel_analysis: Arc<AtomicBool>,
    cancel_export: Arc<AtomicBool>,
}

const RULE_GROUPS: &[(&str, &[(u32, &str)])] = &[
    (
        "초점 · 노출",
        &[
            (reason::BLUR, "초점 흐림"),
            (reason::SHAKE, "흔들림"),
            (reason::FACE_BLUR, "얼굴 초점 빗나감"),
            (reason::FOCUS_BG, "초점이 배경에"),
            (reason::EYES, "눈 감음"),
            (reason::OVER, "노출 과다"),
            (reason::UNDER, "노출 부족"),
            (reason::NOISE, "고감도 노이즈"),
            (reason::BURST, "연사 중 덜 선명"),
        ],
    ),
    (
        "구도 · 프레이밍",
        &[
            (reason::THIRDS, "3분할"),
            (reason::HEADROOM, "헤드룸 (머리 위 여백)"),
            (reason::LEAD_ROOM, "시선 앞 공간"),
            (reason::MERGER, "머리 뒤 기둥 · 가로선"),
            (reason::TILT, "수평 · 수직 기울어짐"),
            (reason::CUT, "얼굴이 프레임에 잘림"),
        ],
    ),
    ("내 취향", &[(reason::MY_COMPOSITION, "내가 자주 쓰는 구도와 다름"), (reason::TASTE, "내가 거절한 사진과 비슷")]),
];

fn pct(v: f32) -> Option<i32> {
    v.is_finite().then(|| (v * 100.0).round().clamp(0.0, 100.0) as i32)
}

fn framing_label(f: Framing) -> Option<String> {
    if f.is_identity() {
        return None;
    }
    Some(match (f.angle.abs() >= 0.05, f.w * f.h < 0.999) {
        (true, true) => format!("{:.1}° 돌리고 {:.0}% 크기로 자름", f.angle, f.w * f.h * 100.0),
        (true, false) => format!("{:.1}° 돌림", f.angle),
        _ => format!("{:.0}% 크기로 자름", f.w * f.h * 100.0),
    })
}

fn make_summary(a: &AssetRecord) -> PhotoSummary {
    let swap = (5..=8).contains(&a.orientation);
    let (w, h) = if swap { (a.height, a.width) } else { (a.width, a.height) };
    PhotoSummary {
        id: a.id,
        name: a.name.clone(),
        captured_ms: a.captured_ms,
        rating: a.rating as i32,
        flag: a.flag as i32,
        b_pct: pct(a.b_prob),
        top_pct: pct(a.keep_rank).map(|p| p.max(1)),
        burst_rank: if a.burst_size > 1 { a.burst_rank as i32 + 1 } else { 0 },
        burst_size: if a.burst_size > 1 { a.burst_size as i32 } else { 0 },
        has_framing: a.framing.is_some_and(|f| !f.is_identity()),
        video: a.is_video(),
        raw: a.formats & formats::RAW != 0,
        jpeg: a.formats & formats::JPEG != 0,
        width: w as i32,
        height: h as i32,
    }
}

fn make_overlay(comp: &Composition, off: u32) -> CompositionOverlay {
    CompositionOverlay {
        has_subject: comp.subject != composition::subject::NONE && comp.bw > 0.0 && comp.bh > 0.0,
        subject_x: comp.bx,
        subject_y: comp.by,
        subject_w: comp.bw,
        subject_h: comp.bh,
        key_x: comp.kx,
        key_y: comp.ky,
        head_top: comp.head_top,
        facing: comp.facing,
        pole: comp.pole,
        pole_x: comp.pole_x,
        neck_line: comp.neck_line,
        neck_y: comp.neck_y,
        tilt: comp.tilt,
        tilt_conf: comp.tilt_conf,
        tilt_vertical: comp.tilt_vertical,
        tilt_at: comp.tilt_at,
        show_thirds: off & reason::THIRDS == 0,
        show_headroom: off & reason::HEADROOM == 0,
        show_lead_room: off & reason::LEAD_ROOM == 0,
        show_merger: off & reason::MERGER == 0,
        show_tilt: off & reason::TILT == 0,
    }
}

fn make_detail(index: &LibraryIndex, a: &AssetRecord) -> PhotoDetail {
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
    let strengths =
        if a.b_prob < 0.4 { strength::labels(a.strengths).into_iter().map(String::from).collect() } else { Vec::new() };
    PhotoDetail {
        summary: make_summary(a),
        reasons: reason::labels(a.reasons).into_iter().map(String::from).collect(),
        strengths,
        scene: if a.b_prob.is_finite() { Scene::from_u8(a.scene).label().to_string() } else { String::new() },
        framing: a.framing.and_then(framing_label),
        comp_notes: Vec::new(),
        overlay: None,
        camera: index.cameras.name(a.camera).to_string(),
        lens: index.lenses.name(a.lens).to_string(),
        exposure: exposure.join(" · "),
    }
}

fn compute_counts(index: &LibraryIndex, settings: &Settings) -> Counts {
    let mut c = Counts::default();
    for a in &index.assets {
        c.all += 1;
        if settings.matches(LibraryView::Undecided, a) {
            c.undecided += 1;
        }
        if settings.matches(LibraryView::Candidates, a) {
            c.candidates += 1;
        }
        if settings.matches(LibraryView::Keepers, a) {
            c.keepers += 1;
        }
        if settings.matches(LibraryView::Rejected, a) {
            c.rejected += 1;
        }
        if settings.matches(LibraryView::Picked, a) {
            c.picked += 1;
        }
        if settings.matches(LibraryView::Rated, a) {
            c.rated += 1;
        }
        if a.b_prob.is_finite() {
            c.analysed += 1;
        }
    }
    c
}

fn compute_learn_status(index: &LibraryIndex, report: Option<&Report>) -> LearnStatus {
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

fn apply_scores(index: &mut LibraryIndex, scores: &[Score]) {
    for s in scores {
        if let Some(a) = index.get_mut(s.id) {
            a.b_prob = s.b_prob;
            a.reasons = s.reasons;
            a.strengths = s.strengths;
            a.burst = s.burst;
            a.burst_rank = s.burst_rank;
            a.burst_size = s.burst_size;
            a.scene = s.scene;
        }
    }
    index.rank_keepers();
}

fn dir_size(dir: &Path) -> u64 {
    let Ok(rd) = std::fs::read_dir(dir) else { return 0 };
    let mut total = 0u64;
    for entry in rd.flatten() {
        let p = entry.path();
        if p.is_dir() {
            total += dir_size(&p);
        } else if let Ok(md) = entry.metadata() {
            total += md.len();
        }
    }
    total
}

#[uniffi::export]
impl PhotoLibrary {
    /// Open (or create) a LOUPE library at `root` (app Documents folder or an external SSD folder).
    #[uniffi::constructor]
    pub fn open(root: String) -> Res<Arc<Self>> {
        let lib = Library::open(Path::new(&root))?;
        let catalog = lib.catalog()?;
        let thumbs = lib.thumbs()?;
        let mut index = catalog.load_index()?;
        index.rank_keepers();
        Ok(Arc::new(PhotoLibrary {
            lib,
            index: RwLock::new(IndexState { index, report: None }),
            catalog: Mutex::new(Some(catalog)),
            thumbs: Mutex::new(Some(thumbs)),
            scan: Mutex::new(None),
            settings: RwLock::new(Settings::default()),
            ops: OpBarrier::new(),
            revision: AtomicI64::new(1),
            suspended: AtomicBool::new(false),
            cancel_import: Arc::new(AtomicBool::new(false)),
            cancel_analysis: Arc::new(AtomicBool::new(false)),
            cancel_export: Arc::new(AtomicBool::new(false)),
        }))
    }

    pub fn root(&self) -> String {
        self.lib.root.display().to_string()
    }

    pub fn revision(&self) -> i64 {
        self.revision.load(Ordering::Acquire)
    }

    pub fn is_suspended(&self) -> bool {
        self.suspended.load(Ordering::Acquire)
    }

    /// Gracefully suspend database connections before iOS moves the app to background:
    /// cancels in-flight background work, drains all active operations so no untracked SQLite
    /// connections remain open, truncates WAL files, and drops both `Catalog` and `ThumbStore`
    /// handles so zero file locks remain held (prevents `0xdead10cc`).
    pub fn suspend(&self) -> Res<()> {
        self.suspended.store(true, Ordering::Release);
        self.cancel_import.store(true, Ordering::Relaxed);
        self.cancel_analysis.store(true, Ordering::Relaxed);
        self.cancel_export.store(true, Ordering::Relaxed);
        self.ops.wait_drained(Duration::from_secs(4));
        {
            let mut cat = self.catalog_guard();
            if let Some(c) = cat.as_ref() {
                let _ = c.checkpoint_truncate();
            }
            *cat = None;
        }
        {
            let mut th = self.thumbs_guard();
            if let Some(t) = th.as_ref() {
                let _ = t.checkpoint_truncate();
            }
            *th = None;
        }
        Ok(())
    }

    /// Reopen SQLite connections and refresh the in-memory index after returning to foreground.
    pub fn resume(&self) -> Res<()> {
        let catalog = self.lib.catalog()?;
        let thumbs = self.lib.thumbs()?;
        let mut fresh = catalog.load_index()?;
        fresh.rank_keepers();
        *self.catalog_guard() = Some(catalog);
        *self.thumbs_guard() = Some(thumbs);
        {
            let mut st = self.index_write();
            st.index = fresh;
        }
        self.suspended.store(false, Ordering::Release);
        self.revision.fetch_add(1, Ordering::AcqRel);
        Ok(())
    }

    /// Atomic snapshot of matching photo IDs, sidebar counts, learn status, and monotonic revision.
    pub fn snapshot(&self, view: LibraryView) -> Res<LibrarySnapshot> {
        let settings = *self.settings_read();
        let st = self.index_read();
        let ids = st.index.assets.iter().filter(|a| settings.matches(view, a)).map(|a| a.id).collect();
        let counts = compute_counts(&st.index, &settings);
        let learn = compute_learn_status(&st.index, st.report.as_ref());
        let revision = self.revision.load(Ordering::Acquire);
        Ok(LibrarySnapshot { view, ids, counts, learn, revision })
    }

    /// Fetch compact metadata only for a requested window of photo IDs (keeps FFI O(window)).
    pub fn photos_batch(&self, ids: Vec<i64>) -> Res<Vec<PhotoSummary>> {
        let st = self.index_read();
        Ok(ids.into_iter().filter_map(|id| st.index.get(id).map(make_summary)).collect())
    }

    /// Fetch full photo details, composition evaluation notes, and toggleable overlay geometry.
    pub fn photo_detail(&self, id: i64) -> Res<Option<PhotoDetail>> {
        let _op = self.try_enter_op()?;
        let off = self.settings_read().learn.off;
        let (mut detail, framing) = {
            let st = self.index_read();
            let Some(a) = st.index.get(id) else {
                return Ok(None);
            };
            (make_detail(&st.index, a), a.framing)
        };
        let cat_guard = self.catalog_guard();
        if let Some(cat) = cat_guard.as_ref() {
            if let Some(an) = cat.analysis(id).ok().flatten() {
                if an.version >= 4 {
                    let comp = framing.map(|f| f.composition(&an.comp)).unwrap_or_else(|| an.comp.clone());
                    detail.comp_notes = composition::notes(&comp)
                        .into_iter()
                        .map(|n| CompNote { text: n.text, ok: n.ok, ignored: n.rule != 0 && off & n.rule != 0 })
                        .collect();
                    detail.overlay = Some(make_overlay(&comp, off));
                }
            }
        }
        Ok(Some(detail))
    }

    /// Persistent-connection thumbnail fetch (zero per-call SQLite open). Applies non-destructive
    /// crop/straighten framing only when active.
    pub fn thumbnail(&self, id: i64) -> Res<Option<Vec<u8>>> {
        let _op = self.try_enter_op()?;
        let framing = self.index_read().index.get(id).and_then(|a| a.framing).filter(|f| !f.is_identity());
        let bytes = {
            let th_guard = self.thumbs_guard();
            let Some(th) = th_guard.as_ref() else {
                return Err(PhotoError::LibrarySuspended);
            };
            th.get(id)?
        };
        let Some(bytes) = bytes else {
            return Ok(None);
        };
        if let Some(f) = framing {
            if let Ok(img) = photoorg_core::image::load_from_memory(&bytes) {
                let framed = f.apply(&img.to_rgb8());
                if let Ok(out) = preview::encode_jpeg(&framed, 85) {
                    return Ok(Some(out));
                }
            }
        }
        Ok(Some(bytes))
    }

    /// Return the local path to a video asset so iOS `AVAssetImageGenerator` can extract a
    /// hardware-accelerated video thumbnail.
    pub fn video_path(&self, id: i64) -> Res<Option<String>> {
        let _op = self.try_enter_op()?;
        let cat_guard = self.catalog_guard();
        let Some(cat) = cat_guard.as_ref() else {
            return Err(PhotoError::LibrarySuspended);
        };
        let files = cat.files_for(id)?;
        Ok(files.iter().find(|f| f.kind.is_video()).map(|f| self.lib.abs(&f.rel_path).display().to_string()))
    }

    /// Describe how iOS should load the viewer preview for `id`.
    /// Returns [`PreviewDescriptor::EmbeddedSlice`] (zero software decode/re-encode, zero FFI byte
    /// copy) whenever possible so Apple ImageIO decodes directly via mmap.
    pub fn preview_descriptor(&self, id: i64, max_edge: i32) -> Res<PreviewDescriptor> {
        let _op = self.try_enter_op()?;
        let edge = max_edge.clamp(64, 8192) as u32;
        let framing = self.index_read().index.get(id).and_then(|a| a.framing).filter(|f| !f.is_identity());
        let (files, orientation) = {
            let cat_guard = self.catalog_guard();
            let Some(cat) = cat_guard.as_ref() else {
                return Err(PhotoError::LibrarySuspended);
            };
            (cat.files_for(id)?, cat.metadata(id).map(|m| m.orientation).unwrap_or(1))
        };
        let Some(f) = files.iter().filter(|f| !f.kind.is_video()).min_by_key(|f| f.kind.preview_priority()) else {
            return Err(PhotoError::NotFound {
                detail: "미리보기 가능한 이미지 파일이 없습니다".into()
            });
        };
        let abs = self.lib.abs(&f.rel_path);
        if framing.is_none() {
            if let Ok(emb) = preview::find_embedded_jpeg(&abs, f.kind, edge, None) {
                return Ok(PreviewDescriptor::EmbeddedSlice {
                    path: abs.display().to_string(),
                    offset: emb.offset,
                    length: emb.len,
                    orientation: orientation as i32,
                });
            }
        }
        let img = preview::load_image(&abs, f.kind, orientation, edge, None)?;
        let img = match framing {
            Some(fr) => fr.apply(&img),
            None => img,
        };
        Ok(PreviewDescriptor::RenderedJpeg { bytes: preview::encode_jpeg(&img, 88)? })
    }

    /// Set star rating (0 clears). Writes catalog + XMP sidecar and returns the new revision.
    pub fn set_rating(&self, ids: Vec<i64>, rating: i32) -> Res<i64> {
        let _op = self.try_enter_op()?;
        let rating = rating.clamp(0, 5) as u8;
        {
            let mut cat_guard = self.catalog_guard();
            let Some(cat) = cat_guard.as_mut() else {
                return Err(PhotoError::LibrarySuspended);
            };
            cat.set_rating(&ids, rating)?;
            self.write_sidecars(cat, &ids);
        }
        {
            let mut st = self.index_write();
            for id in &ids {
                if let Some(a) = st.index.get_mut(*id) {
                    a.rating = rating;
                }
            }
        }
        Ok(self.revision.fetch_add(1, Ordering::AcqRel) + 1)
    }

    /// Set flag: 1 = pick (A컷), -1 = reject (B컷), 0 = undecided. Returns the new revision.
    pub fn set_flag(&self, ids: Vec<i64>, flag: i32) -> Res<i64> {
        let _op = self.try_enter_op()?;
        let flag = flag.signum() as i8;
        {
            let mut cat_guard = self.catalog_guard();
            let Some(cat) = cat_guard.as_mut() else {
                return Err(PhotoError::LibrarySuspended);
            };
            cat.set_flag(&ids, flag)?;
            self.write_sidecars(cat, &ids);
        }
        {
            let mut st = self.index_write();
            for id in &ids {
                if let Some(a) = st.index.get_mut(*id) {
                    a.flag = flag;
                }
            }
        }
        Ok(self.revision.fetch_add(1, Ordering::AcqRel) + 1)
    }

    /// Atomically restore prior rating and flag values for a batch of photos (Undo support).
    pub fn restore_decisions(&self, items: Vec<DecisionRestore>) -> Res<i64> {
        let _op = self.try_enter_op()?;
        if items.is_empty() {
            return Ok(self.revision());
        }
        let ids: Vec<i64> = items.iter().map(|i| i.id).collect();
        {
            let mut cat_guard = self.catalog_guard();
            let Some(cat) = cat_guard.as_mut() else {
                return Err(PhotoError::LibrarySuspended);
            };
            for item in &items {
                cat.set_rating(&[item.id], item.rating.clamp(0, 5) as u8)?;
                cat.set_flag(&[item.id], item.flag.signum() as i8)?;
            }
            self.write_sidecars(cat, &ids);
        }
        {
            let mut st = self.index_write();
            for item in &items {
                if let Some(a) = st.index.get_mut(item.id) {
                    a.rating = item.rating.clamp(0, 5) as u8;
                    a.flag = item.flag.signum() as i8;
                }
            }
        }
        Ok(self.revision.fetch_add(1, Ordering::AcqRel) + 1)
    }

    pub fn set_threshold(&self, percent: i32) {
        self.settings_write().threshold = percent.clamp(30, 90) as u8;
        self.revision.fetch_add(1, Ordering::AcqRel);
    }

    pub fn threshold(&self) -> i32 {
        self.settings_read().threshold as i32
    }

    pub fn set_keep_pct(&self, percent: i32) {
        self.settings_write().keep_pct = percent.clamp(5, 50) as u8;
        self.revision.fetch_add(1, Ordering::AcqRel);
    }

    pub fn keep_pct(&self) -> i32 {
        self.settings_read().keep_pct as i32
    }

    pub fn disabled_rules(&self) -> i64 {
        self.settings_read().learn.off as i64
    }

    pub fn set_disabled_rules(&self, off: i64) {
        self.settings_write().learn.off = off as u32;
        self.revision.fetch_add(1, Ordering::AcqRel);
    }

    pub fn rules(&self) -> Vec<RuleItem> {
        let off = self.settings_read().learn.off;
        let mut out = Vec::new();
        for (group, items) in RULE_GROUPS {
            for (bit, label) in *items {
                out.push(RuleItem {
                    bit: *bit as i64,
                    group: (*group).to_string(),
                    label: (*label).to_string(),
                    enabled: off & bit == 0,
                });
            }
        }
        out
    }

    /// Non-destructive auto straighten (and optional gentle composition crop).
    pub fn auto_frame(&self, id: i64, compose: bool) -> Res<String> {
        let _op = self.try_enter_op()?;
        let off = self.settings_read().learn.off;
        let a = self
            .index_read()
            .index
            .get(id)
            .cloned()
            .ok_or_else(|| PhotoError::NotFound { detail: format!("사진 #{id}") })?;
        if a.is_video() {
            return Err(PhotoError::UnsupportedForVideo);
        }
        let an = {
            let cat_guard = self.catalog_guard();
            let Some(cat) = cat_guard.as_ref() else {
                return Err(PhotoError::LibrarySuspended);
            };
            cat.analysis(id)?.ok_or(PhotoError::NotAnalyzed)?
        };
        let swap = (5..=8).contains(&a.orientation);
        let (w, h) = if swap { (a.height, a.width) } else { (a.width, a.height) };
        let aspect = if w > 0 && h > 0 {
            w as f32 / h as f32
        } else if an.comp.aspect > 0.0 {
            an.comp.aspect
        } else {
            1.5
        };
        let mut c = an.comp;
        c.aspect = aspect;
        let f = framing::suggest(&c, Some(aspect), compose, off);
        let level = c.tilt_conf >= 0.3 && c.tilt.abs() >= 0.4 && c.tilt.abs() < 9.5;
        let level_only = Framing::straightened(f.angle, aspect, Some(aspect));
        let composed = compose && f.w * f.h < level_only.w * level_only.h - 1e-3;
        let stored = (!f.is_identity()).then_some(f);
        {
            let cat_guard = self.catalog_guard();
            let Some(cat) = cat_guard.as_ref() else {
                return Err(PhotoError::LibrarySuspended);
            };
            cat.set_framing(id, stored)?;
        }
        if let Some(rec) = self.index_write().index.get_mut(id) {
            rec.framing = stored;
        }
        self.revision.fetch_add(1, Ordering::AcqRel);
        let _ = self.learn_locked();
        Ok(match (level, composed) {
            (true, true) => format!("{:.1}° 돌려 수평을 맞추고 구도에 맞춰 잘랐습니다", f.angle.abs()),
            (true, false) => format!("{:.1}° 돌려 수평을 맞췄습니다", f.angle.abs()),
            (false, true) => "구도에 맞춰 잘랐습니다".to_string(),
            (false, false) if compose => "기울기 · 구도 모두 괜찮아 그대로 둡니다".to_string(),
            (false, false) => "뚜렷한 기울기가 없어 그대로 둡니다".to_string(),
        })
    }

    pub fn reset_framing(&self, id: i64) -> Res<()> {
        let _op = self.try_enter_op()?;
        {
            let cat_guard = self.catalog_guard();
            let Some(cat) = cat_guard.as_ref() else {
                return Err(PhotoError::LibrarySuspended);
            };
            cat.set_framing(id, None)?;
        }
        if let Some(rec) = self.index_write().index.get_mut(id) {
            rec.framing = None;
        }
        self.revision.fetch_add(1, Ordering::AcqRel);
        let _ = self.learn_locked();
        Ok(())
    }

    /// Scan an SD card / USB-C external drive / local folder chosen via iOS `UIDocumentPicker`.
    /// Caller must hold `startAccessingSecurityScopedResource()` on `path` until import finishes.
    pub fn scan_directory(&self, path: String, label: String, include_videos: bool) -> Res<ScanInfo> {
        let _op = self.try_enter_op()?;
        let root = PathBuf::from(&path);
        if !root.exists() {
            return Err(PhotoError::NotFound { detail: path });
        }
        if let Err(e) = std::fs::read_dir(&root) {
            return Err(if e.kind() == ErrorKind::PermissionDenied {
                PhotoError::PermissionDenied { detail: format!("{path}: {e}") }
            } else {
                PhotoError::Io { detail: format!("{path}: {e}") }
            });
        }
        self.cancel_import.store(false, Ordering::Relaxed);
        let fs = Source::local();
        let mut scan = import::scan_in(&fs, &root, include_videos, &self.cancel_import, &|_, _| {})?;
        if self.cancel_import.load(Ordering::Relaxed) {
            return Err(PhotoError::Cancelled);
        }
        scan.label = if label.trim().is_empty() {
            root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "카드".into())
        } else {
            label
        };
        {
            let cat_guard = self.catalog_guard();
            let Some(cat) = cat_guard.as_ref() else {
                return Err(PhotoError::LibrarySuspended);
            };
            import::mark_existing(&mut scan, cat)?;
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
        *self.scan_guard() = Some(scan);
        Ok(info)
    }

    /// Import the previously scanned card/folder into the library with preflight storage check.
    /// Pass `available_bytes` from `URLResourceKey.volumeAvailableCapacityForImportantUsageKey`
    /// (pass `-1` if unknown to skip the preflight check).
    pub fn import_scanned(&self, available_bytes: i64, listener: Box<dyn ImportListener>) -> Res<ImportResult> {
        let _op = self.try_enter_op()?;
        let scan = self
            .scan_guard()
            .take()
            .ok_or_else(|| PhotoError::NotFound { detail: "먼저 카드나 폴더를 선택해 주세요".into() })?;
        let summary_before = scan.summary(true);
        let needed = summary_before.new_bytes as i64;
        // 5% headroom capped at 256 MB for catalog + thumbnails + atomic write buffers.
        let margin = (needed / 20).min(256 * 1024 * 1024);
        let required = needed.saturating_add(margin);
        if available_bytes >= 0 && required > available_bytes {
            // Put the scan back so the user can retry after freeing space.
            *self.scan_guard() = Some(scan);
            return Err(PhotoError::InsufficientStorage { required_bytes: required, available_bytes });
        }
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
        if !self.is_suspended() {
            self.reload_locked()?;
        }
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

    pub fn pending_analysis(&self) -> Res<i32> {
        let _op = self.try_enter_op()?;
        Ok(self.pending()?.len() as i32)
    }

    /// Analyze unanalyzed photos in incremental 64-photo chunks with a caller-supplied thread
    /// budget (`1` under thermal/memory pressure, `2..=3` normal). Each completed chunk reloads
    /// and rescores so B-cut/A-cut candidates appear live while a large card finishes analyzing.
    pub fn analyze(&self, max_threads: i32, listener: Box<dyn ProgressListener>) -> Res<i32> {
        let _op = self.try_enter_op()?;
        self.cancel_analysis.store(false, Ordering::Relaxed);
        let ids = self.pending()?;
        let total = ids.len() as i32;
        listener.progress(0, total);
        if ids.is_empty() {
            return Ok(0);
        }
        let budget = max_threads.clamp(1, 4) as usize;
        const CHUNK: usize = 64;
        let mut completed: usize = 0;

        for chunk in ids.chunks(CHUNK) {
            if self.cancel_analysis.load(Ordering::Relaxed) || self.is_suspended() {
                break;
            }
            let base = completed;
            let done_chunk =
                cull::analyze_assets_with_threads(&self.lib, chunk, &self.cancel_analysis, budget, &|_, n| {
                    let overall = (base + n) as i32;
                    if overall % 4 == 0 || overall >= total {
                        listener.progress(overall.min(total), total);
                    }
                })?;
            completed += done_chunk;
            if !self.is_suspended() && done_chunk > 0 {
                let _ = self.reload_locked();
                let _ = self.learn_locked();
            }
        }
        listener.progress((completed as i32).min(total), total);
        Ok(completed as i32)
    }

    pub fn cancel_analysis(&self) {
        self.cancel_analysis.store(true, Ordering::Relaxed);
    }

    /// Retrain and rescore every photo from current decisions using the persistent catalog
    /// connection (no untracked SQLite connections left open across background suspension).
    pub fn learn(&self) -> Res<LearnStatus> {
        let _op = self.try_enter_op()?;
        self.learn_locked()
    }

    pub fn learn_status(&self) -> LearnStatus {
        let st = self.index_read();
        compute_learn_status(&st.index, st.report.as_ref())
    }

    /// Flush WAL files and run SQLite `PRAGMA optimize` on catalog & thumbnail databases.
    pub fn optimize_storage(&self) -> Res<()> {
        let _op = self.try_enter_op()?;
        {
            let cat_guard = self.catalog_guard();
            let Some(cat) = cat_guard.as_ref() else {
                return Err(PhotoError::LibrarySuspended);
            };
            let _ = cat.conn().execute_batch("PRAGMA optimize;");
            cat.checkpoint_truncate()?;
        }
        {
            let th_guard = self.thumbs_guard();
            let Some(th) = th_guard.as_ref() else {
                return Err(PhotoError::LibrarySuspended);
            };
            th.checkpoint_truncate()?;
        }
        Ok(())
    }

    /// Estimate bytes and counts before exporting selected photos (for AirDrop / Files / Lightroom).
    pub fn export_plan(&self, ids: Vec<i64>) -> Res<ExportPlan> {
        let _op = self.try_enter_op()?;
        let framed_count = {
            let st = self.index_read();
            ids.iter().filter(|id| st.index.get(**id).is_some_and(|a| a.framing.is_some())).count() as i32
        };
        let cat_guard = self.catalog_guard();
        let Some(cat) = cat_guard.as_ref() else {
            return Err(PhotoError::LibrarySuspended);
        };
        let p = export::plan_info(cat, &ids)?;
        Ok(ExportPlan {
            photos: p.photos as i32,
            originals_bytes: p.originals_bytes as i64,
            jpeg_bytes: p.jpeg_bytes as i64,
            raw_only: p.raw_only as i32,
            videos: p.videos as i32,
            framed_count,
        })
    }

    /// Export `ids` to `dest_dir` (e.g., temporary share staging dir or a security-scoped folder).
    #[allow(clippy::too_many_arguments)]
    pub fn export_photos(
        &self,
        ids: Vec<i64>,
        dest_dir: String,
        mode: ExportMode,
        long_edge: i32,
        quality: i32,
        date_names: bool,
        listener: Box<dyn ProgressListener>,
    ) -> Res<ExportResult> {
        let _op = self.try_enter_op()?;
        self.cancel_export.store(false, Ordering::Relaxed);
        let kind = match mode {
            ExportMode::Originals => ExportKind::Originals,
            ExportMode::JpegOnly => ExportKind::JpegOnly,
            ExportMode::Resized => {
                ExportKind::Resized { long_edge: long_edge.max(0) as u32, quality: quality.clamp(30, 100) as u8 }
            }
        };
        let opts = ExportOptions { kind, date_names, zip: None, groups: Vec::new() };
        let cat = self.lib.catalog()?;
        let summary =
            export::export(&self.lib, &cat, &ids, Path::new(&dest_dir), &opts, &self.cancel_export, &|d, t| {
                listener.progress(d as i32, t as i32);
            })?;
        drop(cat);
        Ok(ExportResult {
            files: summary.files as i32,
            from_raw: summary.from_raw as i32,
            bytes: summary.bytes as i64,
            skipped: summary.skipped.into_iter().map(|(name, why)| format!("{name}: {why}")).collect(),
            cancelled: summary.cancelled,
        })
    }

    pub fn cancel_export(&self) {
        self.cancel_export.store(true, Ordering::Relaxed);
    }

    pub fn delete_plan(&self, ids: Vec<i64>) -> Res<DeletePlan> {
        let _op = self.try_enter_op()?;
        let cat_guard = self.catalog_guard();
        let Some(cat) = cat_guard.as_ref() else {
            return Err(PhotoError::LibrarySuspended);
        };
        let p = trash::plan(&self.lib, cat, &ids)?;
        Ok(DeletePlan { photos: p.photos as i32, files: p.files as i32, bytes: p.bytes as i64 })
    }

    /// Move photos into the library's own trash folder while preserving culling decisions for learning.
    pub fn delete_photos(&self, ids: Vec<i64>) -> Res<DeleteResult> {
        let _op = self.try_enter_op()?;
        let report = {
            let mut cat_guard = self.catalog_guard();
            let Some(cat) = cat_guard.as_mut() else {
                return Err(PhotoError::LibrarySuspended);
            };
            trash::trash_assets_in(&self.lib, cat, &ids, trash::Bin::Library)?
        };
        if !report.trashed.is_empty() {
            self.index_write().index.remove(&report.trashed);
            self.revision.fetch_add(1, Ordering::AcqRel);
        }
        Ok(DeleteResult {
            deleted: report.trashed.len() as i32,
            failed: report.failed.len() as i32,
            bytes: report.bytes as i64,
            first_error: report.failed.first().map(|(_, e)| e.clone()),
        })
    }

    pub fn trash_bytes(&self) -> i64 {
        dir_size(&self.lib.trash_dir()) as i64
    }

    pub fn empty_trash(&self) -> Res<()> {
        let dir = self.lib.trash_dir();
        if dir.exists() {
            std::fs::remove_dir_all(&dir)
                .map_err(|e| PhotoError::Io { detail: format!("휴지통을 비우지 못했습니다: {e}") })?;
        }
        Ok(())
    }
}

impl PhotoLibrary {
    fn try_enter_op(&self) -> Res<OpGuard<'_>> {
        self.ops.try_enter(&self.suspended)
    }

    fn index_read(&self) -> RwLockReadGuard<'_, IndexState> {
        self.index.read().unwrap_or_else(|e| e.into_inner())
    }

    fn index_write(&self) -> RwLockWriteGuard<'_, IndexState> {
        self.index.write().unwrap_or_else(|e| e.into_inner())
    }

    fn catalog_guard(&self) -> MutexGuard<'_, Option<Catalog>> {
        self.catalog.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn thumbs_guard(&self) -> MutexGuard<'_, Option<ThumbStore>> {
        self.thumbs.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn scan_guard(&self) -> MutexGuard<'_, Option<ScanResult>> {
        self.scan.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn settings_read(&self) -> RwLockReadGuard<'_, Settings> {
        self.settings.read().unwrap_or_else(|e| e.into_inner())
    }

    fn settings_write(&self) -> RwLockWriteGuard<'_, Settings> {
        self.settings.write().unwrap_or_else(|e| e.into_inner())
    }

    fn pending(&self) -> Res<Vec<i64>> {
        let done = {
            let cat_guard = self.catalog_guard();
            let Some(cat) = cat_guard.as_ref() else {
                return Err(PhotoError::LibrarySuspended);
            };
            cat.analyzed_ids(cull::ANALYSIS_VERSION)?
        };
        let st = self.index_read();
        Ok(st.index.assets.iter().rev().filter(|a| !a.is_video() && !done.contains(&a.id)).map(|a| a.id).collect())
    }

    fn learn_locked(&self) -> Res<LearnStatus> {
        let settings = self.settings_read().learn;
        let (scores, report) = {
            let mut cat_guard = self.catalog_guard();
            let Some(cat) = cat_guard.as_mut() else {
                return Err(PhotoError::LibrarySuspended);
            };
            cull::rescore(cat, &settings)?
        };
        let mut st = self.index_write();
        apply_scores(&mut st.index, &scores);
        let status = compute_learn_status(&st.index, Some(&report));
        st.report = Some(report);
        self.revision.fetch_add(1, Ordering::AcqRel);
        Ok(status)
    }

    fn reload_locked(&self) -> Res<()> {
        let mut fresh = {
            let cat_guard = self.catalog_guard();
            let Some(cat) = cat_guard.as_ref() else {
                return Err(PhotoError::LibrarySuspended);
            };
            cat.load_index()?
        };
        fresh.rank_keepers();
        self.index_write().index = fresh;
        self.revision.fetch_add(1, Ordering::AcqRel);
        Ok(())
    }

    fn write_sidecars(&self, catalog: &Catalog, ids: &[i64]) {
        for &id in ids {
            if let Err(e) = self.lib.write_xmp(catalog, id) {
                log::debug!("xmp {id}: {e:#}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use photoorg_core::image::{Rgb, RgbImage};
    use std::sync::atomic::AtomicUsize;

    struct CountImport(Arc<AtomicUsize>);
    impl ImportListener for CountImport {
        fn progress(&self, _status: ImportStatus) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }

    struct CountProgress(Arc<AtomicUsize>);
    impl ProgressListener for CountProgress {
        fn progress(&self, _done: i32, _total: i32) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn write_sample_jpeg(path: &Path, seed: u8) {
        let img = RgbImage::from_fn(480, 320, |x, y| {
            let v = (((x * 7 + y * 13) ^ (seed as u32 * 31)) & 0xFF) as u8;
            Rgb([v, v.wrapping_add(40), v.wrapping_add(90)])
        });
        let bytes = preview::encode_jpeg(&img, 90).unwrap();
        std::fs::write(path, bytes).unwrap();
    }

    #[test]
    fn lifecycle_suspend_resume_and_snapshot_consistency() {
        let tmp = tempfile::tempdir().unwrap();
        let card = tmp.path().join("card");
        let lib_dir = tmp.path().join("lib");
        std::fs::create_dir_all(&card).unwrap();
        write_sample_jpeg(&card.join("IMG_0001.JPG"), 1);
        write_sample_jpeg(&card.join("IMG_0002.JPG"), 2);

        let lib = PhotoLibrary::open(lib_dir.display().to_string()).unwrap();
        let scan = lib.scan_directory(card.display().to_string(), "TEST_SD".into(), true).unwrap();
        assert_eq!(scan.new_photos, 2);

        // Preflight storage check rejects insufficient space cleanly with structured error.
        let err = lib.import_scanned(10, Box::new(CountImport(Arc::new(AtomicUsize::new(0))))).unwrap_err();
        assert!(matches!(err, PhotoError::InsufficientStorage { .. }), "got {err:?}");

        // Retrying after storage check succeeds because scan was preserved.
        let ticks = Arc::new(AtomicUsize::new(0));
        let res = lib.import_scanned(100 * 1024 * 1024, Box::new(CountImport(ticks.clone()))).unwrap();
        assert_eq!(res.added, 2);
        assert!(ticks.load(Ordering::Relaxed) >= 1);

        // Atomic snapshot + windowed batch match identically.
        let snap = lib.snapshot(LibraryView::All).unwrap();
        assert_eq!(snap.ids.len(), 2);
        assert_eq!(snap.counts.all, 2);
        assert_eq!(snap.counts.undecided, 2);

        let batch = lib.photos_batch(snap.ids.clone()).unwrap();
        assert_eq!(batch.len(), 2);
        assert!(batch.iter().all(|p| p.jpeg && !p.raw));

        // Zero-reencode preview descriptor returns an EmbeddedSlice for unframed JPEG.
        let desc = lib.preview_descriptor(snap.ids[0], 1600).unwrap();
        assert!(
            matches!(desc, PreviewDescriptor::EmbeddedSlice { length, .. } if length > 0),
            "expected zero-reencode EmbeddedSlice, got {desc:?}"
        );

        // Thumbnail fetch works via persistent ThumbStore connection.
        let thumb = lib.thumbnail(snap.ids[0]).unwrap();
        assert!(thumb.is_some_and(|b| b.len() > 100));

        // Suspend cleanly drops DB connections and rejects mutations without panicking.
        lib.suspend().unwrap();
        assert!(lib.is_suspended());
        assert!(matches!(lib.set_rating(vec![snap.ids[0]], 5), Err(PhotoError::LibrarySuspended)));
        assert!(matches!(lib.thumbnail(snap.ids[0]), Err(PhotoError::LibrarySuspended)));

        // Resume restores full operation.
        lib.resume().unwrap();
        assert!(!lib.is_suspended());
        let rev = lib.set_rating(vec![snap.ids[0]], 5).unwrap();
        assert!(rev > snap.revision);
        let snap2 = lib.snapshot(LibraryView::Rated).unwrap();
        assert_eq!(snap2.ids, vec![snap.ids[0]]);
        assert_eq!(snap2.counts.rated, 1);
        assert_eq!(snap2.counts.undecided, 1);
    }

    #[test]
    fn bounded_analysis_model_release_auto_frame_and_export() {
        let tmp = tempfile::tempdir().unwrap();
        let card = tmp.path().join("card");
        let lib_dir = tmp.path().join("lib");
        let export_dir = tmp.path().join("export");
        std::fs::create_dir_all(&card).unwrap();
        write_sample_jpeg(&card.join("IMG_0100.JPG"), 10);

        let lib = PhotoLibrary::open(lib_dir.display().to_string()).unwrap();
        lib.scan_directory(card.display().to_string(), "SD".into(), true).unwrap();
        lib.import_scanned(-1, Box::new(CountImport(Arc::new(AtomicUsize::new(0))))).unwrap();

        let snap = lib.snapshot(LibraryView::All).unwrap();
        let id = snap.ids[0];

        // Calling auto_frame before analysis returns structured NotAnalyzed error.
        assert!(matches!(lib.auto_frame(id, false), Err(PhotoError::NotAnalyzed)));

        // Single-thread thermal-budget analysis + model memory release.
        let done = lib.analyze(1, Box::new(CountProgress(Arc::new(AtomicUsize::new(0))))).unwrap();
        assert_eq!(done, 1);
        release_ml_models();

        // Detail includes composition notes and overlay geometry.
        let detail = lib.photo_detail(id).unwrap().unwrap();
        assert!(detail.summary.b_pct.is_some());
        assert!(detail.overlay.is_some());

        // Auto-frame succeeds after analysis.
        let msg = lib.auto_frame(id, true).unwrap();
        assert!(!msg.is_empty());

        // Export plan & resized export produce JPEG in destination folder.
        let plan = lib.export_plan(vec![id]).unwrap();
        assert_eq!(plan.photos, 1);
        let exp = lib
            .export_photos(
                vec![id],
                export_dir.display().to_string(),
                ExportMode::Resized,
                1080,
                85,
                true,
                Box::new(CountProgress(Arc::new(AtomicUsize::new(0)))),
            )
            .unwrap();
        assert_eq!(exp.files, 1);
        assert!(!exp.cancelled);

        // Trash preserves learning memory and empties cleanly.
        let del = lib.delete_photos(vec![id]).unwrap();
        assert_eq!(del.deleted, 1);
        assert!(lib.trash_bytes() > 0);
        lib.empty_trash().unwrap();
        assert_eq!(lib.trash_bytes(), 0);
    }

    #[test]
    fn restore_decisions_supports_batch_undo() {
        let tmp = tempfile::tempdir().unwrap();
        let card = tmp.path().join("card");
        let lib_dir = tmp.path().join("lib");
        std::fs::create_dir_all(&card).unwrap();
        write_sample_jpeg(&card.join("IMG_0001.JPG"), 1);
        write_sample_jpeg(&card.join("IMG_0002.JPG"), 2);
        write_sample_jpeg(&card.join("IMG_0003.JPG"), 3);

        let lib = PhotoLibrary::open(lib_dir.display().to_string()).unwrap();
        lib.scan_directory(card.display().to_string(), "SD".into(), true).unwrap();
        lib.import_scanned(-1, Box::new(CountImport(Arc::new(AtomicUsize::new(0))))).unwrap();

        let ids = lib.snapshot(LibraryView::All).unwrap().ids;
        lib.set_rating(vec![ids[0]], 4).unwrap();
        lib.set_flag(vec![ids[1]], 1).unwrap();

        // Capture prior state, perform a bulk reject, then undo atomically.
        let prior = lib
            .photos_batch(ids.clone())
            .unwrap()
            .into_iter()
            .map(|p| DecisionRestore { id: p.id, rating: p.rating, flag: p.flag })
            .collect::<Vec<_>>();

        lib.set_flag(ids.clone(), -1).unwrap();
        assert_eq!(lib.snapshot(LibraryView::Rejected).unwrap().ids.len(), 3);

        lib.restore_decisions(prior).unwrap();
        let after = lib.photos_batch(ids.clone()).unwrap();
        assert_eq!(after[0].rating, 4);
        assert_eq!(after[0].flag, 0);
        assert_eq!(after[1].flag, 1);
        assert_eq!(after[2].flag, 0);
        assert_eq!(lib.snapshot(LibraryView::Rejected).unwrap().ids.len(), 0);
    }

    #[test]
    fn suspend_drains_inflight_ops_and_truncates_wal() {
        let tmp = tempfile::tempdir().unwrap();
        let card = tmp.path().join("card");
        let lib_dir = tmp.path().join("lib");
        std::fs::create_dir_all(&card).unwrap();
        for i in 0..4u8 {
            write_sample_jpeg(&card.join(format!("IMG_00{i}.JPG")), i + 10);
        }

        let lib = PhotoLibrary::open(lib_dir.display().to_string()).unwrap();
        lib.scan_directory(card.display().to_string(), "SD".into(), true).unwrap();
        lib.import_scanned(-1, Box::new(CountImport(Arc::new(AtomicUsize::new(0))))).unwrap();

        // Start background analysis and call suspend concurrently; suspend must drain cleanly.
        let lib_bg = Arc::clone(&lib);
        let worker = std::thread::spawn(move || {
            let _ = lib_bg.analyze(1, Box::new(CountProgress(Arc::new(AtomicUsize::new(0)))));
        });
        std::thread::sleep(Duration::from_millis(50));
        lib.suspend().unwrap();
        worker.join().unwrap();

        assert!(lib.is_suspended());
        assert_eq!(lib.ops.active_count(), 0);
        // WAL file is either absent or truncated to 0 bytes after suspend().
        let wal = lib_dir.join(".photo-organize").join("catalog.db-wal");
        if wal.exists() {
            assert_eq!(std::fs::metadata(&wal).unwrap().len(), 0);
        }

        lib.resume().unwrap();
        lib.optimize_storage().unwrap();
        assert_eq!(lib.snapshot(LibraryView::All).unwrap().ids.len(), 4);
    }

    #[test]
    fn concurrent_readers_and_mutations_preserve_snapshot_invariants() {
        let tmp = tempfile::tempdir().unwrap();
        let card = tmp.path().join("card");
        let lib_dir = tmp.path().join("lib");
        std::fs::create_dir_all(&card).unwrap();
        for i in 0..6u8 {
            write_sample_jpeg(&card.join(format!("IMG_10{i}.JPG")), i + 20);
        }

        let lib = PhotoLibrary::open(lib_dir.display().to_string()).unwrap();
        lib.scan_directory(card.display().to_string(), "SD".into(), true).unwrap();
        lib.import_scanned(-1, Box::new(CountImport(Arc::new(AtomicUsize::new(0))))).unwrap();
        let ids = lib.snapshot(LibraryView::All).unwrap().ids;

        std::thread::scope(|s| {
            let reader_lib = Arc::clone(&lib);
            let reader_ids = ids.clone();
            s.spawn(move || {
                for _ in 0..25 {
                    let snap = reader_lib.snapshot(LibraryView::Picked).unwrap();
                    // Atomic snapshot invariant: ids.len() must always equal counts.picked.
                    assert_eq!(snap.ids.len() as i32, snap.counts.picked);
                    let _ = reader_lib.thumbnail(reader_ids[0]).unwrap();
                }
            });

            let writer_lib = Arc::clone(&lib);
            s.spawn(move || {
                for (k, &id) in ids.iter().enumerate() {
                    writer_lib.set_flag(vec![id], if k % 2 == 0 { 1 } else { -1 }).unwrap();
                    writer_lib.set_rating(vec![id], ((k % 5) + 1) as i32).unwrap();
                }
            });
        });

        let final_snap = lib.snapshot(LibraryView::All).unwrap();
        assert_eq!(final_snap.counts.picked, 3);
        assert_eq!(final_snap.counts.rejected, 3);
        assert_eq!(final_snap.counts.rated, 6);
    }
}
