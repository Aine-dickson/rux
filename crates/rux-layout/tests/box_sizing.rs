//! `box-sizing`: whether `width` and `height` take the padding and border in.

use rux_layout::*;

fn rect(out: &Layout, r: f32) -> Option<(f32, f32)> {
    out.paints.iter().find_map(|p| match p {
        Paint::Rect(PaintRect { background: Some(Background::Color(c)), width, height, .. }) if c.r == r => {
            Some((*width, *height))
        }
        _ => None,
    })
}

fn lay(root: &Node) -> Layout {
    let mut measure = |_: &TextContent, _: Option<f32>| (10.0, 10.0);
    layout(root, 800.0, 600.0, &mut measure)
}

fn sized(box_sizing: BoxSizing) -> (f32, f32) {
    let child = Node::new(Style {
        background: Some(Background::Color(Rgba::new(0.5, 0.0, 0.0, 1.0))),
        width: Some(Len::Px(100.0)),
        height: Some(Len::Px(40.0)),
        padding: Sides { top: 10.0, right: 20.0, bottom: 10.0, left: 20.0 },
        box_sizing,
        ..Default::default()
    });
    let mut root = Node::new(Style::default());
    root.children = vec![child];
    rect(&lay(&root), 0.5).expect("the child is painted")
}

/// Rux's default holds the padding inside the width; `content-box` adds it.
#[test]
fn content_box_adds_the_padding_to_the_width() {
    assert_eq!(sized(BoxSizing::BorderBox), (100.0, 40.0));
    assert_eq!(sized(BoxSizing::ContentBox), (140.0, 60.0));
}
