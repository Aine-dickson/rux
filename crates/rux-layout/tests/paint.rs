use rux_layout::*;

fn on_screen(node: Node) -> Node {
    let mut screen = Node::new(Style {
        display: Display::Flex,
        axis: Axis::Column,
        ..Default::default()
    });
    screen.children.push(node);
    screen
}

fn paints(root: Node) -> Vec<Paint> {
    let mut measure = |_: &rux_layout::TextContent, _: Option<f32>| (50.0, 20.0);
    layout(&on_screen(root), 1000.0, 800.0, &mut measure).paints
}

/// Plain white 16px text, for tests that care about geometry rather than type.
fn label(text: &str) -> TextContent {
    TextContent {
        text: text.into(),
        font_size: 16.0,
        weight: 400,
        color: Rgba::new(1.0, 1.0, 1.0, 1.0),
        align: TextAlign::Start,
        wrap: TextWrap::Normal,
        font_family: None,
        letter_spacing: None,
        word_spacing: None,
        line_height: None,
        italic: false,
        underline: false,
        strikethrough: false,
        nowrap: false,
        caret: None,
        selection: None,
        preedit: None,
        selection_style: Default::default(),
        rich: None,
        white_space: Default::default(),
        direction: None,
        isolate: Default::default(),
    }
}

/// A text node is a box too, its background and border paint under the glyphs.
/// (Only container boxes used to paint, so a styled <text> came out bare.)
#[test]
fn text_node_paints_its_background_then_its_glyphs() {
    let node = Node::text(
        Style {
            background: Some(Background::Color(Rgba::new(0.2, 0.2, 0.2, 1.0))),
            radius: [6.0; 4],
            ..Default::default()
        },
        TextContent {
            text: "hi".into(),
            font_size: 16.0,
            weight: 400,
            color: Rgba::new(1.0, 1.0, 1.0, 1.0),
            align: TextAlign::Start,
            wrap: TextWrap::Normal,
            font_family: None,
            letter_spacing: None,
            word_spacing: None,
            line_height: None,
            italic: false,
            underline: false,
            strikethrough: false,
            nowrap: false,
            caret: None,
        selection: None,
        preedit: None,
        selection_style: Default::default(),
        rich: None,
        white_space: Default::default(),
        direction: None,
        isolate: Default::default(),
        },
    );

    let paints = paints(node);
    assert!(
        matches!(paints[0], Paint::Rect(_)),
        "text node should paint its background box first, got {:?}",
        paints[0]
    );
    assert!(matches!(paints[1], Paint::Text(_)));
}

/// Text is drawn in its node's *content* box, so padding moves the words and
/// not just the background behind them.
///
/// It used to be painted at the border box: a padded label sat flush against
/// the edge of its own pill while the pill grew around it, and raising the
/// padding widened the box without moving the text at all.
#[test]
fn padding_insets_the_glyphs_not_just_the_box() {
    let node = Node::text(
        Style {
            padding: Sides { top: 8.0, right: 40.0, bottom: 8.0, left: 40.0 },
            background: Some(Background::Color(Rgba::new(0.2, 0.2, 0.2, 1.0))),
            // This test is about padding, not cross-axis sizing: hug
            // explicitly so it keeps its meaning under the flex default
            // of `stretch`.
            align_self: Some(Align::Start),
            ..Default::default()
        },
        label("hi"),
    );

    let paints = paints(node);
    let Paint::Rect(bg) = &paints[0] else { panic!("background first: {:?}", paints[0]) };
    let Paint::Text(text) = &paints[1] else { panic!("then glyphs: {:?}", paints[1]) };

    // The measure stub reports 50x20, so the box is that plus the padding.
    assert_eq!((bg.width, bg.height), (130.0, 36.0), "the box grew by the padding");
    assert_eq!((text.x - bg.x, text.y - bg.y), (40.0, 8.0), "and the glyphs moved with it");
    assert_eq!(
        (text.width, text.height),
        (50.0, 20.0),
        "the run is aligned and wrapped within the content box, not the border box"
    );
}

