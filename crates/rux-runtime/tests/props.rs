//! `prop`: what a component declares its caller owes it, and the errors that
//! declaration makes possible.
//!
//! Before it, a component read `{{ label }}` with nothing declared, so a name a
//! caller passes and a typo looked the same, and any attribute at all was
//! accepted on any tag. Now an attribute is either one Rux reads on that
//! element or a prop the component declared, and anything else is an error.

use std::fs;
use std::path::PathBuf;

use rux_runtime::Document;

static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// A throwaway project. The counter is there for the reason `imports.rs` gives:
/// Windows' clock alone hands two tests the same directory.
fn project(files: &[(&str, &str)]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "rux_props_{}_{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
    ));
    let _ = fs::remove_dir_all(&dir);
    for (rel, body) in files {
        let path = dir.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }
    dir
}

fn app(tag: &str) -> String {
    format!(
        "<template><screen>{tag}</screen></template>\n\
         <script>\nuse components::stat;\nlet n = signal(7);\n</script>"
    )
}

fn load(tag: &str, component: &str) -> Document {
    let dir = project(&[("app.rux", &app(tag)), ("components/stat.rux", component)]);
    Document::load(dir.join("app.rux")).expect("loads")
}

fn errors(doc: &Document) -> Vec<String> {
    doc.diagnostics().warnings.iter().filter(|w| w.is_error()).map(|w| w.message.clone()).collect()
}

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

const STAT: &str = "<template><view><text>{{ label }}={{ value }}</text></view></template>\n\
                    <script>\n  prop label;\n  prop value = 0;\n</script>";

#[test]
fn a_declared_prop_arrives_and_a_default_fills_one_left_off() {
    let doc = load("<stat :label='\"a\"' :value='n' />", STAT);
    assert!(errors(&doc).is_empty(), "{:?}", errors(&doc));
    assert!(shown(&doc).contains(&"a=7".to_string()), "{:?}", shown(&doc));

    let doc = load("<stat :label='\"b\"' />", STAT);
    assert!(errors(&doc).is_empty(), "{:?}", errors(&doc));
    assert!(shown(&doc).contains(&"b=0".to_string()), "the default: {:?}", shown(&doc));
}

/// HTML's way: a plain attribute passes its text.
#[test]
fn a_plain_attribute_naming_a_prop_passes_its_text() {
    let doc = load("<stat label=\"Battery\" />", STAT);
    assert!(errors(&doc).is_empty(), "{:?}", errors(&doc));
    assert!(shown(&doc).contains(&"Battery=0".to_string()), "{:?}", shown(&doc));
}

/// Watchlist #13: tags are kebab and scripts snake, so a prop is written the
/// tag's way and read the script's.
#[test]
fn a_kebab_attribute_reaches_a_snake_prop() {
    let comp = "<template><view><text>{{ id_of }}</text></view></template>\n\
                <script>\n  prop id_of;\n</script>";
    let doc = load("<stat :id-of='\"x9\"' />", comp);
    assert!(errors(&doc).is_empty(), "{:?}", errors(&doc));
    assert!(shown(&doc).contains(&"x9".to_string()), "{:?}", shown(&doc));
}

#[test]
fn an_undeclared_attribute_on_a_component_tag_is_an_error() {
    let doc = load("<stat :label='\"a\"' :valeu='n' />", STAT);
    let found = errors(&doc);
    assert!(
        found.iter().any(|e| e.contains("`:valeu`") && e.contains("Did you mean `value`?")),
        "{found:?}"
    );
}

/// `class`, `style`, `id` and `to` on a component tag used to be dropped with
/// nothing said. A component tag is not an element, so they reach nothing.
#[test]
fn element_attributes_on_a_component_tag_are_errors_unless_declared() {
    let doc = load("<stat :label='\"a\"' class=\"big\" to=\"/x\" />", STAT);
    let found = errors(&doc);
    assert!(found.iter().any(|e| e.contains("`class`") && e.contains("not an element")), "{found:?}");
    assert!(found.iter().any(|e| e.contains("`to`")), "{found:?}");

    // Declared, `to` is just a prop.
    let comp = "<template><view><text>{{ to }}</text></view></template>\n\
                <script>\n  prop to;\n</script>";
    let doc = load("<stat to=\"/crew\" />", comp);
    assert!(errors(&doc).is_empty(), "{:?}", errors(&doc));
    assert!(shown(&doc).contains(&"/crew".to_string()), "{:?}", shown(&doc));
}

