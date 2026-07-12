//! Rux layout — milestone M1.
//!
//! A minimal styled node tree fed through `taffy` (a flexbox engine) to produce
//! absolute, painted rectangles. This is a deliberately small slice of Stage 4
//! in `docs/04-architecture.md`: no CSS parsing yet (that's M2), no text (M4).
//! The node tree here is built in Rust by hand; later it comes from the parser.

use taffy::prelude::*;

/// Straight RGBA in the 0..=1 range. Kept renderer-agnostic so this crate has no
/// dependency on the paint backend; `rux-paint` converts to its own colour type.
#[derive(Clone, Copy, Debug)]
pub struct Rgba {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Rgba {
    pub const fn new(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }
}

/// How a node lays out its children.
#[derive(Clone, Copy, Debug, Default)]
pub enum Axis {
    #[default]
    Column,
    Row,
}

/// The subset of style M1 understands. A hand-built stand-in for the
/// `ComputedStyle` that the CSS cascade will produce in M2.
#[derive(Clone, Debug)]
pub struct Style {
    /// `None` = auto (fill/according to content); `Some(px)` = fixed length.
    pub width: Option<f32>,
    pub height: Option<f32>,
    pub grow: f32,
    pub padding: f32,
    pub gap: f32,
    pub axis: Axis,
    pub background: Option<Rgba>,
    pub radius: f32,
}

impl Default for Style {
    fn default() -> Self {
        Self {
            width: None,
            height: None,
            grow: 0.0,
            padding: 0.0,
            gap: 0.0,
            axis: Axis::Column,
            background: None,
            radius: 0.0,
        }
    }
}

/// A node in the view tree: a style plus children.
#[derive(Clone, Debug)]
pub struct Node {
    pub style: Style,
    pub children: Vec<Node>,
}

impl Node {
    pub fn new(style: Style) -> Self {
        Self {
            style,
            children: Vec::new(),
        }
    }

    pub fn with(mut self, child: Node) -> Self {
        self.children.push(child);
        self
    }
}

/// A resolved rectangle in absolute window coordinates, ready to paint.
#[derive(Clone, Copy, Debug)]
pub struct PaintRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub color: Rgba,
    pub radius: f32,
}

/// Translate our `Style` into a taffy `Style`.
fn to_taffy(style: &Style) -> taffy::Style {
    taffy::Style {
        display: Display::Flex,
        flex_direction: match style.axis {
            Axis::Column => FlexDirection::Column,
            Axis::Row => FlexDirection::Row,
        },
        flex_grow: style.grow,
        size: Size {
            width: style.width.map(length).unwrap_or(auto()),
            height: style.height.map(length).unwrap_or(auto()),
        },
        padding: length(style.padding),
        gap: Size {
            width: length(style.gap),
            height: length(style.gap),
        },
        ..Default::default()
    }
}

/// Build the taffy tree recursively, recording paint info alongside each node id.
fn build(
    tree: &mut TaffyTree<()>,
    node: &Node,
    paint: &mut Vec<(NodeId, Option<Rgba>, f32)>,
) -> NodeId {
    let child_ids: Vec<NodeId> = node.children.iter().map(|c| build(tree, c, paint)).collect();
    let id = tree
        .new_with_children(to_taffy(&node.style), &child_ids)
        .expect("taffy node");
    paint.push((id, node.style.background, node.style.radius));
    id
}

/// Walk the computed layout, accumulating parent offsets into absolute rects.
fn collect(
    tree: &TaffyTree<()>,
    id: NodeId,
    origin_x: f32,
    origin_y: f32,
    paint: &[(NodeId, Option<Rgba>, f32)],
    out: &mut Vec<PaintRect>,
) {
    let layout = tree.layout(id).expect("layout");
    let x = origin_x + layout.location.x;
    let y = origin_y + layout.location.y;

    if let Some((_, Some(color), radius)) = paint.iter().find(|(nid, _, _)| *nid == id).copied() {
        out.push(PaintRect {
            x,
            y,
            width: layout.size.width,
            height: layout.size.height,
            color,
            radius,
        });
    }

    for child in tree.children(id).expect("children") {
        collect(tree, child, x, y, paint, out);
    }
}

/// Lay out `root` into an `avail_w` x `avail_h` viewport and return the
/// absolute rectangles to paint, parents before children (painter's order).
pub fn layout(root: &Node, avail_w: f32, avail_h: f32) -> Vec<PaintRect> {
    let mut tree: TaffyTree<()> = TaffyTree::new();
    let mut paint = Vec::new();
    let root_id = build(&mut tree, root, &mut paint);

    // Force the root to fill the viewport regardless of its own size style, so a
    // `screen` always covers the window.
    let mut root_style = to_taffy(&root.style);
    root_style.size = Size {
        width: length(avail_w),
        height: length(avail_h),
    };
    tree.set_style(root_id, root_style).expect("set root style");

    tree.compute_layout(
        root_id,
        Size {
            width: AvailableSpace::Definite(avail_w),
            height: AvailableSpace::Definite(avail_h),
        },
    )
    .expect("compute layout");

    let mut out = Vec::new();
    collect(&tree, root_id, 0.0, 0.0, &paint, &mut out);
    out
}
