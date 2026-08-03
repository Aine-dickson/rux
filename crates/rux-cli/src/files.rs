//! Turning the paths on a command line into the `.rux` files to work on.
//!
//! Shared by `check` and `fmt` so the two cannot disagree about what is in a
//! tree, which would be its own small source of confusion: a file that formats
//! but is never checked, or the reverse.

use std::path::{Path, PathBuf};

/// Whether a walked file that is a component should be left out. Formatting a
/// component is fine; *checking* one on its own is not, because its props come
/// from whoever uses it. See [`rux_runtime::is_entry_point`].
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Components {
    Include,
    SkipWhenWalking,
}

/// Expand the requested paths into `.rux` files: a file is itself, a directory
/// is everything under it. No paths at all means the current directory, so a
/// bare `rux check` or `rux fmt` does the obvious thing in CI.
///
/// A file named explicitly is always included, even when walking would have
/// skipped it: naming it was deliberate.
pub fn collect(paths: &[PathBuf], components: Components) -> Result<Vec<PathBuf>, String> {
    let (mut explicit, mut walked) = (Vec::new(), Vec::new());
    if paths.is_empty() {
        walk(Path::new("."), &mut walked)?;
    }
    for root in paths {
        if root.is_file() {
            explicit.push(root.clone());
        } else if root.is_dir() {
            walk(root, &mut walked)?;
        } else {
            return Err(format!("no such file or directory: {}", root.display()));
        }
    }

    let mut files = explicit;
    files.extend(walked.into_iter().filter(|f| match components {
        Components::Include => true,
        // `Some(false)` is "definitely a component". `None` means the file would
        // not parse, and that is never a reason to skip it: the parse error is
        // the whole point of looking.
        Components::SkipWhenWalking => rux_runtime::is_entry_point(f) != Some(false),
    }));
    files.sort();
    files.dedup();
    Ok(files)
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries = std::fs::read_dir(dir).map_err(|e| format!("reading {}: {e}", dir.display()))?;
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        // Build output and version control hold no source worth looking at, and
        // `target/` in particular is large enough to make this feel broken.
        if name.starts_with('.') || name == "target" || name == "node_modules" {
            continue;
        }
        if path.is_dir() {
            walk(&path, out)?;
        } else if path.extension().is_some_and(|e| e == "rux") {
            out.push(path);
        }
    }
    Ok(())
}
