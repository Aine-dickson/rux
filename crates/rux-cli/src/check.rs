//! `rux check`: load documents and report what is wrong with them, in a form a
//! machine can read.
//!
//! Until this existed, a broken `.rux` file told you nothing useful without
//! opening a window: the app fell back to an empty screen, and the warnings the
//! cascade already computes went to stderr where nobody running a GUI was
//! looking. The dev overlay fixed that for someone watching the window. This
//! fixes it for CI and for an editor, which are the two readers that cannot
//! watch a window.
//!
//! It deliberately reuses the loader rather than re-implementing a parse pass:
//! a checker that disagrees with the runtime is worse than no checker.

use std::path::{Path, PathBuf};

// `json_string` is shared with the browser playground, which serialises the same
// warnings. Two hand-rolled escapers is how the re-indenter came to disagree
// with itself.
use rux_runtime::{json_string, Document};

/// How bad a finding is. Errors mean the document will not load at all.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Severity {
    Error,
    Warning,
}

impl Severity {
    fn label(self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warning => "warning",
        }
    }
}

/// One thing wrong with one file.
#[derive(Clone, Debug)]
pub struct Diagnostic {
    pub file: PathBuf,
    /// 1-based, when the stage that produced this knew where it was. Parse
    /// errors and CSS warnings do; expression failures do not yet, because the
    /// template parser does not record where each binding started.
    pub line: Option<usize>,
    pub column: Option<usize>,
    pub severity: Severity,
    pub message: String,
}

/// What `rux check` was asked to do.
pub struct Options {
    pub paths: Vec<PathBuf>,
    pub json: bool,
    /// Treat warnings as failures, which is what CI wants and what keeps
    /// `examples/` clean.
    pub deny_warnings: bool,
}

/// Check every requested file. Returns the process exit code.
pub fn run(options: Options) -> i32 {
    // The sink is reported as diagnostics, so the runtime must not also print
    // each warning to stderr as prose.
    rux_runtime::set_stderr_echo(false);

    let (files, skipped) = match collect_files(&options.paths) {
        Ok(both) => both,
        Err(err) => {
            eprintln!("rux: {err}");
            return 2;
        }
    };
    if files.is_empty() {
        eprintln!("rux: no .rux files found");
        return 2;
    }

    let mut found = Vec::new();
    for file in &files {
        found.extend(check_file(file));
    }

    if options.json {
        print!("{}", to_json(&found));
    } else {
        for d in &found {
            println!("{}", render(d));
        }
    }

    let errors = found.iter().filter(|d| d.severity == Severity::Error).count();
    let warnings = found.len() - errors;
    if !options.json {
        report_summary(files.len(), errors, warnings, &skipped);
    }

    if errors > 0 || (options.deny_warnings && warnings > 0) {
        1
    } else {
        0
    }
}

/// Load one file and turn whatever it says into diagnostics.
fn check_file(file: &Path) -> Vec<Diagnostic> {
    // The warning sinks are global. Clear them first so a previous file's
    // leftovers cannot be attributed to this one. The `print` sink goes with
    // them: `rux check` never reports prints, so anything left in it would be
    // carried silently into whichever document happens to be built next.
    let _ = rux_runtime::take_warnings();
    let _ = rux_runtime::take_prints();

    match Document::load_checked(file) {
        Ok(doc) => doc
            .diagnostics()
            .warnings
            .iter()
            .map(|w| Diagnostic {
                // A warning raised inside a `use`d component names that
                // component's file, matching what the error path above has done
                // since components landed. Anything else pairs the component's
                // line number with the importing file's name, which reads as a
                // precise location and is not one.
                file: w.file.clone().unwrap_or_else(|| file.to_path_buf()),
                line: w.line,
                // No column: the CSS parser locates a *rule*, not the
                // declaration inside it, so pointing at a column would be
                // pointing at the selector.
                column: None,
                // A document can build and still contain something simply
                // wrong: an expression that cannot resolve, an `@event` the
                // runtime never dispatches. Those are errors even though the
                // load succeeded, and `rux check` used to call such a file
                // clean and exit 0.
                severity: if w.is_error() { Severity::Error } else { Severity::Warning },
                message: w.message.clone(),
            })
            .collect(),
        Err(err) => {
            // A failed load can still have warned or printed on its way down, and
            // those would otherwise surface against the next file.
            let _ = rux_runtime::take_warnings();
            let _ = rux_runtime::take_prints();
            vec![Diagnostic {
                // A `use`d component reports against its own file, not the one
                // that imported it, so the squiggle lands where the mistake is.
                file: err.file.clone().unwrap_or_else(|| file.to_path_buf()),
                line: err.line,
                column: err.column,
                severity: Severity::Error,
                message: err.message.clone(),
            }]
        }
    }
}

/// `path:line:col: severity: message`, the shape every compiler emits and every
/// editor and CI log already knows how to parse.
fn render(d: &Diagnostic) -> String {
    let path = d.file.display();
    match (d.line, d.column) {
        (Some(l), Some(c)) => format!("{path}:{l}:{c}: {}: {}", d.severity.label(), d.message),
        (Some(l), None) => format!("{path}:{l}: {}: {}", d.severity.label(), d.message),
        _ => format!("{path}: {}: {}", d.severity.label(), d.message),
    }
}

