//! Rux layout, milestones M1–M4.
//!
//! A styled node tree fed through `taffy` (flexbox) to produce absolute paint
//! items. Boxes come straight from taffy; text leaves are sized through a
//! caller-supplied `measure` callback (so this crate stays free of any font
//! dependency, the shell owns the text engine). See `docs/04-architecture.md`,
//! Stage 4.
//!
//! The crate is mostly its vocabulary. [`Style`] is the honored subset of CSS
//! as the engine actually sees it, and [`Node`] is one styled element with its
//! children. `rux-style` produces that tree; this crate turns it into the flat
//! [`Paint`] list `rux-paint` consumes, in absolute coordinates with the
//! cascade and the box model already collapsed into numbers.
//!
//! Layout emits more than pictures, because a frame's geometry is the only
//! place several other questions can be answered honestly. Alongside the paint
//! items come the regions the shell needs and cannot recompute for itself:
//! [`HitRegion`] for pointer targets, [`ScrollRegion`] for what scrolls and how
//! far, [`FocusRegion`] and [`FocusItem`] for tab order, [`SelectRegion`] for
//! selectable text, [`StateRegion`] for hover and active, and [`AccessNode`]
//! for the accessibility tree. All of them are in the same coordinate space as
//! the paint items, which is the point: two places doing the same coordinate
//! arithmetic eventually disagree, so there is one conversion and everyone
//! reads its output.
//!
//! Both flexbox and grid come from taffy. What does not come from taffy is
//! text sizing, which is why `measure` is a callback: pulling a font
//! stack into this crate would put shaping under layout, and the shell already
//! owns one.

pub mod path;
pub use path::{PathCmd, PathContent};

use std::collections::HashMap;

use taffy::prelude::*;
use taffy::geometry::Point;

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
    /// `minmax(min, max)`. Its whole point over a bare `1fr` is a `0` (or `px`)
    /// minimum, which lets the track shrink *below* its content's min-content,
    /// so a grid of fixed-size cards squeezes to fit instead of overflowing.
    MinMax(TrackSide, TrackSide),
}

/// One side of a `minmax()`, never itself a `minmax`. A `Fr` is only valid on
/// the max side (a flex minimum is meaningless), and degrades to `auto` if used
/// as a minimum.
#[derive(Clone, Copy, Debug)]
pub enum TrackSide {
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

/// How a line may break when a word is wider than its box (`overflow-wrap` /
/// `word-break`). CSS's default lets a long word overflow rather than break.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum TextWrap {
    #[default]
    Normal,
    /// `overflow-wrap: break-word`: break inside a word rather than overflow.
    BreakWord,
    /// `word-break: break-all`: break anywhere.
    Anywhere,
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
    /// Clip the subtree to this box (`hidden` / `clip`).
    Clip,
    /// Clip, and let the wheel move the content (`auto` / `scroll`). The box
    /// keeps its own size; taffy reports how tall the content actually is.
    Scroll,
}

/// The mouse cursor shown while the pointer is over a box (`cursor`). Only the
/// values the shell maps to a winit `CursorIcon` are modelled; the default is
/// the arrow.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Cursor {
    #[default]
    Default,
    /// `cursor: pointer`: the hand, for tappable things.
    Pointer,
}

/// `position`, with CSS's meanings.
///
/// `Static` is the default and the only value that is **not** a containing
/// block: it is the normal in-flow box, and its `inset` is ignored. `Relative`
/// is in flow too but offset by its inset, and it *is* a containing block.
/// `Sticky` is in flow like `relative`, but its inset is a **threshold** rather
/// than an offset: it stays where it was laid out until its scroller reaches
/// that edge, then rides along it, and stops again at the end of its parent.
/// `Absolute` is out of flow, positioned by its inset against the nearest
/// non-static ancestor. `Fixed` is out of flow against the window, and does not
/// move when an ancestor scrolls.
///
/// **The default used to be `Relative`**, which made every box a containing
/// block, which in turn made "against the nearest positioned ancestor" and
/// "against the parent" the same sentence. They are not the same sentence, and
/// an author who wrote `position: relative` on the box they meant, as CSS
/// requires, was being ignored and getting the right answer anyway. Only a
/// wrapper in between revealed it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Position {
    #[default]
    Static,
    Relative,
    Sticky,
    Absolute,
    Fixed,
}

impl Position {
    /// Whether a box with this `position` is a containing block, so an
    /// out-of-flow descendant's insets are measured against it.
    ///
    /// Not the whole rule: a box with a `transform` is a containing block too,
    /// whatever its `position`, and for `fixed` descendants as well as absolute
    /// ones. See the hoisting in `build`.
    pub fn is_containing_block(self) -> bool {
        self != Position::Static
    }

    /// Whether the box is taken out of the flow.
    pub fn is_out_of_flow(self) -> bool {
        matches!(self, Position::Absolute | Position::Fixed)
    }

    /// Whether the box is pinned against its scroller at paint time.
    pub fn is_sticky(self) -> bool {
        self == Position::Sticky
    }
}

/// Corner radii in CSS order, top-left, top-right, bottom-right, bottom-left.
/// A single `border-radius` fills all four; the per-corner longhands override.
pub type Corners = [f32; 4];

/// A 2-D affine `transform`, as the six coefficients `[a, b, c, d, e, f]` (kurbo
/// `Affine` order: `x' = a·x + c·y + e`, `y' = b·x + d·y + f`). Translations are
/// in logical px; the origin is applied at paint time (CSS default: box centre).
pub type Transform = [f32; 6];

/// One `transition` entry: which property animates when it changes, for how
/// long, after what delay, and on which curve.
///
/// The style itself only carries the *declaration*. Nothing here knows what the
/// property's value was a frame ago; that memory belongs to whoever is driving
/// the clock, since it is per-frame state and the style is rebuilt from scratch
/// on every build.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transition {
    pub property: AnimProp,
    /// Milliseconds. A duration of `0` means the change lands immediately, which
    /// is how CSS disables a transition without removing the declaration.
    pub duration: f32,
    pub delay: f32,
    pub easing: Easing,
}

/// A property that can be transitioned.
///
/// One variant per *field* of [`Style`], not per CSS longhand: `padding` is a
/// [`Sides`] and animates as a unit, so `transition: padding-left` is rejected
/// with a diagnostic rather than quietly animating all four sides. The set is
/// deliberately small; everything in it is a value that means something when
/// interpolated halfway.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AnimProp {
    /// `all`: stands for every other variant whose value actually changed.
    All,
    Opacity,
    BackgroundColor,
    /// Text colour, which lives on the node's own text content.
    Color,
    BorderColor,
    BorderWidth,
    BorderRadius,
    Width,
    Height,
    Padding,
    Margin,
    Gap,
    FontSize,
    Transform,
    /// `top`/`right`/`bottom`/`left` as a unit.
    Inset,
    /// `<path>` paint: the colour inside, the colour of the outline, and how
    /// thick the outline is.
    Fill,
    Stroke,
    StrokeWidth,
    /// `d`: the path's geometry itself.
    ///
    /// The one animatable property that is not a style property, because
    /// geometry is an attribute. It is here anyway, and reached through the
    /// node rather than through the style, because an author asking for a
    /// shape to morph is asking for exactly what `transition` means everywhere
    /// else and should not have to learn a second word for it.
    PathData,
}

impl AnimProp {
    /// Every property `all` expands to, in a fixed order so a frame's work is
    /// deterministic.
    pub const EVERY: &'static [AnimProp] = &[
        AnimProp::Opacity,
        AnimProp::BackgroundColor,
        AnimProp::Color,
        AnimProp::BorderColor,
        AnimProp::BorderWidth,
        AnimProp::BorderRadius,
        AnimProp::Width,
        AnimProp::Height,
        AnimProp::Padding,
        AnimProp::Margin,
        AnimProp::Gap,
        AnimProp::FontSize,
        AnimProp::Transform,
        AnimProp::Inset,
        AnimProp::Fill,
        AnimProp::Stroke,
        AnimProp::StrokeWidth,
        AnimProp::PathData,
    ];

    /// The CSS name, for diagnostics.
    pub fn name(self) -> &'static str {
        match self {
            AnimProp::All => "all",
            AnimProp::Opacity => "opacity",
            AnimProp::BackgroundColor => "background-color",
            AnimProp::Color => "color",
            AnimProp::BorderColor => "border-color",
            AnimProp::BorderWidth => "border-width",
            AnimProp::BorderRadius => "border-radius",
            AnimProp::Width => "width",
            AnimProp::Height => "height",
            AnimProp::Padding => "padding",
            AnimProp::Margin => "margin",
            AnimProp::Gap => "gap",
            AnimProp::FontSize => "font-size",
            AnimProp::Transform => "transform",
            AnimProp::Inset => "inset",
            AnimProp::Fill => "fill",
            AnimProp::Stroke => "stroke",
            AnimProp::StrokeWidth => "stroke-width",
            AnimProp::PathData => "d",
        }
    }
}

/// A timing function. The named curves are the CSS keywords, and all of them are
/// the same cubic Bézier underneath, so there is one evaluator to be wrong in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Easing {
    Linear,
    /// `cubic-bezier(x1, y1, x2, y2)`; the keywords resolve to these.
    Bezier(f32, f32, f32, f32),
}

impl Easing {
    pub const EASE: Easing = Easing::Bezier(0.25, 0.1, 0.25, 1.0);
    pub const EASE_IN: Easing = Easing::Bezier(0.42, 0.0, 1.0, 1.0);
    pub const EASE_OUT: Easing = Easing::Bezier(0.0, 0.0, 0.58, 1.0);
    pub const EASE_IN_OUT: Easing = Easing::Bezier(0.42, 0.0, 0.58, 1.0);

    /// Map linear progress `t` (0..=1) to eased progress.
    ///
    /// The curve is parametric in a third variable: `x` and `y` are both cubics
    /// in `s`, and `t` is an `x`. So invert `x(s) = t` first (Newton, falling
    /// back to bisection when the derivative goes flat), then read `y(s)`.
    pub fn eval(self, t: f32) -> f32 {
        let (x1, y1, x2, y2) = match self {
            Easing::Linear => return t,
            Easing::Bezier(x1, y1, x2, y2) => (x1, y1, x2, y2),
        };
        if t <= 0.0 || t >= 1.0 {
            return t;
        }
        let s = solve_bezier_x(t, x1, x2);
        bezier(s, y1, y2)
    }
}

/// A unit cubic Bézier's coordinate at parameter `s`, with the endpoints pinned
/// at 0 and 1: `3(1-s)²s·p1 + 3(1-s)s²·p2 + s³`.
fn bezier(s: f32, p1: f32, p2: f32) -> f32 {
    let u = 1.0 - s;
    3.0 * u * u * s * p1 + 3.0 * u * s * s * p2 + s * s * s
}

/// Derivative of [`bezier`] with respect to `s`.
fn bezier_slope(s: f32, p1: f32, p2: f32) -> f32 {
    let u = 1.0 - s;
    3.0 * u * u * p1 + 6.0 * u * s * (p2 - p1) + 3.0 * s * s * (1.0 - p2)
}

/// Find `s` such that `x(s) == t`.
fn solve_bezier_x(t: f32, x1: f32, x2: f32) -> f32 {
    let mut s = t; // x is near-linear for the usual curves, so t is a good start
    for _ in 0..8 {
        let err = bezier(s, x1, x2) - t;
        if err.abs() < 1e-5 {
            return s;
        }
        let slope = bezier_slope(s, x1, x2);
        if slope.abs() < 1e-5 {
            break; // flat: Newton would step off to nowhere
        }
        s -= err / slope;
    }
    // Bisection, which cannot diverge, for the curves Newton gives up on.
    let (mut lo, mut hi) = (0.0f32, 1.0f32);
    let mut s = t.clamp(0.0, 1.0);
    for _ in 0..20 {
        let x = bezier(s, x1, x2);
        if (x - t).abs() < 1e-5 {
            break;
        }
        if x > t {
            hi = s;
        } else {
            lo = s;
        }
        s = (lo + hi) / 2.0;
    }
    s
}

