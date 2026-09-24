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
    /// Print the types the checker worked out beside the diagnostics, which
    /// makes the JSON an object rather than an array. See `docs/10-types.md`,
    /// "Editor".
    pub types: bool,
    /// Treat warnings as failures, which is what CI wants and what keeps
    /// `examples/` clean.
    pub deny_warnings: bool,
}

/// Check every requested file. Returns the process exit code.
pub fn run(options: Options) -> i32 {
    // The sink is reported as diagnostics, so the runtime must not also print
    // each warning to stderr as prose.
    rux_runtime::set_stderr_echo(false);
    rux_runtime::record_types(options.types);

    let collected = match collect_files(&options.paths) {
        Ok(it) => it,
        Err(err) => {
            eprintln!("rux: {err}");
            return 2;
        }
    };
    let (mut files, skipped) = (collected.files, collected.skipped);

    // A page cannot be read on its own, so being asked about one is answered by
    // checking the document it belongs to. That covers both ways of asking:
    // `rux check pages/home.rux`, where the page was named, and `rux check
    // pages/`, where walking finds nothing but pages and would otherwise report
    // an empty directory.
    //
    // Only when the document is not already being checked. Over a whole project
    // it always is, and saying so there would be noise about the ordinary case.
    let asked_about: Vec<PathBuf> = files.iter().chain(skipped.iter()).cloned().collect();
    let mut borrowed: Vec<(PathBuf, Vec<PathBuf>)> = Vec::new();
    for file in &asked_about {
        let Some(context) = context_for(file) else { continue };
        if files.iter().any(|f| same_file(f, &context)) {
            continue;
        }
        match borrowed.iter_mut().find(|(c, _)| same_file(c, &context)) {
            Some((_, pages)) => pages.push(file.clone()),
            None => borrowed.push((context, vec![file.clone()])),
        }
    }
    // What the answer is allowed to be about, when a document had to be brought
    // in that nobody asked for. Its *own* problems count, because a page cannot
    // be checked through a document that is broken; its other pages do not,
    // since nobody asked about those.
    let mut only_about: Vec<PathBuf> = Vec::new();
    for (context, pages) in &borrowed {
        eprintln!(
            "rux: {} checked through {}, where the app's signals and functions are in scope",
            if pages.len() == 1 {
                pages[0].display().to_string()
            } else {
                format!("{} pages", pages.len())
            },
            context.display()
        );
        // The page itself comes off the list: checking it *as well* would
        // report exactly the findings this exists to stop, the app's own state
        // read as undefined names in a file that never declared it.
        files.retain(|f| !pages.iter().any(|p| same_file(p, f)));
        only_about.extend(pages.iter().cloned());
        only_about.push(context.clone());
        files.push(context.clone());
    }

    if files.is_empty() {
        eprintln!("rux: no .rux files found");
        return 2;
    }

    let mut found = Vec::new();
    let mut tables: Vec<(PathBuf, rux_runtime::TypeTable)> = Vec::new();
    let mut reached: Vec<PathBuf> = Vec::new();
    for file in &files {
        // Whether anything in this project writes this file as a tag, which is
        // what decides whether a name it never declares is a mistake or a prop
        // somebody hands it. Only ever true for a file named on the command
        // line: walking drops the ones with callers.
        let has_caller = collected.used.contains(&rux_runtime::same_file_key(file));
        rux_runtime::set_may_have_caller(has_caller);
        let checked = check_file(file);
        found.extend(checked.diagnostics);
        if options.types {
            // Only the document's own script is checked for types, so what was
            // kept is all this file's.
            tables.push((file.clone(), rux_runtime::take_type_table()));
        }
        for page in checked.reached {
            if !reached.contains(&page) {
                reached.push(page);
            }
        }
    }
    rux_runtime::set_may_have_caller(false);

    // A page checked through the document that routes to it was looked at, and
    // calling it skipped is the exact misreport this release set out to end.
    let skipped: Vec<PathBuf> =
        skipped.into_iter().filter(|p| !reached.iter().any(|r| same_file(r, p))).collect();

    if !only_about.is_empty() {
        found.retain(|d| only_about.iter().any(|p| same_file(p, &d.file)));
    }

    if options.json && options.types {
        print!("{}", to_json_with_types(&found, &tables));
    } else if options.json {
        print!("{}", to_json(&found));
    } else {
        for d in &found {
            println!("{}", render(d));
        }
    }

    let errors = found.iter().filter(|d| d.severity == Severity::Error).count();
    let warnings = found.len() - errors;
    if !options.json {
        report_summary(files.len(), reached.len(), errors, warnings, &skipped);
    }

    if errors > 0 || (options.deny_warnings && warnings > 0) {
        1
    } else {
        0
    }
}

/// One file's findings, and the other files looking at it reached.
struct Checked {
    diagnostics: Vec<Diagnostic>,
    /// The pages this document routes to, which were checked through it rather
    /// than on their own. Reported so the summary can say they were looked at:
    /// they are not components and calling them skipped is how a project came
    /// to believe five of its six files had been read.
    reached: Vec<PathBuf>,
}

