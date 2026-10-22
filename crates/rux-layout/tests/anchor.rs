//! CSS anchor positioning: `anchor-name`, `position-anchor`, `anchor()` in
//! the insets and `anchor-size()` in `width` and `height`.
//!
//! An anchored box is placed at paint time against where its anchor was
//! drawn, so these check what was painted, after scrolling, rather than what
//! the layout said.

use rux_layout::*;

fn colour(r: f32) -> Option<Background> {
    Some(Background::Color(Rgba::new(r, 0.0, 0.0, 1.0)))
}

/// A 40 x 20 button named `--btn` at (100, 50) of a plain screen.
fn button() -> Node {
    Node::new(Style {
        position: Position::Absolute,
        inset: [Some(Len::Px(50.0)), None, None, Some(Len::Px(100.0))],
        width: Some(Len::Px(40.0)),
        height: Some(Len::Px(20.0)),
        anchor: Some(Box::new(AnchorStyle { name: Some("--btn".into()), ..Default::default() })),
        background: colour(0.1),
        ..Default::default()
    })
}

fn at(name: Option<&str>, side: AnchorSide) -> Option<AnchorRef> {
    Some(AnchorRef { name: name.map(String::from), side, fallback: None })
}

/// A fixed 60 x 30 menu with the given insets, anchored to `--btn` by default.
fn menu(inset: [Option<AnchorRef>; 4]) -> Node {
    Node::new(Style {
        position: Position::Fixed,
        anchor: Some(Box::new(AnchorStyle {
            position_anchor: Some("--btn".into()),
            inset,
            ..Default::default()
        })),
        width: Some(Len::Px(60.0)),
        height: Some(Len::Px(30.0)),
        background: colour(0.2),
        ..Default::default()
    })
}

fn lay(root: &Node, offsets: &[Offset]) -> Layout {
    let mut measure = |_: &TextContent, _: Option<f32>| (10.0, 10.0);
    layout_scrolled(root, 800.0, 600.0, offsets, &mut measure)
}

/// The painted `(x, y, width, height)` of the box labelled `r`, if drawn.
fn rect(out: &Layout, r: f32) -> Option<(f32, f32, f32, f32)> {
    out.paints.iter().find_map(|p| match p {
        Paint::Rect(PaintRect { background: Some(Background::Color(c)), x, y, width, height, .. }) if c.r == r => {
            Some((*x, *y, *width, *height))
        }
        _ => None,
    })
}

#[test]
fn a_menu_sits_under_its_button() {
    let mut root = Node::new(Style::default());
    root.children.push(button());
    root.children.push(menu([at(None, AnchorSide::Bottom), None, None, at(None, AnchorSide::Left)]));
    let out = lay(&root, &[]);
    assert_eq!(rect(&out, 0.2), Some((100.0, 70.0, 60.0, 30.0)));
    assert!(out.anchor_misses.is_empty());
}

#[test]
fn the_far_insets_measure_from_the_box_s_own_far_edge() {
    let mut root = Node::new(Style::default());
    root.children.push(button());
    // Above it and ending at its right edge.
    root.children.push(menu([None, at(None, AnchorSide::Right), at(None, AnchorSide::Top), None]));
    assert_eq!(rect(&lay(&root, &[]), 0.2), Some((80.0, 20.0, 60.0, 30.0)));
}

#[test]
fn center_and_percentages_and_explicit_names() {
    let mut root = Node::new(Style::default());
    root.children.push(button());
    let mut other = button();
    other.style.anchor.as_mut().unwrap().name = Some("--other".into());
    other.style.inset = [Some(Len::Px(300.0)), None, None, Some(Len::Px(300.0))];
    other.style.background = colour(0.3);
    root.children.push(other);
    root.children.push(menu([
        at(Some("--other"), AnchorSide::Frac(0.5)),
        None,
        None,
        at(None, AnchorSide::Center),
    ]));
    assert_eq!(rect(&lay(&root, &[]), 0.2), Some((120.0, 310.0, 60.0, 30.0)));
}

