//! Application state, background work plumbing and the frame loop.

use crate::config::{Config, ThemePref};
use crate::layout::{CardLayout, GridLayout, GridMode};
use crate::theme::{self, Palette};
use crate::thumbs::{color_image, ImageCache, Loaded, LoaderFactory};
use crossbeam_channel::{unbounded, Receiver, Sender};
use eframe::egui::{self, Key};
use photoorg_core::catalog::{FileRecord, TagKind};
use photoorg_core::filter::{self, Facets, Filter, FormatFilter, ProjectFilter, RatingFilter};
use photoorg_core::import::{self, ImportEvent, ImportProgress, ImportSummary, ScanResult, ScanSummary};
use photoorg_core::metadata::JpegRef;
use photoorg_core::volumes::{self, Volume};
use photoorg_core::{preview, Catalog, FileKind, Library, LibraryIndex, Metadata};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

pub struct LibState {
    pub lib: Library,
    pub catalog: Catalog,
    pub index: LibraryIndex,
    /// Facet counts over the whole library (sidebar).
    pub totals: Facets,
    /// [B-cut candidates, rejected, picked, undecided, burst best] over the whole library.
    /// B-cut candidates, rejected, picked, undecided, burst best, A-cut candidates.
    pub cull_counts: [usize; 6],
    /// Photos not judged yet: no stars, not picked or rejected.
    pub open_count: usize,
    /// A-cut candidates: the best this many percent (from the settings).
    pub keep_pct: u8,
    /// Photos with a B-cut score.
    pub analysed: usize,
    /// Photos in each project.
    pub project_counts: std::collections::HashMap<u32, usize>,
}

