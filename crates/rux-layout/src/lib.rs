//! Rux layout — milestones M1–M4.
//!
//! A styled node tree fed through `taffy` (flexbox) to produce absolute paint
//! items. Boxes come straight from taffy; text leaves are sized through a
//! caller-supplied `measure` callback (so this crate stays free of any font
//! dependency — the shell owns the text engine). See `docs/04-architecture.md`,
//! Stage 4.

use taffy::prelude::*;

/// Straight RGBA in the 0..=1 range. Renderer-agnostic.
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

/// Per-side box-model lengths (padding / margin / border widths).
#[derive(Clone, Copy, Debug, Default)]
pub struct Sides {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}

impl Sides {
    pub const fn uniform(v: f32) -> Self {
        Self {
            top: v,
            right: v,
            bottom: v,
            left: v,
        }
    }
}

/// How a node lays out its children. Defaults to `Row` to match CSS's
/// `flex-direction` initial value.
#[derive(Clone, Copy, Debug, Default)]
pub enum Axis {
    #[default]
    Row,
    Column,
}

/// Main-axis distribution (`justify-content`).
#[derive(Clone, Copy, Debug)]
pub enum Justify {
    Start,
    Center,
    End,
    SpaceBetween,
    SpaceAround,
}

/// Cross-axis alignment (`align-items`).
#[derive(Clone, Copy, Debug)]
pub enum Align {
    Start,
    Center,
    End,
    Stretch,
}

/// Horizontal text alignment within a text box (`text-align`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum TextAlign {
    #[default]
    Start,
    Center,
    End,
    Justify,
}

/// CSS `display`. Defaults to `Block` (strict-CSS fidelity): flex layout,
/// `gap`, and `flex-direction` only apply under `Flex`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Display {
    #[default]
    Block,
    Flex,
    /// Removed from layout entirely (no space reserved).
    None,
}

/// The style subset M-series understands (a stand-in for the CSS `ComputedStyle`).
#[derive(Clone, Debug)]
pub struct Style {
    pub display: Display,
    pub width: Option<f32>,
    pub height: Option<f32>,
    pub min_width: Option<f32>,
    pub max_width: Option<f32>,
    pub min_height: Option<f32>,
    pub max_height: Option<f32>,
    pub grow: f32,
    pub padding: Sides,
    pub margin: Sides,
    pub border: Sides,
    pub border_color: Option<Rgba>,
    pub gap: f32,
    pub axis: Axis,
    pub justify: Option<Justify>,
    pub align: Option<Align>,
    pub background: Option<Rgba>,
    pub radius: f32,
}

impl Default for Style {
    fn default() -> Self {
        Self {
            display: Display::Block,
            width: None,
            height: None,
            min_width: None,
            max_width: None,
            min_height: None,
            max_height: None,
            grow: 0.0,
            padding: Sides::default(),
            margin: Sides::default(),
            border: Sides::default(),
            border_color: None,
            gap: 0.0,
            axis: Axis::Row,
            justify: None,
            align: None,
            background: None,
            radius: 0.0,
        }
    }
}

/// Text carried by a leaf node.
#[derive(Clone, Debug)]
pub struct TextContent {
    pub text: String,
    pub font_size: f32,
    pub weight: u16,
    pub color: Rgba,
    pub align: TextAlign,
}

/// A node in the view tree: a style, optional text, children, and an optional
/// `@tap` handler (raw handler source, run by the shell on tap).
#[derive(Clone, Debug)]
pub struct Node {
    pub style: Style,
    pub text: Option<TextContent>,
    pub children: Vec<Node>,
    pub on_tap: Option<String>,
    /// `r-show="false"`: laid out (space reserved) but not painted.
    pub hidden: bool,
}

impl Node {
    pub fn new(style: Style) -> Self {
        Self {
            style,
            text: None,
            children: Vec::new(),
            on_tap: None,
            hidden: false,
        }
    }

    pub fn text(style: Style, text: TextContent) -> Self {
        Self {
            style,
            text: Some(text),
            children: Vec::new(),
            on_tap: None,
            hidden: false,
        }
    }

    pub fn with(mut self, child: Node) -> Self {
        self.children.push(child);
        self
    }
}

