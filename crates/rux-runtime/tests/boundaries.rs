//! Props checked when the program runs, and `x is T`. See `docs/10-types.md`,
//! "Props" and "Boundaries".
//!
//! The checker sees a tag's props when it can, but not a value that was `any`,
//! one from a route, or one from a file checked on its own. Those are held to
//! the prop's declared type as the component is built, in release builds too.

use std::fs;
use std::path::PathBuf;

use rux_runtime::Document;

static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

fn project(files: &[(&str, &str)]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "rux_boundaries_{}_{}",
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

fn errors(doc: &Document) -> Vec<String> {
    doc.diagnostics().warnings.iter().filter(|w| w.is_error()).map(|w| w.message.clone()).collect()
}

fn texts(node: &rux_layout::Node, out: &mut Vec<String>) {
    if let Some(t) = &node.text {
        out.push(t.text.clone());
    }
    for child in &node.children {
        texts(child, out);
    }
}

fn shown(doc: &Document) -> Vec<String> {
    let mut out = Vec::new();
    texts(&doc.root, &mut out);
    out
}

const STAT: &str = "<template><view><text>{{ label }}={{ value }}</text></view></template>\n\
                    <script>\n  prop label: string;\n  prop value: number = 0;\n</script>";

fn with_stat(tag: &str, script: &str) -> Document {
    let dir = project(&[
        (
            "app.rux",
            &format!(
                "<template><screen>{tag}</screen></template>\n\
                 <script>\nuse components::stat;\n{script}\n</script>"
            ),
        ),
        ("components/stat.rux", STAT),
    ]);
    Document::load(dir.join("app.rux")).expect("loads")
}

/// An `any` gets past the checker; the build is where it is caught, and the
/// default stands in for it.
#[test]
fn a_prop_that_does_not_fit_is_reported_and_left_out() {
    let doc = with_stat("<stat label=\"a\" :value=\"raw\" />", "let raw: any = \"lots\";");
    let found = errors(&doc);
    assert!(
        found.iter().any(|e| e.contains("`<stat>` was given the text \"lots\" for `:value`")
            && e.contains("not the `number`")
            && e.contains("takes its default")),
        "{found:?}"
    );
    assert!(shown(&doc).contains(&"a=0".to_string()), "the default in its place: {:?}", shown(&doc));

    let doc = with_stat("<stat label=\"a\" :value=\"raw\" />", "let raw: any = 5;");
    assert!(errors(&doc).is_empty(), "{:?}", errors(&doc));
    assert!(shown(&doc).contains(&"a=5".to_string()), "{:?}", shown(&doc));
}

/// A prop may name a type the component imports, and the check walks it.
#[test]
fn a_named_prop_type_is_walked() {
    let card = "<template><view><text>{{ task.title }}</text></view></template>\n\
                <script>\n  use types::Task;\n  prop task: Task;\n</script>";
    let types = "<script>\ntype Tag = \"home\" | \"work\";\n\
                 type Task = { id: int, title: string, tag: Tag };\n</script>";
    let page = |value: &str| {
        format!(
            "<template><screen><card :task=\"t\" /></screen></template>\n\
             <script>\nuse components::card;\nlet t: any = {value};\n</script>"
        )
    };
    let load = |value: &str| {
        let dir = project(&[
            ("app.rux", &page(value)),
            ("components/card.rux", card),
            ("types.rux", types),
        ]);
        Document::load(dir.join("app.rux")).expect("loads")
    };
    let doc = load("{ id: 1, title: \"ok\", tag: \"home\" }");
    assert!(errors(&doc).is_empty(), "{:?}", errors(&doc));
    assert!(shown(&doc).contains(&"ok".to_string()), "{:?}", shown(&doc));

    let doc = load("{ id: 1, title: \"bad\", tag: \"play\" }");
    let found = errors(&doc);
    assert!(
        found.iter().any(|e| e.contains("`<card>` was given a map for `:task`") && e.contains("`Task`")),
        "{found:?}"
    );
    assert!(!shown(&doc).contains(&"bad".to_string()), "not built with it: {:?}", shown(&doc));
}

/// A route segment is text; a prop that says `int` gets a number.
#[test]
fn a_route_parameter_becomes_what_its_prop_takes() {
    let detail = "<template><view><text>{{ id + 1 }}</text></view></template>\n\
                  <script>\n  prop id: int = 0;\n</script>";
    let dir = project(&[
        (
            "app.rux",
            "<template><screen><router><route path=\"/\" view=\"detail\" />\
             <route path=\"/task/:id\" view=\"detail\" /></router></screen></template>\n\
             <script>\nuse components::detail;\n</script>",
        ),
        ("components/detail.rux", detail),
    ]);
    let mut doc = Document::load(dir.join("app.rux")).expect("loads");
    assert!(doc.navigate("/task/41"));
    assert!(errors(&doc).is_empty(), "{:?}", errors(&doc));
    assert!(shown(&doc).contains(&"42".to_string()), "a number, not \"411\": {:?}", shown(&doc));

    assert!(doc.navigate("/task/abc"));
    let found = errors(&doc);
    assert!(
        found.iter().any(|e| e.contains("the route gave `<detail>` the text \"abc\" for `id`")
            && e.contains("cannot be the `int`")),
        "{found:?}"
    );
    assert!(shown(&doc).contains(&"1".to_string()), "the default in its place: {:?}", shown(&doc));
}

/// `is` in a document, against a type it imports.
#[test]
fn is_checks_an_imported_type_when_the_program_runs() {
    let dir = project(&[
        (
            "app.rux",
            "<template><screen><text>{{ verdict }}</text></screen></template>\n\
             <script>\nuse types::Task;\n\
             let raw: any = { id: 1, title: \"x\", tag: \"work\" };\n\
             let verdict = signal(\"\");\n\
             mounted {\n  verdict = if raw is Task { \"task\" } else { \"not\" };\n}\n</script>",
        ),
        (
            "types.rux",
            "<script>\ntype Tag = \"home\" | \"work\";\n\
             type Task = { id: int, title: string, tag: Tag };\n</script>",
        ),
    ]);
    let doc = Document::load(dir.join("app.rux")).expect("loads");
    assert!(errors(&doc).is_empty(), "{:?}", errors(&doc));
    assert!(shown(&doc).contains(&"task".to_string()), "{:?}", shown(&doc));
}

/// `rux check` visits `/task/:id` with a placeholder segment. It is not a bad
/// parameter, and the view is built with a stand-in of the prop's type.
#[test]
fn the_checkers_placeholder_segment_is_not_a_bad_prop() {
    let detail = "<template><view><text>{{ id + 1 }}</text></view></template>\n\
                  <script>\n  prop id: int;\n</script>";
    let dir = project(&[
        (
            "app.rux",
            "<template><screen><router><route path=\"/task/:id\" view=\"detail\" />\
             </router></screen></template>\n\
             <script>\nuse components::detail;\n</script>",
        ),
        ("components/detail.rux", detail),
    ]);
    let mut doc = Document::load(dir.join("app.rux")).expect("loads");
    let visits = doc.route_visits();
    assert_eq!(visits, vec![format!("/task/{}", Document::CHECK_PARAM)]);
    doc.navigate(&visits[0]);
    assert!(errors(&doc).is_empty(), "{:?}", errors(&doc));
    assert!(shown(&doc).contains(&"1".to_string()), "a stand-in 0: {:?}", shown(&doc));
}
