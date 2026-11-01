//! Display formatting helpers (Korean UI).

use chrono::{Datelike, NaiveDate, NaiveDateTime, Timelike, Weekday};

pub fn shutter(secs: f32) -> String {
    if !secs.is_finite() || secs <= 0.0 {
        return String::new();
    }
    if secs >= 0.3 {
        let r = (secs * 10.0).round() / 10.0;
        if (r - r.round()).abs() < 0.05 {
            format!("{}s", r.round() as i64)
        } else {
            format!("{r:.1}s")
        }
    } else {
        format!("1/{}s", (1.0 / secs).round() as i64)
    }
}

/// Shutter without unit suffix, used as a facet key label ("1/250", "2\"").
pub fn shutter_short(secs: f32) -> String {
    let s = shutter(secs);
    s.trim_end_matches('s').to_string()
}

pub fn aperture(f: f32) -> String {
    if !f.is_finite() || f <= 0.0 {
        return String::new();
    }
    let r = (f * 10.0).round() / 10.0;
    if (r - r.round()).abs() < 0.05 {
        format!("ƒ/{}", r.round() as i64)
    } else {
        format!("ƒ/{r:.1}")
    }
}

pub fn focal(mm: f32) -> String {
    if !mm.is_finite() || mm <= 0.0 {
        return String::new();
    }
    if (mm - mm.round()).abs() < 0.05 {
        format!("{}mm", mm.round() as i64)
    } else {
        format!("{mm:.1}mm")
    }
}

pub fn iso(v: u32) -> String {
    if v == 0 {
        String::new()
    } else {
        format!("ISO {v}")
    }
}

pub fn ev(v: f32) -> String {
    if !v.is_finite() {
        return String::new();
    }
    if v.abs() < 0.01 {
        "0 ev".into()
    } else {
        let s = format!("{:+.1}", v);
        format!("{} ev", s.trim_end_matches(".0"))
    }
}

pub fn bytes(n: u64) -> String {
    const U: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = n as f64;
    let mut i = 0;
    while v >= 1000.0 && i < U.len() - 1 {
        v /= 1000.0;
        i += 1;
    }
    if i == 0 {
        format!("{n} B")
    } else if v >= 100.0 {
        format!("{v:.0} {}", U[i])
    } else {
        format!("{v:.1} {}", U[i])
    }
}

pub fn weekday_ko(w: Weekday) -> &'static str {
    match w {
        Weekday::Mon => "월요일",
        Weekday::Tue => "화요일",
        Weekday::Wed => "수요일",
        Weekday::Thu => "목요일",
        Weekday::Fri => "금요일",
        Weekday::Sat => "토요일",
        Weekday::Sun => "일요일",
    }
}

/// "2026년 11월 1일 일요일"
pub fn date_long(d: NaiveDate) -> String {
    format!("{}년 {}월 {}일 {}", d.year(), d.month(), d.day(), weekday_ko(d.weekday()))
}

/// "2026년 11월 1일"
pub fn date_medium(d: NaiveDate) -> String {
    format!("{}년 {}월 {}일", d.year(), d.month(), d.day())
}

/// "2026년 11월"
pub fn month(y: i32, m: u32) -> String {
    format!("{y}년 {m}월")
}

/// "오후 3:04"
pub fn time_ko(t: NaiveDateTime) -> String {
    let h = t.hour();
    let (ampm, h12) = match h {
        0 => ("오전", 12),
        1..=11 => ("오전", h),
        12 => ("오후", 12),
        _ => ("오후", h - 12),
    };
    format!("{ampm} {h12}:{:02}", t.minute())
}

pub fn datetime_ko(t: NaiveDateTime) -> String {
    format!("{} {}", date_long(t.date()), time_ko(t))
}

pub fn count(n: usize) -> String {
    // 1,234
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

pub fn duration(secs: f64) -> String {
    let s = secs.max(0.0).round() as u64;
    if s < 60 {
        format!("{s}초")
    } else if s < 3600 {
        format!("{}분 {}초", s / 60, s % 60)
    } else {
        format!("{}시간 {}분", s / 3600, (s % 3600) / 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fmt() {
        assert_eq!(shutter(1.0 / 250.0), "1/250s");
        assert_eq!(shutter(2.0), "2s");
        assert_eq!(shutter(0.5), "0.5s");
        assert_eq!(aperture(2.8), "ƒ/2.8");
        assert_eq!(aperture(8.0), "ƒ/8");
        assert_eq!(focal(35.0), "35mm");
        assert_eq!(ev(0.67), "+0.7 ev");
        assert_eq!(ev(-1.0), "-1 ev");
        assert_eq!(bytes(1_500_000), "1.5 MB");
        assert_eq!(count(1234567), "1,234,567");
        let d = NaiveDate::from_ymd_opt(2026, 11, 1).unwrap();
        assert_eq!(date_long(d), "2026년 11월 1일 일요일");
    }
}
