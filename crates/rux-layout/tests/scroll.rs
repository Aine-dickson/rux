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
/// 200px box and there is nothing to scroll, the same trap CSS has.
fn scroller() -> Node {
    let row = || {
        boxed(
            Style {
                display: Display::Flex,
                width: Some(Len::Px(300.0)),
                height: Some(Len::Px(100.0)),
                shrink: 0.0,
                background: Some(Background::Color(Rgba::new(0.0, 0.0, 0.0, 1.0))),
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
fn run(offsets: &[Offset]) -> Layout {
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
    assert_eq!(region.max.y, 300.0);
    assert_eq!(region.height, 200.0);
    assert_eq!(region.content_height, 500.0);
    // The rows are exactly as wide as the box, so there is nothing to scroll
    // sideways.
    assert_eq!(region.max.x, 0.0);
}

/// Scrolling moves the content up under a clip; the box itself stays put.
#[test]
fn offset_shifts_the_content_not_the_box() {
    let unscrolled = run(&[Offset::default()]);
    let scrolled = run(&[Offset { x: 0.0, y: 150.0 }]);

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

/// An offset past the end is clamped, you can't scroll into empty space.
#[test]
fn offset_is_clamped_to_the_content() {
    let overscrolled = row_tops(&run(&[Offset { x: 0.0, y: 10_000.0 }]));
    let at_end = row_tops(&run(&[Offset { x: 0.0, y: 300.0 }]));
    assert_eq!(overscrolled, at_end);
}

/// The same scroller, but with rows wider than the box: 500px of content in a
/// 300px-wide box, so it scrolls sideways as well as down.
fn wide_run(offsets: &[Offset]) -> Layout {
    let mut node = scroller();
    for row in &mut node.children {
        row.style.width = Some(Len::Px(500.0));
    }
    let mut measure = |_: &rux_layout::TextContent, _: Option<f32>| (0.0, 0.0);
    let screen = boxed(
        Style {
            display: Display::Flex,
            axis: Axis::Column,
            ..Default::default()
        },
        vec![node],
    );
    layout_scrolled(&screen, 400.0, 600.0, offsets, &mut measure)
}

fn row_lefts(layout: &Layout) -> Vec<f32> {
    layout
        .paints
        .iter()
        .filter_map(|p| match p {
            Paint::Rect(r) => Some(r.x),
            _ => None,
        })
        .collect()
}

/// Content wider than its box reports horizontal travel: 500 - 300.
#[test]
fn horizontal_extent_is_reported() {
    let layout = wide_run(&[]);
    let region = &layout.scrolls[0];
    assert_eq!(region.max.x, 200.0);
    assert_eq!(region.content_width, 500.0);
}

/// A horizontal offset shifts the content left, independently of the vertical one.
#[test]
fn horizontal_offset_shifts_the_content() {
    let before = row_lefts(&wide_run(&[Offset::default()]));
    let after = row_lefts(&wide_run(&[Offset { x: 80.0, y: 0.0 }]));
    for (b, a) in before.iter().zip(after.iter()) {
        assert_eq!(*b - *a, 80.0, "every row should shift left by the offset");
    }
    // …and the vertical offset is untouched by an x-only scroll.
    assert_eq!(
        row_tops(&wide_run(&[Offset { x: 80.0, y: 0.0 }])),
        row_tops(&wide_run(&[Offset::default()]))
    );
}

/// Each axis clamps on its own extent, not the other's.
#[test]
fn horizontal_offset_is_clamped() {
    let overscrolled = row_lefts(&wide_run(&[Offset { x: 10_000.0, y: 0.0 }]));
    let at_end = row_lefts(&wide_run(&[Offset { x: 200.0, y: 0.0 }]));
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

/// A flex scroller's end padding is counted once. Taffy's content size for a
/// flex container already ends with it; adding it again let the box travel its
/// padding too far, and a row whose children exactly filled it showed a
/// scrollbar with nothing to scroll.
#[test]
fn a_flex_scroller_counts_its_end_padding_once() {
    let pad = Sides { top: 8.0, right: 8.0, bottom: 8.0, left: 8.0 };
    let row = |h: f32| {
        boxed(
            Style {
                display: Display::Flex,
                width: Some(Len::Px(100.0)),
                height: Some(Len::Px(h)),
                shrink: 0.0,
                ..Default::default()
            },
            vec![],
        )
    };
    let scroller = |children: Vec<Node>| {
        boxed(
            Style {
                display: Display::Flex,
                axis: Axis::Column,
                width: Some(Len::Px(300.0)),
                height: Some(Len::Px(96.0)),
                padding: pad,
                box_sizing: BoxSizing::BorderBox,
                overflow: Overflow::Scroll,
                ..Default::default()
            },
            children,
        )
    };
    let mut measure = |_: &rux_layout::TextContent, _: Option<f32>| (0.0, 0.0);
    let run = |inner: Node, measure: &mut dyn FnMut(&rux_layout::TextContent, Option<f32>) -> (f32, f32)| {
        let screen = boxed(Style { display: Display::Flex, axis: Axis::Column, ..Default::default() }, vec![inner]);
        layout_scrolled(&screen, 400.0, 600.0, &[], measure)
    };

    // 8 + 80 + 8 fills 96 exactly: nothing to scroll.
    let exact = run(scroller(vec![row(80.0)]), &mut measure);
    assert_eq!(exact.scrolls[0].max.y, 0.0, "{:?}", exact.scrolls[0]);

    // 8 + 200 + 8 in a 96 box: 120 of travel, the bottom padding counted once.
    let over = run(scroller(vec![row(200.0)]), &mut measure);
    assert_eq!(over.scrolls[0].max.y, 120.0, "{:?}", over.scrolls[0]);
}

/// A `fixed` box stays put when the `<screen>` itself is what scrolls
/// (watchlist #43). Fixed boxes are handed to the root, and the root's offset
/// used to move every child it had, so a header written inside a scrolling
/// screen scrolled out of view. In a browser the viewport scrolls and a fixed
/// box does not; content written beside it still does.
#[test]
fn a_fixed_box_stays_put_when_the_screen_itself_scrolls() {
    let red = Rgba::new(1.0, 0.0, 0.0, 1.0);
    let tall = boxed(
        Style { height: Some(Len::Px(2000.0)), shrink: 0.0, ..Default::default() },
        vec![],
    );
    let header = boxed(
        Style {
            position: Position::Fixed,
            inset: [Some(Len::Px(0.0)), Some(Len::Px(0.0)), None, Some(Len::Px(0.0))],
            height: Some(Len::Px(40.0)),
            background: Some(Background::Color(red)),
            ..Default::default()
        },
        vec![],
    );
    let marker = boxed(
        Style {
            height: Some(Len::Px(10.0)),
            shrink: 0.0,
            background: Some(Background::Color(Rgba::new(0.0, 1.0, 0.0, 1.0))),
            ..Default::default()
        },
        vec![],
    );
    let screen = boxed(
        Style { display: Display::Flex, axis: Axis::Column, overflow: Overflow::Scroll, ..Default::default() },
        vec![header, marker, tall],
    );
    let mut measure = |_: &TextContent, _: Option<f32>| (0.0, 0.0);
    let out = layout_scrolled(&screen, 400.0, 300.0, &[Offset { x: 0.0, y: 500.0 }], &mut measure);
    let y_of = |r: f32| {
        out.paints
            .iter()
            .find_map(|p| match p {
                Paint::Rect(PaintRect { background: Some(Background::Color(c)), y, .. }) if c.r == r && c.g == 1.0 - r => Some(*y),
                _ => None,
            })
            .expect("painted")
    };
    assert_eq!(y_of(1.0), 0.0, "the fixed header is still at the top of the window");
    assert_eq!(y_of(0.0), -500.0, "the content beside it scrolled");
}

/// A scroller wider and taller than its box, that scrolls only down: the
/// sideways overflow is clipped, with no range to travel (`overflow-x:
/// hidden; overflow-y: auto`).
#[test]
fn an_axis_that_only_clips_has_no_range() {
    let mut measure = |_: &rux_layout::TextContent, _: Option<f32>| (0.0, 0.0);
    let wide = boxed(
        Style { width: Some(Len::Px(600.0)), height: Some(Len::Px(500.0)), shrink: 0.0, ..Default::default() },
        vec![],
    );
    let scroller = |overflow_x: Option<Overflow>| {
        boxed(
            Style {
                width: Some(Len::Px(300.0)),
                height: Some(Len::Px(200.0)),
                overflow: Overflow::Scroll,
                overflow_x,
                ..Default::default()
            },
            vec![wide.clone()],
        )
    };
    let screen = |s: Node| boxed(Style { display: Display::Flex, axis: Axis::Column, ..Default::default() }, vec![s]);
    let both = layout_scrolled(&screen(scroller(None)), 400.0, 600.0, &[], &mut measure);
    assert_eq!((both.scrolls[0].max.x, both.scrolls[0].max.y), (300.0, 300.0));
    let down = layout_scrolled(&screen(scroller(Some(Overflow::Clip))), 400.0, 600.0, &[], &mut measure);
    assert_eq!((down.scrolls[0].max.x, down.scrolls[0].max.y), (0.0, 300.0), "sideways is clipped, not scrolled");
}
