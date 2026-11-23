//! MIME type → preferred filename extension normalization (S11-i B8).
//!
//! Our table, compiled from the public IANA media-type registry plus a
//! small set of widely-served vendor aliases (`x-` tree entries that real
//! servers send). It is NOT copied from the reference product — the
//! S11 spec records that distinction; common entries such as
//! `video/mp2t` → `ts` are inherent to the media types themselves.
//!
//! The Rust table is the single source of truth:
//! `scripts/gen-mime-table.py` regenerates
//! `extensions/shared/mime-table.js` from it (for the extension side),
//! and a drift test enforces that the checked-in JS copy equals this
//! table.

/// MIME type → preferred extension. Multiple aliases may map to one
/// extension (e.g. the HLS playlist family all → `m3u8`). `application/
/// octet-stream` is deliberately absent: "unknown bytes" never earns a
/// suggestion (a guess would be worse than nothing).
const MIME_TABLE: &[(&str, &str)] = &[
    // media manifests
    ("application/vnd.apple.mpegurl", "m3u8"),
    ("application/x-mpegurl", "m3u8"),
    ("audio/mpegurl", "m3u8"),
    ("audio/x-mpegurl", "m3u8"),
    ("application/dash+xml", "mpd"),
    // video
    ("video/mp4", "mp4"),
    ("video/quicktime", "mov"),
    ("video/x-msvideo", "avi"),
    ("video/avi", "avi"),
    ("video/x-matroska", "mkv"),
    ("video/webm", "webm"),
    ("video/mpeg", "mpeg"),
    ("video/ogg", "ogv"),
    ("video/3gpp", "3gp"),
    ("video/3gpp2", "3g2"),
    ("video/mp2t", "ts"),
    ("video/iso.segment", "m4s"),
    ("video/x-flv", "flv"),
    ("video/x-ms-wmv", "wmv"),
    ("video/x-ms-asf", "asf"),
    ("video/jpeg2000", "jp2"),
    // audio
    ("audio/mpeg", "mp3"),
    ("audio/mp4", "m4a"),
    ("audio/aac", "aac"),
    ("audio/ogg", "oga"),
    ("audio/opus", "opus"),
    ("audio/flac", "flac"),
    ("audio/x-flac", "flac"),
    ("audio/wav", "wav"),
    ("audio/x-wav", "wav"),
    ("audio/vnd.wave", "wav"),
    ("audio/webm", "weba"),
    ("audio/basic", "au"),
    ("audio/midi", "mid"),
    ("audio/x-midi", "mid"),
    ("audio/amr", "amr"),
    // images
    ("image/jpeg", "jpg"),
    ("image/pjpeg", "jpg"),
    ("image/png", "png"),
    ("image/apng", "png"),
    ("image/gif", "gif"),
    ("image/webp", "webp"),
    ("image/avif", "avif"),
    ("image/heic", "heic"),
    ("image/heif", "heif"),
    ("image/bmp", "bmp"),
    ("image/x-ms-bmp", "bmp"),
    ("image/tiff", "tif"),
    ("image/svg+xml", "svg"),
    ("image/x-icon", "ico"),
    ("image/vnd.microsoft.icon", "ico"),
    ("image/jxl", "jxl"),
    ("image/jp2", "jp2"),
    // archives / compressed
    ("application/zip", "zip"),
    ("application/x-7z-compressed", "7z"),
    ("application/x-tar", "tar"),
    ("application/gzip", "gz"),
    ("application/x-gzip", "gz"),
    ("application/x-rar-compressed", "rar"),
    ("application/vnd.rar", "rar"),
    ("application/x-bzip2", "bz2"),
    ("application/x-xz", "xz"),
    ("application/zstd", "zst"),
    ("application/x-iso9660-image", "iso"),
    // documents
    ("application/pdf", "pdf"),
    ("application/msword", "doc"),
    (
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "docx",
    ),
    ("application/vnd.ms-excel", "xls"),
    (
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "xlsx",
    ),
    ("application/vnd.ms-powerpoint", "ppt"),
    (
        "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "pptx",
    ),
    ("application/vnd.oasis.opendocument.text", "odt"),
    ("application/vnd.oasis.opendocument.spreadsheet", "ods"),
    ("application/vnd.oasis.opendocument.presentation", "odp"),
    ("application/rtf", "rtf"),
    ("application/epub+zip", "epub"),
    ("application/x-mobipocket-ebook", "mobi"),
    ("application/vnd.amazon.ebook", "azw"),
    // text / code / data
    ("text/plain", "txt"),
    ("text/html", "html"),
    ("text/css", "css"),
    ("text/csv", "csv"),
    ("text/tab-separated-values", "tsv"),
    ("text/markdown", "md"),
    ("text/calendar", "ics"),
    ("text/vcard", "vcf"),
    ("text/vtt", "vtt"),
    ("text/xml", "xml"),
    ("application/xml", "xml"),
    ("application/json", "json"),
    ("application/manifest+json", "webmanifest"),
    ("application/javascript", "js"),
    ("text/javascript", "js"),
    ("application/ecmascript", "js"),
    ("application/wasm", "wasm"),
    ("application/x-sh", "sh"),
    // fonts
    ("font/woff", "woff"),
    ("font/woff2", "woff2"),
    ("application/font-woff", "woff"),
    ("font/ttf", "ttf"),
    ("application/font-sfnt", "ttf"),
    ("font/otf", "otf"),
    ("font/collection", "ttc"),
    ("application/vnd.ms-fontobject", "eot"),
    // packages / executables
    ("application/vnd.android.package-archive", "apk"),
    ("application/java-archive", "jar"),
    ("application/x-shockwave-flash", "swf"),
    ("application/vnd.microsoft.portable-executable", "exe"),
    ("application/postscript", "ps"),
    ("image/vnd.adobe.photoshop", "psd"),
];

