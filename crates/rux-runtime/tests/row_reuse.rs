//! A keyed `r-for` row that has not changed is reused, not built again.
//!
//! A reconcile builds the whole document, so a list of 300 rows restyled all
//! 300 when one of them moved. Rows are now kept between builds (see
//! `rux_style::RowCache`). What these check is that a reused row is always
//! the row a build would have made: debug builds also build again without
//! reusing anything and panic on any difference, so every test here, and
//! every other test that reconciles a keyed list, is a proof of that as well.

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

fn find<'a>(node: &'a Node, pred: &dyn Fn(&Node) -> bool) -> Option<&'a Node> {
    if pred(node) {
        return Some(node);
    }
    node.children.iter().find_map(|c| find(c, pred))
}

const ITEMS: &str = "let items = signal([#{ id: 1, name: \"a\" }, #{ id: 2, name: \"b\" }, #{ id: 3, name: \"c\" }]);";
const ROTATE: &str = "let first = items[0]; let rest = items.slice(1, items.length); rest.push(first); items = rest;";

#[test]
fn a_list_that_moves_reuses_its_rows() {
    let mut d = doc(
        r#"<view r-for="item in items" r-key="item.id"><text>{{ item.name }}</text></view>"#,
        ITEMS,
    );
    assert!(d.apply_handler(ROTATE));
    assert_eq!(all_text(&d), ["b", "c", "a"]);
    // The first reconcile had nothing kept; the second reuses every row.
    assert!(d.apply_handler(ROTATE));
    assert_eq!(all_text(&d), ["c", "a", "b"]);
    assert_eq!(d.row_reuse(), (3, 0));
}

#[test]
fn a_row_whose_item_changed_is_built_again() {
    let mut d = doc(
        r#"<view r-for="item in items" r-key="item.id"><text>{{ item.name }}</text></view>"#,
        ITEMS,
    );
    assert!(d.apply_handler(ROTATE));
    assert!(d.apply_handler("items[1].name = \"changed\";"));
    assert_eq!(all_text(&d), ["b", "changed", "a"]);
    assert_eq!(d.row_reuse(), (2, 1));
}

/// A row reads a signal the list does not: a change to it restyles the row.
#[test]
fn a_row_that_read_a_signal_follows_it() {
    let mut d = doc(
        r##"<view r-for="item in items" r-key="item.id" :class="#{ on: item.id == picked }">
             <text>{{ item.name }}</text>
           </view>"##,
        &format!("{ITEMS} let picked = signal(1);"),
    );
    assert!(d.apply_handler(ROTATE));
    assert!(d.apply_handler(ROTATE));
    assert_eq!(d.row_reuse(), (3, 0), "{:?}", d.diagnostics().warnings);
    // Every row read `picked`, so every row is built again.
    assert!(d.apply_handler("picked = 2;"));
    assert_eq!(d.row_reuse(), (0, 3));
    assert!(d.apply_handler(ROTATE));
    assert_eq!(d.row_reuse(), (3, 0));
}

/// An `r-model` writes through the row's place in its list, and a moved row's
/// place is its new one.
#[test]
fn a_moved_row_writes_to_where_it_now_is() {
    let mut d = doc(
        r#"<view r-for="item in items" r-key="item.id"><input r-model="item.name" /></view>
           <text>{{ items[0].name }}</text>"#,
        ITEMS,
    );
    assert!(d.apply_handler(ROTATE));
    assert!(d.apply_handler(ROTATE));
    assert_eq!(d.row_reuse().0, 3, "every row was reused, at a new place");
    // Row 3 is now first in the list.
    d.apply_edit_in("item.name", Some("3"), None, "typed");
    assert_eq!(d.value_in("item.name", Some("3"), None), "typed");
    assert!(all_text(&d).contains(&"typed".to_string()), "items[0] is the row typed into");
}

