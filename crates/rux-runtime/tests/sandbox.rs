//! A document opened under a sandbox (roadmap v0.10 item 2): what the opener
//! grants it, and what it may spend. The script half is tested in
//! `rux-script/tests/sandbox.rs`; this is what a `Document` adds: markup, the
//! rows a list builds, and what a handler leaves behind.

use rux_runtime::{Document, Sandbox};

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

fn source(body: &str, script: &str) -> String {
    format!("<template><screen>{body}</screen></template>\n<script>\n{script}\n</script>")
}

fn warnings(doc: &Document) -> String {
    doc.diagnostics().warnings.iter().map(|w| w.message.clone()).collect::<Vec<_>>().join("; ")
}

#[test]
fn an_ungranted_module_stops_a_sandboxed_document_loading() {
    let src = source("<text>hi</text>", "fn copy() { clipboard.writeText(\"x\"); }");
    let err = Document::from_source_sandboxed(&src, Sandbox::new()).err().expect("refused");
    assert!(err.to_string().contains("`clipboard`"), "{err}");
    assert!(Document::from_source_sandboxed(&src, Sandbox::new().grant("clipboard")).is_ok());
    // And a trusted document is as it was.
    assert!(Document::from_source(&src).is_ok());
}

#[test]
fn a_list_builds_no_more_rows_than_the_sandbox_allows_and_says_so() {
    let rows = (0..20).map(|n| n.to_string()).collect::<Vec<_>>().join(", ");
    let src = source(
        "<text r-for=\"n in rows\">row</text>",
        &format!("let rows = [{rows}];"),
    );
    let mut sandbox = Sandbox::new();
    sandbox.limits.max_nodes = 5;
    let doc = Document::from_source_sandboxed(&src, sandbox).expect("loads");
    assert_eq!(shown(&doc).iter().filter(|t| *t == "row").count(), 5);
    assert!(warnings(&doc).contains("5 elements"), "{}", warnings(&doc));
    // Trusted, all twenty.
    let doc = Document::from_source(&src).expect("loads");
    assert_eq!(shown(&doc).iter().filter(|t| *t == "row").count(), 20);
}

#[test]
fn a_document_past_the_size_or_nesting_limit_is_refused_before_it_is_parsed() {
    let deep = format!("{}<text>x</text>{}", "<view>".repeat(5000), "</view>".repeat(5000));
    let src = source(&deep, "let a = 1;");
    let mut sandbox = Sandbox::new();
    sandbox.limits.max_nesting = 64;
    let err = Document::from_source_sandboxed(&src, sandbox).err().expect("refused");
    assert!(err.to_string().contains("64"), "{err}");

    let mut sandbox = Sandbox::new();
    sandbox.limits.max_source_bytes = 200;
    let big = source("<text>x</text>", &"let a = 1;\n".repeat(100));
    let err = Document::from_source_sandboxed(&big, sandbox).err().expect("refused");
    assert!(err.to_string().contains("200"), "{err}");
}

#[test]
fn a_handler_that_takes_the_state_past_its_limit_is_undone() {
    let src = source("<text>{{ log.length }}</text>", "let log: string[] = signal([]);");
    let mut sandbox = Sandbox::new();
    sandbox.limits.state_bytes = 100_000;
    let mut doc = Document::from_source_sandboxed(&src, sandbox).expect("loads");
    for _ in 0..2 {
        doc.apply_handler_in("log.push(\"x\".repeat(40000))", None);
    }
    assert_eq!(shown(&doc), vec!["2".to_string()]);
    let _ = rux_script::take_warnings();
    doc.apply_handler_in("log.push(\"x\".repeat(40000))", None);
    assert_eq!(shown(&doc), vec!["2".to_string()], "the third was undone");
    // Said where any failing handler says it: on the panel, after the tap.
    assert!(warnings(&doc).contains("state"), "{}", warnings(&doc));
}

#[test]
fn a_remote_image_is_not_fetched_without_the_images_grant() {
    let src = source("<image src=\"https://example.invalid/a.png\" />", "let a = 1;");
    let _ = Document::from_source_sandboxed(&src, Sandbox::new()).expect("loads");
    assert!(!rux_runtime::remote_images_allowed());
    let _ = Document::from_source_sandboxed(&src, Sandbox::new().grant("images")).expect("loads");
    assert!(rux_runtime::remote_images_allowed());
    let _ = Document::from_source(&src).expect("loads");
    assert!(rux_runtime::remote_images_allowed(), "a trusted document fetches as it always did");
}

#[test]
fn markup_that_reads_an_ungranted_window_is_told_who_grants_it() {
    let src = source("<text>{{ window.maximized }}</text>", "let n = 1;");
    let said = match Document::from_source_sandboxed(&src, Sandbox::new()) {
        Err(err) => err.to_string(),
        Ok(doc) => format!("{}; {:?}", warnings(&doc), doc.diagnostics().error),
    };
    assert!(said.contains("`window` is not available in this document"), "{said}");
    assert!(!said.contains("not defined"), "{said}");
    // Granted, it reads.
    let doc = Document::from_source_sandboxed(&src, Sandbox::new().grant("window")).expect("loads");
    assert!(!warnings(&doc).contains("window"), "{}", warnings(&doc));
}