/// A border insets the text as well, and stacks with padding: both are box
/// model, and the glyphs belong inside both.
#[test]
fn a_border_insets_the_glyphs_too() {
    let node = Node::text(
        Style {
            padding: Sides { top: 4.0, right: 4.0, bottom: 4.0, left: 4.0 },
            border: Sides { top: 3.0, right: 3.0, bottom: 3.0, left: 3.0 },
            border_color: Some(Rgba::new(1.0, 0.0, 0.0, 1.0)),
            ..Default::default()
        },
        label("hi"),
    );

    let paints = paints(node);
    let Paint::Rect(bg) = &paints[0] else { panic!("border box first: {:?}", paints[0]) };
    let Paint::Text(text) = &paints[1] else { panic!("then glyphs: {:?}", paints[1]) };
    assert_eq!((text.x - bg.x, text.y - bg.y), (7.0, 7.0), "padding plus border");
}

/// An <image> with no CSS size lays out at its intrinsic pixel size; a CSS size
/// scales it.
#[test]
fn image_sizes_from_intrinsic_then_css() {
    let intrinsic = Node::image(
        Style::default(),
        ImageContent {
            src: "logo.png".into(),
            intrinsic: (160.0, 90.0),
        },
    );
    let sized = Node::image(
        Style {
            width: Some(Len::Px(64.0)),
            height: Some(Len::Px(64.0)),
            ..Default::default()
        },
        ImageContent {
            src: "logo.png".into(),
            intrinsic: (160.0, 90.0),
        },
    );

    let boxes: Vec<(f32, f32)> = paints(intrinsic)
        .iter()
        .chain(paints(sized).iter())
        .filter_map(|p| match p {
            Paint::Image(i) => Some((i.width, i.height)),
            _ => None,
        })
        .collect();
    assert_eq!(boxes, vec![(160.0, 90.0), (64.0, 64.0)]);
}

/// opacity wraps the node *and its subtree* in a layer, so the node's own
/// background fades with its children.
#[test]
fn opacity_wraps_the_subtree() {
    let mut faded = Node::new(Style {
        opacity: 0.5,
        background: Some(Background::Color(Rgba::new(0.2, 0.2, 0.2, 1.0))),
        ..Default::default()
    });
    faded.children.push(Node::new(Style {
        background: Some(Background::Color(Rgba::new(1.0, 0.0, 0.0, 1.0))),
        ..Default::default()
    }));

    let paints = paints(faded);
    assert!(
        matches!(paints[0], Paint::PushOpacity { alpha, .. } if alpha == 0.5),
        "layer must open before the node's own background"
    );
    assert!(matches!(paints.last(), Some(Paint::PopOpacity)));
    assert_eq!(
        paints
            .iter()
            .filter(|p| matches!(p, Paint::Rect(_)))
            .count(),
        2,
        "both the node and its child paint inside the layer"
    );
}

// ── per-side borders ────────────────────────────────────────────────────────
//
// `PaintRect` used to carry one `border_width`, filled from `style.border.top`,
// and the painter stroked the whole box with it. So the cascade computed all
// four sides and three of them were thrown away here: a `border-bottom: 6px`
// drew nothing at all, and a `border-top: 6px` drew a box on every side. Both
// silent, and `border-bottom` is in the editor's completion list, which is
// supposed to mean it works.
//
// Reported 2026-09-15 as "the css isn't being applied", against an `<input>`
// styled with `border-bottom`.

/// A box with only one side set reaches paint with only that side set.
#[test]
fn a_one_sided_border_keeps_its_one_side() {
    let node = Node::new(Style {
        width: Some(Len::Px(100.0)),
        height: Some(Len::Px(40.0)),
        border: Sides { top: 0.0, right: 0.0, bottom: 6.0, left: 0.0 },
        border_color: Some(Rgba::new(0.0, 1.0, 0.0, 1.0)),
        ..Default::default()
    });
    let rect = paints(node)
        .into_iter()
        .find_map(|p| match p {
            Paint::Rect(r) if r.border_color.is_some() => Some(r),
            _ => None,
        })
        .expect("the box paints");
    assert_eq!(rect.border.bottom, 6.0, "the side that was written");
    assert_eq!(rect.border.top, 0.0, "and not the one that was not");
    assert_eq!(rect.border.left, 0.0);
    assert_eq!(rect.border.right, 0.0);
}

/// A box with a border only at the top does not come out with four.
#[test]
fn a_top_border_does_not_become_a_box() {
    let node = Node::new(Style {
        width: Some(Len::Px(100.0)),
        height: Some(Len::Px(40.0)),
        border: Sides { top: 6.0, right: 0.0, bottom: 0.0, left: 0.0 },
        border_color: Some(Rgba::new(1.0, 0.0, 0.0, 1.0)),
        ..Default::default()
    });
    let rect = paints(node)
        .into_iter()
        .find_map(|p| match p {
            Paint::Rect(r) if r.border_color.is_some() => Some(r),
            _ => None,
        })
        .expect("the box paints");
    assert!(!rect.border.is_uniform(), "one side is not four");
    assert_eq!((rect.border.top, rect.border.bottom), (6.0, 0.0));
}

