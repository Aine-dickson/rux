//! An `<icon>` is drawn in its own `color`.
//!
//! It used to take its parent's, so `color` written on the icon itself, by a
//! class or by `style=`, was read by nothing and the icon drew in the text colour
//! around it (MS-014 follow-up, found driving `style="color: #f38ba8"`).

use rux_layout::{Node, Rgba};
use rux_runtime::{set_icons, Document, IconPath, MemoryIcons};
use std::rc::Rc;

fn near(a: Rgba, r: f32, g: f32, b: f32) -> bool {
    (a.r - r).abs() < 0.01 && (a.g - g).abs() < 0.01 && (a.b - b).abs() < 0.01
}

/// The (stroke, fill) of every path an icon drew, in document order.
fn paints(node: &Node, out: &mut Vec<(Option<Rgba>, Option<Rgba>)>) {
    if node.path.is_some() {
        out.push((node.style.stroke, node.style.fill));
    }
    for child in &node.children {
        paints(child, out);
    }
}

fn drawn(markup: &str, style: &str) -> Vec<(Option<Rgba>, Option<Rgba>)> {
    let mut icons = MemoryIcons::new(24.0);
    let path = |fill_current| IconPath { d: "M 0 0 L 24 24".into(), fill_current, no_stroke: false, half_opacity: false };
    icons.insert("heart", vec![path(false)], vec![path(false)]);
    set_icons(Rc::new(icons));
    let doc = Document::from_source(&format!(
        "<template><screen>{markup}</screen></template><style>{style}</style>"
    ))
    .expect("loads");
    let mut out = Vec::new();
    paints(&doc.root, &mut out);
    out
}

#[test]
fn a_class_on_the_icon_colours_it() {
    let got = drawn(
        r#"<view class="row"><icon class="red" name="heart" size="16px" /></view>"#,
        ".row { color: #0000ff; } .red { color: #ff0000; }",
    );
    let stroke = got[0].0.expect("an outline icon is stroked");
    assert!(near(stroke, 1.0, 0.0, 0.0), "the icon's own color, not its parent's blue: {stroke:?}");
}

#[test]
fn style_on_the_icon_colours_it() {
    let got = drawn(
        r##"<view class="row"><icon name="heart" size="16px" style="color: #00ff00" /></view>"##,
        ".row { color: #0000ff; }",
    );
    assert!(near(got[0].0.unwrap(), 0.0, 1.0, 0.0), "{got:?}");
}

#[test]
fn a_filled_icon_is_filled_in_its_own_color() {
    let got = drawn(
        r#"<icon class="red" name="heart" variant="filled" size="16px" />"#,
        ".red { color: #ff0000; }",
    );
    assert!(near(got[0].1.expect("filled"), 1.0, 0.0, 0.0), "{got:?}");
    assert!(got[0].0.is_none());
}

#[test]
fn with_no_color_of_its_own_it_still_takes_the_parents() {
    let got = drawn(
        r#"<view class="row"><icon name="heart" size="16px" /></view>"#,
        ".row { color: #0000ff; }",
    );
    assert!(near(got[0].0.unwrap(), 0.0, 0.0, 1.0), "{got:?}");
}

#[test]
fn a_stroke_written_on_the_icon_beats_its_color() {
    let got = drawn(
        r#"<icon class="a" name="heart" size="16px" />"#,
        ".a { color: #ff0000; stroke: #00ff00; }",
    );
    assert!(near(got[0].0.unwrap(), 0.0, 1.0, 0.0), "{got:?}");
    assert!(got[0].1.is_none(), "an outline icon stays unfilled: {got:?}");
}

#[test]
fn a_fill_written_on_an_outline_icon_fills_it_and_the_lines_keep_color() {
    let got = drawn(
        r#"<icon class="a" name="heart" size="16px" />"#,
        ".a { color: #ff0000; fill: #0000ff; }",
    );
    assert!(near(got[0].0.unwrap(), 1.0, 0.0, 0.0), "{got:?}");
    assert!(near(got[0].1.expect("filled"), 0.0, 0.0, 1.0), "{got:?}");
}

#[test]
fn a_fill_written_on_a_filled_icon_beats_its_color() {
    let got = drawn(
        r#"<icon class="a" name="heart" variant="filled" size="16px" />"#,
        ".a { color: #ff0000; fill: #0000ff; }",
    );
    assert!(near(got[0].1.unwrap(), 0.0, 0.0, 1.0), "{got:?}");
}

#[test]
fn a_hover_rule_recolours_the_icon_and_leaving_restores_it() {
    use rux_style::InteractionState;
    let mut icons = MemoryIcons::new(24.0);
    icons.insert(
        "heart",
        vec![IconPath { d: "M 0 0 L 24 24".into(), fill_current: false, no_stroke: false, half_opacity: false }],
        vec![],
    );
    set_icons(Rc::new(icons));
    let mut doc = Document::from_source(
        r#"<template><screen><icon class="a" name="heart" size="16px" /></screen></template>
           <style>.a { color: #ff0000; } .a:hover { color: #00ff00; }</style>"#,
    )
    .expect("loads");
    let read = |doc: &Document| {
        let mut out = Vec::new();
        paints(&doc.root, &mut out);
        out[0].0.unwrap()
    };
    assert!(near(read(&doc), 1.0, 0.0, 0.0));
    doc.set_interaction(InteractionState { hovered: Some(vec![0]), ..InteractionState::default() });
    assert!(near(read(&doc), 0.0, 1.0, 0.0), "hovered: {:?}", read(&doc));
    doc.set_interaction(InteractionState::default());
    assert!(near(read(&doc), 1.0, 0.0, 0.0), "after leaving: {:?}", read(&doc));
}
