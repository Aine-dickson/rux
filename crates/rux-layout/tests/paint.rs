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
    let mut measure = |_: &str, _: f32, _: u16, _: Option<f32>| (50.0, 20.0);
    layout(&on_screen(root), 1000.0, 800.0, &mut measure).paints
}

/// A text node is a box too — its background and border paint under the glyphs.
/// (Only container boxes used to paint, so a styled <text> came out bare.)
#[test]
fn text_node_paints_its_background_then_its_glyphs() {
    let node = Node::text(
        Style {
            background: Some(Rgba::new(0.2, 0.2, 0.2, 1.0)),
            radius: 6.0,
            ..Default::default()
        },
        TextContent {
            text: "hi".into(),
            font_size: 16.0,
            weight: 400,
            color: Rgba::new(1.0, 1.0, 1.0, 1.0),
            align: TextAlign::Start,
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
        background: Some(Rgba::new(0.2, 0.2, 0.2, 1.0)),
        ..Default::default()
    });
    faded.children.push(Node::new(Style {
        background: Some(Rgba::new(1.0, 0.0, 0.0, 1.0)),
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