/// A box that is painted only because of its border is still painted.
///
/// The emit test is `widest > 0`, not `border.top > 0`, or a box whose only
/// styling is a `border-bottom` would produce no `PaintRect` at all.
#[test]
fn a_box_with_only_a_bottom_border_is_still_emitted() {
    let node = Node::new(Style {
        width: Some(Len::Px(100.0)),
        height: Some(Len::Px(40.0)),
        border: Sides { top: 0.0, right: 0.0, bottom: 2.0, left: 0.0 },
        border_color: Some(Rgba::new(0.0, 0.0, 1.0, 1.0)),
        ..Default::default()
    });
    assert!(
        paints(node).iter().any(|p| matches!(p, Paint::Rect(r) if r.border.bottom == 2.0)),
        "no background, no uniform border, and it still has to be drawn"
    );
}

/// `display: none` reached taffy, which gave the box no size and no slot, and
/// then the node was painted anyway. A text node has glyphs to draw whether or
/// not it has a box, so `display: none` on a `<text>` drew its words at its
/// parent's origin, on top of whatever was really there.
#[test]
fn a_display_none_node_paints_nothing() {
    let mut parent = Node::new(Style {
        display: Display::Flex,
        axis: Axis::Column,
        ..Default::default()
    });
    parent.children.push(Node::text(
        Style { display: Display::None, ..Default::default() },
        label("gone"),
    ));
    parent.children.push(Node::text(Style::default(), label("here")));

    let drawn: Vec<String> = paints(parent)
        .iter()
        .filter_map(|p| match p {
            Paint::Text(t) => Some(t.content.text.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(drawn, vec!["here".to_string()], "only the shown node draws");
}

/// And it takes its subtree with it: a child of a `display: none` box is gone
/// too, however visible its own style says it is.
#[test]
fn a_display_none_subtree_paints_nothing() {
    let mut gone = Node::new(Style {
        display: Display::None,
        background: Some(Background::Color(Rgba::new(1.0, 0.0, 0.0, 1.0))),
        ..Default::default()
    });
    gone.children.push(Node::text(Style::default(), label("inside")));

    let paints = paints(gone);
    assert!(paints.iter().all(|p| !matches!(p, Paint::Text(_))), "{paints:?}");
    assert!(
        paints.iter().all(|p| !matches!(p, Paint::Rect(r) if r.background.is_some())),
        "{paints:?}"
    );
}

/// A shadow list: the outer ones under the box, last written first so the
/// first is on top, and the inset ones over the background, inside the
/// border, around a hole moved by the offset and shrunk by the spread.
#[test]
fn shadows_go_under_and_inset_ones_over_the_background() {
    let shadow = |dx: f32, spread: f32, inset: bool| BoxShadow {
        dx,
        dy: 0.0,
        blur: 4.0,
        spread,
        color: Rgba::new(0.0, 0.0, 0.0, 1.0),
        inset,
    };
    let node = Node::new(Style {
        width: Some(Len::Px(100.0)),
        height: Some(Len::Px(50.0)),
        background: Some(Background::Color(Rgba::new(1.0, 1.0, 1.0, 1.0))),
        border: Sides { top: 2.0, right: 2.0, bottom: 2.0, left: 2.0 },
        border_color: Some(Rgba::new(0.0, 0.0, 0.0, 1.0)),
        radius: [10.0; 4],
        box_shadow: Some(vec![shadow(1.0, 0.0, false), shadow(5.0, 0.0, true), shadow(2.0, 3.0, false)].into()),
        ..Default::default()
    });
    let items = paints(node);
    let kinds: Vec<&str> = items
        .iter()
        .map(|p| match p {
            Paint::Shadow { .. } => "shadow",
            Paint::InsetShadow { .. } => "inset",
            Paint::Rect(_) => "rect",
            _ => "other",
        })
        .collect();
    assert_eq!(kinds, ["shadow", "shadow", "rect", "inset"]);
    // The last written first: its spread grows it by 3 on each side.
    let Paint::Shadow { x, width, .. } = items[0] else { unreachable!() };
    assert_eq!((x, width), (2.0 - 3.0, 106.0));
    let Paint::Shadow { x, .. } = items[1] else { unreachable!() };
    assert_eq!(x, 1.0);
    let Paint::InsetShadow { x, y, width, height, radius, hole, .. } = items[3] else { unreachable!() };
    assert_eq!((x, y, width, height), (2.0, 2.0, 96.0, 46.0));
    assert_eq!(radius, [8.0; 4]);
    assert_eq!(hole, (7.0, 2.0, 96.0, 46.0));
}

/// `visibility: hidden` takes a box's own paint and its tap away and keeps
/// its place; a child that is `visible` again still draws and answers.
#[test]
fn a_hidden_box_draws_nothing_but_a_visible_child_does() {
    let red = Rgba::new(1.0, 0.0, 0.0, 1.0);
    let blue = Rgba::new(0.0, 0.0, 1.0, 1.0);
    let mut parent = Node::new(Style {
        width: Some(Len::Px(100.0)),
        height: Some(Len::Px(50.0)),
        background: Some(Background::Color(red)),
        invisible: true,
        ..Default::default()
    });
    parent.on_tap = Some("a()".into());
    let mut child = Node::new(Style {
        width: Some(Len::Px(20.0)),
        height: Some(Len::Px(20.0)),
        background: Some(Background::Color(blue)),
        ..Default::default()
    });
    child.on_tap = Some("b()".into());
    parent.children.push(child);
    let mut measure = |_: &rux_layout::TextContent, _: Option<f32>| (50.0, 20.0);
    let out = layout(&on_screen(parent), 1000.0, 800.0, &mut measure);
    let fills: Vec<f32> = out
        .paints
        .iter()
        .filter_map(|p| match p {
            Paint::Rect(PaintRect { background: Some(Background::Color(c)), .. }) => Some(c.b),
            _ => None,
        })
        .collect();
    assert_eq!(fills, [1.0], "only the visible child draws");
    let taps: Vec<&str> = out.hits.iter().filter_map(|h| h.on_tap.as_deref()).collect();
    assert_eq!(taps, ["b()"]);
    assert!(out.metrics.iter().any(|m| m.path == [0] && m.width == 100.0), "still measured");
}

/// A blend mode draws the box and its subtree as one layer, opened before
/// its background and closed after its children, at full opacity.
#[test]
fn a_blend_mode_opens_a_layer_around_the_subtree() {
    let mut parent = Node::new(Style {
        width: Some(Len::Px(100.0)),
        height: Some(Len::Px(50.0)),
        background: Some(Background::Color(Rgba::new(1.0, 0.0, 0.0, 1.0))),
        blend: BlendMode::Multiply,
        ..Default::default()
    });
    parent.children.push(Node::new(Style { height: Some(Len::Px(10.0)), ..Default::default() }));
    let items = paints(parent);
    assert!(matches!(items[0], Paint::PushOpacity { alpha, blend: BlendMode::Multiply, .. } if alpha == 1.0));
    assert!(matches!(items[1], Paint::Rect(_)));
    assert!(matches!(items.last(), Some(Paint::PopOpacity)));
}

/// A mask opens a layer before the box and keeps only what it covers just
/// before the layer closes, after the box's children.
#[test]
fn a_mask_closes_its_layer_after_the_subtree() {
    let mut parent = Node::new(Style {
        width: Some(Len::Px(100.0)),
        height: Some(Len::Px(50.0)),
        background: Some(Background::Color(Rgba::new(1.0, 0.0, 0.0, 1.0))),
        mask: Some(Box::new(Mask { image: Background::Color(Rgba::new(0.0, 0.0, 0.0, 1.0)), luminance: false })),
        ..Default::default()
    });
    parent.children.push(Node::new(Style {
        height: Some(Len::Px(10.0)),
        background: Some(Background::Color(Rgba::new(0.0, 1.0, 0.0, 1.0))),
        ..Default::default()
    }));
    let items = paints(parent);
    let n = items.len();
    assert!(matches!(items[0], Paint::PushOpacity { alpha, .. } if alpha == 1.0));
    assert!(matches!(items[n - 3], Paint::Rect(_)), "the child");
    assert!(matches!(items[n - 2], Paint::MaskWith { width, .. } if width == 100.0));
    assert!(matches!(items[n - 1], Paint::PopOpacity));
}
