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

// ── Failures after the first build ───────────────────────────────────────────

fn tap(doc: &mut Document, label: &str) {
    fn texts_of(n: &rux_layout::Node) -> Vec<String> {
        let mut out: Vec<String> = n.text.iter().map(|t| t.text.trim().to_string()).collect();
        for c in &n.children {
            out.extend(texts_of(c));
        }
        out
    }
    fn find<'a>(n: &'a rux_layout::Node, l: &str) -> Option<&'a rux_layout::Node> {
        if n.on_tap.is_some() && texts_of(n).iter().any(|t| t == l) {
            return Some(n);
        }
        n.children.iter().find_map(|c| find(c, l))
    }
    let node = find(&doc.root, label).unwrap_or_else(|| panic!("nothing says {label}: {}", shown(doc))).clone();
    doc.apply_handler_in(&node.on_tap.clone().unwrap(), node.instance.as_deref());
}

#[test]
fn a_binding_that_starts_failing_after_a_change_shows_the_fallback() {
    let mut doc = load(
        "later-binding",
        r#"<view @tap="n = 0"><text>break</text></view>
<boundary>
  <text>[{{ show(n) }}]</text>
  <fallback><text>broken: {{ error.message }}</text></fallback>
</boundary>"#,
        &format!("{FAILS}\nlet n = signal(1);"),
        &[],
    );
    assert!(shown(&doc).contains("[row1]"), "{}", shown(&doc));
    tap(&mut doc, "break");
    let s = shown(&doc);
    assert!(s.contains("broken: zero") && !s.contains("[row"), "{s}");
}

const CRASHY: &str = "<template><view @tap=\"go()\"><text>crash</text></view></template>\n\
<script>\nfn go() { throw \"oops\"; }\n</script>";

#[test]
fn a_components_failing_handler_shows_the_fallback() {
    let mut doc = load(
        "later-handler",
        r#"<boundary>
  <crashy />
  <fallback><text>failed: {{ error.message }}</text></fallback>
</boundary>"#,
        "use components::crashy;",
        &[("components/crashy.rux", CRASHY)],
    );
    assert!(shown(&doc).contains("crash") && !shown(&doc).contains("failed"), "{}", shown(&doc));
    tap(&mut doc, "crash");
    let s = shown(&doc);
    assert!(s.contains("failed: oops") && !s.contains("crash|") && !s.starts_with("crash"), "{s}");
}

#[test]
fn a_pages_own_failing_handler_inside_a_boundary_shows_the_fallback() {
    let mut doc = load(
        "page-handler",
        r#"<text>outside</text>
<boundary>
  <view @tap="boom()"><text>page crash</text></view>
  <fallback><text>failed: {{ error.message }}</text></fallback>
</boundary>"#,
        FAILS,
        &[],
    );
    tap(&mut doc, "page crash");
    let s = shown(&doc);
    assert!(s.contains("failed: no orders") && !s.contains("page crash") && s.contains("outside"), "{s}");
}

#[test]
fn a_pages_handler_outside_every_boundary_still_only_reports() {
    let mut doc = load(
        "page-handler-outside",
        r#"<view @tap="boom()"><text>outside crash</text></view>
<boundary><text>[{{ fine() }}]</text><fallback><text>failed</text></fallback></boundary>"#,
        FAILS,
        &[],
    );
    tap(&mut doc, "outside crash");
    let s = shown(&doc);
    assert!(s.contains("outside crash") && s.contains("[fine]") && !s.contains("failed"), "{s}");
}

#[test]
fn a_page_handler_in_a_nested_boundary_fails_the_innermost() {
    let mut doc = load(
        "page-handler-nested",
        r#"<boundary>
  <text>outer ok</text>
  <boundary>
    <view @tap="boom()"><text>inner crash</text></view>
    <fallback><text>inner failed</text></fallback>
  </boundary>
  <fallback><text>outer failed</text></fallback>
</boundary>"#,
        FAILS,
        &[],
    );
    tap(&mut doc, "inner crash");
    let s = shown(&doc);
    assert!(s.contains("outer ok") && s.contains("inner failed") && !s.contains("outer failed"), "{s}");
}

