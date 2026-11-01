//! `photoorg` — command line companion of Photo Organize.
//!
//! Useful for scripting, headless machines and debugging metadata parsing.

use anyhow::{bail, Context, Result};
use photoorg_core::filter::{self, Filter, FormatFilter, RatingFilter};
use photoorg_core::import::{self, ImportEvent, ImportOptions};
use photoorg_core::{format as fmt, metadata, preview, volumes, FileKind, Library};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

const HELP: &str = "photoorg — EXIF 기반 사진 정리 도구 (CLI)

사용법:
  photoorg volumes                          연결된 SD 카드(DCIM 폴더가 있는 볼륨) 목록
  photoorg info <파일>...                   EXIF 메타데이터와 내장 미리보기 정보 출력
  photoorg scan <원본폴더> [라이브러리]     가져오기 전 미리보기 (새 사진/중복 개수)
  photoorg import <원본폴더> <라이브러리> [옵션]
        --folders <템플릿>    폴더 구조 (기본: {YYYY}/{YYYY}-{MM}-{DD})
        --names <템플릿>      파일 이름 (기본: {orig})
        --raw-subfolder <이름> RAW 파일을 하위 폴더로 분리 (예: RAW)
        --no-videos           동영상 제외
        --no-verify           복사 후 체크섬 검증 생략
  photoorg list <라이브러리> [필터]
        --camera <이름> --lens <이름> --iso <최소-최대> --focal <최소-최대>
        --aperture <최소-최대> --min-rating <n> --format raw+jpeg|raw|jpeg|video
        --text <검색어>
  photoorg stats <라이브러리>                카메라/렌즈/ISO 통계
  photoorg rate <라이브러리> <별점 0-5> <asset id>...
  photoorg export-csv <라이브러리> <출력.csv>
  photoorg thumbs <라이브러리> [--all]       누락된(또는 전체) 썸네일 다시 만들기
  photoorg thumb <파일> <출력.jpg>           파일 하나의 썸네일 생성 (디버그)
";

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    if let Err(e) = run() {
        eprintln!("오류: {e:#}");
        std::process::exit(1);
    }
}

fn arg_value(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1).cloned())
}

fn range<T: std::str::FromStr>(s: &str) -> Result<(T, T)> {
    let (a, b) = s.split_once('-').unwrap_or((s, s));
    Ok((
        a.trim().parse().map_err(|_| anyhow::anyhow!("잘못된 범위 {s}"))?,
        b.trim().parse().map_err(|_| anyhow::anyhow!("잘못된 범위 {s}"))?,
    ))
}