/// The authoritative table (generator + drift-test accessor).
pub fn mime_table() -> &'static [(&'static str, &'static str)] {
    MIME_TABLE
}

/// Preferred extension for a MIME type: parameters (`; charset=…`) and case
/// are normalized away; unknown (or parameter-only/empty) types yield
/// `None` — never a guess.
pub fn extension_for_mime(mime: &str) -> Option<&'static str> {
    let bare = mime
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    if bare.is_empty() {
        return None;
    }
    MIME_TABLE.iter().find(|(m, _)| *m == bare).map(|(_, e)| *e)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_normalizes_case_and_parameters() {
        assert_eq!(extension_for_mime("video/mp2t"), Some("ts"));
        assert_eq!(extension_for_mime("Application/PDF"), Some("pdf"));
        assert_eq!(
            extension_for_mime("application/pdf; charset=binary"),
            Some("pdf")
        );
        assert_eq!(extension_for_mime("  audio/mp4 ;  x=y "), Some("m4a"));
        // the HLS playlist family: aliases collapse to one extension
        assert_eq!(
            extension_for_mime("application/vnd.apple.mpegurl"),
            Some("m3u8")
        );
        assert_eq!(extension_for_mime("audio/mpegurl"), Some("m3u8"));
    }

    #[test]
    fn unknown_mime_yields_no_suggestion() {
        assert_eq!(extension_for_mime("application/x-nope-unknown"), None);
        assert_eq!(extension_for_mime("application/octet-stream"), None);
        assert_eq!(extension_for_mime(""), None);
        assert_eq!(extension_for_mime("; charset=utf-8"), None);
        assert_eq!(extension_for_mime("not-a-mime"), None);
    }

    #[test]
    fn table_shape() {
        let t = mime_table();
        assert!(t.len() > 90, "expected ~100 entries, got {}", t.len());
        // every entry is lowercase + a bare extension (invariants the
        // generator and drift test rely on)
        for (m, e) in t.iter() {
            assert_eq!(*m, m.to_ascii_lowercase(), "{:?} not lowercase", m);
            assert!(!e.contains('/') && !e.contains(';'), "bad ext {:?}", e);
        }
    }

    /// Drift tripwire (the `md3-tokens-baseline.json` precedent): the
    /// checked-in `extensions/shared/mime-table.js` must equal this table.
    /// Skipped only in trees with no extension side (the win7 payload).
    #[test]
    fn generated_js_table_matches_rust_table() {
        let js = std::path::Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../extensions/shared/mime-table.js"
        ));
        let text = match std::fs::read_to_string(js) {
            Ok(t) => t,
            Err(_) => return, // no extension side beside this tree
        };
        let line = text
            .lines()
            .find(|l| l.trim_start().starts_with("var MIME_TABLE = "))
            .expect("mime-table.js has no MIME_TABLE definition");
        let json_part = line
            .trim()
            .trim_start_matches("var MIME_TABLE = ")
            .trim_end_matches(';');
        let obj: serde_json::Map<String, serde_json::Value> =
            serde_json::from_str(json_part).expect("MIME_TABLE is not valid JSON");
        let table: Vec<(&str, &str)> = MIME_TABLE.to_vec();
        assert_eq!(
            obj.len(),
            table.len(),
            "JS/Rust MIME table size drift — regenerate with scripts/gen-mime-table.py"
        );
        for (m, e) in table {
            assert_eq!(
                obj.get(m).and_then(|v| v.as_str()),
                Some(e),
                "entry {:?} drifted — regenerate with scripts/gen-mime-table.py",
                m
            );
        }
    }
}
