//! An `<input r-model>` inside a component edits *that instance's* state.
//!
//! Every `r-model` test in the runtime was written against a `<screen>` root,
//! so the component case had never been exercised anywhere: focus carried only
//! `(model, row)` and the value was read and written in the document's engine,
//! where a component's own `let` is not a name at all. The read failed once, the
//! writes went nowhere, and from outside the field simply took nothing.
//!
//! Reported as "my inputs do not receive values" against a routed page, which is
//! a component instance like any other.

use std::fs;

use rux_runtime::Document;

/// Distinguishes two scratch directories created inside one clock tick, for the
/// reason spelled out in `cascade.rs`: the Windows clock is coarse enough that
/// `as_nanos()` repeats.
static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

fn load(app: &str, component: &str) -> Document {
    let dir = std::env::temp_dir().join(format!(
        "rux_model_instance_{}_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
    ));
    fs::create_dir_all(dir.join("components")).unwrap();
    fs::write(dir.join("components/field.rux"), component).unwrap();
    fs::write(dir.join("app.rux"), app).unwrap();
    let doc = Document::load(dir.join("app.rux")).expect("loads");
    let _ = fs::remove_dir_all(&dir);
    doc
}

/// Every `(instance, model)` pair in the tree, in document order. The instance
/// is what the shell reads off the focused input's region, so this is the same
/// identity a tap produces.
fn bound_inputs(node: &rux_layout::Node, out: &mut Vec<(Option<String>, String)>) {
    if let Some(model) = &node.model {
        out.push((node.instance.clone(), model.clone()));
    }
    for child in &node.children {
        bound_inputs(child, out);
    }
}

fn inputs(doc: &Document) -> Vec<(Option<String>, String)> {
    let mut out = Vec::new();
    bound_inputs(&doc.root, &mut out);
    out
}

/// The text actually shown by the input bound to `model`, so the assertions are
/// about what is on screen and not only about what the engine holds.
fn shown(node: &rux_layout::Node, model: &str) -> Option<String> {
    if node.model.as_deref() == Some(model) {
        return node.children.first()?.text.as_ref().map(|t| t.text.clone());
    }
    node.children.iter().find_map(|c| shown(c, model))
}

/// The `@input` body the last build baked into the input bound to `model`,
/// which is what the shell runs: it holds the frame that was on screen when
/// the key arrived, not one built after it.
fn baked_input(node: &rux_layout::Node, model: &str) -> Option<String> {
    if node.model.as_deref() == Some(model) {
        return node.field.on_input.clone();
    }
    node.children.iter().find_map(|c| baked_input(c, model))
}

const FIELD: &str = "<template><view><input r-model=\"mine\" /></view></template>\
                     <script>let mine = signal(\"start\");</script>";

const APP: &str = "<template><screen><field /></screen></template>\
                   <script>use components::field;</script>";

/// The defect itself: the value is read in the instance's scope, and a write
/// lands in it.
#[test]
fn an_input_in_a_component_reads_and_writes_its_own_state() {
    let mut doc = load(APP, FIELD);

    let found = inputs(&doc);
    assert_eq!(found.len(), 1, "one bound input: {found:?}");
    let instance = found[0].0.clone();
    assert!(instance.is_some(), "the input must know which instance it is in");

    assert_eq!(
        doc.value_in("mine", None, instance.as_deref()),
        "start",
        "read in the document's scope this was empty, and warned once"
    );

    doc.apply_edit_in("mine", None, instance.as_deref(), "typed");
    assert_eq!(doc.value_in("mine", None, instance.as_deref()), "typed", "the edit landed");
    assert_eq!(shown(&doc.root, "mine").as_deref(), Some("typed"), "and is on screen");
}

/// Two instances of one component are two values. They carry the same `r-model`
/// text, so the model alone cannot tell them apart, which is the same reason a
/// list's rows need their key.
#[test]
fn two_instances_of_one_component_keep_their_own_values() {
    let mut doc = load(
        "<template><screen><field /><field /></screen></template>\
         <script>use components::field;</script>",
        FIELD,
    );

    let found = inputs(&doc);
    assert_eq!(found.len(), 2, "two bound inputs: {found:?}");
    let (first, second) = (found[0].0.clone(), found[1].0.clone());
    assert_ne!(first, second, "two instances, two keys");

    eprintln!("KEYS {:?}", found);
    doc.apply_edit_in("mine", None, first.as_deref(), "one");
    eprintln!("AFTER {:?}", inputs(&doc));
    eprintln!("FIRST {:?} SECOND {:?}",
        doc.value_in("mine", None, first.as_deref()),
        doc.value_in("mine", None, second.as_deref()));
    assert_eq!(doc.value_in("mine", None, first.as_deref()), "one");
    assert_eq!(
        doc.value_in("mine", None, second.as_deref()),
        "start",
        "the other instance is untouched"
    );
}