/// The point of anchoring rather than measuring in a handler: the button
/// scrolls, and the menu goes with it, though it is `fixed` and so outside
/// the scroller's clip.
#[test]
fn a_menu_follows_its_button_when_the_panel_scrolls() {
    let mut root = Node::new(Style::default());
    let mut panel = Node::new(Style {
        overflow: Overflow::Scroll,
        width: Some(Len::Px(300.0)),
        height: Some(Len::Px(200.0)),
        display: Display::Flex,
        axis: Axis::Column,
        ..Default::default()
    });
    panel.children.push(Node::new(Style { height: Some(Len::Px(100.0)), shrink: 0.0, ..Default::default() }));
    let mut b = button();
    b.style.position = Position::Static;
    b.style.inset = [None; 4];
    b.style.shrink = 0.0;
    panel.children.push(b);
    panel.children.push(Node::new(Style { height: Some(Len::Px(600.0)), shrink: 0.0, ..Default::default() }));
    root.children.push(panel);
    root.children.push(menu([at(None, AnchorSide::Bottom), None, None, at(None, AnchorSide::Left)]));

    let still = lay(&root, &[]);
    assert_eq!(rect(&still, 0.2).map(|r| r.1), Some(120.0));
    let scrolled = lay(&root, &[Offset { x: 0.0, y: 60.0 }]);
    assert_eq!(rect(&scrolled, 0.1).map(|r| r.1), Some(40.0), "the button moved up");
    assert_eq!(rect(&scrolled, 0.2).map(|r| r.1), Some(60.0), "and the menu with it");
    // Scrolled until the button is out of the panel entirely: the menu is
    // not drawn, rather than floating on with nothing to belong to.
    let gone = lay(&root, &[Offset { x: 0.0, y: 400.0 }]);
    assert_eq!(rect(&gone, 0.2), None);
    assert!(!gone.hits.iter().any(|h| (h.width, h.height) == (60.0, 30.0)), "nor tapped");
}

#[test]
fn a_missing_anchor_uses_the_fallback_or_says_so() {
    let mut root = Node::new(Style::default());
    let mut m = menu([
        Some(AnchorRef { name: Some("--nothing".into()), side: AnchorSide::Bottom, fallback: Some(Len::Px(12.0)) }),
        None,
        None,
        Some(AnchorRef { name: Some("--nothing".into()), side: AnchorSide::Left, fallback: None }),
    ]);
    m.style.background = colour(0.2);
    root.children.push(m);
    let out = lay(&root, &[]);
    assert_eq!(rect(&out, 0.2).map(|r| r.1), Some(12.0), "the fallback, against the window");
    assert_eq!(out.anchor_misses, vec!["--nothing".to_string()]);
}

#[test]
fn a_hidden_anchor_is_no_anchor() {
    let mut root = Node::new(Style::default());
    let mut b = button();
    b.hidden = true;
    root.children.push(b);
    root.children.push(menu([at(None, AnchorSide::Bottom), None, None, at(None, AnchorSide::Left)]));
    assert_eq!(lay(&root, &[]).anchor_misses, vec!["--btn".to_string()]);
}

/// Two boxes with one name: the later one is the anchor, as in CSS.
#[test]
fn the_last_box_with_a_name_is_its_anchor() {
    let mut root = Node::new(Style::default());
    root.children.push(button());
    let mut later = button();
    later.style.inset = [Some(Len::Px(400.0)), None, None, Some(Len::Px(10.0))];
    later.style.background = colour(0.3);
    root.children.push(later);
    root.children.push(menu([at(None, AnchorSide::Bottom), None, None, at(None, AnchorSide::Left)]));
    assert_eq!(rect(&lay(&root, &[]), 0.2).map(|r| (r.0, r.1)), Some((10.0, 420.0)));
}

#[test]
fn anchor_size_and_both_insets_size_the_box() {
    let mut root = Node::new(Style::default());
    root.children.push(button());
    let mut m = menu([at(None, AnchorSide::Bottom), None, None, at(None, AnchorSide::Left)]);
    m.style.width = None;
    m.style.anchor.as_mut().unwrap().size = [Some(AnchorSize { name: None, height: false, fallback: None }), None];
    root.children.push(m);
    assert_eq!(rect(&lay(&root, &[]), 0.2).map(|r| r.2), Some(40.0), "as wide as the button");

    let mut root = Node::new(Style::default());
    root.children.push(button());
    let mut m = menu([at(None, AnchorSide::Bottom), at(None, AnchorSide::Right), None, at(None, AnchorSide::Center)]);
    m.style.width = None;
    root.children.push(m);
    assert_eq!(rect(&lay(&root, &[]), 0.2), Some((120.0, 70.0, 20.0, 30.0)), "from its middle to its right edge");
}

