//! Where a document's bytes come from.
//!
//! Loading a `.rux` file means reading several files, not one: the document, the
//! components it `use`s, the stylesheets it includes, and the images it names.
//! Every one of those was `std::fs` called directly, which is correct on a
//! desktop and impossible anywhere else.
//!
//! Two targets already need it to be something else. A browser has no
//! filesystem at all, which the web shell works around by accepting one source
//! string with no import graph behind it. An Android app's documents live inside
//! the APK, reachable only through `AssetManager`, and a release build embeds
//! them in the binary with no paths involved at any point.
//!
//! So a path stops being a filesystem location and becomes a *key*: a name the
//! provider resolves however its target resolves names. The filesystem provider
//! joins it and reads it, and is what every desktop run uses. The in-memory
//! provider looks it up in a map, which is what a browser, an embedded release
//! build and a test all want.
//!
//! **The provider is thread-local, not a parameter.** Threading it through would
//! touch `Document::load`, every caller, and every one of the hundred-odd tests
//! that load a file, to express something no caller ever varies within a run.
//! `rux_script::set_is_fragment` already sets load-time state this way, and
//! thread-local rather than global is what keeps `cargo test`'s parallel threads
//! from racing each other: each test thread gets its own, and the default is the
//! filesystem, so a test that does not care never knows this module exists.

use std::cell::RefCell;
use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::rc::Rc;

/// How the runtime reaches the files a document is made of.
///
/// Implementations resolve a path however their target does. Nothing here may
/// assume a filesystem exists.
pub trait Source {
    /// Read a text file: a document, a component, a stylesheet.
    fn read_text(&self, path: &Path) -> io::Result<String>;

    /// Read a binary file. Only images today, and only their headers are
    /// actually parsed by the runtime; the painter decodes the pixels.
    fn read_bytes(&self, path: &Path) -> io::Result<Vec<u8>>;

    /// Whether this path names something readable.
    ///
    /// Import resolution tries several spellings and takes the first that is
    /// there, so this is a probe and not an error path. A provider that cannot
    /// answer cheaply should still answer, because the alternative is reading a
    /// file to find out it exists.
    fn exists(&self, path: &Path) -> bool;

    /// The one name this path is filed under.
    ///
    /// Two `use` lines reaching the same component by different spellings must
    /// produce one entry in the registry, which is what keys the component map.
    /// On a filesystem that is `canonicalize`; anywhere else it is whatever
    /// makes the same file the same string, and the default is enough for any
    /// provider whose paths are already unique.
    fn canonical(&self, path: &Path) -> PathBuf {
        path.to_path_buf()
    }
}

/// The filesystem. What every desktop run and every CLI command uses.
pub struct FsSource;

impl Source for FsSource {
    fn read_text(&self, path: &Path) -> io::Result<String> {
        std::fs::read_to_string(path)
    }

    fn read_bytes(&self, path: &Path) -> io::Result<Vec<u8>> {
        std::fs::read(path)
    }

    fn exists(&self, path: &Path) -> bool {
        path.is_file()
    }

    fn canonical(&self, path: &Path) -> PathBuf {
        // A path that cannot be canonicalised (it may have been deleted between
        // resolving and here) falls back to itself, which is still unique enough
        // to key a map.
        std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
    }
}

/// A set of files held in memory, keyed by path.
///
/// This is the browser's provider, the embedded-release provider, and the one a
/// test uses when it would rather not touch a temporary directory. Keys are
/// normalised to `/` so a document written on Windows and a map built by hand
/// agree about what a path is.
#[derive(Default)]
pub struct MemorySource {
    files: HashMap<String, Vec<u8>>,
}

impl MemorySource {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a file. Chainable, so a small tree reads as one expression.
    pub fn with(mut self, path: impl AsRef<str>, contents: impl Into<Vec<u8>>) -> Self {
        self.insert(path, contents);
        self
    }

    pub fn insert(&mut self, path: impl AsRef<str>, contents: impl Into<Vec<u8>>) {
        self.files.insert(normalise(path.as_ref()), contents.into());
    }

    fn get(&self, path: &Path) -> Option<&Vec<u8>> {
        self.files.get(&normalise(&path.to_string_lossy()))
    }
}