/// Load one file and turn whatever it says into diagnostics.
fn check_file(file: &Path) -> Checked {
    // The warning sinks are global. Clear them first so a previous file's
    // leftovers cannot be attributed to this one. The `print` sink goes with
    // them: `rux check` never reports prints, so anything left in it would be
    // carried silently into whichever document happens to be built next.
    let _ = rux_runtime::take_warnings();
    let _ = rux_runtime::take_prints();
    let _ = rux_runtime::take_type_table();

    match Document::load_checked(file) {
        Ok(mut doc) => {
            let mut warnings = doc.diagnostics().warnings.clone();
            // A router builds the matching route and nothing else, so the load
            // above has looked at exactly one page: the one at `/`. Walking the
            // rest is what makes a routed project checkable at all, and it is
            // the only way a page gets checked with its app's signals and
            // functions in scope, which is the whole reason naming one on the
            // command line reports the app's own state as undefined.
            let reached = rux_runtime::route_views_of(file);
            for path in doc.route_visits() {
                doc.navigate(&path);
                // A rebuild *replaces* what the document says is wrong with
                // what this build found, so each visit has to be taken as it
                // happens rather than read once at the end.
                for w in doc.diagnostics().warnings.clone() {
                    if !warnings.contains(&w) {
                        warnings.push(w);
                    }
                }
            }
            let diagnostics = warnings
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
            .collect();
            Checked { diagnostics, reached }
        }
        Err(err) => {
            // A failed load can still have warned or printed on its way down, and
            // those would otherwise surface against the next file.
            let _ = rux_runtime::take_warnings();
            let _ = rux_runtime::take_prints();
            Checked {
                diagnostics: vec![Diagnostic {
                    // A `use`d component reports against its own file, not the
                    // one that imported it, so the squiggle lands where the
                    // mistake is.
                    file: err.file.clone().unwrap_or_else(|| file.to_path_buf()),
                    line: err.line,
                    column: err.column,
                    severity: Severity::Error,
                    message: err.message.clone(),
                }],
                // A document that would not load routed nowhere.
                reached: Vec::new(),
            }
        }
    }
}

/// Whether two paths name the same file, whatever they are spelled like.
fn same_file(a: &Path, b: &Path) -> bool {
    rux_runtime::same_file_key(a) == rux_runtime::same_file_key(b)
}

/// The document a file can only be checked *through*, if there is one.
///
/// A page reads the app's signals and calls its functions, and those are shared
/// into the engine by the document that declares them. Read on its own a page
/// reports every one of them as missing, which is watchlist item 12: the one
/// command that reached a file the walk skipped invented failures in it.
///
/// So naming a page checks the app, which builds that page at the route it
/// belongs to with everything in scope. Only for a page: a plain component has
/// no single caller to be checked through, and inventing one would pick a
/// context out of however many use it.
fn context_for(file: &Path) -> Option<PathBuf> {
    let entry = rux_runtime::project_entry(file)?;
    if same_file(&entry, file) {
        return None;
    }
    rux_runtime::route_views_of(&entry)
        .contains(&rux_runtime::same_file_key(file))
        .then_some(entry)
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

fn report_summary(
    files: usize,
    reached: usize,
    errors: usize,
    warnings: usize,
    skipped: &[PathBuf],
) {
    let file_word = if files == 1 { "file" } else { "files" };
    // Pages are counted apart from documents rather than folded in, because
    // "checked 5 files" over a project of five would hide the one thing worth
    // knowing: four of them were checked *through* the app, with its signals
    // and functions in scope, and cannot be checked any other way.
    let through = if reached == 0 {
        String::new()
    } else {
        format!(
            " and the {reached} page{} {}",
            if reached == 1 { "" } else { "s" },
            if files == 1 { "it routes to" } else { "they route to" }
        )
    };
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
        eprintln!("rux: checked {files} {file_word}{through}, no problems found");
    } else {
        eprintln!(
            "rux: checked {files} {file_word}{through}, {errors} error{}, {warnings} warning{}",
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

/// `{ "diagnostics": [...], "types": [...], "guesses": [...] }`: what
/// [`to_json`] prints, beside every name, field and function the checker gave
/// a type, and the type each unannotated parameter was handed by its calls.
fn to_json_with_types(found: &[Diagnostic], tables: &[(PathBuf, rux_runtime::TypeTable)]) -> String {
    let mut out = String::from("{\"diagnostics\": ");
    out.push_str(to_json(found).trim_end());
    out.push_str(",
\"types\": [");
    let mut first = true;
    for (file, table) in tables {
        let file = json_string(&file.display().to_string());
        for s in &table.seen {
            out.push_str(if first { "
  " } else { ",
  " });
            first = false;
            let fields: Vec<String> = s
                .fields
                .iter()
                .map(|(name, ty, optional)| {
                    format!(
                        "{{\"name\": {}, \"type\": {}, \"optional\": {optional}}}",
                        json_string(name),
                        json_string(ty)
                    )
                })
                .collect();
            out.push_str(&format!(
                "{{\"file\": {file}, \"line\": {}, \"column\": {}, \"kind\": \"{}\", \"name\": {}, \
                 \"path\": {}, \"type\": {}, \"nullable\": {}, \"fields\": [{}]}}",
                s.line,
                s.column.map_or("null".to_string(), |c| c.to_string()),
                s.kind,
                json_string(&s.name),
                s.path.as_deref().map_or("null".to_string(), json_string),
                json_string(&s.ty),
                s.nullable,
                fields.join(", ")
            ));
        }
    }
    out.push_str("
],
\"guesses\": [");
    let mut first = true;
    for (file, table) in tables {
        let file = json_string(&file.display().to_string());
        for g in &table.guesses {
            out.push_str(if first { "
  " } else { ",
  " });
            first = false;
            out.push_str(&format!(
                "{{\"file\": {file}, \"line\": {}, \"function\": {}, \"param\": {}, \"type\": {}}}",
                g.line,
                json_string(&g.function),
                json_string(&g.param),
                json_string(&g.ty)
            ));
        }
    }
    out.push_str("
]}
");
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
fn collect_files(paths: &[PathBuf]) -> Result<crate::files::Collected, String> {
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