/// Escaping a clip is what `fixed` is for: the menu is drawn whole although
/// the button is inside a small `overflow: clip` panel.
#[test]
fn a_fixed_menu_is_not_cut_by_the_button_s_panel() {
    let mut root = Node::new(Style::default());
    let mut panel = Node::new(Style {
        overflow: Overflow::Clip,
        width: Some(Len::Px(50.0)),
        height: Some(Len::Px(25.0)),
        ..Default::default()
    });
    let mut b = button();
    b.style.position = Position::Static;
    b.style.inset = [None; 4];
    panel.children.push(b);
    panel.children.push(menu([at(None, AnchorSide::Bottom), None, None, at(None, AnchorSide::Left)]));
    root.children.push(panel);
    let paints = lay(&root, &[]).paints;
    let i = paints
        .iter()
        .position(|p| matches!(p, Paint::Rect(PaintRect { background: Some(Background::Color(c)), .. }) if c.r == 0.2))
        .expect("drawn");
    let open = paints[..i].iter().filter(|p| matches!(p, Paint::PushClip { .. })).count();
    let shut = paints[..i].iter().filter(|p| matches!(p, Paint::PopClip)).count();
    assert_eq!(open, shut, "outside every clip: {paints:?}");
}

/// The tap finds the menu where it is drawn, not where the layout put it.
#[test]
fn a_tap_finds_the_menu_where_it_is_drawn() {
    let mut root = Node::new(Style::default());
    root.children.push(button());
    let mut m = menu([at(None, AnchorSide::Bottom), None, None, at(None, AnchorSide::Left)]);
    m.on_tap = Some("pick".into());
    root.children.push(m);
    let hit = lay(&root, &[]).hits.into_iter().find(|h| h.on_tap.as_deref() == Some("pick")).expect("tappable");
    assert_eq!((hit.x, hit.y), (100.0, 70.0));
}

// ── position-try ─────────────────────────────────────────────────────────────

/// A button at `top`, with a 60 x 30 menu under it that flips above it when
/// there is no room below, in an 800 x 600 window.
fn flipping(top: f32, tries: Vec<PositionTry>) -> Node {
    let mut root = Node::new(Style::default());
    let mut b = button();
    b.style.inset[0] = Some(Len::Px(top));
    root.children.push(b);
    let mut m = menu([at(None, AnchorSide::Bottom), None, None, at(None, AnchorSide::Left)]);
    m.style.anchor.as_mut().unwrap().tries = tries;
    root.children.push(m);
    root
}

fn flip_block() -> PositionTry {
    PositionTry { flip_block: true, ..Default::default() }
}

/// Was dropped before position-try: the anchored axis ignored its margin.
#[test]
fn a_margin_moves_an_anchored_box_off_its_anchor() {
    let mut root = flipping(50.0, Vec::new());
    root.children[1].style.margin.top = 4.0;
    assert_eq!(rect(&lay(&root, &[]), 0.2).map(|r| r.1), Some(74.0));
}

#[test]
fn flip_block_goes_above_when_below_has_no_room() {
    // Room below: its own place.
    let out = lay(&flipping(50.0, vec![flip_block()]), &[]);
    assert_eq!(rect(&out, 0.2).map(|r| r.1), Some(70.0));
    assert!(out.position_tries.is_empty());
    // At 560 the button ends at 580, and 30 more does not fit in 600.
    let out = lay(&flipping(560.0, vec![flip_block()]), &[]);
    assert_eq!(rect(&out, 0.2).map(|r| r.1), Some(530.0), "its bottom on the button's top");
    assert_eq!(out.position_tries.len(), 1);
}

