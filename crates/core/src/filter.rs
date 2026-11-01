//! In-memory filtering and facet counting.

use crate::catalog::{AssetRecord, LibraryIndex};
use crate::media::formats;
use chrono::NaiveDate;
use rayon::prelude::*;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RatingFilter {
    #[default]
    Any,
    AtLeast(u8),
    Exactly(u8),
    Unrated,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FormatFilter {
    #[default]
    Any,
    /// RAW and JPEG/HEIF together.
    Pair,
    /// Has a RAW file.
    HasRaw,
    /// Only JPEG/HEIF (no RAW).
    NoRaw,
    Video,
    Photo,
}

impl FormatFilter {
    pub const ALL: [FormatFilter; 6] = [
        FormatFilter::Pair,
        FormatFilter::HasRaw,
        FormatFilter::NoRaw,
        FormatFilter::Photo,
        FormatFilter::Video,
        FormatFilter::Any,
    ];

    pub fn label(self) -> &'static str {
        match self {
            FormatFilter::Any => "전체",
            FormatFilter::Pair => "RAW+JPEG",
            FormatFilter::HasRaw => "RAW 있음",
            FormatFilter::NoRaw => "JPEG/HEIF만",
            FormatFilter::Video => "동영상",
            FormatFilter::Photo => "사진",
        }
    }

    pub fn matches(self, f: u8) -> bool {
        let raw = f & formats::RAW != 0;
        let dev = f & (formats::JPEG | formats::HEIF) != 0;
        match self {
            FormatFilter::Any => true,
            FormatFilter::Pair => raw && dev,
            FormatFilter::HasRaw => raw,
            FormatFilter::NoRaw => !raw && f & formats::VIDEO == 0,
            FormatFilter::Video => f & formats::VIDEO != 0,
            FormatFilter::Photo => f & formats::VIDEO == 0,
        }
    }
}

/// Culling state filter.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CullFilter {
    #[default]
    Any,
    /// Undecided photos whose B-cut probability is at least this many percent.
    Candidates(u8),
    Rejected,
    Picked,
    Undecided,
    /// Photos that are not a weaker frame of a burst.
    BurstBest,
}

impl CullFilter {
    pub fn label(self) -> String {
        match self {
            CullFilter::Any => "전체".into(),
            CullFilter::Candidates(_) => "B컷 후보".into(),
            CullFilter::Rejected => "거절 (B컷)".into(),
            CullFilter::Picked => "픽 (A컷)".into(),
            CullFilter::Undecided => "아직 안 고름".into(),
            CullFilter::BurstBest => "연사 베스트만".into(),
        }
    }

