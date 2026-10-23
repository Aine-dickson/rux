//! The `File` value: a file the person chose, as `<input type="file">` binds
//! it and a script reads it (`docs/12-file-input-proposal.md`).
//!
//! A `File` is a record of the built-in module, `name`, `size`, `type`, `url`
//! and `path`, so it crosses signals and props as any record does, and its
//! one method, `text()`, is found by the checker through its type. What it
//! reads is named by the record itself: `path` on the desktop.

use rux_runtime::native::{Any, Call, Error};

/// The calls behind `File`'s methods, keyed as the module's calls are.
pub(crate) fn calls() -> Vec<(String, Call)> {
    vec![("File.text".to_string(), Call::future(|args: Vec<Any>| text(args.into_iter().next().unwrap_or_default())))]
}

/// `await file.text()`: the whole file, as UTF-8, with what is not UTF-8
/// replaced, as a browser's `File.text()` does. An async native runs on a
/// thread of its own, so the read here does not stall a frame.
async fn text(file: Any) -> Result<Any, Error> {
    let bytes = read(&file)?;
    Ok(Any::Str(String::from_utf8_lossy(&bytes).into_owned()))
}

#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
fn read(file: &Any) -> Result<Vec<u8>, Error> {
    let Any::Map(m) = file else { return Err(Error::new("type", "`text` is a method of a `File`")) };
    let Some(Any::Str(path)) = m.get("path") else {
        return Err(Error::new("type", "this `File` has no `path` to read"));
    };
    std::fs::read(path).map_err(|e| Error::new("io", format!("cannot read {path}: {e}")))
}

#[cfg(any(target_arch = "wasm32", target_os = "android"))]
fn read(_file: &Any) -> Result<Vec<u8>, Error> {
    let here = if cfg!(target_os = "android") { "on Android" } else { "on the web" };
    Err(Error::new("unsupported", format!("reading a `File` is not built {here} yet")))
}

/// The `File` for the file at `path`, or `None` when it cannot be looked at
/// (gone between the dialog closing and now).
#[cfg_attr(any(target_arch = "wasm32", target_os = "android"), allow(dead_code))]
pub(crate) fn from_path(path: &std::path::Path) -> Option<Any> {
    let size = std::fs::metadata(path).ok()?.len();
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    let mut m = std::collections::BTreeMap::new();
    m.insert("name".to_string(), Any::Str(name));
    m.insert("size".to_string(), Any::Int(size as i64));
    m.insert("type".to_string(), Any::Str(media_type(&ext).unwrap_or("").to_string()));
    // Numbered, so the painter, which keeps an image by its `src`, decodes
    // a file picked again afresh: it may have changed in between.
    static PICKS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let pick = PICKS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    m.insert("url".to_string(), Any::Str(format!("{}#{pick}", file_url(path))));
    m.insert("path".to_string(), Any::Str(path.to_string_lossy().into_owned()));
    Some(Any::Map(m))
}

