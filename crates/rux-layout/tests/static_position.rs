//! An absolutely positioned box with no insets keeps its **static position**:
//! where it would have been in normal flow.
//!
//! taffy has no such concept and places such a box at its parent's content-box
//! origin. Before this it meant anything taken out of the flow jumped to the top
//! of its parent, which is what sent a departing page up over the nav bar during
//! a route transition, and why overlaying two pages needed a wrapper box and an
//! explicit `top` to look right at all.

use rux_layout::*;

fn box_of(w: f32, h: f32, position: Position) -> Node {
    Node::new(Style {
        width: Some(Len::Px(w)),
        height: Some(Len::Px(h)),
        position,
        ..Default::default()
    })
}

/// Two boxes stacked in a padded column, the second optionally out of flow.
fn column(second: Position) -> Vec<PaintRect> {
    let mut screen = Node::new(Style {
        display: Display::Flex,
        axis: Axis::Column,
        padding: Sides::uniform(30.0),
        background: Some(Background::Color(Rgba::new(0.0, 0.0, 0.0, 1.0))),
        ..Default::default()
    });
    screen.children.push(box_of(40.0, 50.0, Position::Relative));
    let mut detached = box_of(40.0, 50.0, second);
    detached.style.background = Some(Background::Color(Rgba::new(1.0, 0.0, 0.0, 1.0)));
    screen.children.push(detached);

    let mut measure = |_: &TextContent, _: Option<f32>| (50.0, 20.0);
    layout(&screen, 1000.0, 800.0, &mut measure)
        .paints
        .into_iter()
        .filter_map(|p| match p {
            Paint::Rect(r) => Some(r),
            _ => None,
        })
        .collect()
}

/// The one that matters: taken out of the flow and naming no inset, it does not
/// move at all.
#[test]
fn an_absolute_box_with_no_insets_stays_where_it_was() {
    let in_flow = column(Position::Relative);
    let detached = column(Position::Absolute);

    // The red box is the second one in each.
    let flowed = in_flow.last().expect("a rect");
    let floated = detached.last().expect("a rect");

    assert!(flowed.y > 30.0, "it sits below the first box, not at the padding edge");
    assert_eq!(
        (floated.x, floated.y),
        (flowed.x, flowed.y),
        "out of the flow but in the same place: that is the static position"
    );
}

/// A box that *does* name an inset is asking to be placed against its parent,
/// and still is. That is what `top: 0` means, and it must not regress.
#[test]
fn a_named_inset_still_positions_against_the_parent() {
    let mut screen = Node::new(Style {
        display: Display::Flex,
        axis: Axis::Column,
        padding: Sides::uniform(30.0),
        ..Default::default()
    });
    screen.children.push(box_of(40.0, 50.0, Position::Relative));
    let mut pinned = box_of(40.0, 50.0, Position::Absolute);
    pinned.style.inset[0] = Some(Len::Px(0.0)); // top
    pinned.style.background = Some(Background::Color(Rgba::new(1.0, 0.0, 0.0, 1.0)));
    screen.children.push(pinned);

    let mut measure = |_: &TextContent, _: Option<f32>| (50.0, 20.0);
    let rects: Vec<PaintRect> = layout(&screen, 1000.0, 800.0, &mut measure)
        .paints
        .into_iter()
        .filter_map(|p| match p {
            Paint::Rect(r) => Some(r),
            _ => None,
        })
        .collect();
    let pinned = rects.last().expect("a rect");
    // Insets run from the parent's *border* box here, so `top: 0` is the
    // parent's own top edge and not inside its padding. Recorded because it is
    // the fact the static-position pass depends on.
    assert_eq!(pinned.y, 0.0, "`top: 0` is the parent's border edge, as before");
}

/// Two out-of-flow siblings with no insets land in the *same* place.
///
/// Neither occupies space, so neither can push the other down: the static
/// position of the second is where it would have stood in normal flow, and in
/// a flow that the first one has already left, that is the top.
///
/// Found by looking at `examples/chart.rux`, where a filled band and the line
/// over it are two absolutely positioned paths over the same points. The line
/// was drawn a whole band's height below the band, out of the frame entirely
/// and across the buttons underneath it.
#[test]
fn two_out_of_flow_siblings_do_not_stack() {
    let mut screen = Node::new(Style {
        display: Display::Flex,
        axis: Axis::Column,
        padding: Sides::uniform(30.0),
        ..Default::default()
    });
    for _ in 0..2 {
        let mut b = box_of(40.0, 50.0, Position::Absolute);
        b.style.background = Some(Background::Color(Rgba::new(1.0, 0.0, 0.0, 1.0)));
        screen.children.push(b);
    }

    let mut measure = |_: &TextContent, _: Option<f32>| (50.0, 20.0);
    let rects: Vec<PaintRect> = layout(&screen, 1000.0, 800.0, &mut measure)
        .paints
        .into_iter()
        .filter_map(|p| match p {
            Paint::Rect(r) => Some(r),
            _ => None,
        })
        .filter(|r| r.width == 40.0)
        .collect();

    assert_eq!(rects.len(), 2, "both boxes painted");
    assert_eq!(
        rects[0].y, rects[1].y,
        "an out-of-flow box holds no space, so it cannot push its out-of-flow \
         sibling down: {} then {}",
        rects[0].y, rects[1].y
    );
    assert_eq!(rects[0].y, 30.0, "and both sit at the content origin");
}
