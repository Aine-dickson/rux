use rux_layout::*;

fn boxed(style: Style, children: Vec<Node>) -> Node {
    let mut n = Node::new(style);
    n.children = children;
    n
}

fn flex(axis: Axis) -> Style {
    Style {
        display: Display::Flex,
        axis,
        ..Default::default()
    }
}

/// Lay the tree out under a full-viewport screen. (`layout()` stretches its
/// root to fill the window, so a fixed-width box has to be a child to stay
/// fixed-width.)
fn on_screen(node: Node) -> Node {
    boxed(flex(Axis::Column), vec![node])
}

fn paints(root: &Node) -> Vec<Paint> {
    let mut measure = |_: &rux_layout::TextContent, _: Option<f32>| (0.0, 0.0);
    layout(&on_screen(root.clone()), 1260.0, 790.0, &mut measure).paints
}

fn rects(root: &Node) -> Vec<(f32, f32)> {
    let mut measure = |_: &rux_layout::TextContent, _: Option<f32>| (0.0, 0.0);
    layout(&on_screen(root.clone()), 1260.0, 790.0, &mut measure)
        .paints
        .iter()
        .filter_map(|p| match p {
            Paint::Rect(r) => Some((r.x, r.width)),
            _ => None,
        })
        .collect()
}

/// A box with no width hugs its content, and hug means CSS `fit-content`:
/// min(max-content, available). A row of three 320px boxes inside a 320px card
/// used to be handed its full 976px max-content width and burst out through the
/// side of the card. It must clamp to the card's inner width and let the
/// children shrink.
#[test]
fn hugging_box_clamps_to_parent_inner_width() {
    let child = || {
        boxed(
            Style {
                width: Some(Len::Px(320.0)),
                background: Some(Rgba::new(0.0, 0.0, 0.0, 1.0)),
                ..flex(Axis::Row)
            },
            vec![],
        )
    };
    let row = boxed(
        Style {
            gap: 8.0,
            background: Some(Rgba::new(0.1, 0.1, 0.1, 1.0)),
            ..flex(Axis::Row)
        },
        vec![child(), child(), child()],
    );
    let card = boxed(
        Style {
            width: Some(Len::Px(320.0)),
            padding: Sides::uniform(16.0),
            background: Some(Rgba::new(0.2, 0.2, 0.2, 1.0)),
            ..flex(Axis::Column)
        },
        vec![row],
    );
    let screen = boxed(
        Style {
            padding: Sides::uniform(24.0),
            ..flex(Axis::Column)
        },
        vec![card],
    );

    let rects = rects(&screen);
    let (card_x, card_w) = rects[0];
    let (row_x, row_w) = rects[1];
    assert_eq!((card_x, card_w), (24.0, 320.0));
    assert_eq!((row_x, row_w), (40.0, 288.0), "row escaped the card");

    for &(x, w) in &rects[2..] {
        assert!(
            x + w <= card_x + card_w,
            "child at {x} + {w} overflows the card's right edge"
        );
    }
}

/// The clamp is a default for hugging boxes only — an explicit width is the
/// author's call, even when it overflows.
#[test]
fn explicit_width_is_left_alone() {
    let wide = boxed(
        Style {
            width: Some(Len::Px(900.0)),
            background: Some(Rgba::new(0.0, 0.0, 0.0, 1.0)),
            ..flex(Axis::Column)
        },
        vec![],
    );
    let card = boxed(
        Style {
            width: Some(Len::Px(320.0)),
            background: Some(Rgba::new(0.2, 0.2, 0.2, 1.0)),
            ..flex(Axis::Column)
        },
        vec![wide],
    );

    let rects = rects(&card);
    assert_eq!(rects[1].1, 900.0, "explicit width was clamped");
}

/// `flex-shrink: 0` means "keep my size". The item must overflow rather than be
/// quietly shrunk -- and must not be clamped by the fit-content default either.
#[test]
fn shrink_zero_keeps_its_width_and_overflows() {
    let stiff = |w: f32| {
        boxed(
            Style {
                width: Some(Len::Px(w)),
                shrink: 0.0,
                background: Some(Rgba::new(0.0, 0.0, 0.0, 1.0)),
                ..flex(Axis::Row)
            },
            vec![],
        )
    };
    let card = boxed(
        Style {
            width: Some(Len::Px(300.0)),
            background: Some(Rgba::new(0.2, 0.2, 0.2, 1.0)),
            ..flex(Axis::Row)
        },
        vec![stiff(200.0), stiff(200.0)],
    );

    let rects = rects(&card);
    assert_eq!(rects[1].1, 200.0, "shrink:0 item was shrunk anyway");
    assert_eq!(rects[2].1, 200.0, "shrink:0 item was shrunk anyway");
    assert!(
        rects[2].0 + rects[2].1 > 300.0,
        "shrink:0 items should overflow the 300px card"
    );
}

/// The default is still CSS's shrink: 1 -- items give up space to fit.
#[test]
fn default_items_shrink_to_fit() {
    let item = || {
        boxed(
            Style {
                width: Some(Len::Px(200.0)),
                background: Some(Rgba::new(0.0, 0.0, 0.0, 1.0)),
                ..flex(Axis::Row)
            },
            vec![],
        )
    };
    let card = boxed(
        Style {
            width: Some(Len::Px(300.0)),
            background: Some(Rgba::new(0.2, 0.2, 0.2, 1.0)),
            ..flex(Axis::Row)
        },
        vec![item(), item()],
    );

    let rects = rects(&card);
    assert_eq!(rects[1].1, 150.0);
    assert_eq!(rects[2].1, 150.0);
}

/// `flex-wrap: wrap` sends the overflowing item to a second line.
#[test]
fn flex_wrap_starts_a_new_line() {
    let item = || {
        boxed(
            Style {
                width: Some(Len::Px(200.0)),
                shrink: 0.0,
                height: Some(Len::Px(20.0)),
                background: Some(Rgba::new(0.0, 0.0, 0.0, 1.0)),
                ..flex(Axis::Row)
            },
            vec![],
        )
    };
    let card = boxed(
        Style {
            width: Some(Len::Px(300.0)),
            wrap: true,
            background: Some(Rgba::new(0.2, 0.2, 0.2, 1.0)),
            ..flex(Axis::Row)
        },
        vec![item(), item()],
    );

    let ys: Vec<f32> = paints(&card)
        .iter()
        .filter_map(|p| match p {
            Paint::Rect(r) => Some(r.y),
            _ => None,
        })
        .collect();
    assert!(ys[2] > ys[1], "second item should wrap onto a new line");
}