/// One spelling for a path used as a key.
///
/// Separators are unified, and a leading `./` is dropped because `base.join()`
/// produces one for a document loaded as a bare filename and nobody writing a
/// map by hand would type it.
fn normalise(path: &str) -> String {
    let slashed = path.replace('\\', "/");
    slashed.strip_prefix("./").unwrap_or(&slashed).to_string()
}

impl Source for MemorySource {
    fn read_text(&self, path: &Path) -> io::Result<String> {
        let bytes = self.read_bytes(path)?;
        String::from_utf8(bytes)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))
    }

    fn read_bytes(&self, path: &Path) -> io::Result<Vec<u8>> {
        self.get(path).cloned().ok_or_else(|| {
            // The message matches what a filesystem would have said, because it
            // reaches the author through the same overlay either way and "No
            // such file or directory" is what they are used to reading.
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("No such file or directory: {}", path.display()),
            )
        })
    }

    fn exists(&self, path: &Path) -> bool {
        self.get(path).is_some()
    }

    fn canonical(&self, path: &Path) -> PathBuf {
        PathBuf::from(normalise(&path.to_string_lossy()))
    }
}

thread_local! {
    static SOURCE: RefCell<Rc<dyn Source>> = RefCell::new(Rc::new(FsSource));
}

/// Install the provider this thread loads through.
///
/// Returns the one it replaced, so a caller that is borrowing the thread for one
/// load (a checker, a test) can put the previous one back rather than assume the
/// default was in place.
pub fn set_source(source: Rc<dyn Source>) -> Rc<dyn Source> {
    SOURCE.with(|s| std::mem::replace(&mut *s.borrow_mut(), source))
}

/// Go back to reading the filesystem.
pub fn reset_source() {
    set_source(Rc::new(FsSource));
}

fn with<R>(f: impl FnOnce(&dyn Source) -> R) -> R {
    // Cloned out of the cell before the call, so a provider that itself loads a
    // document (nothing does today, and the borrow panic if one ever did would
    // be baffling) cannot find the cell already borrowed.
    let source = SOURCE.with(|s| Rc::clone(&*s.borrow()));
    f(&*source)
}

pub fn read_text(path: &Path) -> io::Result<String> {
    with(|s| s.read_text(path))
}

pub fn read_bytes(path: &Path) -> io::Result<Vec<u8>> {
    with(|s| s.read_bytes(path))
}

pub fn exists(path: &Path) -> bool {
    with(|s| s.exists(path))
}

pub fn canonical(path: &Path) -> PathBuf {
    with(|s| s.canonical(path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_source_reads_what_was_put_in_it() {
        let mem = MemorySource::new().with("app.rux", "<template><screen /></template>");
        assert_eq!(
            mem.read_text(Path::new("app.rux")).unwrap(),
            "<template><screen /></template>"
        );
        assert!(mem.exists(Path::new("app.rux")));
        assert!(!mem.exists(Path::new("other.rux")));
    }

    #[test]
    fn separators_and_a_leading_dot_do_not_make_a_second_file() {
        let mem = MemorySource::new().with("components/task.rux", "x");
        assert!(mem.exists(Path::new("components/task.rux")));
        assert!(mem.exists(Path::new("components\\task.rux")));
        assert!(mem.exists(Path::new("./components/task.rux")));
        assert_eq!(
            mem.canonical(Path::new("components\\task.rux")),
            PathBuf::from("components/task.rux")
        );
    }

    #[test]
    fn a_missing_file_reads_like_a_missing_file() {
        let mem = MemorySource::new();
        let err = mem.read_text(Path::new("gone.rux")).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
        assert!(err.to_string().contains("gone.rux"));
    }

    #[test]
    fn the_default_is_the_filesystem_and_set_source_hands_back_the_old_one() {
        // Whatever this thread had, restore it, so this test cannot leak into
        // another one running after it on the same thread.
        let previous = set_source(Rc::new(MemorySource::new().with("a.rux", "hello")));
        assert_eq!(read_text(Path::new("a.rux")).unwrap(), "hello");
        set_source(previous);
        // The filesystem is back: this file exists, that one does not.
        assert!(!exists(Path::new("a.rux")));
    }
}
