//! `::selection`: what the build hands the painter for a field's highlight.
//! The drawing itself, and the platform's own highlight where no rule names
//! one, are the shell's and are driven by hand (see `docs/08-user-tests.md`).

use rux_layout::{Node, SelectionStyle};
use rux_runtime::Document;

fn doc(template: &str, css: &str) -> Document {
    Document::from_source(&format!(
        "<template><screen>{template}</screen></template>\n<script>\nlet a = signal(\"hello\"); let b = signal(\"world\");\n</script>\n<style>\n{css}\n</style>"
    ))
    .expect("loads")
}

fn find<'a>(node: &'a Node, pred: &dyn Fn(&Node) -> bool) -> Option<&'a Node> {
    if pred(node) {
        return Some(node);
    }
    node.children.iter().find_map(|c| find(c, pred))
}

fn style_of(doc: &Document, model: &str) -> SelectionStyle {
    let input = find(&doc.root, &|n| n.model.as_deref() == Some(model)).expect("the input is built");
    input.children[0].text.as_ref().expect("a text child").selection_style
}

fn rgb(c: Option<rux_layout::Rgba>) -> Option<(u8, u8, u8)> {
    c.map(|c| ((c.r * 255.0).round() as u8, (c.g * 255.0).round() as u8, (c.b * 255.0).round() as u8))
}

#[test]
fn no_rule_leaves_the_highlight_to_the_platform() {
    let d = doc(r#"<input r-model="a" />"#, "");
    let s = style_of(&d, "a");
    assert!(s.background.is_none() && s.color.is_none() && s.handle.is_none(), "{s:?}");
}

#[test]
fn a_selection_rule_styles_the_highlight_and_not_the_element() {
    let d = doc(
        r#"<input class="f" r-model="a" />"#,
        ".f::selection { background-color: #ff0000; color: #00ff00; }",
    );
    let s = style_of(&d, "a");
    assert_eq!(rgb(s.background), Some((255, 0, 0)));
    assert_eq!(rgb(s.color), Some((0, 255, 0)));
    // The element's own text colour is untouched: the rule named its
    // highlight, not it.
    let input = find(&d.root, &|n| n.model.as_deref() == Some("a")).unwrap();
    assert_ne!(rgb(Some(input.children[0].text.as_ref().unwrap().color)), Some((0, 255, 0)));
}

/// `::selection` on the page reaches the fields inside it, which is what an
/// author writing one rule for the whole app expects.
#[test]
fn a_selection_rule_is_inherited_by_the_fields_below() {
    let d = doc(
        r#"<view class="page"><input r-model="a" /></view><input r-model="b" />"#,
        ".page::selection { background: #0000ff; }",
    );
    assert_eq!(rgb(style_of(&d, "a").background), Some((0, 0, 255)));
    assert!(style_of(&d, "b").background.is_none(), "outside the page");
}

/// The nearer rule wins, a half at a time: the field's own colour over the
/// page's, and the page's background still showing through.
#[test]
fn a_nearer_selection_rule_overrides_one_half_at_a_time() {
    let d = doc(
        r#"<view class="page"><input class="f" r-model="a" /></view>"#,
        ".page::selection { background: #0000ff; color: #ffffff; } .f::selection { color: #000000; }",
    );
    let s = style_of(&d, "a");
    assert_eq!(rgb(s.background), Some((0, 0, 255)));
    assert_eq!(rgb(s.color), Some((0, 0, 0)));
}

/// A bare `::selection` is every element's, so it reaches every field.
#[test]
fn a_bare_selection_rule_reaches_every_field() {
    let d = doc(r#"<input r-model="a" /><input r-model="b" />"#, "::selection { background: #00ff00; }");
    assert_eq!(rgb(style_of(&d, "a").background), Some((0, 255, 0)));
    assert_eq!(rgb(style_of(&d, "b").background), Some((0, 255, 0)));
}

/// `::selection` in the middle of a selector is not a pseudo-element anything
/// can be inside of, so the rule matches nothing, as a browser drops it.
#[test]
fn a_selection_that_is_not_last_matches_nothing() {
    let d = doc(r#"<view class="page"><input r-model="a" /></view>"#, ".page::selection input { color: #ff0000; }");
    let input = find(&d.root, &|n| n.model.as_deref() == Some("a")).unwrap();
    assert_ne!(rgb(Some(input.children[0].text.as_ref().unwrap().color)), Some((255, 0, 0)));
}

/// `accent-color` on the field colours its selection handles.
#[test]
fn a_fields_accent_colours_its_handles() {
    let d = doc(r#"<input class="f" r-model="a" /><input r-model="b" />"#, ".f { accent-color: #ff8800; }");
    assert_eq!(rgb(style_of(&d, "a").handle), Some((255, 136, 0)));
    assert!(style_of(&d, "b").handle.is_none());
}
