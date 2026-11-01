//! Use the platform's UI font (SF Pro / Segoe UI / system sans) plus a Korean font, found through
//! the OS font database, so text looks native. A `bold` family is always registered.

use eframe::egui::{self, FontData, FontDefinitions, FontFamily};
use fontique::{
    Attributes, Collection, CollectionOptions, FallbackKey, FontStyle, FontWeight, FontWidth, GenericFamily, Language,
    QueryFamily, QueryStatus, Script, SourceCache,
};
use std::sync::Arc;

pub const BOLD: &str = "bold";

struct Found {
    blob: fontique::Blob<u8>,
    index: u32,
}

fn find(
    collection: &mut Collection,
    cache: &mut SourceCache,
    family: QueryFamily<'_>,
    weight: FontWeight,
    fallback: Option<FallbackKey>,
    probe: char,
    require_static: bool,
) -> Option<Found> {
    let mut query = collection.query(cache);
    query.set_families([family]);
    query.set_attributes(Attributes::new(FontWidth::NORMAL, FontStyle::Normal, weight));
    if let Some(fb) = fallback {
        query.set_fallbacks(fb);
    }
    let mut out = None;
    query.matches_with(|font| {
        let has = font.charmap().and_then(|c| c.map(probe)).is_some_and(|g| g != 0);
        let ok_weight =
            !require_static || (!font.synthesis.embolden() && font.synthesis.variation_settings().is_empty());
        if has && ok_weight {
            out = Some(Found { blob: font.blob.clone(), index: font.index });
            QueryStatus::Stop
        } else {
            QueryStatus::Continue
        }
    });
    out
}

fn data(f: Found) -> Arc<FontData> {
    Arc::new(FontData::from_blob(Arc::new(f.blob), f.index))
}

pub fn install(ctx: &egui::Context) {
    let mut defs = FontDefinitions::default();
    let mut collection = Collection::new(CollectionOptions { shared: false, system_fonts: true });
    let mut cache = SourceCache::new(Default::default());
    let ko = Language::parse("ko").ok();
    let hang = || FallbackKey::new(Script::from_bytes(*b"Hang"), ko.as_ref());

    let ui_regular = find(
        &mut collection,
        &mut cache,
        QueryFamily::Generic(GenericFamily::SystemUi),
        FontWeight::NORMAL,
        None,
        'a',
        false,
    )
    .or_else(|| {
        find(
            &mut collection,
            &mut cache,
            QueryFamily::Generic(GenericFamily::SansSerif),
            FontWeight::NORMAL,
            None,
            'a',
            false,
        )
    });
    let ko_regular = find(
        &mut collection,
        &mut cache,
        QueryFamily::Generic(GenericFamily::SansSerif),
        FontWeight::NORMAL,
        Some(hang()),
        '한',
        false,
    );
    let ko_bold = find(
        &mut collection,
        &mut cache,
        QueryFamily::Generic(GenericFamily::SansSerif),
        FontWeight::BOLD,
        Some(hang()),
        '한',
        true,
    );
    let ui_bold = find(
        &mut collection,
        &mut cache,
        QueryFamily::Generic(GenericFamily::SystemUi),
        FontWeight::BOLD,
        None,
        'a',
        true,
    )
    .or_else(|| {
        find(
            &mut collection,
            &mut cache,
            QueryFamily::Generic(GenericFamily::SansSerif),
            FontWeight::BOLD,
            None,
            'a',
            true,
        )
    });

    let mut prop: Vec<String> = Vec::new();
    let mut bold: Vec<String> = Vec::new();
    if let Some(f) = ui_regular {
        defs.font_data.insert("ui".into(), data(f));
        prop.push("ui".into());
    }
    if let Some(f) = ko_regular {
        defs.font_data.insert("ko".into(), data(f));
        prop.push("ko".into());
    }
    if let Some(f) = ui_bold {
        defs.font_data.insert("ui-bold".into(), data(f));
        bold.push("ui-bold".into());
    }
    if let Some(f) = ko_bold {
        defs.font_data.insert("ko-bold".into(), data(f));
        bold.push("ko-bold".into());
    }
    let default_prop = defs.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    // Our fonts first, egui defaults (incl. emoji) as fallback.
    let mut p = prop.clone();
    p.extend(default_prop.iter().cloned());
    defs.families.insert(FontFamily::Proportional, p.clone());
    let mut b = bold;
    b.extend(p);
    defs.families.insert(FontFamily::Name(BOLD.into()), b);
    ctx.set_fonts(defs);
}
