//! Geometry of the photo grid (Days / All Photos) and the Years / Months card views.

use chrono::{Datelike, NaiveDate};
use photoorg_core::format;
use photoorg_core::LibraryIndex;
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GridMode {
    Years,
    Months,
    Days,
    All,
}

/// Gap between tiles in the flush (classic) look.
pub const GAP: f32 = 2.0;
pub const SIDE_PAD: f32 = 2.0;
pub const HEADER_H: f32 = 58.0;

#[derive(Clone, Debug)]
pub struct TileRow {
    pub y: f32,
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, Debug)]
pub struct Header {
    pub y: f32,
    pub title: String,
    pub subtitle: String,
    pub date: NaiveDate,
}

#[derive(Clone, Debug)]
pub struct GridLayout {
    pub key: (u64, u32, u32, GridMode, u32),
    pub cols: usize,
    pub tile: f32,
    pub gap: f32,
    pub rows: Vec<TileRow>,
    pub headers: Vec<Header>,
    pub height: f32,
    /// Row index of each filtered item.
    pub row_of: Vec<u32>,
}

impl GridLayout {
    pub fn build(
        idx: &LibraryIndex,
        filtered: &[usize],
        width: f32,
        target: f32,
        mode: GridMode,
        gen: u64,
        gap: f32,
    ) -> GridLayout {
        let usable = (width - SIDE_PAD * 2.0).max(50.0);
        let cols = (((usable + gap) / (target + gap)).round() as usize).max(1);
        let tile = ((usable - gap * (cols as f32 - 1.0)) / cols as f32).floor();
        let mut rows = Vec::new();
        let mut headers = Vec::new();
        let mut row_of = vec![0u32; filtered.len()];
        let mut y = 0.0;
        let mut push_rows = |start: usize, end: usize, y: &mut f32, rows: &mut Vec<TileRow>| {
            let mut s = start;
            while s < end {
                let e = (s + cols).min(end);
                for r in &mut row_of[s..e] {
                    *r = rows.len() as u32;
                }
                rows.push(TileRow { y: *y, start: s, end: e });
                *y += tile + gap;
                s = e;
            }
        };
        match mode {
            GridMode::Days => {
                let mut s = 0;
                while s < filtered.len() {
                    let d = idx.assets[filtered[s]].captured().date();
                    let mut e = s + 1;
                    while e < filtered.len() && idx.assets[filtered[e]].captured().date() == d {
                        e += 1;
                    }
                    let mut cams: BTreeMap<&str, usize> = BTreeMap::new();
                    for &i in &filtered[s..e] {
                        let c = idx.camera_name(&idx.assets[i]);
                        if !c.is_empty() {
                            *cams.entry(c).or_default() += 1;
                        }
                    }
                    let mut cams: Vec<(&str, usize)> = cams.into_iter().collect();
                    cams.sort_by_key(|c| std::cmp::Reverse(c.1));
                    let cam_txt: Vec<&str> = cams.iter().take(2).map(|c| c.0).collect();
                    let mut subtitle = format!("{}장", format::count(e - s));
                    if !cam_txt.is_empty() {
                        subtitle.push_str(" · ");
                        subtitle.push_str(&cam_txt.join(", "));
                    }
                    headers.push(Header { y, title: format::date_long(d), subtitle, date: d });
                    y += HEADER_H;
                    push_rows(s, e, &mut y, &mut rows);
                    y += 10.0;
                    s = e;
                }
            }
            _ => {
                y += SIDE_PAD;
                push_rows(0, filtered.len(), &mut y, &mut rows);
            }
        }
        GridLayout {
            key: (gen, width as u32, target as u32, mode, gap as u32),
            cols,
            tile,
            gap,
            rows,
            headers,
            height: y + 24.0,
            row_of,
        }
    }

    pub fn tile_rect_x(&self, col: usize) -> f32 {
        SIDE_PAD + col as f32 * (self.tile + self.gap)
    }

    /// Rows intersecting [y0, y1].
    pub fn visible_rows(&self, y0: f32, y1: f32) -> std::ops::Range<usize> {
        let a = self.rows.partition_point(|r| r.y + self.tile < y0);
        let b = self.rows.partition_point(|r| r.y <= y1);
        a..b.max(a)
    }

    pub fn y_of_item(&self, filtered_idx: usize) -> Option<f32> {
        self.row_of.get(filtered_idx).map(|&r| self.rows[r as usize].y)
    }