/// Short confirmation shown in the middle of the screen.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Hud {
    Rating(u8),
    Flag(i8),
    /// A theme was switched on / off.
    /// Theme `tag` put on (true) or taken off `n` photos.
    Theme(u32, bool, usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MainView {
    Welcome,
    Library,
    Import,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SideSel {
    All,
    Rated,
    Recent,
    Videos,
    Pairs,
    Unrated,
    Camera(u32),
    Lens(u32),
    Import(i64),
    Device(PathBuf),
    Candidates,
    Rejected,
    Picked,
    /// A-cut candidates.
    Keepers,
    BurstBest,
    Scene(u8),
    /// Photos of a project (작품).
    Project(u32),
    /// Photos of one theme.
    Theme(u32),
    None,
}

impl SideSel {
    /// Views of chosen photos (rated, picked) from which photos are sent to a project's themes.
    pub fn sends_to_project(&self) -> bool {
        matches!(self, SideSel::Rated | SideSel::Picked)
    }
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub struct PreviewKey {
    pub id: i64,
    pub edge: u32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ScrollReq {
    Top,
    Bottom,
    Item(usize),
    Date(chrono::NaiveDate),
    /// Put this asset's row at the top, `f32` points further down (returning to the library).
    Anchor(i64, f32),
    /// A plain offset (year / month cards).
    Offset(f32),
}

/// Where the library ("라이브러리") was last scrolled to, so returning to it shows the same place.
#[derive(Clone, Copy, Debug)]
pub struct LibraryPlace {
    pub mode: GridMode,
    /// Asset at the top of the grid (None for year / month cards).
    pub anchor: Option<i64>,
    /// Offset below that asset's row (or the plain offset for cards).
    pub offset: f32,
}

#[derive(Default)]
pub struct Selection {
    pub ids: BTreeSet<i64>,
    /// Asset id with keyboard focus.
    pub focus: Option<i64>,
    /// Anchor (filtered index) for shift-selection.
    pub anchor: Option<usize>,
}

pub struct Viewer {
    pub id: i64,
    /// `None` = fit to window; `Some(center)` = 100% zoom centred at the normalised point.
    /// Kept when stepping to the next photo (comparing focus across a burst), reset when the
    /// viewer is opened from the grid.
    pub zoom: Option<egui::Vec2>,
}

/// 100% zoom on the middle of the photo (Z, Space twice).
pub const ZOOM_CENTER: egui::Vec2 = egui::vec2(0.5, 0.5);

/// Two Space presses within this many seconds zoom in instead of going back to the grid.
const DOUBLE_TAP: f64 = 0.3;

pub struct InfoCache {
    pub id: i64,
    pub meta: Metadata,
    pub files: Vec<FileRecord>,
    pub tags: Vec<(u32, String, TagKind)>,
    pub analysis: Option<photoorg_core::cull::Analysis>,
    pub scene_user: Option<u8>,
}

pub enum ImportPhase {
    Scanning { done: usize, total: usize },
    Ready,
    Running,
    Done,
    Failed(String),
}

pub enum ScanMsg {
    Progress(usize, usize),
    Done(Result<ScanResult, String>),
}

#[derive(Clone)]
pub struct SourceItem {
    pub path: PathBuf,
    pub kind: FileKind,
    pub orientation: u8,
    pub previews: Vec<JpegRef>,
}

pub struct ImportSession {
    pub source: PathBuf,
    pub label: String,
    pub volume: Option<Volume>,
    pub phase: ImportPhase,
    pub cancel: Arc<AtomicBool>,
    pub scan_rx: Option<Receiver<ScanMsg>>,
    pub run_rx: Option<Receiver<ImportEvent>>,
    pub scan: Option<Arc<ScanResult>>,
    pub summary: Option<ScanSummary>,
    pub progress: ImportProgress,
    pub result: Option<ImportSummary>,
    pub thumbs: Option<ImageCache<usize>>,
    pub hide_imported: bool,
    pub selected: BTreeSet<usize>,
    pub warnings: Vec<String>,
    /// Visible group indices (after hide_imported), in display order.
    pub visible: Vec<usize>,
}

pub struct Toast {
    pub text: String,
    pub until: f64,
    pub error: bool,
}

/// Work run on the catalog writer thread with its connection.
pub type WriterTask = Box<dyn FnOnce(&Library, &mut Catalog) + Send>;

pub enum WriteJob {
    Rating(Vec<i64>, u8),
    Xmp(Vec<i64>),
    /// Work that changes files and the catalog together (moving photos into a 작품 folder), run
    /// in order with the sidecar writes so a sidecar is never written to a file's old place.
    Run(WriterTask),
}

pub enum Maintenance {
    Thumbs { done: Arc<std::sync::atomic::AtomicUsize>, total: usize, cancel: Arc<AtomicBool>, rx: Receiver<i64> },
}

pub struct PhotoApp {
    pub cfg: Config,
    pub pal: Palette,
    pub applied_look: Option<theme::Look>,
    pub lib: Option<LibState>,
    pub lib_error: Option<String>,
    pub volumes: Vec<Volume>,
    volume_rx: Receiver<Vec<Volume>>,
    _volume_stop: Arc<AtomicBool>,
    known_volumes: HashSet<PathBuf>,
    pub view: MainView,
    pub side: SideSel,
    pub mode: GridMode,
    pub filter: Filter,
    pub filtered: Vec<usize>,
    pub filtered_pos: HashMap<i64, usize>,
    pub filter_gen: u64,
    pub facets: Option<Facets>,
    pub sel: Selection,
    pub viewer: Option<Viewer>,
    /// When Space was last pressed: a second press within [`DOUBLE_TAP`] zooms in.
    pub space_at: Option<f64>,
    /// Space in the viewer goes back to the grid only once no second press came (this time).
    pub space_close_at: Option<f64>,
    pub thumbs: Option<ImageCache<i64>>,
    pub previews: Option<ImageCache<PreviewKey>>,
    pub import: Option<ImportSession>,
    pub show_filters: bool,
    pub settings_open: bool,
    pub toasts: Vec<Toast>,
    pub hud: Option<(Hud, f64)>,
    pub grid_layout: Option<GridLayout>,
    pub card_layout: Option<CardLayout>,
    pub scroll_req: Option<ScrollReq>,
    pub info: Option<InfoCache>,
    pub tag_input: String,
    pub search: String,
    pub writer: Option<Sender<WriteJob>>,
    pub analysis_job: Option<crate::app_cull::AnalysisJob>,
    pub analysis_pending: bool,
    /// Photos analysed when the running analysis last triggered a rescore.
    pub analysed_at_rescore: usize,
    pub export_job: Option<crate::app_export::ExportJob>,
    pub export_dialog: Option<crate::app_export::ExportDialog>,
    /// 작품 폴더: running gather / restore, the dialog, and the status of the shown project.
    pub works_job: Option<crate::app_works::WorksJob>,
    pub works_dialog: Option<crate::app_works::WorksDialog>,
    pub works_status: Option<(u32, photoorg_core::works::GatherPlan)>,
    /// The project (작품) whose themes the chips and ⇧1–9 assign.
    pub active_project: Option<u32>,
    pub project_dialog: Option<crate::app_projects::ProjectDialog>,
    pub rescore_rx: Option<Receiver<crate::app_cull::RescoreResult>>,
    pub labels_dirty_at: Option<f64>,
    pub cull_report: Option<photoorg_core::cull::Report>,
    /// "내 셀렉 정확도" measurements, job and dialog.
    pub accuracy: crate::app_accuracy::Accuracy,
    /// 설정 → 라이선스 (website build).
    #[cfg(feature = "license")]
    pub license: crate::app_license::LicenseState,
    pub xmp_enabled: Arc<AtomicBool>,
    pub refilter_needed: bool,
    /// Refilter once the focus leaves this photo (themes in "고를 사진": the photo stays until the
    /// user moves on, so several themes can go on it).
    pub refilter_on_move: Option<i64>,
    pub maintenance: Option<Maintenance>,
    pub last_grid_cols: usize,
    pub grid_offset: f32,
    /// Last scroll position of the library view (see [`LibraryPlace`]).
    pub library_place: Option<LibraryPlace>,
    /// Area of the central panel (for centring overlays).
    pub central_rect: egui::Rect,
    /// Strip under the photos (grid, viewer) where action feedback appears, this frame.
    pub status_band: Option<egui::Rect>,
    pub focus_search: bool,
    pub library_missing_since: Option<f64>,
    /// App Sandbox: a card or folder waiting to be granted once (see app_access.rs).
    pub access_request: Option<(PathBuf, Option<Volume>)>,
    /// 지우기 (휴지통으로): the confirmation and the running job.
    pub delete: crate::app_delete::DeleteState,
}

impl PhotoApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        crate::fonts::install(&cc.egui_ctx);
        let cfg = Config::load();
        let (vtx, vrx) = unbounded();
        let ctx = cc.egui_ctx.clone();
        let stop = volumes::spawn_watcher(Duration::from_millis(1500), move |v| {
            let _ = vtx.send(v);
            ctx.request_repaint();
        });
        let mut app = PhotoApp {
            pal: Palette::cream(),
            applied_look: None,
            cfg,
            lib: None,
            lib_error: None,
            volumes: Vec::new(),
            volume_rx: vrx,
            _volume_stop: stop,
            known_volumes: HashSet::new(),
            view: MainView::Welcome,
            side: SideSel::All,
            mode: GridMode::All,
            filter: Filter::default(),
            filtered: Vec::new(),
            filtered_pos: HashMap::new(),
            filter_gen: 0,
            facets: None,
            sel: Selection::default(),
            viewer: None,
            space_at: None,
            space_close_at: None,
            thumbs: None,
            previews: None,
            import: None,
            show_filters: false,
            settings_open: false,
            toasts: Vec::new(),
            hud: None,
            grid_layout: None,
            card_layout: None,
            scroll_req: None,
            info: None,
            tag_input: String::new(),
            search: String::new(),
            writer: None,
            analysis_job: None,
            analysis_pending: false,
            analysed_at_rescore: 0,
            export_job: None,
            export_dialog: None,
            works_job: None,
            works_dialog: None,
            works_status: None,
            active_project: None,
            project_dialog: None,
            rescore_rx: None,
            labels_dirty_at: None,
            cull_report: None,
            accuracy: Default::default(),
            #[cfg(feature = "license")]
            license: Default::default(),
            xmp_enabled: Arc::new(AtomicBool::new(true)),
            refilter_needed: false,
            refilter_on_move: None,
            maintenance: None,
            last_grid_cols: 1,
            grid_offset: 0.0,
            library_place: None,
            central_rect: egui::Rect::NOTHING,
            status_band: None,
            focus_search: false,
            library_missing_since: None,
            access_request: None,
            delete: Default::default(),
        };
        app.xmp_enabled.store(app.cfg.write_xmp, Ordering::Relaxed);
        app.apply_theme(&cc.egui_ctx);
        #[cfg(feature = "license")]
        app.license_init(&cc.egui_ctx);
        // App Sandbox: take up the folders granted in earlier runs (library, cards).
        photoorg_macos::restore();
        if let Some(p) = app.cfg.library.clone() {
            app.open_library(&cc.egui_ctx, p, false);
        }
        app
    }

    // ------------------------------------------------------------------------------------------
    // Library

    pub fn open_library(&mut self, ctx: &egui::Context, path: PathBuf, create: bool) {
        self.cancel_maintenance();
        self.cancel_analysis();
        if !create && !path.exists() {
            self.lib_error = Some(if photoorg_macos::is_sandboxed() {
                format!(
                    "라이브러리 폴더를 열 수 없습니다: {}\n외장 하드를 연결하고, 아래에서 폴더를 한 번 다시 골라 주세요.",
                    path.display()
                )
            } else {
                format!("라이브러리를 찾을 수 없습니다: {}\n외장 하드가 연결되어 있는지 확인하세요.", path.display())
            });
            self.view = MainView::Welcome;
            return;
        }
        match Self::load_library(&path) {
            Ok(state) => {
                self.thumbs = Some(ImageCache::new("thumb", ctx, 3, 900, asset_thumb_factory(state.lib.clone())));
                self.previews = Some(
                    ImageCache::new("preview", ctx, 2, 6, preview_factory(state.lib.clone())).with_uploads_per_frame(1),
                );
                let (wtx, wrx) = unbounded();
                spawn_writer(state.lib.clone(), wrx, self.xmp_enabled.clone());
                self.writer = Some(wtx);
                self.cfg.remember_library(state.lib.root.clone());
                self.cfg.save();
                self.lib = Some(state);
                self.lib_error = None;
                self.view = MainView::Library;
                self.sel = Selection::default();
                self.viewer = None;
                if let Some(st) = &mut self.lib {
                    st.keep_pct = self.cfg.keep_pct;
                    crate::app_cull::refresh_totals(st, self.cfg.cull_threshold);
                }
                self.cull_report = None;
                self.accuracy =
                    self.lib.as_ref().map(|st| crate::app_accuracy::Accuracy::load(&st.catalog)).unwrap_or_default();
                self.load_active_project();
                self.library_place = None;
                self.select_side(SideSel::All);
                self.ensure_missing_thumbnails(ctx);
                self.ensure_analysis(ctx);
            }
            Err(e) => {
                self.lib_error = Some(format!("라이브러리를 열 수 없습니다: {e:#}"));
                self.view = MainView::Welcome;
            }
        }
    }

    fn load_library(path: &std::path::Path) -> anyhow::Result<LibState> {
        let lib = Library::open(path)?;
        let catalog = lib.catalog()?;
        let index = catalog.load_index()?;
        let totals = filter::facets(&index, &Filter::default());
        Ok(LibState {
            lib,
            catalog,
            index,
            totals,
            cull_counts: [0; 6],
            open_count: 0,
            keep_pct: 15,
            analysed: 0,
            project_counts: Default::default(),
        })
    }

    pub fn choose_library(&mut self, ctx: &egui::Context) {
        let mut dlg = rfd::FileDialog::new().set_title("사진 라이브러리 폴더 선택 (외장 하드 권장)");
        if let Some(p) = &self.cfg.library {
            dlg = dlg.set_directory(p);
        }
        if let Some(dir) = dlg.pick_folder() {
            photoorg_macos::remember(&dir);
            self.open_library(ctx, dir, true);
        }
    }

    /// Generate thumbnails that are missing (e.g. after an interrupted import) in the background.
    pub fn ensure_missing_thumbnails(&mut self, ctx: &egui::Context) {
        let Some(st) = &self.lib else { return };
        let have = st.lib.thumbs().and_then(|t| t.ids()).unwrap_or_default();
        let todo: Vec<i64> =
            st.index.assets.iter().filter(|a| !a.is_video() && !have.contains(&a.id)).map(|a| a.id).collect();
        if !todo.is_empty() {
            self.rebuild_thumbnails(ctx, todo);
        }
    }

    pub fn cancel_maintenance(&mut self) {
        if let Some(Maintenance::Thumbs { cancel, .. }) = self.maintenance.take() {
            cancel.store(true, Ordering::Relaxed);
        }
    }

    pub fn rebuild_thumbnails(&mut self, ctx: &egui::Context, ids: Vec<i64>) {
        let Some(st) = &self.lib else { return };
        if self.maintenance.is_some() {
            return;
        }
        let lib = st.lib.clone();
        let total = ids.len();
        let done = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let cancel = Arc::new(AtomicBool::new(false));
        let (tx, rx) = unbounded();
        let (d2, c2, ctx2) = (done.clone(), cancel.clone(), ctx.clone());
        std::thread::spawn(move || {
            let _ = import::generate_thumbnails(&lib, &ids, &c2, &|id, n| {
                d2.store(n, Ordering::Relaxed);
                let _ = tx.send(id);
                ctx2.request_repaint();
            });
            d2.store(total, Ordering::Relaxed);
            ctx2.request_repaint();
        });
        self.maintenance = Some(Maintenance::Thumbs { done, total, cancel, rx });
    }

    // ------------------------------------------------------------------------------------------
    // Filtering / navigation

    pub fn refilter(&mut self) {
        let Some(st) = &self.lib else {
            self.filtered.clear();
            return;
        };
        self.filter.text = self.search.clone();
        self.filtered = filter::apply(&st.index, &self.filter);
        self.filtered_pos = self.filtered.iter().enumerate().map(|(p, &i)| (st.index.assets[i].id, p)).collect();
        self.filter_gen += 1;
        self.facets = None;
        self.refilter_needed = false;
        self.refilter_on_move = None;
        let pos = &self.filtered_pos;
        self.sel.ids.retain(|id| pos.contains_key(id));
        if self.sel.focus.is_some_and(|f| !pos.contains_key(&f)) {
            self.sel.focus = self.sel.ids.iter().next().copied();
        }
    }

    pub fn facets(&mut self) -> Option<&Facets> {
        if self.facets.is_none() {
            let st = self.lib.as_ref()?;
            let mut f = self.filter.clone();
            f.text = self.search.clone();
            self.facets = Some(filter::facets_with(&st.index, &f, self.cfg.cull_threshold, self.cfg.keep_pct));
        }
        self.facets.as_ref()
    }

    pub fn select_side(&mut self, s: SideSel) {
        if let SideSel::Device(p) = &s {
            self.start_scan(p.clone(), self.volumes.iter().find(|v| &v.path == p).cloned());
            self.side = s;
            return;
        }
        // The new view is current while its filter is set up (set_active_project looks at it).
        self.side = s.clone();
        self.filter = Filter::default();
        self.search.clear();
        let mut scroll = ScrollReq::Top;
        match &s {
            // Back to where the library was left (the newest photos the first time).
            SideSel::All => {
                scroll = match self.library_place {
                    Some(LibraryPlace { mode, anchor: Some(id), offset }) => {
                        self.mode = mode;
                        ScrollReq::Anchor(id, offset)
                    }
                    Some(LibraryPlace { mode, anchor: None, offset }) => {
                        self.mode = mode;
                        ScrollReq::Offset(offset)
                    }
                    None => ScrollReq::Bottom,
                }
            }
            SideSel::Rated => self.filter.rating = RatingFilter::AtLeast(1),
            // Not judged yet: no stars and not picked or rejected (X takes a photo out of here).
            SideSel::Unrated => {
                self.filter.rating = RatingFilter::Unrated;
                self.filter.cull = photoorg_core::filter::CullFilter::Undecided;
            }
            SideSel::Recent => {
                if let Some(imp) = self.lib.as_ref().and_then(|s| s.index.imports.first()) {
                    self.filter.import_id = Some(imp.id);
                }
            }
            SideSel::Videos => self.filter.format = FormatFilter::Video,
            SideSel::Pairs => self.filter.format = FormatFilter::Pair,
            SideSel::Camera(c) => {
                self.filter.cameras.insert(*c);
            }
            SideSel::Lens(l) => {
                self.filter.lenses.insert(*l);
            }
            SideSel::Import(id) => self.filter.import_id = Some(*id),
            SideSel::Candidates => {
                self.filter.cull = photoorg_core::filter::CullFilter::Candidates(self.cfg.cull_threshold)
            }
            SideSel::Rejected => self.filter.cull = photoorg_core::filter::CullFilter::Rejected,
            SideSel::Picked => self.filter.cull = photoorg_core::filter::CullFilter::Picked,
            SideSel::Keepers => self.filter.cull = photoorg_core::filter::CullFilter::Keepers(self.cfg.keep_pct),
            SideSel::BurstBest => self.filter.cull = photoorg_core::filter::CullFilter::BurstBest,
            SideSel::Scene(s) => {
                self.filter.scenes.insert(*s);
            }
            SideSel::Project(p) => {
                self.filter.project = Some(ProjectFilter::In(*p));
                self.set_active_project(Some(*p));
            }
            SideSel::Theme(t) => {
                self.filter.tags.insert(*t);
                let p = self.lib.as_ref().and_then(|st| st.index.tags.get(t).and_then(|t| t.project));
                if p.is_some() {
                    self.set_active_project(p);
                }
            }
            SideSel::Device(_) | SideSel::None => {}
        }
        // Rated / picked photos: optionally without those already sent to the project.
        if s.sends_to_project() && self.cfg.hide_sent {
            if let Some(p) = self.active_project {
                self.filter.project = Some(ProjectFilter::Missing(p));
            }
        }
        if !matches!(s, SideSel::All) && matches!(self.mode, GridMode::Years | GridMode::Months) {
            self.mode = GridMode::Days;
        }
        self.side = s;
        self.view = if self.lib.is_some() { MainView::Library } else { MainView::Welcome };
        self.viewer = None;
        self.refilter();
        self.scroll_req = Some(scroll);
    }

    pub fn filter_changed(&mut self) {
        self.refilter();
        self.scroll_req = Some(ScrollReq::Top);
    }

    pub fn asset_at(&self, pos: usize) -> Option<&photoorg_core::AssetRecord> {
        let st = self.lib.as_ref()?;
        self.filtered.get(pos).map(|&i| &st.index.assets[i])
    }

    pub fn focus_pos(&self) -> Option<usize> {
        self.sel.focus.and_then(|id| self.filtered_pos.get(&id).copied())
    }

    pub fn set_focus(&mut self, pos: usize, extend: bool) {
        let Some(id) = self.asset_at(pos).map(|a| a.id) else { return };
        if extend {
            let anchor = self.sel.anchor.unwrap_or(pos);
            let (a, b) = (anchor.min(pos), anchor.max(pos));
            let ids: Vec<i64> = (a..=b).filter_map(|p| self.asset_at(p).map(|a| a.id)).collect();
            self.sel.ids = ids.into_iter().collect();
        } else {
            self.sel.ids = [id].into();
            self.sel.anchor = Some(pos);
        }
        self.sel.focus = Some(id);
        if self.viewer.is_none() {
            self.scroll_req = Some(ScrollReq::Item(pos));
        }
    }

    pub fn open_viewer(&mut self, id: i64) {
        self.sel.focus = Some(id);
        if !self.sel.ids.contains(&id) {
            self.sel.ids = [id].into();
        }
        self.viewer = Some(Viewer { id, zoom: None });
        self.space_close_at = None;
    }

    pub fn close_viewer(&mut self) {
        if let Some(v) = self.viewer.take() {
            if self.refilter_needed {
                self.refilter();
            }
            if let Some(&p) = self.filtered_pos.get(&v.id) {
                self.scroll_req = Some(ScrollReq::Item(p));
            }
        }
    }

    pub fn viewer_step(&mut self, delta: i64) {
        let Some(v) = &self.viewer else { return };
        let Some(&p) = self.filtered_pos.get(&v.id) else { return };
        let np = (p as i64 + delta).clamp(0, self.filtered.len() as i64 - 1) as usize;
        let zoom = v.zoom;
        if let Some(id) = self.asset_at(np).map(|a| a.id) {
            self.viewer = Some(Viewer { id, zoom });
            self.sel.ids = [id].into();
            self.sel.focus = Some(id);
            self.sel.anchor = Some(np);
        }
    }

    // ------------------------------------------------------------------------------------------
    // Rating & tags

    pub fn target_ids(&self) -> Vec<i64> {
        if let Some(v) = &self.viewer {
            return vec![v.id];
        }
        self.sel.ids.iter().copied().collect()
    }

    pub fn rate(&mut self, ctx: &egui::Context, ids: Vec<i64>, rating: u8) {
        if ids.is_empty() {
            return;
        }
        let Some(st) = &mut self.lib else { return };
        for id in &ids {
            if let Some(a) = st.index.get_mut(*id) {
                a.rating = rating;
            }
        }
        crate::app_cull::refresh_totals(st, self.cfg.cull_threshold);
        if let Some(w) = &self.writer {
            let _ = w.send(WriteJob::Rating(ids.clone(), rating));
        }
        self.hud = Some((Hud::Rating(rating), ctx.input(|i| i.time) + 0.9));
        self.facets = None;
        self.labels_dirty_at = Some(ctx.input(|i| i.time));
        self.after_label_change(ids.len());
    }

    pub fn add_tag(&mut self, ids: Vec<i64>, name: &str) {
        let name = name.trim();
        if name.is_empty() || ids.is_empty() {
            return;
        }
        let Some(st) = &mut self.lib else { return };
        match st.catalog.add_tag(&ids, name, TagKind::User) {
            Ok(tag) => {
                for id in &ids {
                    if let Some(a) = st.index.get_mut(*id) {
                        if !a.tags.contains(&tag.id) {
                            a.tags.push(tag.id);
                        }
                    }
                }
                st.index.tags.entry(tag.id).or_insert(tag);
                crate::app_cull::refresh_totals(st, self.cfg.cull_threshold);
                if let Some(w) = &self.writer {
                    let _ = w.send(WriteJob::Xmp(ids));
                }
                self.info = None;
                self.facets = None;
            }
            Err(e) => self.toast(format!("태그 추가 실패: {e:#}"), true),
        }
    }

    pub fn remove_tag(&mut self, ids: Vec<i64>, tag: u32) {
        let Some(st) = &mut self.lib else { return };
        if st.catalog.remove_tag(&ids, tag).is_ok() {
            for id in &ids {
                if let Some(a) = st.index.get_mut(*id) {
                    a.tags.retain(|t| *t != tag);
                }
            }
            crate::app_cull::refresh_totals(st, self.cfg.cull_threshold);
            if let Some(w) = &self.writer {
                let _ = w.send(WriteJob::Xmp(ids));
            }
            self.info = None;
            self.facets = None;
        }
    }

    pub fn info_for(&mut self, id: i64) -> Option<&InfoCache> {
        if self.info.as_ref().map(|i| i.id) != Some(id) {
            let st = self.lib.as_ref()?;
            let meta = st.catalog.metadata(id).ok()?;
            let files = st.catalog.files_for(id).unwrap_or_default();
            let a = st.index.get(id)?;
            let mut tags: Vec<(u32, String, TagKind)> = a
                .tags
                .iter()
                .filter_map(|t| st.index.tags.get(t).map(|ti| (ti.id, ti.name.clone(), ti.kind)))
                .collect();
            tags.sort_by(|a, b| b.2.cmp(&a.2).then(a.1.cmp(&b.1)));
            let analysis = st.catalog.analysis(id).ok().flatten();
            let scene_user = st.catalog.scene_user(id).ok().flatten();
            self.info = Some(InfoCache { id, meta, files, tags, analysis, scene_user });
        }
        self.info.as_ref()
    }

    pub fn toast(&mut self, text: impl Into<String>, error: bool) {
        self.toasts.push(Toast { text: text.into(), until: f64::NAN, error });
    }

    // ------------------------------------------------------------------------------------------
    // Import

    pub fn start_scan(&mut self, source: PathBuf, volume: Option<Volume>) {
        if let Some(s) = &self.import {
            if matches!(s.phase, ImportPhase::Running) {
                if s.source == source {
                    self.view = MainView::Import;
                    return;
                }
                self.toast("가져오기가 진행 중입니다. 완료 후 다시 시도하세요.", true);
                return;
            }
            if s.source == source && !matches!(s.phase, ImportPhase::Failed(_)) {
                self.view = MainView::Import;
                self.viewer = None;
                return;
            }
        }
        // App Sandbox: a card or folder is granted once in the folder panel (app_access.rs).
        if photoorg_macos::is_sandboxed() && !photoorg_macos::can_read(&source) {
            photoorg_macos::restore();
            if !photoorg_macos::can_read(&source) {
                self.access_request = Some((source, volume));
                return;
            }
        }
        let cancel = Arc::new(AtomicBool::new(false));
        let (tx, rx) = unbounded();
        let include_videos = self.cfg.import.include_videos;
        let (src, c2) = (source.clone(), cancel.clone());
        std::thread::spawn(move || {
            let tx2 = tx.clone();
            let r = import::scan_source(&src, include_videos, &c2, &move |d, t| {
                let _ = tx2.send(ScanMsg::Progress(d, t));
            });
            let _ = tx.send(ScanMsg::Done(r.map_err(|e| format!("{e:#}"))));
        });
        let label = volume.as_ref().map(|v| v.name.clone()).unwrap_or_else(|| import::source_label(&source));
        self.import = Some(ImportSession {
            source,
            label,
            volume,
            phase: ImportPhase::Scanning { done: 0, total: 0 },
            cancel,
            scan_rx: Some(rx),
            run_rx: None,
            scan: None,
            summary: None,
            progress: ImportProgress::default(),
            result: None,
            thumbs: None,
            hide_imported: false,
            selected: BTreeSet::new(),
            warnings: Vec::new(),
            visible: Vec::new(),
        });
        self.view = MainView::Import;
        self.viewer = None;
    }

    pub fn pick_import_folder(&mut self) {
        let mut dlg = rfd::FileDialog::new().set_title("가져올 폴더 선택");
        if photoorg_macos::is_sandboxed() {
            // Cards are here; the sandbox may not let the app find them by itself.
            dlg = dlg.set_directory("/Volumes");
        }
        if let Some(dir) = dlg.pick_folder() {
            photoorg_macos::remember(&dir);
            self.side = SideSel::Device(dir.clone());
            self.import = None;
            self.start_scan(dir, None);
        }
    }

    pub fn start_import(&mut self, ctx: &egui::Context, only_selected: bool) {
        let Some(st) = &self.lib else { return };
        let Some(sess) = &mut self.import else { return };
        let Some(scan) = sess.scan.clone() else { return };
        let mut scan: ScanResult = (*scan).clone();
        if only_selected && !sess.selected.is_empty() {
            let sel = &sess.selected;
            scan.groups =
                scan.groups.into_iter().enumerate().filter(|(i, _)| sel.contains(i)).map(|(_, g)| g).collect();
        }
        let lib = st.lib.clone();
        let opts = self.cfg.import.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        sess.cancel = cancel.clone();
        let (tx, rx) = unbounded();
        let ctx2 = ctx.clone();
        std::thread::Builder::new()
            .name("import".into())
            .spawn(move || {
                let (etx, erx) = import::channel();
                let fwd_tx = tx.clone();
                let fwd_ctx = ctx2.clone();
                let fwd = std::thread::spawn(move || {
                    for ev in erx {
                        let _ = fwd_tx.send(ev);
                        fwd_ctx.request_repaint();
                    }
                });
                let r = import::run_import(&lib, &scan, &opts, &etx, cancel);
                drop(etx);
                let _ = fwd.join();
                let _ = tx.send(ImportEvent::Finished(r.map_err(|e| format!("{e:#}"))));
                ctx2.request_repaint();
            })
            .expect("spawn import");
        sess.run_rx = Some(rx);
        sess.phase = ImportPhase::Running;
        sess.progress = ImportProgress::default();
        sess.result = None;
        sess.warnings.clear();
    }

    // ------------------------------------------------------------------------------------------
    // Frame

    /// Apply the configured look (re-evaluated every frame for `System`).
    pub fn apply_theme(&mut self, ctx: &egui::Context) {
        let sys_dark = ctx.input(|i| i.raw.system_theme).map(|t| t == egui::Theme::Dark).unwrap_or(false);
        let look = self.cfg.theme.look(sys_dark);
        if self.applied_look != Some(look) {
            self.pal = Palette::for_look(look);
            theme::apply(ctx, &self.pal);
            self.applied_look = Some(look);
        }
    }

    pub fn set_theme(&mut self, ctx: &egui::Context, t: ThemePref) {
        self.cfg.theme = t;
        self.apply_theme(ctx);
        self.cfg.save();
    }

    fn poll(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        // Cards.
        let mut latest = None;
        while let Ok(v) = self.volume_rx.try_recv() {
            latest = Some(v);
        }
        if let Some(v) = latest {
            let new: Vec<Volume> = v.iter().filter(|x| !self.known_volumes.contains(&x.path)).cloned().collect();
            self.known_volumes = v.iter().map(|x| x.path.clone()).collect();
            self.volumes = v;
            let busy = self.import.as_ref().is_some_and(|s| matches!(s.phase, ImportPhase::Running));
            if !new.is_empty() {
                // App Sandbox: a card granted before becomes readable again.
                photoorg_macos::restore();
            }
            if let Some(first) = new.first() {
                if self.cfg.auto_open_import && self.lib.is_some() && !busy {
                    self.side = SideSel::Device(first.path.clone());
                    self.start_scan(first.path.clone(), Some(first.clone()));
                }
                self.toast(format!("카드 연결됨: {}", first.name), false);
            }
        }
        // Library drive re-appeared?
        if self.lib.is_none() {
            if let Some(p) = self.cfg.library.clone() {
                let last = self.library_missing_since.unwrap_or(0.0);
                if self.lib_error.is_some() && now - last > 2.0 {
                    self.library_missing_since = Some(now);
                    // App Sandbox: the drive's remembered access is taken up once it is back.
                    photoorg_macos::restore();
                    if p.exists() {
                        self.open_library(ctx, p, false);
                    }
                }
            }
        }

        // Import scanning / running.
        let mut new_rows = Vec::new();
        let mut finished: Option<Result<ImportSummary, String>> = None;
        if let Some(sess) = &mut self.import {
            if let Some(rx) = &sess.scan_rx {
                let mut done_msg = None;
                while let Ok(m) = rx.try_recv() {
                    match m {
                        ScanMsg::Progress(d, t) => sess.phase = ImportPhase::Scanning { done: d, total: t },
                        ScanMsg::Done(r) => done_msg = Some(r),
                    }
                }
                if let Some(r) = done_msg {
                    sess.scan_rx = None;
                    match r {
                        Ok(mut scan) => {
                            if let Some(st) = &self.lib {
                                let _ = import::mark_existing(&mut scan, &st.catalog);
                            }
                            sess.summary = Some(scan.summary(self.cfg.import.include_videos));
                            let items: Vec<SourceItem> = scan
                                .groups
                                .iter()
                                .map(|g| SourceItem {
                                    path: g.files[0].path.clone(),
                                    kind: g.files[0].kind,
                                    orientation: g.meta.orientation,
                                    previews: g.previews.clone(),
                                })
                                .collect();
                            sess.thumbs = Some(ImageCache::new("src", ctx, 3, 600, source_factory(Arc::new(items))));
                            sess.scan = Some(Arc::new(scan));
                            sess.phase = ImportPhase::Ready;
                        }
                        Err(e) => sess.phase = ImportPhase::Failed(e),
                    }
                }
            }
            if let Some(rx) = &sess.run_rx {
                while let Ok(ev) = rx.try_recv() {
                    match ev {
                        ImportEvent::Progress(p) => sess.progress = p,
                        ImportEvent::AssetAdded(id) | ImportEvent::AssetUpdated(id) => new_rows.push(id),
                        ImportEvent::ThumbReady(id) => {
                            if let Some(t) = &mut self.thumbs {
                                t.invalidate(id);
                            }
                        }
                        ImportEvent::Warning(w) => sess.warnings.push(w),
                        ImportEvent::Finished(r) => finished = Some(r),
                    }
                }
            }
        }
        if !new_rows.is_empty() {
            if let Some(st) = &mut self.lib {
                let rows: Vec<_> = new_rows.iter().filter_map(|id| st.catalog.load_row(*id).ok().flatten()).collect();
                st.index.upsert(rows);
                if let Ok(imps) = st.catalog.imports() {
                    st.index.imports = imps;
                }
                crate::app_cull::refresh_totals(st, self.cfg.cull_threshold);
            }
            for id in &new_rows {
                if let Some(t) = &mut self.thumbs {
                    t.retry_missing(*id);
                }
            }
            if self.viewer.is_some() {
                self.refilter_needed = true;
            } else {
                self.refilter();
            }
        }
        if let Some(r) = finished {
            self.finish_import(ctx, r);
        }

        // Maintenance (thumbnail rebuild).
        let mut maint_done = false;
        if let Some(Maintenance::Thumbs { done, total, rx, .. }) = &self.maintenance {
            while let Ok(id) = rx.try_recv() {
                if let Some(t) = &mut self.thumbs {
                    t.invalidate(id);
                }
            }
            maint_done = done.load(Ordering::Relaxed) >= *total;
        }
        if maint_done {
            self.maintenance = None;
        }

        // Toasts.
        for t in &mut self.toasts {
            if t.until.is_nan() {
                t.until = now + if t.error { 6.0 } else { 3.5 };
            }
        }
        self.toasts.retain(|t| t.until > now);
        if !self.toasts.is_empty() || self.hud.is_some() {
            ctx.request_repaint_after(Duration::from_millis(250));
        }
        if self.hud.is_some_and(|(_, until)| until < now) {
            self.hud = None;
        }
        self.poll_cull(ctx);
        self.poll_export(ctx);
        self.poll_works(ctx);
        self.poll_accuracy(ctx);
        self.poll_delete(ctx);
        #[cfg(feature = "license")]
        self.poll_license();
    }

    fn finish_import(&mut self, ctx: &egui::Context, r: Result<ImportSummary, String>) {
        self.ensure_analysis(ctx);
        if let Some(st) = &mut self.lib {
            if let Ok(imps) = st.catalog.imports() {
                st.index.imports = imps;
            }
            crate::app_cull::refresh_totals(st, self.cfg.cull_threshold);
        }
        let Some(sess) = &mut self.import else { return };
        sess.run_rx = None;
        match r {
            Ok(sum) => {
                let msg = if sum.cancelled {
                    format!("가져오기 취소됨 — {}개 가져옴", sum.assets_added)
                } else {
                    format!(
                        "가져오기 완료 — 새 사진 {}개{}",
                        sum.assets_added,
                        if sum.duplicates > 0 {
                            format!(", 중복 {}개 건너뜀", sum.duplicates)
                        } else {
                            String::new()
                        }
                    )
                };
                let failed = !sum.failures.is_empty();
                sess.result = Some(sum);
                sess.phase = ImportPhase::Done;
                // Refresh "already imported" markers.
                if let (Some(st), Some(scan)) = (&self.lib, &sess.scan) {
                    let mut s: ScanResult = (**scan).clone();
                    let _ = import::mark_existing(&mut s, &st.catalog);
                    sess.summary = Some(s.summary(self.cfg.import.include_videos));
                    sess.scan = Some(Arc::new(s));
                }
                sess.selected.clear();
                let eject = self.cfg.eject_after_import && !failed;
                let vol = sess.volume.clone();
                self.toast(msg, failed);
                if eject {
                    if let Some(v) = vol {
                        self.eject(&v);
                    }
                }
            }
            Err(e) => {
                sess.phase = ImportPhase::Failed(e.clone());
                self.toast(format!("가져오기 실패: {e}"), true);
            }
        }
    }

    pub fn eject(&mut self, v: &Volume) {
        // In the App Sandbox through NSWorkspace (helper programs are not allowed there).
        let res =
            photoorg_macos::eject(&v.path).unwrap_or_else(|| volumes::eject(&v.path).map_err(|e| format!("{e:#}")));
        match res {
            Ok(()) => {
                self.toast(format!("{} 추출됨 — 카드를 안전하게 분리할 수 있습니다", v.name), false);
                if matches!(self.side, SideSel::Device(ref p) if p == &v.path) {
                    self.side = SideSel::None;
                }
            }
            Err(e) => self.toast(format!("{} 추출 실패: {e}", v.name), true),
        }
    }

    fn handle_keys(&mut self, ctx: &egui::Context) {
        if ctx.egui_wants_keyboard_input()
            || self.settings_open
            || self.delete.dialog.is_some()
            || self.access_request.is_some()
        {
            return;
        }
        if self.view != MainView::Library || self.lib.is_none() {
            return;
        }
        let now = ctx.input(|i| i.time);
        let keys: Vec<(Key, Option<Key>, egui::Modifiers, bool)> = ctx.input(|i| {
            i.events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::Key { key, physical_key, pressed: true, modifiers, repeat } => {
                        Some((*key, *physical_key, *modifiers, *repeat))
                    }
                    _ => None,
                })
                .collect()
        });
        for (key, physical, mods, repeat) in keys {
            let (cmd, shift) = (mods.command, mods.shift);
            // ⇧1–9: themes of the active project (the logical key is "!", "@", … with Shift).
            let digit = match physical.unwrap_or(key) {
                Key::Num1 => Some(1),
                Key::Num2 => Some(2),
                Key::Num3 => Some(3),
                Key::Num4 => Some(4),
                Key::Num5 => Some(5),
                Key::Num6 => Some(6),
                Key::Num7 => Some(7),
                Key::Num8 => Some(8),
                Key::Num9 => Some(9),
                _ => None,
            };
            if let (Some(n), true, false) = (digit, shift, cmd) {
                self.theme_key(ctx, n);
                continue;
            }
            let rating = match key {
                Key::F1 => Some(1),
                Key::F2 => Some(2),
                Key::F3 => Some(3),
                Key::F4 => Some(4),
                Key::F5 => Some(5),
                Key::Num1 if !cmd && !shift => Some(1),
                Key::Num2 if !cmd && !shift => Some(2),
                Key::Num3 if !cmd && !shift => Some(3),
                Key::Num4 if !cmd && !shift => Some(4),
                Key::Num5 if !cmd && !shift => Some(5),
                Key::Num0 | Key::Backtick | Key::F6 if !cmd => Some(0),
                _ => None,
            };
            if let Some(r) = rating {
                let ids = self.target_ids();
                self.rate(ctx, ids, r);
                continue;
            }
            match key {
                Key::X if !cmd => self.toggle_flag(ctx, -1),
                Key::P if !cmd => self.toggle_flag(ctx, 1),
                Key::U if !cmd => {
                    let ids = self.target_ids();
                    self.set_flag(ctx, ids, 0);
                }
                // 휴지통으로 (asks first): Delete, Backspace, ⌘⌫.
                Key::Delete | Key::Backspace
                    if !self.accuracy.open
                        && self.export_dialog.is_none()
                        && self.works_dialog.is_none()
                        && self.project_dialog.is_none() =>
                {
                    let ids = self.target_ids();
                    self.ask_delete(ids);
                }
                Key::Num1 if cmd => self.set_mode(GridMode::Years),
                Key::Num2 if cmd => self.set_mode(GridMode::Months),
                Key::Num3 if cmd => self.set_mode(GridMode::Days),
                Key::Num4 if cmd => self.set_mode(GridMode::All),
                Key::I if cmd => {
                    self.cfg.show_info = !self.cfg.show_info;
                }
                Key::F if cmd => {
                    self.focus_search = true;
                }
                Key::A if cmd && self.viewer.is_none() => {
                    let st = self.lib.as_ref().unwrap();
                    self.sel.ids = self.filtered.iter().map(|&i| st.index.assets[i].id).collect();
                }
                Key::Plus | Key::Equals if cmd => self.zoom_tiles(1.2),
                Key::Minus if cmd => self.zoom_tiles(1.0 / 1.2),
                Key::Escape => {
                    if self.viewer.is_some() {
                        self.close_viewer();
                    } else {
                        self.sel.ids.clear();
                    }
                }
                // Space: the grid <-> one photo; twice quickly: 100% on the middle of the photo.
                Key::Space if !repeat => self.space_key(now),
                Key::Space => {}
                Key::Enter => {
                    if self.viewer.is_some() {
                        self.close_viewer();
                    } else {
                        self.open_focused();
                    }
                }
                Key::Z if self.viewer.is_some() => self.toggle_zoom(),
                Key::ArrowLeft | Key::ArrowRight | Key::ArrowUp | Key::ArrowDown => {
                    if self.viewer.is_some() {
                        match key {
                            Key::ArrowLeft | Key::ArrowUp => self.viewer_step(-1),
                            _ => self.viewer_step(1),
                        }
                    } else if matches!(self.mode, GridMode::Days | GridMode::All) && !self.filtered.is_empty() {
                        let cols = self.last_grid_cols.max(1) as i64;
                        let cur = self.focus_pos().map(|p| p as i64);
                        let next = match (cur, key) {
                            (None, _) => 0,
                            (Some(p), Key::ArrowLeft) => p - 1,
                            (Some(p), Key::ArrowRight) => p + 1,
                            (Some(p), Key::ArrowUp) => p - cols,
                            (Some(p), _) => p + cols,
                        };
                        let next = next.clamp(0, self.filtered.len() as i64 - 1) as usize;
                        self.set_focus(next, shift);
                    }
                }
                _ => {}
            }
        }
    }

    pub fn set_mode(&mut self, m: GridMode) {
        if self.mode != m {
            self.mode = m;
            self.viewer = None;
            let anchor = self.sel.focus.and_then(|id| self.filtered_pos.get(&id).copied());
            self.scroll_req = Some(match (m, anchor) {
                (GridMode::Days | GridMode::All, Some(p)) => ScrollReq::Item(p),
                _ => ScrollReq::Bottom,
            });
        }
    }

    pub fn zoom_tiles(&mut self, f: f32) {
        self.cfg.tile_size = (self.cfg.tile_size * f).clamp(64.0, 420.0);
    }

    /// Opens the viewer on the focused (or first selected, or first) photo.
    fn open_focused(&mut self) {
        let id =
            self.sel.focus.or_else(|| self.sel.ids.iter().next().copied()).or_else(|| self.asset_at(0).map(|a| a.id));
        if let Some(id) = id {
            self.open_viewer(id);
        }
    }

    /// Fit <-> 100% on the middle of the photo.
    pub fn toggle_zoom(&mut self) {
        if let Some(v) = &mut self.viewer {
            v.zoom = if v.zoom.is_some() { None } else { Some(ZOOM_CENTER) };
        }
    }

    /// Space opens the photo at once; in the photo, going back to the grid waits [`DOUBLE_TAP`]
    /// for a second press, which zooms in (or back out) instead.
    fn space_key(&mut self, now: f64) {
        if self.space_at.take().is_some_and(|t| now - t < DOUBLE_TAP) {
            self.space_close_at = None;
            self.toggle_zoom();
            return;
        }
        self.space_at = Some(now);
        if self.viewer.is_some() {
            self.space_close_at = Some(now + DOUBLE_TAP);
        } else {
            self.open_focused();
        }
    }

    /// Goes back to the grid once a single Space press has had no second one.
    fn space_tick(&mut self, ctx: &egui::Context) {
        let Some(t) = self.space_close_at else { return };
        let now = ctx.input(|i| i.time);
        if now >= t {
            self.space_close_at = None;
            self.close_viewer();
        } else {
            ctx.request_repaint_after(Duration::from_secs_f64(t - now));
        }
    }
}

