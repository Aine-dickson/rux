//! The document's tree is numbered, and the number moves on every write, so
//! the layout can tell a frame that only scrolled from one that changed
//! anything (AW-018).

use rux_runtime::Document;

const SRC: &str = "<template><screen><view @tap=\"n++\"><text>{{ n }}</text></view></screen></template>\n\
                   <script>\nlet n = signal(0);\n</script>";

#[test]
fn reading_keeps_the_number_and_a_change_moves_it() {
    let mut doc = Document::from_source(SRC).expect("loads");
    let first = doc.root.revision();
    let _ = doc.root.children.len();
    assert_eq!(doc.root.revision(), first, "a read is not a write");
    assert!(doc.apply_handler("n++"));
    assert_ne!(doc.root.revision(), first, "a patch wrote the tree");
}

#[test]
fn a_document_loaded_again_has_a_number_of_its_own() {
    let a = Document::from_source(SRC).expect("loads");
    let b = Document::from_source(SRC).expect("loads");
    assert_ne!(a.root.revision(), b.root.revision(), "a hot reload must not look like the same tree");
}
