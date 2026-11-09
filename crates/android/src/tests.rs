use super::*;
use photoorg_core::image::RgbImage;
use std::os::fd::IntoRawFd;

/// Opens listed files by their real path, like the app does with document URIs.
struct PathOpener;

impl FileOpener for PathOpener {
    fn open(&self, id: String) -> i32 {
        File::open(id).map(|f| f.into_raw_fd()).unwrap_or(-1)
    }
}

struct Collect(Mutex<Vec<ImportStatus>>);

impl ImportListener for Arc<Collect> {
    fn progress(&self, status: ImportStatus) {
        self.0.lock().unwrap().push(status);
    }
}

struct Count(Mutex<(i32, i32)>);

impl ProgressListener for Arc<Count> {
    fn progress(&self, done: i32, total: i32) {
        *self.0.lock().unwrap() = (done, total);
    }
}

fn photo_jpeg(seed: u32, sharp: bool) -> Vec<u8> {
    let img = RgbImage::from_fn(800, 600, |x, y| {
        let v = if sharp {
            if ((x / 7) + (y / 7) + seed).is_multiple_of(2) {
                230
            } else {
                20
            }
        } else {
            ((x + y + seed * 13) / 6 % 256) as u8
        };
        photoorg_core::image::Rgb([v, v.wrapping_add(seed as u8 * 40), 255 - v])
    });
    preview::encode_jpeg(&img, 90).unwrap()
}

/// A "card" with three photos, listed the way the app lists a document tree.
fn card() -> (tempfile::TempDir, Vec<SourceEntry>) {
    let dir = tempfile::tempdir().unwrap();
    let dcim = dir.path().join("DCIM/100TEST");
    std::fs::create_dir_all(&dcim).unwrap();
    let mut entries = Vec::new();
    for (i, sharp) in [(1u32, true), (2, false), (3, true)] {
        let p = dcim.join(format!("IMG_000{i}.JPG"));
        std::fs::write(&p, photo_jpeg(i, sharp)).unwrap();
        entries.push(SourceEntry {
            rel: format!("DCIM/100TEST/IMG_000{i}.JPG"),
            size: std::fs::metadata(&p).unwrap().len() as i64,
            mtime_ms: Some(1_780_000_000_000 + i as i64 * 60_000),
            id: p.display().to_string(),
        });
    }
    std::fs::write(dir.path().join("DCIM/100TEST/notes.txt"), b"not a photo").unwrap();
    entries.push(SourceEntry {
        rel: "DCIM/100TEST/notes.txt".into(),
        size: 11,
        mtime_ms: None,
        id: dir.path().join("DCIM/100TEST/notes.txt").display().to_string(),
    });
    (dir, entries)
}