/// `file:///C:/a%20b.png` for `C:\a b.png`: a URL, with the separators
/// forward and what a URL cannot hold escaped.
fn file_url(path: &std::path::Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    let mut out = String::from(if text.starts_with('/') { "file://" } else { "file:///" });
    for b in text.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b':' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// The media type of a file by its extension, lowercase and without the
/// dot: what `File.type` says, and, read backwards, which extensions an
/// `accept` of `image/*` or `application/pdf` offers on a desktop picker.
/// The common ones only; anything else is `""`, as a browser says too.
const MEDIA_TYPES: &[(&str, &str)] = &[
    ("png", "image/png"),
    ("jpg", "image/jpeg"),
    ("jpeg", "image/jpeg"),
    ("gif", "image/gif"),
    ("webp", "image/webp"),
    ("avif", "image/avif"),
    ("bmp", "image/bmp"),
    ("svg", "image/svg+xml"),
    ("ico", "image/x-icon"),
    ("heic", "image/heic"),
    ("mp4", "video/mp4"),
    ("m4v", "video/mp4"),
    ("mov", "video/quicktime"),
    ("webm", "video/webm"),
    ("mkv", "video/x-matroska"),
    ("avi", "video/x-msvideo"),
    ("mp3", "audio/mpeg"),
    ("m4a", "audio/mp4"),
    ("aac", "audio/aac"),
    ("wav", "audio/wav"),
    ("ogg", "audio/ogg"),
    ("oga", "audio/ogg"),
    ("opus", "audio/opus"),
    ("flac", "audio/flac"),
    ("txt", "text/plain"),
    ("csv", "text/csv"),
    ("md", "text/markdown"),
    ("html", "text/html"),
    ("css", "text/css"),
    ("js", "text/javascript"),
    ("json", "application/json"),
    ("xml", "application/xml"),
    ("pdf", "application/pdf"),
    ("zip", "application/zip"),
    ("doc", "application/msword"),
    ("docx", "application/vnd.openxmlformats-officedocument.wordprocessingml.document"),
    ("xls", "application/vnd.ms-excel"),
    ("xlsx", "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"),
    ("ppt", "application/vnd.ms-powerpoint"),
    ("pptx", "application/vnd.openxmlformats-officedocument.presentationml.presentation"),
];

fn media_type(ext: &str) -> Option<&'static str> {
    MEDIA_TYPES.iter().find(|(e, _)| *e == ext).map(|(_, t)| *t)
}

/// The extensions an `accept` list offers, for a picker that filters by
/// extension: `.png` as written, `application/pdf` by the table,
/// `image/*` as every image in it. Empty when nothing in it is known, which
/// a caller reads as "no filter" rather than "nothing may be picked".
#[cfg_attr(any(target_arch = "wasm32", target_os = "android"), allow(dead_code))]
pub(crate) fn accept_extensions(accept: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut add = |e: &str| {
        if !out.iter().any(|o| o == e) {
            out.push(e.to_string());
        }
    };
    for part in accept.split(',').map(|p| p.trim().to_lowercase()).filter(|p| !p.is_empty()) {
        if let Some(ext) = part.strip_prefix('.') {
            add(ext);
        } else if let Some(family) = part.strip_suffix("/*") {
            MEDIA_TYPES.iter().filter(|(_, t)| t.split('/').next() == Some(family)).for_each(|(e, _)| add(e));
        } else {
            MEDIA_TYPES.iter().filter(|(_, t)| *t == part).for_each(|(e, _)| add(e));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accept_reads_extensions_types_and_families() {
        assert_eq!(accept_extensions(".PNG, application/pdf, image/png"), ["png", "pdf"]);
        let images = accept_extensions("image/*");
        assert!(images.iter().any(|e| e == "jpeg") && images.iter().all(|e| media_type(e).unwrap().starts_with("image/")));
        assert!(accept_extensions("x-unknown/thing").is_empty());
    }

    #[test]
    fn a_path_becomes_a_file_url() {
        assert_eq!(file_url(std::path::Path::new(r"C:\a b\ü.png")), "file:///C:/a%20b/%C3%BC.png");
        assert_eq!(file_url(std::path::Path::new("/home/x.png")), "file:///home/x.png");
    }

    #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
    #[test]
    fn a_file_is_described_and_read() {
        let dir = std::env::temp_dir().join(format!("rux-file-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("note.TXT");
        std::fs::write(&path, b"caf\xc3\xa9 \xff").unwrap();
        let file = from_path(&path).unwrap();
        let Any::Map(m) = &file else { panic!("a record") };
        assert_eq!(m["name"], Any::Str("note.TXT".into()));
        assert_eq!(m["size"], Any::Int(7));
        assert_eq!(m["type"], Any::Str("text/plain".into()));
        assert!(matches!(&m["url"], Any::Str(u) if u.starts_with("file:///") && u.contains("/note.TXT#")));
        let read = rux_runtime::native::block_on(text(file)).unwrap();
        assert_eq!(read, Any::Str("café \u{fffd}".into()));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