fn run() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(cmd) = args.first().map(String::as_str) else {
        print!("{HELP}");
        return Ok(());
    };
    let rest = &args[1..];
    match cmd {
        "volumes" => {
            let v = volumes::camera_volumes();
            if v.is_empty() {
                println!("연결된 카메라 카드가 없습니다.");
            }
            for vol in v {
                println!("{}\t{}", vol.name, vol.path.display());
            }
        }
        "info" => {
            for p in rest {
                info(&PathBuf::from(p))?;
            }
        }
        "scan" => {
            let src = PathBuf::from(rest.first().context("원본 폴더가 필요합니다")?);
            let cancel = AtomicBool::new(false);
            let mut scan = import::scan_source(&src, true, &cancel, &|d, t| eprint!("\r메타데이터 읽는 중 {d}/{t}"))?;
            eprintln!();
            if let Some(lib) = rest.get(1) {
                let lib = Library::open(&PathBuf::from(lib))?;
                import::mark_existing(&mut scan, &lib.catalog()?)?;
            }
            let s = scan.summary(true);
            println!(
                "항목 {}개 (새 항목 {}, 이미 가져옴 {}, RAW+JPEG {}, 동영상 {})",
                s.groups, s.new_groups, s.imported_groups, s.raw_pairs, s.videos
            );
            println!("새 파일 {}개, {}", s.new_files, fmt::bytes(s.new_bytes));
            if let (Some(a), Some(b)) = (s.first, s.last) {
                println!("기간: {} ~ {}", fmt::datetime_ko(a), fmt::datetime_ko(b));
            }
            for (c, n) in s.cameras {
                println!("  {c}: {n}");
            }
        }
        "import" => {
            let src = PathBuf::from(rest.first().context("원본 폴더가 필요합니다")?);
            let lib = Library::open(&PathBuf::from(rest.get(1).context("라이브러리 폴더가 필요합니다")?))?;
            let mut opts = ImportOptions::default();
            if let Some(v) = arg_value(rest, "--folders") {
                opts.folder_template = v;
            }
            if let Some(v) = arg_value(rest, "--names") {
                opts.file_template = v;
            }
            if let Some(v) = arg_value(rest, "--raw-subfolder") {
                opts.raw_subfolder = v;
            }
            opts.include_videos = !rest.iter().any(|a| a == "--no-videos");
            opts.verify = !rest.iter().any(|a| a == "--no-verify");
            let cancel = Arc::new(AtomicBool::new(false));
            let mut scan = import::scan_source(&src, opts.include_videos, &cancel, &|d, t| {
                eprint!("\r메타데이터 읽는 중 {d}/{t}")
            })?;
            eprintln!();
            import::mark_existing(&mut scan, &lib.catalog()?)?;
            let (tx, rx) = crossbeam_channel_unbounded();
            let printer = std::thread::spawn(move || {
                for ev in rx {
                    match ev {
                        ImportEvent::Progress(p) => eprint!(
                            "\r{}/{} 파일, {} / {} ({}/s)        ",
                            p.files_done,
                            p.files_total,
                            fmt::bytes(p.bytes_done),
                            fmt::bytes(p.bytes_total),
                            fmt::bytes(p.speed() as u64)
                        ),
                        ImportEvent::Warning(w) => eprintln!("\n경고: {w}"),
                        _ => {}
                    }
                }
            });
            let sum = import::run_import(&lib, &scan, &opts, &tx, cancel)?;
            drop(tx);
            let _ = printer.join();
            eprintln!();
            println!(
                "완료: 새 사진 {}개, 기존 사진에 파일 추가 {}개, 파일 {}개 ({}) 복사, 중복 건너뜀 {}개, {}",
                sum.assets_added,
                sum.assets_updated,
                sum.files_copied,
                fmt::bytes(sum.bytes_copied),
                sum.duplicates,
                fmt::duration(sum.elapsed)
            );
            for f in &sum.failures {
                println!("실패: {f}");
            }
        }
        "list" => {
            let lib = Library::open(&PathBuf::from(rest.first().context("라이브러리 폴더가 필요합니다")?))?;
            let idx = lib.catalog()?.load_index()?;
            let mut f = Filter::default();
            if let Some(c) = arg_value(rest, "--camera") {
                let c = c.to_lowercase();
                f.cameras = (1..idx.cameras.len() as u32)
                    .filter(|i| idx.cameras.name(*i).to_lowercase().contains(&c))
                    .collect();
                if f.cameras.is_empty() {
                    bail!("카메라 '{c}' 없음");
                }
            }
            if let Some(c) = arg_value(rest, "--lens") {
                let c = c.to_lowercase();
                f.lenses =
                    (1..idx.lenses.len() as u32).filter(|i| idx.lenses.name(*i).to_lowercase().contains(&c)).collect();
                if f.lenses.is_empty() {
                    bail!("렌즈 '{c}' 없음");
                }
            }
            if let Some(v) = arg_value(rest, "--iso") {
                f.iso = Some(range(&v)?);
            }
            if let Some(v) = arg_value(rest, "--focal") {
                f.focal = Some(range(&v)?);
            }
            if let Some(v) = arg_value(rest, "--aperture") {
                f.aperture = Some(range(&v)?);
            }
            if let Some(v) = arg_value(rest, "--min-rating") {
                f.rating = RatingFilter::AtLeast(v.parse()?);
            }
            if let Some(v) = arg_value(rest, "--format") {
                f.format = match v.to_lowercase().as_str() {
                    "raw+jpeg" | "pair" => FormatFilter::Pair,
                    "raw" => FormatFilter::HasRaw,
                    "jpeg" | "jpg" => FormatFilter::NoRaw,
                    "video" => FormatFilter::Video,
                    _ => bail!("알 수 없는 형식 {v}"),
                };
            }
            if let Some(v) = arg_value(rest, "--text") {
                f.text = v;
            }
            let hits = filter::apply(&idx, &f);
            for &i in &hits {
                let a = &idx.assets[i];
                println!(
                    "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                    a.id,
                    a.captured().format("%Y-%m-%d %H:%M:%S"),
                    a.name,
                    photoorg_core::media::formats::label(a.formats),
                    idx.camera_name(a),
                    idx.lens_name(a),
                    fmt::focal(a.focal),
                    fmt::aperture(a.aperture),
                    fmt::shutter(a.shutter),
                    if a.iso > 0 { a.iso.to_string() } else { String::new() },
                );
            }
            eprintln!("{}개", hits.len());
        }
        "stats" => {
            let lib = Library::open(&PathBuf::from(rest.first().context("라이브러리 폴더가 필요합니다")?))?;
            let idx = lib.catalog()?.load_index()?;
            let fc = filter::facets(&idx, &Filter::default());
            println!("전체 {}개", fc.total);
            println!("\n[카메라]");
            for (c, n) in &fc.cameras {
                println!("  {:<32} {n}", idx.cameras.name(*c));
            }
            println!("\n[렌즈]");
            for (c, n) in &fc.lenses {
                println!("  {:<40} {n}", idx.lenses.name(*c));
            }
            println!("\n[ISO]");
            for (v, n) in &fc.isos {
                println!("  {v:<8} {n}");
            }
            println!("\n[별점]");
            for (r, n) in fc.ratings.iter().enumerate() {
                println!("  {}{} {n}", "★".repeat(r), "☆".repeat(5 - r));
            }
        }
        "rate" => {
            let lib = Library::open(&PathBuf::from(rest.first().context("라이브러리 폴더가 필요합니다")?))?;
            let rating: u8 = rest.get(1).context("별점이 필요합니다")?.parse()?;
            let ids: Vec<i64> = rest[2..].iter().map(|s| s.parse()).collect::<Result<_, _>>()?;
            let mut cat = lib.catalog()?;
            cat.set_rating(&ids, rating.min(5))?;
            for id in &ids {
                lib.write_xmp(&cat, *id, rating.min(5))?;
            }
            println!("{}개 사진에 별점 {} 적용", ids.len(), rating.min(5));
        }
        "export-csv" => {
            let lib = Library::open(&PathBuf::from(rest.first().context("라이브러리 폴더가 필요합니다")?))?;
            let out = PathBuf::from(rest.get(1).context("출력 파일이 필요합니다")?);
            let mut file = std::fs::File::create(&out)?;
            use std::io::Write;
            file.write_all("\u{feff}".as_bytes())?; // BOM so Excel reads UTF-8
            let n = lib.catalog()?.export_csv(std::io::BufWriter::new(file))?;
            println!("{n}개 항목을 {}에 저장했습니다", out.display());
        }
        "thumbs" => {
            let lib = Library::open(&PathBuf::from(rest.first().context("라이브러리 폴더가 필요합니다")?))?;
            let all = rest.iter().any(|a| a == "--all");
            let ids = lib.catalog()?.all_asset_ids()?;
            let have = lib.thumbs()?.ids()?;
            let todo: Vec<i64> = ids.into_iter().filter(|i| all || !have.contains(i)).collect();
            let total = todo.len();
            let n =
                import::generate_thumbnails(&lib, &todo, &AtomicBool::new(false), &|_, n| eprint!("\r{n}/{total}"))?;
            eprintln!();
            println!("썸네일 {n}개 생성");
        }
        "thumb" => {
            let src = PathBuf::from(rest.first().context("파일이 필요합니다")?);
            let out = PathBuf::from(rest.get(1).context("출력 파일이 필요합니다")?);
            let kind = FileKind::from_path(&src).context("지원하지 않는 형식")?;
            let p = metadata::probe(&src, kind)?;
            let t = std::time::Instant::now();
            let (w, h, data) = preview::make_thumbnail(&src, kind, p.meta.orientation, Some(&p.previews))?;
            std::fs::write(&out, data)?;
            println!("{w}x{h} ({:.1} ms)", t.elapsed().as_secs_f64() * 1000.0);
        }
        "-h" | "--help" | "help" => print!("{HELP}"),
        other => bail!("알 수 없는 명령 '{other}'\n\n{HELP}"),
    }
    Ok(())
}