/// `grid-auto-flow`: how auto-placed items fill the implicit grid.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum GridFlow {
    #[default]
    Row,
    Column,
    RowDense,
    ColumnDense,
}

/// One endpoint of a `grid-column` / `grid-row` placement.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum GridPlace {
    /// Auto-placed by the grid algorithm.
    #[default]
    Auto,
    /// A specific grid line (1-based; negative counts back from the end).
    Line(i16),
    /// Span this many tracks from the other endpoint.
    Span(u16),
}

/// A box background: a flat colour, a gradient, or an image.
#[derive(Clone, Debug)]
pub enum Background {
    Color(Rgba),
    Gradient(Gradient),
    /// `background-image: url(…)`. The runtime resolves this to an absolute path
    /// (like `<image src>`); the painter decodes it and draws it `cover`-sized.
    Image(String),
}

/// A CSS gradient reduced to what the painter needs: a shape and colour stops.
#[derive(Clone, Debug)]
pub struct Gradient {
    pub kind: GradientKind,
    /// Colour stops as `(colour, offset)` with offset in 0..=1, in order.
    pub stops: Vec<(Rgba, f32)>,
}

#[derive(Clone, Copy, Debug)]
pub enum GradientKind {
    /// `linear-gradient(<angle>, …)`: angle in radians, CSS convention (0 = to
    /// top, increasing clockwise).
    Linear { angle: f32 },
    /// `radial-gradient(…)`: a centred circle out to the nearest edge.
    Radial,
}

/// A single (outer) `box-shadow`. Offsets, blur and spread are logical px.
#[derive(Clone, Copy, Debug)]
pub struct BoxShadow {
    pub dx: f32,
    pub dy: f32,
    pub blur: f32,
    pub spread: f32,
    pub color: Rgba,
    /// `inset` shadows are parsed but not yet drawn.
    pub inset: bool,
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
    /// `grid-column` / `grid-row` placement for a grid item: `(start, end)`.
    pub grid_column: (GridPlace, GridPlace),
    pub grid_row: (GridPlace, GridPlace),
    /// `grid-auto-flow` and the implicit-track sizes `grid-auto-rows`/`-columns`.
    pub grid_auto_flow: GridFlow,
    pub grid_auto_rows: Vec<Track>,
    pub grid_auto_columns: Vec<Track>,
    pub grow: f32,
    /// `flex-shrink`. CSS defaults to 1: a flex item gives up space to fit its
    /// container. `0` keeps the item's size and lets it overflow, which is the
    /// author's call, and what `overflow: clip` is for.
    pub shrink: f32,
    /// `flex-basis`. `None` = `auto` (size from width/content).
    pub basis: Option<Len>,
    /// `flex-wrap: wrap`: items that don't fit start a new line.
    pub wrap: bool,
    /// `opacity`, 0.0–1.0. Applies to the whole subtree.
    pub opacity: f32,
    /// `overflow-wrap` / `word-break`, applied to a text node's own content.
    pub text_wrap: TextWrap,
    pub padding: Sides,
    pub margin: Sides,
    pub border: Sides,
    pub border_color: Option<Rgba>,
    pub gap: f32,
    /// `row-gap` / `column-gap` overrides for the shorthand `gap`. `None` keeps
    /// the shorthand (`gap`) value on that axis.
    pub row_gap: Option<f32>,
    pub column_gap: Option<f32>,
    pub axis: Axis,
    pub justify: Option<Justify>,
    pub align: Option<Align>,
    /// `align-self` (flex/grid cross-axis) and `justify-self` (grid inline-axis)
    /// for this item, overriding the parent's `align-items`/`justify-items`.
    pub align_self: Option<Align>,
    pub justify_self: Option<Align>,
    /// `justify-items` (grid) and `align-content` (multi-line flex / grid).
    pub justify_items: Option<Align>,
    pub align_content: Option<Justify>,
    pub overflow: Overflow,
    pub background: Option<Background>,
    /// `border-radius`, per corner (top-left, top-right, bottom-right, bottom-left).
    pub radius: Corners,
    /// `box-shadow` (single, outer). Drawn behind the box's own background.
    pub box_shadow: Option<BoxShadow>,
    /// `transform`: an affine applied to this box and its subtree at paint time.
    /// Visual only: hit regions are not transformed.
    pub transform: Option<Transform>,
    /// `cursor`: the pointer shape over this box.
    pub cursor: Cursor,
    /// `position` and its `inset` (top, right, bottom, left). `None` per side =
    /// `auto`. Only meaningful when `position: absolute`.
    pub position: Position,
    pub inset: [Option<Len>; 4],
    /// `aspect-ratio` (width / height).
    pub aspect_ratio: Option<f32>,
    /// `transition`: which of this node's properties animate when they change,
    /// and how. Empty for almost every node, and free when it is: the animator
    /// only remembers nodes that declare one.
    pub transitions: Vec<Transition>,
    /// How far through an enter/leave swap this element is, when something
    /// other than the clock is driving it (a finger, normally).
    ///
    /// It rides on the node rather than being looked up because the animator
    /// identifies nodes by key-path and swaps are identified by template path;
    /// carrying the value with the element it belongs to avoids having to keep
    /// those two agreeing.
    pub swap_progress: Option<f32>,
    /// `fill`: the colour inside a `<path>`. `None` is `fill: none`.
    ///
    /// It defaults to opaque black rather than to nothing, which is SVG's rule
    /// and is the right first-five-minutes behaviour: a `<path>` written with
    /// geometry and no paint at all draws, instead of leaving an author staring
    /// at an empty box wondering which of the two they got wrong.
    pub fill: Option<Rgba>,
    /// `stroke`: the colour of the outline. `None` is no outline, which is the
    /// default, again as SVG has it.
    pub stroke: Option<Rgba>,
    /// `stroke-width`, in px.
    pub stroke_width: f32,
    /// `stroke-linecap` / `stroke-linejoin`: how an open end and a corner are
    /// finished.
    pub stroke_linecap: LineCap,
    pub stroke_linejoin: LineJoin,
    /// `fill-rule`: how a self-overlapping path decides what is inside.
    pub fill_rule: FillRule,
}

/// How the open end of a stroke is finished.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum LineCap {
    #[default]
    Butt,
    Round,
    Square,
}

/// How a corner between two stroked segments is finished.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum LineJoin {
    #[default]
    Miter,
    Round,
    Bevel,
}

/// How a path that overlaps itself decides what counts as inside.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum FillRule {
    #[default]
    NonZero,
    EvenOdd,
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
            grid_column: (GridPlace::Auto, GridPlace::Auto),
            grid_row: (GridPlace::Auto, GridPlace::Auto),
            grid_auto_flow: GridFlow::Row,
            grid_auto_rows: Vec::new(),
            grid_auto_columns: Vec::new(),
            grow: 0.0,
            shrink: 1.0,
            basis: None,
            wrap: false,
            opacity: 1.0,
            text_wrap: TextWrap::Normal,
            padding: Sides::default(),
            margin: Sides::default(),
            border: Sides::default(),
            border_color: None,
            gap: 0.0,
            row_gap: None,
            column_gap: None,
            axis: Axis::Row,
            justify: None,
            align: None,
            align_self: None,
            justify_self: None,
            justify_items: None,
            align_content: None,
            overflow: Overflow::Visible,
            background: None,
            radius: [0.0; 4],
            box_shadow: None,
            transform: None,
            cursor: Cursor::Default,
            position: Position::Static,
            inset: [None; 4],
            aspect_ratio: None,
            transitions: Vec::new(),
            swap_progress: None,
            fill: Some(Rgba::new(0.0, 0.0, 0.0, 1.0)),
            stroke: None,
            stroke_width: 1.0,
            stroke_linecap: LineCap::Butt,
            stroke_linejoin: LineJoin::Miter,
            fill_rule: FillRule::NonZero,
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
    pub wrap: TextWrap,
    /// `font-family` as a raw CSS list (e.g. `"Inter, sans-serif"`). `None` uses
    /// the system default. Inherits, like `color` and `font-size`.
    pub font_family: Option<String>,
    /// `letter-spacing` / `word-spacing`, extra px between letters / words.
    pub letter_spacing: Option<f32>,
    pub word_spacing: Option<f32>,
    /// `line-height` as an absolute pixel value; `None` uses the font metrics.
    pub line_height: Option<f32>,
    /// `font-style: italic`.
    pub italic: bool,
    /// `text-decoration: underline` / `line-through`.
    pub underline: bool,
    pub strikethrough: bool,
    /// `white-space: nowrap`: never wrap, even past the box width.
    pub nowrap: bool,
    /// Byte index of the caret, when this text is inside the focused input.
    pub caret: Option<usize>,
    /// The selected byte range (start < end, normalized), when this text is
    /// inside the focused input and its selection isn't collapsed. The painter
    /// highlights it behind the glyphs.
    pub selection: Option<(usize, usize)>,
    /// The byte range holding an in-progress IME composition, when this text is
    /// inside the focused input and something is being composed. The painter
    /// underlines it.
    ///
    /// The composed text is already inside `text`: the shell writes it into the
    /// bound signal as it is typed, exactly as a browser does to an `<input>`'s
    /// value during composition. This range only says which part of it is not
    /// committed yet, so it can be drawn as provisional rather than as text the
    /// author typed and meant.
    pub preedit: Option<(usize, usize)>,
}

/// What an element *is*, for assistive technology. Deliberately a small enum
/// owned by the layout rather than an `accesskit` type: the layout stays free of
/// the platform a11y crate, and only the shell translates these.
///
/// Resolved during the build, where the tag, the `type=` and the `role=`
/// attribute are all still in hand, deriving it later from painted output would
/// be guesswork.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum AccessRole {
    /// Not interesting to a screen reader on its own (a plain layout box).
    #[default]
    None,
    /// Static text.
    Label,
    Heading,
    Button,
    CheckBox,
    RadioButton,
    TextInput,
    /// `type="textarea"`.
    MultilineTextInput,
    /// `type="select"`.
    ComboBox,
    Image,
    /// A navigation target: `to="/path"`, or an explicit `role="link"`. Distinct
    /// from a button because a screen reader announces it differently, and
    /// because the distinction is what tells someone they are moving rather than
    /// acting.
    Link,
    /// A box that scrolls its content.
    ScrollView,
    /// A meaningful grouping (an explicit `role=` we don't map more precisely).
    Group,
}

impl AccessRole {
    /// Does this element carry meaning worth exposing at all?
    pub fn is_meaningful(self) -> bool {
        self != Self::None
    }
}

/// The accessibility facts about one node: what it is, what it's called, and what
/// state it's in. Attached during the build and carried through layout so the
/// shell can publish a tree with real geometry.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Access {
    pub role: AccessRole,
    /// The accessible *name*, what a screen reader announces. For a control this
    /// is its label, not its value.
    pub label: Option<String>,
    /// An input's placeholder. Kept apart from `label` because it is only a
    /// *fallback* name: a real label (authored `label=`, or a `<text for="…">`)
    /// must win, and labels are linked after the build, so baking the placeholder
    /// into `label` would let a hint outrank the actual label.
    pub placeholder: Option<String>,
    /// Current value, for inputs and selects.
    pub value: Option<String>,
    /// Checked state, for checkboxes and radios.
    pub checked: Option<bool>,
}