    pub fn y_of_date(&self, d: NaiveDate) -> Option<f32> {
        if let Some(h) = self.headers.iter().find(|h| h.date >= d) {
            return Some(h.y);
        }
        None
    }
}

#[derive(Clone, Debug)]
pub struct Card {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub title: String,
    pub subtitle: String,
    /// Filtered index of the key photo.
    pub key: usize,
    pub date: NaiveDate,
}

#[derive(Clone, Debug)]
pub struct CardLayout {
    pub key: (u64, u32, GridMode),
    pub cards: Vec<Card>,
    pub headers: Vec<(f32, String)>,
    pub height: f32,
}

/// Best photo of a group: highest rating, otherwise the middle one.
fn key_photo(idx: &LibraryIndex, filtered: &[usize], s: usize, e: usize) -> usize {
    let mid = s + (e - s) / 2;
    let best = (s..e)
        .filter(|&i| !idx.assets[filtered[i]].is_video())
        .max_by_key(|&i| (idx.assets[filtered[i]].rating, std::cmp::Reverse((i as i64 - mid as i64).abs())));
    best.unwrap_or(mid)
}

impl CardLayout {
    pub fn build(idx: &LibraryIndex, filtered: &[usize], width: f32, mode: GridMode, gen: u64) -> CardLayout {
        let pad = 20.0;
        let gap = 14.0;
        let usable = (width - pad * 2.0).max(100.0);
        let mut cards = Vec::new();
        let mut headers = Vec::new();
        let mut y = 16.0;
        // Group boundaries.
        let mut groups: Vec<(usize, usize)> = Vec::new();
        let key_of = |i: usize| {
            let d = idx.assets[filtered[i]].captured().date();
            match mode {
                GridMode::Years => (d.year(), 0),
                _ => (d.year(), d.month()),
            }
        };
        let mut s = 0;
        while s < filtered.len() {
            let k = key_of(s);
            let mut e = s + 1;
            while e < filtered.len() && key_of(e) == k {
                e += 1;
            }
            groups.push((s, e));
            s = e;
        }
        match mode {
            GridMode::Years => {
                let cols = if usable > 1100.0 {
                    3
                } else if usable > 650.0 {
                    2
                } else {
                    1
                };
                let w = (usable - gap * (cols as f32 - 1.0)) / cols as f32;
                let h = (w * 0.62).round();
                for (n, &(s, e)) in groups.iter().enumerate() {
                    let c = n % cols;
                    if c == 0 && n > 0 {
                        y += h + gap;
                    }
                    let d = idx.assets[filtered[s]].captured().date();
                    cards.push(Card {
                        x: pad + c as f32 * (w + gap),
                        y,
                        w,
                        h,
                        title: format!("{}년", d.year()),
                        subtitle: format!("{}장", format::count(e - s)),
                        key: key_photo(idx, filtered, s, e),
                        date: NaiveDate::from_ymd_opt(d.year(), 1, 1).unwrap_or(d),
                    });
                }
                if !groups.is_empty() {
                    y += h + gap;
                }
            }
            _ => {
                let cols = ((usable + gap) / (260.0 + gap)).floor().max(1.0) as usize;
                let w = (usable - gap * (cols as f32 - 1.0)) / cols as f32;
                let h = (w * 0.75).round();
                let mut n_in_year = 0;
                let mut cur_year = None;
                for &(s, e) in &groups {
                    let d = idx.assets[filtered[s]].captured().date();
                    if cur_year != Some(d.year()) {
                        if cur_year.is_some() {
                            y += h + gap + 8.0;
                        }
                        headers.push((y, format!("{}년", d.year())));
                        y += 44.0;
                        cur_year = Some(d.year());
                        n_in_year = 0;
                    } else if n_in_year % cols == 0 {
                        y += h + gap;
                    }
                    let c = n_in_year % cols;
                    cards.push(Card {
                        x: pad + c as f32 * (w + gap),
                        y,
                        w,
                        h,
                        title: format!("{}월", d.month()),
                        subtitle: format!("{}장", format::count(e - s)),
                        key: key_photo(idx, filtered, s, e),
                        date: NaiveDate::from_ymd_opt(d.year(), d.month(), 1).unwrap_or(d),
                    });
                    n_in_year += 1;
                }
                if !groups.is_empty() {
                    y += h + gap;
                }
            }
        }
        CardLayout { key: (gen, width as u32, mode), cards, headers, height: y + 24.0 }
    }
}
