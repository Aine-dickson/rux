//! Turning the paths on a command line into the `.rux` files to work on.
//!
//! Shared by `check` and `fmt` so the two cannot disagree about what is in a
//! tree, which would be its own small source of confusion: a file that formats
//! but is never checked, or the reverse.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Whether a walked file that is a component should be left out. Formatting a
/// component is fine; *checking* one on its own is not, because its props come
/// from whoever uses it. See [`used_by_something`].
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
    collect_reporting_skips(paths, components).map(|c| c.files)
}

/// What one `rux check` or `rux fmt` invocation is working on.
pub struct Collected {
    /// The files to work on, in path order.
    pub files: Vec<PathBuf>,
    /// The ones walking deliberately left out.
    ///
    /// Carried rather than swallowed because the count is the difference
    /// between "your project is clean" and "two of your four files were never
    /// looked at", and those read identically when only the first number is
    /// printed. Someone read `checked 2 files, no problems found` over a
    /// four-file project and reasonably took it for a clean bill of health.
    pub skipped: Vec<PathBuf>,
    /// Every file in the project that another one writes as a tag, keyed by
    /// [`rux_runtime::same_file_key`]. A caller checking one of these on its
    /// own has to say so, or the props it is handed read as mistakes.
    pub used: HashSet<PathBuf>,
}

/// The same as [`collect`], and also what was left out and what has a caller.
pub fn collect_reporting_skips(
    paths: &[PathBuf],
    components: Components,
) -> Result<Collected, String> {
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

    let used = match components {
        Components::Include => HashSet::new(),
        Components::SkipWhenWalking => used_by_something(paths, &walked),
    };

    let mut files = explicit;
    let mut skipped = Vec::new();
    for f in walked {
        let keep = match components {
            Components::Include => true,
            Components::SkipWhenWalking => !used.contains(&rux_runtime::same_file_key(&f)),
        };
        if keep {
            files.push(f);
        } else {
            skipped.push(f);
        }
    }
    files.sort();
    files.dedup();
    // A file named explicitly is checked even if it is a component, so it must
    // not also be reported as skipped.
    skipped.retain(|s| !files.contains(s));
    skipped.sort();
    skipped.dedup();
    Ok(Collected { files, skipped, used })
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
