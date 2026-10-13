//! `z-index` and stacking contexts: which box is drawn over which.
//!
//! CSS paints a stacking context's own box, then its negative layers, then its
//! in-flow content, then its positioned boxes with `z-index: auto` or `0` in
//! document order, then the positive layers from lowest up. These check that
//! order, that a held-back box is still drawn inside the clips it sits in, and
//! that what reads the document rather than the picture (Tab, scroller ids)
//! does not move when the picture does.

use rux_layout::*;

/// A box labelled by the red channel of its background, which is how the
/// tests below find it in the paint list.
fn tagged(label: f32, style: Style) -> Node {
    Node::new(Style {
        width: Some(Len::Px(100.0)),
        height: Some(Len::Px(100.0)),
        background: Some(Background::Color(Rgba::new(label, 0.0, 0.0, 1.0))),
        ..style
    })
}

fn at(position: Position, z: Option<i32>) -> Style {
    Style {
        position,
        z_index: z,
        inset: if position.is_out_of_flow() {
            [Some(Len::Px(0.0)), None, None, Some(Len::Px(0.0))]
        } else {
            [None; 4]
        },
        ..Default::default()
    }
}

fn screen() -> Node {
    Node::new(Style::default())
}

fn lay(root: &Node) -> Layout {
    let mut measure = |_: &TextContent, _: Option<f32>| (50.0, 20.0);
    layout(root, 800.0, 600.0, &mut measure)
}

/// The labels of every labelled box, in the order they are drawn.
fn drawn(root: &Node) -> Vec<f32> {
    lay(root)
        .paints
        .into_iter()
        .filter_map(|p| match p {
            Paint::Rect(PaintRect { background: Some(Background::Color(c)), .. }) => Some(c.r),
            _ => None,
        })
        .filter(|r| *r > 0.0)
        .collect()
}

#[test]
fn a_higher_z_index_is_drawn_over_a_later_sibling() {
    let mut root = screen();
    root.children.push(tagged(0.1, at(Position::Absolute, Some(2))));
    root.children.push(tagged(0.2, at(Position::Absolute, Some(1))));
    assert_eq!(drawn(&root), vec![0.2, 0.1]);
}

#[test]
fn positioned_siblings_paint_in_tree_order() {
    let mut root = screen();
    root.children.push(tagged(0.1, at(Position::Absolute, None)));
    root.children.push(tagged(0.2, at(Position::Relative, None)));
    assert_eq!(drawn(&root), vec![0.1, 0.2], "no z-index: the later one is on top");
}

#[test]
fn equal_z_indexes_keep_tree_order() {
    let mut root = screen();
    root.children.push(tagged(0.1, at(Position::Absolute, Some(3))));
    root.children.push(tagged(0.2, at(Position::Absolute, Some(3))));
    root.children.push(tagged(0.3, at(Position::Absolute, Some(0))));
    assert_eq!(drawn(&root), vec![0.3, 0.1, 0.2]);
}

/// A positioned box is drawn over every in-flow box of its stacking context,
/// not only over its own siblings: CSS sorts the whole context.
#[test]
fn a_positioned_box_is_drawn_over_later_in_flow_content_anywhere_in_its_context() {
    let mut root = screen();
    let mut wrapper = Node::new(Style::default());
    wrapper.children.push(tagged(0.1, at(Position::Relative, None)));
    root.children.push(wrapper);
    root.children.push(tagged(0.2, Style::default()));
    assert_eq!(drawn(&root), vec![0.2, 0.1]);
}

/// The classic: `z-index: -1` inside a positioned box with no `z-index` of its
/// own goes under that box, because the box is not a stacking context and the
/// layer belongs to the one outside it. It still sits over the root's own box.
#[test]
fn a_negative_layer_goes_under_the_in_flow_content_of_its_context() {
    let mut root = Node::new(Style {
        background: Some(Background::Color(Rgba::new(0.9, 0.0, 0.0, 1.0))),
        ..Default::default()
    });
    let mut card = tagged(0.2, at(Position::Relative, None));
    card.children.push(tagged(0.1, at(Position::Absolute, Some(-1))));
    root.children.push(card);
    root.children.push(tagged(0.3, Style::default()));
    assert_eq!(drawn(&root), vec![0.9, 0.1, 0.3, 0.2]);
}

