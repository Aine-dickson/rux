//! `position: sticky`: in flow until its scroller reaches the threshold, then
//! riding the edge, then stopping when its parent runs out from under it.
//!
//! Sticky is the one `position` the layout cannot answer, because it is a
//! question about the scroll offset and the layout does not know one. It is
//! resolved at paint time instead, which is also why nothing else moves: the box
//! keeps its original space the whole time, so its siblings never reflow as it
//! travels. That is CSS's behaviour and the reason sticky is usable at all.

use rux_layout::*;

fn red() -> Option<Background> {
    Some(Background::Color(Rgba::new(1.0, 0.0, 0.0, 1.0)))
}

fn row(h: f32) -> Node {
    Node::new(Style { height: Some(Len::Px(h)), width: Some(Len::Px(300.0)), ..Default::default() })
}

/// A 200px-tall scroller holding two 400px sections, each with a sticky heading
/// at its top. Returns the painted `(y, height)` of every red box, in order, at
/// the given scroll offset.
fn headings_at(offset: f32) -> Vec<(f32, f32)> {
    let section = |body: f32| {
        let mut sec = Node::new(Style {
            display: Display::Flex,
            axis: Axis::Column,
            width: Some(Len::Px(300.0)),
            ..Default::default()
        });
        let mut head = Node::new(Style {
            position: Position::Sticky,
            inset: [Some(Len::Px(0.0)), None, None, None],
            height: Some(Len::Px(40.0)),
            width: Some(Len::Px(300.0)),
            background: red(),
            ..Default::default()
        });
        head.style.position = Position::Sticky;
        sec.children.push(head);
        sec.children.push(row(body));
        sec
    };

    let mut scroller = Node::new(Style {
        display: Display::Flex,
        axis: Axis::Column,
        overflow: Overflow::Scroll,
        width: Some(Len::Px(300.0)),
        height: Some(Len::Px(200.0)),
        ..Default::default()
    });
    scroller.children.push(section(360.0));
    scroller.children.push(section(360.0));

    let mut screen =
        Node::new(Style { display: Display::Flex, axis: Axis::Column, ..Default::default() });
    screen.children.push(scroller);

    let mut measure = |_: &TextContent, _: Option<f32>| (0.0, 0.0);
    layout_scrolled(&screen, 600.0, 400.0, &[Offset { x: 0.0, y: offset }], &mut measure)
        .paints
        .into_iter()
        .filter_map(|p| match p {
            Paint::Rect(r)
                if matches!(r.background, Some(Background::Color(c)) if c.r > 0.5) =>
            {
                Some((r.y, r.height))
            }
            _ => None,
        })
        .collect()
}

/// Unscrolled, a sticky box is exactly where the flow put it. It has not stuck
/// to anything yet, and that is the state it spends most of its life in.
#[test]
fn unscrolled_it_sits_where_it_was_laid_out() {
    let heads = headings_at(0.0);
    assert_eq!(heads.len(), 2, "both headings painted");
    assert_eq!(heads[0].0, 0.0, "the first is at the top of the scroller");
    assert_eq!(heads[1].0, 400.0, "the second is where its section starts");
}

/// Scrolled into its section, the heading rides the top edge of the scroller
/// rather than leaving with the content.
#[test]
fn it_rides_the_edge_once_the_scroll_passes_it() {
    let heads = headings_at(120.0);
    assert_eq!(heads[0].0, 0.0, "pinned to the scroller's top, not at -120");
    assert_eq!(heads[1].0, 280.0, "the one below has simply scrolled up");
}

/// **The clamp is the half that makes a list of sections work.** A heading stops
/// at the end of its own section rather than sitting over the next one's rows,
/// so the arriving section pushes it off the top.
///
/// Without this a sticky heading pins to the edge and stays there for the rest
/// of the document, which looks like it is working right up until the second
/// section arrives.
#[test]
fn it_stops_when_its_section_runs_out() {
    // The first section is 400 tall, so its heading may travel to y = 360 within
    // it. At a 380 scroll the section's bottom is at 400 - 380 = 20, so the
    // heading can only reach 20 - 40 = -20.
    let heads = headings_at(380.0);
    assert_eq!(
        heads[0].0, -20.0,
        "pushed off the top by its own section ending, not still pinned at 0"
    );
    assert_eq!(heads[1].0, 20.0, "and the next heading is arriving at the edge");
}