/// A resolved, absolutely-positioned box: an optional fill and an optional
/// border, sharing one rounded-rect geometry.
#[derive(Clone, Copy, Debug)]
pub struct PaintRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub background: Option<Rgba>,
    pub radius: f32,
    /// Uniform border width for rendering (0 = none).
    pub border_width: f32,
    pub border_color: Option<Rgba>,
}

/// A resolved, absolutely-positioned text block.
#[derive(Clone, Debug)]
pub struct PaintText {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub content: TextContent,
}

/// A drawable item in painter's order (parents before children).
#[derive(Clone, Debug)]
pub enum Paint {
    Rect(PaintRect),
    Text(PaintText),
}

/// An absolutely-positioned tappable region, carrying its `@tap` handler source.
#[derive(Clone, Debug)]
pub struct HitRegion {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub on_tap: String,
}

impl HitRegion {
    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px <= self.x + self.width && py >= self.y && py <= self.y + self.height
    }
}

/// The result of laying out a tree: paint items and hit regions, both in
/// painter's/topmost-last order.
#[derive(Clone, Debug, Default)]
pub struct Layout {
    pub paints: Vec<Paint>,
    pub hits: Vec<HitRegion>,
}

/// Callback that measures a text block: `(text, font_size, weight, max_width) -> (w, h)`.
pub type Measure<'a> = dyn FnMut(&str, f32, u16, Option<f32>) -> (f32, f32) + 'a;

/// What each taffy node paints.
enum PaintKind {
    Box {
        bg: Option<Rgba>,
        radius: f32,
        border_width: f32,
        border_color: Option<Rgba>,
    },
    Text(TextContent),
}

fn to_taffy(style: &Style) -> taffy::Style {
    taffy::Style {
        display: match style.display {
            Display::Block => taffy::Display::Block,
            Display::Flex => taffy::Display::Flex,
            Display::None => taffy::Display::None,
        },
        flex_direction: match style.axis {
            Axis::Column => FlexDirection::Column,
            Axis::Row => FlexDirection::Row,
        },
        justify_content: style.justify.map(|j| match j {
            Justify::Start => JustifyContent::FlexStart,
            Justify::Center => JustifyContent::Center,
            Justify::End => JustifyContent::FlexEnd,
            Justify::SpaceBetween => JustifyContent::SpaceBetween,
            Justify::SpaceAround => JustifyContent::SpaceAround,
        }),
        align_items: style.align.map(|a| match a {
            Align::Start => AlignItems::FlexStart,
            Align::Center => AlignItems::Center,
            Align::End => AlignItems::FlexEnd,
            Align::Stretch => AlignItems::Stretch,
        }),
        flex_grow: style.grow,
        size: Size {
            width: style.width.map(length).unwrap_or(auto()),
            height: style.height.map(length).unwrap_or(auto()),
        },
        min_size: Size {
            width: style.min_width.map(length).unwrap_or(auto()),
            height: style.min_height.map(length).unwrap_or(auto()),
        },
        max_size: Size {
            width: style.max_width.map(length).unwrap_or(auto()),
            height: style.max_height.map(length).unwrap_or(auto()),
        },
        padding: Rect {
            left: length(style.padding.left),
            right: length(style.padding.right),
            top: length(style.padding.top),
            bottom: length(style.padding.bottom),
        },
        margin: Rect {
            left: length(style.margin.left),
            right: length(style.margin.right),
            top: length(style.margin.top),
            bottom: length(style.margin.bottom),
        },
        border: Rect {
            left: length(style.border.left),
            right: length(style.border.right),
            top: length(style.border.top),
            bottom: length(style.border.bottom),
        },
        gap: Size {
            width: length(style.gap),
            height: length(style.gap),
        },
        ..Default::default()
    }
}

fn build(
    tree: &mut TaffyTree<TextContent>,
    node: &Node,
    paint: &mut Vec<(NodeId, PaintKind)>,
    handlers: &mut Vec<(NodeId, String)>,
    hidden: &mut Vec<NodeId>,
) -> NodeId {
    let id = if let Some(tc) = &node.text {
        // Text leaves carry their content as taffy context, so the measure hook
        // can shape them.
        let id = tree
            .new_leaf_with_context(to_taffy(&node.style), tc.clone())
            .expect("taffy text leaf");
        paint.push((id, PaintKind::Text(tc.clone())));
        id
    } else {
        let children: Vec<NodeId> = node
            .children
            .iter()
            .map(|c| build(tree, c, paint, handlers, hidden))
            .collect();
        let id = if children.is_empty() {
            tree.new_leaf(to_taffy(&node.style)).expect("taffy leaf")
        } else {
            tree.new_with_children(to_taffy(&node.style), &children)
                .expect("taffy node")
        };
        paint.push((
            id,
            PaintKind::Box {
                bg: node.style.background,
                radius: node.style.radius,
                // Uniform border for rendering (top width is representative).
                border_width: node.style.border.top,
                border_color: node.style.border_color,
            },
        ));
        id
    };
    if let Some(handler) = &node.on_tap {
        handlers.push((id, handler.clone()));
    }
    if node.hidden {
        hidden.push(id);
    }
    id
}

