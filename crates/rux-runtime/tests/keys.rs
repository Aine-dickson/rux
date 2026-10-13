//! `@key`: a key pressed while focus is on an element or inside it (Mava
//! Studio's MS-004). The shell walks from the focused element outwards; what
//! is tested here is one step of that walk: a body runs with its `event`, in
//! its own instance's scope, and answering `false` takes the key.

use rux_layout::Node;
use rux_reactive::Value;
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

/// The `@key` bodies on the way from the root to the first node that has one.
fn first_key(node: &Node) -> Option<(String, Option<String>)> {
    if let Some(k) = &node.on_key {
        return Some((k.clone(), node.instance.clone()));
    }
    node.children.iter().find_map(first_key)
}

fn key(name: &str, primary: bool) -> Value {
    Value::Map(vec![
        ("key".to_string(), Value::Text(name.to_string())),
        ("ctrl".to_string(), Value::Bool(primary)),
        ("shift".to_string(), Value::Bool(false)),
        ("alt".to_string(), Value::Bool(false)),
        ("meta".to_string(), Value::Bool(false)),
        ("primary".to_string(), Value::Bool(primary)),
        ("repeat".to_string(), Value::Bool(false)),
    ])
}

const SHORTCUTS: &str = "<template><screen @key=\"shortcut(event)\"><text>{{ said }}</text></screen></template>
<script>
let said = signal(\"nothing\");
fn shortcut(e) {
  if e.primary && e.key == \"s\" { said = \"saved\"; return false; }
  if e.key == \"Escape\" { said = \"escaped\"; }
}
</script>";

#[test]
fn a_screen_key_handler_runs_and_false_takes_the_key() {
    let mut d = Document::from_source(SHORTCUTS).expect("loads");
    let (body, instance) = first_key(&d.root).expect("the screen carries its @key");
    assert!(d.dispatch_key(&body, instance.as_deref(), &key("s", true)), "Ctrl+S is taken");
    assert_eq!(shown(&d), "saved");
}

#[test]
fn a_handler_that_does_not_answer_false_lets_the_key_go_on() {
    let mut d = Document::from_source(SHORTCUTS).expect("loads");
    let (body, instance) = first_key(&d.root).unwrap();
    assert!(!d.dispatch_key(&body, instance.as_deref(), &key("Escape", false)), "ran, but did not take it");
    assert_eq!(shown(&d), "escaped");
    assert!(!d.dispatch_key(&body, instance.as_deref(), &key("q", false)), "nothing to do with it");
}

#[test]
fn key_on_any_element_is_accepted_and_typed() {
    let src = "<template><screen><view @key=\"if event.repeat { n = n + 1 }\"><text>{{ n }}</text></view></screen></template>
<script>let n = signal(0);</script>";
    let d = Document::from_source_checked(src);
    assert!(d.is_ok(), "{:?}", d.err().map(|e| e.to_string()));
    let d = d.unwrap();
    let warned: Vec<_> = d.diagnostics().warnings.iter().map(|w| w.message.clone()).collect();
    assert!(warned.iter().all(|w| !w.contains("@key")), "{warned:?}");
}

#[test]
fn a_key_handler_in_a_component_runs_in_its_instance() {
    use std::fs;
    let dir = std::env::temp_dir().join(format!("rux_keys_{}", std::process::id()));
    fs::create_dir_all(dir.join("components")).unwrap();
    fs::write(
        dir.join("components").join("panel.rux"),
        "<template><view @key=\"own = event.key; false\"><text>{{ own }}</text></view></template>\n<script>\nlet own = signal(\"-\");\n</script>",
    )
    .unwrap();
    fs::write(
        dir.join("app.rux"),
        "<template><screen><panel /><panel /></screen></template>\n<script>\nuse components::panel;\n</script>",
    )
    .unwrap();
    let mut d = Document::load(dir.join("app.rux")).expect("loads");
    let _ = fs::remove_dir_all(&dir);
    let (body, instance) = first_key(&d.root).expect("the first panel's @key");
    assert!(instance.is_some(), "written in a component, so it names its instance");
    assert!(d.dispatch_key(&body, instance.as_deref(), &key("k", false)));
    assert_eq!(shown(&d), "k|-", "only the first panel's own state moved");
}

#[test]
fn key_on_a_component_tag_is_still_a_listener_for_its_emit() {
    use std::fs;
    let dir = std::env::temp_dir().join(format!("rux_keys_tag_{}", std::process::id()));
    fs::create_dir_all(dir.join("components")).unwrap();
    fs::write(
        dir.join("components").join("pad.rux"),
        "<template><view @tap=\"emit('key', 3)\"><text>pad</text></view></template>",
    )
    .unwrap();
    fs::write(
        dir.join("app.rux"),
        "<template><screen><pad @key=\"got = event\" /><text>{{ got }}</text></screen></template>\n<script>\nuse components::pad;\nlet got = signal(0);\n</script>",
    )
    .unwrap();
    let d = Document::load(dir.join("app.rux")).expect("loads");
    let _ = fs::remove_dir_all(&dir);
    assert!(first_key(&d.root).is_none(), "the tag's @key is the caller's listener, not a key handler");
}