fn crossbeam_channel_unbounded() -> (photoorg_core::import::EventSender, photoorg_core::import::EventReceiver) {
    photoorg_core::import::channel()
}

fn info(path: &std::path::Path) -> Result<()> {
    let kind = FileKind::from_path(path).context("지원하지 않는 형식")?;
    let t = std::time::Instant::now();
    let p = metadata::probe(path, kind)?;
    let el = t.elapsed();
    let m = &p.meta;
    println!("== {} ({:?}, {:.2} ms)", path.display(), kind, el.as_secs_f64() * 1000.0);
    println!("  카메라      {}", m.camera().unwrap_or_default());
    println!("  렌즈        {}", m.lens.clone().unwrap_or_default());
    println!(
        "  촬영 시각   {} ({:?}){}",
        m.captured_at.map(|d| d.to_string()).unwrap_or_default(),
        m.date_source,
        m.offset.as_deref().map(|o| format!(" {o}")).unwrap_or_default()
    );
    println!(
        "  노출        {}  {}  {}  {}  {}",
        m.iso.map(fmt::iso).unwrap_or_default(),
        m.focal_length.map(fmt::focal).unwrap_or_default(),
        m.aperture.map(fmt::aperture).unwrap_or_default(),
        m.exposure_time.map(fmt::shutter).unwrap_or_default(),
        m.exposure_bias.map(fmt::ev).unwrap_or_default()
    );
    println!("  35mm 환산   {}", m.focal_35mm.map(fmt::focal).unwrap_or_default());
    println!("  크기/방향   {:?}x{:?} / {}", m.width, m.height, m.orientation);
    if let Some(g) = m.gps {
        println!("  GPS         {:.6}, {:.6} {:?}", g.lat, g.lon, g.alt);
    }
    if let Some(r) = m.rating {
        println!("  별점        {r}");
    }
    if let Some(s) = &m.serial {
        println!("  시리얼      {s}");
    }
    let cands = preview::candidates(path, &p.previews);
    for c in &cands {
        println!(
            "  미리보기    {}x{} @ {} ({}) {:?}",
            c.width,
            c.height,
            c.jpeg.offset,
            fmt::bytes(c.jpeg.len),
            c.jpeg.source
        );
    }
    if cands.is_empty() && !p.previews.is_empty() {
        println!("  미리보기    (디코딩 가능한 JPEG 없음: {:?})", p.previews);
    }
    println!("  자동 태그   {}", photoorg_core::tags::auto_tags(m, kind.format_flag()).join(", "));
    Ok(())
}