#[test]
fn a_flip_carries_the_margin_to_the_other_side() {
    let mut root = flipping(560.0, vec![flip_block()]);
    root.children[1].style.margin.top = 4.0;
    assert_eq!(rect(&lay(&root, &[]), 0.2).map(|r| r.1), Some(526.0));
}

#[test]
fn flip_inline_and_flip_start() {
    // Button at the right edge: a menu starting at its left overflows.
    let mut root = flipping(50.0, vec![PositionTry { flip_inline: true, ..Default::default() }]);
    root.children[0].style.inset[3] = Some(Len::Px(760.0));
    let r = rect(&lay(&root, &[]), 0.2).unwrap();
    assert_eq!((r.0, r.1), (740.0, 70.0), "ends at the button's right edge");

    // flip-start: top with left, so `top: anchor(bottom)` becomes
    // `left: anchor(right)`, beside the button rather than under it.
    let root = flipping(560.0, vec![PositionTry { flip_start: true, ..Default::default() }]);
    let r = rect(&lay(&root, &[]), 0.2).unwrap();
    assert_eq!((r.0, r.1), (140.0, 560.0));
}

#[test]
fn a_named_rule_replaces_the_sides_it_names() {
    // `@position-try --above { top: auto; bottom: anchor(top) }`
    let above = PositionTry {
        inset: [Some(TryInset::Auto), None, Some(TryInset::Anchor(AnchorRef { name: None, side: AnchorSide::Top, fallback: None })), None],
        margin: [None, None, Some(2.0), None],
        ..Default::default()
    };
    let r = rect(&lay(&flipping(560.0, vec![above]), &[]), 0.2).unwrap();
    assert_eq!((r.0, r.1), (100.0, 528.0), "left is still its own");
}

#[test]
fn nothing_fits_keeps_its_own_place() {
    // A 700-high menu fits neither below nor above.
    let mut root = flipping(300.0, vec![flip_block()]);
    root.children[1].style.height = Some(Len::Px(700.0));
    let out = lay(&root, &[]);
    assert_eq!(rect(&out, 0.2).map(|r| r.1), Some(320.0));
    assert!(out.position_tries.is_empty());
}

#[test]
fn most_height_picks_the_roomier_side() {
    // Both fit, but there is more room above a button at 400.
    let mut root = flipping(400.0, vec![flip_block()]);
    root.children[1].style.anchor.as_mut().unwrap().try_order = TryOrder::MostHeight;
    assert_eq!(rect(&lay(&root, &[]), 0.2).map(|r| r.1), Some(370.0));
}

/// CSS keeps the place that last fitted while it still does, so a menu that
/// flipped stays flipped as its button moves back into room below.
#[test]
fn the_place_that_last_fitted_is_tried_first() {
    let mut state = LayoutState::new();
    let mut measure = |_: &TextContent, _: Option<f32>| (10.0, 10.0);
    let out = state.layout_scrolled(&flipping(560.0, vec![flip_block()]), 800.0, 600.0, &[], &mut measure);
    assert_eq!(rect(&out, 0.2).map(|r| r.1), Some(530.0));
    let out = state.layout_scrolled(&flipping(300.0, vec![flip_block()]), 800.0, 600.0, &[], &mut measure);
    assert_eq!(rect(&out, 0.2).map(|r| r.1), Some(270.0), "still above, which still fits");
    let out = state.layout_scrolled(&flipping(20.0, vec![flip_block()]), 800.0, 600.0, &[], &mut measure);
    assert_eq!(rect(&out, 0.2).map(|r| r.1), Some(40.0), "no room above: back under");

    // Anchored to another box, it starts again from its own place.
    let out = state.layout_scrolled(&flipping(560.0, vec![flip_block()]), 800.0, 600.0, &[], &mut measure);
    assert_eq!(rect(&out, 0.2).map(|r| r.1), Some(530.0));
    let mut moved = flipping(300.0, vec![flip_block()]);
    moved.children[0].style.anchor.as_mut().unwrap().name = Some("--other".into());
    moved.children[1].style.anchor.as_mut().unwrap().position_anchor = Some("--other".into());
    let out = state.layout_scrolled(&moved, 800.0, 600.0, &[], &mut measure);
    assert_eq!(rect(&out, 0.2).map(|r| r.1), Some(320.0), "under its new anchor");
}