/// A handler is baked with the row's place inside it, so a row that has one
/// and moved is built again rather than tapping the wrong item.
#[test]
fn a_moved_row_with_a_handler_taps_its_own_item() {
    let mut d = doc(
        r#"<view r-for="item in items" r-key="item.id" @tap="picked = item.name"><text>{{ item.name }}</text></view>
           <text>{{ picked }}</text>"#,
        &format!("{ITEMS} let picked = signal(\"\");"),
    );
    assert!(d.apply_handler(ROTATE));
    assert!(d.apply_handler(ROTATE));
    let tap = find(&d.root, &|n| n.on_tap.is_some()).and_then(|n| n.on_tap.clone()).expect("a row");
    assert!(d.apply_handler(&tap));
    assert!(all_text(&d).iter().filter(|t| *t == "c").count() == 2, "the first row is c, and it said so");
}

#[test]
fn a_nested_list_is_reused_and_still_writes_inside() {
    let mut d = doc(
        r#"<view r-for="group in groups" r-key="group.id">
             <view r-for="task in group.tasks" r-key="task.id"><input r-model="task.title" /></view>
           </view>
           <text>{{ groups[0].tasks[0].title }}</text>"#,
        "let groups = signal([#{ id: 1, tasks: [#{ id: 10, title: \"a\" }] }, #{ id: 2, tasks: [#{ id: 20, title: \"b\" }] }]);",
    );
    let swap = "let g = groups[0]; groups = [groups[1], g];";
    assert!(d.apply_handler(swap));
    assert!(d.apply_handler(swap));
    assert!(d.apply_handler(swap));
    d.apply_edit_in("task.title", Some("20"), None, "written");
    assert!(all_text(&d).contains(&"written".to_string()), "groups[0].tasks[0] is task 20");
}

/// Hover is matched by path, and a hovered row is built, not reused.
#[test]
fn a_hovered_row_is_restyled() {
    let mut d = doc(
        r#"<view class="row" r-for="item in items" r-key="item.id"><text>{{ item.name }}</text></view>"#,
        ITEMS,
    );
    assert!(d.apply_handler(ROTATE));
    let mut state = d.interaction().clone();
    state.hovered = Some(vec![1, 0]);
    d.set_interaction(state);
    assert!(d.apply_handler(ROTATE));
    let (hits, misses) = d.row_reuse();
    assert!(misses >= 1, "the hovered row was built");
    assert_eq!(hits + misses, 3);
}

/// A binding that fails in a row keeps saying so on every build.
#[test]
fn a_row_with_a_failing_binding_keeps_its_warning() {
    let mut d = doc(
        r#"<view r-for="item in items" r-key="item.id"><text>{{ item.name.nope() }}</text></view>"#,
        ITEMS,
    );
    assert!(d.apply_handler(ROTATE));
    let before = d.diagnostics().warnings.len();
    assert!(before > 0, "the binding fails");
    assert!(d.apply_handler(ROTATE));
    assert_eq!(d.diagnostics().warnings.len(), before);
    assert_eq!(d.row_reuse().0, 0, "a row that warned is never kept");
}

/// A component row is never reused: an instance has to be touched by the build.
#[test]
fn a_row_of_components_keeps_its_instances() {
    let dir = std::env::temp_dir().join(format!(
        "rux_row_reuse_{}_{}",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("app.rux"),
        "<template><screen>\
           <item-row r-for=\"item in items\" r-key=\"item.id\" :name=\"item.name\" />\
         </screen></template>\n\
         <script>\nuse item_row;\nlet items = signal([#{ id: 1, name: \"a\" }, #{ id: 2, name: \"b\" }]);\n</script>",
    )
    .unwrap();
    std::fs::write(
        dir.join("item-row.rux"),
        "<template><view><text>{{ name }} {{ count }}</text></view></template>\n\
         <script>\nprop name: string;\nlet count = 7;\n</script>",
    )
    .unwrap();
    let d = Document::load(dir.join("app.rux"));
    let _ = std::fs::remove_dir_all(&dir);
    let mut d = d.expect("loads");
    assert!(d.apply_handler("items = [items[1], items[0]];"));
    assert!(d.apply_handler("items = [items[1], items[0]];"));
    assert_eq!(d.row_reuse(), (0, 2), "an instance row is always built");
    assert_eq!(all_text(&d), ["a 7", "b 7"]);
}
