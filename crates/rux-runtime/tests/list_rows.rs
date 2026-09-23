//! `r-model` on a loop variable writes into the real list.
//!
//! Inside `r-for="item in items"`, `item` is a copy of its row. A model of
//! `item.name` used to write to that copy, so typing, pasting and cutting in a
//! list row were all thrown away on the next build, silently. Found on the
//! phone in inputs phase 5. The typing itself is the shell's and is driven by
//! hand; what the document does with an edit is here.

use rux_layout::Node;
use rux_runtime::Document;

fn doc(template: &str, script: &str) -> Document {
    Document::from_source(&format!(
        "<template><screen>{template}</screen></template>\n<script>\n{script}\n</script>"
    ))
    .expect("loads")
}

fn texts(node: &Node, out: &mut Vec<String>) {
    if let Some(t) = &node.text {
        out.push(t.text.clone());
    }
    for c in &node.children {
        texts(c, out);
    }
}

fn shows(doc: &Document, want: &str) -> bool {
    let mut out = Vec::new();
    texts(&doc.root, &mut out);
    out.iter().any(|t| t == want)
}

fn find<'a>(node: &'a Node, pred: &dyn Fn(&Node) -> bool) -> Option<&'a Node> {
    if pred(node) {
        return Some(node);
    }
    node.children.iter().find_map(|c| find(c, pred))
}

const LIST: &str = "let items = signal([#{ id: 1, name: \"alpha\", done: false }, #{ id: 2, name: \"beta\", done: false }]);";

#[test]
fn an_edit_in_a_row_writes_to_the_list() {
    let mut d = doc(
        r#"<view r-for="item in items" r-key="item.id"><input r-model="item.name" /></view>
           <text>{{ items[1].name }}</text>"#,
        LIST,
    );
    d.apply_edit_in("item.name", Some("2"), None, "changed");
    assert_eq!(d.value_in("item.name", Some("2"), None), "changed");
    assert!(shows(&d, "changed"), "the list itself changed");
    // The other row is untouched.
    assert_eq!(d.value_in("item.name", Some("1"), None), "alpha");
}

#[test]
fn a_checkbox_in_a_row_writes_to_the_list() {
    let mut d = doc(
        r#"<view r-for="item in items" r-key="item.id"><input type="checkbox" r-model="item.done" /></view>
           <text>{{ items[0].done }}</text>"#,
        LIST,
    );
    let tap = find(&d.root, &|n| n.on_tap.is_some()).and_then(|n| n.on_tap.clone()).expect("a toggle");
    assert!(d.apply_handler(&tap), "the tap changed something");
    assert!(shows(&d, "true"), "items[0].done is now true");
}

#[test]
fn a_row_of_a_nested_list_writes_to_the_inner_list() {
    let mut d = doc(
        r#"<view r-for="group in groups" r-key="group.id">
             <view r-for="task in group.tasks" r-key="task.id"><input r-model="task.title" /></view>
           </view>
           <text>{{ groups[1].tasks[0].title }}</text>"#,
        "let groups = signal([#{ id: 1, tasks: [#{ id: 10, title: \"a\" }] }, #{ id: 2, tasks: [#{ id: 20, title: \"b\" }] }]);",
    );
    d.apply_edit_in("task.title", Some("20"), None, "written");
    assert!(shows(&d, "written"), "groups[1].tasks[0].title changed");
}

/// The form that always worked still does.
#[test]
fn an_indexed_model_still_writes() {
    let mut d = doc(
        r#"<view r-for="item in items" r-key="item.id"><input r-model="items[0].name" /></view>
           <text>{{ items[0].name }}</text>"#,
        LIST,
    );
    d.apply_edit_in("items[0].name", Some("1"), None, "direct");
    assert!(shows(&d, "direct"));
}