impl Access {
    /// What to announce as this element's name: its label, else its placeholder.
    pub fn name(&self) -> Option<&str> {
        self.label.as_deref().or(self.placeholder.as_deref())
    }
}

/// A node in the view tree: a style, optional text, children, and an optional
/// `@tap` handler (raw handler source, run by the shell on tap).
#[derive(Clone, Debug)]
pub struct Node {
    pub style: Style,
    pub text: Option<TextContent>,
    /// `<image src=…>`.
    pub image: Option<ImageContent>,
    /// `<path d=…>`: vector geometry in the element's own box, drawn with the
    /// `fill` and `stroke` its style resolved to.
    pub path: Option<PathContent>,
    /// A checkmark stroked to fill this box, in the given colour. Drawn as a
    /// path rather than a font glyph, since ✓ is whatever the system font happens to
    /// ship, which is not a control mark.
    pub tick: Option<Rgba>,
    pub children: Vec<Node>,
    pub on_tap: Option<String>,
    /// `@press`, `@release`, `@longpress`, `@swipe`, `@drag`. Empty for almost
    /// every node, so a `Vec` rather than five more `Option<String>` fields.
    pub gestures: Vec<(Gesture, String)>,
    /// `r-model` signal name for `<input>` nodes (focus target + edit binding).
    pub model: Option<String>,
    /// `type="textarea"`: a multi-line text input, `Enter` inserts a newline.
    pub multiline: bool,
    /// `type="select"`: the bound `:options`, so the shell can open a dropdown.
    pub options: Option<Vec<String>>,
    /// `r-show="false"`: laid out (space reserved) but not painted.
    pub hidden: bool,
    /// `id="…"`: a stable identifier a label's `for=` can target.
    pub id: Option<String>,
    /// `for="…"` on a label, the `id` of the input it labels. Resolved at build
    /// time (the label inherits its target's `@tap`), so tapping the label toggles
    /// the target the same way tapping the target would.
    pub label_for: Option<String>,
    /// A label whose `for=` targets a *text* input: the target input's `r-model`.
    /// The layout emits a `FocusRegion` here so tapping the label focuses that input
    /// (the caret lands in the input itself, matched by model).
    pub focus_model: Option<String>,
    /// This node's tree path, set only when some `:hover`/`:active` rule could
    /// match it. The layout emits a [`StateRegion`] for such nodes so the shell can
    /// tell what the pointer is over and hand the path back as interaction state.
    /// `None`: the common case, costs nothing.
    pub state_path: Option<Vec<usize>>,
    /// What this element is, for assistive technology.
    pub access: Access,
    /// Which component instance this node belongs to, when it is inside one.
    ///
    /// A component's own state is private to the instance, so a handler written
    /// in a component has to say which instance it is running in: two `<panel>`
    /// elements are two separate sets of state, and the handler text is
    /// identical in both.
    pub instance: Option<String>,
    /// `r-key` on an `r-for` row: which *item* this node stands for, rather than
    /// which slot it happens to occupy.
    ///
    /// Without it a list is identified by position, so reordering the data moves
    /// every row's identity by one and anything attached to a row (the caret,
    /// most visibly) stays behind with the slot. The runtime uses this to follow
    /// a row across a reorder. Layout itself ignores it.
    pub key: Option<String>,
}

impl Node {
    pub fn new(style: Style) -> Self {
        Self {
            style,
            text: None,
            image: None,
            path: None,
            tick: None,
            children: Vec::new(),
            on_tap: None,
            gestures: Vec::new(),
            model: None,
            multiline: false,
            options: None,
            hidden: false,
            id: None,
            label_for: None,
            focus_model: None,
            state_path: None,
            access: Access::default(),
            instance: None,
            key: None,
        }
    }

    pub fn text(style: Style, text: TextContent) -> Self {
        Self {
            style,
            text: Some(text),
            image: None,
            path: None,
            tick: None,
            children: Vec::new(),
            on_tap: None,
            gestures: Vec::new(),
            model: None,
            multiline: false,
            options: None,
            hidden: false,
            id: None,
            label_for: None,
            focus_model: None,
            state_path: None,
            access: Access::default(),
            instance: None,
            key: None,
        }
    }

    pub fn image(style: Style, image: ImageContent) -> Self {
        Self {
            style,
            text: None,
            image: Some(image),
            path: None,
            tick: None,
            children: Vec::new(),
            on_tap: None,
            gestures: Vec::new(),
            model: None,
            multiline: false,
            options: None,
            hidden: false,
            id: None,
            label_for: None,
            focus_model: None,
            state_path: None,
            access: Access::default(),
            instance: None,
            key: None,
        }
    }

    /// A `<path>` leaf.
    ///
    /// Its geometry is in the element's own coordinates, so the box CSS gives
    /// it is what the drawing sits in. Nothing about the path changes the box:
    /// see the note on `PaintKind::Path` for why that is the whole design.
    pub fn path(style: Style, path: PathContent) -> Self {
        Self {
            style,
            text: None,
            image: None,
            path: Some(path),
            tick: None,
            children: Vec::new(),
            on_tap: None,
            gestures: Vec::new(),
            model: None,
            multiline: false,
            options: None,
            hidden: false,
            id: None,
            label_for: None,
            focus_model: None,
            state_path: None,
            access: Access::default(),
            instance: None,
            key: None,
        }
    }

    pub fn with(mut self, child: Node) -> Self {
        self.children.push(child);
        self
    }
}

/// A resolved, absolutely-positioned box: an optional fill and an optional
/// border, sharing one rounded-rect geometry.
#[derive(Clone, Debug)]
pub struct PaintRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub background: Option<Background>,
    pub radius: Corners,
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

/// A checkmark stroked inside its laid-out box.
#[derive(Clone, Copy, Debug)]
pub struct PaintTick {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub color: Rgba,
}

/// Vector geometry, positioned at its laid-out box's content corner.
#[derive(Clone, Debug)]
pub struct PaintPath {
    pub x: f32,
    pub y: f32,
    pub content: PathContent,
    pub paint: PathPaint,
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
    Path(PaintPath),
    Tick(PaintTick),
    /// A blurred `box-shadow`, drawn behind its box. Geometry already has the
    /// offset and spread applied.
    Shadow {
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        radius: f32,
        blur: f32,
        color: Rgba,
    },
    /// Begin clipping subsequent items to this rounded rect (overflow: clip).
    PushClip {
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        radius: Corners,
    },
    /// End the most recent clip.
    PopClip,
    /// Begin an affine `transform` on the subtree. The matrix already has the
    /// transform-origin baked in, so it applies directly to absolute coords.
    PushTransform(Transform),
    /// End the most recent transform.
    PopTransform,
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

/// How far a scroller's content has travelled, in logical pixels. Positive
/// moves the content up / left, i.e. `y` is "how far down the content we are".
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Offset {
    pub x: f32,
    pub y: f32,
}

/// Which scroller a box belongs to, for `scrollIntoView`.
///
/// **By content, not by what is currently on screen.** The obvious test is
/// whether the scroller's visible rectangle contains the box, and it is wrong in
/// exactly the case the call exists for: an element scrolled past the bottom
/// sits *outside* that rectangle, so no scroller claims it and nothing moves.
/// A message list that reveals its newest row silently did nothing, and the only
/// reveals that worked were the ones already a nudge away from being visible.
///
/// So the comparison is against the scroller's content extent, which is where
/// its children actually are. `x`/`y` on a region are already shifted by the
/// current offset, so the content starts one offset *above* the visible edge.
///
/// The innermost match wins, `scrolls` being in tree order, which is the one
/// whose offset moves this box.
pub fn containing_scroller(
    scrolls: &[ScrollRegion],
    offsets: &[Offset],
    x: f32,
    y: f32,
) -> Option<usize> {
    scrolls.iter().rposition(|s| {
        let off = offsets.get(s.id).copied().unwrap_or_default();
        let (cx, cy) = (s.x - off.x, s.y - off.y);
        x >= cx && y >= cy && x <= cx + s.content_width && y <= cy + s.content_height
    })
}

impl Offset {
    pub fn clamp_to(self, max: Offset) -> Offset {
        Offset {
            x: self.x.clamp(0.0, max.x),
            y: self.y.clamp(0.0, max.y),
        }
    }
}

/// A scrollable box. `id` is its index in tree order, stable across rebuilds
/// as long as the tree's shape is, which is what the shell keys offsets by.
#[derive(Clone, Debug)]
pub struct ScrollRegion {
    /// The accumulated `transform` in force where this box sits, and the
    /// accumulated `opacity`.
    ///
    /// Scrollbars and focus rings are drawn *outside* the paint list, over the
    /// content, because a scroller clips its own children and would eat them.
    /// That means they do not inherit the transform and opacity stack the paint
    /// list carries, and until these were recorded they did not follow at all:
    /// a page transitioning at `opacity: 0` still showed its scrollbar at full
    /// strength, sitting over whatever it was transitioning away from.
    pub transform: Option<Transform>,
    pub alpha: f32,
    pub id: usize,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    /// The size of the content inside, which may exceed the box on either axis.
    pub content_width: f32,
    pub content_height: f32,
    /// How far the content can travel on each axis: content - visible (>= 0).
    pub max: Offset,
}

impl ScrollRegion {
    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px <= self.x + self.width && py >= self.y && py <= self.y + self.height
    }

    /// Whether this box scrolls at all on either axis.
    pub fn scrollable(&self) -> bool {
        self.max.x > 0.0 || self.max.y > 0.0
    }
}

/// One thing a finger can do to an element, beyond tapping it.
///
/// `@tap` stays separate because it is not a pointer event: it is the finished
/// gesture, it is what a keyboard activation produces, and it is what `tap()`
/// from script synthesises. Everything here is raw pointer traffic.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Gesture {
    /// A finger or button went down on this element.
    Press,
    /// It came up again, whether or not it stayed still enough to be a tap.
    Release,
    /// It stayed down, and still, for long enough.
    LongPress,
    /// It travelled far enough in one direction and left. Discrete: it reports
    /// once, at the end, with a direction.
    Swipe,
    /// It is moving with the button or finger down. Reports at the start, on
    /// every move, and at the end.
    Drag,
}

impl Gesture {
    /// The attribute that declares it, without the `@`.
    pub fn from_attr(name: &str) -> Option<Self> {
        match name {
            "press" => Some(Gesture::Press),
            "release" => Some(Gesture::Release),
            "longpress" => Some(Gesture::LongPress),
            "swipe" => Some(Gesture::Swipe),
            "drag" => Some(Gesture::Drag),
            _ => None,
        }
    }
}

/// An absolutely-positioned tappable region, carrying its `@tap` handler source.
#[derive(Clone, Debug)]
pub struct HitRegion {
    /// The `transform` in force where this box sits.
    ///
    /// The rect itself is untransformed, because that is what the layout
    /// produced. Testing a point against it without undoing the transform means
    /// a tap on a sliding element lands where the element *used* to be, which
    /// is a correctness bug rather than a cosmetic one: mid-transition the
    /// wrong thing responds, or nothing does.
    pub transform: Option<Transform>,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    /// The `@tap` body, if the element has one. A region may exist without it:
    /// an element with only `@drag` still has to be found by a hit test.
    pub on_tap: Option<String>,
    /// The pointer handlers this element declared, by gesture.
    pub gestures: Vec<(Gesture, String)>,
    /// The `cursor` for this region, so the shell can set the pointer shape when
    /// it hovers here. Carried on the hit region because that is the geometry the
    /// shell already hit-tests; a `cursor` on a non-tappable box is not honored.
    pub cursor: Cursor,
    /// The component instance this handler was written in, if any. Its state is
    /// what the handler reads and writes, and two instances of one component
    /// carry identical handler text, so the text alone cannot say which.
    pub instance: Option<String>,
}

