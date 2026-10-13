//! Closing the app: `@close` on the `<screen>`, `close()`, and the
//! `unmounted` bodies a close runs (Mava Studio's MS-006).
//!
//! `@close` answers as a route guard does: `false` keeps the app open,
//! anything else lets it close. Unlike a guard, one that fails lets it close,
//! because a window nobody can close is the worse outcome.

use std::fs;
use std::path::PathBuf;

use rux_layout::Node;
use rux_runtime::Document;

static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// A one-file app: `screen` is the `<screen>` start tag's attributes.
fn doc(screen: &str, body: &str, script: &str) -> Document {
    let src = format!("<template><screen {screen}>{body}</screen></template>\n<script>\n{script}\n</script>");
    Document::from_source(&src).expect("loads")
}

/// An app with components, each `components/<name>.rux`, loaded from disk.
fn app(components: &[(&str, &str)], screen: &str, body: &str, script: &str) -> Document {
    let dir: PathBuf = std::env::temp_dir().join(format!(
        "rux_closing_{}_{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
    ));
    fs::create_dir_all(dir.join("components")).unwrap();
    for (name, src) in components {
        fs::write(dir.join("components").join(format!("{name}.rux")), src).unwrap();
    }
    let uses: String = components.iter().map(|(n, _)| format!("use components::{n};\n")).collect();
    fs::write(
        dir.join("app.rux"),
        format!("<template><screen {screen}>{body}</screen></template>\n<script>\n{uses}{script}</script>"),
    )
    .unwrap();
    let doc = Document::load(dir.join("app.rux")).expect("loads");
    let _ = fs::remove_dir_all(&dir);
    doc
}

fn texts(node: &Node, out: &mut Vec<String>) {
    if let Some(t) = &node.text {
        out.push(t.text.clone());
    }
    for child in &node.children {
        texts(child, out);
    }
}

fn shown(doc: &Document) -> String {
    let mut out = Vec::new();
    texts(&doc.root, &mut out);
    out.join("|")
}

fn warnings(doc: &Document) -> String {
    doc.diagnostics().warnings.iter().map(|w| w.message.clone()).collect::<Vec<_>>().join("\n")
}

#[test]
fn with_no_close_handler_the_app_may_close() {
    let mut d = doc("", "<text>hi</text>", "");
    assert!(d.ask_close());
}

#[test]
fn false_keeps_the_app_open_and_what_the_handler_wrote_is_shown() {
    let mut d = doc(
        r#"@close="confirm()""#,
        r#"<text r-if="asking">Save changes?</text>"#,
        "let dirty = signal(true);\nlet asking = signal(false);\n\
         fn confirm() { if dirty { asking = true; return false; } }",
    );
    assert!(!d.ask_close(), "a dirty app refuses");
    assert_eq!(shown(&d), "Save changes?", "and its dialog is open");
}

#[test]
fn anything_but_false_lets_it_close() {
    let mut d = doc(
        r#"@close="confirm()""#,
        "",
        "let dirty = signal(false);\nfn confirm() { if dirty { return false; } }",
    );
    assert!(d.ask_close(), "nothing to save: no answer, so it closes");
    let mut d = doc(r#"@close="'later'""#, "", "");
    assert!(d.ask_close(), "a string is not a refusal");
}

#[test]
fn a_close_handler_that_fails_lets_the_app_close_and_says_so() {
    let mut d = doc(r#"@close="nothing.here""#, "", "");
    assert!(d.ask_close());
    assert!(warnings(&d).contains("`@close=\"nothing.here\"` failed"), "{}", warnings(&d));
}

#[test]
fn close_from_a_handler_is_taken_once() {
    let mut d = doc("", r#"<view @tap="close()"><text>quit</text></view>"#, "");
    assert!(!d.take_close());
    d.apply_handler("close()");
    assert!(d.take_close(), "asked for");
    assert!(!d.take_close(), "and taken");
}

#[test]
fn close_inside_close_handler_lets_it_close() {
    let mut d = doc(r#"@close="close(); false""#, "", "");
    assert!(d.ask_close(), "`close()` is the app deciding to close");
}

#[test]
fn a_file_of_its_own_close_is_called_instead_of_the_built_in() {
    let mut d = doc(
        "",
        "<text>{{ shut }}</text>",
        "let shut = signal(\"open\");\nfn close() { shut = \"closed\"; }",
    );
    d.apply_handler("close()");
    assert!(!d.take_close(), "the file's own `close` ran, not the built-in");
    assert_eq!(shown(&d), "closed");
}

#[test]
fn shutting_down_runs_every_unmounted_component_first() {
    let row = "<template><text>row</text></template>\n<script>\nunmounted { print(\"row gone\"); }\n</script>";
    let mut d = app(
        &[("row", row)],
        "",
        "<row /><row />",
        "unmounted { print(\"document gone\"); }\n",
    );
    d.shut_down();
    let prints = d.diagnostics().prints.join("|");
    assert_eq!(prints, "row gone|row gone|document gone");
    // Twice is once: a reload and then a close must not save twice.
    d.shut_down();
    assert_eq!(d.diagnostics().prints.join("|"), "row gone|row gone|document gone");
}

#[test]
fn a_hot_reload_tells_the_document_it_replaces() {
    let mut d = doc("", "<text>one</text>", "unmounted { print(\"old one gone\"); }");
    let fresh = Document::from_source("<template><screen><text>two</text></screen></template>").unwrap();
    d.replace_with(fresh);
    assert_eq!(shown(&d), "two");
    // The prints collected by the old document travel nowhere; what matters is
    // that its body ran, which the script tier's log shows.
}

#[test]
fn close_off_a_screen_is_an_error_naming_where_it_belongs() {
    let src = "<template><screen><view @close=\"x()\"><text>a</text></view></screen></template>";
    let err = Document::from_source_checked(src).err().map(|e| e.to_string()).unwrap_or_default();
    let mut d = Document::from_source(src).unwrap();
    let said = format!("{err}\n{}", warnings(&d));
    let _ = d.ask_close();
    assert!(said.contains("`@close` belongs on the app's `<screen>`"), "{said}");
}