#[test]
fn a_failing_async_fn_of_a_component_shows_the_fallback() {
    let mut doc = load(
        "later-async",
        r#"<boundary>
  <loader />
  <fallback><text>load failed: {{ error.message }}</text></fallback>
</boundary>"#,
        "use components::loader;",
        &[(
            "components/loader.rux",
            "<template><view @tap=\"load()\"><text>load</text></view></template>\n\
             <script>\nasync fn load() { throw \"no network\"; }\n</script>",
        )],
    );
    tap(&mut doc, "load");
    assert!(shown(&doc).contains("load failed: no network"), "{}", shown(&doc));
}

#[test]
fn a_components_failing_effect_shows_the_fallback() {
    let mut doc = load(
        "later-effect",
        r#"<view @tap="go = 1"><text>trigger</text></view>
<boundary>
  <watcher :go="go" />
  <fallback><text>watch failed: {{ error.message }}</text></fallback>
</boundary>"#,
        "use components::watcher;\nlet go = signal(0);",
        &[(
            "components/watcher.rux",
            "<template><text>watching</text></template>\n\
             <script>\nprop go: int;\neffect { if go == 1 { throw \"effect broke\"; } }\n</script>",
        )],
    );
    assert!(shown(&doc).contains("watching"), "{}", shown(&doc));
    tap(&mut doc, "trigger");
    assert!(shown(&doc).contains("watch failed: effect broke"), "{}", shown(&doc));
}

#[test]
fn the_instances_in_a_failed_boundary_unmount() {
    let mut doc = load(
        "unmounts",
        r#"<boundary>
  <crashy />
  <fallback><text>failed</text></fallback>
</boundary>"#,
        "use components::crashy;",
        &[(
            "components/crashy.rux",
            "<template><view @tap=\"go()\"><text>crash</text></view></template>\n\
             <script>\nmounted { print(\"up\"); }\nunmounted { print(\"down\"); }\nfn go() { throw \"oops\"; }\n</script>",
        )],
    );
    // What `mounted` printed while loading is not what is asked about.
    let _ = rux_runtime::take_prints();
    tap(&mut doc, "crash");
    let printed = format!("{:?} {:?}", doc.diagnostics().prints, rux_runtime::take_prints());
    assert!(printed.contains("down"), "{printed}");
    assert!(shown(&doc).contains("failed"), "{}", shown(&doc));
}

// ── retry() ──────────────────────────────────────────────────────────────────

const RETRY_PAGE: &str = r#"<view @tap="n = 0"><text>break</text></view>
<boundary>
  <text>[{{ show(n) }}]</text>
  <fallback>
    <text>broken: {{ error.message }}</text>
    <view @tap="n = 1; retry()"><text>fix and retry</text></view>
    <view @tap="retry()"><text>retry as it is</text></view>
  </fallback>
</boundary>"#;

#[test]
fn retry_builds_the_children_again() {
    let mut doc = load("retry", RETRY_PAGE, &format!("{FAILS}\nlet n = signal(1);"), &[]);
    tap(&mut doc, "break");
    assert!(shown(&doc).contains("broken: zero"), "{}", shown(&doc));
    tap(&mut doc, "fix and retry");
    let s = shown(&doc);
    assert!(s.contains("[row1]") && !s.contains("broken"), "{s}");
}

#[test]
fn a_retry_that_fails_again_shows_the_fallback_again() {
    let mut doc = load("retry-again", RETRY_PAGE, &format!("{FAILS}\nlet n = signal(1);"), &[]);
    tap(&mut doc, "break");
    tap(&mut doc, "retry as it is");
    let s = shown(&doc);
    assert!(s.contains("broken: zero") && !s.contains("[row"), "{s}");
}

