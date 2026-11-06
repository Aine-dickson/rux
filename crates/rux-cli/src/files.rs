//! Turning the paths on a command line into the `.rux` files to work on.
//!
//! Shared by `check` and `fmt` so the two cannot disagree about what is in a
//! tree, which would be its own small source of confusion: a file that formats
//! but is never checked, or the reverse.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Expand the requested paths into `.rux` files: a file is itself, a directory
/// is everything under it. No paths at all means the project the current
/// directory is in (see [`project_dir`]), so a bare `rux check` or `rux fmt`
/// does the obvious thing in CI, from any folder of the project.
pub fn collect(paths: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    collect_with_callers(paths).map(|c| c.files)
}

/// What one `rux check` or `rux fmt` invocation is working on.
pub struct Collected {
    /// The files to work on, in path order.
    pub files: Vec<PathBuf>,
    /// Every file in the project that another one writes as a tag, keyed by
    /// [`rux_runtime::same_file_key`]. Such a file is checked on its own as a
    /// component: its props come from a caller, so it owes them rather than
    /// reporting them as undefined.
    pub used: HashSet<PathBuf>,
}

/// The same as [`collect`], and also which of the files have a caller.
pub fn collect_with_callers(paths: &[PathBuf]) -> Result<Collected, String> {
    let (mut files, mut walked) = (Vec::new(), Vec::new());
    if paths.is_empty() {
        walk(&project_dir(), &mut walked)?;
    }
    for root in paths {
        if root.is_file() {
            files.push(root.clone());
        } else if root.is_dir() {
            walk(root, &mut walked)?;
        } else {
            return Err(format!("no such file or directory: {}", root.display()));
        }
    }
    let used = used_by_something(paths, &walked);
    files.extend(walked);
    files.sort();
    files.dedup();
    Ok(Collected { files, used })
}

/// The folder a bare `rux check` or `rux fmt` works on: the nearest one above
/// the current directory holding `rux.toml`, else the entry document's folder,
/// else the current directory itself (a folder of standalone documents).
fn project_dir() -> PathBuf {
    let here = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    for dir in here.ancestors() {
        if dir.join(crate::manifest::MANIFEST).is_file() {
            // Spelled `.` from the root itself, so the paths in a report stay short.
            return if dir == here { PathBuf::from(".") } else { dir.to_path_buf() };
        }
    }
    match rux_runtime::project_root(&here) {
        Some(root) if !root.as_os_str().is_empty() => {
            if root == here { PathBuf::from(".") } else { root }
        }
        _ => PathBuf::from("."),
    }
}

/// Every file in the project that some other file writes as a tag.
///
/// This is what tells a component from a document, and it replaces asking
/// whether the template's root was `<screen>`. That test let a *layout* choice
/// decide whether a file was ever opened, and it got the ordinary case
/// backwards: a page named by a `<route>` has a `<view>` root, so a whole
/// project's pages were filed as components and skipped while the summary said
/// "no problems found" about the one file it had looked at.
///
/// **Asked of the project, not of the paths on the command line.** `rux check
/// pages/` has to know that `app.rux` one directory up uses everything in
/// there; classifying against the walk alone would call every page a document
/// and report the app's own signals as undefined in each one.
///
/// A file that will not parse imports nothing as far as this is concerned,
/// which is right in both directions: it is still checked itself (the parse
/// error is the point of looking), and it cannot vouch for anything else.
fn used_by_something(requested: &[PathBuf], walked: &[PathBuf]) -> HashSet<PathBuf> {
    // Where to look for importers: the project above each requested path, and
    // the requested path itself when it is outside a project (a directory of
    // standalone documents, which is what `examples/` is).
    let mut roots: Vec<PathBuf> = Vec::new();
    let requested: Vec<PathBuf> =
        if requested.is_empty() { vec![PathBuf::from(".")] } else { requested.to_vec() };
    for path in &requested {
        let from = if path.is_dir() { path.clone() } else {
            path.parent().unwrap_or(Path::new(".")).to_path_buf()
        };
        let root = rux_runtime::project_root(&from).unwrap_or(from);
        // `Path::new("pages").parent()` is the *empty* path, and walking up
        // from there finds `app.rux` in the working directory and calls the
        // empty string the project root. Nothing can be read from it, so every
        // importer goes unseen and every page is called a document: the bug
        // this classifier exists to fix, reintroduced one layer down.
        let root = if root.as_os_str().is_empty() { PathBuf::from(".") } else { root };
        if !roots.contains(&root) {
            roots.push(root);
        }
    }

    let mut candidates: Vec<PathBuf> = walked.to_vec();
    for root in &roots {
        let mut found = Vec::new();
        // A root that cannot be read contributes nothing rather than failing
        // the command: whether a file is a component is not worth refusing to
        // check over.
        if walk(root, &mut found).is_ok() {
            candidates.extend(found);
        }
    }

    let mut used = HashSet::new();
    let mut seen = HashSet::new();
    for file in candidates {
        if !seen.insert(rux_runtime::same_file_key(&file)) {
            continue;
        }
        for (_, import) in rux_runtime::imports_of(&file).unwrap_or_default() {
            used.insert(import);
        }
    }
    used
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries = std::fs::read_dir(dir).map_err(|e| format!("reading {}: {e}", dir.display()))?;
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        // Build output and version control hold no source worth looking at, and
        // `target/` in particular is large enough to make this feel broken.
        // `locks/`, `dist/` and `native/` hold generated output and Rust.
        if name.starts_with('.')
            || matches!(&*name, "target" | "node_modules" | "locks" | "dist" | "native")
        {
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