/// A component's state is not a document signal, so a document of the same name
/// is a different value and neither write reaches the other.
#[test]
fn the_document_and_the_component_do_not_share_a_name() {
    let mut doc = load(
        "<template><screen><input r-model=\"mine\" /><field /></screen></template>\
         <script>use components::field;
let mine = signal(\"page\");</script>",
        FIELD,
    );

    let found = inputs(&doc);
    let page = found.iter().find(|(i, _)| i.is_none()).expect("the page's own input").0.clone();
    let inner = found.iter().find(|(i, _)| i.is_some()).expect("the component's input").0.clone();

    doc.apply_edit_in("mine", None, page.as_deref(), "edited page");
    assert_eq!(doc.value_in("mine", None, page.as_deref()), "edited page");
    assert_eq!(
        doc.value_in("mine", None, inner.as_deref()),
        "start",
        "the component's own `mine` is a different value"
    );
}

/// An `r-model` bound to a **prop** is not written back: a prop belongs to the
/// caller, and a write here would look like it worked and be forgotten by the
/// next build. That is the rule handlers already follow.
#[test]
fn an_r_model_on_a_prop_does_not_stick() {
    let mut doc = load(
        "<template><screen><field :label=\"caption\" /></screen></template>\
         <script>use components::field;\nlet caption = signal(\"from the caller\");</script>",
        "<template><view><input r-model=\"label\" /></view></template><script></script>",
    );

    let found = inputs(&doc);
    let instance = found[0].0.clone();
    assert_eq!(doc.value_in("label", None, instance.as_deref()), "from the caller");

    doc.apply_edit_in("label", None, instance.as_deref(), "typed over it");
    assert_eq!(
        doc.value_in("label", None, instance.as_deref()),
        "from the caller",
        "a prop is the caller's, so the edit is dropped rather than half-kept"
    );
}

/// **An `@input` that touches other state must not put the field back.** A
/// handler in an instance writes back the names the instance owns, and the
/// field's own name is one of them, so a stale copy of it undoes the typing.
/// Found on the phone: `@input="typed += 1"` counted every key while the field
/// stayed on its first letter.
#[test]
fn an_input_handler_in_a_component_keeps_the_typing() {
    let mut doc = load(
        APP,
        "<template><view><input r-model=\"mine\" @input=\"typed += 1\" /></view></template>\
         <script>let mine = signal(\"\");\nlet typed = signal(0);</script>",
    );
    let instance = inputs(&doc)[0].0.clone();
    for text in ["a", "ab", "abc"] {
        let body = baked_input(&doc.root, "mine").expect("an @input");
        doc.apply_edit_in("mine", None, instance.as_deref(), text);
        let event = rux_reactive::Value::Text(text.into());
        doc.apply_handler_with_event(&body, instance.as_deref(), &event);
        assert_eq!(doc.value_in("mine", None, instance.as_deref()), text, "after the handler");
    }
    assert_eq!(doc.value_in("typed", None, instance.as_deref()), "3");
}

/// The same, on a routed page, which is where the phone found it.
#[test]
fn an_input_handler_on_a_routed_page_keeps_the_typing() {
    let mut doc = load(
        "<template><screen><router><route path=\"/\" view=\"field\" /></router></screen></template>\
         <script>use components::field;</script>",
        "<template><view><input r-model=\"mine\" @input=\"typed += 1\" /></view></template>\
         <script>let mine = signal(\"\");\nlet typed = signal(0);</script>",
    );
    let instance = inputs(&doc)[0].0.clone();
    for text in ["a", "ab", "abc"] {
        let body = baked_input(&doc.root, "mine").expect("an @input");
        doc.apply_edit_in("mine", None, instance.as_deref(), text);
        let event = rux_reactive::Value::Text(text.into());
        doc.apply_handler_with_event(&body, instance.as_deref(), &event);
        let now = inputs(&doc)[0].0.clone();
        assert_eq!(now, instance, "the page is still the same instance");
        assert_eq!(doc.value_in("mine", None, instance.as_deref()), text, "after the handler");
    }
}
