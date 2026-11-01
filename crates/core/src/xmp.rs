//! XMP sidecar files (`IMG_0001.xmp`) so ratings and keywords are readable by Lightroom,
//! Capture One, Bridge, darktable, digiKam, ...
//!
//! Sidecars that were created by another application are never rewritten wholesale: only the
//! `xmp:Rating` value is updated in place so develop settings stored there are preserved.

use anyhow::Result;
use std::path::{Path, PathBuf};

const MARKER: &str = "xmlns:photoorg=\"https://github.com/MJ-best/exif-photo-organize/ns/1.0/\"";

pub fn sidecar_path(file: &Path) -> PathBuf {
    file.with_extension("xmp")
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn fresh(rating: u8, keywords: &[String]) -> String {
    let mut kw = String::new();
    if !keywords.is_empty() {
        kw.push_str("   <dc:subject>\n    <rdf:Bag>\n");
        for k in keywords {
            kw.push_str(&format!("     <rdf:li>{}</rdf:li>\n", esc(k)));
        }
        kw.push_str("    </rdf:Bag>\n   </dc:subject>\n");
        kw.push_str("   <lr:hierarchicalSubject>\n    <rdf:Bag>\n");
        for k in keywords {
            kw.push_str(&format!("     <rdf:li>{}</rdf:li>\n", esc(&k.replacen(':', "|", 1))));
        }
        kw.push_str("    </rdf:Bag>\n   </lr:hierarchicalSubject>\n");
    }
    let mut out = String::new();
    out.push_str("<?xpacket begin=\"\u{feff}\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?>\n");
    out.push_str("<x:xmpmeta xmlns:x=\"adobe:ns:meta/\" x:xmptk=\"Photo Organize\">\n");
    out.push_str(" <rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\n");
    out.push_str("  <rdf:Description rdf:about=\"\"\n");
    out.push_str("    xmlns:xmp=\"http://ns.adobe.com/xap/1.0/\"\n");
    out.push_str("    xmlns:dc=\"http://purl.org/dc/elements/1.1/\"\n");
    out.push_str("    xmlns:lr=\"http://ns.adobe.com/lightroom/1.0/\"\n");
    out.push_str(&format!("    {MARKER}\n"));
    out.push_str(&format!("   xmp:Rating=\"{rating}\">\n"));
    out.push_str(&kw);
    out.push_str("  </rdf:Description>\n </rdf:RDF>\n</x:xmpmeta>\n<?xpacket end=\"w\"?>\n");
    out
}

/// Replace or insert `xmp:Rating` in a foreign XMP document.
fn patch_rating(doc: &str, rating: u8) -> Option<String> {
    if let Some(i) = doc.find("xmp:Rating=\"") {
        let start = i + "xmp:Rating=\"".len();
        let end = start + doc[start..].find('"')?;
        return Some(format!("{}{}{}", &doc[..start], rating, &doc[end..]));
    }
    if let Some(i) = doc.find("<xmp:Rating>") {
        let start = i + "<xmp:Rating>".len();
        let end = start + doc[start..].find("</xmp:Rating>")?;
        return Some(format!("{}{}{}", &doc[..start], rating, &doc[end..]));
    }
    let i = doc.find("<rdf:Description")?;
    let insert_at = i + "<rdf:Description".len();
    let ns = if doc.contains("xmlns:xmp=") { "" } else { " xmlns:xmp=\"http://ns.adobe.com/xap/1.0/\"" };
    Some(format!("{}{} xmp:Rating=\"{}\"{}", &doc[..insert_at], ns, rating, &doc[insert_at..]))
}

/// Write (or update) the sidecar for `file`.
pub fn write_sidecar(file: &Path, rating: u8, keywords: &[String]) -> Result<()> {
    let path = sidecar_path(file);
    let content = match std::fs::read_to_string(&path) {
        Ok(existing) if !existing.contains("xmlns:photoorg=") => match patch_rating(&existing, rating) {
            Some(s) => s,
            None => return Ok(()), // unknown structure: leave it alone
        },
        _ => fresh(rating, keywords),
    };
    let tmp = path.with_extension("xmp.tmp");
    std::fs::write(&tmp, content)?;
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

/// Rating from an existing sidecar, if any.
pub fn read_sidecar_rating(file: &Path) -> Option<u8> {
    let s = std::fs::read(sidecar_path(file)).ok()?;
    crate::metadata::xmp_rating(&s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let raw = dir.path().join("IMG_0001.CR3");
        write_sidecar(&raw, 4, &["카메라:Canon EOS R5".into(), "<x>".into()]).unwrap();
        assert_eq!(read_sidecar_rating(&raw), Some(4));
        let txt = std::fs::read_to_string(dir.path().join("IMG_0001.xmp")).unwrap();
        assert!(txt.contains("&lt;x&gt;"));
        assert!(txt.contains("카메라|Canon EOS R5"));
        write_sidecar(&raw, 2, &[]).unwrap();
        assert_eq!(read_sidecar_rating(&raw), Some(2));
    }

    #[test]
    fn foreign_sidecar_preserved() {
        let dir = tempfile::tempdir().unwrap();
        let raw = dir.path().join("DSC_1.NEF");
        let foreign = "<x:xmpmeta><rdf:RDF><rdf:Description rdf:about=\"\" xmlns:crs=\"x\" crs:Exposure2012=\"+0.50\"/></rdf:RDF></x:xmpmeta>";
        std::fs::write(dir.path().join("DSC_1.xmp"), foreign).unwrap();
        write_sidecar(&raw, 5, &["a".into()]).unwrap();
        let txt = std::fs::read_to_string(dir.path().join("DSC_1.xmp")).unwrap();
        assert!(txt.contains("crs:Exposure2012=\"+0.50\""));
        assert_eq!(read_sidecar_rating(&raw), Some(5));
        write_sidecar(&raw, 1, &[]).unwrap();
        assert_eq!(read_sidecar_rating(&raw), Some(1));
    }
}
