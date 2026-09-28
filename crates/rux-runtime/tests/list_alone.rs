//! A change to a list builds that list, not the whole document.
//!
//! A reconcile used to build the whole document to splice in the one list
//! that changed (docs/11-next.md, track (g)). Now a structural parent in the
//! document's own markup keeps what its children were built from, and a
//! change to it builds only them (`rux_style::rebuild_children`). Debug builds
//! check every such reconcile against a whole build, tree and registry, and
//! panic on any difference, so each test here is also that proof, on a shape
//! chosen to break it: entries recorded after the list, an empty list filling,
//! two lists side by side, rows with their own structure, inputs in rows.

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

fn all_text(doc: &Document) -> Vec<String> {
    let mut out = Vec::new();
    texts(&doc.root, &mut out);
    out
}

const ITEMS: &str = "let items = signal([#{ id: 1, name: \"a\" }, #{ id: 2, name: \"b\" }, #{ id: 3, name: \"c\" }]);";
const ROTATE: &str = "let first = items[0]; let rest = items.slice(1, items.length); rest.push(first); items = rest;";

#[test]
fn a_rotating_list_is_built_alone() {
    let mut d = doc(
        r#"<view class="rows"><view r-for="item in items" r-key="item.id"><text>{{ item.name }}</text></view></view>
           <text>after {{ tick }}</text>"#,
        &format!("{ITEMS} let tick = signal(0);"),
    );
    for _ in 0..4 {
        assert!(d.apply_handler(&format!("{ROTATE} tick += 1;")));
    }
    assert_eq!(all_text(&d), ["b", "c", "a", "after 4"]);
    assert_eq!(d.lists_built_alone(), 4);
    // The text after the list still follows its signal: its binding kept its
    // place in the registry.
    assert!(d.apply_handler("tick = 10;"));
    assert_eq!(all_text(&d).last().map(String::as_str), Some("after 10"));
}

#[test]
fn a_list_grows_shrinks_empties_and_fills_again() {
    let mut d = doc(
        r#"<view><view r-for="item in items" r-key="item.id"><text>{{ item.name }}</text></view></view>
           <text>{{ items.length }} items</text>"#,
        ITEMS,
    );
    assert!(d.apply_handler("items.insert(0, #{ id: 9, name: \"z\" });"));
    assert_eq!(all_text(&d), ["z", "a", "b", "c", "4 items"]);
    assert!(d.apply_handler("items.remove(2);"));
    assert_eq!(all_text(&d), ["z", "a", "c", "3 items"]);
    assert!(d.apply_handler("items = [];"));
    assert_eq!(all_text(&d), ["0 items"]);
    assert!(d.apply_handler("items = [#{ id: 5, name: \"e\" }];"));
    assert_eq!(all_text(&d), ["e", "1 items"]);
    assert_eq!(d.lists_built_alone(), 4);
}

#[test]
fn a_list_that_starts_empty_fills() {
    let mut d = doc(
        r#"<view><view r-for="item in items" r-key="item.id"><text>{{ item.name }}</text></view></view>
           <text r-if="items.length == 0">none</text>
           <text>{{ label }}</text>"#,
        "let items = signal([]); let label = signal(\"end\");",
    );
    assert_eq!(all_text(&d), ["none", "end"]);
    assert!(d.apply_handler("items = [#{ id: 1, name: \"a\" }, #{ id: 2, name: \"b\" }];"));
    assert_eq!(all_text(&d), ["a", "b", "end"]);
    assert!(d.apply_handler("label = \"still\";"));
    assert_eq!(all_text(&d), ["a", "b", "still"]);
}

#[test]
fn two_lists_side_by_side_each_alone() {
    let mut d = doc(
        r#"<view><view r-for="item in items" r-key="item.id"><text>{{ item.name }}</text></view></view>
           <view><text r-for="n in nums" r-key="n">{{ n }}</text></view>"#,
        &format!("{ITEMS} let nums = signal([1, 2]);"),
    );
    assert!(d.apply_handler(ROTATE));
    assert!(d.apply_handler("nums.push(3);"));
    assert!(d.apply_handler("nums = [3, 2, 1];"));
    assert!(d.apply_handler(ROTATE));
    assert_eq!(all_text(&d), ["c", "a", "b", "3", "2", "1"]);
    assert_eq!(d.lists_built_alone(), 4);
    // Each list's rows are still kept for the next build of it.
    assert!(d.apply_handler(ROTATE));
    assert_eq!(d.row_reuse(), (3, 0));
}

#[test]
fn two_lists_and_a_condition_under_one_parent() {
    let mut d = doc(
        r#"<view>
             <text r-for="item in items" r-key="item.id">{{ item.name }}</text>
             <text r-if="open">open</text>
             <text r-for="n in nums" r-key="n">{{ n }}</text>
           </view>"#,
        &format!("{ITEMS} let nums = signal([1, 2]); let open = signal(false);"),
    );
    assert!(d.apply_handler("open = true;"));
    assert!(d.apply_handler(ROTATE));
    assert!(d.apply_handler("nums = [2]; open = false;"));
    assert_eq!(all_text(&d), ["b", "c", "a", "2"]);
    assert_eq!(d.lists_built_alone(), 3);
}