/// Give the card a `z-index` and it becomes the context: the negative layer is
/// drawn over the card's own box and under the card's in-flow content.
#[test]
fn a_negative_layer_stays_inside_a_context_that_has_one() {
    let mut root = screen();
    let mut card = tagged(0.2, at(Position::Relative, Some(0)));
    card.children.push(tagged(0.3, Style::default()));
    card.children.push(tagged(0.1, at(Position::Absolute, Some(-1))));
    root.children.push(card);
    assert_eq!(drawn(&root), vec![0.2, 0.1, 0.3]);
}

#[test]
fn negative_layers_sort_among_themselves() {
    let mut root = screen();
    root.children.push(tagged(0.5, Style::default()));
    root.children.push(tagged(0.1, at(Position::Absolute, Some(-1))));
    root.children.push(tagged(0.2, at(Position::Absolute, Some(-3))));
    let mut holder = tagged(0.4, at(Position::Relative, None));
    holder.children.push(tagged(0.3, at(Position::Absolute, Some(-2))));
    root.children.push(holder);
    assert_eq!(drawn(&root), vec![0.2, 0.3, 0.1, 0.5, 0.4]);
}

/// A stacking context is drawn whole: a huge `z-index` inside it cannot climb
/// over a sibling of the context.
#[test]
fn a_z_index_does_not_escape_its_stacking_context() {
    let mut root = screen();
    let mut low = tagged(0.1, at(Position::Relative, Some(1)));
    low.children.push(tagged(0.2, at(Position::Absolute, Some(999))));
    root.children.push(low);
    root.children.push(tagged(0.3, at(Position::Relative, Some(2))));
    assert_eq!(drawn(&root), vec![0.1, 0.2, 0.3]);
}

#[test]
fn opacity_and_transform_make_contexts_too() {
    for style in [
        Style { opacity: 0.5, ..Default::default() },
        Style { transform: Some([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]), ..Default::default() },
        Style { isolate: true, ..Default::default() },
    ] {
        let mut root = screen();
        let mut group = Node::new(Style { position: Position::Relative, z_index: Some(1), ..style });
        group.children.push(tagged(0.1, at(Position::Absolute, Some(999))));
        root.children.push(group);
        root.children.push(tagged(0.2, at(Position::Relative, Some(2))));
        assert_eq!(drawn(&root), vec![0.1, 0.2], "the 999 is held inside its group");
    }
}

/// CSS's rule: `z-index` counts on a flex or grid item whatever its position,
/// and on a static block box it does nothing.
#[test]
fn z_index_counts_on_a_flex_item_and_not_on_a_static_block() {
    let row = |display| {
        let mut root = Node::new(Style { display, ..Default::default() });
        root.children.push(tagged(0.1, Style { z_index: Some(1), margin: Sides::default(), ..Default::default() }));
        root.children.push(tagged(0.2, at(Position::Relative, None)));
        drawn(&root)
    };
    assert_eq!(row(Display::Flex), vec![0.2, 0.1], "a flex item with z-index 1 is over a z-index 0");
    assert_eq!(row(Display::Block), vec![0.1, 0.2], "ignored on a block: in flow, under the positioned one");
}

/// A `fixed` box is a stacking context in tree order like any other: one with
/// no `z-index` is covered by a positioned box after it, which is what an
/// author's `z-index: 1` on a header is for.
#[test]
fn a_fixed_box_needs_a_z_index_to_stay_over_later_positioned_boxes() {
    let page = |z| {
        let mut root = screen();
        root.children.push(tagged(0.1, at(Position::Fixed, z)));
        root.children.push(tagged(0.2, at(Position::Relative, None)));
        drawn(&root)
    };
    assert_eq!(page(None), vec![0.1, 0.2]);
    assert_eq!(page(Some(1)), vec![0.2, 0.1]);
}

