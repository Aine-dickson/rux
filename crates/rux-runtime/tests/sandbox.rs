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
    // Said where any failing handler says it, until the next rebuild reads it.
    let said: Vec<String> = rux_script::take_warnings().into_iter().map(|w| w.message).collect();
    assert!(said.iter().any(|m| m.contains("state")), "{said:?}");
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
