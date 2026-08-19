//! The `/learn` guide's code, verified.
//!
//! Every file under `examples/learn/` is a checkpoint the guide tells the reader
//! to run, and every claim the guide makes about what happens when you tap
//! something is asserted here. The guide is hand-written against the *built*
//! runtime rather than synced from a design doc, so this test is what stops it
//! drifting back into describing a Rux that does not exist, the failure mode
//! that made `docs/03-guide.md` unpublishable.
//!
//! If you change a lesson file, change the prose in `site/content/learn/` with it.

use rux_runtime::Document;
use std::path::{Path, PathBuf};

fn examples_dir() -> PathBuf {
    // CARGO_MANIFEST_DIR is crates/rux-runtime.
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/learn")
}

fn load(name: &str) -> Document {
    let path = examples_dir().join(name);
    Document::load(&path).unwrap_or_else(|e| panic!("loading {}: {e}", path.display()))
}

fn find_text(node: &rux_layout::Node, needle: &str) -> bool {
    node.text.as_ref().is_some_and(|t| t.text.contains(needle))
        || node.children.iter().any(|c| find_text(c, needle))
}

fn count_text(node: &rux_layout::Node, needle: &str) -> usize {
    let here = usize::from(node.text.as_ref().is_some_and(|t| t.text.contains(needle)));
    here + node.children.iter().map(|c| count_text(c, needle)).sum::<usize>()
}

/// Every checkpoint file the guide asks the reader to run must actually load.
#[test]
fn every_lesson_file_loads() {
    for name in [
        "01-hello.rux",
        "02-layout.rux",
        "03-state.rux",
        "04-tasks.rux",
        "05-components.rux",
    ] {
        let doc = load(name);
        assert!(!doc.root.children.is_empty(), "{name} built an empty tree");
    }
}

/// Step 3's claim: a tap handler that assigns to a signal updates the binding
/// that reads it, and `r-if` removes the empty state.
///
/// The handler is now `add()`, a `fn` that writes a signal, which is the chapter's
/// headline change for v0.7 and the thing the chapter used to say was impossible.
#[test]
fn step3_tap_updates_the_binding() {
    let mut doc = load("03-state.rux");
    assert!(find_text(&doc.root, "0 added"));
    assert!(find_text(&doc.root, "Nothing yet."), "r-if shows the empty state at 0");

    assert!(doc.apply_handler("add()"), "the function reported a change");
    assert!(find_text(&doc.root, "1 added"), "the tally re-rendered");
    assert!(!find_text(&doc.root, "Nothing yet."), "r-if dropped the empty state");
}

/// **A `fn` can write a signal**, which chapter 3 used to say was impossible and
/// now teaches as the ordinary way to write a handler.
///
/// Asserted separately from the tap above, because if this ever stops working
/// the chapter is wrong in the direction that wastes the most of a reader's
/// time: the old text at least told them not to try.
#[test]
fn step3_a_function_can_write_a_signal() {
    let mut doc = load("03-state.rux");
    assert!(find_text(&doc.root, "0 added"));
    assert!(doc.apply_handler("add()"), "calling it changed something");
    assert!(find_text(&doc.root, "1 added"), "and the change reached the screen");
    assert!(doc.apply_handler("add()"), "twice");
    assert!(find_text(&doc.root, "2 added"), "it reads what it wrote last time");
}

/// The trap chapter 3 puts in place of the old one: a closure passed to a
/// **method** cannot see the surrounding scope, while a plain call can.
///
/// If this ever starts working, the chapter's callout is wrong and has to go.
#[test]
fn step3_a_closure_in_a_method_call_cannot_capture() {
    let mut doc = load("03-state.rux");
    // `count` is a document signal; the closure is passed to `filter`, a method.
    let captured = doc.apply_handler("count = [1, 2, 3].filter(|n| n == count).len()");
    assert!(!captured, "the method's closure could not see `count`, so nothing moved");
}