/// A held-back box is drawn after the walk has left the box that clips it, so
/// the clip has to be put back around it, or a positioned row in a scroller
/// would be drawn over everything outside the scroller.
#[test]
fn a_held_back_box_is_still_drawn_inside_its_clips() {
    let mut root = screen();
    let mut panel = Node::new(Style {
        overflow: Overflow::Clip,
        width: Some(Len::Px(50.0)),
        height: Some(Len::Px(50.0)),
        ..Default::default()
    });
    panel.children.push(tagged(0.1, at(Position::Relative, None)));
    root.children.push(panel);
    root.children.push(tagged(0.2, Style::default()));
    let paints = lay(&root).paints;
    let row = paints
        .iter()
        .position(|p| matches!(p, Paint::Rect(PaintRect { background: Some(Background::Color(c)), .. }) if c.r == 0.1))
        .expect("drawn");
    let after = paints
        .iter()
        .position(|p| matches!(p, Paint::Rect(PaintRect { background: Some(Background::Color(c)), .. }) if c.r == 0.2))
        .expect("drawn");
    assert!(after < row, "the positioned row is drawn last");
    let open = paints[..row].iter().filter(|p| matches!(p, Paint::PushClip { .. })).count();
    let shut = paints[..row].iter().filter(|p| matches!(p, Paint::PopClip)).count();
    assert_eq!(open - shut, 1, "inside the panel's clip: {paints:?}");
    match paints[..row].iter().rev().find(|p| matches!(p, Paint::PushClip { .. })) {
        Some(Paint::PushClip { width, height, .. }) => assert_eq!((*width, *height), (50.0, 50.0)),
        _ => unreachable!(),
    }
}

/// A tap finds the topmost box, which is the one drawn last.
#[test]
fn the_box_drawn_on_top_is_the_one_a_tap_finds() {
    let mut root = screen();
    let mut over = tagged(0.1, at(Position::Absolute, Some(1)));
    over.on_tap = Some("over".into());
    let mut under = tagged(0.2, at(Position::Absolute, None));
    under.on_tap = Some("under".into());
    root.children.push(over);
    root.children.push(under);
    let hits = lay(&root).hits;
    assert_eq!(hits.last().and_then(|h| h.on_tap.as_deref()), Some("over"));
}

/// Tab follows the document, not the picture.
#[test]
fn tab_order_stays_in_document_order() {
    let mut root = screen();
    for (i, style) in [at(Position::Relative, Some(5)), Style::default(), at(Position::Relative, Some(-1))]
        .into_iter()
        .enumerate()
    {
        let mut b = tagged(0.1 * (i + 1) as f32, style);
        b.on_tap = Some(format!("b{i}"));
        root.children.push(b);
    }
    let order: Vec<String> = lay(&root)
        .focusables
        .into_iter()
        .map(|f| match f.kind {
            FocusKind::Activate { on_tap, .. } => on_tap,
            _ => unreachable!(),
        })
        .collect();
    assert_eq!(order, vec!["b0", "b1", "b2"]);
}

/// A scroller keeps its id when the box around it starts or stops being a
/// stacking context, which a fading page does twice per route change. The
/// shell keeps offsets by id, so a changing id is a jump.
#[test]
fn a_scroller_keeps_its_id_while_its_page_fades() {
    let scroller = |label: f32| {
        let mut s = tagged(label, Style { overflow: Overflow::Scroll, ..Default::default() });
        s.children.push(Node::new(Style { height: Some(Len::Px(400.0)), ..Default::default() }));
        s
    };
    let ids = |opacity: f32| {
        let mut root = screen();
        let mut page = Node::new(Style { opacity, ..Default::default() });
        page.children.push(scroller(0.1));
        root.children.push(page);
        root.children.push(scroller(0.2));
        lay(&root).scrolls.iter().map(|s| (s.id, s.y)).collect::<Vec<_>>()
    };
    assert_eq!(ids(0.5), ids(1.0));
}
