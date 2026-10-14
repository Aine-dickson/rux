//! `direction: rtl` mirrors a layout: rows start from the right.

use rux_layout::*;

fn cell() -> Node {
    Node::new(Style { width: Some(Len::Px(50.0)), height: Some(Len::Px(20.0)), ..Default::default() })
}

fn row(direction: Direction) -> Node {
    let mut row = Node::new(Style { display: Display::Flex, direction, ..Default::default() });
    row.children = vec![cell(), cell()];
    let mut screen = Node::new(Style { display: Display::Flex, axis: Axis::Column, direction, ..Default::default() });
    screen.children.push(row);
    screen
}

fn xs(direction: Direction) -> Vec<f32> {
    let mut measure = |_: &TextContent, _: Option<f32>| (0.0, 0.0);
    let out = layout(&row(direction), 400.0, 300.0, &mut measure);
    let mut cells: Vec<&NodeMetrics> = out.metrics.iter().filter(|m| m.path.len() == 2).collect();
    cells.sort_by_key(|m| m.path[1]);
    cells.iter().map(|m| m.x).collect()
}

#[test]
fn an_rtl_row_starts_from_the_right() {
    assert_eq!(xs(Direction::Ltr), [0.0, 50.0]);
    assert_eq!(xs(Direction::Rtl), [350.0, 300.0]);
}

/// Six 50px cells in a 100px right-to-left scroller.
fn strip(offset: f32) -> (Layout, Vec<f32>) {
    let mut strip = Node::new(Style {
        display: Display::Flex,
        direction: Direction::Rtl,
        overflow: Overflow::Scroll,
        width: Some(Len::Px(100.0)),
        height: Some(Len::Px(20.0)),
        ..Default::default()
    });
    strip.children = (0..6).map(|_| Node::new(Style {
        width: Some(Len::Px(50.0)),
        height: Some(Len::Px(20.0)),
        shrink: 0.0,
        ..Default::default()
    })).collect();
    let mut screen = Node::new(Style { display: Display::Flex, axis: Axis::Column, ..Default::default() });
    screen.children.push(strip);
    let mut measure = |_: &TextContent, _: Option<f32>| (0.0, 0.0);
    let out = layout_scrolled(&screen, 400.0, 300.0, &[Offset { x: offset, y: 0.0 }], &mut measure);
    let mut cells: Vec<&NodeMetrics> = out.metrics.iter().filter(|m| m.path.len() == 2).collect();
    cells.sort_by_key(|m| m.path[1]);
    let xs = cells.iter().map(|m| m.x).collect();
    (out, xs)
}

/// A right-to-left scroller opens on its first cell at the right, can travel
/// as far as its content reaches to the left, and scrolling brings that in.
#[test]
fn an_rtl_scroller_starts_at_the_right_and_scrolls_left() {
    let (out, xs) = strip(0.0);
    let s = &out.scrolls[0];
    assert!(s.rtl);
    assert_eq!(s.max.x, 200.0, "the four cells past the box, all to its left");
    assert_eq!(xs[0], 50.0, "the first cell at the right of the box");
    let (_, xs) = strip(200.0);
    assert_eq!(xs[5], 0.0, "scrolled all the way, the last cell at the left");
}