fn report_summary(files: usize, errors: usize, warnings: usize, skipped: &[PathBuf]) {
    let file_word = if files == 1 { "file" } else { "files" };
    // Said out loud, because "checked 2 files, no problems found" over a project
    // of four reads as a clean bill of health for all four. A component is
    // skipped on purpose (its props come from whoever uses it, so reading it
    // alone invents warnings), but skipping in silence is how somebody comes to
    // believe a file was looked at when it never was.
    if !skipped.is_empty() {
        // Named, but not all of them: a project can hold dozens of components
        // and a line listing every one is scrolled past rather than read, which
        // would put this straight back where it started.
        const SHOWN: usize = 3;
        let mut names: Vec<String> =
            skipped.iter().take(SHOWN).map(|p| p.display().to_string()).collect();
        if skipped.len() > SHOWN {
            names.push(format!("and {} more", skipped.len() - SHOWN));
        }
        eprintln!(
            "rux: skipped {} component{} ({})",
            skipped.len(),
            if skipped.len() == 1 { "" } else { "s" },
            names.join(", ")
        );
        eprintln!(
            "rux: a component's props come from its caller, so name one to check it on its own"
        );
    }
    if errors == 0 && warnings == 0 {
        eprintln!("rux: checked {files} {file_word}, no problems found");
    } else {
        eprintln!(
            "rux: checked {files} {file_word}, {errors} error{}, {warnings} warning{}",
            if errors == 1 { "" } else { "s" },
            if warnings == 1 { "" } else { "s" },
        );
    }
}

/// Hand-rolled rather than pulled from a JSON crate: the shape is fixed and
/// four fields wide, and the CLI is the one place a dependency is most visible
/// to someone running `cargo install`.
fn to_json(found: &[Diagnostic]) -> String {
    let mut out = String::from("[");
    for (i, d) in found.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str("\n  {");
        out.push_str(&format!("\"file\": {}", json_string(&d.file.display().to_string())));
        match d.line {
            Some(l) => out.push_str(&format!(", \"line\": {l}")),
            None => out.push_str(", \"line\": null"),
        }
        match d.column {
            Some(c) => out.push_str(&format!(", \"column\": {c}")),
            None => out.push_str(", \"column\": null"),
        }
        out.push_str(&format!(", \"severity\": \"{}\"", d.severity.label()));
        out.push_str(&format!(", \"message\": {}", json_string(&d.message)));
        out.push('}');
    }
    if !found.is_empty() {
        out.push('\n');
    }
    out.push_str("]\n");
    out
}


/// Expand the requested paths into `.rux` files: a file is itself, a directory
/// is everything under it. No paths at all means the current directory, so
/// `rux check` on its own does the obvious thing in CI.
///
/// Components found by walking are dropped: their props come from the parent
/// that passes them, so checking one on its own reports every prop as an
/// undefined variable, and a checker whose default output is four false
/// failures is one nobody will keep in CI.
fn collect_files(paths: &[PathBuf]) -> Result<(Vec<PathBuf>, Vec<PathBuf>), String> {
    crate::files::collect_reporting_skips(paths, crate::files::Components::SkipWhenWalking)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn diag(line: Option<usize>, column: Option<usize>, severity: Severity) -> Diagnostic {
        Diagnostic {
            file: PathBuf::from("app.rux"),
            line,
            column,
            severity,
            message: "something".into(),
        }
    }

    #[test]
    fn a_located_error_renders_like_a_compiler() {
        let d = diag(Some(12), Some(5), Severity::Error);
        assert_eq!(render(&d), "app.rux:12:5: error: something");
    }

    /// CSS warnings have no position yet, and must still be reported rather than
    /// dropped for want of a line number.
    #[test]
    fn an_unlocated_warning_still_names_its_file() {
        let d = diag(None, None, Severity::Warning);
        assert_eq!(render(&d), "app.rux: warning: something");
    }

    #[test]
    fn json_escapes_what_would_otherwise_break_it() {
        assert_eq!(json_string(r#"a "b" \ c"#), r#""a \"b\" \\ c""#);
        assert_eq!(json_string("line\nbreak"), r#""line\nbreak""#);
        // Windows paths are the everyday case for the backslash rule.
        assert_eq!(json_string(r"examples\form.rux"), r#""examples\\form.rux""#);
    }

    #[test]
    fn json_is_an_array_and_survives_being_empty() {
        assert_eq!(to_json(&[]), "[]\n");
        let out = to_json(&[diag(Some(3), Some(9), Severity::Error)]);
        assert!(out.starts_with("[\n  {"), "{out}");
        assert!(out.contains(r#""line": 3"#), "{out}");
        assert!(out.contains(r#""column": 9"#), "{out}");
        assert!(out.contains(r#""severity": "error""#), "{out}");
        assert!(out.trim_end().ends_with(']'), "{out}");
    }

    /// An unlocated diagnostic must still be valid JSON, so the fields are
    /// present and null rather than absent.
    #[test]
    fn json_keeps_null_positions_rather_than_dropping_them() {
        let out = to_json(&[diag(None, None, Severity::Warning)]);
        assert!(out.contains(r#""line": null"#), "{out}");
        assert!(out.contains(r#""column": null"#), "{out}");
    }
}
