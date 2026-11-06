//! `rux check` of `window.open`: a call written out is looked up. See
//! `rux_runtime::window_open`.

use rux_runtime::Document;
use std::path::PathBuf;

const INSPECTOR: &str = "<template><screen><text>{{ id }}</text></screen></template>\n<script>prop id: int;</script>\n";

/// A project on disk with `app` as its entry and `files` beside it. Each test
/// has a folder of its own (they run in parallel).
fn project(name: &str, call: &str, files: &[(&str, &str)]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rux-window-open-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("windows")).unwrap();
    let app = format!(
        "<template><screen><button @tap=\"go()\">go</button></screen></template>\n<script>\nlet target = \"windows/inspector.rux\";\nfn go() {{ {call} }}\n</script>\n"
    );
    std::fs::write(dir.join("app.rux"), app).unwrap();
    for (path, body) in files {
        std::fs::write(dir.join(path), body).unwrap();
    }
    dir
}

/// What loading the app says, as (is it an error, the message).
fn said(dir: &PathBuf) -> Vec<(bool, String)> {
    let doc = Document::load_checked(dir.join("app.rux")).expect("the app loads");
    doc.diagnostics().warnings.iter().map(|w| (w.is_error(), w.message.clone())).collect()
}

#[test]
fn a_call_that_matches_its_window_is_clean() {
    let dir = project(
        "clean",
        "window.open(\"windows/inspector.rux\", { title: \"x\", props: { id: 7 } });",
        &[("windows/inspector.rux", INSPECTOR)],
    );
    assert_eq!(said(&dir), Vec::<(bool, String)>::new());
}

#[test]
fn a_file_that_is_not_there_is_an_error() {
    let dir = project("missing", "window.open(\"windows/nope.rux\", {});", &[]);
    let found = said(&dir);
    assert!(found.iter().any(|(e, m)| *e && m.contains("names a file that is not there")), "{found:?}");
}

#[test]
fn a_prop_the_window_needs_is_an_error_when_left_out() {
    let dir = project("needs", "window.open(\"windows/inspector.rux\", {});", &[("windows/inspector.rux", INSPECTOR)]);
    let found = said(&dir);
    assert!(found.iter().any(|(e, m)| *e && m.contains("needs the prop `id`")), "{found:?}");
}

#[test]
fn a_prop_of_the_wrong_type_is_an_error() {
    let dir = project(
        "type",
        "window.open(\"windows/inspector.rux\", { props: { id: \"seven\" } });",
        &[("windows/inspector.rux", INSPECTOR)],
    );
    let found = said(&dir);
    assert!(
        found.iter().any(|(e, m)| *e && m.contains("passed the prop `id` text") && m.contains("declares it `int`")),
        "{found:?}"
    );
}

#[test]
fn a_prop_the_window_does_not_declare_is_an_error() {
    let dir = project(
        "unknown-prop",
        "window.open(\"windows/inspector.rux\", { props: { id: 1, extra: 2 } });",
        &[("windows/inspector.rux", INSPECTOR)],
    );
    let found = said(&dir);
    assert!(found.iter().any(|(e, m)| *e && m.contains("does not declare it") && m.contains("`extra`")), "{found:?}");
}

#[test]
fn an_option_the_call_does_not_take_is_named() {
    let dir = project(
        "option",
        "window.open(\"windows/inspector.rux\", { colour: \"red\", props: { id: 1 } });",
        &[("windows/inspector.rux", INSPECTOR)],
    );
    let found = said(&dir);
    assert!(found.iter().any(|(e, m)| *e && m.contains("no option `colour`") && m.contains("titleBar")), "{found:?}");
}

#[test]
fn a_path_that_is_not_written_out_is_a_warning() {
    let dir = project("dynamic", "window.open(target, { props: { id: 1 } });", &[("windows/inspector.rux", INSPECTOR)]);
    let found = said(&dir);
    assert!(found.iter().any(|(e, m)| !*e && m.contains("not written out")), "{found:?}");
    assert!(!found.iter().any(|(e, _)| *e), "{found:?}");
}

#[test]
fn a_file_that_is_not_a_window_is_an_error() {
    let dir = project(
        "root",
        "window.open(\"windows/inspector.rux\", { props: { id: 1 } });",
        &[("windows/inspector.rux", "<template><view><text>x</text></view></template>\n<script>prop id: int;</script>\n")],
    );
    let found = said(&dir);
    assert!(found.iter().any(|(e, m)| *e && m.contains("`<screen>` as its root")), "{found:?}");
}

#[test]
fn a_prop_passed_as_a_variable_is_not_judged() {
    let dir = project(
        "variable",
        "let n = 3; window.open(\"windows/inspector.rux\", { props: { id: n } });",
        &[("windows/inspector.rux", INSPECTOR)],
    );
    assert_eq!(said(&dir), Vec::<(bool, String)>::new());
}
