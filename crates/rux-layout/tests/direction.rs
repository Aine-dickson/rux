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