#[test]
fn a_required_prop_left_off_is_an_error_at_the_tag() {
    let doc = load("<stat :value='n' />", STAT);
    let found = errors(&doc);
    assert!(found.iter().any(|e| e.contains("needs `label`")), "{found:?}");
}

/// Watchlist #7: invented and near-miss attributes on Rux's own elements.
#[test]
fn an_unknown_attribute_on_an_element_is_an_error() {
    let comp = "<template><view :clas='\"a\"' frobnicate=\"1\" :id='\"b\"'>\
                <text :key='1'>x</text></view></template>";
    let doc = load("<stat />", comp);
    let found = errors(&doc);
    assert!(found.iter().any(|e| e.contains("`:clas`") && e.contains("`:class`")), "{found:?}");
    assert!(found.iter().any(|e| e.contains("`frobnicate`")), "{found:?}");
    assert!(found.iter().any(|e| e.contains("`:id`") && e.contains("no bound form")), "{found:?}");
    assert!(found.iter().any(|e| e.contains("`:key`") && e.contains("`r-key`")), "{found:?}");
}

/// A `<route>` stands in for its view's tag: its bound attributes are props.
#[test]
fn a_route_passes_bound_attributes_to_its_view_as_props() {
    let view = "<template><view><text>{{ crew }} {{ id }}</text></view></template>\n\
                <script>\n  prop crew;\n  prop id;\n</script>";
    let page = |route: &str| {
        format!(
            "<template><screen><router>{route}</router></screen></template>\n\
             <script>\nuse components::detail;\nlet all = signal(\"everyone\");\n</script>"
        )
    };
    let fine = project(&[
        ("app.rux", &page("<route path=\"/\" view=\"detail\" :crew=\"all\" :id='\"a\"' />")),
        ("components/detail.rux", view),
    ]);
    let doc = Document::load(fine.join("app.rux")).expect("loads");
    assert!(errors(&doc).is_empty(), "{:?}", errors(&doc));

    let missing = project(&[
        ("app.rux", &page("<route path=\"/\" view=\"detail\" :crew=\"all\" :nope=\"1\" />")),
        ("components/detail.rux", view),
    ]);
    let doc = Document::load(missing.join("app.rux")).expect("loads");
    let found = errors(&doc);
    assert!(found.iter().any(|e| e.contains("needs `id`")), "{found:?}");
    assert!(found.iter().any(|e| e.contains("`:nope`")), "{found:?}");
}

#[test]
fn a_malformed_declaration_is_an_error() {
    let comp = "<template><view><text>x</text></view></template>\n\
                <script>\n  prop id-of;\n  prop a: number;\n  prop b, c = 1;\n</script>";
    let doc = load("<stat />", comp);
    let found = errors(&doc);
    assert!(found.iter().any(|e| e.contains("prop id_of;")), "{found:?}");
    assert!(found.iter().any(|e| e.contains("no types yet")), "{found:?}");
    assert!(found.iter().any(|e| e.contains("one default to 2 props")), "{found:?}");
}

/// The fix that made the rest of these observable: a document whose `mounted`
/// ran rebuilt, and the rebuild threw away every error the load had found.
#[test]
fn load_time_errors_survive_a_mounted_hook() {
    let dir = project(&[(
        "app.rux",
        "<template><screen @frob=\"x\"><text>{{ n }}</text></screen></template>\n\
         <script>\nlet n = signal(0);\nmounted {\n  n = 1;\n}\n</script>",
    )]);
    let doc = Document::load(dir.join("app.rux")).expect("loads");
    assert!(errors(&doc).iter().any(|e| e.contains("`@frob`")), "{:?}", errors(&doc));
}