    pub fn matches(self, a: &AssetRecord) -> bool {
        match self {
            CullFilter::Any => true,
            CullFilter::Candidates(t) => a.flag == 0 && a.b_prob.is_finite() && a.b_prob * 100.0 >= t as f32,
            CullFilter::Rejected => a.flag < 0,
            CullFilter::Picked => a.flag > 0,
            CullFilter::Undecided => a.flag == 0,
            CullFilter::BurstBest => a.burst_size < 2 || a.burst_rank == 0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Facet {
    Camera = 0,
    Lens,
    Iso,
    Focal,
    Aperture,
    Shutter,
    Rating,
    Format,
    Tag,
    Date,
    Import,
    Text,
    Cull,
    Scene,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Filter {
    pub cameras: BTreeSet<u32>,
    pub lenses: BTreeSet<u32>,
    /// Inclusive ranges.
    pub iso: Option<(u32, u32)>,
    pub focal: Option<(f32, f32)>,
    pub aperture: Option<(f32, f32)>,
    /// Seconds.
    pub shutter: Option<(f32, f32)>,
    pub rating: RatingFilter,
    pub format: FormatFilter,
    /// All of these tags must be present.
    pub tags: BTreeSet<u32>,
    pub date: Option<(NaiveDate, NaiveDate)>,
    pub import_id: Option<i64>,
    pub text: String,
    pub cull: CullFilter,
    /// Any of these scenes (see `cull::Scene`).
    pub scenes: BTreeSet<u8>,
}

fn in_range_f(v: f32, r: (f32, f32)) -> bool {
    v.is_finite() && v >= r.0 * 0.999 && v <= r.1 * 1.001
}

impl Filter {
    pub fn is_empty(&self) -> bool {
        *self == Filter::default()
    }

    /// Number of active facets (for the UI badge).
    pub fn active_count(&self) -> usize {
        [
            !self.cameras.is_empty(),
            !self.lenses.is_empty(),
            self.iso.is_some(),
            self.focal.is_some(),
            self.aperture.is_some(),
            self.shutter.is_some(),
            self.rating != RatingFilter::Any,
            self.format != FormatFilter::Any,
            !self.tags.is_empty(),
            self.date.is_some(),
            !self.text.trim().is_empty(),
            self.cull != CullFilter::Any,
            !self.scenes.is_empty(),
        ]
        .iter()
        .filter(|b| **b)
        .count()
    }

    /// Bit mask of facets that reject `a`.
    pub fn failing(&self, a: &AssetRecord, idx: &LibraryIndex, text: &str) -> u16 {
        let mut m = 0u16;
        let mut fail = |f: Facet| m |= 1 << f as u16;
        if !self.cameras.is_empty() && !self.cameras.contains(&a.camera) {
            fail(Facet::Camera);
        }
        if !self.lenses.is_empty() && !self.lenses.contains(&a.lens) {
            fail(Facet::Lens);
        }
        if let Some((lo, hi)) = self.iso {
            if a.iso == 0 || a.iso < lo || a.iso > hi {
                fail(Facet::Iso);
            }
        }
        if let Some(r) = self.focal {
            if !in_range_f(a.focal, r) {
                fail(Facet::Focal);
            }
        }
        if let Some(r) = self.aperture {
            if !in_range_f(a.aperture, r) {
                fail(Facet::Aperture);
            }
        }
        if let Some(r) = self.shutter {
            if !in_range_f(a.shutter, r) {
                fail(Facet::Shutter);
            }
        }
        let rating_ok = match self.rating {
            RatingFilter::Any => true,
            RatingFilter::AtLeast(n) => a.rating >= n,
            RatingFilter::Exactly(n) => a.rating == n,
            RatingFilter::Unrated => a.rating == 0,
        };
        if !rating_ok {
            fail(Facet::Rating);
        }
        if !self.format.matches(a.formats) {
            fail(Facet::Format);
        }
        if !self.tags.is_empty() && !self.tags.iter().all(|t| a.tags.contains(t)) {
            fail(Facet::Tag);
        }
        if let Some((from, to)) = self.date {
            let d = a.captured().date();
            if d < from || d > to {
                fail(Facet::Date);
            }
        }
        if let Some(imp) = self.import_id {
            if a.import_id != imp {
                fail(Facet::Import);
            }
        }
        if !text.is_empty() && !text_match(a, idx, text) {
            fail(Facet::Text);
        }
        if !self.cull.matches(a) {
            fail(Facet::Cull);
        }
        if !self.scenes.is_empty() && !self.scenes.contains(&a.scene) {
            fail(Facet::Scene);
        }
        m
    }

    pub fn matches(&self, a: &AssetRecord, idx: &LibraryIndex) -> bool {
        let text = self.text.trim().to_lowercase();
        self.failing(a, idx, &text) == 0
    }
}

fn text_match(a: &AssetRecord, idx: &LibraryIndex, needle: &str) -> bool {
    let hay = |s: &str| s.to_lowercase().contains(needle);
    hay(&a.name) || hay(idx.camera_name(a)) || hay(idx.lens_name(a)) || a.tags.iter().any(|t| hay(idx.tag_name(*t)))
}

/// Indices (into `idx.assets`) of matching assets, in capture order.
pub fn apply(idx: &LibraryIndex, f: &Filter) -> Vec<usize> {
    if f.is_empty() {
        return (0..idx.assets.len()).collect();
    }
    let text = f.text.trim().to_lowercase();
    idx.assets.par_iter().enumerate().filter(|(_, a)| f.failing(a, idx, &text) == 0).map(|(i, _)| i).collect()
}

/// Facet value counts. For each facet the count ignores that facet's own constraint, so users
/// can see what selecting another value would give.
#[derive(Clone, Debug, Default)]
pub struct Facets {
    pub cameras: Vec<(u32, usize)>,
    pub lenses: Vec<(u32, usize)>,
    pub isos: Vec<(u32, usize)>,
    /// Focal length rounded to mm.
    pub focals: Vec<(u32, usize)>,
    /// Aperture * 10.
    pub apertures: Vec<(u32, usize)>,
    /// Shutter in microseconds.
    pub shutters: Vec<(u64, usize)>,
    pub ratings: [usize; 6],
    pub formats: Vec<(FormatFilter, usize)>,
    pub tags: Vec<(u32, usize)>,
    pub total: usize,
    /// [B-cut candidates (at the filter's threshold, default 60 %), rejected, picked, undecided, burst best]
    pub cull: [usize; 5],
    pub scenes: Vec<(u8, usize)>,
}

#[derive(Default)]
struct Acc {
    cameras: BTreeMap<u32, usize>,
    lenses: BTreeMap<u32, usize>,
    isos: BTreeMap<u32, usize>,
    focals: BTreeMap<u32, usize>,
    apertures: BTreeMap<u32, usize>,
    shutters: BTreeMap<u64, usize>,
    ratings: [usize; 6],
    formats: [usize; 6],
    tags: BTreeMap<u32, usize>,
    total: usize,
    cull: [usize; 5],
    scenes: BTreeMap<u8, usize>,
}

impl Acc {
    fn merge(mut self, o: Acc) -> Acc {
        fn m<K: Ord + Copy>(a: &mut BTreeMap<K, usize>, b: BTreeMap<K, usize>) {
            for (k, v) in b {
                *a.entry(k).or_default() += v;
            }
        }
        m(&mut self.cameras, o.cameras);
        m(&mut self.lenses, o.lenses);
        m(&mut self.isos, o.isos);
        m(&mut self.focals, o.focals);
        m(&mut self.apertures, o.apertures);
        m(&mut self.shutters, o.shutters);
        m(&mut self.tags, o.tags);
        m(&mut self.scenes, o.scenes);
        for i in 0..5 {
            self.cull[i] += o.cull[i];
        }
        for i in 0..6 {
            self.ratings[i] += o.ratings[i];
            self.formats[i] += o.formats[i];
        }
        self.total += o.total;
        self
    }
}

pub fn facets(idx: &LibraryIndex, f: &Filter) -> Facets {
    facets_with(idx, f, 60)
}

/// Like [`facets`], counting B-cut candidates from `candidate_threshold` percent
/// (unless the filter itself selects candidates with its own threshold).
pub fn facets_with(idx: &LibraryIndex, f: &Filter, candidate_threshold: u8) -> Facets {
    let text = f.text.trim().to_lowercase();
    let threshold = match f.cull {
        CullFilter::Candidates(t) => t,
        _ => candidate_threshold,
    };
    let acc = idx
        .assets
        .par_iter()
        .fold(Acc::default, |mut acc, a| {
            let fail = f.failing(a, idx, &text);
            let ok_except = |facet: Facet| fail & !(1u16 << facet as u16) == 0;
            if fail == 0 {
                acc.total += 1;
            }
            if ok_except(Facet::Camera) && a.camera != 0 {
                *acc.cameras.entry(a.camera).or_default() += 1;
            }
            if ok_except(Facet::Lens) && a.lens != 0 {
                *acc.lenses.entry(a.lens).or_default() += 1;
            }
            if ok_except(Facet::Iso) && a.iso > 0 {
                *acc.isos.entry(a.iso).or_default() += 1;
            }
            if ok_except(Facet::Focal) && a.focal.is_finite() && a.focal > 0.0 {
                *acc.focals.entry(a.focal.round() as u32).or_default() += 1;
            }
            if ok_except(Facet::Aperture) && a.aperture.is_finite() && a.aperture > 0.0 {
                *acc.apertures.entry((a.aperture * 10.0).round() as u32).or_default() += 1;
            }
            if ok_except(Facet::Shutter) && a.shutter.is_finite() && a.shutter > 0.0 {
                *acc.shutters.entry((a.shutter as f64 * 1e6).round() as u64).or_default() += 1;
            }
            if ok_except(Facet::Rating) {
                acc.ratings[a.rating.min(5) as usize] += 1;
            }
            if ok_except(Facet::Format) {
                for (i, ff) in FormatFilter::ALL.iter().enumerate() {
                    if ff.matches(a.formats) {
                        acc.formats[i] += 1;
                    }
                }
            }
            if ok_except(Facet::Cull) {
                let kinds = [
                    CullFilter::Candidates(threshold),
                    CullFilter::Rejected,
                    CullFilter::Picked,
                    CullFilter::Undecided,
                    CullFilter::BurstBest,
                ];
                for (k, c) in kinds.iter().enumerate() {
                    if c.matches(a) {
                        acc.cull[k] += 1;
                    }
                }
            }
            if ok_except(Facet::Scene) && a.scene != 0 {
                *acc.scenes.entry(a.scene).or_default() += 1;
            }
            if fail == 0 || ok_except(Facet::Tag) {
                for t in &a.tags {
                    *acc.tags.entry(*t).or_default() += 1;
                }
            }
            acc
        })
        .reduce(Acc::default, Acc::merge);

    let sorted_by_count = |m: BTreeMap<u32, usize>| {
        let mut v: Vec<(u32, usize)> = m.into_iter().collect();
        v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        v
    };
    Facets {
        cameras: sorted_by_count(acc.cameras),
        lenses: sorted_by_count(acc.lenses),
        isos: acc.isos.into_iter().collect(),
        focals: acc.focals.into_iter().collect(),
        apertures: acc.apertures.into_iter().collect(),
        shutters: acc.shutters.into_iter().rev().collect(),
        ratings: acc.ratings,
        formats: FormatFilter::ALL.iter().copied().zip(acc.formats).collect(),
        tags: sorted_by_count(acc.tags),
        total: acc.total,
        cull: acc.cull,
        scenes: acc.scenes.into_iter().collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::{AssetRow, LibraryIndex};

    fn rec(id: i64, iso: u32, focal: f32, rating: u8, fmt: u8) -> AssetRecord {
        AssetRecord {
            id,
            captured_ms: id * 1000,
            camera: 0,
            lens: 0,
            focal,
            focal35: f32::NAN,
            aperture: 2.8,
            shutter: 0.004,
            iso,
            bias: 0.0,
            width: 0,
            height: 0,
            orientation: 1,
            rating,
            formats: fmt,
            flash: false,
            has_gps: false,
            date_from_file: false,
            import_id: 1,
            tags: vec![],
            name: format!("IMG_{id:04}"),
            flag: 0,
            scene: 0,
            b_prob: f32::NAN,
            reasons: 0,
            burst: 0,
            burst_rank: 0,
            burst_size: 0,
        }
    }

    fn index() -> LibraryIndex {
        let mut idx = LibraryIndex::new();
        let rows = vec![
            AssetRow {
                rec: rec(1, 100, 35.0, 0, formats::JPEG),
                camera: "Canon EOS R5".into(),
                lens: "RF35".into(),
                tags: vec![],
            },
            AssetRow {
                rec: rec(2, 3200, 85.0, 3, formats::RAW | formats::JPEG),
                camera: "Canon EOS R5".into(),
                lens: "RF85".into(),
                tags: vec![],
            },
            AssetRow {
                rec: rec(3, 800, 50.0, 5, formats::RAW),
                camera: "Sony ILCE-7M4".into(),
                lens: "FE50".into(),
                tags: vec![],
            },
            AssetRow { rec: rec(4, 0, f32::NAN, 0, formats::VIDEO), camera: "".into(), lens: "".into(), tags: vec![] },
        ];
        idx.upsert(rows);
        idx
    }

    #[test]
    fn filtering() {
        let idx = index();
        let mut f = Filter::default();
        assert_eq!(apply(&idx, &f).len(), 4);
        f.iso = Some((400, 6400));
        assert_eq!(apply(&idx, &f).len(), 2);
        f.rating = RatingFilter::AtLeast(4);
        assert_eq!(apply(&idx, &f).len(), 1);
        f = Filter { format: FormatFilter::Pair, ..Default::default() };
        assert_eq!(apply(&idx, &f).len(), 1);
        f = Filter { focal: Some((35.0, 50.0)), ..Default::default() };
        assert_eq!(apply(&idx, &f).len(), 2);
        f = Filter { text: "sony".into(), ..Default::default() };
        assert_eq!(apply(&idx, &f).len(), 1);
        let canon = idx.cameras.intern_lookup("Canon EOS R5");
        f = Filter { cameras: [canon].into(), ..Default::default() };
        assert_eq!(apply(&idx, &f).len(), 2);
    }

    #[test]
    fn cull_filter() {
        let mut idx = index();
        idx.get_mut(1).unwrap().b_prob = 0.9;
        idx.get_mut(2).unwrap().b_prob = 0.2;
        idx.get_mut(3).unwrap().flag = -1;
        idx.get_mut(3).unwrap().b_prob = 0.95;
        let f = Filter { cull: CullFilter::Candidates(60), ..Default::default() };
        assert_eq!(apply(&idx, &f).len(), 1, "rejected photos are no longer candidates");
        let f = Filter { cull: CullFilter::Rejected, ..Default::default() };
        assert_eq!(apply(&idx, &f).len(), 1);
        let fc = facets(&idx, &Filter::default());
        assert_eq!(fc.cull[0], 1);
        assert_eq!(fc.cull[1], 1);
    }

    #[test]
    fn facet_counts() {
        let idx = index();
        let canon = idx.cameras.intern_lookup("Canon EOS R5");
        let f = Filter { cameras: [canon].into(), ..Default::default() };
        let fc = facets(&idx, &f);
        assert_eq!(fc.total, 2);
        // Camera facet ignores its own selection.
        assert_eq!(fc.cameras.len(), 2);
        assert_eq!(fc.isos, vec![(100, 1), (3200, 1)]);
        assert_eq!(fc.ratings[3], 1);
    }
}

#[cfg(test)]
mod bench {
    use super::*;
    use crate::catalog::{AssetRecord, AssetRow, LibraryIndex};

    /// `cargo test --release -p photoorg-core bench_100k -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn bench_100k() {
        let cams = ["Canon EOS R5", "Sony ILCE-7M4", "Fujifilm X-T5", "Nikon Z 6_2", "Leica Q3"];
        let lenses = ["24-70", "70-200", "35mm", "50mm", "85mm", "16-35", "100-400"];
        let rows: Vec<AssetRow> = (0..100_000i64)
            .map(|i| AssetRow {
                rec: AssetRecord {
                    id: i + 1,
                    captured_ms: i * 60_000,
                    camera: 0,
                    lens: 0,
                    focal: [24.0, 35.0, 50.0, 85.0, 135.0, 200.0][(i % 6) as usize],
                    focal35: f32::NAN,
                    aperture: [1.4, 2.8, 4.0, 5.6, 8.0][(i % 5) as usize],
                    shutter: [1.0 / 4000.0, 1.0 / 250.0, 1.0 / 60.0, 0.5][(i % 4) as usize],
                    iso: [100, 200, 400, 800, 1600, 3200, 6400][(i % 7) as usize],
                    bias: 0.0,
                    width: 6000,
                    height: 4000,
                    orientation: 1,
                    rating: (i % 6) as u8,
                    formats: if i % 3 == 0 { 3 } else { 2 },
                    flash: false,
                    has_gps: false,
                    date_from_file: false,
                    import_id: i / 1000,
                    tags: vec![(i % 20) as u32, 20 + (i % 7) as u32],
                    name: format!("IMG_{:05}", i % 10000),
                    flag: 0,
                    scene: 1 + (i % 7) as u8,
                    b_prob: (i % 100) as f32 / 100.0,
                    reasons: 0,
                    burst: 0,
                    burst_rank: 0,
                    burst_size: 0,
                },
                camera: cams[(i % 5) as usize].into(),
                lens: lenses[(i % 7) as usize].into(),
                tags: vec![],
            })
            .collect();
        let t = std::time::Instant::now();
        let mut idx = LibraryIndex::new();
        idx.upsert(rows);
        println!("index build: {:?}", t.elapsed());
        let f = Filter {
            iso: Some((400, 3200)),
            rating: RatingFilter::AtLeast(3),
            focal: Some((35.0, 85.0)),
            ..Default::default()
        };
        let t = std::time::Instant::now();
        let n = apply(&idx, &f).len();
        println!("filter 100k -> {n}: {:?}", t.elapsed());
        let t = std::time::Instant::now();
        let fc = facets(&idx, &f);
        println!("facets 100k -> total {}: {:?}", fc.total, t.elapsed());
        let f = Filter { text: "sony".into(), ..Default::default() };
        let t = std::time::Instant::now();
        let n = apply(&idx, &f).len();
        println!("text search 100k -> {n}: {:?}", t.elapsed());
    }
}
