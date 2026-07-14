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

/// A CSS length. Percentages are stored as a fraction (`0.0..=1.0`); `vh`/`vw`
/// hold the raw viewport-percentage number (e.g. `100vh` → `Vh(100.0)`). `rem`
/// is resolved to pixels at parse time.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Len {
    Px(f32),
    Pct(f32),
    Vw(f32),
    Vh(f32),
}

/// A grid track size (`grid-template-columns`/`-rows`).
#[derive(Clone, Copy, Debug)]
pub enum Track {
    Px(f32),
    Fr(f32),
    Auto,
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
    /// Hugs its content and does not stretch to fill (works inside flex parents;
    /// taffy has no true inline text flow).
    Inline,
    Flex,
    Grid,
    /// Removed from layout entirely (no space reserved).
    None,
}

/// Overflow behaviour for content exceeding a box.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Overflow {
    #[default]
    Visible,
    /// Clip the subtree to this box (covers hidden/auto/scroll for now).
    Clip,
}

/// The style subset M-series understands (a stand-in for the CSS `ComputedStyle`).
#[derive(Clone, Debug)]
pub struct Style {
    pub display: Display,
    pub width: Option<Len>,
    pub height: Option<Len>,
    pub min_width: Option<Len>,
    pub max_width: Option<Len>,
    pub min_height: Option<Len>,
    pub max_height: Option<Len>,
    pub grid_columns: Vec<Track>,
    pub grid_rows: Vec<Track>,
    pub grow: f32,
    /// `flex-shrink`. CSS defaults to 1: a flex item gives up space to fit its
    /// container. `0` keeps the item's size and lets it overflow — which is the
    /// author's call, and what `overflow: clip` is for.
    pub shrink: f32,
    /// `flex-basis`. `None` = `auto` (size from width/content).
    pub basis: Option<Len>,
    /// `flex-wrap: wrap` — items that don't fit start a new line.
    pub wrap: bool,
    /// `opacity`, 0.0–1.0. Applies to the whole subtree.
    pub opacity: f32,
    pub padding: Sides,
    pub margin: Sides,
    pub border: Sides,
    pub border_color: Option<Rgba>,
    pub gap: f32,
    pub axis: Axis,
    pub justify: Option<Justify>,
    pub align: Option<Align>,
    pub overflow: Overflow,
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
            grid_columns: Vec::new(),
            grid_rows: Vec::new(),
            grow: 0.0,
            shrink: 1.0,
            basis: None,
            wrap: false,
            opacity: 1.0,
            padding: Sides::default(),
            margin: Sides::default(),
            border: Sides::default(),
            border_color: None,
            gap: 0.0,
            axis: Axis::Row,
            justify: None,
            align: None,
            overflow: Overflow::Visible,
            background: None,
            radius: 0.0,
        }
    }
}

/// An image carried by a leaf node. `src` is resolved to a path the painter can
/// open; the intrinsic size is filled in by the runtime (it reads the file's
/// header) and sizes the box when CSS gives no width/height.
#[derive(Clone, Debug)]
pub struct ImageContent {
    pub src: String,
    pub intrinsic: (f32, f32),
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
    /// `<image src=…>`.
    pub image: Option<ImageContent>,
    pub children: Vec<Node>,
    pub on_tap: Option<String>,
    /// `r-model` signal name for `<input>` nodes (focus target + edit binding).
    pub model: Option<String>,
    /// `r-show="false"`: laid out (space reserved) but not painted.
    pub hidden: bool,
}

impl Node {
    pub fn new(style: Style) -> Self {
        Self {
            style,
            text: None,
            image: None,
            children: Vec::new(),
            on_tap: None,
            model: None,
            hidden: false,
        }
    }

    pub fn text(style: Style, text: TextContent) -> Self {
        Self {
            style,
            text: Some(text),
            image: None,
            children: Vec::new(),
            on_tap: None,
            model: None,
            hidden: false,
        }
    }