impl HitRegion {
    pub fn contains(&self, px: f32, py: f32) -> bool {
        // Undo the transform on the *point* rather than transforming the rect:
        // a rotated or skewed box is no longer a rectangle, and the inverse
        // point test stays correct for every affine.
        let (px, py) = match self.transform {
            Some(m) => match invert(m) {
                Some(inv) => apply(inv, px, py),
                // Collapsed to nothing: there is no area left to hit.
                None => return false,
            },
            None => (px, py),
        };
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
    /// The `r-key` of the `r-for` row this input sits in, when it sits in one.
    ///
    /// `model` alone does not identify an input: `r-model` is stored as written,
    /// so every row of a list carries the *same* model text. Without this, two
    /// inputs in one list are indistinguishable and the caret lands in the first
    /// of them whichever one was tapped.
    pub row: Option<String>,
    /// The input's text box (its laid-out child). The shell needs it to turn a
    /// click into a caret position.
    pub text: Option<PaintText>,
    /// `type="textarea"`: `Enter` inserts a newline instead of being ignored.
    pub multiline: bool,
    /// If this input scrolls (a textarea), the index of its `ScrollRegion` in
    /// `Layout.scrolls`, so the shell can scroll the caret into view.
    pub scroll_id: Option<usize>,
}

impl FocusRegion {
    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px <= self.x + self.width && py >= self.y && py <= self.y + self.height
    }
}

/// An absolutely-positioned `type="select"`, carrying its bound options so the
/// shell can open a dropdown and write the chosen value back to `model`.
#[derive(Clone, Debug)]
pub struct SelectRegion {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub model: String,
    /// The `r-key` of the `r-for` row this select is in, when it is in one. The
    /// model repeats across a list's rows, so without this the shell opens the
    /// first row's dropdown wherever you tapped, draws it over that row, and
    /// writes the chosen option into it.
    pub row: Option<String>,
    pub options: Vec<String>,
}

impl SelectRegion {
    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px <= self.x + self.width && py >= self.y && py <= self.y + self.height
    }
}

/// An absolutely-positioned box whose styling depends on pointer state
/// (`:hover` / `:active`), carrying the tree path that identifies it to the
/// builder. Emitted only for nodes some pointer-state rule could match, so a
/// document with no such rules produces none.
#[derive(Clone, Debug)]
pub struct StateRegion {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    /// The node's child-index path from the root, the same identity the binding
    /// registry uses.
    pub path: Vec<usize>,
}

impl StateRegion {
    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px <= self.x + self.width && py >= self.y && py <= self.y + self.height
    }
}

/// One element exposed to assistive technology, with the geometry it ended up
/// occupying. Emitted in document order, and only for nodes whose role is
/// meaningful, a plain layout box contributes nothing.
///
/// Flat rather than nested: the shell publishes these as children of the window,
/// which is enough for a screen reader to enumerate and hit-test the UI. Nesting
/// (landmarks, grouping) can layer on later without changing what is collected.
#[derive(Clone, Debug)]
pub struct AccessNode {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub access: Access,
    /// `r-model`, when this element is an input, lets the shell match it against
    /// the focused model and report focus to the platform.
    pub model: Option<String>,
}

/// One keyboard-focusable element, in document (Tab) order. Carries the geometry
/// (for the focus ring) plus how the shell should act on it.
#[derive(Clone, Debug)]
pub struct FocusItem {
    /// See [`ScrollRegion::transform`]: a ring is drawn over the content and so
    /// has to be told what the content is being drawn through.
    pub transform: Option<Transform>,
    pub alpha: f32,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub kind: FocusKind,
    /// The scroller this item sits inside, if any, as an index into
    /// [`Layout::scrolls`].
    ///
    /// The focus ring is painted by the shell as its own scene *after* the
    /// document's, so it never passes through the `PushClip` a scroller emits
    /// around its children. Without knowing the enclosing scroller, a ring on a
    /// row scrolled out of a list draws over whatever is above the list. This
    /// is the enclosing one, not the item's own: a scroller that is itself
    /// focusable is clipped by its parent, not by itself.
    pub scroll: Option<usize>,
}

impl FocusItem {
    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px <= self.x + self.width && py >= self.y && py <= self.y + self.height
    }
}

#[derive(Clone, Debug)]
pub enum FocusKind {
    /// A text / textarea input: focusing it starts caret editing.
    Text { model: String, row: Option<String>, multiline: bool, text: Option<PaintText> },
    /// A button / checkbox / radio: Space or Enter runs its handler.
    Activate { on_tap: String, instance: Option<String> },
    /// A select: Space or Enter opens its dropdown.
    Select { model: String, row: Option<String>, options: Vec<String> },
}

/// The result of laying out a tree: paint items, hit regions, and focus regions,
/// all in painter's/topmost-last order.
#[derive(Clone, Debug, Default)]
pub struct Layout {
    pub paints: Vec<Paint>,
    pub hits: Vec<HitRegion>,
    pub focuses: Vec<FocusRegion>,
    pub selects: Vec<SelectRegion>,
    /// Keyboard-focusable elements in document (Tab) order.
    pub focusables: Vec<FocusItem>,
    pub scrolls: Vec<ScrollRegion>,
    /// Boxes with `:hover`/`:active` styling, in painter's order (topmost last).
    pub states: Vec<StateRegion>,
    /// Elements exposed to assistive technology, in document order.
    pub access: Vec<AccessNode>,
    /// Where every laid-out node ended up, for `query()` to read back.
    pub metrics: Vec<NodeMetrics>,
}

