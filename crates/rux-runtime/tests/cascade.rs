//! A document's rules reach the components it uses, **including** the ones that
//! need an ancestor.
//!
//! Rules cascading into components was settled in v0.7, but only half of it
//! worked: a component tag expanded with an empty ancestor chain, so a simple
//! selector like `.page` matched a component's root and `.stage.slow .page`
//! silently did not.
//!
//! Silent is the operative word, and it is the same failure this project has
//! now hit three times (`rem` honored by half the box model, a swipe made
//! unreachable by a drag, decorations drawn outside their transform). The rule
//! looked right, it was in a stylesheet that demonstrably applied, and it never
//! fired. Found by turning on the router example's slow motion and watching a
//! navigation go by at full speed.

use std::fs;
use std::path::PathBuf;

use rux_runtime::Document;

/// Write a tiny workspace and load it.
fn load(app: &str, component: &str) -> Document {
    let dir = std::env::temp_dir().join(format!(
        "rux_cascade_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(dir.join("components")).unwrap();
    fs::write(dir.join("components/inner.rux"), component).unwrap();
    fs::write(dir.join("app.rux"), app).unwrap();
    let doc = Document::load(dir.join("app.rux")).expect("loads");
    let _ = fs::remove_dir_all(&dir);
    doc
}

fn backgrounds(node: &rux_layout::Node, out: &mut Vec<(f32, f32, f32)>) {
    if let Some(rux_layout::Background::Color(c)) = &node.style.background {
        out.push((c.r, c.g, c.b));
    }
    for child in &node.children {
        backgrounds(child, out);
    }
}

const INNER: &str = "<template><view class=\"page\"><text>inner</text></view></template>";

fn colours(app: &str) -> Vec<(f32, f32, f32)> {
    let doc = load(app, INNER);
    let mut v = Vec::new();
    backgrounds(&doc.root, &mut v);
    v
}

/// The bug: a descendant selector written outside reaches a component's root.
#[test]
fn a_descendant_selector_reaches_a_component_root() {
    let app = "<template><screen class=\"app\">\
           <view class=\"stage slow\"><inner /></view>\
         </screen></template>\n\
         <style>\n\
         .page { background: #ff0000; }\n\
         .stage.slow .page { background: #00ff00; }\n\
         </style>\n\
         <script>\nuse components::inner;\n</script>";
    let found = colours(app);
    assert!(
        found.contains(&(0.0, 1.0, 0.0)),
        "the later, more specific rule won: {found:?}"
    );
    assert!(
        !found.contains(&(1.0, 0.0, 0.0)),
        "and the simple one did not stay: {found:?}"
    );
}

/// A child combinator too, which is the stricter case: the component's root is
/// a real child of the element carrying the tag's place in the tree.
#[test]
fn a_child_combinator_reaches_a_component_root() {
    let app = "<template><screen class=\"app\">\
           <view class=\"stage\"><inner /></view>\
         </screen></template>\n\
         <style>\n\
         .page { background: #ff0000; }\n\
         .stage > .page { background: #00ff00; }\n\
         </style>\n\
         <script>\nuse components::inner;\n</script>";
    assert!(
        colours(app).contains(&(0.0, 1.0, 0.0)),
        "a component's root is its caller's child"
    );
}

/// A selector whose ancestor is not there still does not match, or the fix
/// would be "everything matches" rather than "the chain is real".
#[test]
fn an_ancestor_that_is_not_there_still_does_not_match() {
    let app = "<template><screen class=\"app\">\
           <view class=\"stage\"><inner /></view>\
         </screen></template>\n\
         <style>\n\
         .page { background: #ff0000; }\n\
         .nowhere .page { background: #00ff00; }\n\
         </style>\n\
         <script>\nuse components::inner;\n</script>";
    let found = colours(app);
    assert!(found.contains(&(1.0, 0.0, 0.0)), "the real rule still won");
    assert!(!found.contains(&(0.0, 1.0, 0.0)), "the absent one did not: {found:?}");
}

/// `<style scoped>` on the component still keeps the caller out. The chain
/// being available must not become a way around the opt-out.
#[test]
fn a_scoped_component_is_still_not_reachable() {
    let scoped = "<template><view class=\"page\"><text>inner</text></view></template>\n\
         <style scoped>\n.page { background: #0000ff; }\n</style>";
    let app = "<template><screen class=\"app\">\
           <view class=\"stage\"><inner /></view>\
         </screen></template>\n\
         <style>\n.stage .page { background: #00ff00; }\n</style>\n\
         <script>\nuse components::inner;\n</script>";
    let doc = load(app, scoped);
    let mut found = Vec::new();
    backgrounds(&doc.root, &mut found);
    assert!(found.contains(&(0.0, 0.0, 1.0)), "its own rule applied: {found:?}");
    assert!(
        !found.contains(&(0.0, 1.0, 0.0)),
        "and the caller's did not reach in: {found:?}"
    );
}

/// The same, through a `<router>`: a page is a component the router chose, and
/// a rule written outside the router reaches it the same way.
#[test]
fn a_descendant_selector_reaches_a_route_view() {
    let dir: PathBuf = std::env::temp_dir().join(format!(
        "rux_cascade_route_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(dir.join("components")).unwrap();
    fs::write(
        dir.join("components/home.rux"),
        "<template><view class=\"page\"><text>home</text></view></template>",
    )
    .unwrap();
    fs::write(
        dir.join("app.rux"),
        "<template><screen>\
           <view class=\"stage slow\">\
             <router><route path=\"/\" view=\"home\" /></router>\
           </view>\
         </screen></template>\n\
         <style>\n\
         .page { background: #ff0000; }\n\
         .stage.slow .page { background: #00ff00; }\n\
         </style>
\n         <script>
use components::home;
</script>",
    )
    .unwrap();
    let doc = Document::load(dir.join("app.rux")).expect("loads");
    let _ = fs::remove_dir_all(&dir);

    let mut found = Vec::new();
    backgrounds(&doc.root, &mut found);
    assert!(
        found.contains(&(0.0, 1.0, 0.0)),
        "a route view is reachable too, which is what slow motion needed: {found:?}"
    );
}