    pub fn image(style: Style, image: ImageContent) -> Self {
        Self {
            style,
            text: None,
            image: Some(image),
            children: Vec::new(),
            on_tap: None,
            model: None,
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

/// An image scaled to fill its laid-out box.
#[derive(Clone, Debug)]
pub struct PaintImage {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub content: ImageContent,
}

/// A drawable item in painter's order (parents before children).
#[derive(Clone, Debug)]
pub enum Paint {
    Rect(PaintRect),
    Text(PaintText),
    Image(PaintImage),
    /// Begin clipping subsequent items to this rounded rect (overflow: clip).
    PushClip {
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        radius: f32,
    },
    /// End the most recent clip.
    PopClip,
    /// Begin a translucent layer over the subtree (`opacity`). The shape is the
    /// whole viewport, so the layer fades without also clipping.
    PushOpacity {
        alpha: f32,
        width: f32,
        height: f32,
    },
    /// End the most recent opacity layer.
    PopOpacity,
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

/// An absolutely-positioned focusable region for an `<input>`, carrying its
/// `r-model` signal name.
#[derive(Clone, Debug)]
pub struct FocusRegion {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub model: String,
}

impl FocusRegion {
    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px <= self.x + self.width && py >= self.y && py <= self.y + self.height
    }
}

/// The result of laying out a tree: paint items, hit regions, and focus regions,
/// all in painter's/topmost-last order.
#[derive(Clone, Debug, Default)]
pub struct Layout {
    pub paints: Vec<Paint>,
    pub hits: Vec<HitRegion>,
    pub focuses: Vec<FocusRegion>,
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
        clip: bool,
    },
    Text(TextContent),
    Image(ImageContent),
}

fn to_dim(l: Len, vp: (f32, f32)) -> Dimension {
    match l {
        Len::Px(v) => length(v),
        Len::Pct(p) => percent(p),
        Len::Vw(v) => length(vp.0 * v / 100.0),
        Len::Vh(v) => length(vp.1 * v / 100.0),
    }
}

fn to_track(t: Track) -> TrackSizingFunction {
    match t {
        Track::Px(v) => length(v),
        Track::Fr(f) => fr(f),
        Track::Auto => auto(),
    }
}

/// `vp` is the viewport `(width, height)` in physical pixels, for `vw`/`vh`.
fn to_taffy(style: &Style, vp: (f32, f32)) -> taffy::Style {
    taffy::Style {
        display: match style.display {
            // Inline is a normal (block) box; the hug comes from width:auto plus
            // not stretching (taffy has no true inline flow).
            Display::Block | Display::Inline => taffy::Display::Block,
            Display::Flex => taffy::Display::Flex,
            Display::Grid => taffy::Display::Grid,
            Display::None => taffy::Display::None,
        },
        grid_template_columns: style.grid_columns.iter().copied().map(to_track).collect(),
        grid_template_rows: style.grid_rows.iter().copied().map(to_track).collect(),
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
        // Default flex cross-alignment is flex-start (hug), not taffy's stretch,
        // so children keep their own width unless the author asks to stretch.
        align_items: style
            .align
            .map(|a| match a {
                Align::Start => AlignItems::FlexStart,
                Align::Center => AlignItems::Center,
                Align::End => AlignItems::FlexEnd,
                Align::Stretch => AlignItems::Stretch,
            })
            .or(if style.display == Display::Flex {
                Some(AlignItems::FlexStart)
            } else {
                None
            }),
        flex_grow: style.grow,
        flex_shrink: style.shrink,
        flex_basis: style.basis.map(|l| to_dim(l, vp)).unwrap_or(auto()),
        flex_wrap: if style.wrap {
            FlexWrap::Wrap
        } else {
            FlexWrap::NoWrap
        },
        size: Size {
            width: style.width.map(|l| to_dim(l, vp)).unwrap_or(auto()),
            height: style.height.map(|l| to_dim(l, vp)).unwrap_or(auto()),
        },
        min_size: Size {
            width: style.min_width.map(|l| to_dim(l, vp)).unwrap_or(auto()),
            height: style.min_height.map(|l| to_dim(l, vp)).unwrap_or(auto()),
        },
        max_size: Size {
            // A box with no width hugs its content. Hug means CSS `fit-content`
            // — min(max-content, available) — so clamp it to the parent's inner
            // width. Without this, taffy hands a hugging box its full max-content
            // size and it bursts out of a narrower parent. An explicit width or
            // max-width is the author's call and is left alone.
            width: match (style.max_width, style.width) {
                (Some(l), _) => to_dim(l, vp),
                // `flex-shrink: 0` says "keep my size" — don't clamp behind the
                // author's back; let it overflow and let the parent clip it.
                (None, None) if style.shrink != 0.0 => percent(1.0),
                (None, _) => auto(),
            },
            height: style.max_height.map(|l| to_dim(l, vp)).unwrap_or(auto()),
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

#[allow(clippy::too_many_arguments)]
fn build(
    tree: &mut TaffyTree<TextContent>,
    node: &Node,
    paint: &mut Vec<(NodeId, PaintKind)>,
    handlers: &mut Vec<(NodeId, String)>,
    models: &mut Vec<(NodeId, String)>,
    hidden: &mut Vec<NodeId>,
    opacities: &mut Vec<(NodeId, f32)>,
    vp: (f32, f32),
) -> NodeId {
    let id = if let Some(tc) = &node.text {
        // Text leaves carry their content as taffy context so the measure hook
        // can shape them.
        let id = tree
            .new_leaf_with_context(to_taffy(&node.style, vp), tc.clone())
            .expect("taffy text leaf");
        // A text node is a box too: its background and border paint under the
        // glyphs. (collect() walks every paint entry for a node, in order.)
        paint.push((
            id,
            PaintKind::Box {
                bg: node.style.background,
                radius: node.style.radius,
                border_width: node.style.border.top,
                border_color: node.style.border_color,
                clip: node.style.overflow == Overflow::Clip,
            },
        ));
        paint.push((id, PaintKind::Text(tc.clone())));
        id
    } else if let Some(ic) = &node.image {
        // An image with no CSS size falls back to its intrinsic pixel size, the
        // way a browser sizes an <img>.
        let mut ts = to_taffy(&node.style, vp);
        if node.style.width.is_none() {
            ts.size.width = length(ic.intrinsic.0);
        }
        if node.style.height.is_none() {
            ts.size.height = length(ic.intrinsic.1);
        }
        let id = tree.new_leaf(ts).expect("taffy image leaf");
        paint.push((
            id,
            PaintKind::Box {
                bg: node.style.background,
                radius: node.style.radius,
                border_width: node.style.border.top,
                border_color: node.style.border_color,
                clip: node.style.overflow == Overflow::Clip,
            },
        ));
        paint.push((id, PaintKind::Image(ic.clone())));
        id
    } else {
        let children: Vec<NodeId> = node
            .children
            .iter()
            .map(|c| build(tree, c, paint, handlers, models, hidden, opacities, vp))
            .collect();
        let id = if children.is_empty() {
            tree.new_leaf(to_taffy(&node.style, vp)).expect("taffy leaf")
        } else {
            tree.new_with_children(to_taffy(&node.style, vp), &children)
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
                clip: node.style.overflow == Overflow::Clip,
            },
        ));
        id
    };
    if let Some(handler) = &node.on_tap {
        handlers.push((id, handler.clone()));
    }
    if let Some(model) = &node.model {
        models.push((id, model.clone()));
    }
    if node.hidden {
        hidden.push(id);
    }
    if node.style.opacity < 1.0 {
        opacities.push((id, node.style.opacity.max(0.0)));
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
    models: &[(NodeId, String)],
    hidden: &[NodeId],
    opacities: &[(NodeId, f32)],
    vp: (f32, f32),
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

    // opacity fades this node and everything under it, so the layer opens
    // before the node paints its own background.
    let alpha = opacities
        .iter()
        .find(|(nid, _)| *nid == id)
        .map(|(_, a)| *a)
        .unwrap_or(1.0);
    if alpha < 1.0 {
        out.paints.push(Paint::PushOpacity {
            alpha,
            width: vp.0,
            height: vp.1,
        });
    }

    let mut clip = false;
    let mut clip_radius = 0.0;
    // A node can emit more than one paint (a text node paints its box, then its
    // glyphs), so walk every entry it owns, in order.
    for (_, kind) in paint.iter().filter(|(nid, _)| *nid == id) {
        match kind {
            PaintKind::Box {
                bg,
                radius,
                border_width,
                border_color,
                clip: c,
            } => {
                clip = *c;
                clip_radius = *radius;
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
            PaintKind::Image(ic) => out.paints.push(Paint::Image(PaintImage {
                x,
                y,
                width: layout.size.width,
                height: layout.size.height,
                content: ic.clone(),
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

    if let Some((_, model)) = models.iter().find(|(nid, _)| *nid == id) {
        out.focuses.push(FocusRegion {
            x,
            y,
            width: layout.size.width,
            height: layout.size.height,
            model: model.clone(),
        });
    }

    // overflow: clip — bound the subtree to this box (following its corners).
    if clip {
        out.paints.push(Paint::PushClip {
            x,
            y,
            width: layout.size.width,
            height: layout.size.height,
            radius: clip_radius,
        });
    }
    for child in tree.children(id).expect("children") {
        collect(
            tree, child, x, y, paint, handlers, models, hidden, opacities, vp, out,
        );
    }
    if clip {
        out.paints.push(Paint::PopClip);
    }
    if alpha < 1.0 {
        out.paints.push(Paint::PopOpacity);
    }
}

/// Lay out `root` into an `avail_w` x `avail_h` viewport, returning paint items
/// and hit regions. Text leaves are sized via `measure`.
pub fn layout(root: &Node, avail_w: f32, avail_h: f32, measure: &mut Measure) -> Layout {
    let mut tree: TaffyTree<TextContent> = TaffyTree::new();
    // Taffy rounds boxes to whole pixels by default, which can shave a fraction
    // off a text box and make paint re-wrap the last word into a line the box
    // has no height for. Keep the exact sizes measure asked for.
    tree.disable_rounding();
    let mut paint = Vec::new();
    let mut handlers = Vec::new();
    let mut models = Vec::new();
    let mut hidden = Vec::new();
    let mut opacities = Vec::new();
    let vp = (avail_w, avail_h);
    let root_id = build(
        &mut tree,
        root,
        &mut paint,
        &mut handlers,
        &mut models,
        &mut hidden,
        &mut opacities,
        vp,
    );

    // Force the root to fill the viewport so a `screen` always covers the window.
    let mut root_style = to_taffy(&root.style, vp);
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
                    // Wrap to a definite width; otherwise (content sizing) let
                    // the text take its natural single-line width.
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
    collect(
        &tree, root_id, 0.0, 0.0, &paint, &handlers, &models, &hidden, &opacities, vp, &mut out,
    );
    out
}