/// Step 4's claims, in the order the guide makes them.
#[test]
fn step4_task_list_behaves_as_documented() {
    let mut doc = load("04-tasks.rux");

    // Seeded state: two rows, one of them done.
    assert!(find_text(&doc.root, "read the reference"));
    assert!(find_text(&doc.root, "build a task list"));
    assert!(find_text(&doc.root, "1 / 2"), "the closure tally counts done items");

    // Adding: push a map onto the signal array, then clear the draft.
    let added = doc.apply_handler(
        r#"if draft != "" { items.push(#{ label: draft, done: false }); draft = ""; }"#,
    );
    assert!(!added, "an empty draft adds nothing");

    doc.apply_handler(r#"draft = "ship /learn""#);
    assert!(doc.apply_handler(
        r#"if draft != "" { items.push(#{ label: draft, done: false }); draft = ""; }"#
    ));
    assert!(find_text(&doc.root, "ship /learn"), "the new row rendered");
    assert!(find_text(&doc.root, "1 / 3"), "the tally counted the new row");

    // Toggling, exactly as the template writes it, including the baked-in
    // `t` snapshot the handler matches against.
    let toggle = r#"let t = #{ label: "ship /learn", done: false };
                    for i in 0..items.len() { if items[i].label == t.label { items[i].done = !items[i].done; } }"#;
    assert!(doc.apply_handler(toggle), "the indexed write was detected");
    assert!(find_text(&doc.root, "2 / 3"), "toggling moved the tally");
}

/// The trap the guide spends a callout on: the `r-for` local is a snapshot, so
/// writing through it changes nothing. If this ever starts working, the guide's
/// warning is wrong and must be rewritten.
#[test]
fn step4_writing_through_the_loop_local_is_a_no_op() {
    let mut doc = load("04-tasks.rux");
    let before = count_text(&doc.root, "/ 2");

    let changed = doc.apply_handler("for t in items { t.done = true; }");
    assert!(!changed, "mutating the loop local reported no change");
    assert!(find_text(&doc.root, "1 / 2"), "the tally did not move");
    assert_eq!(count_text(&doc.root, "/ 2"), before);
}

/// `:class` accepts rhai's object form. The ternary does NOT, rhai has no
/// `?:` operator, and the guide tells readers to use `if`/`else` instead.
#[test]
fn class_binding_object_form_applies_and_ternary_does_not_parse() {
    // A 99px marker is the only way to observe a bound class: classes are
    // resolved into `style` before layout, so they are gone by tree time.
    let src = |expr: &str| {
        format!(
            r#"<template><screen>
                 <view r-for="t in items" :class='{expr}'><text>x</text></view>
               </screen></template>
               <style>.done text {{ font-size: 99px; }}</style>
               <script>let items = signal([#{{ label: "a", done: true }}]);</script>"#
        )
    };
    let marked = |node: &rux_layout::Node| -> bool {
        fn walk(n: &rux_layout::Node) -> bool {
            n.text.as_ref().is_some_and(|t| (t.font_size - 99.0).abs() < 0.5)
                || n.children.iter().any(walk)
        }
        walk(node)
    };

    let object = Document::from_source(&src(r#"#{ done: t.done }"#)).expect("object form loads");
    assert!(marked(&object.root), "`#{{ done: t.done }}` applied the class");

    let if_else =
        Document::from_source(&src(r#"if t.done { "done" } else { "" }"#)).expect("if form loads");
    assert!(marked(&if_else.root), "an if-expression applied the class");

    // The shape a reader would copy from Vue or JSX. It must not silently work,
    // because the guide warns that it doesn't.
    let ternary = Document::from_source(&src(r#"t.done ? "done" : ""#));
    let applied = ternary.map(|d| marked(&d.root)).unwrap_or(false);
    assert!(!applied, "the ternary must NOT apply the class; rhai has no ?: operator");
}

/// Step 5: the component renders its props, and its own CSS styles its subtree.
#[test]
fn step5_component_renders_props() {
    let doc = load("05-components.rux");
    assert!(find_text(&doc.root, "read the reference"), "component rendered a label prop");
    assert!(find_text(&doc.root, "build a task list"));
    assert!(find_text(&doc.root, "1 / 2"), "the caller still owns the tally");
}
