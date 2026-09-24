//! The type checker, as a document meets it: findings on the file's own lines,
//! types imported from another file, and a component checked on its own with
//! its props typed. The rules themselves are tested in `rux-script`
//! (`check.rs`); these are about the wiring. See `docs/10-types.md`.

use std::fs;
use std::path::PathBuf;

use rux_runtime::Document;

static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

fn project(files: &[(&str, &str)]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "rux_type_check_{}_{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
    ));
    let _ = fs::remove_dir_all(&dir);
    for (rel, body) in files {
        let path = dir.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }
    dir
}

/// Every finding as (line, is it an error, message).
fn found(doc: &Document) -> Vec<(Option<usize>, bool, String)> {
    doc.diagnostics().warnings.iter().map(|w| (w.line, w.is_error(), w.message.clone())).collect()
}

fn load_err(path: PathBuf) -> String {
    match Document::load(path) {
        Ok(_) => panic!("loaded, and should not have"),
        Err(e) => e,
    }
}

const TYPES: &str = "<script>\n\
    type Task = { id: int, title: string, done: bool };\n\
    type Filter = \"all\" | \"open\" | \"done\";\n\
    type Board = { tasks: Task[], filter: Filter };\n\
    </script>\n";

fn page(script: &str) -> String {
    format!("<template><screen><text>x</text></screen></template>\n<script>\n{script}\n</script>\n")
}

/// A type error is placed on the line of the file it is on, counting the
/// template above the script.
#[test]
fn a_type_error_is_on_its_file_line() {
    let dir = project(&[("app.rux", &page("let a = 1;\nlet n: int = \"x\";"))]);
    let doc = Document::load(dir.join("app.rux")).expect("loads");
    let f = found(&doc);
    // Line 1 is the template, 2 is `<script>`, 3 is `let a`, 4 is `let n`.
    assert!(f.iter().any(|(line, err, m)| *line == Some(4) && *err && m.contains("where `int`")), "{f:?}");
}

/// `use types::Task` makes `Task` known, and checks against it.
#[test]
fn an_imported_type_is_checked_against() {
    let script = "use types::Task;\nlet t: Task = { id: 1, titel: \"a\", done: false };";
    let dir = project(&[("app.rux", &page(script)), ("types.rux", TYPES)]);
    let doc = Document::load(dir.join("app.rux")).expect("loads");
    let f = found(&doc);
    assert!(f.iter().any(|(_, err, m)| *err && m.contains("did you mean `title`")), "{f:?}");
}

/// An imported type may be built from the types beside it, and those resolve;
/// naming one without importing it is an error that says how to.
#[test]
fn a_type_beside_an_imported_one_resolves_and_is_not_nameable() {
    let script = "use types::Board;\n\
                  let b: Board = { tasks: [{ id: 1, title: \"a\", done: false }], filter: \"al\" };\n\
                  let f: Filter = \"all\";";
    let dir = project(&[("app.rux", &page(script)), ("types.rux", TYPES)]);
    let doc = Document::load(dir.join("app.rux")).expect("loads");
    let f = found(&doc);
    assert!(f.iter().any(|(_, err, m)| *err && m.contains("did you mean \"all\"")), "{f:?}");
    assert!(
        f.iter().any(|(_, err, m)| *err && m.contains("`Filter` is not imported here; bring it in with `use types::Filter;`")),
        "{f:?}"
    );
}

/// A type import naming a type its file does not declare says what the file
/// does declare.
#[test]
fn a_type_the_file_does_not_declare() {
    let dir = project(&[("app.rux", &page("use types::Tasks;")), ("types.rux", TYPES)]);
    let err = load_err(dir.join("app.rux"));
    assert!(err.contains("`types.rux` has no type `Tasks`"), "{err}");
    assert!(err.contains("`Task`, `Filter`, `Board`"), "{err}");
}

/// A component checked on its own: a typed prop is what it says, an untyped
/// one with no default is warned, and a default that does not fit its type is
/// an error.
#[test]
fn a_component_on_its_own_knows_its_props() {
    let comp = "<template><view><text>{{ label }}</text></view></template>\n\
        <script>\n\
          prop label: string;\n\
          prop size: int = \"big\";\n\
          prop tone;\n\
          fn shout(): int { label }\n\
        </script>\n";
    let dir = project(&[("components/badge.rux", comp)]);
    let doc = Document::load(dir.join("components/badge.rux")).expect("loads");
    let f = found(&doc);
    assert!(f.iter().any(|(_, err, m)| *err && m.contains("where `int` is expected")), "the default: {f:?}");
    assert!(f.iter().any(|(_, err, m)| !*err && m.contains("`prop tone` has no type")), "{f:?}");
    assert!(
        f.iter().any(|(_, err, m)| *err && m.contains("declared to return `int`, and it returns `string`")),
        "the prop's type reaches the body: {f:?}"
    );
}

/// A component's functions ride in the document's script, and a finding in
/// one is the component's to report, not the document's.
#[test]
fn a_components_function_is_not_reported_against_the_document() {
    let comp = "<template><view><text>c</text></view></template>\n\
        <script>\n  fn broken(): int { \"x\" }\n</script>\n";
    let app = "<template><screen><card /></screen></template>\n<script>\nuse components::card;\n</script>\n";
    let dir = project(&[("app.rux", app), ("components/card.rux", comp)]);
    let doc = Document::load(dir.join("app.rux")).expect("loads");
    let f = found(&doc);
    assert!(!f.iter().any(|(_, _, m)| m.contains("broken")), "{f:?}");
}

/// A program with nothing wrong in it says nothing, and the typed version of
/// an ordinary page runs as it did.
#[test]
fn a_clean_typed_page_is_quiet() {
    let script = "use types::Task;\nuse types::Filter;\n\
                  let tasks: Task[] = signal([]);\n\
                  let filter: Filter = signal(\"all\");\n\
                  fn visible(): Task[] { tasks.filter(t => filter == \"all\" || t.done == (filter == \"done\")) }\n\
                  fn add(title: string) { tasks = tasks + [{ id: tasks.length, title: title, done: false }]; }";
    let dir = project(&[("app.rux", &page(script)), ("types.rux", TYPES)]);
    let doc = Document::load(dir.join("app.rux")).expect("loads");
    assert!(found(&doc).is_empty(), "{:?}", found(&doc));
}