/// A sticky box occupies its original space the whole time, so nothing else
/// moves as it travels. Its sibling is where it always was.
#[test]
fn nothing_else_moves_while_it_travels() {
    let body_top = |offset: f32| {
        let mut scroller = Node::new(Style {
            display: Display::Flex,
            axis: Axis::Column,
            overflow: Overflow::Scroll,
            width: Some(Len::Px(300.0)),
            height: Some(Len::Px(200.0)),
            ..Default::default()
        });
        // Children of a flex column shrink unless told not to, and a shrunk
        // heading would make this test about flex rather than about sticky.
        let mut head = Node::new(Style {
            position: Position::Sticky,
            inset: [Some(Len::Px(0.0)), None, None, None],
            height: Some(Len::Px(40.0)),
            width: Some(Len::Px(300.0)),
            shrink: 0.0,
            ..Default::default()
        });
        head.style.position = Position::Sticky;
        let mut body = row(600.0);
        body.style.shrink = 0.0;
        body.style.background = red();
        scroller.children.push(head);
        scroller.children.push(body);

        let mut screen =
            Node::new(Style { display: Display::Flex, axis: Axis::Column, ..Default::default() });
        screen.children.push(scroller);
        let mut measure = |_: &TextContent, _: Option<f32>| (0.0, 0.0);
        layout_scrolled(&screen, 600.0, 400.0, &[Offset { x: 0.0, y: offset }], &mut measure)
            .paints
            .into_iter()
            .find_map(|p| match p {
                Paint::Rect(r)
                    if matches!(r.background, Some(Background::Color(c)) if c.r > 0.5) =>
                {
                    Some(r.y)
                }
                _ => None,
            })
            .expect("the body painted")
    };
    assert_eq!(body_top(0.0), 40.0, "below the heading, as laid out");
    assert_eq!(body_top(100.0), -60.0, "and it has simply scrolled, by exactly 100");
}

/// A sticky box with no scroller above it has the window to stick to, which is
/// what it means in an unscrolled document.
#[test]
fn with_no_scroller_it_sticks_to_the_window() {
    let mut screen =
        Node::new(Style { display: Display::Flex, axis: Axis::Column, ..Default::default() });
    let mut head = Node::new(Style {
        position: Position::Sticky,
        inset: [Some(Len::Px(10.0)), None, None, None],
        height: Some(Len::Px(40.0)),
        width: Some(Len::Px(300.0)),
        background: red(),
        ..Default::default()
    });
    head.style.position = Position::Sticky;
    screen.children.push(head);

    let mut measure = |_: &TextContent, _: Option<f32>| (0.0, 0.0);
    let y = layout(&screen, 600.0, 400.0, &mut measure)
        .paints
        .into_iter()
        .find_map(|p| match p {
            Paint::Rect(r) if matches!(r.background, Some(Background::Color(c)) if c.r > 0.5) => {
                Some(r.y)
            }
            _ => None,
        })
        .expect("painted");
    assert_eq!(y, 10.0, "held at its threshold below the window's top");
}

/// **Two sticky headings do not interact, and the hand-over is emergent.**
///
/// It looks as though an arriving heading shoves the one at the top out of the
/// way. Nothing of the sort happens: neither box can see the other. Each is
/// clamped to its own section, and section one's bottom edge is exactly where
/// section two's heading begins, so "clamped to the bottom of my section" and
/// "pushed by the next heading" describe the same pixel.
///
/// The consequence is the one worth knowing, and it is the next test.
#[test]
fn the_hand_over_is_each_heading_clamped_to_its_own_section() {
    let heads = headings_at(380.0);
    // Section one ends at 400 - 380 = 20, so its heading is clamped to -20.
    // Section two's heading is simply in flow, arriving at 20.
    assert_eq!(heads[0].0, -20.0, "clamped to the end of its own section");
    assert_eq!(heads[1].0, 20.0, "still in flow, not yet stuck to anything");
    assert_eq!(
        heads[1].0 - heads[0].0,
        heads[0].1,
        "exactly one heading apart, which is what reads as a shove"
    );
}

/// And the corollary: **flat siblings do not hand over, they pile up.**
///
/// Headings written as siblings of the rows, with no section box around each
/// group, are all clamped to the scroller itself, so every one of them pins at
/// the same edge and they sit on top of each other. The wrapper per section is
/// not tidiness, it is the thing that makes the effect work at all.
#[test]
fn without_a_box_each_they_pile_up_at_the_same_edge() {
    let mut scroller = Node::new(Style {
        display: Display::Flex,
        axis: Axis::Column,
        overflow: Overflow::Scroll,
        width: Some(Len::Px(300.0)),
        height: Some(Len::Px(200.0)),
        ..Default::default()
    });
    for _ in 0..2 {
        let mut head = Node::new(Style {
            position: Position::Sticky,
            inset: [Some(Len::Px(0.0)), None, None, None],
            height: Some(Len::Px(40.0)),
            width: Some(Len::Px(300.0)),
            shrink: 0.0,
            background: red(),
            ..Default::default()
        });
        head.style.position = Position::Sticky;
        scroller.children.push(head);
        let mut body = row(360.0);
        body.style.shrink = 0.0;
        scroller.children.push(body);
    }

    let mut screen =
        Node::new(Style { display: Display::Flex, axis: Axis::Column, ..Default::default() });
    screen.children.push(scroller);

    let mut measure = |_: &TextContent, _: Option<f32>| (0.0, 0.0);
    // Far enough down that both headings have reached the edge: the first
    // passed it long ago, the second has just arrived.
    let heads: Vec<f32> =
        layout_scrolled(&screen, 600.0, 400.0, &[Offset { x: 0.0, y: 500.0 }], &mut measure)
            .paints
            .into_iter()
            .filter_map(|p| match p {
                Paint::Rect(r)
                    if matches!(r.background, Some(Background::Color(c)) if c.r > 0.5) =>
                {
                    Some(r.y)
                }
                _ => None,
            })
            .collect();
    assert_eq!(heads[0], 0.0, "the first pins to the scroller and stays");
    assert_eq!(heads[1], 0.0, "and so does the second, on top of it");
}
