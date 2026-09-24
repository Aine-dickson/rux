//! A stretched box's text is measured at the width it is drawn at.
//!
//! Watchlist #32, found reverting `align-items` to `stretch`: in
//! `examples/slots.rux` a button in a narrow column, stretched to the column's
//! width, drew its label on two lines while its box was one line tall.

use rux_layout::*;

fn boxed(style: Style, children: Vec<Node>) -> Node {
    let mut n = Node::new(style);
    n.children = children;
    n
}

fn label(text: &str) -> TextContent {
    TextContent {
        text: text.into(),
        font_size: 16.0,
        weight: 400,
        color: Rgba::new(1.0, 1.0, 1.0, 1.0),
        align: TextAlign::Start,
        wrap: TextWrap::Normal,
        font_family: None,
        letter_spacing: None,
        word_spacing: None,
        line_height: None,
        italic: false,
        underline: false,
        strikethrough: false,
        nowrap: false,
        caret: None,
        selection: None,
        preedit: None,
        selection_style: Default::default(),
    }
}

/// "one more" is 72 wide on one line and 36 wide on two, 20 per line.
fn measure(_: &TextContent, max: Option<f32>) -> (f32, f32) {
    match max {
        Some(w) if w < 72.0 => (36.0, 40.0),
        _ => (72.0, 20.0),
    }
}

#[test]
fn a_stretched_button_is_as_tall_as_its_wrapped_label() {
    let text = Node::text(Style::default(), label("one more"));
    let button = boxed(
        Style {
            display: Display::Flex,
            align: Some(Align::Center),
            padding: Sides { top: 8.0, right: 18.0, bottom: 8.0, left: 18.0 },
            background: Some(Background::Color(Rgba::new(0.5, 0.5, 1.0, 1.0))),
            ..Default::default()
        },
        vec![text],
    );
    // A column 104 wide: the button stretches to 104, leaving 68 for the
    // label, which is less than its 72 on one line.
    let card = boxed(
        Style {
            display: Display::Flex,
            axis: Axis::Column,
            width: Some(Len::Px(104.0)),
            ..Default::default()
        },
        vec![button],
    );
    let screen = boxed(Style { display: Display::Flex, axis: Axis::Column, ..Default::default() }, vec![card]);
    let mut m = measure;
    let out = layout(&screen, 400.0, 400.0, &mut m);
    let button_box = out
        .paints
        .iter()
        .find_map(|p| match p {
            Paint::Rect(r) => Some((r.width, r.height)),
            _ => None,
        })
        .expect("the button paints");
    let text_box = out
        .paints
        .iter()
        .find_map(|p| match p {
            Paint::Text(t) => Some((t.width, t.height)),
            _ => None,
        })
        .expect("the label paints");
    eprintln!("button {button_box:?} label {text_box:?}");
    assert_eq!(button_box.0, 104.0, "stretched to the column");
    assert!(
        button_box.1 >= text_box.1 + 16.0,
        "the button ({button_box:?}) holds its label ({text_box:?}) and its padding"
    );
}

/// The shape `examples/slots.rux` has: three panels sharing a row and
/// shrinking to fit it, a block button stretched inside the first one.
///
/// Watchlist #32. The button came out 32 tall (one line and its padding)
/// around a label drawn 40 tall (two lines). The cause was Rux's own automatic
/// `max-width: 100%` on the text, which Taffy resolved against a parent size
/// from an earlier pass while the row was still flexing; pure Taffy gets this
/// right. Text in a block box no longer takes that cap.
#[test]
fn a_block_button_in_a_shrinking_panel_is_as_tall_as_its_label() {
    fn text_measure(tc: &TextContent, max: Option<f32>) -> (f32, f32) {
        let one_line = if tc.text == "one more" { 72.0 } else { 200.0 };
        match max {
            Some(w) if w < one_line => {
                let w = w.max(1.0);
                let lines = (one_line / w).ceil();
                (w.min(one_line), 20.0 * lines)
            }
            _ => (one_line, 20.0),
        }
    }
    let panel = |content: Vec<Node>| {
        boxed(
            Style {
                display: Display::Flex,
                axis: Axis::Column,
                width: Some(Len::Pct(1.0)),
                max_width: Some(Len::Px(300.0)),
                ..Default::default()
            },
            vec![boxed(
                Style {
                    display: Display::Flex,
                    axis: Axis::Column,
                    width: Some(Len::Pct(1.0)),
                    padding: Sides::uniform(14.0),
                    ..Default::default()
                },
                content,
            )],
        )
    };
    let button = boxed(
        Style {
            padding: Sides { top: 6.0, right: 12.0, bottom: 6.0, left: 12.0 },
            background: Some(Background::Color(Rgba::new(0.5, 0.5, 1.0, 1.0))),
            ..Default::default()
        },
        vec![Node::text(Style::default(), label("one more"))],
    );
    let row = boxed(
        Style {
            display: Display::Flex,
            gap: 16.0,
            align: Some(Align::Start),
            ..Default::default()
        },
        vec![
            panel(vec![button]),
            panel(vec![Node::text(Style::default(), label("long note"))]),
            panel(vec![Node::text(Style::default(), label("long note"))]),
        ],
    );
    let app = boxed(
        Style { display: Display::Flex, axis: Axis::Column, padding: Sides::uniform(28.0), ..Default::default() },
        vec![row],
    );
    let screen = boxed(Style { display: Display::Flex, axis: Axis::Column, ..Default::default() }, vec![app]);
    let mut m = text_measure;
    let out = layout(&screen, 400.0, 800.0, &mut m);
    let button_box = out
        .paints
        .iter()
        .find_map(|p| match p {
            Paint::Rect(r) => Some((r.width, r.height)),
            _ => None,
        })
        .expect("the button paints");
    let label_box = out
        .paints
        .iter()
        .find_map(|p| match p {
            Paint::Text(t) if t.content.text == "one more" => Some((t.width, t.height)),
            _ => None,
        })
        .expect("the label paints");
    eprintln!("button {button_box:?} label {label_box:?}");
    assert!(
        button_box.1 >= label_box.1 + 12.0,
        "the button ({button_box:?}) holds its label ({label_box:?}) and its padding"
    );
}
