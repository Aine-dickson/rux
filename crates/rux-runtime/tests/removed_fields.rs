//! A field whose component is gone is read as nothing and takes no writes.
//!
//! The shell keeps a field's identity, `(model, row, instance)`, past the
//! frame that removed its component: Android's autofill list, an input
//! method's last commit, a picker that answers late. Read or written then,
//! the field's model fell back to the page's scope: a read failed with
//! "`email` is not defined" (Urbandrop's UD-012 logcat), and a write landed
//! in a page signal that happened to have the same name.

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

const PAGE: &str = "<template><screen><text>[{{ email }}]</text></screen></template>\n\
<script>\nlet email = signal(\"page@example.com\");\n</script>";

/// An instance key no component holds, as after the `r-if` that showed it
/// went false.
const GONE: Option<&str> = Some("0.9.9.");

#[test]
fn a_removed_components_field_reads_as_nothing_and_warns_nothing() {
    let mut doc = Document::from_source(PAGE).expect("loads");
    let _ = rux_runtime::take_warnings();
    assert_eq!(doc.value_in("email", None, GONE), "");
    assert_eq!(doc.value_in("code", None, GONE), "");
    assert_eq!(doc.typed_value_in("code", None, GONE), None);
    let warned: Vec<String> = rux_runtime::take_warnings().into_iter().map(|w| w.message).collect();
    assert!(warned.is_empty(), "{warned:?}");
}

#[test]
fn a_removed_components_field_does_not_write_the_pages_signal_of_the_same_name() {
    let mut doc = Document::from_source(PAGE).expect("loads");
    doc.apply_edit_in("email", None, GONE, "typed@late.com");
    assert!(shown(&doc).contains("[page@example.com]"), "{}", shown(&doc));
    // The page's own field still writes it.
    doc.apply_edit_in("email", None, None, "page@new.com");
    assert!(shown(&doc).contains("[page@new.com]"), "{}", shown(&doc));
}
