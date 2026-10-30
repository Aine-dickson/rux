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
        shadows: None,
        selection_style: Default::default(),
        look: None,
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
        shadows: None,
        selection_style: Default::default(),
        look: None,
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
        ImageContent { intrinsic: (160.0, 90.0), ..ImageContent::new("logo.png".into()) },
    );
    let sized = Node::image(
        Style {
            width: Some(Len::Px(64.0)),
            height: Some(Len::Px(64.0)),
            ..Default::default()
        },
        ImageContent { intrinsic: (160.0, 90.0), ..ImageContent::new("logo.png".into()) },
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
        box_sizing: BoxSizing::BorderBox,
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
    // Its round corners grow by the spread too, as CSS's do.
    let Paint::Shadow { x, width, radius, .. } = items[0] else { unreachable!() };
    assert_eq!((x, width, radius), (2.0 - 3.0, 106.0, 13.0));
    let Paint::Shadow { x, radius, .. } = items[1] else { unreachable!() };
    assert_eq!((x, radius), (1.0, 10.0));
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
    assert!(out.metrics.iter().any(|m| *m.path == [0] && m.width == 100.0), "still measured");
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

/// A text's shadows go under its glyphs in one colour, the last written
/// lowest; a blurred one is drawn apart, in bounds around the text.
#[test]
fn text_shadows_go_under_the_glyphs() {
    let mut content = label("hi");
    content.shadows = Some(std::sync::Arc::new(vec![
        TextShadow { dx: 1.0, dy: 1.0, blur: 0.0, color: None },
        TextShadow { dx: 2.0, dy: 2.0, blur: 4.0, color: Some(Rgba::new(1.0, 0.0, 0.0, 1.0)) },
    ]));
    let mut node = Node::new(Style::default());
    node.text = Some(Box::new(content));
    let items = paints(node);
    let kinds: Vec<&str> = items
        .iter()
        .map(|p| match p {
            Paint::PushFilter { .. } => "push",
            Paint::PopFilter => "pop",
            Paint::Ink(..) => "ink",
            Paint::Text(_) => "text",
            _ => "other",
        })
        .filter(|k| *k != "other")
        .collect();
    assert_eq!(kinds, ["push", "ink", "pop", "ink", "text"]);
    let Some(Paint::PushFilter { bounds: Some((bx, _, bw, _)), .. }) = items.iter().find(|p| matches!(p, Paint::PushFilter { .. })) else {
        panic!("no bounds")
    };
    assert!(*bx < 2.0 && *bw > 50.0, "{bx} {bw}");
    // No colour of its own is the text's.
    let inks: Vec<Rgba> = items.iter().filter_map(|p| if let Paint::Ink(_, c) = p { Some(*c) } else { None }).collect();
    assert_eq!((inks[0].r, inks[1].g), (1.0, 1.0));
}

/// Image layers are drawn on their own, under the border, with the box's
/// border and padding to place them by; the rect after them keeps only the
/// border.
#[test]
fn image_layers_paint_apart_with_the_boxes_they_are_placed_in() {
    let layers = std::sync::Arc::new(BgLayers {
        color: Some(Rgba::new(1.0, 0.0, 0.0, 1.0)),
        layers: vec![BgLayer::of(Background::Image("a.png".into()))],
    });
    let node = Node::new(Style {
        width: Some(Len::Px(100.0)),
        height: Some(Len::Px(50.0)),
        border: Sides::uniform(2.0),
        border_color: Some(Rgba::new(0.0, 0.0, 0.0, 1.0)),
        border_style: [BorderStyle::Solid; 4],
        padding: Sides::uniform(3.0),
        box_sizing: BoxSizing::BorderBox,
        background: Some(Background::Layers(layers)),
        ..Default::default()
    });
    let out = paints(node);
    let at = out.iter().position(|p| matches!(p, Paint::Background(_))).expect("a background paint");
    let Paint::Background(b) = &out[at] else { unreachable!() };
    assert_eq!((b.width, b.height), (100.0, 50.0));
    assert_eq!((b.border.left, b.padding.top), (2.0, 3.0));
    assert_eq!(b.view, (0.0, 0.0, 1000.0, 800.0));
    // Not scrolled, `local` is the padding box.
    assert_eq!(b.local, (b.x + 2.0, b.y + 2.0, 96.0, 46.0));
    match &out[at + 1] {
        Paint::Rect(r) => assert!(r.background.is_none() && r.border_color.is_some()),
        other => panic!("expected the border after the background, got {other:?}"),
    }
}

/// `object-fit` sizes a picture in its box and `object-position` places it:
/// a 200 x 100 picture in a 100 x 100 box.
#[test]
fn object_fit_and_position_place_the_picture() {
    let at = |fit: ObjectFit, position: [BgPos; 2]| {
        let image = ImageContent { fit, position, ..ImageContent::new("a.png".into()) };
        image.placed((0.0, 0.0, 100.0, 100.0), (200.0, 100.0))
    };
    let centre = [BgPos { pct: 50.0, px: 0.0 }; 2];
    assert_eq!(at(ObjectFit::Fill, centre), (0.0, 0.0, 100.0, 100.0));
    assert_eq!(at(ObjectFit::Contain, centre), (0.0, 25.0, 100.0, 50.0));
    assert_eq!(at(ObjectFit::Cover, centre), (-50.0, 0.0, 200.0, 100.0));
    assert_eq!(at(ObjectFit::None, centre), (-50.0, 0.0, 200.0, 100.0));
    assert_eq!(at(ObjectFit::ScaleDown, centre), (0.0, 25.0, 100.0, 50.0));
    let left_top = [BgPos::default(); 2];
    assert_eq!(at(ObjectFit::Contain, left_top), (0.0, 0.0, 100.0, 50.0));
}

/// The picture goes in the content box, inside the image's padding.
#[test]
fn an_image_is_drawn_inside_its_padding() {
    let node = Node::image(
        Style {
            width: Some(Len::Px(100.0)),
            height: Some(Len::Px(100.0)),
            padding: Sides::uniform(10.0),
            box_sizing: BoxSizing::BorderBox,
            ..Default::default()
        },
        ImageContent { intrinsic: (50.0, 50.0), ..ImageContent::new("a.png".into()) },
    );
    let out = paints(node);
    let img = out.iter().find_map(|p| if let Paint::Image(i) = p { Some(i) } else { None }).expect("image");
    assert_eq!((img.width, img.height), (80.0, 80.0));
}

/// `pointer-events: none` takes a box out of hit testing, so a tap goes to
/// what is under it, but the keyboard still reaches it: Tab lands on it and
/// Enter runs its `@tap`.
#[test]
fn pointer_events_none_is_not_hit_but_is_still_focusable() {
    let mut measure = |_: &TextContent, _: Option<f32>| (50.0, 20.0);
    let button = |no_pointer: bool| {
        let mut n = Node::new(Style { width: Some(Len::Px(80.0)), height: Some(Len::Px(30.0)), no_pointer, ..Default::default() });
        n.on_tap = Some("go()".into());
        n
    };
    let shown = layout(&on_screen(button(false)), 400.0, 300.0, &mut measure);
    let taps = |l: &Layout| l.hits.iter().filter(|h| h.on_tap.is_some() || !h.gestures.is_empty()).count();
    assert_eq!((taps(&shown), shown.focusables.len()), (1, 1));
    let passed = layout(&on_screen(button(true)), 400.0, 300.0, &mut measure);
    assert_eq!((taps(&passed), passed.focusables.len()), (0, 1));
}

/// A 200 x 100 screen holding a button at its corner and whatever `over` is,
/// absolutely placed over the whole screen after it.
fn under(over: Node) -> Layout {
    let mut measure = |_: &TextContent, _: Option<f32>| (20.0, 10.0);
    let mut root = Node::new(Style { width: Some(Len::Px(200.0)), height: Some(Len::Px(100.0)), ..Default::default() });
    let mut button = Node::new(Style { width: Some(Len::Px(80.0)), height: Some(Len::Px(30.0)), ..Default::default() });
    button.on_tap = Some("go()".into());
    button.style.cursor = Cursor::Pointer;
    root.children.push(button);
    root.children.push(over);
    layout(&on_screen(root), 200.0, 100.0, &mut measure)
}

fn cover() -> Node {
    Node::new(Style {
        position: Position::Absolute,
        inset: [Some(Len::Px(0.0)); 4],
        ..Default::default()
    })
}

/// A backdrop with no `@tap` keeps a tap from the button under it, as in a
/// browser, where the backdrop is the target and the button never hears it.
/// Before watchlist #46 the tap went straight through.
#[test]
fn a_box_with_no_handler_takes_the_tap() {
    let out = under(cover());
    assert!(tap_at(&out.hits, 10.0, 10.0).is_none(), "the backdrop has the tap");
    assert_eq!(cursor_at(&out.hits, 10.0, 10.0), Cursor::Auto);
}

/// `cursor` on a box with no handler shows. It once rode on the handler
/// record, and a `cursor-pointer` card with its `@tap` on a parent, or none
/// at all, kept the arrow.
#[test]
fn a_box_with_no_handler_shows_its_own_cursor() {
    let mut c = cover();
    c.style.cursor = Cursor::Pointer;
    assert_eq!(cursor_at(&under(c).hits, 10.0, 10.0), Cursor::Pointer);
    let mut c = cover();
    c.style.cursor = Cursor::NotAllowed;
    assert_eq!(cursor_at(&under(c).hits, 150.0, 80.0), Cursor::NotAllowed, "away from the button too");
}

/// A box inside a button that says `cursor: default` keeps the arrow, as in
/// a browser; one that says nothing shows the button's hand.
#[test]
fn an_explicit_cursor_inside_a_button_is_its_own() {
    let mut measure = |_: &TextContent, _: Option<f32>| (20.0, 10.0);
    let mut button = Node::new(Style { width: Some(Len::Px(80.0)), height: Some(Len::Px(30.0)), ..Default::default() });
    button.on_tap = Some("go()".into());
    button.style.cursor = Cursor::Pointer;
    let mut plain = Node::new(Style { width: Some(Len::Px(20.0)), height: Some(Len::Px(20.0)), ..Default::default() });
    plain.style.cursor = Cursor::Default;
    button.children.push(plain);
    let out = layout(&on_screen(button), 200.0, 100.0, &mut measure);
    assert_eq!(cursor_at(&out.hits, 5.0, 5.0), Cursor::Default);
    assert_eq!(cursor_at(&out.hits, 50.0, 5.0), Cursor::Pointer);
}

/// Under `pointer-events: none` the backdrop is passed through again.
#[test]
fn a_backdrop_under_pointer_events_none_lets_the_tap_through() {
    let mut c = cover();
    c.style.no_pointer = true;
    let out = under(c);
    assert_eq!(tap_at(&out.hits, 10.0, 10.0).and_then(|h| h.on_tap.as_deref()), Some("go()"));
}

/// A tap on something inside a button is the button's, as a click bubbles,
/// and so is its cursor. Something inside a backdrop is the backdrop's: an
/// inner box does not reach past it to the button.
#[test]
fn a_tap_inside_a_box_goes_to_its_nearest_handler() {
    let mut measure = |_: &TextContent, _: Option<f32>| (20.0, 10.0);
    let mut button = Node::new(Style { width: Some(Len::Px(80.0)), height: Some(Len::Px(30.0)), ..Default::default() });
    button.on_tap = Some("go()".into());
    button.style.cursor = Cursor::Pointer;
    let mut dragged = Node::new(Style { width: Some(Len::Px(20.0)), height: Some(Len::Px(20.0)), ..Default::default() });
    dragged.gestures = vec![(Gesture::Drag, "move()".into())];
    button.children.push(Node::new(Style { width: Some(Len::Px(20.0)), height: Some(Len::Px(20.0)), ..Default::default() }));
    button.children.push(dragged);
    let out = layout(&on_screen(button), 200.0, 100.0, &mut measure);
    // The children stack: the plain one at the top, the dragged one under it.
    for y in [5.0, 25.0] {
        let tap = tap_at(&out.hits, 5.0, y).expect("the button");
        assert_eq!(tap.on_tap.as_deref(), Some("go()"), "at {y}");
        assert_eq!(cursor_at(&out.hits, 5.0, y), Cursor::Pointer, "at {y}");
    }
    assert!(gesture_at(&out.hits, 5.0, 5.0).is_none(), "the plain child has no drag");
    assert!(gesture_at(&out.hits, 5.0, 25.0).is_some(), "the dragged one has");
    assert!(gesture_at(&out.hits, 60.0, 5.0).is_none());

    let mut c = cover();
    c.children.push(Node::new(Style { width: Some(Len::Px(50.0)), height: Some(Len::Px(50.0)), ..Default::default() }));
    let out = under(c);
    assert!(tap_at(&out.hits, 10.0, 10.0).is_none(), "the backdrop's child is the backdrop's");
}

/// A box over a text field keeps the press from it, and so does
/// `pointer-events: none` on the field (watchlist #47). A label reaches.
#[test]
fn a_covered_field_is_not_reached() {
    let mut measure = |_: &TextContent, _: Option<f32>| (20.0, 10.0);
    let screen = |field_none: bool, covered: bool| {
        let mut root = Node::new(Style { width: Some(Len::Px(200.0)), height: Some(Len::Px(100.0)), ..Default::default() });
        let mut field = Node::new(Style { width: Some(Len::Px(80.0)), height: Some(Len::Px(30.0)), no_pointer: field_none, ..Default::default() });
        field.model = Some("name".into());
        // What it shows, which a press lands on first.
        field.children.push(Node::new(Style { width: Some(Len::Px(60.0)), height: Some(Len::Px(20.0)), ..Default::default() }));
        let mut label = Node::new(Style { width: Some(Len::Px(80.0)), height: Some(Len::Px(30.0)), ..Default::default() });
        label.focus_model = Some("name".into());
        root.children.push(field);
        root.children.push(label);
        if covered {
            root.children.push(cover());
        }
        root
    };
    let mut reach = |field_none, covered| {
        let out = layout(&on_screen(screen(field_none, covered)), 200.0, 100.0, &mut measure);
        (field_reachable(&out.hits, 10.0, 10.0), field_reachable(&out.hits, 10.0, 40.0))
    };
    assert_eq!(reach(false, false), (true, true), "the field and its label");
    assert_eq!(reach(false, true), (false, false), "both covered");
    assert_eq!(reach(true, false), (false, true), "the field passes the press on; its label still reaches");
}

/// `transform-origin` moves the point a transform turns about: a box scaled
/// to nothing about its top left shrinks into that corner, not its centre.
#[test]
fn a_transform_turns_about_its_origin() {
    let pushed = |origin: Option<[Len; 2]>| {
        let node = Node::new(Style {
            width: Some(Len::Px(100.0)),
            height: Some(Len::Px(50.0)),
            transform: Some([0.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
            rare: origin.map(|o| Box::new(Rare { transform_origin: Some(o), ..Default::default() })),
            ..Default::default()
        });
        paints(node).into_iter().find_map(|p| match p {
            Paint::PushTransform(m) => Some((m[4], m[5])),
            _ => None,
        })
    };
    assert_eq!(pushed(None), Some((50.0, 25.0)), "the centre by default");
    assert_eq!(pushed(Some([Len::Pct(0.0), Len::Pct(0.0)])), Some((0.0, 0.0)));
    assert_eq!(pushed(Some([Len::Px(10.0), Len::Pct(1.0)])), Some((10.0, 50.0)));
}

/// A translation in a percentage is of the box's own size, put in front of
/// the rest of the transform: Tailwind's `-translate-x-1/2` centring.
#[test]
fn a_percentage_translation_moves_by_the_box() {
    let node = Node::new(Style {
        width: Some(Len::Px(100.0)),
        height: Some(Len::Px(50.0)),
        rare: Some(Box::new(Rare { translate: Some([Len::Pct(-0.5), Len::Pct(1.0)]), ..Default::default() })),
        ..Default::default()
    });
    let moved = paints(node).into_iter().find_map(|p| match p {
        Paint::PushTransform(m) => Some((m[4], m[5])),
        _ => None,
    });
    assert_eq!(moved, Some((-50.0, 50.0)));
}
