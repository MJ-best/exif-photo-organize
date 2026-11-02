//! Culling assistant state: background analysis, pick/reject flags, retraining and scores.

use crate::app::{Hud, LibState, PhotoApp, WriteJob};
use crossbeam_channel::unbounded;
use eframe::egui;
use photoorg_core::cull::{self, Report, Score};
use photoorg_core::filter::{self, CullFilter, Filter, RatingFilter};
use photoorg_core::LibraryIndex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

pub struct AnalysisJob {
    pub done: Arc<AtomicUsize>,
    pub total: usize,
    pub cancel: Arc<AtomicBool>,
    pub finished: Arc<AtomicBool>,
}

pub type RescoreResult = Result<(Vec<Score>, Report, Option<LibraryIndex>), String>;

/// Recompute sidebar counts (whole library).
pub fn refresh_totals(st: &mut LibState, threshold: u8) {
    st.totals = filter::facets(&st.index, &Filter::default());
    let kinds = [
        CullFilter::Candidates(threshold),
        CullFilter::Rejected,
        CullFilter::Picked,
        CullFilter::Undecided,
        CullFilter::BurstBest,
    ];
    let mut c = [0usize; 5];
    for a in &st.index.assets {
        for (k, f) in kinds.iter().enumerate() {
            if f.matches(a) {
                c[k] += 1;
            }
        }
    }
    st.cull_counts = c;
    st.analysed = st.index.assets.iter().filter(|a| a.b_prob.is_finite()).count();
    // Projects: photos in each, and rated photos still to sort.
    let mut pc: std::collections::HashMap<u32, (usize, usize)> =
        st.index.projects.iter().map(|p| (p.id, (0, 0))).collect();
    let min = st.project_min_rating.max(1);
    let mut in_projects = Vec::new();
    for a in &st.index.assets {
        in_projects.clear();
        for t in &a.tags {
            if let Some(p) = st.index.tags.get(t).and_then(|t| t.project) {
                if !in_projects.contains(&p) {
                    in_projects.push(p);
                }
            }
        }
        for (p, (n, todo)) in pc.iter_mut() {
            if in_projects.contains(p) {
                *n += 1;
            } else if a.rating >= min {
                *todo += 1;
            }
        }
    }
    st.project_counts = pc;
}

impl PhotoApp {
    /// Pick (1), reject (-1) or clear (0) the given photos.
    pub fn set_flag(&mut self, ctx: &egui::Context, ids: Vec<i64>, flag: i8) {
        if ids.is_empty() {
            return;
        }
        let threshold = self.cfg.cull_threshold;
        let Some(st) = &mut self.lib else { return };
        if let Err(e) = st.catalog.set_flag(&ids, flag) {
            self.toast(format!("저장 실패: {e:#}"), true);
            return;
        }
        for id in &ids {
            if let Some(a) = st.index.get_mut(*id) {
                a.flag = flag;
            }
        }
        refresh_totals(st, threshold);
        if let Some(w) = &self.writer {
            let _ = w.send(WriteJob::Xmp(ids.clone()));
        }
        self.hud = Some((Hud::Flag(flag), ctx.input(|i| i.time) + 0.9));
        self.facets = None;
        self.info = None;
        self.labels_dirty_at = Some(ctx.input(|i| i.time));
        self.after_label_change(ids.len());
    }

    /// X / P: toggle reject or pick on the target photos.
    pub fn toggle_flag(&mut self, ctx: &egui::Context, want: i8) {
        let ids = self.target_ids();
        let all_set = self.lib.as_ref().is_some_and(|st| {
            !ids.is_empty() && ids.iter().all(|id| st.index.get(*id).is_some_and(|a| a.flag == want))
        });
        self.set_flag(ctx, ids, if all_set { 0 } else { want });
    }

    /// Keep the view consistent after a rating / flag change (filters, auto-advance).
    pub fn after_label_change(&mut self, n: usize) {
        let filtered_by_label = self.filter.cull != CullFilter::Any
            || self.filter.rating != RatingFilter::Any
            || self.filter.project.is_some()
            || !self.filter.tags.is_empty();
        if filtered_by_label {
            self.refilter_in_place();
        }
        if self.viewer.is_some() && self.cfg.auto_advance && n == 1 {
            self.viewer_step(1);
        }
    }