#[test]
fn a_tapped_loop_that_never_ends_is_stopped_and_said() {
    let src = source(
        "<text>{{ n }}</text>",
        "let n = signal(0);\nfn spin() { while true { n = n + 1; } }",
    );
    let mut sandbox = Sandbox::new();
    sandbox.limits.steps_per_run = 10_000;
    let mut doc = Document::from_source_sandboxed(&src, sandbox).expect("loads");
    let _ = rux_script::take_warnings();
    doc.apply_handler_in("spin()", None);
    // Said on the panel after the tap itself, though the tap only patched:
    // the web has no stderr, and a stopped tap said nothing there.
    assert!(warnings(&doc).contains("stopped after 10000 steps"), "{}", warnings(&doc));
    // And still there after the next tap, said once.
    doc.apply_handler_in("n = n + 1", None);
    assert_eq!(warnings(&doc).matches("stopped after").count(), 1, "{}", warnings(&doc));
}

#[test]
fn a_trusted_tap_that_fails_is_said_on_the_panel_too() {
    let src = source("<text>{{ n }}</text>", "let n = signal(0);");
    let mut doc = Document::from_source(&src).expect("loads");
    // Says it changed something, so the shell draws a frame with the panel.
    assert!(doc.apply_handler_in("n = nope + 1", None), "a frame is drawn for the panel");
    assert!(warnings(&doc).contains("`nope`"), "{}", warnings(&doc));
    // The same failure again adds nothing new to draw.
    assert!(!doc.apply_handler_in("n = nope + 1", None));
}

#[test]
fn a_failed_tap_stays_listed_when_the_press_is_restyled() {
    use rux_runtime::InteractionState;
    let src = "<template><screen><view class=\"b\" @tap=\"n = nope + 1\"><text>{{ n }}</text></view></screen></template>\n\
               <script>\nlet n = signal(0);\n</script>\n<style>\n.b:active { opacity: 0.5; }\n</style>";
    let mut doc = Document::from_source(src).expect("loads");
    let pressed = InteractionState { active: Some(vec![0]), ..InteractionState::default() };
    doc.set_interaction(pressed);
    assert!(doc.apply_handler_in("n = nope + 1", None));
    // The release restyles the box, which builds the list again.
    assert!(doc.set_interaction(InteractionState::default()), "the release restyles");
    assert!(warnings(&doc).contains("`nope`"), "kept past the restyle: {}", warnings(&doc));
    // A later tap that raises something else replaces it.
    doc.apply_handler_in("n = other + 1", None);
    let said = warnings(&doc);
    assert!(said.contains("`other`") && !said.contains("`nope`"), "{said}");
}

#[test]
fn a_tap_that_writes_before_it_fails_stays_listed() {
    use rux_runtime::InteractionState;
    // The write rebuilds inside the tap, before the tap is over.
    let src = "<template><screen><view class=\"b\"><text>{{ n }}</text></view></screen></template>\n\
               <script>\nlet n = signal(0);\nfn go() { n = 1; let x = nope + 1; }\n</script>\n<style>\n.b:active { opacity: 0.5; }\n</style>";
    let mut doc = Document::from_source(src).expect("loads");
    doc.set_interaction(InteractionState { active: Some(vec![0]), ..InteractionState::default() });
    assert!(doc.apply_handler_in("go()", None));
    doc.set_interaction(InteractionState::default());
    assert!(warnings(&doc).contains("`nope`"), "kept past the restyle: {}", warnings(&doc));
    assert_eq!(shown(&doc), vec!["1".to_string()]);
}

#[test]
fn a_tap_caught_by_a_boundary_is_still_listed_after_the_press_is_restyled() {
    use rux_runtime::InteractionState;
    // The boundary's rebuild runs inside the tap and reads the sinks itself.
    let src = "<template><screen><view class=\"b\"><boundary><view @tap=\"boom()\"><text>go</text></view>\
               <fallback><text>failed</text></fallback></boundary></view></screen></template>\n\
               <script>\nfn boom() { throw \"no orders\"; }\n</script>\n<style>\n.b:active { opacity: 0.5; }\n</style>";
    let mut doc = Document::from_source(src).expect("loads");
    doc.set_interaction(InteractionState { active: Some(vec![0]), ..InteractionState::default() });
    fn on_tap(node: &rux_layout::Node) -> Option<String> {
        node.on_tap.clone().or_else(|| node.children.iter().find_map(on_tap))
    }
    let handler = on_tap(&doc.root).expect("a handler");
    doc.apply_handler_in(&handler, None);
    assert_eq!(shown(&doc), vec!["failed".to_string()]);
    doc.set_interaction(InteractionState::default());
    assert!(warnings(&doc).contains("no orders"), "kept past the restyle: {}", warnings(&doc));
}
