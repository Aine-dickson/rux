//! The declarations only a document or component script holds (`use`,
//! `prop`, `computed`, `effect`, `mounted`, `unmounted`), found by Rux's own
//! parser rather than line by line. See `docs/11-next.md`, step 2.

use rux_layout::Node;
use rux_runtime::Document;

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

/// A `computed` could only be one line while declarations were read line by
/// line. Parsed, it may run over as many as it needs.
#[test]
fn a_computed_may_run_over_several_lines() {
    let src = "<template><screen><text>{{ total }}</text></screen></template>\n\
        <script>\n\
        let qty = signal(2);\n\
        let price = signal(3);\n\
        computed total = qty *\n    price +\n    1;\n\
        let after = 1;\n\
        </script>";
    let doc = Document::from_source(src).expect("loads");
    assert!(shown(&doc).contains('7'), "{}", shown(&doc));
}

/// A lifecycle block is closed by its own `}`, so one holding a string with a
/// brace in it is read whole.
#[test]
fn a_brace_in_a_string_does_not_close_a_block() {
    let src = "<template><screen><text>{{ said }}</text></screen></template>\n\
        <script>\n\
        let said = signal(\"\");\n\
        mounted { said = \"{ok}\"; }\n\
        </script>";
    let doc = Document::from_source(src).expect("loads");
    assert!(shown(&doc).contains("{ok}"), "{}", shown(&doc));
}

/// A script that does not parse is reported where it goes wrong, not at the
/// first declaration above it, which is what a parser that did not know
/// `prop` or `computed` would have said.
#[test]
fn a_syntax_error_below_a_declaration_is_placed_on_its_own_line() {
    let src = "<template><screen><text>x</text></screen></template>\n\
        <script>\n\
        computed doubled = 2 * 2;\n\
        let n = ;\n\
        </script>";
    let Err(err) = Document::from_source_checked(src) else { panic!("loaded") };
    assert_eq!(err.line, Some(4), "{err}");
    assert!(!err.to_string().contains("computed"), "{err}");
}
