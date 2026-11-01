//! Automatic tags derived from EXIF data.

use crate::media::formats;
use crate::metadata::Metadata;
use chrono::{Datelike, Timelike};

/// Tag categories (prefix before ':').
pub const CATEGORIES: &[&str] =
    &["카메라", "렌즈", "화각", "감도", "조리개", "셔터", "시간대", "계절", "방향", "형식", "기타"];

pub fn split(tag: &str) -> (&str, &str) {
    match tag.split_once(':') {
        Some((c, v)) => (c, v),
        None => ("", tag),
    }
}

pub fn auto_tags(m: &Metadata, fmt: u8) -> Vec<String> {
    let mut t = Vec::new();
    if let Some(c) = m.camera() {
        t.push(format!("카메라:{c}"));
    }
    if let Some(l) = &m.lens {
        t.push(format!("렌즈:{l}"));
    }
    if let Some(f) = m.focal_35mm.or(m.focal_length) {
        let v = match f {
            f if f < 21.0 => "초광각",
            f if f < 35.0 => "광각",
            f if f <= 70.0 => "표준",
            f if f <= 135.0 => "준망원",
            f if f <= 300.0 => "망원",
            _ => "초망원",
        };
        t.push(format!("화각:{v}"));
    }
    if let Some(iso) = m.iso {
        let v = match iso {
            0..=400 => "저감도",
            401..=1600 => "중감도",
            1601..=6400 => "고감도",
            _ => "초고감도",
        };
        t.push(format!("감도:{v}"));
    }
    if let Some(a) = m.aperture {
        let v = if a <= 2.8 {
            "개방"
        } else if a < 8.0 {
            "중간"
        } else {
            "조임"
        };
        t.push(format!("조리개:{v}"));
    }
    if let Some(s) = m.exposure_time {
        let v = if s >= 1.0 {
            "장노출"
        } else if s > 1.0 / 30.0 {
            "저속"
        } else if s <= 1.0 / 1000.0 {
            "고속"
        } else {
            "일반"
        };
        t.push(format!("셔터:{v}"));
    }
    if let Some(dt) = m.captured_at {
        let v = match dt.hour() {
            4..=6 => "새벽",
            7..=10 => "아침",
            11..=15 => "낮",
            16..=18 => "저녁",
            _ => "밤",
        };
        t.push(format!("시간대:{v}"));
        let s = match dt.month() {
            3..=5 => "봄",
            6..=8 => "여름",
            9..=11 => "가을",
            _ => "겨울",
        };
        t.push(format!("계절:{s}"));
    }
    if let (Some(w), Some(h)) = (m.width, m.height) {
        let (w, h) = if matches!(m.orientation, 5..=8) { (h, w) } else { (w, h) };
        let r = w as f32 / h.max(1) as f32;
        let v = if (r - 1.0).abs() < 0.05 {
            "정방형"
        } else if r > 1.0 {
            "가로"
        } else {
            "세로"
        };
        t.push(format!("방향:{v}"));
    }
    t.push(format!("형식:{}", formats::label(fmt)));
    if m.flash_fired == Some(true) {
        t.push("기타:플래시".into());
    }
    if m.gps.is_some() {
        t.push("기타:위치정보".into());
    }
    if m.exposure_bias.is_some_and(|b| b.abs() >= 0.3) {
        t.push(format!("기타:노출보정 {}", if m.exposure_bias.unwrap() > 0.0 { "+" } else { "-" }));
    }
    t
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tags() {
        let m = Metadata {
            make: Some("Canon".into()),
            model: Some("Canon EOS R6".into()),
            focal_length: Some(85.0),
            iso: Some(3200),
            aperture: Some(1.4),
            exposure_time: Some(1.0 / 2000.0),
            width: Some(6000),
            height: Some(4000),
            orientation: 6,
            captured_at: Some(chrono::NaiveDate::from_ymd_opt(2026, 10, 3).unwrap().and_hms_opt(17, 0, 0).unwrap()),
            ..Default::default()
        };
        let t = auto_tags(&m, formats::RAW | formats::JPEG);
        for want in [
            "카메라:Canon EOS R6",
            "화각:준망원",
            "감도:고감도",
            "조리개:개방",
            "셔터:고속",
            "시간대:저녁",
            "계절:가을",
            "방향:세로",
            "형식:RAW+JPEG",
        ] {
            assert!(t.iter().any(|x| x == want), "missing {want} in {t:?}");
        }
    }
}
