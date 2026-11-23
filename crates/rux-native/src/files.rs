//! A file the person chose, read from the app's own Rust.
//!
//! A script holds a [`File`] (`name`, `size`, `type`, `url` and, on the
//! desktop, `path`) and passes it on to an export that takes `rux::File`.
//! The bytes never cross to script: an upload reads them here and sends them
//! itself.
//!
//! ```ignore
//! #[rux::export]
//! pub async fn upload(file: rux::File) -> Result<(), ApiError> {
//!     let bytes = file.bytes().await?;
//!     client.post(url).body(bytes).send().await?;
//!     Ok(())
//! }
//! ```
//!
//! [`File::bytes`] reads whatever the platform gave: the path on the desktop,
//! the `content://` URI through Android's `ContentResolver`, the browser's own
//! `File` on the web. The shell installs the reader at start-up with
//! [`set_reader`], so a read fails with `unavailable` in a program that never
//! started one.

use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::OnceLock;

use crate::{Any, Error, FromRux, FromRuxRef, IntoRux};

/// What the person chose, as `<input type="file">` binds it and `pickFile`
/// gives it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct File {
    pub name: String,
    /// Bytes, as the platform said when it was chosen.
    pub size: u64,
    /// The media type, `""` when it is not known.
    pub media_type: String,
    /// What the painter and the platform read it by: `file:///...`, a
    /// `content://` URI or a `blob:` URL.
    pub url: String,
    /// The desktop's alone: a browser and Android never tell a page one.
    pub path: Option<String>,
}

type Bytes = Pin<Box<dyn Future<Output = Result<Vec<u8>, Error>> + Send + 'static>>;

/// Reads a file's bytes. The shell installs one.
pub type Reader = fn(File) -> Bytes;

static READER: OnceLock<Reader> = OnceLock::new();

/// Install the reader. The first one wins, as the shell may start twice.
pub fn set_reader(reader: Reader) {
    let _ = READER.set(reader);
}

impl File {
    /// The whole file. A file that has gone since it was chosen, or that the
    /// platform will not open, is an `io` error.
    pub async fn bytes(&self) -> Result<Vec<u8>, Error> {
        let read = READER
            .get()
            .ok_or_else(|| Error::new("unavailable", "no file reader is installed: the shell has not started"))?;
        read(self.clone()).await
    }
}

impl FromRux for File {
    fn from_rux(v: Any) -> Result<File, Error> {
        let Any::Map(mut m) = v else { return Err(Error::wrong("a File", &v)) };
        let mut text = |name: &str| match m.remove(name) {
            Some(Any::Str(s)) => Ok(s),
            Some(other) => Err(Error::new("type", format!("a File's `{name}` is a string, not a {}", other.kind()))),
            None => Err(Error::new("type", format!("a File has a `{name}`"))),
        };
        let name = text("name")?;
        let media_type = text("type")?;
        let url = text("url")?;
        let size = match m.remove("size") {
            Some(Any::Int(n)) if n >= 0 => n as u64,
            Some(other) => return Err(Error::new("type", format!("a File's `size` is a whole number, not a {}", other.kind()))),
            None => return Err(Error::new("type", "a File has a `size`")),
        };
        let path = match m.remove("path") {
            Some(Any::Str(p)) => Some(p),
            Some(Any::None) | None => None,
            Some(other) => return Err(Error::new("type", format!("a File's `path` is a string, not a {}", other.kind()))),
        };
        Ok(File { name, size, media_type, url, path })
    }
}

impl IntoRux for File {
    fn into_rux(self) -> Result<Any, Error> {
        let mut m = BTreeMap::new();
        m.insert("name".to_string(), Any::Str(self.name));
        m.insert("size".to_string(), Any::Int(i64::try_from(self.size).unwrap_or(i64::MAX)));
        m.insert("type".to_string(), Any::Str(self.media_type));
        m.insert("url".to_string(), Any::Str(self.url));
        m.insert("path".to_string(), self.path.map_or(Any::None, Any::Str));
        Ok(Any::Map(m))
    }
}

impl FromRuxRef for File {
    type Holder = Box<File>;
    fn hold(v: Any) -> Result<Box<File>, Error> {
        File::from_rux(v).map(Box::new)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picked() -> File {
        File { name: "id.png".into(), size: 12, media_type: "image/png".into(), url: "file:///id.png#0".into(), path: Some("/id.png".into()) }
    }

    #[test]
    fn a_file_crosses_as_the_record_script_holds() {
        let any = picked().into_rux().unwrap();
        let Any::Map(m) = &any else { panic!("a record") };
        assert_eq!(m["type"], Any::Str("image/png".into()));
        assert_eq!(File::from_rux(any).unwrap(), picked());
        let mut no_path = picked();
        no_path.path = None;
        assert_eq!(File::from_rux(no_path.clone().into_rux().unwrap()).unwrap(), no_path);
    }

    #[test]
    fn what_is_not_a_file_is_refused_by_name() {
        assert_eq!(File::from_rux(Any::Str("x".into())).unwrap_err().kind, "type");
        let mut m = BTreeMap::new();
        m.insert("name".to_string(), Any::Str("a".into()));
        let e = File::from_rux(Any::Map(m)).unwrap_err();
        assert!(e.message.contains("`type`"), "{}", e.message);
    }

    #[test]
    fn a_read_with_no_reader_is_unavailable() {
        // Only meaningful while no test installs one: this crate's tests never do.
        let e = crate::block_on(picked().bytes()).unwrap_err();
        assert_eq!(e.kind, "unavailable");
    }
}
