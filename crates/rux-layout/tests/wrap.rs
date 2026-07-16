use rux_layout::*;

fn boxed(style: Style, children: Vec<Node>) -> Node {
    let mut n = Node::new(style);
    n.children = children;
    n
}

fn all_rects(root: &Node, w: f32, h: f32) -> Vec<(f32, f32, f32, f32)> {
    let mut measure = |_: &rux_layout::TextContent, _: Option<f32>| (0.0, 0.0);
    layout(root, w, h, &mut measure)
        .paints
        .iter()
        .filter_map(|p| match p {
            Paint::Rect(r) => Some((r.x, r.y, r.width, r.height)),
            _ => None,
        })
        .collect()
}

/// A `flex-wrap` grid whose width is a percentage (`width: 100%`) sits in a
/// flex column, followed by a sibling. The grid holds eight 64px boxes and, at
/// this width, must wrap onto two rows. The bug: taffy measures the grid's
/// height as if everything fits on one row (max-content), so the column places
/// the sibling as if the grid were one row tall — while the grid actually paints
/// the 8th box on a second row, under the sibling. The sibling must sit below
/// the whole grid, not on top of the wrapped row.
#[test]
fn wrapped_grid_reserves_height_for_every_row() {
    let thumb = || {
        boxed(
            Style {
                width: Some(Len::Px(64.0)),
                height: Some(Len::Px(64.0)),
                shrink: 0.0,
                background: Some(Background::Color(Rgba::new(0.5, 0.5, 0.5, 1.0))),
                ..Default::default()
            },
            vec![],
        )
    };
    let grid = boxed(
        Style {
            display: Display::Flex,
            axis: Axis::Row,
            wrap: true,
            gap: 8.0,
            width: Some(Len::Pct(1.0)),
            max_width: Some(Len::Px(520.0)),
            ..Default::default()
        },
        (0..8).map(|_| thumb()).collect(),
    );
    // A distinct-width sentinel so we can pick it out of the paint list.
    let sentinel = boxed(
        Style {
            width: Some(Len::Px(200.0)),
            height: Some(Len::Px(20.0)),
            background: Some(Background::Color(Rgba::new(1.0, 0.0, 0.0, 1.0))),
            ..Default::default()
        },
        vec![],
    );
    let screen = boxed(
        Style {
            display: Display::Flex,
            axis: Axis::Column,
            gap: 12.0,
            ..Default::default()
        },
        vec![grid, sentinel],
    );

    let rects = all_rects(&screen, 1260.0, 790.0);
    // Only the thumbs (64px) and the sentinel (200px) have a background, so
    // those are the only rects. The grid and screen paint nothing themselves.
    let thumbs: Vec<_> = rects.iter().filter(|r| r.2 == 64.0).collect();
    let sentinel = *rects.iter().find(|r| r.2 == 200.0).expect("sentinel");
    assert_eq!(thumbs.len(), 8, "all eight thumbs should paint");

    let thumbs_bottom = thumbs
        .iter()
        .map(|t| t.1 + t.3)
        .fold(0.0_f32, f32::max);

    // At 520px the row fits 7 thumbs, so the 8th wraps: two rows, ~136px tall.
    assert!(
        thumbs_bottom > 100.0,
        "expected the thumbs to wrap onto a second row (bottom {thumbs_bottom})"
    );
    // The sentinel must sit below the whole grid, not overlap the wrapped row.
    assert!(
        sentinel.1 >= thumbs_bottom - 0.5,
        "sentinel (y={}) overlaps the wrapped thumbnails (bottom {thumbs_bottom})",
        sentinel.1
    );
}
