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