impl eframe::App for PhotoApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.apply_theme(&ctx);
        self.poll(&ctx);
        if let Some(t) = &mut self.thumbs {
            t.begin_frame(&ctx);
        }
        if let Some(p) = &mut self.previews {
            p.begin_frame(&ctx);
        }
        if let Some(s) = &mut self.import {
            if let Some(t) = &mut s.thumbs {
                t.begin_frame(&ctx);
            }
        }
        if self.viewer.is_none()
            && (self.refilter_needed || self.refilter_on_move.is_some_and(|id| self.sel.focus != Some(id)))
        {
            self.refilter();
        }
        self.space_tick(&ctx);
        self.handle_keys(&ctx);

        self.draw(ui);

        if let Some(t) = &mut self.thumbs {
            t.end_frame();
        }
        if let Some(p) = &mut self.previews {
            p.end_frame();
        }
        if let Some(s) = &mut self.import {
            if let Some(t) = &mut s.thumbs {
                t.end_frame();
            }
            if matches!(s.phase, ImportPhase::Running | ImportPhase::Scanning { .. }) {
                ctx.request_repaint_after(Duration::from_millis(200));
            }
        }
        if self.maintenance.is_some() {
            ctx.request_repaint_after(Duration::from_millis(300));
        }
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        self.pal.content_bg.to_normalized_gamma_f32()
    }

    fn on_exit(&mut self) {
        self.cfg.save();
        self.cancel_maintenance();
        if let Some(s) = &self.import {
            s.cancel.store(true, Ordering::Relaxed);
        }
    }
}

