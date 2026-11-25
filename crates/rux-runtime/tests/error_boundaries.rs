//! `<boundary>`: a subtree that shows its `<fallback>` when something inside it
//! fails (roadmap v0.10, error boundaries).
//!
//! The failure is still reported, as it always was: the overlay and `rux check`
//! hear of it. A boundary changes what the person sees, not what the author is
//! told.

use rux_runtime::Document;

fn texts(node: &rux_layout::Node, out: &mut Vec<String>) {
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

/// A document with `body` as its template and `script` as its script, and the
/// components named by `parts` (path, source) beside it.
fn load(name: &str, body: &str, script: &str, parts: &[(&str, &str)]) -> Document {
    let dir = std::env::temp_dir().join(format!("rux_error_boundaries_{name}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let app = format!("<template><screen>{body}</screen></template>\n<script>\n{script}\n</script>");
    let mut files: Vec<(String, String)> = vec![("app.rux".into(), app)];
    files.extend(parts.iter().map(|(p, s)| (p.to_string(), s.to_string())));
    for (rel, text) in &files {
        std::fs::create_dir_all(dir.join(rel).parent().unwrap()).unwrap();
        std::fs::write(dir.join(rel), text).unwrap();
    }
    Document::load(dir.join("app.rux")).expect("loads")
}

const FAILS: &str = r#"
fn boom(): string { throw { message: "no orders", kind: "network" }; }
fn fine(): string { return "fine"; }
fn show(n: int): string { if n == 0 { throw "zero"; } return "row" + String(n); }
"#;

#[test]
fn a_failing_binding_shows_the_fallback_with_the_error() {
    let doc = load(
        "binding",
        r#"<text>before</text>
<boundary>
  <text>[{{ boom() }}]</text>
  <fallback><text>failed: {{ error.message }} ({{ error.kind }})</text></fallback>
</boundary>
<text>after</text>"#,
        FAILS,
        &[],
    );
    let s = shown(&doc);
    assert!(s.contains("failed: no orders (network)"), "{s}");
    assert!(!s.contains("[") , "the failed children are not shown: {s}");
    assert!(s.starts_with("before") && s.ends_with("after"), "{s}");
}

#[test]
fn a_boundary_that_does_not_fail_shows_its_children_and_not_the_fallback() {
    let doc = load(
        "clean",
        r#"<boundary>
  <text>[{{ fine() }}]</text>
  <fallback><text>failed</text></fallback>
</boundary>"#,
        FAILS,
        &[],
    );
    let s = shown(&doc);
    assert!(s.contains("[fine]") && !s.contains("failed"), "{s}");
}

#[test]
fn a_failure_is_still_reported() {
    let doc = load(
        "reported",
        r#"<boundary><text>{{ boom() }}</text><fallback><text>failed</text></fallback></boundary>"#,
        FAILS,
        &[],
    );
    let said: Vec<String> = doc.diagnostics().warnings.iter().map(|w| w.message.clone()).collect();
    assert!(said.iter().any(|m| m.contains("no orders")), "{said:?}");
}

#[test]
fn a_boundary_without_a_fallback_shows_nothing_when_it_fails() {
    let doc = load(
        "nofallback",
        r#"<text>a</text><boundary><text>[{{ boom() }}]</text></boundary><text>b</text>"#,
        FAILS,
        &[],
    );
    assert_eq!(shown(&doc), "a|b");
}

#[test]
fn a_failure_outside_a_boundary_is_not_caught_by_a_neighbour() {
    let doc = load(
        "neighbour",
        r#"<text>[{{ boom() }}]</text>
<boundary><text>ok {{ fine() }}</text><fallback><text>failed</text></fallback></boundary>"#,
        FAILS,
        &[],
    );
    let s = shown(&doc);
    assert!(s.contains("ok fine") && !s.contains("failed"), "{s}");
}

#[test]
fn the_innermost_boundary_catches_and_the_outer_one_carries_on() {
    let doc = load(
        "nested",
        r#"<boundary>
  <text>outer {{ fine() }}</text>
  <boundary>
    <text>[{{ boom() }}]</text>
    <fallback><text>inner failed</text></fallback>
  </boundary>
  <fallback><text>outer failed</text></fallback>
</boundary>"#,
        FAILS,
        &[],
    );
    let s = shown(&doc);
    assert!(s.contains("outer fine") && s.contains("inner failed") && !s.contains("outer failed"), "{s}");
}

#[test]
fn a_failure_inside_a_fallback_goes_to_the_boundary_around_it() {
    let doc = load(
        "fallback-fails",
        r#"<boundary>
  <text>outer content {{ fine() }}</text>
  <boundary>
    <text>{{ boom() }}</text>
    <fallback><text>{{ boom() }}</text></fallback>
  </boundary>
  <fallback><text>outer failed</text></fallback>
</boundary>"#,
        FAILS,
        &[],
    );
    let s = shown(&doc);
    assert!(s.contains("outer failed") && !s.contains("outer content"), "{s}");
}

#[test]
fn a_component_whose_script_fails_is_caught_and_discarded() {
    let doc = load(
        "component",
        r#"<boundary>
  <panel />
  <fallback><text>panel failed: {{ error.message }}</text></fallback>
</boundary>"#,
        "use components::panel;",
        &[(
            "components/panel.rux",
            "<template><view><text>panel body</text></view></template>\n<script>\nlet items = signal(missing_thing);\n</script>",
        )],
    );
    let s = shown(&doc);
    assert!(s.contains("panel failed:") && !s.contains("panel body"), "{s}");
}

#[test]
fn each_row_of_a_list_has_its_own_boundary() {
    let doc = load(
        "rows",
        r#"<view r-for="n in rows">
  <boundary>
    <text>{{ show(n) }}</text>
    <fallback><text>row {{ n }} failed</text></fallback>
  </boundary>
</view>"#,
        &format!("{FAILS}\nlet rows = signal([1, 0, 2]);"),
        &[],
    );
    let s = shown(&doc);
    assert!(s.contains("row1") && s.contains("row 0 failed") && s.contains("row2"), "{s}");
    assert!(!s.contains("row 1 failed") && !s.contains("row 2 failed"), "{s}");
}

#[test]
fn a_directive_inside_a_boundary_still_works() {
    let doc = load(
        "directive",
        r#"<boundary>
  <text r-if="on">shown</text>
  <text r-else>hidden</text>
  <text r-for="n in rows">item {{ n }}</text>
  <fallback><text>failed</text></fallback>
</boundary>"#,
        "let on = signal(true);\nlet rows = signal([1, 2]);",
        &[],
    );
    let s = shown(&doc);
    assert!(s.contains("shown") && !s.contains("hidden") && s.contains("item 1") && s.contains("item 2"), "{s}");
    assert!(!s.contains("failed"), "{s}");
}

#[test]
fn a_fallback_that_is_not_the_last_child_or_that_comes_twice_is_an_error() {
    for (name, body) in [
        ("first", "<boundary><fallback><text>f</text></fallback><text>a</text></boundary>"),
        ("twice", "<boundary><text>a</text><fallback><text>f</text></fallback><fallback><text>g</text></fallback></boundary>"),
    ] {
        let doc = load(name, body, FAILS, &[]);
        let errors: Vec<String> =
            doc.diagnostics().warnings.iter().filter(|w| w.is_error()).map(|w| w.message.clone()).collect();
        assert!(errors.iter().any(|m| m.contains("one `<fallback>`, as its last child")), "{name}: {errors:?}");
    }
}