#[test]
fn rows_with_their_own_conditions_and_lists() {
    let mut d = doc(
        r#"<view>
             <view r-for="item in items" r-key="item.id">
               <text>{{ item.name }}</text>
               <text r-if="item.id == picked">picked</text>
               <text r-for="t in item.tags">#{{ t }}</text>
             </view>
           </view>"#,
        "let items = signal([#{ id: 1, name: \"a\", tags: [\"x\"] }, #{ id: 2, name: \"b\", tags: [] }]);
         let picked = signal(1);",
    );
    // A condition inside a keyed row has nothing kept to be built from (a
    // row may be replayed somewhere else), so this one builds the document.
    assert!(d.apply_handler("picked = 2;"));
    assert_eq!(all_text(&d), ["a", "#x", "b", "picked"]);
    assert_eq!(d.lists_built_alone(), 0);
    assert!(d.apply_handler("items[1].tags.push(\"y\");"));
    assert!(d.apply_handler("let first = items[0]; items = [items[1], first];"));
    assert_eq!(all_text(&d), ["b", "picked", "#y", "a", "#x"]);
    assert_eq!(d.lists_built_alone(), 2);
}

#[test]
fn an_unkeyed_row_holds_a_list_built_alone() {
    // An unkeyed row is never replayed, so the list inside it keeps what it
    // was built from and is built alone when only it changes.
    let mut d = doc(
        r#"<view>
             <view r-for="group in groups">
               <text>{{ group }}</text>
               <view><text r-for="n in nums" r-key="n">{{ group }}{{ n }}</text></view>
             </view>
           </view>"#,
        "let groups = signal([\"a\", \"b\"]); let nums = signal([1]);",
    );
    assert!(d.apply_handler("nums.push(2);"));
    assert_eq!(all_text(&d), ["a", "a1", "a2", "b", "b1", "b2"]);
    assert!(d.apply_handler("groups = [\"c\"];"));
    assert_eq!(all_text(&d), ["c", "c1", "c2"]);
    assert!(d.apply_handler("nums = [3];"));
    assert_eq!(all_text(&d), ["c", "c3"]);
    assert!(d.lists_built_alone() >= 3);
}

#[test]
fn inputs_in_rows_still_write_to_their_own_item() {
    let mut d = doc(
        r#"<view>
             <view r-for="item in items" r-key="item.id">
               <input r-model="item.name" />
               <input type="checkbox" r-model="item.done" />
               <text r-show="item.done">done</text>
             </view>
           </view>"#,
        "let items = signal([#{ id: 1, name: \"a\", done: false }, #{ id: 2, name: \"b\", done: false }]);",
    );
    assert!(d.apply_handler("let first = items[0]; items = [items[1], first];"));
    // The second row now is item 1; an edit there writes to it.
    d.apply_edit_in("item.name", Some("1"), None, "edited");
    assert!(d.apply_handler("items[0].done = true;"));
    let items = d.engine_mut().eval_display("items.map(i => `${i.id}:${i.name}:${i.done}`).join(\",\")", &[]);
    assert_eq!(items, "2:b:true,1:edited:false");
    assert!(d.lists_built_alone() >= 2);
}

#[test]
fn a_list_in_a_scroller_follows_it() {
    let mut d = doc(
        r#"<view style="overflow: scroll; height: 100px">
             <text r-for="item in items" r-key="item.id">{{ item.name }}</text>
           </view>"#,
        ITEMS,
    );
    assert!(d.apply_handler(ROTATE));
    assert!(d.apply_handler("items.pop();"));
    assert_eq!(all_text(&d), ["b", "c"]);
    assert_eq!(d.lists_built_alone(), 2);
}

#[test]
fn a_label_anywhere_builds_the_whole_document() {
    // A whole build links every `for=` label to its field across the whole
    // tree, which a build of one list would not redo, so it is not tried.
    let mut d = doc(
        r#"<view><text r-for="item in items" r-key="item.id">{{ item.name }}</text></view>
           <view><text for="field">label</text><input id="field" r-model="name" /></view>"#,
        &format!("{ITEMS} let name = signal(\"\");"),
    );
    assert!(d.apply_handler(ROTATE));
    assert_eq!(d.lists_built_alone(), 0, "a `for=` label anywhere builds the whole document");
    assert_eq!(all_text(&d)[..3], ["b", "c", "a"]);
}

/// A key naming a field of the row (`item.id`) is read off the record rather
/// than evaluated; it must show exactly as the evaluation would, whatever the
/// field holds.
#[test]
fn a_field_key_shows_as_its_evaluation_would() {
    let mut d = doc(
        r#"<view><text r-for="item in items" r-key="item.id">x</text></view>"#,
        r#"let items = signal([
             #{ id: 1 }, #{ id: 2.5 }, #{ id: -0.0 }, #{ id: 1e21 }, #{ id: "a b" }, #{ id: true }, #{ id: 3.0 }
           ]);"#,
    );
    fn keys(node: &Node, out: &mut Vec<String>) {
        if let Some(k) = &node.key {
            out.push(k.clone());
        }
        for c in &node.children {
            keys(c, out);
        }
    }
    let mut shown = Vec::new();
    keys(&d.root, &mut shown);
    let evaluated: Vec<String> =
        (0..7).map(|i| d.engine_mut().eval_display(&format!("items[{i}].id"), &[])).collect();
    assert_eq!(shown, evaluated);
}