#[test]
fn a_retry_makes_new_instances_that_mount_again() {
    let mut doc = load(
        "retry-mounts",
        r#"<boundary>
  <crashy />
  <fallback><view @tap="retry()"><text>again</text></view></fallback>
</boundary>"#,
        "use components::crashy;",
        &[(
            "components/crashy.rux",
            "<template><view @tap=\"go()\"><text>crash</text></view></template>\n\
             <script>\nmounted { print(\"up\"); }\nfn go() { throw \"oops\"; }\n</script>",
        )],
    );
    let _ = rux_runtime::take_prints();
    tap(&mut doc, "crash");
    tap(&mut doc, "again");
    let printed = format!("{:?} {:?}", doc.diagnostics().prints, rux_runtime::take_prints());
    assert!(printed.contains("up"), "{printed}");
    assert!(shown(&doc).contains("crash"), "{}", shown(&doc));
}

#[test]
fn retry_outside_a_fallback_does_nothing() {
    let mut doc = load(
        "retry-outside",
        r#"<view @tap="retry()"><text>retry</text></view><boundary><text>[{{ fine() }}]</text></boundary>"#,
        FAILS,
        &[],
    );
    tap(&mut doc, "retry");
    assert!(shown(&doc).contains("[fine]"), "{}", shown(&doc));
}

#[test]
fn a_retry_in_a_nested_fallback_retries_the_innermost_boundary() {
    let mut doc = load(
        "retry-nested",
        r#"<view @tap="n = 0"><text>break</text></view>
<boundary>
  <text>outer ok</text>
  <boundary>
    <text>[{{ show(n) }}]</text>
    <fallback><view @tap="n = 1; retry()"><text>inner retry</text></view></fallback>
  </boundary>
  <fallback><text>outer failed</text></fallback>
</boundary>"#,
        &format!("{FAILS}\nlet n = signal(1);"),
        &[],
    );
    tap(&mut doc, "break");
    assert!(shown(&doc).contains("outer ok") && !shown(&doc).contains("outer failed"), "{}", shown(&doc));
    tap(&mut doc, "inner retry");
    assert!(shown(&doc).contains("[row1]"), "{}", shown(&doc));
}

#[test]
fn the_boundary_a_handler_sits_in_is_read_back_from_its_prelude() {
    let one = "let __boundary = \"|0.1.#a\\\"b\"; let error = 1; retry()";
    assert_eq!(rux_style::boundary_of_handler(one), Some("|0.1.#a\"b".to_string()));
    let nested = "let __boundary = \"outer\"; let __boundary = \"inner\"; retry()";
    assert_eq!(rux_style::boundary_of_handler(nested), Some("inner".to_string()));
    assert_eq!(rux_style::boundary_of_handler("count = count + 1"), None);
}

#[test]
fn a_failure_after_the_first_build_is_still_reported_once_the_fallback_shows() {
    let mut doc = load(
        "later-reported",
        r#"<boundary>
  <crashy />
  <fallback><text>failed</text></fallback>
</boundary>"#,
        "use components::crashy;",
        &[("components/crashy.rux", CRASHY)],
    );
    tap(&mut doc, "crash");
    assert!(shown(&doc).contains("failed"), "{}", shown(&doc));
    let said: Vec<String> = doc.diagnostics().warnings.iter().map(|w| w.message.clone()).collect();
    assert!(said.iter().any(|m| m.contains("oops")), "{said:?}");
}

#[test]
fn a_binding_failure_after_a_change_is_still_reported_once_the_fallback_shows() {
    let mut doc = load(
        "later-binding-reported",
        r#"<view @tap="n = 0"><text>break</text></view>
<boundary><text>[{{ show(n) }}]</text><fallback><text>failed</text></fallback></boundary>"#,
        &format!("{FAILS}\nlet n = signal(1);"),
        &[],
    );
    tap(&mut doc, "break");
    assert!(shown(&doc).contains("failed"), "{}", shown(&doc));
    let said: Vec<String> = doc.diagnostics().warnings.iter().map(|w| w.message.clone()).collect();
    assert!(said.iter().any(|m| m.contains("zero")), "{said:?}");
}