#[test]
fn import_rate_analyse_and_delete_from_a_listed_card() {
    let (_card, entries) = card();
    let libdir = tempfile::tempdir().unwrap();
    let lib = PhotoLibrary::open(libdir.path().join("Photos").display().to_string()).unwrap();
    assert_eq!(lib.counts(), Counts::default());

    // Scan: three new photos; the text file is ignored.
    let info = lib.scan("EOS_DIGITAL".into(), entries.clone(), Box::new(PathOpener), true).unwrap();
    assert_eq!((info.photos, info.new_photos, info.new_files), (3, 3, 3));
    assert!(info.new_bytes > 0);
    assert!(info.first_ms.is_some() && info.first_ms < info.last_ms);

    // Import, with progress.
    let seen = Arc::new(Collect(Mutex::new(Vec::new())));
    let res = lib.import_scanned(Box::new(seen.clone())).unwrap();
    assert_eq!((res.added, res.duplicates, res.cancelled), (3, 0, false), "{:?}", res.failures);
    let last = seen.0.lock().unwrap().last().cloned().unwrap();
    assert_eq!((last.files_done, last.files_total), (3, 3));
    let photos = lib.photos(View::All);
    assert_eq!(photos.len(), 3);
    assert!(photos.windows(2).all(|w| w[0].captured_ms <= w[1].captured_ms), "in shooting order");
    assert!(photos.iter().all(|p| p.jpeg && !p.raw && p.b_pct.is_none()));
    let day = libdir.path().join("Photos").join("2026");
    assert!(day.is_dir(), "copied into dated folders");
    for p in &photos {
        let t = lib.thumbnail(p.id).expect("thumbnail made on import");
        assert!(t.starts_with(&[0xFF, 0xD8]));
    }
    let big = lib.preview(photos[0].id, 400).unwrap();
    let img = photoorg_core::image::load_from_memory(&big).unwrap();
    assert_eq!(img.width().max(img.height()), 400);

    // The same card again: nothing new.
    let again = lib.scan("EOS_DIGITAL".into(), entries, Box::new(PathOpener), true).unwrap();
    assert_eq!(again.new_photos, 0);

    // Decisions.
    lib.set_rating(vec![photos[0].id], 4).unwrap();
    lib.set_flag(vec![photos[1].id], -1).unwrap();
    let c = lib.counts();
    assert_eq!((c.rated, c.rejected, c.undecided), (1, 1, 1));
    assert_eq!(lib.photos(View::Rejected)[0].id, photos[1].id);
    assert_eq!(lib.photo(photos[0].id).unwrap().rating, 4);
    let xmp = photoorg_core::xmp::sidecar_path(
        &libdir.path().join("Photos").join(lib.state().catalog.files_for(photos[0].id).unwrap()[0].rel_path.clone()),
    );
    assert!(std::fs::read_to_string(xmp).unwrap().contains("xmp:Rating=\"4\""), "rating in the sidecar");

    // The culling assistant looks at every photo, then scores them.
    assert_eq!(lib.pending_analysis(), 3);
    let count = Arc::new(Count(Mutex::new((0, 0))));
    assert_eq!(lib.analyze(Box::new(count.clone())).unwrap(), 3);
    assert_eq!(*count.0.lock().unwrap(), (3, 3));
    assert_eq!(lib.pending_analysis(), 0);
    let c = lib.counts();
    assert_eq!(c.analysed, 3);
    assert!(lib.photos(View::All).iter().all(|p| p.b_pct.is_some() && !p.scene.is_empty()));
    let status = lib.learn_status();
    assert_eq!((status.analysed, status.total), (3, 3));
    assert!(status.summary.starts_with("기본 기준"), "{}", status.summary);

    // Delete the rejected photo: gone from the library (to its trash folder), its decision kept.
    let plan = lib.delete_plan(vec![photos[1].id]);
    assert_eq!((plan.photos, plan.files), (1, 2), "the JPEG and its sidecar");
    let del = lib.delete(vec![photos[1].id]).unwrap();
    assert_eq!((del.deleted, del.failed), (1, 0), "{:?}", del.first_error);
    assert_eq!(lib.counts().all, 2);
    assert!(lib.photo(photos[1].id).is_none());
    assert!(lib.trash_bytes() > 0, "in the library's trash");
    lib.empty_trash().unwrap();
    assert_eq!(lib.trash_bytes(), 0);
}

#[test]
fn reasons_and_views_follow_the_threshold() {
    let mut index = LibraryIndex::new();
    let mut a = AssetRecord {
        id: 1,
        captured_ms: 0,
        camera: index.cameras.intern(""),
        lens: index.lenses.intern(""),
        focal: 0.0,
        focal35: f32::NAN,
        aperture: 0.0,
        shutter: 0.0,
        iso: 0,
        bias: 0.0,
        width: 0,
        height: 0,
        orientation: 1,
        rating: 0,
        formats: formats::JPEG,
        flash: false,
        has_gps: false,
        date_from_file: false,
        import_id: 1,
        tags: vec![],
        name: "a".into(),
        flag: 0,
        scene: 0,
        b_prob: 0.7,
        reasons: reason::BLUR | reason::EYES,
        burst: 0,
        burst_rank: 0,
        burst_size: 0,
        keep_rank: f32::NAN,
    };
    index.assets.push(a.clone());
    let p = photo(&index, &a);
    assert_eq!(p.b_pct, Some(70));
    assert_eq!(p.reasons, ["초점 흐림", "눈 감음 의심"]);
    let mut s = Settings::default();
    assert!(s.matches(View::Candidates, &a));
    s.threshold = 80;
    assert!(!s.matches(View::Candidates, &a));
    assert!(s.matches(View::Undecided, &a));
    a.rating = 2;
    assert!(!s.matches(View::Undecided, &a), "stars count as a decision");
    assert!(s.matches(View::Rated, &a));
}