    /// Refilter after labels changed, keeping the focus at the same place in the list (the next
    /// photo when the focused one left it). Waits until the viewer closes.
    pub fn refilter_in_place(&mut self) {
        if self.viewer.is_some() {
            self.refilter_needed = true;
            return;
        }
        let focus = self.focus_pos();
        self.refilter();
        if let Some(p) = focus {
            let p = p.min(self.filtered.len().saturating_sub(1));
            if let Some(id) = self.asset_at(p).map(|a| a.id) {
                self.sel.focus = Some(id);
                if self.sel.ids.is_empty() {
                    self.sel.ids.insert(id);
                }
            }
        }
    }

    /// Override the detected scene (None = back to automatic).
    pub fn set_scene_override(&mut self, ctx: &egui::Context, ids: Vec<i64>, scene: Option<u8>) {
        let Some(st) = &mut self.lib else { return };
        if let Err(e) = st.catalog.set_scene_user(&ids, scene) {
            self.toast(format!("저장 실패: {e:#}"), true);
            return;
        }
        if let Some(s) = scene {
            for id in &ids {
                if let Some(a) = st.index.get_mut(*id) {
                    a.scene = s;
                }
            }
        }
        self.info = None;
        // Rescoring also restores the automatic scene when the override is removed.
        self.labels_dirty_at = Some(ctx.input(|i| i.time) - 10.0);
    }

    /// Analyse photos that have no (current) analysis yet.
    pub fn ensure_analysis(&mut self, ctx: &egui::Context) {
        if !self.cfg.cull_auto {
            return;
        }
        if self.analysis_job.is_some() {
            self.analysis_pending = true;
            return;
        }
        let Some(st) = &self.lib else { return };
        let done = st.catalog.analyzed_ids(cull::ANALYSIS_VERSION).unwrap_or_default();
        let ids: Vec<i64> =
            st.index.assets.iter().rev().filter(|a| !a.is_video() && !done.contains(&a.id)).map(|a| a.id).collect();
        if ids.is_empty() {
            if self.cull_report.is_none() && st.index.assets.iter().any(|a| a.b_prob.is_finite()) {
                self.start_rescore(ctx, false);
            }
            return;
        }
        self.start_analysis(ctx, ids);
    }

    pub fn start_analysis(&mut self, ctx: &egui::Context, ids: Vec<i64>) {
        let Some(st) = &self.lib else { return };
        let lib = st.lib.clone();
        let job = AnalysisJob {
            done: Arc::new(AtomicUsize::new(0)),
            total: ids.len(),
            cancel: Arc::new(AtomicBool::new(false)),
            finished: Arc::new(AtomicBool::new(false)),
        };
        let (done, cancel, finished, ctx2) = (job.done.clone(), job.cancel.clone(), job.finished.clone(), ctx.clone());
        std::thread::Builder::new()
            .name("cull-analysis".into())
            .spawn(move || {
                let _ = cull::analyze_assets(&lib, &ids, &cancel, &|_, n| {
                    done.store(n, Ordering::Relaxed);
                    if n % 8 == 0 {
                        ctx2.request_repaint();
                    }
                });
                finished.store(true, Ordering::Relaxed);
                ctx2.request_repaint();
            })
            .expect("spawn analysis");
        self.analysis_job = Some(job);
        self.analysis_pending = false;
    }

    pub fn cancel_analysis(&mut self) {
        if let Some(j) = self.analysis_job.take() {
            j.cancel.store(true, Ordering::Relaxed);
        }
    }

    /// Retrain from the current labels and rescore (in the background).
    pub fn start_rescore(&mut self, ctx: &egui::Context, reload_index: bool) {
        if self.rescore_rx.is_some() {
            self.labels_dirty_at = Some(ctx.input(|i| i.time));
            return;
        }
        let Some(st) = &self.lib else { return };
        let lib = st.lib.clone();
        let settings = self.cfg.learn;
        let (tx, rx) = unbounded();
        let ctx2 = ctx.clone();
        std::thread::Builder::new()
            .name("cull-rescore".into())
            .spawn(move || {
                let r = (|| -> anyhow::Result<(Vec<Score>, Report, Option<LibraryIndex>)> {
                    let mut cat = lib.catalog()?;
                    let (scores, report) = cull::rescore(&mut cat, &settings)?;
                    let idx = if reload_index { Some(cat.load_index()?) } else { None };
                    Ok((scores, report, idx))
                })();
                let _ = tx.send(r.map_err(|e| format!("{e:#}")));
                ctx2.request_repaint();
            })
            .expect("spawn rescore");
        self.rescore_rx = Some(rx);
        self.labels_dirty_at = None;
    }