#[allow(clippy::too_many_arguments)]
fn collect(
    tree: &TaffyTree<TextContent>,
    id: NodeId,
    origin_x: f32,
    origin_y: f32,
    paint: &[(NodeId, PaintKind)],
    handlers: &[(NodeId, String)],
    hidden: &[NodeId],
    out: &mut Layout,
) {
    let layout = tree.layout(id).expect("layout");
    let x = origin_x + layout.location.x;
    let y = origin_y + layout.location.y;

    // r-show=false: the node kept its layout slot but paints nothing (nor its
    // subtree, nor its hit regions).
    if hidden.contains(&id) {
        return;
    }

    if let Some((_, kind)) = paint.iter().find(|(nid, _)| *nid == id) {
        match kind {
            PaintKind::Box {
                bg,
                radius,
                border_width,
                border_color,
            } => {
                let has_border = *border_width > 0.0 && border_color.is_some();
                if bg.is_some() || has_border {
                    out.paints.push(Paint::Rect(PaintRect {
                        x,
                        y,
                        width: layout.size.width,
                        height: layout.size.height,
                        background: *bg,
                        radius: *radius,
                        border_width: *border_width,
                        border_color: *border_color,
                    }));
                }
            }
            PaintKind::Text(tc) => out.paints.push(Paint::Text(PaintText {
                x,
                y,
                width: layout.size.width,
                height: layout.size.height,
                content: tc.clone(),
            })),
        }
    }

    if let Some((_, handler)) = handlers.iter().find(|(nid, _)| *nid == id) {
        out.hits.push(HitRegion {
            x,
            y,
            width: layout.size.width,
            height: layout.size.height,
            on_tap: handler.clone(),
        });
    }

    for child in tree.children(id).expect("children") {
        collect(tree, child, x, y, paint, handlers, hidden, out);
    }
}

/// Lay out `root` into an `avail_w` x `avail_h` viewport, returning paint items
/// and hit regions. Text leaves are sized via `measure`.
pub fn layout(root: &Node, avail_w: f32, avail_h: f32, measure: &mut Measure) -> Layout {
    let mut tree: TaffyTree<TextContent> = TaffyTree::new();
    let mut paint = Vec::new();
    let mut handlers = Vec::new();
    let mut hidden = Vec::new();
    let root_id = build(&mut tree, root, &mut paint, &mut handlers, &mut hidden);

    // Force the root to fill the viewport so a `screen` always covers the window.
    let mut root_style = to_taffy(&root.style);
    root_style.size = Size {
        width: length(avail_w),
        height: length(avail_h),
    };
    tree.set_style(root_id, root_style).expect("set root style");

    tree.compute_layout_with_measure(
        root_id,
        Size {
            width: AvailableSpace::Definite(avail_w),
            height: AvailableSpace::Definite(avail_h),
        },
        |known, available, _id, ctx, _style| {
            if let (Some(w), Some(h)) = (known.width, known.height) {
                return Size { width: w, height: h };
            }
            match ctx {
                Some(tc) => {
                    let max = known.width.or(match available.width {
                        AvailableSpace::Definite(w) => Some(w),
                        _ => None,
                    });
                    let (w, h) = measure(&tc.text, tc.font_size, tc.weight, max);
                    Size {
                        width: known.width.unwrap_or(w),
                        height: known.height.unwrap_or(h),
                    }
                }
                None => Size {
                    width: 0.0,
                    height: 0.0,
                },
            }
        },
    )
    .expect("compute layout");

    let mut out = Layout::default();
    collect(&tree, root_id, 0.0, 0.0, &paint, &handlers, &hidden, &mut out);
    out
}