// ----------------------------------------------------------------------------------------------
// Loaders & background writer

fn asset_thumb_factory(lib: Library) -> LoaderFactory<i64> {
    Arc::new(move || {
        let store = lib.thumbs().ok();
        Box::new(move |id: i64| {
            let Some(store) = &store else { return Loaded::Missing };
            match store.get(id) {
                Ok(Some(bytes)) => match preview::decode_jpeg(&bytes, None) {
                    Ok(img) => Loaded::Image(color_image(&img)),
                    Err(_) => Loaded::Missing,
                },
                _ => Loaded::Missing,
            }
        })
    })
}

fn preview_factory(lib: Library) -> LoaderFactory<PreviewKey> {
    Arc::new(move || {
        let catalog = lib.catalog().ok();
        let lib = lib.clone();
        Box::new(move |k: PreviewKey| {
            let Some(cat) = &catalog else { return Loaded::Missing };
            let files = cat.files_for(k.id).unwrap_or_default();
            let orientation = cat.metadata(k.id).map(|m| m.orientation).unwrap_or(1);
            for f in files.iter().filter(|f| !f.kind.is_video()) {
                match preview::load_image(&lib.abs(&f.rel_path), f.kind, orientation, k.edge, None) {
                    Ok(img) => return Loaded::Image(color_image(&img)),
                    Err(e) => log::debug!("preview {}: {e:#}", f.rel_path),
                }
            }
            Loaded::Missing
        })
    })
}

