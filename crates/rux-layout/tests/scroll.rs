use rux_layout::*;

fn boxed(style: Style, children: Vec<Node>) -> Node {
    let mut n = Node::new(style);
    n.children = children;
    n
}

/// A 200px-tall scroller holding 5 x 100px rows: 500px of content, 300px of it
/// out of view.
///
/// The rows need `flex-shrink: 0` or the flex column squeezes all five into the
/// 200px box and there is nothing to scroll — the same trap CSS has.
fn scroller() -> Node {
    let row = || {
        boxed(
            Style {
                display: Display::Flex,
                width: Some(Len::Px(300.0)),
                height: Some(Len::Px(100.0)),
                shrink: 0.0,
                background: Some(Rgba::new(0.0, 0.0, 0.0, 1.0)),
                ..Default::default()
            },
            vec![],
        )
    };
    boxed(
        Style {
            display: Display::Flex,
            axis: Axis::Column,
            width: Some(Len::Px(300.0)),
            height: Some(Len::Px(200.0)),
            overflow: Overflow::Scroll,
            ..Default::default()
        },
        vec![row(), row(), row(), row(), row()],
    )
}

/// The scroller must be a child, not the root: `layout()` force-sizes its root
/// to the viewport.
fn run(offsets: &[f32]) -> Layout {
    let mut measure = |_: &rux_layout::TextContent, _: Option<f32>| (0.0, 0.0);
    let screen = boxed(
        Style {
            display: Display::Flex,
            axis: Axis::Column,
            ..Default::default()
        },
        vec![scroller()],
    );
    layout_scrolled(&screen, 400.0, 600.0, offsets, &mut measure)
}

fn row_tops(layout: &Layout) -> Vec<f32> {
    layout
        .paints
        .iter()
        .filter_map(|p| match p {
            Paint::Rect(r) => Some(r.y),
            _ => None,
        })
        .collect()
}

/// The scroller reports how far it can travel: content (500) - visible (200).
#[test]
fn scroll_region_reports_its_extent() {
    let layout = run(&[]);
    assert_eq!(layout.scrolls.len(), 1);
    let region = &layout.scrolls[0];
    assert_eq!(region.max_offset, 300.0);
    assert_eq!(region.height, 200.0);
}

/// Scrolling moves the content up under a clip; the box itself stays put.
#[test]
fn offset_shifts_the_content_not_the_box() {
    let unscrolled = run(&[0.0]);
    let scrolled = run(&[150.0]);

    let before = row_tops(&unscrolled);
    let after = row_tops(&scrolled);
    for (b, a) in before.iter().zip(after.iter()) {
        assert_eq!(*b - *a, 150.0, "every row should shift up by the offset");
    }
    assert!(
        unscrolled
            .paints
            .iter()
            .any(|p| matches!(p, Paint::PushClip { .. })),
        "a scroller must clip its content"
    );
}

/// An offset past the end is clamped — you can't scroll into empty space.
#[test]
fn offset_is_clamped_to_the_content() {
    let overscrolled = row_tops(&run(&[10_000.0]));
    let at_end = row_tops(&run(&[300.0]));
    assert_eq!(overscrolled, at_end);
}

/// A box that only clips doesn't scroll, so it registers no scroll region.
#[test]
fn clip_alone_does_not_scroll() {
    let mut node = scroller();
    node.style.overflow = Overflow::Clip;
    let mut measure = |_: &rux_layout::TextContent, _: Option<f32>| (0.0, 0.0);
    let screen = boxed(
        Style {
            display: Display::Flex,
            axis: Axis::Column,
            ..Default::default()
        },
        vec![node],
    );
    let layout = layout_scrolled(&screen, 400.0, 600.0, &[], &mut measure);
    assert!(layout.scrolls.is_empty());
}