    /// Called every frame from `poll`.
    pub fn poll_cull(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        if let Some(job) = &self.analysis_job {
            if job.finished.load(Ordering::Relaxed) {
                self.analysis_job = None;
                self.start_rescore(ctx, true);
                if self.analysis_pending {
                    self.ensure_analysis(ctx);
                }
            } else {
                ctx.request_repaint_after(std::time::Duration::from_millis(300));
            }
        }
        if let Some(rx) = &self.rescore_rx {
            if let Ok(r) = rx.try_recv() {
                self.rescore_rx = None;
                match r {
                    Ok((scores, report, idx)) => self.apply_scores(scores, report, idx),
                    Err(e) => log::warn!("rescore: {e}"),
                }
            }
        }
        if let Some(t) = self.labels_dirty_at {
            if now - t > 1.5 && self.rescore_rx.is_none() && self.analysis_job.is_none() {
                self.start_rescore(ctx, false);
            } else {
                ctx.request_repaint_after(std::time::Duration::from_millis(500));
            }
        }
    }

    fn apply_scores(&mut self, scores: Vec<Score>, report: Report, idx: Option<LibraryIndex>) {
        let threshold = self.cfg.cull_threshold;
        let Some(st) = &mut self.lib else { return };
        if let Some(mut idx) = idx {
            // Keep any flags / ratings changed while the background job ran.
            for a in &mut idx.assets {
                if let Some(cur) = st.index.get(a.id) {
                    a.flag = cur.flag;
                    a.rating = cur.rating;
                }
            }
            st.index = idx;
            st.index.upsert(Vec::new());
        }
        for s in &scores {
            if let Some(a) = st.index.get_mut(s.id) {
                a.b_prob = s.b_prob;
                a.reasons = s.reasons;
                a.burst = s.burst;
                a.burst_rank = s.burst_rank;
                a.burst_size = s.burst_size;
                a.scene = s.scene;
            }
        }
        refresh_totals(st, threshold);
        self.cull_report = Some(report);
        self.facets = None;
        self.info = None;
        if self.viewer.is_some() {
            self.refilter_needed = true;
        } else {
            let keep = self.sel.focus;
            self.refilter();
            if keep.is_some() {
                self.sel.focus = keep.filter(|id| self.filtered_pos.contains_key(id));
            }
        }
    }

    /// Change the candidate threshold (sidebar counts, active filter).
    pub fn set_cull_threshold(&mut self, t: u8) {
        self.cfg.cull_threshold = t.clamp(5, 95);
        let t = self.cfg.cull_threshold;
        if let Some(st) = &mut self.lib {
            refresh_totals(st, t);
        }
        self.facets = None;
        if matches!(self.filter.cull, CullFilter::Candidates(_)) {
            self.filter.cull = CullFilter::Candidates(t);
            self.refilter();
        }
    }

    /// Analyse every photo again (e.g. after an update of the analysis).
    pub fn reanalyse_all(&mut self, ctx: &egui::Context) {
        self.cancel_analysis();
        let Some(st) = &self.lib else { return };
        let ids: Vec<i64> = st.index.assets.iter().rev().filter(|a| !a.is_video()).map(|a| a.id).collect();
        self.start_analysis(ctx, ids);
    }

    /// Mark every current B-cut candidate (in the filtered view) as rejected.
    pub fn reject_all_candidates(&mut self, ctx: &egui::Context) {
        let threshold = self.cfg.cull_threshold;
        let Some(st) = &self.lib else { return };
        let ids: Vec<i64> = self
            .filtered
            .iter()
            .map(|&i| &st.index.assets[i])
            .filter(|a| CullFilter::Candidates(threshold).matches(a))
            .map(|a| a.id)
            .collect();
        let n = ids.len();
        self.set_flag(ctx, ids, -1);
        self.toast(
            format!("{n}장을 B컷(거절)으로 표시했습니다. 잘못 걸러진 사진은 P(픽)로 되돌리면 학습에 반영됩니다."),
            false,
        );
    }
}