/// One node's laid-out box, keyed by the same child-index path the binding
/// registry and the element index use.
///
/// Absolute window pixels with any scroll offset already applied, which is what
/// a script asking "where is this" means. A node hidden by `r-show="false"` has
/// no entry: it reserves layout space but is not on screen, and geometry is a
/// property of what is actually shown.
#[derive(Clone, Debug, PartialEq)]
pub struct NodeMetrics {
    pub path: Vec<usize>,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// A node's content box, as an offset from its border-box origin plus a size.
///
/// Taffy resolves padding and border against the container during layout, so
/// these are already absolute pixels: percentage padding and `em` borders are
/// handled by the time this is asked. Clamped at zero, because padding wider
/// than the box itself is arithmetic, not a crash.
fn content_box(layout: &taffy::Layout) -> (f32, f32, f32, f32) {
    let (p, b) = (layout.padding, layout.border);
    (
        p.left + b.left,
        p.top + b.top,
        (layout.size.width - p.left - p.right - b.left - b.right).max(0.0),
        (layout.size.height - p.top - p.bottom - b.top - b.bottom).max(0.0),
    )
}

/// Callback that measures a text block:
/// `(text, font_size, weight, wrap, max_width) -> (w, h)`.
/// Measures a text node to `(width, height)` given an optional max width. Takes
/// the whole [`TextContent`] so new text properties (family, spacing, style…)
/// don't each widen this signature.
pub type Measure<'a> = dyn FnMut(&TextContent, Option<f32>) -> (f32, f32) + 'a;

/// What each taffy node paints.
enum PaintKind {
    Box {
        bg: Option<Background>,
        radius: Corners,
        border_width: f32,
        border_color: Option<Rgba>,
        clip: bool,
        shadow: Option<BoxShadow>,
    },
    Text(TextContent),
    Image(ImageContent),
    Tick(Rgba),
    /// Vector geometry with the paint properties its style resolved to.
    ///
    /// The paint travels with the geometry rather than being looked up later
    /// because `fill` and `stroke` are ordinary cascaded properties: they can
    /// come from a `:hover`, from a `:class`, or from halfway through a
    /// `transition`, and the value that matters is the one this frame computed.
    Path(PathContent, PathPaint),
}

/// The resolved paint for one `<path>`, lifted off the style at build time.
#[derive(Clone, Copy, Debug)]
pub struct PathPaint {
    pub fill: Option<Rgba>,
    pub fill_rule: FillRule,
    pub stroke: Option<Rgba>,
    pub stroke_width: f32,
    pub cap: LineCap,
    pub join: LineJoin,
}

fn to_dim(l: Len, vp: (f32, f32)) -> Dimension {
    match l {
        Len::Px(v) => length(v),
        Len::Pct(p) => percent(p),
        Len::Vw(v) => length(vp.0 * v / 100.0),
        Len::Vh(v) => length(vp.1 * v / 100.0),
    }
}

fn to_placement(p: GridPlace) -> GridPlacement {
    match p {
        GridPlace::Auto => auto(),
        GridPlace::Line(i) => line(i),
        GridPlace::Span(n) => span(n),
    }
}

fn to_track(t: Track) -> TrackSizingFunction {
    match t {
        Track::Px(v) => length(v),
        Track::Fr(f) => fr(f),
        Track::Auto => auto(),
        Track::MinMax(lo, hi) => minmax(
            // A flex minimum is invalid; fall back to `auto` (min-content).
            match lo {
                TrackSide::Px(v) => length(v),
                TrackSide::Fr(_) | TrackSide::Auto => auto(),
            },
            match hi {
                TrackSide::Px(v) => length(v),
                TrackSide::Fr(f) => fr(f),
                TrackSide::Auto => auto(),
            },
        ),
    }
}

/// Like [`to_track`] but for `grid-auto-rows`/`-columns`, whose tracks can't hold
/// a `repeat(…)` and so use taffy's non-repeated track type.
fn to_auto_track(t: Track) -> taffy::NonRepeatedTrackSizingFunction {
    match t {
        Track::Px(v) => length(v),
        Track::Fr(f) => fr(f),
        Track::Auto => auto(),
        Track::MinMax(lo, hi) => minmax(
            match lo {
                TrackSide::Px(v) => length(v),
                TrackSide::Fr(_) | TrackSide::Auto => auto(),
            },
            match hi {
                TrackSide::Px(v) => length(v),
                TrackSide::Fr(f) => fr(f),
                TrackSide::Auto => auto(),
            },
        ),
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
        grid_column: Line {
            start: to_placement(style.grid_column.0),
            end: to_placement(style.grid_column.1),
        },
        grid_row: Line {
            start: to_placement(style.grid_row.0),
            end: to_placement(style.grid_row.1),
        },
        grid_auto_flow: match style.grid_auto_flow {
            GridFlow::Row => taffy::GridAutoFlow::Row,
            GridFlow::Column => taffy::GridAutoFlow::Column,
            GridFlow::RowDense => taffy::GridAutoFlow::RowDense,
            GridFlow::ColumnDense => taffy::GridAutoFlow::ColumnDense,
        },
        grid_auto_rows: style.grid_auto_rows.iter().copied().map(to_auto_track).collect(),
        grid_auto_columns: style.grid_auto_columns.iter().copied().map(to_auto_track).collect(),
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
            .map(to_align_items)
            .or(if style.display == Display::Flex {
                Some(AlignItems::FlexStart)
            } else {
                None
            }),
        align_self: style.align_self.map(to_align_items),
        justify_self: style.justify_self.map(to_align_items),
        justify_items: style.justify_items.map(to_align_items),
        align_content: style.align_content.map(to_align_content),
        position: match style.position {
            Position::Static | Position::Relative | Position::Sticky => taffy::Position::Relative,
            Position::Absolute | Position::Fixed => taffy::Position::Absolute,
        },
        // A static box ignores its insets, which is the rule that makes `static`
        // worth having rather than being a synonym for `relative`. A sticky box
        // ignores them *here* for a different reason: its insets say where it
        // stops, not where it starts, and applying them as offsets would move it
        // before anything had scrolled.
        inset: if matches!(style.position, Position::Static | Position::Sticky) {
            Rect { left: auto(), right: auto(), top: auto(), bottom: auto() }
        } else {
            Rect {
                left: to_inset(style.inset[3], vp),
                right: to_inset(style.inset[1], vp),
                top: to_inset(style.inset[0], vp),
                bottom: to_inset(style.inset[2], vp),
            }
        },
        aspect_ratio: style.aspect_ratio,
        // taffy needs to know the box scrolls: it then sizes the box from its own
        // width/height (not its content) and reports `content_size`, which is how
        // far we can scroll.
        overflow: match style.overflow {
            Overflow::Scroll => Point {
                x: taffy::Overflow::Scroll,
                y: taffy::Overflow::Scroll,
            },
            _ => Point {
                x: taffy::Overflow::Visible,
                y: taffy::Overflow::Visible,
            },
        },
        flex_grow: style.grow,
        flex_shrink: style.shrink,
        flex_basis: style.basis.map(|l| to_dim(l, vp)).unwrap_or(auto()),
        flex_wrap: if style.wrap {
            FlexWrap::Wrap
        } else {
            FlexWrap::NoWrap
        },
        size: Size {
            // `flex-wrap` + a *percentage* width + a `max-width` trips a taffy
            // bug (still present in 0.12): it measures the container's content
            // at the full percentage width, ignoring the cap, so it sees one
            // row and sizes the cross-axis for one row, then clamps the width
            // to `max-width`, wraps to two rows, and never revisits the height.
            // The wrapped rows then paint *under* the following sibling. Both a
            // definite width and `auto` measure correctly, so for this exact
            // combination we drop the percentage to `auto` (fit-content, capped
            // by the same `max-width`), which fills available width up to the
            // cap for any content that overflows it, i.e. the wrap case.
            width: match style.width {
                Some(Len::Pct(_)) if style.wrap && style.max_width.is_some() => auto(),
                Some(l) => to_dim(l, vp),
                None => auto(),
            },
            height: style.height.map(|l| to_dim(l, vp)).unwrap_or(auto()),
        },
        min_size: Size {
            width: style.min_width.map(|l| to_dim(l, vp)).unwrap_or(auto()),
            height: style.min_height.map(|l| to_dim(l, vp)).unwrap_or(auto()),
        },
        max_size: Size {
            // A box with no width hugs its content. Hug means CSS `fit-content`
            //, min(max-content, available), so clamp it to the parent's inner
            // width. Without this, taffy hands a hugging box its full max-content
            // size and it bursts out of a narrower parent. An explicit width or
            // max-width is the author's call and is left alone.
            width: match (style.max_width, style.width) {
                (Some(l), _) => to_dim(l, vp),
                // `flex-shrink: 0` says "keep my size", don't clamp behind the
                // author's back; let it overflow and let the parent clip it.
                // `1.0_f32` spelled out: an unsuffixed float literal here makes
                // rustc fall back to f32 through a trait bound it warns about,
                // and that fallback is due to become a hard error. Only newer
                // toolchains than the one used on Windows report it, so it
                // surfaced from CI rather than locally.
                (None, None) if style.shrink != 0.0 => percent(1.0_f32),
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
        // taffy's gap is (column, row): width is the inline gap, height the block
        // gap. `column-gap`/`row-gap` override the `gap` shorthand per axis.
        gap: Size {
            width: length(style.column_gap.unwrap_or(style.gap)),
            height: length(style.row_gap.unwrap_or(style.gap)),
        },
        ..Default::default()
    }
}

fn to_align_items(a: Align) -> AlignItems {
    match a {
        Align::Start => AlignItems::FlexStart,
        Align::Center => AlignItems::Center,
        Align::End => AlignItems::FlexEnd,
        Align::Stretch => AlignItems::Stretch,
    }
}

fn to_align_content(j: Justify) -> AlignContent {
    match j {
        Justify::Start => AlignContent::FlexStart,
        Justify::Center => AlignContent::Center,
        Justify::End => AlignContent::FlexEnd,
        Justify::SpaceBetween => AlignContent::SpaceBetween,
        Justify::SpaceAround => AlignContent::SpaceAround,
    }
}

/// Every absolutely positioned box that has no insets of its own, with its
/// parent, so its static position can be discovered and then pinned.
///
/// A box that *does* name an inset is asking to be placed against its parent
/// and is left alone: that is what `top: 0` means and it already works.
fn collect_statics(
    tree: &TaffyTree<TextContent>,
    id: NodeId,
    out: &mut Vec<(NodeId, NodeId)>,
) {
    for child in tree.children(id).expect("children") {
        let st = tree.style(child).expect("style");
        let none_named = st.inset.left == auto()
            && st.inset.right == auto()
            && st.inset.top == auto()
            && st.inset.bottom == auto();
        if st.position == taffy::Position::Absolute && none_named {
            out.push((id, child));
        }
        collect_statics(tree, child, out);
    }
}

/// Where a `sticky` box actually paints, given where it was laid out.
///
/// Sticky is the one `position` that is not answered by the layout: the box
/// keeps its place in the flow, and its insets are **thresholds** rather than
/// offsets. It sits where it was put until its scroller's edge reaches that
/// threshold, then travels along the edge, and stops again when its parent runs
/// out from under it. That last clamp is the half that makes a list of sections
/// work: a heading rides the top of the scroller until the next section arrives
/// and pushes it off, rather than sitting there over the wrong rows.
///
/// Nothing else moves, which is also CSS: a sticky box occupies its original
/// space the whole time, so its siblings do not reflow as it travels.
///
/// `view` is the scroller's visible rectangle, `holder` the box the sticky one
/// sits in, and both are in the same window coordinates as `rect`.
fn stick(
    rect: (f32, f32, f32, f32),
    inset: [Option<f32>; 4],
    view: (f32, f32, f32, f32),
    holder: (f32, f32, f32, f32),
) -> (f32, f32) {
    let (x, y, w, h) = rect;
    let (vx, vy, vw, vh) = view;
    let (hx, hy, hw, hh) = holder;
    let (mut nx, mut ny) = (x, y);

    if let Some(top) = inset[0] {
        ny = ny.max(vy + top);
    }
    if let Some(bottom) = inset[2] {
        ny = ny.min(vy + vh - bottom - h);
    }
    if let Some(left) = inset[3] {
        nx = nx.max(vx + left);
    }
    if let Some(right) = inset[1] {
        nx = nx.min(vx + vw - right - w);
    }

    // Never outside the box it belongs to. The `max` after the `min` matters:
    // when the holder is shorter than the sticky box there is no valid position
    // and the top edge is the least surprising answer.
    ny = ny.min(hy + hh - h).max(hy);
    nx = nx.min(hx + hw - w).max(hx);
    (nx, ny)
}

/// Whether this box has to be laid out under its **containing block** rather
/// than under its parent, and whether it is `fixed` (so only the window will do).
///
/// Only when at least one inset is named. With all four `auto` the box keeps its
/// **static position**, which is defined as where it would have sat in its
/// parent's flow, so its parent is exactly where it has to be laid out and
/// moving it would lose the very thing it is asking for.
fn hoists(style: &Style) -> Option<bool> {
    if !style.position.is_out_of_flow() {
        return None;
    }
    let named = style.inset.iter().any(Option::is_some);
    named.then_some(style.position == Position::Fixed)
}

/// The inverse of an affine, or `None` when it collapses (zero determinant).
fn invert(m: Transform) -> Option<Transform> {
    let [a, b, c, d, e, f] = m;
    let det = a * d - b * c;
    if det.abs() < 1e-9 {
        return None;
    }
    let inv = 1.0 / det;
    Some([
        d * inv,
        -b * inv,
        -c * inv,
        a * inv,
        (c * f - d * e) * inv,
        (b * e - a * f) * inv,
    ])
}

/// A point through an affine.
fn apply(m: Transform, x: f32, y: f32) -> (f32, f32) {
    (m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5])
}

/// `outer * inner`, both as kurbo's `[a, b, c, d, e, f]`.
///
/// Needed because a transform is now *accumulated* as well as pushed: the paint
/// list nests its matrices, but a scrollbar drawn outside that list has to be
/// handed one matrix that already means the same thing.
fn compose(outer: Transform, inner: Transform) -> Transform {
    let [a1, b1, c1, d1, e1, f1] = outer;
    let [a2, b2, c2, d2, e2, f2] = inner;
    [
        a1 * a2 + c1 * b2,
        b1 * a2 + d1 * b2,
        a1 * c2 + c1 * d2,
        b1 * c2 + d1 * d2,
        a1 * e2 + c1 * f2 + e1,
        b1 * e2 + d1 * f2 + f1,
    ]
}

fn to_inset(l: Option<Len>, vp: (f32, f32)) -> LengthPercentageAuto {
    match l {
        None => auto(),
        Some(Len::Px(v)) => length(v),
        Some(Len::Pct(p)) => percent(p),
        Some(Len::Vw(v)) => length(vp.0 * v / 100.0),
        Some(Len::Vh(v)) => length(vp.1 * v / 100.0),
    }
}

/// A laid-out `<input>`: its model plus what kind it is. Becomes either a
/// `FocusRegion` (text/textarea) or a `SelectRegion` (select) in `collect`.
struct Bound {
    id: NodeId,
    model: String,
    /// The enclosing `r-for` row's key, the other half of an input's identity.
    row: Option<String>,
    multiline: bool,
    options: Option<Vec<String>>,
}

/// The widest a box can ever be, given its own CSS and everything above it.
///
/// `parent` is the parent's *inner* width bound, `None` when nothing above has
/// pinned one down. A `%` resolves against it; `vw`/`vh` against the viewport.
/// `min-width` wins over `max-width`, as in CSS.
fn width_cap(style: &Style, parent: Option<f32>, vp: (f32, f32)) -> Option<f32> {
    let resolve = |l: Len| match l {
        Len::Px(px) => Some(px),
        Len::Pct(p) => parent.map(|b| b * p),
        Len::Vw(v) => Some(vp.0 * v / 100.0),
        Len::Vh(v) => Some(vp.1 * v / 100.0),
    };
    let capped = match (style.width.and_then(resolve), style.max_width.and_then(resolve)) {
        (Some(w), Some(m)) => Some(w.min(m)),
        (Some(w), None) => Some(w),
        (None, Some(m)) => Some(parent.map_or(m, |p| p.min(m))),
        (None, None) => parent,
    };
    match style.min_width.and_then(resolve) {
        Some(min) => Some(capped.map_or(min, |c| c.max(min))),
        None => capped,
    }
}

/// The cap to hand this box's children: its own, less what its padding and
/// border take out of it.
fn inner_cap(style: &Style, own: Option<f32>) -> Option<f32> {
    own.map(|w| {
        let horizontal = style.padding.left + style.padding.right + style.border.left + style.border.right;
        (w - horizontal).max(0.0)
    })
}

#[allow(clippy::too_many_arguments)]
fn build(
    tree: &mut TaffyTree<TextContent>,
    node: &Node,
    paint: &mut Vec<(NodeId, PaintKind)>,
    handlers: &mut Vec<(NodeId, Option<String>, Vec<(Gesture, String)>, Cursor, Option<String>)>,
    models: &mut Vec<Bound>,
    focus_labels: &mut Vec<(NodeId, String, Option<String>)>,
    hidden: &mut Vec<NodeId>,
    opacities: &mut Vec<(NodeId, f32)>,
    scrolls: &mut Vec<NodeId>,
    transforms: &mut Vec<(NodeId, Transform)>,
    states: &mut Vec<(NodeId, Vec<usize>)>,
    access: &mut Vec<(NodeId, Access, Option<String>)>,
    // Where this node sits, and where every node's path is collected.
    //
    // Computed here rather than in `collect` on purpose: this walker recurses
    // over `node.children` directly, so a child's index is its index. `collect`
    // walks taffy ids, where the same assumption is only *probably* true, and a
    // mismatch there would hand back confident, wrong geometry.
    path: &[usize],
    paths: &mut Vec<(NodeId, Vec<usize>)>,
    vp: (f32, f32),
    // `cap` is the widest this node can end up, from the constraint chain above
    // it; `caps` is where each text leaf's own cap is left for the measure hook.
    cap: Option<f32>,
    caps: &mut HashMap<NodeId, f32>,
    // The `r-key` of the row this node is inside, inherited by everything under
    // it. A keyed node starts a new row; nothing else changes it.
    row: Option<&str>,
    // Sticky boxes and their thresholds, resolved to px, in `inset` order.
    stickies: &mut Vec<(NodeId, [Option<f32>; 4])>,
    // Out-of-flow boxes waiting for their containing block, as `(id, fixed)`.
    //
    // taffy positions an absolute child against its *parent*, and CSS positions
    // it against the nearest non-static ancestor, so the two agree only when
    // those are the same box. They are made the same box here: such a child is
    // built but withheld from its parent's child list, carried up this vector,
    // and handed to the first ancestor that is a containing block. A `fixed` box
    // is withheld from all of them and taken by the root.
    //
    // Doing it in the tree rather than by correcting coordinates afterwards is
    // what makes the *size* right too: `left: 0; right: 0` means "as wide as the
    // containing block", which is a question only the layout can answer.
    hoisted: &mut Vec<(NodeId, bool)>,
) -> NodeId {
    let own_cap = width_cap(&node.style, cap, vp);
    let child_cap = inner_cap(&node.style, own_cap);
    let row = node.key.as_deref().or(row);
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
                bg: node.style.background.clone(),
                radius: node.style.radius,
                border_width: node.style.border.top,
                border_color: node.style.border_color,
                clip: node.style.overflow != Overflow::Visible,
                shadow: node.style.box_shadow,
            },
        ));
        paint.push((id, PaintKind::Text(tc.clone())));
        // The text wraps inside this box, so its own padding and border come
        // out of the width available to the glyphs.
        if let Some(c) = child_cap {
            caps.insert(id, c);
        }
        id
    } else if let Some(color) = node.tick {
        let id = tree.new_leaf(to_taffy(&node.style, vp)).expect("taffy tick");
        paint.push((id, PaintKind::Tick(color)));
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
                bg: node.style.background.clone(),
                radius: node.style.radius,
                border_width: node.style.border.top,
                border_color: node.style.border_color,
                clip: node.style.overflow != Overflow::Visible,
                shadow: node.style.box_shadow,
            },
        ));
        paint.push((id, PaintKind::Image(ic.clone())));
        id
    } else if let Some(pc) = &node.path {
        // A path given no CSS size takes the size of its own geometry, the way
        // an <image> takes its intrinsic pixels. That makes the common case
        // (paste some path data, see it) work with no box to write, and the
        // geometry is in the element's own coordinates either way: naming a
        // width does not rescale the drawing, it changes the box the drawing
        // sits in. Scaling is `transform`, which every other element already
        // uses for the same thing.
        let mut ts = to_taffy(&node.style, vp);
        let bounds = pc.bounds();
        if node.style.width.is_none() {
            ts.size.width = length(bounds.map_or(0.0, |b| b.2.max(0.0)));
        }
        if node.style.height.is_none() {
            ts.size.height = length(bounds.map_or(0.0, |b| b.3.max(0.0)));
        }
        let id = tree.new_leaf(ts).expect("taffy path leaf");
        paint.push((
            id,
            PaintKind::Box {
                bg: node.style.background.clone(),
                radius: node.style.radius,
                border_width: node.style.border.top,
                border_color: node.style.border_color,
                clip: node.style.overflow != Overflow::Visible,
                shadow: node.style.box_shadow,
            },
        ));
        paint.push((
            id,
            PaintKind::Path(
                pc.clone(),
                PathPaint {
                    fill: node.style.fill,
                    fill_rule: node.style.fill_rule,
                    stroke: node.style.stroke,
                    stroke_width: node.style.stroke_width,
                    cap: node.style.stroke_linecap,
                    join: node.style.stroke_linejoin,
                },
            ),
        ));
        id
    } else {
        let mark = hoisted.len();
        let mut children: Vec<NodeId> = node
            .children
            .iter()
            .enumerate()
            .filter_map(|(i, c)| {
                let mut cp = path.to_vec();
                cp.push(i);
                let cid = build(tree, c, paint, handlers, models, focus_labels, hidden, opacities, scrolls, transforms, states, access, &cp, paths, vp, child_cap, caps, row, stickies, hoisted);
                match hoists(&c.style) {
                    Some(fixed) => {
                        hoisted.push((cid, fixed));
                        None
                    }
                    None => Some(cid),
                }
            })
            .collect();
        // This box is a containing block, so the out-of-flow descendants that
        // came up from its subtree stop here.
        //
        // **A `transform` makes a containing block for both kinds**, whatever
        // the box's own `position` says, which is CSS's rule and the reason
        // `position: fixed` famously stops being fixed inside a transformed
        // parent. It is not an oddity to work around: a transform moves the
        // whole subtree, so there is no way to hold a descendant still against
        // the window while its ancestor slides, and pretending otherwise would
        // mean drawing it somewhere the transform says it is not.
        //
        // Without a transform, only a non-static box claims anything, and only
        // the absolute ones: `fixed` carries on to the root.
        //
        // They go on the end, which is also where CSS paints them: a positioned
        // descendant is drawn over its containing block's in-flow content.
        let transformed = node.style.transform.is_some();
        if transformed || node.style.position.is_containing_block() {
            let mut i = mark;
            while i < hoisted.len() {
                let carries_on = hoisted[i].1 && !transformed;
                if carries_on {
                    i += 1;
                } else {
                    children.push(hoisted.remove(i).0);
                }
            }
        }
        let id = if children.is_empty() {
            tree.new_leaf(to_taffy(&node.style, vp)).expect("taffy leaf")
        } else {
            tree.new_with_children(to_taffy(&node.style, vp), &children)
                .expect("taffy node")
        };
        paint.push((
            id,
            PaintKind::Box {
                bg: node.style.background.clone(),
                radius: node.style.radius,
                // Uniform border for rendering (top width is representative).
                border_width: node.style.border.top,
                border_color: node.style.border_color,
                clip: node.style.overflow != Overflow::Visible,
                shadow: node.style.box_shadow,
            },
        ));
        id
    };
    // A hit region is needed for anything a pointer can reach, not only for a
    // `@tap`: an element with just `@drag` still has to be found by a hit test.
    if node.on_tap.is_some() || !node.gestures.is_empty() {
        handlers.push((
            id,
            node.on_tap.clone(),
            node.gestures.clone(),
            node.style.cursor,
            node.instance.clone(),
        ));
    }
    if let Some(model) = &node.model {
        models.push(Bound {
            id,
            model: model.clone(),
            row: row.map(str::to_string),
            multiline: node.multiline,
            options: node.options.clone(),
        });
    }
    if let Some(fm) = &node.focus_model {
        focus_labels.push((id, fm.clone(), row.map(str::to_string)));
    }
    if node.hidden {
        hidden.push(id);
    }
    if node.style.opacity < 1.0 {
        opacities.push((id, node.style.opacity.max(0.0)));
    }
    if let Some(tf) = node.style.transform {
        transforms.push((id, tf));
    }
    if node.style.overflow == Overflow::Scroll {
        scrolls.push(id);
    }
    if node.style.position.is_sticky() {
        // Resolved to px here, where the viewport is in hand, so `collect` can
        // compare them against window coordinates without carrying units.
        let px = |l: Option<Len>| match l {
            None => None,
            Some(Len::Px(v)) => Some(v),
            Some(Len::Pct(p)) => Some(vp.1 * p / 100.0),
            Some(Len::Vw(v)) => Some(vp.0 * v / 100.0),
            Some(Len::Vh(v)) => Some(vp.1 * v / 100.0),
        };
        stickies.push((
            id,
            [px(node.style.inset[0]), px(node.style.inset[1]), px(node.style.inset[2]), px(node.style.inset[3])],
        ));
    }
    if let Some(path) = &node.state_path {
        states.push((id, path.clone()));
    }
    paths.push((id, path.to_vec()));
    if node.access.role.is_meaningful() {
        access.push((id, node.access.clone(), node.model.clone()));
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
    handlers: &[(NodeId, Option<String>, Vec<(Gesture, String)>, Cursor, Option<String>)],
    models: &[Bound],
    focus_labels: &[(NodeId, String, Option<String>)],
    hidden: &[NodeId],
    opacities: &[(NodeId, f32)],
    scrolls: &[NodeId],
    transforms: &[(NodeId, Transform)],
    states: &[(NodeId, Vec<usize>)],
    access: &[(NodeId, Access, Option<String>)],
    paths: &[(NodeId, Vec<usize>)],
    offsets: &[Offset],
    vp: (f32, f32),
    stickies: &[(NodeId, [Option<f32>; 4])],
    // The box this node sits in, as `(x, y, width, height)` in window
    // coordinates. Only `sticky` reads it, to stop travelling when its parent
    // runs out from under it.
    holder: (f32, f32, f32, f32),
    // The nearest scroller above this node, so a focus ring can be clipped to
    // the box that clips everything else in it.
    inside_scroll: Option<usize>,
    // The `transform` and `opacity` accumulated from the ancestors, so anything
    // drawn outside the paint list can be drawn through the same lens.
    xform: Option<Transform>,
    dim: f32,
    out: &mut Layout,
) {
    let layout = tree.layout(id).expect("layout");
    let mut x = origin_x + layout.location.x;
    let mut y = origin_y + layout.location.y;

    // `sticky` is resolved here rather than in the layout, because it is a
    // question about the scroll offset and the layout does not know one. Done
    // before anything is recorded, so the box's paint, its hit region, its
    // metrics and its children all move together: a sticky header you can see
    // but cannot tap would be worse than one that does not stick.
    if let Some((_, inset)) = stickies.iter().find(|(nid, _)| *nid == id) {
        // Against the scroller it is inside; with none, against the window,
        // which is what a sticky box in an unscrolled document sits in.
        let view = match inside_scroll.and_then(|sid| out.scrolls.get(sid)) {
            Some(s) => (s.x, s.y, s.width, s.height),
            None => (0.0, 0.0, vp.0, vp.1),
        };
        let (nx, ny) = stick(
            (x, y, layout.size.width, layout.size.height),
            *inset,
            view,
            holder,
        );
        x = nx;
        y = ny;
    }
    let x = x;
    let y = y;

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

    // `transform` wraps the box and its subtree. The parsed matrix is in local
    // coords; bake in the origin (CSS default: the box centre) so it applies to
    // absolute coordinates directly.
    let transform = transforms.iter().find(|(nid, _)| *nid == id).map(|(_, m)| *m);
    // What the subtree, and anything drawn on its behalf outside the paint
    // list, is seen through.
    let mut child_xform = xform;
    if let Some(m) = transform {
        let (ox, oy) = (x + layout.size.width / 2.0, y + layout.size.height / 2.0);
        let baked = centre_transform(m, ox, oy);
        out.paints.push(Paint::PushTransform(baked));
        child_xform = Some(match xform {
            Some(outer) => compose(outer, baked),
            None => baked,
        });
    }
    let child_dim = dim * alpha;

    let mut clip = false;
    let mut clip_radius = [0.0; 4];
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
                shadow,
            } => {
                clip = *c;
                clip_radius = *radius;
                // The shadow goes down first, so the box's own fill sits on top.
                // Outer shadows only for now; inset is parsed but not drawn.
                if let Some(sh) = shadow.filter(|s| !s.inset) {
                    out.paints.push(Paint::Shadow {
                        x: x + sh.dx - sh.spread,
                        y: y + sh.dy - sh.spread,
                        width: layout.size.width + 2.0 * sh.spread,
                        height: layout.size.height + 2.0 * sh.spread,
                        // vello's blurred rect takes one radius; use the largest
                        // corner as a stand-in (per-corner blur isn't supported).
                        radius: radius.iter().copied().fold(0.0, f32::max),
                        blur: sh.blur,
                        color: sh.color,
                    });
                }
                let has_border = *border_width > 0.0 && border_color.is_some();
                if bg.is_some() || has_border {
                    out.paints.push(Paint::Rect(PaintRect {
                        x,
                        y,
                        width: layout.size.width,
                        height: layout.size.height,
                        background: bg.clone(),
                        radius: *radius,
                        border_width: *border_width,
                        border_color: *border_color,
                    }));
                }
            }
            // Glyphs go in the *content* box, inside this node's own padding and
            // border. Painting them at the border box put a padded label flush
            // against the edge of its own background: the box grew, the words
            // did not move. The size matters as much as the origin, since it is
            // what the run is aligned and wrapped within.
            PaintKind::Text(tc) => {
                let (cx, cy, cw, ch) = content_box(layout);
                out.paints.push(Paint::Text(PaintText {
                    x: x + cx,
                    y: y + cy,
                    width: cw,
                    height: ch,
                    content: tc.clone(),
                }))
            }
            PaintKind::Tick(color) => out.paints.push(Paint::Tick(PaintTick {
                x,
                y,
                width: layout.size.width,
                height: layout.size.height,
                color: *color,
            })),
            PaintKind::Image(ic) => out.paints.push(Paint::Image(PaintImage {
                x,
                y,
                width: layout.size.width,
                height: layout.size.height,
                content: ic.clone(),
            })),
            // From the content corner, not the border corner, so padding moves
            // the drawing the way it moves text rather than being ignored.
            PaintKind::Path(pc, pp) => {
                let (cx, cy, _, _) = content_box(layout);
                out.paints.push(Paint::Path(PaintPath {
                    x: x + cx,
                    y: y + cy,
                    content: pc.clone(),
                    paint: *pp,
                }))
            }
        }
    }

    // A `for=` label targeting a text input: a focus region at the label's box,
    // carrying the *target's* model, so tapping the label focuses that input.
    if let Some((_, model, row)) = focus_labels.iter().find(|(nid, ..)| *nid == id) {
        out.focuses.push(FocusRegion {
            x,
            y,
            width: layout.size.width,
            height: layout.size.height,
            model: model.clone(),
            row: row.clone(),
            text: None,
            multiline: false,
            scroll_id: None,
        });
    }

    // Assistive technology needs the same geometry the pointer uses, so this rides
    // the same walk. `hidden` nodes returned above, so an `r-show="false"` element
    // is absent from the a11y tree too, not merely invisible.
    if let Some((_, node_access, model)) = access.iter().find(|(nid, ..)| *nid == id) {
        out.access.push(AccessNode {
            x,
            y,
            width: layout.size.width,
            height: layout.size.height,
            access: node_access.clone(),
            model: model.clone(),
        });
    }

    // Emitted after the `hidden` check above, so an `r-show="false"` subtree has
    // no metrics at all rather than metrics nobody can see.
    if let Some((_, path)) = paths.iter().find(|(nid, _)| *nid == id) {
        out.metrics.push(NodeMetrics {
            path: path.clone(),
            x,
            y,
            width: layout.size.width,
            height: layout.size.height,
        });
    }

    // Emitted for any box a `:hover`/`:active` rule could style, tappable or not,
    // unlike `cursor`, pointer-state styling is not limited to `@tap` boxes.
    if let Some((_, path)) = states.iter().find(|(nid, _)| *nid == id) {
        out.states.push(StateRegion {
            x,
            y,
            width: layout.size.width,
            height: layout.size.height,
            path: path.clone(),
        });
    }

    if let Some((_, handler, gestures, cursor, instance)) =
        handlers.iter().find(|(nid, ..)| *nid == id)
    {
        out.hits.push(HitRegion {
            transform: child_xform,
            x,
            y,
            width: layout.size.width,
            height: layout.size.height,
            on_tap: handler.clone(),
            gestures: gestures.clone(),
            cursor: *cursor,
            instance: instance.clone(),
        });
    }

    let (fw, fh) = (layout.size.width, layout.size.height);
    if let Some(bound) = models.iter().find(|b| b.id == id) {
        if let Some(options) = &bound.options {
            // A select: no caret, just a tappable box that opens a dropdown.
            out.selects.push(SelectRegion {
                x,
                y,
                width: fw,
                height: fh,
                model: bound.model.clone(),
                row: bound.row.clone(),
                options: options.clone(),
            });
            out.focusables.push(FocusItem {
                transform: child_xform,
                alpha: child_dim,
                x,
                y,
                width: fw,
                height: fh,
                kind: FocusKind::Select {
                    model: bound.model.clone(),
                    row: bound.row.clone(),
                    options: options.clone(),
                },
                scroll: inside_scroll,
            });
        } else {
            // A text/textarea input: its value is rendered by its single text
            // child; find that child's box so a tap resolves to a caret index.
            let text = tree
                .children(id)
                .ok()
                .and_then(|kids| kids.first().copied())
                .and_then(|kid| {
                    let child = tree.layout(kid).ok()?;
                    let content = paint.iter().find_map(|(nid, k)| match k {
                        PaintKind::Text(tc) if *nid == kid => Some(tc.clone()),
                        _ => None,
                    })?;
                    // The same content box the glyphs are painted in, or the
                    // caret would sit at the border box while the text it is
                    // supposed to be inside sits within the padding.
                    let (cx, cy, cw, ch) = content_box(child);
                    Some(PaintText {
                        x: x + child.location.x + cx,
                        y: y + child.location.y + cy,
                        width: cw,
                        height: ch,
                        content,
                    })
                });
            out.focuses.push(FocusRegion {
                x,
                y,
                width: fw,
                height: fh,
                model: bound.model.clone(),
                row: bound.row.clone(),
                text: text.clone(),
                multiline: bound.multiline,
                // The scroll block below assigns ids as `out.scrolls.len()`, so if
                // this node scrolls it will get the current length as its id.
                scroll_id: scrolls.contains(&id).then(|| out.scrolls.len()),
            });
            out.focusables.push(FocusItem {
                transform: child_xform,
                alpha: child_dim,
                x,
                y,
                width: fw,
                height: fh,
                kind: FocusKind::Text {
                    model: bound.model.clone(),
                    row: bound.row.clone(),
                    multiline: bound.multiline,
                    text,
                },
                scroll: inside_scroll,
            });
        }
    } else if let Some((_, Some(handler), _, _, instance)) =
        handlers.iter().find(|(nid, ..)| *nid == id)
    {
        // A button / checkbox / radio (anything with a `@tap` handler) is
        // keyboard-reachable: Space or Enter runs the same handler as a tap.
        //
        // Only `@tap`. A keyboard has no pointer, so an element that declares
        // only `@drag` has nothing a key could stand in for, and offering Enter
        // as a fake drag would be worse than leaving it alone.
        out.focusables.push(FocusItem {
            transform: child_xform,
            alpha: child_dim,
            x,
            y,
            width: fw,
            height: fh,
            kind: FocusKind::Activate { on_tap: handler.clone(), instance: instance.clone() },
            scroll: inside_scroll,
        });
    }

    // overflow: clip/scroll, bound the subtree to this box (following its corners).
    if clip {
        out.paints.push(Paint::PushClip {
            x,
            y,
            width: layout.size.width,
            height: layout.size.height,
            radius: clip_radius,
        });
    }

    // A scroller shifts its children by the current offset and registers itself
    // so the wheel, the scrollbars and the keyboard can find it.
    let mut shift = Offset::default();
    // What the children are clipped by: this box if it scrolls, otherwise
    // whatever was clipping us.
    let mut child_scroll = inside_scroll;
    if scrolls.contains(&id) {
        let sid = out.scrolls.len();
        child_scroll = Some(sid);
        let max = Offset {
            x: (layout.content_size.width - layout.size.width).max(0.0),
            y: (layout.content_size.height - layout.size.height).max(0.0),
        };
        shift = offsets.get(sid).copied().unwrap_or_default().clamp_to(max);
        out.scrolls.push(ScrollRegion {
            transform: child_xform,
            alpha: child_dim,
            id: sid,
            x,
            y,
            width: layout.size.width,
            height: layout.size.height,
            content_width: layout.content_size.width,
            content_height: layout.content_size.height,
            max,
        });
    }

    // In-flow children first, sticky ones after, because a positioned box paints
    // over its in-flow siblings and a sticky one is positioned. Without this a
    // sticky heading is drawn *before* the rows it is meant to sit over, so the
    // content scrolls straight through it and the heading is unreadable exactly
    // when it is doing its job. Found by looking, not by the tests: every
    // assertion about geometry passed.
    //
    // Ordering here also puts the sticky box on top for hit testing, since the
    // topmost hit region wins and later means topmost.
    let kids = tree.children(id).expect("children");
    let (stuck, flowing): (Vec<NodeId>, Vec<NodeId>) =
        kids.iter().partition(|c| stickies.iter().any(|(nid, _)| nid == *c));
    for child in flowing.into_iter().chain(stuck) {
        collect(
            tree,
            child,
            x - shift.x,
            y - shift.y,
            paint,
            handlers,
            models,
            focus_labels,
            hidden,
            opacities,
            scrolls,
            transforms,
            states,
            access,
            paths,
            offsets,
            vp,
            stickies,
            // Children of this node are held by this node, measured where the
            // scroll has put it, so a sticky grandchild stops at the same edge a
            // reader sees.
            (x - shift.x, y - shift.y, layout.size.width, layout.size.height),
            child_scroll,
            child_xform,
            child_dim,
            out,
        );
    }
    if clip {
        out.paints.push(Paint::PopClip);
    }
    if transform.is_some() {
        out.paints.push(Paint::PopTransform);
    }
    if alpha < 1.0 {
        out.paints.push(Paint::PopOpacity);
    }
}

