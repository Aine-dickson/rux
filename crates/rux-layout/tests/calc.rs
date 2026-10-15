//! `calc()`, `min()`, `max()` and `clamp()` with a percentage, which Taffy
//! works out once it knows what the percentage is of (vendor/README.md).

use rux_layout::*;

fn colour(r: f32) -> Option<Background> {
    Some(Background::Color(Rgba::new(r, 0.0, 0.0, 1.0)))
}

fn calc(text: &str) -> Option<Len> {
    Some(Len::Calc(calc::intern(text).expect("maths")))
}

fn rect(out: &Layout, r: f32) -> Option<(f32, f32, f32, f32)> {
    out.paints.iter().find_map(|p| match p {
        Paint::Rect(PaintRect { background: Some(Background::Color(c)), x, y, width, height, .. }) if c.r == r => {
            Some((*x, *y, *width, *height))
        }
        _ => None,
    })
}

fn lay(root: &Node) -> Layout {
    let mut measure = |_: &TextContent, _: Option<f32>| (10.0, 10.0);
    layout(root, 800.0, 600.0, &mut measure)
}

fn boxed(style: Style, children: Vec<Node>) -> Node {
    let mut n = Node::new(style);
    n.children = children;
    n
}

/// Against a parent of a known width, and in `min()` and `clamp()`.
#[test]
fn a_percentage_in_maths_is_of_the_parent() {
    let parent = |child: Style| {
        boxed(Style::default(), vec![boxed(
            Style { width: Some(Len::Px(300.0)), height: Some(Len::Px(100.0)), ..Default::default() },
            vec![Node::new(Style { background: colour(0.5), height: Some(Len::Px(10.0)), ..child })],
        )])
    };
    let width = |child: Style| rect(&lay(&parent(child)), 0.5).map(|r| r.2);
    assert_eq!(width(Style { width: calc("calc(100% - 20px)"), ..Default::default() }), Some(280.0));
    assert_eq!(width(Style { width: calc("min(50%, 100px)"), ..Default::default() }), Some(100.0));
    assert_eq!(width(Style { width: calc("clamp(10px, 10%, 50px)"), ..Default::default() }), Some(30.0));
    assert_eq!(
        width(Style { width: Some(Len::Pct(1.0)), max_width: calc("calc(100% - 40px)"), ..Default::default() }),
        Some(260.0)
    );
}

/// An absolutely positioned box's insets are of its containing block.
#[test]
fn an_inset_in_maths_is_of_the_containing_block() {
    let root = boxed(Style::default(), vec![boxed(
        Style {
            width: Some(Len::Px(300.0)),
            height: Some(Len::Px(200.0)),
            position: Position::Relative,
            ..Default::default()
        },
        vec![Node::new(Style {
            background: colour(0.5),
            position: Position::Absolute,
            width: Some(Len::Px(20.0)),
            height: Some(Len::Px(20.0)),
            inset: [calc("calc(50% - 10px)"), None, None, calc("calc(50% - 10px)")],
            ..Default::default()
        })],
    )]);
    assert_eq!(rect(&lay(&root), 0.5), Some((140.0, 90.0, 20.0, 20.0)));
}

/// A parent sized by its content does not shrink round and round: the
/// percentage is of the size the parent came to without it, as in CSS.
#[test]
fn a_parent_sized_by_its_content_holds_still() {
    let hugging = boxed(
        Style { display: Display::Flex, axis: Axis::Column, background: colour(0.2), ..Default::default() },
        vec![
            Node::new(Style { width: Some(Len::Px(200.0)), height: Some(Len::Px(10.0)), ..Default::default() }),
            Node::new(Style {
                background: colour(0.5),
                width: calc("calc(100% - 20px)"),
                height: Some(Len::Px(10.0)),
                ..Default::default()
            }),
        ],
    );
    let root = boxed(Style { display: Display::Flex, axis: Axis::Row, ..Default::default() }, vec![hugging]);
    let mut state = LayoutState::new();
    let mut measure = |_: &TextContent, _: Option<f32>| (10.0, 10.0);
    for _ in 0..3 {
        let out = state.layout_scrolled(&root, 800.0, 600.0, &[], &mut measure);
        assert_eq!(rect(&out, 0.2).map(|r| r.2), Some(200.0));
        assert_eq!(rect(&out, 0.5).map(|r| r.2), Some(180.0));
    }
}
