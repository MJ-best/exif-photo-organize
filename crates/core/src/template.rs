//! Destination path templates, e.g. `{YYYY}/{YYYY}-{MM}-{DD}` and `{orig}`.

use crate::metadata::Metadata;
use chrono::{Datelike, NaiveDateTime, Timelike};

pub struct FolderPreset {
    pub template: &'static str,
    pub label: &'static str,
}

pub const FOLDER_PRESETS: &[FolderPreset] = &[
    FolderPreset { template: "{YYYY}/{YYYY}-{MM}-{DD}", label: "연도 / 날짜  (2026/2026-11-01)" },
    FolderPreset { template: "{YYYY}/{MM}/{DD}", label: "연 / 월 / 일  (2026/11/01)" },
    FolderPreset { template: "{YYYY}/{YYYY}-{MM}-{DD}/{camera}", label: "연도 / 날짜 / 카메라" },
    FolderPreset { template: "{camera}/{YYYY}/{YYYY}-{MM}-{DD}", label: "카메라 / 연도 / 날짜" },
    FolderPreset { template: "{YYYY}/{YYYY}-{MM}", label: "연도 / 월  (2026/2026-11)" },
];

pub const FILE_PRESETS: &[FolderPreset] = &[
    FolderPreset { template: "{orig}", label: "원본 파일명 유지  (IMG_0001)" },
    FolderPreset {
        template: "{YYYY}{MM}{DD}_{hh}{mm}{ss}_{orig}",
        label: "날짜_시간_원본  (20261101_142530_IMG_0001)",
    },
    FolderPreset { template: "{YYYY}-{MM}-{DD}_{orig}", label: "날짜_원본  (2026-11-01_IMG_0001)" },
];

pub const TOKENS: &[(&str, &str)] = &[
    ("{YYYY}", "연도 4자리"),
    ("{YY}", "연도 2자리"),
    ("{MM}", "월"),
    ("{DD}", "일"),
    ("{hh}", "시"),
    ("{mm}", "분"),
    ("{ss}", "초"),
    ("{camera}", "카메라"),
    ("{make}", "제조사"),
    ("{lens}", "렌즈"),
    ("{orig}", "원본 파일명"),
];

/// Make a string safe to use as a single path component on Windows, macOS and Linux.
pub fn sanitize(s: &str) -> String {
    let mut out: String = s
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    out = out.trim().trim_end_matches('.').trim().to_string();
    if out.is_empty() || out == "." || out == ".." {
        out = "_".into();
    }
    out
}

/// Render a template. `/` separates folders; each component is sanitised individually.
pub fn render(template: &str, meta: &Metadata, date: Option<NaiveDateTime>, orig_stem: &str) -> String {
    let unknown = "Unknown";
    let date = date.or(meta.captured_at);
    let (y, mo, d, h, mi, s) = match date {
        Some(t) => (t.year(), t.month(), t.day(), t.hour(), t.minute(), t.second()),
        None => (0, 0, 0, 0, 0, 0),
    };
    let camera = meta.camera().unwrap_or_else(|| unknown.into());
    let make = meta.make.as_deref().map(crate::metadata::clean_make).unwrap_or_else(|| unknown.into());
    let lens = meta.lens.clone().unwrap_or_else(|| unknown.into());
    let comps: Vec<String> = template
        .split(['/', '\\'])
        .filter(|c| !c.trim().is_empty())
        .map(|comp| {
            let mut c = comp.to_string();
            if date.is_some() {
                c = c
                    .replace("{YYYY}", &format!("{y:04}"))
                    .replace("{YY}", &format!("{:02}", y % 100))
                    .replace("{MM}", &format!("{mo:02}"))
                    .replace("{DD}", &format!("{d:02}"))
                    .replace("{hh}", &format!("{h:02}"))
                    .replace("{mm}", &format!("{mi:02}"))
                    .replace("{ss}", &format!("{s:02}"));
            } else {
                for t in ["{YYYY}", "{YY}", "{MM}", "{DD}", "{hh}", "{mm}", "{ss}"] {
                    c = c.replace(t, "0000");
                }
            }
            c = c
                .replace("{camera}", &camera)
                .replace("{make}", &make)
                .replace("{lens}", &lens)
                .replace("{orig}", orig_stem);
            sanitize(&c)
        })
        .collect();
    comps.join("/")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn render_templates() {
        let m = Metadata {
            make: Some("NIKON CORPORATION".into()),
            model: Some("NIKON Z 6".into()),
            captured_at: Some(chrono::NaiveDate::from_ymd_opt(2026, 3, 9).unwrap().and_hms_opt(14, 5, 6).unwrap()),
            ..Default::default()
        };
        assert_eq!(render("{YYYY}/{YYYY}-{MM}-{DD}", &m, None, "x"), "2026/2026-03-09");
        assert_eq!(render("{camera}/{YYYY}", &m, None, "x"), "Nikon Z 6/2026");
        assert_eq!(render("{YYYY}{MM}{DD}_{hh}{mm}{ss}_{orig}", &m, None, "DSC_0001"), "20260309_140506_DSC_0001");
        assert_eq!(sanitize("a:b/c"), "a_b_c");
        assert_eq!(sanitize(" .. "), "_");
    }
}