/// Bake a transform-origin at `(ox, oy)` into a local transform matrix `m`, so
/// the result maps absolute coordinates: `p ↦ M·(p − o) + o`.
fn centre_transform(m: Transform, ox: f32, oy: f32) -> Transform {
    let [a, b, c, d, e, f] = m;
    [
        a,
        b,
        c,
        d,
        e + ox - a * ox - c * oy,
        f + oy - b * ox - d * oy,
    ]
}

/// Lay out `root` into an `avail_w` x `avail_h` viewport, returning paint items
/// and hit regions. Text leaves are sized via `measure`.
pub fn layout(root: &Node, avail_w: f32, avail_h: f32, measure: &mut Measure) -> Layout {
    layout_scrolled(root, avail_w, avail_h, &[], measure)
}

/// Lay out with the shell's current scroll offsets (one per scrollable box, in
/// tree order). A missing entry is 0.
pub fn layout_scrolled(
    root: &Node,
    avail_w: f32,
    avail_h: f32,
    offsets: &[Offset],
    measure: &mut Measure,
) -> Layout {
    let mut tree: TaffyTree<TextContent> = TaffyTree::new();
    // Taffy rounds boxes to whole pixels by default, which can shave a fraction
    // off a text box and make paint re-wrap the last word into a line the box
    // has no height for. Keep the exact sizes measure asked for.
    tree.disable_rounding();
    let mut paint = Vec::new();
    let mut handlers = Vec::new();
    let mut models = Vec::new();
    let mut focus_labels = Vec::new();
    let mut hidden = Vec::new();
    let mut opacities = Vec::new();
    let mut scrolls = Vec::new();
    let mut transforms = Vec::new();
    let mut states = Vec::new();
    let mut access = Vec::new();
    let mut paths = Vec::new();
    let vp = (avail_w, avail_h);
    let mut caps: HashMap<NodeId, f32> = HashMap::new();
    let mut stickies: Vec<(NodeId, [Option<f32>; 4])> = Vec::new();
    let mut hoisted: Vec<(NodeId, bool)> = Vec::new();
    let root_id = build(
        &mut tree,
        root,
        &mut paint,
        &mut handlers,
        &mut models,
        &mut focus_labels,
        &mut hidden,
        &mut opacities,
        &mut scrolls,
        &mut transforms,
        &mut states,
        &mut access,
        &[], // the root's own path is empty
        &mut paths,
        vp,
        // The root is forced to the viewport below, so that is the widest
        // anything can be.
        Some(avail_w),
        &mut caps,
        None, // the root is not inside any row
        &mut stickies,
        &mut hoisted,
    );

    // The root is the initial containing block, so it takes whatever is still
    // looking for one: every `fixed` box, and any absolute box with no
    // non-static ancestor. It is also the window, which is what makes `fixed`
    // mean fixed, since a box parented to the root is outside every scroller and
    // so is never shifted by one.
    for (id, _) in hoisted.drain(..) {
        tree.add_child(root_id, id).expect("the root takes what is left");
    }

    // Force the root to fill the viewport so a `screen` always covers the window.
    let mut root_style = to_taffy(&root.style, vp);
    root_style.size = Size {
        width: length(avail_w),
        height: length(avail_h),
    };
    tree.set_style(root_id, root_style).expect("set root style");

    // An absolutely positioned box with no insets sits at its **static**
    // position in CSS: where it would have been in normal flow. taffy has no
    // such concept and puts it at the parent's content-box origin instead, so
    // such a box jumped to the top-left of its parent. That is what made a
    // departing page fly up over the nav bar, and it is why a route transition
    // needed a wrapper box to look right at all.
    //
    // So it is laid out twice. The first pass leaves it in the flow, which is
    // what discovers where it would have been; the second pins it there and
    // takes it out. The cost is one extra layout, and only when such a box
    // exists at all, which is normally never and during a swap is brief.
    //
    // **Only one out-of-flow sibling goes back in the flow at a time**, which
    // is the whole reason for the `rounds` below. Putting them all back at once
    // measures each one against the others, and a box that holds no space
    // cannot push its sibling down: two of them landed a whole box apart
    // instead of on top of one another. Seen in `examples/chart.rux`, where a
    // filled band and the line over it are two absolutely positioned paths over
    // the same points, and the line was drawn a band's height below the band,
    // outside the frame entirely.
    //
    // Siblings are the only ones that interfere, so the number of passes is the
    // most no-inset absolute children any single parent has: one in almost
    // every document, two for a chart, and never the total.
    let mut statics: Vec<(NodeId, NodeId)> = Vec::new();
    collect_statics(&tree, root_id, &mut statics);
    // Each one's index among its own parent's statics, which is the pass it
    // gets to be in the flow for.
    let mut rounds: Vec<usize> = Vec::with_capacity(statics.len());
    {
        let mut seen: HashMap<NodeId, usize> = HashMap::new();
        for (parent, _) in &statics {
            let n = seen.entry(*parent).or_insert(0);
            rounds.push(*n);
            *n += 1;
        }
    }
    let passes = rounds.iter().copied().max().map_or(0, |m| m + 1);

    let mut measure_fn = |known: Size<Option<f32>>,
                          available: Size<AvailableSpace>,
                          id: NodeId,
                          ctx: Option<&mut TextContent>,
                          _style: &taffy::Style| {
            if let (Some(w), Some(h)) = (known.width, known.height) {
                return Size { width: w, height: h };
            }
            match ctx {
                Some(tc) => {
                    // Wrap to a definite width; otherwise (content sizing) let
                    // the text take its natural single-line width.
                    let max = known.width.or(match available.width {
                        AvailableSpace::Definite(w) => Some(w),
                        // Min-content is the narrowest the box can be without
                        // its content spilling, which for text is the longest
                        // unbreakable word. Wrapping at zero asks exactly that.
                        // Answering it with the single-line width (which is
                        // what "no constraint" means here) told taffy the box
                        // could never be narrower than one long line.
                        AvailableSpace::MinContent => Some(0.0),
                        AvailableSpace::MaxContent => None,
                    });
                    // Never measure at a width this box can never have. Taffy
                    // sizes a capped box from its *un*capped content, clamps
                    // the width afterwards, and does not revisit the height, so
                    // a `max-width` card was measured as one long line and
                    // drawn as three. Wrapping at the cap up front is what
                    // makes the measured height the height that gets drawn.
                    let cap = caps.get(&id).copied();
                    let max = match (max, cap) {
                        (Some(m), Some(c)) => Some(m.min(c)),
                        (None, Some(c)) => Some(c),
                        (m, None) => m,
                    };
                    let (w, h) = measure(tc, max);
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
        };

    let space = Size {
        width: AvailableSpace::Definite(avail_w),
        height: AvailableSpace::Definite(avail_h),
    };
    tree.compute_layout_with_measure(root_id, space, &mut measure_fn)
        .expect("compute layout");

    if !statics.is_empty() {
        // One pass per round. In round `k` the k-th static child of every
        // parent is in the flow and all the others are out of it, which is
        // exactly the arrangement its static position is defined against.
        let mut found: Vec<(f32, f32)> = vec![(0.0, 0.0); statics.len()];
        for round in 0..passes {
            for (i, (_, id)) in statics.iter().enumerate() {
                let mut st = tree.style(*id).expect("style").clone();
                st.position = if rounds[i] == round {
                    taffy::Position::Relative
                } else {
                    taffy::Position::Absolute
                };
                tree.set_style(*id, st).expect("one of them back in the flow");
            }
            tree.compute_layout_with_measure(root_id, space, &mut measure_fn)
                .expect("discover the static positions");
            for (i, (_, id)) in statics.iter().enumerate() {
                if rounds[i] == round {
                    let here = tree.layout(*id).expect("layout").location;
                    found[i] = (here.x, here.y);
                }
            }
        }
        for (i, (_, id)) in statics.iter().enumerate() {
            let mut st = tree.style(*id).expect("style").clone();
            st.position = taffy::Position::Absolute;
            st.inset.left = length(found[i].0);
            st.inset.top = length(found[i].1);
            tree.set_style(*id, st).expect("pinned where it stood");
        }
        tree.compute_layout_with_measure(root_id, space, &mut measure_fn)
            .expect("compute layout again");
    }

    let mut out = Layout::default();
    collect(
        &tree, root_id, 0.0, 0.0, &paint, &handlers, &models, &focus_labels, &hidden, &opacities,
        &scrolls, &transforms, &states, &access, &paths, offsets, vp, &stickies,
        // The root is held by the window.
        (0.0, 0.0, vp.0, vp.1),
        None, None, 1.0, &mut out,
    );
    out
}

#[cfg(test)]
mod hit_transform_tests {
    use super::*;

    fn region(m: Option<Transform>) -> HitRegion {
        HitRegion {
            transform: m,
            x: 100.0,
            y: 100.0,
            width: 50.0,
            height: 20.0,
            on_tap: None,
            gestures: Vec::new(),
            cursor: Cursor::default(),
            instance: None,
        }
    }

    /// A hit region follows the transform its box is drawn through.
    ///
    /// Silent when wrong: the tap simply lands on whatever else is there, or on
    /// nothing, and the element looks unresponsive rather than misplaced.
    #[test]
    fn a_transformed_region_is_hit_where_it_is_drawn() {
        let plain = region(None);
        assert!(plain.contains(110.0, 110.0), "inside");
        assert!(!plain.contains(210.0, 110.0), "100px to the right is outside");

        // The same box slid 100px right: kurbo order is [a, b, c, d, e, f].
        let slid = region(Some([1.0, 0.0, 0.0, 1.0, 100.0, 0.0]));
        assert!(
            !slid.contains(110.0, 110.0),
            "where it used to be is no longer where it is"
        );
        assert!(slid.contains(210.0, 110.0), "it is hit where it is drawn");
    }

    /// A box scaled to nothing has no area to hit.
    #[test]
    fn a_collapsed_region_cannot_be_hit() {
        let gone = region(Some([0.0, 0.0, 0.0, 0.0, 0.0, 0.0]));
        assert!(!gone.contains(110.0, 110.0));
        assert!(!gone.contains(0.0, 0.0));
    }
}

#[cfg(test)]
mod reveal_tests {
    use super::*;

    /// A 300px-tall scroller holding 900px of content, scrolled to the top.
    fn thread() -> Vec<ScrollRegion> {
        vec![ScrollRegion {
            transform: None,
            alpha: 1.0,
            id: 0,
            x: 20.0,
            y: 100.0,
            width: 400.0,
            height: 300.0,
            content_width: 400.0,
            content_height: 900.0,
            max: Offset { x: 0.0, y: 600.0 },
        }]
    }

    /// The case the whole function exists for: a row **below the fold** still
    /// belongs to the scroller that holds it.
    ///
    /// Matching on the scroller's visible rectangle instead answers "no
    /// scroller", so the reveal is dropped and a list asked to scroll to its
    /// newest row does nothing at all. That shipped, and only driving a message
    /// list in the window found it: every reveal that worked was of an element
    /// already a nudge away from being visible.
    #[test]
    fn an_element_past_the_bottom_still_has_a_scroller() {
        let scrolls = thread();
        let offsets = vec![Offset { x: 0.0, y: 0.0 }];
        // 880px down the content, which is 580px below the visible bottom edge.
        assert_eq!(containing_scroller(&scrolls, &offsets, 30.0, 980.0), Some(0));
    }

    /// And one above the top, which is the same failure in the other direction:
    /// scrolled down, the rows already read are outside the visible band.
    #[test]
    fn an_element_above_the_top_still_has_a_scroller() {
        // The scroller's own box does not move when its content does, so `y`
        // stays at 100 and the offset is what puts the content above it: the
        // content now runs from -500 to 400.
        let scrolls = thread();
        let offsets = vec![Offset { x: 0.0, y: 600.0 }];
        assert_eq!(containing_scroller(&scrolls, &offsets, 30.0, -450.0), Some(0));
    }

    /// Something genuinely outside is still outside, so the fix did not simply
    /// make every scroller claim everything.
    #[test]
    fn a_box_outside_the_content_belongs_to_no_scroller() {
        let scrolls = thread();
        let offsets = vec![Offset { x: 0.0, y: 0.0 }];
        assert_eq!(
            containing_scroller(&scrolls, &offsets, 30.0, 50.0),
            None,
            "above where the content starts"
        );
        assert_eq!(
            containing_scroller(&scrolls, &offsets, 500.0, 200.0),
            None,
            "off to the side of it"
        );
    }

    /// Nested scrollers: the innermost one is the one whose offset moves the box.
    #[test]
    fn the_innermost_scroller_wins() {
        let mut scrolls = thread();
        scrolls.push(ScrollRegion {
            transform: None,
            alpha: 1.0,
            id: 1,
            x: 30.0,
            y: 150.0,
            width: 200.0,
            height: 100.0,
            content_width: 200.0,
            content_height: 400.0,
            max: Offset { x: 0.0, y: 300.0 },
        });
        let offsets = vec![Offset { x: 0.0, y: 0.0 }, Offset { x: 0.0, y: 0.0 }];
        assert_eq!(containing_scroller(&scrolls, &offsets, 40.0, 500.0), Some(1));
    }
}