fn source_factory(items: Arc<Vec<SourceItem>>) -> LoaderFactory<usize> {
    Arc::new(move || {
        let items = items.clone();
        Box::new(move |i: usize| {
            let Some(it) = items.get(i) else { return Loaded::Missing };
            if it.kind.is_video() {
                return Loaded::Missing;
            }
            match preview::quick_thumbnail(&it.path, it.kind, it.orientation, &it.previews) {
                Ok(img) => Loaded::Image(color_image(&img)),
                Err(_) => Loaded::Missing,
            }
        })
    })
}

fn spawn_writer(lib: Library, rx: Receiver<WriteJob>, write_xmp: Arc<AtomicBool>) {
    std::thread::Builder::new()
        .name("catalog-writer".into())
        .spawn(move || {
            let Ok(mut cat) = lib.catalog() else { return };
            for job in rx {
                match job {
                    WriteJob::Rating(ids, r) => {
                        if let Err(e) = cat.set_rating(&ids, r) {
                            log::error!("rating: {e:#}");
                        }
                        if write_xmp.load(Ordering::Relaxed) {
                            for id in &ids {
                                if let Err(e) = lib.write_xmp(&cat, *id) {
                                    log::warn!("xmp {id}: {e:#}");
                                }
                            }
                        }
                    }
                    WriteJob::Xmp(ids) => {
                        if write_xmp.load(Ordering::Relaxed) {
                            for id in &ids {
                                let _ = lib.write_xmp(&cat, *id);
                            }
                        }
                    }
                    WriteJob::Run(f) => f(&lib, &mut cat),
                }
            }
        })
        .expect("spawn writer");
}
