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

mod inline;
pub mod rich;
pub mod white_space;
pub use white_space::WhiteSpace;
pub use rich::{InlineItem, RichText, RunStyle, TextRun, VerticalAlign};

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
    /// All four zero, the common case for a box with no border.
    pub const ZERO: Self = Self::uniform(0.0);

    /// Whether all four are the same, which is what lets a border be drawn as
    /// one stroke around a rounded rectangle rather than four filled edges.
    pub fn is_uniform(&self) -> bool {
        self.top == self.right && self.right == self.bottom && self.bottom == self.left
    }

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
    /// `vmin` and `vmax`: of the window's shorter and longer side.
    Vmin(f32),
    Vmax(f32),
}

/// The edge of an anchor an `anchor()` measures to (CSS anchor positioning).
///
/// `Frac` is a percentage along the anchor from its start edge, so
/// `anchor(50%)` and `anchor(center)` are the same place.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AnchorSide {
    Top,
    Bottom,
    Left,
    Right,
    Center,
    Frac(f32),
}

/// `anchor()` in an inset: which anchor (`None` is the box's
/// `position-anchor`), which edge of it, and what to use when there is no
/// such anchor on screen.
#[derive(Clone, Debug, PartialEq)]
pub struct AnchorRef {
    pub name: Option<String>,
    pub side: AnchorSide,
    pub fallback: Option<Len>,
}

/// `anchor-size()` in `width` or `height`: the anchor's width, or with
/// `height` its height.
#[derive(Clone, Debug, PartialEq)]
pub struct AnchorSize {
    pub name: Option<String>,
    pub height: bool,
    pub fallback: Option<Len>,
}

/// A box's anchor-positioning properties. See `Style::anchor`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AnchorStyle {
    /// `anchor-name`: what an anchored box calls this one. Document-wide,
    /// and the last box in document order with a name is the one it means.
    pub name: Option<String>,
    /// `position-anchor`: the anchor an `anchor()` naming none measures to.
    pub position_anchor: Option<String>,
    /// An `anchor()` in `top`, `right`, `bottom`, `left`, in `inset` order.
    /// That side's `inset` is `None`: it is placed at paint time, against
    /// where the anchor is drawn, and not by the layout. See `Anchoring`.
    pub inset: [Option<AnchorRef>; 4],
    /// `anchor-size()` in `width` and `height`. Resolved by laying out a
    /// second time once the anchor has a size.
    pub size: [Option<AnchorSize>; 2],
    /// `position-try-fallbacks`: the other places to try, in order, when the
    /// box does not fit where its own insets put it.
    pub tries: Vec<PositionTry>,
    /// `position-try-order`.
    pub try_order: TryOrder,
}

impl AnchorStyle {
    /// Whether the box is placed or sized against an anchor at all.
    pub fn anchored(&self) -> bool {
        self.inset.iter().any(Option::is_some) || self.size.iter().any(Option::is_some) || !self.tries.is_empty()
    }
}

/// One side's inset in a `@position-try` rule.
#[derive(Clone, Debug, PartialEq)]
pub enum TryInset {
    Auto,
    Len(Len),
    Anchor(AnchorRef),
}

/// One entry of `position-try-fallbacks`: a `@position-try` rule's insets,
/// margins and `position-anchor` over the box's own, then the flips.
///
/// Each `None` keeps what the box itself says on that side. A margin is in
/// pixels, or with `margin_pct` a fraction of the containing block's width,
/// as in CSS.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PositionTry {
    pub inset: [Option<TryInset>; 4],
    pub margin: [Option<f32>; 4],
    pub margin_pct: [Option<f32>; 4],
    pub position_anchor: Option<String>,
    /// `flip-block`: top and bottom trade places, insets, margins and the
    /// anchor edges they measure to.
    pub flip_block: bool,
    /// `flip-inline`: left and right do.
    pub flip_inline: bool,
    /// `flip-start`: top with left and bottom with right.
    pub flip_start: bool,
}

/// `position-try-order`: which of the places that fit wins. `MostWidth` is
/// also `most-inline-size`, and `MostHeight` `most-block-size`, since Rux
/// lays out horizontal text only.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TryOrder {
    #[default]
    Normal,
    MostWidth,
    MostHeight,
}

/// `content-visibility`, CSS's way of not paying for what is off screen.
///
/// A box whose contents are **skipped** is laid out with no children, at
/// its `contain-intrinsic-size` (plus its own padding and border), and draws
/// its own background and border but nothing inside it. `Auto` skips while
/// the box is far from the screen, `Hidden` always. Either way the box
/// clips what is inside it, as CSS's paint containment does.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ContentVisibility {
    #[default]
    Visible,
    Auto,
    Hidden,
}

/// One axis of `contain-intrinsic-size`: a length in px (`None` is
/// `none`, which is zero), and with `auto` the size the box was last drawn
/// at, once it has been.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct IntrinsicSize {
    pub auto: bool,
    pub len: Option<f32>,
}

/// A grid track size (`grid-template-columns`/`-rows`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Track {
    Px(f32),
    /// A fraction of the grid's own size on that axis.
    Pct(f32),
    Fr(f32),
    Auto,
    MinContent,
    MaxContent,
    /// `fit-content(<length>)`: as wide as its content, up to this.
    FitContent(f32),
    /// `minmax(min, max)`. Its whole point over a bare `1fr` is a `0` (or `px`)
    /// minimum, which lets the track shrink *below* its content's min-content,
    /// so a grid of fixed-size cards squeezes to fit instead of overflowing.
    MinMax(TrackSide, TrackSide),
}

/// One side of a `minmax()`, never itself a `minmax`. A `Fr` is only valid on
/// the max side (a flex minimum is meaningless), and degrades to `auto` if used
/// as a minimum.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TrackSide {
    Px(f32),
    Pct(f32),
    Fr(f32),
    Auto,
    MinContent,
    MaxContent,
}

/// One entry of a grid template: a track, or `repeat()` of some.
#[derive(Clone, Debug, PartialEq)]
pub enum GridTrack {
    One(Track),
    /// `repeat(count, tracks)`, with the names of the lines inside one
    /// repetition (one set per line, both edges included, or none at all).
    Repeat(Repeat, Vec<Track>, Vec<Vec<GridName>>),
}

/// How many times a `repeat()` repeats.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Repeat {
    Count(u16),
    /// `auto-fill`: as many as fit, empty ones kept.
    AutoFill,
    /// `auto-fit`: as many as fit, empty ones collapsed.
    AutoFit,
}

/// A grid line or area name, interned so a placement stays small and `Copy`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct GridName(u32);

impl GridName {
    pub fn new(name: &str) -> GridName {
        let mut t = grid_names().lock().expect("grid names");
        if let Some(&id) = t.1.get(name) {
            return GridName(id);
        }
        let id = t.0.len() as u32;
        t.0.push(name.to_string());
        t.1.insert(name.to_string(), id);
        GridName(id)
    }

    pub fn as_string(self) -> String {
        grid_names().lock().expect("grid names").0[self.0 as usize].clone()
    }
}

/// Every grid name met, by id, and the id of each. Never shrinks: there are
/// as many as a stylesheet writes.
fn grid_names() -> &'static std::sync::Mutex<(Vec<String>, HashMap<String, u32>)> {
    use std::sync::{Mutex, OnceLock};
    static NAMES: OnceLock<Mutex<(Vec<String>, HashMap<String, u32>)>> = OnceLock::new();
    NAMES.get_or_init(Default::default)
}

/// The names in a grid template: of its lines, between the entries of
/// `grid-template-columns` and `-rows` (one set more than entries, or none),
/// and `grid-template-areas`. Boxed in `Style`, as `anchor` is.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GridNames {
    pub columns: Vec<Vec<GridName>>,
    pub rows: Vec<Vec<GridName>>,
    pub areas: Option<GridAreas>,
}

/// `grid-template-areas`: each named area's rows and columns, as lines from
/// 1 (end exclusive), and the template's size.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GridAreas {
    pub areas: Vec<(GridName, u16, u16, u16, u16)>,
    pub rows: u16,
    pub columns: u16,
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

/// Horizontal text alignment within a text box (`text-align`). `Start` and
/// `End` follow the text's direction; `Left` and `Right` do not.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum TextAlign {
    #[default]
    Start,
    Center,
    End,
    Justify,
    Left,
    Right,
}

/// `direction`, set by the `dir` attribute or the CSS property, and inherited.
/// Which way a line of text runs, and so which side flex rows, grid columns
/// and `start`/`end` begin from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Direction {
    #[default]
    Ltr,
    Rtl,
}

impl Direction {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim() {
            "ltr" => Some(Direction::Ltr),
            "rtl" => Some(Direction::Rtl),
            _ => None,
        }
    }
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

/// What the layout keeps of a scroller's style until its region is made.
#[derive(Clone, Copy, Debug)]
struct ScrollFacts {
    id: NodeId,
    rtl: bool,
    axes: (bool, bool),
    contain: (bool, bool),
    bar: Scrollbar,
}

impl Style {
    /// Whether the box scrolls sideways: `overflow-x`, else `overflow`.
    pub fn scrolls_x(&self) -> bool {
        self.overflow_x.unwrap_or(self.overflow) == Overflow::Scroll
    }

    /// Whether the box scrolls up and down: `overflow-y`, else `overflow`.
    pub fn scrolls_y(&self) -> bool {
        self.overflow_y.unwrap_or(self.overflow) == Overflow::Scroll
    }
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

/// `touch-action`: which axes a finger may still scroll when it starts on this
/// element, and therefore who wins when a `@drag` and a scroller both want the
/// same gesture.
///
/// This exists because a `@drag` used to claim every finger that landed on it,
/// in any direction, which made a draggable row inside a scrolling list a dead
/// zone: a thumb that happened to start on it could not scroll the page at all.
/// Found on a phone, where scrolling is the primary gesture and the failure is
/// immediate.
///
/// The default is **not** "the element wins". It is CSS's `auto`, under which a
/// scroller keeps whichever axis it can actually scroll and the drag takes what
/// is left. An author who wants the element to have the finger outright says so,
/// exactly as on the web.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum TouchAction {
    /// The scroller keeps any axis it can scroll. A drag still starts on an
    /// axis nothing can scroll, which is what makes a horizontal carousel work
    /// inside a vertical page with no CSS at all.
    #[default]
    Auto,
    /// The element takes the finger whatever it does, and nothing scrolls.
    /// The opt-out, for a drag that genuinely owns both axes.
    None,
    /// The scroller keeps the horizontal axis only.
    PanX,
    /// The scroller keeps the vertical axis only.
    PanY,
}

impl TouchAction {
    /// Whether a scroller may take a gesture whose dominant axis is vertical
    /// (`vertical = true`) or horizontal.
    ///
    /// The question is asked once, at the moment the finger passes the slop
    /// threshold, because that is when the dominant axis first means anything
    /// and because no platform hands a gesture back once it has started.
    pub fn scroller_may_take(self, vertical: bool) -> bool {
        match self {
            TouchAction::Auto => true,
            TouchAction::None => false,
            TouchAction::PanX => !vertical,
            TouchAction::PanY => vertical,
        }
    }
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
    /// `z-index`, which CSS animates as an integer: it steps through every
    /// value between the two ends rather than jumping at the end.
    ZIndex,
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
        AnimProp::ZIndex,
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
            AnimProp::ZIndex => "z-index",
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
    /// A named line, the `n`th of that name (0 for the first), or an area's
    /// name, which means its `-start` or `-end` line.
    Named(GridName, i16),
    /// `span <name>`: to the `n`th line of that name (0 for the first).
    NamedSpan(GridName, u16),
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

/// `outline`: a line drawn outside the border box, over the element's own
/// content, taking no room in the layout.
///
/// It follows the box's `border-radius`, as it does in every current browser:
/// an outline `offset + width` outside a rounded corner is rounded by that much
/// more. A corner with no radius stays square, except in `auto`.
#[derive(Clone, Copy, Debug)]
pub struct Outline {
    /// Logical px. The line's thickness, drawn outward from `offset`.
    pub width: f32,
    /// Resolved: `currentColor` has already been replaced by the element's own
    /// `color`. `None` only for `auto` with no colour written, which is the
    /// focus ring's own colour.
    pub color: Option<Rgba>,
    /// `outline-offset`, logical px: the gap between the border edge and the
    /// line. Negative draws it inside the box.
    pub offset: f32,
    /// `outline-style: auto`, the platform's focus ring. It is what the default
    /// stylesheet gives a focused element, so an author's `outline` of any
    /// kind replaces it, and `outline: none` removes it, as in CSS.
    pub auto: bool,
}

impl Outline {
    /// The focus ring's colour when an `auto` outline names none.
    pub const RING: Rgba = Rgba { r: 0.54, g: 0.71, b: 0.98, a: 1.0 }; // #89b4fa
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
    pub grid_columns: Vec<GridTrack>,
    pub grid_rows: Vec<GridTrack>,
    /// The line names and areas of a grid template, `None` for almost every
    /// box.
    pub grid_names: Option<Box<GridNames>>,
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
    /// `vertical-align`, for an `<image>` or `<path>` sitting in a line of text.
    pub vertical_align: VerticalAlign,
    /// `direction`: which side rows, grid columns and the text start from.
    pub direction: Direction,
    pub padding: Sides,
    pub margin: Sides,
    /// `padding` and `margin` sides written as a **percentage**, in `Sides`
    /// order (top, right, bottom, left). `Some(10.0)` is `10%` of the
    /// containing block's width, as CSS has it for all four sides, resolved
    /// by the layout; it overrides that side's px. Watchlist #6.
    pub padding_pct: [Option<f32>; 4],
    pub margin_pct: [Option<f32>; 4],
    pub border: Sides,
    pub border_color: Option<Rgba>,
    pub gap: f32,
    /// `row-gap` / `column-gap` overrides for the shorthand `gap`. `None` keeps
    /// the shorthand (`gap`) value on that axis.
    pub row_gap: Option<f32>,
    pub column_gap: Option<f32>,
    /// The gap per axis, `(column, row)`, when written as a percentage of the
    /// box's own content size on that axis; overrides the px gap there.
    pub gap_pct: [Option<f32>; 2],
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
    /// What the box does with content past its edges, taken as a whole: it
    /// clips if either axis does, and is a scroller if either axis scrolls.
    pub overflow: Overflow,
    /// An axis that differs from `overflow`: `overflow-x: hidden;
    /// overflow-y: auto` is a scroller (`overflow`) that does not scroll
    /// sideways (`overflow_x`). `None` is `overflow`. See [`Style::scrolls_x`].
    pub overflow_x: Option<Overflow>,
    pub overflow_y: Option<Overflow>,
    /// `overscroll-behavior`, per axis (x, y): `contain` or `none` keeps a
    /// scroller's leftover movement from passing to the one around it.
    pub overscroll_contain: (bool, bool),
    pub scrollbar_width: ScrollbarWidth,
    /// `scrollbar-color`: thumb, then track. Boxed, as the rarely set
    /// fields of `Style` are, to keep it small (watchlist #41).
    pub scrollbar_color: Option<Box<(Rgba, Rgba)>>,
    pub background: Option<Background>,
    /// `border-radius`, per corner (top-left, top-right, bottom-right, bottom-left).
    pub radius: Corners,
    /// The same four corners when written as a **percentage**, which cannot be
    /// resolved here: the cascade runs before there is a box. `Some(50.0)` on a
    /// corner means "half the shorter side", worked out at paint time, and
    /// overrides that corner's `radius`.
    ///
    /// Rux draws circular corners only (see `parse_border_radius`), so there is
    /// one scalar per corner and a percentage resolves against the **shorter**
    /// side rather than per axis. On a square that is CSS's answer exactly; on
    /// an oblong CSS would give an ellipse, which this cannot represent, and
    /// half the shorter side is the pill an author reaching for `50%` wants.
    pub radius_pct: [Option<f32>; 4],
    /// `box-shadow` (single, outer). Drawn behind the box's own background.
    pub box_shadow: Option<BoxShadow>,
    /// `outline`. `None` for `outline-style: none`, the initial value, and for
    /// a width of zero.
    pub outline: Option<Outline>,
    /// `transform`: an affine applied to this box and its subtree at paint time.
    /// Visual only: hit regions are not transformed.
    pub transform: Option<Transform>,
    /// `cursor`: the pointer shape over this box.
    pub cursor: Cursor,
    /// `touch-action`: which axes a scroller may still take from a finger that
    /// starts here. Read only where a gesture handler exists, since it only
    /// arbitrates between a `@drag` and a scroller.
    pub touch_action: TouchAction,
    /// `position` and its `inset` (top, right, bottom, left). `None` per side =
    /// `auto`. Only meaningful when `position: absolute`.
    pub position: Position,
    pub inset: [Option<Len>; 4],
    /// `z-index`. `None` is `auto`. CSS's rule for when it counts: on a
    /// positioned box, and on a flex or grid item whatever its `position`. On
    /// anything else it is ignored, as a browser ignores it. See `Stack`.
    pub z_index: Option<i32>,
    /// `isolation: isolate`: a stacking context and nothing else.
    pub isolate: bool,
    /// Anchor positioning, `None` for almost every box.
    ///
    /// **Boxed on purpose.** Laid out flat it made every `Style` some 300
    /// bytes bigger, and a debug build keeps one per level of a deep
    /// recursion on the stack: `rux check` on a fresh project overflowed the
    /// main thread's (watchlist #41).
    pub anchor: Option<Box<AnchorStyle>>,
    /// `content-visibility`: whether this box's contents are laid out and
    /// drawn while it is off screen. See [`ContentVisibility`].
    pub content_visibility: ContentVisibility,
    /// `contain-intrinsic-size`, width then height: the size a box whose
    /// contents are skipped takes in their place. Boxed, as `anchor` is.
    pub contain_intrinsic: Option<Box<[IntrinsicSize; 2]>>,
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
            grid_names: None,
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
            vertical_align: VerticalAlign::Baseline,
            direction: Direction::Ltr,
            padding: Sides::default(),
            margin: Sides::default(),
            padding_pct: [None; 4],
            margin_pct: [None; 4],
            border: Sides::default(),
            border_color: None,
            gap: 0.0,
            row_gap: None,
            column_gap: None,
            gap_pct: [None; 2],
            axis: Axis::Row,
            justify: None,
            align: None,
            align_self: None,
            justify_self: None,
            justify_items: None,
            align_content: None,
            overflow: Overflow::Visible,
            overflow_x: None,
            overflow_y: None,
            overscroll_contain: (false, false),
            scrollbar_width: ScrollbarWidth::Auto,
            scrollbar_color: None,
            background: None,
            radius: [0.0; 4],
            radius_pct: [None; 4],
            box_shadow: None,
            outline: None,
            transform: None,
            cursor: Cursor::Default,
            touch_action: TouchAction::Auto,
            position: Position::Static,
            inset: [None; 4],
            z_index: None,
            isolate: false,
            anchor: None,
            content_visibility: ContentVisibility::Visible,
            contain_intrinsic: None,
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

/// How the painter gets an image's bytes.
///
/// `rux-paint` used to call `image::open(src)`, which is a filesystem read, and
/// that is the one thing an embedded build cannot do: `rux build --release`
/// puts the documents inside the executable, where a path names nothing. The
/// layout was right and the picture was an empty box.
///
/// It could not be fixed where it was. The runtime resolves everything else
/// through its `Source`, but `rux-paint` and `rux-runtime` share no dependency
/// edge, so the provider cannot reach the painter. Both depend on this crate,
/// which is why the hook lives here: it is the only place the two can meet.
///
/// The runtime installs a reader when it loads a document, so a path becomes a
/// key resolved the same way a component or a stylesheet is. Nothing installs
/// one in a bare `rux-layout` or `rux-paint` test, and the painter falls back
/// to reading the path, which is what it always did.
///
/// Thread-local and lazy, deliberately. The alternative was carrying the bytes
/// in `ImageContent`, which would read every image on every load whether or not
/// it is ever drawn, and clone them through every rebuild of the tree. The
/// painter already caches what it decodes, so it asks once.
type ImageReader = std::rc::Rc<dyn Fn(&str) -> Option<Vec<u8>>>;

thread_local! {
    static IMAGE_READER: std::cell::RefCell<Option<ImageReader>> =
        const { std::cell::RefCell::new(None) };
}

/// The bullet a masked character is painted as.
///
/// U+2022, which is what every platform uses and what a font is certain to
/// have. Not an asterisk: that reads as a footnote and as a required-field
/// marker.
pub const SECRET_BULLET: char = '\u{2022}';

/// Replace every character of a `type="password"` value with a bullet.
///
/// **By `char`, because that is what the caret counts.** A grapheme is the
/// better unit in the abstract, but the shell steps the caret and Backspace by
/// scalar, so a grapheme mask would paint one bullet where the caret has two
/// places to stand. A second way of counting inside one field is worse than a
/// coarse one used everywhere. Whether editing should be grapheme-aware is a
/// codebase-wide question, and when it is answered this follows it.
///
/// **Lives here because both sides of the split need it.** `rux-style` masks
/// what is painted; `rux-shell` has to measure the same string to put a caret
/// in it, and the two crates share only this one. Measuring the real text
/// while painting bullets is what put the caret a third of the way along a
/// password on a phone.
pub fn mask(text: &str) -> String {
    text.chars().map(|_| SECRET_BULLET).collect()
}

/// A byte offset into a value, as a byte offset into [`mask`] of that value.
///
/// One bullet per `char`, and a bullet is three bytes in UTF-8, so this is a
/// character count rather than anything to do with the original widths.
pub fn masked_offset(value: &str, byte: usize) -> usize {
    let byte = byte.min(value.len());
    let chars = value[..byte].chars().count();
    chars * SECRET_BULLET.len_utf8()
}

/// The inverse of [`masked_offset`]: a byte offset into a mask, back into the
/// value it was made from.
///
/// Needed because a tap lands on a bullet and has to become a caret in the real
/// text.
pub fn unmasked_offset(value: &str, masked_byte: usize) -> usize {
    let chars = masked_byte / SECRET_BULLET.len_utf8();
    value.char_indices().nth(chars).map(|(i, _)| i).unwrap_or(value.len())
}

/// The most pixels one image may decode to: 48 megapixels, a 50 MP phone
/// camera's photo with room to spare, and 192 MB once it is RGBA.
///
/// An image's header says how large it is before any pixel is decoded, and a
/// small file can claim to be enormous: a PNG of one colour compresses a
/// 30000 by 30000 image into a few hundred kilobytes, and decoding it takes
/// 3.6 GB. Harmless while every image is the author's own file; a way to end
/// the app once images come from the network. The `image` crate caps one
/// allocation at 512 MiB, but the conversion to RGBA after it is unbounded.
pub const MAX_IMAGE_PIXELS: u64 = 48_000_000;

/// The longest side one image may have, whatever its area. A GPU texture has a
/// ceiling too, 16384 on current desktops and often 8192 on phones.
pub const MAX_IMAGE_SIDE: u32 = 16_384;

/// Why an image of `width` by `height` will not be drawn, or `None` if it will.
pub fn image_too_large(width: u32, height: u32) -> Option<String> {
    if width > MAX_IMAGE_SIDE || height > MAX_IMAGE_SIDE {
        return Some(format!(
            "it is {width} by {height} pixels, and a side longer than {MAX_IMAGE_SIDE} is \
             refused; scale it down"
        ));
    }
    if u64::from(width) * u64::from(height) > MAX_IMAGE_PIXELS {
        return Some(format!(
            "it is {width} by {height} pixels, over the {} megapixel limit; scale it down",
            MAX_IMAGE_PIXELS / 1_000_000
        ));
    }
    None
}

/// Install the reader the painter asks for image bytes.
pub fn set_image_reader(reader: ImageReader) {
    IMAGE_READER.with(|r| *r.borrow_mut() = Some(reader));
}

/// Forget the reader, so the painter reads paths again.
pub fn clear_image_reader() {
    IMAGE_READER.with(|r| *r.borrow_mut() = None);
}

/// The bytes for `src`, or `None` when no reader is installed or it has none.
///
/// `None` and "installed but empty-handed" are deliberately the same answer:
/// the painter's fallback is to read the path, and a reader that cannot find a
/// file is in no better position than the painter to say why.
pub fn read_image_bytes(src: &str) -> Option<Vec<u8>> {
    // Cloned out of the cell before the call so a reader cannot find the cell
    // already borrowed.
    let reader = IMAGE_READER.with(|r| r.borrow().clone())?;
    reader(src)
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
    /// What `::selection` said for this text: the highlight behind the
    /// selected range and the colour of the glyphs inside it. `None` in either
    /// leaves that half to the platform, which is what CSS does when no rule
    /// names it.
    pub selection_style: SelectionStyle,
    /// The runs and inline boxes of a `<text>` with more than one run, `None`
    /// for plain text, which is nearly all of it. Shared, because a text is
    /// cloned into the frame's paint list every frame.
    pub rich: Option<std::sync::Arc<RichText>>,
    /// `white-space`. A plain text arrives collapsed already; a run of a
    /// sentence arrives as written, and is collapsed with the runs beside it
    /// when the layout joins them.
    pub white_space: WhiteSpace,
    /// The paragraph's base direction, which decides where its lines start and
    /// how text of both directions in it is ordered. `None` takes it from the
    /// first letter with a direction, which is `dir="auto"`, and what a text
    /// field does.
    pub direction: Option<Direction>,
    /// For a run of a sentence with a `dir` of its own: its words are set
    /// apart from the sentence's, as HTML sets apart an inline element with
    /// `dir` (`unicode-bidi: isolate`). See [`Isolate`].
    pub isolate: Isolate,
}

/// How a run's words stand in the sentence's bidirectional order. An
/// isolated run is ordered on its own, in its own direction, and is one
/// neutral piece to the words around it: a Hebrew name in an English sentence
/// cannot drag the comma after it to the wrong end.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Isolate {
    #[default]
    None,
    Ltr,
    Rtl,
    /// `dir="auto"`: the run's first letter with a direction decides.
    Auto,
}

impl Isolate {
    /// The Unicode isolate control that opens it, `None` for no isolation.
    /// Closed by U+2069 POP DIRECTIONAL ISOLATE.
    pub fn opener(self) -> Option<&'static str> {
        match self {
            Isolate::None => None,
            Isolate::Ltr => Some("\u{2066}"),
            Isolate::Rtl => Some("\u{2067}"),
            Isolate::Auto => Some("\u{2068}"),
        }
    }
}

/// What closes an isolate.
pub const POP_ISOLATE: &str = "\u{2069}";

/// How a field's selection is drawn: the two properties `::selection`
/// honours, and the colour of the handles a finger drags.
///
/// `::selection` gets a background and a text colour and nothing else, which
/// matches what browsers apply to a highlight: nothing that would move the
/// text.
#[derive(Clone, Copy, Debug, Default)]
pub struct SelectionStyle {
    pub background: Option<Rgba>,
    pub color: Option<Rgba>,
    /// The field's own `accent-color`, which CSS defines as the accent of the
    /// controls an element generates. Selection handles are exactly that.
    /// `None` leaves them the platform's accent.
    pub handle: Option<Rgba>,
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
    /// `type="switch"`: on or off, announced as a switch rather than a box.
    Switch,
    /// `type="slider"`: a number chosen along a track.
    Slider,
    /// `type="date"`.
    DateInput,
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

/// Which text field an `<input>` is, from its `type=`.
///
/// **One value, not a bool per type.** It was three bools (`multiline`,
/// `secret`, `search`), each added on its own, and three bools make eight
/// states of which four were ever chosen: nothing stopped a field from being a
/// password *and* a textarea, and the shell's answer to that depended on which
/// `if` it happened to check first. The spec's list of types grows by several
/// in the same release (number, and the keyboard hints beside it), which is the
/// point past which the combinations stop being hypothetical.
///
/// Only the fields a person types into. A `type="select"` is told apart by its
/// `options`, and becomes a region of its own in the layout; for one of those
/// this reads `Text` and nothing consults it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum InputKind {
    /// A one-line field, the default.
    #[default]
    Text,
    /// `type="textarea"`: wraps, scrolls, and `Enter` inserts a newline.
    Textarea,
    /// `type="password"`: the shown text is bullets and the clipboard is
    /// refused. The bound signal still holds the real value.
    Password,
    /// `type="search"`: only the keyboard differs, a Search key for its action.
    Search,
    /// `type="number"`: the bound signal holds a number, never text.
    ///
    /// While what is typed is not yet a number (`-`, `1.`, nothing at all)
    /// the signal keeps the last number it had, and the half-typed text lives
    /// only in the field, the way a composition does. Nothing typed is
    /// refused: a keystroke that vanishes reads as a broken keyboard.
    Number,
    /// `type="date"`: the bound signal holds `YYYY-MM-DD`, as HTML's does, or
    /// nothing. A phone opens the platform's date picker; elsewhere it is
    /// typed, with the same rule as a number: only a real date reaches the
    /// signal.
    Date,
}

impl InputKind {
    /// The kind a `type=` attribute names. Anything that is not a text field of
    /// its own, `select` and an absent `type` included, is `Text`.
    pub fn from_type(ty: Option<&str>) -> Self {
        match ty {
            Some("textarea") => Self::Textarea,
            Some("password") => Self::Password,
            Some("search") => Self::Search,
            Some("number") => Self::Number,
            Some("date") => Self::Date,
            _ => Self::Text,
        }
    }

    /// Whether the text runs over several lines, and `Enter` breaks one.
    pub fn multiline(self) -> bool {
        self == Self::Textarea
    }

    /// Whether the text is masked, and may not leave by the clipboard.
    pub fn secret(self) -> bool {
        self == Self::Password
    }

    /// Whether the field's signal holds something narrower than text, so
    /// that what is typed reaches it only once it is one: a number, a date.
    pub fn typed(self) -> bool {
        matches!(self, Self::Number | Self::Date)
    }
}

/// A calendar date written `YYYY-MM-DD`, as HTML writes one: `(year, month,
/// day)` if it is a day that exists, month and day counting from 1.
///
/// One or two digits are taken for the month and day, so a date typed by hand
/// as `2026-9-3` is still a date; [`format_date`] writes it back padded.
pub fn parse_date(text: &str) -> Option<(i32, u32, u32)> {
    let mut parts = text.trim().split('-');
    let (y, m, d) = (parts.next()?, parts.next()?, parts.next()?);
    let digits = |s: &str, min: usize, max: usize| {
        (min..=max).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_digit())
    };
    if parts.next().is_some() || !digits(y, 4, 4) || !digits(m, 1, 2) || !digits(d, 1, 2) {
        return None;
    }
    let (year, month, day): (i32, u32, u32) = (y.parse().ok()?, m.parse().ok()?, d.parse().ok()?);
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return None,
    };
    (year >= 1 && (1..=days).contains(&day)).then_some((year, month, day))
}

/// A date as `YYYY-MM-DD`, the inverse of [`parse_date`].
pub fn format_date((year, month, day): (i32, u32, u32)) -> String {
    format!("{year:04}-{month:02}-{day:02}")
}

/// Which keyboard a text field asks for: `inputmode=`, named and valued as
/// HTML names it.
///
/// **A keyboard, not a control.** An email field is a text field that happens
/// to want an `@` key, and nothing about its value, binding or rendering is
/// different. That is why these are not `type=` values: a type is a different
/// control, and claiming `email` was one would promise a distinction that
/// exists nowhere but the keyboard. Phone numbers, card numbers and one-time
/// codes are digit *strings*, which is `inputmode="numeric"` on a text field
/// and never a `number`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Keyboard {
    /// The platform's ordinary text keyboard, the default.
    #[default]
    Text,
    /// Digits only: PINs, codes, card numbers.
    Numeric,
    /// Digits and the locale's decimal separator.
    Decimal,
    /// A telephone keypad.
    Tel,
    /// A text keyboard with `@` and `.` to hand.
    Email,
    /// A text keyboard with `/` and `.` to hand.
    Url,
    /// A text keyboard whose action key searches.
    Search,
    /// No on-screen keyboard at all: the app draws its own keys.
    None,
}

impl Keyboard {
    /// Every value `inputmode=` accepts, for the error that names them.
    pub const NAMES: [&'static str; 8] =
        ["text", "numeric", "decimal", "tel", "email", "url", "search", "none"];

    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "text" => Self::Text,
            "numeric" => Self::Numeric,
            "decimal" => Self::Decimal,
            "tel" => Self::Tel,
            "email" => Self::Email,
            "url" => Self::Url,
            "search" => Self::Search,
            "none" => Self::None,
            _ => return None,
        })
    }
    /// The `inputmode` value that names it, the inverse of [`Self::parse`].
    pub fn name(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Numeric => "numeric",
            Self::Decimal => "decimal",
            Self::Tel => "tel",
            Self::Email => "email",
            Self::Url => "url",
            Self::Search => "search",
            Self::None => "none",
        }
    }
}

/// What the keyboard's action key says: `enterkeyhint=`, as HTML names it.
///
/// A label, and for two of them a behaviour: `next` and `previous` move focus
/// the way Tab does, and `done` closes the field. The rest only name what the
/// app is about to do with Enter, which is the app's business.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EnterKey {
    /// Whatever the platform shows when nobody says.
    #[default]
    Default,
    Enter,
    Done,
    Go,
    Next,
    Previous,
    Search,
    Send,
}

impl EnterKey {
    /// Every value `enterkeyhint=` accepts, for the error that names them.
    pub const NAMES: [&'static str; 7] =
        ["enter", "done", "go", "next", "previous", "search", "send"];

    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "enter" => Self::Enter,
            "done" => Self::Done,
            "go" => Self::Go,
            "next" => Self::Next,
            "previous" => Self::Previous,
            "search" => Self::Search,
            "send" => Self::Send,
            _ => return None,
        })
    }
    /// The `enterkeyhint` value that names it; `None` when it was not given.
    pub fn name(self) -> Option<&'static str> {
        Some(match self {
            Self::Default => return None,
            Self::Enter => "enter",
            Self::Done => "done",
            Self::Go => "go",
            Self::Next => "next",
            Self::Previous => "previous",
            Self::Search => "search",
            Self::Send => "send",
        })
    }
}

/// What an `<input>` says about itself beyond its type: the attributes that
/// constrain editing, what its keyboard looks like, and who to tell when
/// something happens to it.
///
/// Carried from the build to the shell on the field's [`FocusRegion`], because
/// the shell is where every one of them is enforced: it owns the keystrokes,
/// the focus and the input method. Default is an ordinary editable field that
/// tells nobody anything.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Field {
    /// `disabled`: cannot be focused, tapped or edited. A disabled field has no
    /// focus region at all, so the shell never sees one.
    pub disabled: bool,
    /// `readonly`: can be focused, selected and copied, but not changed, and
    /// raises no keyboard.
    pub readonly: bool,
    /// `maxlength`: the most text the field accepts, in UTF-16 code units as
    /// HTML counts them. Typing past it is refused; a paste is cut short.
    pub maxlength: Option<usize>,
    /// `inputmode`.
    pub keyboard: Keyboard,
    /// `enterkeyhint`.
    pub enter_key: EnterKey,
    /// `autofocus`: takes focus when it first appears, if nothing has it.
    pub autofocus: bool,
    /// `@input`: after every change to the text, however it was made.
    pub on_input: Option<String>,
    /// `@change`: when a changed value is committed, by leaving the field or
    /// by Enter in a one-line field.
    pub on_change: Option<String>,
    /// `@focus`.
    pub on_focus: Option<String>,
    /// `@blur`.
    pub on_blur: Option<String>,
    /// A date field's earliest and latest day, `YYYY-MM-DD`. A day outside
    /// them is not offered by the picker and does not reach the signal.
    pub min: Option<(i32, u32, u32)>,
    pub max: Option<(i32, u32, u32)>,
    /// `name=`: the key its value is sent under in a form's `event.values`.
    /// Without one, the `r-model` is.
    pub name: Option<String>,
    /// The tree path of the `role="form"` around it, the nearest one. `None`
    /// for a field in no form.
    pub form: Option<Vec<usize>>,
    /// What a submission checks. See [`Checks`].
    pub checks: Checks,
    /// The `r-model` a submission reads the value from. Set on every kind of
    /// input, the tap-toggles included, which carry no `model` of their own.
    pub bind: Option<String>,
    /// What kind of value that is. See [`Shape`].
    pub shape: Shape,
    /// `autocomplete=`: what the field holds, for the platform's autofill,
    /// as HTML's tokens. `off` keeps the field away from autofill entirely.
    pub autocomplete: Option<String>,
}

/// HTML's `autocomplete` field names Rux passes on, with the Android autofill
/// hint each one becomes. Android's own names where the platform has one,
/// and AndroidX's `HintConstants` where only it does; autofill services read
/// both.
pub const AUTOCOMPLETE: &[(&str, &str)] = &[
    ("username", "username"),
    ("current-password", "password"),
    ("new-password", "newPassword"),
    ("email", "emailAddress"),
    ("tel", "phone"),
    ("tel-national", "phoneNational"),
    ("name", "name"),
    ("given-name", "personGivenName"),
    ("family-name", "personFamilyName"),
    ("street-address", "postalAddress"),
    ("address-line1", "streetAddress"),
    ("address-level1", "addressRegion"),
    ("address-level2", "addressLocality"),
    ("country-name", "addressCountry"),
    ("postal-code", "postalCode"),
    ("cc-name", "personName"),
    ("cc-number", "creditCardNumber"),
    ("cc-exp", "creditCardExpirationDate"),
    ("cc-exp-month", "creditCardExpirationMonth"),
    ("cc-exp-year", "creditCardExpirationYear"),
    ("cc-csc", "creditCardSecurityCode"),
    ("bday", "birthDateFull"),
    ("one-time-code", "smsOTPCode"),
];

/// Tokens that may come before a field name and change nothing on a phone:
/// which address, which phone. `on` says only that autofill is welcome.
pub const AUTOCOMPLETE_MODIFIERS: &[&str] =
    &["on", "shipping", "billing", "home", "work", "mobile", "fax", "pager"];

/// The Android autofill hints for a field, from its `autocomplete` or, with
/// none, from what it is: a password, or an email or phone keyboard.
/// Empty for `autocomplete="off"`, which [`Field::autofill`] also refuses.
pub fn autofill_hints(field: &Field, kind: InputKind) -> Vec<&'static str> {
    if let Some(tokens) = &field.autocomplete {
        let hints: Vec<&'static str> = tokens
            .split_whitespace()
            .filter_map(|t| AUTOCOMPLETE.iter().find(|(name, _)| name.eq_ignore_ascii_case(t)))
            .map(|(_, hint)| *hint)
            .collect();
        if !hints.is_empty() || tokens.trim().eq_ignore_ascii_case("off") {
            return hints;
        }
    }
    match (kind, field.keyboard) {
        (InputKind::Password, _) => vec!["password"],
        (_, Keyboard::Email) => vec!["emailAddress"],
        (_, Keyboard::Tel) => vec!["phone"],
        _ => Vec::new(),
    }
}

impl Field {
    /// Whether the platform's autofill may see and fill this field: not
    /// `autocomplete="off"`, and something a person could type into.
    pub fn autofill(&self) -> bool {
        !self.disabled
            && !self.readonly
            && !self.autocomplete.as_deref().is_some_and(|a| a.trim().eq_ignore_ascii_case("off"))
    }
}

/// What kind of value a field holds, which decides how its checks read it and
/// what `event.values` carries for it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Shape {
    /// Text, as every text field, a select and a radio group hold.
    #[default]
    Text,
    /// On or off: a checkbox or a switch.
    Flag,
    /// A number field or a slider.
    Number,
}

/// The constraints a field states about its value, checked when its form is
/// submitted and matched by `:invalid` once the person has been through it.
///
/// An empty field fails only `required`: every other check is about a value
/// that is there, as HTML has it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Checks {
    /// `required`: not empty; for a checkbox or switch, on.
    pub required: bool,
    /// `minlength`, in UTF-16 code units like `maxlength`.
    pub minlength: Option<usize>,
    /// `pattern`: a regular expression the *whole* value matches.
    pub pattern: Option<String>,
    /// A number field's `min` and `max`.
    pub low: Option<f64>,
    pub high: Option<f64>,
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
    /// `@key`: runs on a key press while focus is on this element or inside
    /// it. See the shell's `dispatch_key`.
    pub on_key: Option<String>,
    /// `r-model` signal name for `<input>` nodes (focus target + edit binding).
    pub model: Option<String>,
    /// Which text field an `<input>` is. See [`InputKind`].
    pub kind: InputKind,
    /// An `<input>`'s attributes and handlers. See [`Field`].
    pub field: Field,
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
    /// This node's tree path, set on anything keyboard focus can land on that
    /// is not a text field (a `@tap` box, a select). The layout hands it to
    /// the [`FocusItem`], and the shell hands it back as the focused path, so
    /// `:focus` can match an element that has no `r-model` to be known by.
    pub focus_path: Option<Vec<usize>>,
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
    /// `role="form"`: what submitting this form runs. See [`Form`]. Layout
    /// ignores it; the runtime finds it by the node's path.
    pub form: Option<Form>,
}

/// A `<view role="form">`: the handlers its submission runs.
///
/// The role only says which fields belong together. What happens when they
/// are sent is the `@submit` the author wrote, and a submission whose fields
/// fail their checks runs `@invalid` instead, as HTML refuses to submit.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Form {
    /// `@submit`, handed `event.values`.
    pub on_submit: Option<String>,
    /// `@invalid`, handed `event.errors` and `event.values`.
    pub on_invalid: Option<String>,
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
            on_key: None,
            model: None,
            kind: InputKind::Text,
            field: Field::default(),
            options: None,
            hidden: false,
            id: None,
            label_for: None,
            focus_model: None,
            state_path: None,
            focus_path: None,
            access: Access::default(),
            instance: None,
            key: None,
            form: None,
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
            on_key: None,
            model: None,
            kind: InputKind::Text,
            field: Field::default(),
            options: None,
            hidden: false,
            id: None,
            label_for: None,
            focus_model: None,
            state_path: None,
            focus_path: None,
            access: Access::default(),
            instance: None,
            key: None,
            form: None,
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
            on_key: None,
            model: None,
            kind: InputKind::Text,
            field: Field::default(),
            options: None,
            hidden: false,
            id: None,
            label_for: None,
            focus_model: None,
            state_path: None,
            focus_path: None,
            access: Access::default(),
            instance: None,
            key: None,
            form: None,
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
            on_key: None,
            model: None,
            kind: InputKind::Text,
            field: Field::default(),
            options: None,
            hidden: false,
            id: None,
            label_for: None,
            focus_model: None,
            state_path: None,
            focus_path: None,
            access: Access::default(),
            instance: None,
            key: None,
            form: None,
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
    /// Border width **per side** (all zero = none).
    ///
    /// This used to be one `f32` filled from `style.border.top`, so the three
    /// other sides were computed by the cascade and then thrown away here. A
    /// `border-bottom: 6px` drew nothing at all, and a `border-top: 6px` drew a
    /// box on all four sides. Both were silent, and `border-bottom` is offered
    /// by the editor's completion list, which is supposed to mean it works.
    pub border: Sides,
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
        // A right-to-left box's content ends at its right edge, plus the offset.
        let cx = if s.rtl { s.x + s.width + off.x - s.content_width } else { s.x - off.x };
        let cy = s.y - off.y;
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
    /// Scrollbars are drawn *outside* the paint list, over the content,
    /// because a scroller clips its own children and would eat them.
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
    /// The id of the scroller this one sits inside, if any. With
    /// [`FocusItem::scroll`] it is the chain of scrollers that hold an
    /// element, which is the only chain that should move to show it.
    pub within: Option<usize>,
    /// Whether its content starts at the right (`direction: rtl`). Its offset
    /// is still how far it has scrolled from its start, so a larger `x` shows
    /// more of what lies to the left; whatever moves it by the pointer or the
    /// keyboard turns the sign round on that axis, and its thumb starts at the
    /// right.
    pub rtl: bool,
    /// Per axis (x, y), whether movement it cannot take stays with it rather
    /// than passing to the scroller around it: `overscroll-behavior: contain`
    /// or `none`.
    pub contain: (bool, bool),
    /// How its scrollbars are drawn: `scrollbar-width` and `scrollbar-color`.
    pub bar: Scrollbar,
}

/// `scrollbar-width` and `scrollbar-color` for one scroller.
#[derive(Clone, Copy, Debug, Default)]
pub struct Scrollbar {
    pub width: ScrollbarWidth,
    /// The thumb's colour and the track's, as `scrollbar-color` writes them.
    /// `None` is `auto`: the shell's own.
    pub color: Option<(Rgba, Rgba)>,
}

/// `scrollbar-width`: `auto`, `thin`, or `none`, which draws no bar and still
/// scrolls.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ScrollbarWidth {
    #[default]
    Auto,
    Thin,
    None,
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
    /// The `touch-action` for this region, carried for the same reason as
    /// `cursor`: the shell arbitrates the gesture against the geometry it
    /// already hit-tests, and the answer has to travel with it.
    pub touch_action: TouchAction,
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
    /// The component instance this input was written in, when it is inside one.
    ///
    /// The third part of its identity, and the one that says *where the model
    /// means anything*. An `r-model` inside a component names that instance's
    /// own state, which the document knows nothing about: read it in the
    /// document's scope and the name resolves to nothing, so the field shows
    /// empty and every keystroke is written somewhere nobody reads.
    pub instance: Option<String>,
    /// The input's text box (its laid-out child). The shell needs it to turn a
    /// click into a caret position.
    pub text: Option<PaintText>,
    /// Which text field this is. See [`InputKind`].
    pub kind: InputKind,
    /// Its attributes and handlers. See [`Field`].
    pub field: Field,
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
    /// The component instance this select was written in. Same reason as
    /// [`FocusRegion::instance`]: choosing an option writes the model, and a
    /// write in the wrong scope lands nowhere.
    pub instance: Option<String>,
    pub options: Vec<String>,
    /// Its attributes; the shell reads `@change` off it. See [`Field`].
    pub field: Field,
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

/// One keyboard-focusable element, in document (Tab) order. Carries its box,
/// for a tap to find it and for scrolling it into view, and how the shell
/// should act on it. Its focus ring is not drawn from here: it is the
/// element's own `outline`, painted with the element.
#[derive(Clone, Debug)]
pub struct FocusItem {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub kind: FocusKind,
    /// The element's tree path, for anything that is not a text field. See
    /// [`Node::focus_path`]. A text field is known by its model instead.
    pub path: Option<Vec<usize>>,
    /// The innermost scroller this element sits in, as an index into
    /// [`Layout::scrolls`]. A scroller that is itself focusable is held by its
    /// parent, not by itself. See [`ScrollRegion::within`].
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
    Text {
        model: String,
        row: Option<String>,
        /// The component instance the input was written in: the scope its model
        /// is read and written in. See [`FocusRegion::instance`].
        instance: Option<String>,
        /// Which text field. See [`InputKind`].
        kind: InputKind,
        text: Option<PaintText>,
    },
    /// A button / checkbox / radio: Space or Enter runs its handler.
    Activate { on_tap: String, instance: Option<String> },
    /// A select: Space or Enter opens its dropdown.
    Select {
        model: String,
        row: Option<String>,
        /// The component instance the select was written in.
        instance: Option<String>,
        options: Vec<String>,
    },
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
    /// Anchor names an `anchor()` asked for that nothing on screen has, for
    /// the shell to warn about. An empty name is an `anchor()` that names no
    /// anchor on a box with no `position-anchor`.
    pub anchor_misses: Vec<String>,
    /// Each anchored box drawn at one of its `position-try-fallbacks` rather
    /// than its own place: its path, which fallback, from 1, and its
    /// `position-anchor`. Kept by [`LayoutState`] so the next frame tries
    /// that one first, as CSS does, until the box is anchored elsewhere.
    pub position_tries: Vec<(Vec<usize>, u8, Option<String>)>,
    /// Every box with `content-visibility` other than `visible`, as this
    /// frame found it. Kept by [`LayoutState`] to decide next frame which to
    /// skip, and what size a skipped one takes.
    pub contained: Vec<Contained>,
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
/// How far a box's content reaches, from its border box's top-left corner and
/// out through its far border: what a scroller scrolls across.
///
/// Taffy's scrollable overflow rectangle is measured from the padding box and
/// takes in the end padding (css-overflow-3), so only the borders are added.
fn scroll_extent(layout: &taffy::Layout, rtl: bool) -> (f32, f32) {
    let (rect, border) = (layout.scrollable_overflow_rect, layout.border);
    // A right-to-left box overflows to its left, past its own origin: that is
    // where the content it scrolls to lies, so it counts as far as it goes.
    let left = if rtl { rect.left.min(0.0) } else { 0.0 };
    (border.left + rect.right - left + border.right, border.top + rect.bottom + border.bottom)
}

fn content_box(layout: &taffy::Layout) -> (f32, f32, f32, f32) {
    let (p, b) = (layout.padding, layout.border);
    (
        p.left + b.left,
        p.top + b.top,
        (layout.size.width - p.left - p.right - b.left - b.right).max(0.0),
        (layout.size.height - p.top - p.bottom - b.top - b.bottom).max(0.0),
    )
}

/// Callback that measures a text block to `(width, height)` given an optional
/// max width. Takes the whole [`TextContent`] so new text properties (family,
/// spacing, style…) don't each widen this signature.
pub type Measure<'a> = dyn FnMut(&TextContent, Option<f32>) -> (f32, f32) + 'a;

/// What the layout asks the text engine, which it does not own: see the crate
/// doc. A [`Measure`] closure answers only the size, which is all a document
/// without inline runs needs; the shell's engine answers
/// [`TextMeasure::inline`] as well, through [`LayoutState::lay_out_text`].
pub trait TextMeasure {
    /// A text block's `(width, height)` given an optional max width. Takes the
    /// whole [`TextContent`] so new text properties (family, spacing, style…)
    /// don't each widen this signature.
    fn measure(&mut self, text: &TextContent, max_width: Option<f32>) -> (f32, f32);

    /// Where the parts of a sentence landed, laid out `width` wide: see
    /// [`InlineGeometry`]. Asked only of a text with [`TextContent::rich`],
    /// on every frame, so an engine that keeps the answer hands out a share
    /// of it rather than a copy.
    fn inline(&mut self, text: &TextContent, width: f32) -> std::sync::Arc<InlineGeometry> {
        let _ = (text, width);
        std::sync::Arc::default()
    }
}

/// A [`Measure`] closure as a [`TextMeasure`] that knows only sizes.
struct SizeOnly<'m, 'a>(&'m mut Measure<'a>);

impl TextMeasure for SizeOnly<'_, '_> {
    fn measure(&mut self, text: &TextContent, max_width: Option<f32>) -> (f32, f32) {
        (self.0)(text, max_width)
    }
}

/// Where the parts of a sentence are, from its content box's top-left: each
/// inline box's `(x, y)` in the order of [`RichText::items`], and the
/// rectangles `(x, y, width, height)` each run covers, one per line it is on,
/// in the order of [`RichText::runs`]. Empty when the engine was not asked or
/// cannot say, which leaves boxes at the text's corner and runs untappable
/// rather than anywhere wrong.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InlineGeometry {
    pub items: Vec<(f32, f32)>,
    pub runs: Vec<Vec<(f32, f32, f32, f32)>>,
}

/// Turn any percentage corners into pixels, now that the box has a size.
///
/// A percentage is resolved against the **shorter side**, because Rux draws
/// circular corners: there is one scalar per corner, so there is no way to say
/// "half the width horizontally and half the height vertically", which is what
/// CSS's `50%` means on an oblong. On a square the two answers are identical.
/// On an oblong CSS draws an ellipse and this draws a pill, which is what
/// somebody writing `border-radius: 50%` on a button is after.
///
/// The value is not clamped here. A radius wider than the box is already
/// clamped by the rounded-rect builder, which is the same path that makes
/// `9999px` a reliable way to say "as round as it goes", and clamping twice
/// would be one place too many to keep in agreement.
fn resolve_radius(radius: Corners, pct: [Option<f32>; 4], width: f32, height: f32) -> Corners {
    if pct.iter().all(Option::is_none) {
        return radius;
    }
    let shorter = width.min(height).max(0.0);
    let mut out = radius;
    for (i, p) in pct.iter().enumerate() {
        if let Some(p) = p {
            out[i] = shorter * p / 100.0;
        }
    }
    out
}

/// What each taffy node paints.
enum PaintKind {
    Box {
        bg: Option<Background>,
        radius: Corners,
        /// Percentage corners, resolved against this box once it has a size.
        radius_pct: [Option<f32>; 4],
        border: Sides,
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
    /// Drawn after the node's subtree, not with its box. See [`Outline`].
    Outline(Outline),
    /// The elements of a sentence, placed against its glyphs once the text
    /// engine says where they landed. Follows the node's `Text`.
    Inline(std::sync::Arc<Vec<inline::InlinePart>>),
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

fn to_dim<T: FromLength + FromPercent>(l: Len, vp: (f32, f32)) -> T {
    match l {
        Len::Px(v) => length(v),
        Len::Pct(p) => percent(p),
        Len::Vw(v) => length(vp.0 * v / 100.0),
        Len::Vh(v) => length(vp.1 * v / 100.0),
        Len::Vmin(v) => length(vp.0.min(vp.1) * v / 100.0),
        Len::Vmax(v) => length(vp.0.max(vp.1) * v / 100.0),
    }
}

fn to_placement(p: GridPlace) -> GridPlacement {
    match p {
        GridPlace::Auto => auto(),
        GridPlace::Line(i) => line(i),
        GridPlace::Span(n) => span(n),
        GridPlace::Named(name, n) => GridPlacement::NamedLine(name.as_string(), n),
        GridPlace::NamedSpan(name, n) => GridPlacement::NamedSpan(name.as_string(), n),
    }
}

fn to_track(t: Track) -> TrackSizingFunction {
    let min = |s: TrackSide| match s {
        TrackSide::Px(v) => MinTrackSizingFunction::length(v),
        TrackSide::Pct(p) => MinTrackSizingFunction::percent(p),
        // A flex minimum is invalid; fall back to `auto` (min-content).
        TrackSide::Fr(_) | TrackSide::Auto => MinTrackSizingFunction::auto(),
        TrackSide::MinContent => MinTrackSizingFunction::min_content(),
        TrackSide::MaxContent => MinTrackSizingFunction::max_content(),
    };
    let max = |s: TrackSide| match s {
        TrackSide::Px(v) => MaxTrackSizingFunction::length(v),
        TrackSide::Pct(p) => MaxTrackSizingFunction::percent(p),
        TrackSide::Fr(f) => MaxTrackSizingFunction::fr(f),
        TrackSide::Auto => MaxTrackSizingFunction::auto(),
        TrackSide::MinContent => MaxTrackSizingFunction::min_content(),
        TrackSide::MaxContent => MaxTrackSizingFunction::max_content(),
    };
    match t {
        Track::Px(v) => length(v),
        Track::Pct(p) => percent(p),
        Track::Fr(f) => fr(f),
        Track::Auto => auto(),
        Track::MinContent => min_content(),
        Track::MaxContent => max_content(),
        Track::FitContent(px) => fit_content(LengthPercentage::length(px)),
        Track::MinMax(lo, hi) => minmax(min(lo), max(hi)),
    }
}

fn to_component(t: &GridTrack) -> taffy::GridTemplateComponent<String> {
    match t {
        GridTrack::One(t) => to_track(*t).into(),
        GridTrack::Repeat(count, tracks, names) => taffy::GridTemplateComponent::Repeat(taffy::GridTemplateRepetition {
            count: match count {
                Repeat::Count(n) => taffy::RepetitionCount::Count(*n),
                Repeat::AutoFill => taffy::RepetitionCount::AutoFill,
                Repeat::AutoFit => taffy::RepetitionCount::AutoFit,
            },
            tracks: tracks.iter().copied().map(to_track).collect(),
            line_names: names.iter().map(|set| set.iter().map(|n| n.as_string()).collect()).collect(),
        }),
    }
}

/// Line names as taffy takes them: owned strings.
fn to_names(names: &[Vec<GridName>]) -> Vec<Vec<String>> {
    names.iter().map(|set| set.iter().map(|n| n.as_string()).collect()).collect()
}

/// Like [`to_track`] but for `grid-auto-rows`/`-columns`, whose tracks can't hold
/// a `repeat(…)` and so use taffy's non-repeated track type.
fn to_taffy(style: &Style, vp: (f32, f32)) -> taffy::Style {
    taffy::Style {
        // Taffy mirrors flex rows, grid columns and a block's start side.
        direction: match style.direction {
            Direction::Ltr => taffy::Direction::Ltr,
            Direction::Rtl => taffy::Direction::Rtl,
        },
        display: match style.display {
            // Inline is a normal (block) box; the hug comes from width:auto plus
            // not stretching (taffy has no true inline flow).
            Display::Block | Display::Inline => taffy::Display::Block,
            Display::Flex => taffy::Display::Flex,
            Display::Grid => taffy::Display::Grid,
            Display::None => taffy::Display::None,
        },
        grid_template_columns: style.grid_columns.iter().map(to_component).collect(),
        grid_template_rows: style.grid_rows.iter().map(to_component).collect(),
        grid_template_column_names: style.grid_names.as_ref().map(|n| to_names(&n.columns)).unwrap_or_default(),
        grid_template_row_names: style.grid_names.as_ref().map(|n| to_names(&n.rows)).unwrap_or_default(),
        grid_template_areas: style.grid_names.as_ref().and_then(|n| n.areas.as_ref()).map(|a| taffy::GridTemplateAreas {
            areas: a
                .areas
                .iter()
                .map(|&(name, row_start, row_end, column_start, column_end)| taffy::GridTemplateArea {
                    name: name.as_string(),
                    row_start,
                    row_end,
                    column_start,
                    column_end,
                })
                .collect(),
            row_count: a.rows,
            column_count: a.columns,
        }),
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
        grid_auto_rows: style.grid_auto_rows.iter().copied().map(to_track).collect(),
        grid_auto_columns: style.grid_auto_columns.iter().copied().map(to_track).collect(),
        flex_direction: match style.axis {
            Axis::Column => FlexDirection::Column,
            Axis::Row => FlexDirection::Row,
        },
        // Unset is `normal`, which taffy 0.15 names rather than spelling `None`.
        justify_content: style.justify.map(to_align_content).unwrap_or(JustifyContent::NORMAL),
        align_items: style.align.map(to_align_items).unwrap_or(AlignItems::NORMAL),
        align_self: style.align_self.map(to_align_items),
        justify_self: style.justify_self.map(to_align_items),
        justify_items: style.justify_items.map(to_align_items).unwrap_or(AlignItems::NORMAL),
        align_content: style.align_content.map(to_align_content).unwrap_or(AlignContent::NORMAL),
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
            // An axis placed by `anchor()` is held at its containing block's
            // start edge for the layout, which only has to find the box's
            // size, and is moved against the anchor at paint time.
            let a = |i: usize| style.anchor.as_ref().is_some_and(|a| a.inset[i].is_some());
            let (x_anchored, y_anchored) = (a(1) || a(3), a(0) || a(2));
            let held = Some(Len::Px(0.0));
            Rect {
                left: to_inset(if x_anchored { held } else { style.inset[3] }, vp),
                right: to_inset(if x_anchored { None } else { style.inset[1] }, vp),
                top: to_inset(if y_anchored { held } else { style.inset[0] }, vp),
                bottom: to_inset(if y_anchored { None } else { style.inset[2] }, vp),
            }
        },
        aspect_ratio: style.aspect_ratio,
        // taffy needs to know the box scrolls: it then sizes the box from its own
        // width/height (not its content) and reports its scrollable overflow,
        // which is how far we can scroll (`scroll_extent`).
        overflow: {
            let axis = |scrolls: bool| if scrolls { taffy::Overflow::Scroll } else { taffy::Overflow::Visible };
            Point { x: axis(style.scrolls_x()), y: axis(style.scrolls_y()) }
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
            // `flex-wrap` + a percentage width + a `max-width` once had its
            // percentage dropped to `auto` here: Taffy 0.7 measured the rows
            // at the uncapped width and sized the box for one row while two
            // were drawn. Fixed in Taffy by 0.15: tests/wrap.rs
            // (`wrapped_grid_reserves_height_for_every_row`) fails on 0.7
            // without the exception and passes on 0.15 without it.
            width: style.width.map(|l| to_dim(l, vp)).unwrap_or(auto()),
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
            left: or_pct(style.padding.left, style.padding_pct[3]),
            right: or_pct(style.padding.right, style.padding_pct[1]),
            top: or_pct(style.padding.top, style.padding_pct[0]),
            bottom: or_pct(style.padding.bottom, style.padding_pct[2]),
        },
        margin: Rect {
            left: or_pct(style.margin.left, style.margin_pct[3]),
            right: or_pct(style.margin.right, style.margin_pct[1]),
            top: or_pct(style.margin.top, style.margin_pct[0]),
            bottom: or_pct(style.margin.bottom, style.margin_pct[2]),
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
            width: or_pct(style.column_gap.unwrap_or(style.gap), style.gap_pct[0]),
            height: or_pct(style.row_gap.unwrap_or(style.gap), style.gap_pct[1]),
        },
        ..Default::default()
    }
}

/// A length, or a percentage for Taffy to resolve when one was written: what
/// CSS resolves `padding`, `margin` and `gap` percentages against is a box
/// that exists only once Taffy is laying it out.
fn or_pct<T: taffy::style_helpers::FromLength + taffy::style_helpers::FromPercent>(px: f32, pct: Option<f32>) -> T {
    match pct {
        Some(p) => percent(p / 100.0),
        None => length(px),
    }
}

fn to_align_items(a: Align) -> AlignItems {
    match a {
        Align::Start => AlignItems::FLEX_START,
        Align::Center => AlignItems::CENTER,
        Align::End => AlignItems::FLEX_END,
        Align::Stretch => AlignItems::STRETCH,
    }
}

fn to_align_content(j: Justify) -> AlignContent {
    match j {
        Justify::Start => AlignContent::FLEX_START,
        Justify::Center => AlignContent::CENTER,
        Justify::End => AlignContent::FLEX_END,
        Justify::SpaceBetween => AlignContent::SPACE_BETWEEN,
        Justify::SpaceAround => AlignContent::SPACE_AROUND,
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
        if static_absolute(tree.style(child).expect("style")) {
            out.push((id, child));
        }
        collect_statics(tree, child, out);
    }
}

/// An absolute box that names no inset, and so keeps its static position.
fn static_absolute(st: &taffy::Style) -> bool {
    st.position == taffy::Position::Absolute
        && st.inset.left == auto()
        && st.inset.right == auto()
        && st.inset.top == auto()
        && st.inset.bottom == auto()
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
    let named = style.inset.iter().any(Option::is_some)
        || style.anchor.as_ref().is_some_and(|a| a.inset.iter().any(Option::is_some));
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
        Some(Len::Vmin(v)) => length(vp.0.min(vp.1) * v / 100.0),
        Some(Len::Vmax(v)) => length(vp.0.max(vp.1) * v / 100.0),
    }
}

/// A laid-out `<input>`: its model plus what kind it is. Becomes either a
/// `FocusRegion` (text/textarea) or a `SelectRegion` (select) in `collect`.
/// A node a pointer can reach: its `@tap`, its gestures, its cursor, its
/// `touch-action`, its component instance, and its [`Node::focus_path`].
type Handler = (
    NodeId,
    Option<String>,
    Vec<(Gesture, String)>,
    Cursor,
    TouchAction,
    Option<String>,
    Option<Vec<usize>>,
);

struct Bound {
    id: NodeId,
    model: String,
    /// The enclosing `r-for` row's key, the other half of an input's identity.
    row: Option<String>,
    /// The component instance the input was written in, the scope its model is
    /// resolved in. See [`FocusRegion::instance`].
    instance: Option<String>,
    /// Which text field. See [`InputKind`].
    kind: InputKind,
    field: Field,
    options: Option<Vec<String>>,
    /// See [`Node::focus_path`]. Only a select's is ever read.
    focus_path: Option<Vec<usize>>,
}

/// Hashes a taffy `NodeId`, which is a `u64` whose low half is a dense slot
/// index, by one multiply. The default hasher cost more than the lookups it
/// served: `collect` and the measure hook ask once per node every frame.
#[derive(Default)]
struct IdHasher(u64);

impl std::hash::Hasher for IdHasher {
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 = (self.0.rotate_left(8) ^ b as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        }
    }
    fn write_u64(&mut self, n: u64) {
        self.0 = n.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    }
    fn finish(&self) -> u64 {
        self.0
    }
}

type IdMap<V> = HashMap<NodeId, V, std::hash::BuildHasherDefault<IdHasher>>;

/// Where one node's entries sit in the lists `build` fills.
///
/// `collect` used to find them by walking each list from the start for every
/// node, so a layout cost the square of its size: 5 ms of a 300-row list's
/// 12 ms went there (docs/11-next.md, track (g)). Found once per layout here.
#[derive(Default, Clone, Copy)]
struct At {
    /// A node's paints are pushed one after another, so they are a range.
    paint: (u32, u32),
    handler: Option<u32>,
    model: Option<u32>,
    focus_label: Option<u32>,
    access: Option<u32>,
    path: Option<u32>,
    state: Option<u32>,
    sticky: Option<u32>,
    opacity: Option<u32>,
    transform: Option<u32>,
    hidden: bool,
    scroll: bool,
    /// A scroller whose content starts at its right: see `ScrollRegion::rtl`.
    scroll_rtl: bool,
    /// Which axes a scroller scrolls: the other has no range to travel.
    scroll_axes: (bool, bool),
    scroll_contain: (bool, bool),
    scroll_bar: Scrollbar,
    stack: Stack,
    /// Into `Anchoring::names`, when this box is an anchor.
    anchor_name: Option<u32>,
    /// Into `Anchoring::boxes`, when this box is placed against one.
    anchored: Option<u32>,
    /// Into `Built::contained`, when this box has `content-visibility`.
    contained: Option<u32>,
}

/// The lists `build` fills and the index into them, which is everything
/// `collect` reads about a node besides its box.
struct Built<'a> {
    paint: &'a [(NodeId, PaintKind)],
    handlers: &'a [Handler],
    models: &'a [Bound],
    focus_labels: &'a [(NodeId, String, Option<String>)],
    transforms: &'a [(NodeId, Transform)],
    opacities: &'a [(NodeId, f32)],
    states: &'a [(NodeId, Vec<usize>)],
    access: &'a [(NodeId, Access, Option<String>)],
    paths: &'a [(NodeId, Vec<usize>)],
    stickies: &'a [(NodeId, [Option<f32>; 4])],
    /// The boxes with `content-visibility`, and whether each was skipped.
    contained: &'a [(NodeId, bool)],
    at: IdMap<At>,
    /// Each visible scroller's id. See `number_scrollers`.
    scroll_ids: IdMap<usize>,
    anchoring: &'a Anchoring,
}

impl Built<'_> {
    fn at(&self, id: NodeId) -> At {
        self.at.get(&id).copied().unwrap_or_default()
    }

    fn paints(&self, id: NodeId) -> &[(NodeId, PaintKind)] {
        let (start, len) = self.at(id).paint;
        &self.paint[start as usize..(start + len) as usize]
    }
}

#[allow(clippy::too_many_arguments)]
fn index(
    paint: &[(NodeId, PaintKind)],
    handlers: &[Handler],
    models: &[Bound],
    focus_labels: &[(NodeId, String, Option<String>)],
    hidden: &[NodeId],
    opacities: &[(NodeId, f32)],
    scrolls: &[ScrollFacts],
    transforms: &[(NodeId, Transform)],
    states: &[(NodeId, Vec<usize>)],
    access: &[(NodeId, Access, Option<String>)],
    paths: &[(NodeId, Vec<usize>)],
    stickies: &[(NodeId, [Option<f32>; 4])],
    stacks: &[(NodeId, Stack)],
    anchoring: &Anchoring,
) -> IdMap<At> {
    let mut at: IdMap<At> = IdMap::default();
    at.reserve(paths.len());
    for (i, (id, _)) in paint.iter().enumerate() {
        let e = at.entry(*id).or_default();
        if e.paint.1 == 0 {
            e.paint.0 = i as u32;
        }
        debug_assert_eq!(e.paint.0 + e.paint.1, i as u32, "a node's paints are pushed together");
        e.paint.1 += 1;
    }
    // Each list is indexed by its *first* entry for a node, which is what the
    // `find` it replaces returned.
    fn first(at: &mut IdMap<At>, ids: impl Iterator<Item = NodeId>, slot: fn(&mut At) -> &mut Option<u32>) {
        for (i, id) in ids.enumerate() {
            slot(at.entry(id).or_default()).get_or_insert(i as u32);
        }
    }
    first(&mut at, handlers.iter().map(|h| h.0), |a| &mut a.handler);
    first(&mut at, models.iter().map(|b| b.id), |a| &mut a.model);
    first(&mut at, focus_labels.iter().map(|f| f.0), |a| &mut a.focus_label);
    first(&mut at, access.iter().map(|a| a.0), |a| &mut a.access);
    first(&mut at, paths.iter().map(|p| p.0), |a| &mut a.path);
    first(&mut at, states.iter().map(|s| s.0), |a| &mut a.state);
    first(&mut at, stickies.iter().map(|s| s.0), |a| &mut a.sticky);
    first(&mut at, opacities.iter().map(|o| o.0), |a| &mut a.opacity);
    first(&mut at, transforms.iter().map(|t| t.0), |a| &mut a.transform);
    for id in hidden {
        at.entry(*id).or_default().hidden = true;
    }
    for f in scrolls {
        let a = at.entry(f.id).or_default();
        a.scroll = true;
        a.scroll_rtl = f.rtl;
        a.scroll_axes = f.axes;
        a.scroll_contain = f.contain;
        a.scroll_bar = f.bar;
    }
    for (id, stack) in stacks {
        at.entry(*id).or_default().stack = *stack;
    }
    first(&mut at, anchoring.names.iter().map(|n| n.0), |a| &mut a.anchor_name);
    first(&mut at, anchoring.boxes.iter().map(|b| b.0), |a| &mut a.anchored);
    at
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
        Len::Vmin(v) => Some(vp.0.min(vp.1) * v / 100.0),
        Len::Vmax(v) => Some(vp.0.max(vp.1) * v / 100.0),
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
/// border take out of it. A percentage padding is of the containing block's
/// width, which `parent` stands for; with none known it takes nothing out.
fn inner_cap(style: &Style, own: Option<f32>, parent: Option<f32>) -> Option<f32> {
    let side = |px: f32, pct: Option<f32>| match pct {
        Some(p) => parent.map_or(0.0, |b| b * p / 100.0),
        None => px,
    };
    own.map(|w| {
        let horizontal = side(style.padding.left, style.padding_pct[3])
            + side(style.padding.right, style.padding_pct[1])
            + style.border.left
            + style.border.right;
        (w - horizontal).max(0.0)
    })
}

/// A node as the last frame laid it out: its Taffy node, and what it takes to
/// match it to the node that stands in its place this frame.
///
/// Kept so Taffy keeps its cache (track (g).4): a node handed the same style,
/// text and children as before is left alone, and Taffy answers for it, and
/// everything under it, from what it worked out last time.
///
/// **Which old node a new one is given only decides how much is reused, never
/// what is laid out.** Whatever a node is matched to, [`node_for`] makes its
/// style, text and children what this frame says they are, and marks it dirty
/// when any of them differ. So a key that collides, or a row that swaps places
/// with an unkeyed sibling, costs a layout of that part and nothing worse.
struct Kept {
    id: NodeId,
    /// The `r-key`, hashed. A hash is enough for the reason above.
    key: Option<u64>,
    /// The width cap the measure hook wrapped this text at. Taffy cannot see it,
    /// so a change to it has to mark the node dirty by hand.
    cap: Option<f32>,
    /// The size a skipped box took in place of its contents, which the
    /// measure hook answers with and Taffy cannot see either.
    intrinsic: Option<(f32, f32)>,
    children: Vec<Kept>,
}

fn key_hash(key: &str) -> u64 {
    use std::hash::Hasher;
    let mut h = IdHasher::default();
    h.write(key.as_bytes());
    h.finish()
}

/// A node no longer in the document: it and everything under it are removed
/// once the frame's tree is built. See [`LayoutState`].
fn bury(kept: Kept, garbage: &mut Vec<NodeId>) {
    garbage.push(kept.id);
    for child in kept.children {
        bury(child, garbage);
    }
}

/// Whether two texts measure the same: everything `measure` is handed that can
/// change a size. Colour, alignment, caret and selection cannot.
fn measures_alike(a: &TextContent, b: &TextContent) -> bool {
    a.text == b.text
        && a.font_size == b.font_size
        && a.weight == b.weight
        && a.wrap == b.wrap
        && a.font_family == b.font_family
        && a.letter_spacing == b.letter_spacing
        && a.word_spacing == b.word_spacing
        && a.line_height == b.line_height
        && a.italic == b.italic
        && a.underline == b.underline
        && a.strikethrough == b.strikethrough
        && a.nowrap == b.nowrap
        && a.rich == b.rich
        && a.direction == b.direction
        && a.isolate == b.isolate
}

/// The Taffy node for one of this frame's nodes: `old` brought up to date, or a
/// new one when there is nothing to reuse.
///
/// Each setter marks the node and its ancestors dirty, so each is called only
/// when something differs; an unchanged node keeps its cache.
fn node_for(
    tree: &mut TaffyTree<TextContent>,
    old: Option<NodeId>,
    style: taffy::Style,
    text: Option<&TextContent>,
    children: &[NodeId],
) -> NodeId {
    let Some(id) = old else {
        let id = match text {
            Some(tc) => tree.new_leaf_with_context(style, tc.clone()),
            None if children.is_empty() => tree.new_leaf(style),
            None => tree.new_with_children(style, children),
        }
        .expect("taffy node");
        return id;
    };
    if *tree.style(id).expect("taffy style") != style {
        tree.set_style(id, style).expect("taffy style");
    }
    match (tree.get_node_context_mut(id), text) {
        (Some(have), Some(tc)) => {
            // What measure is never asked about is left as it was: the
            // context is read by the measure hook alone.
            if !measures_alike(have, tc) {
                *have = tc.clone();
                tree.mark_dirty(id).expect("taffy dirty");
            }
        }
        (None, None) => {}
        (_, tc) => tree.set_node_context(id, tc.cloned()).expect("taffy context"),
    }
    if !TraversePartialTree::child_ids(tree, id).eq(children.iter().copied()) {
        tree.set_children(id, children).expect("taffy children");
    }
    id
}

#[allow(clippy::too_many_arguments)]
fn build(
    tree: &mut TaffyTree<TextContent>,
    node: &Node,
    paint: &mut Vec<(NodeId, PaintKind)>,
    handlers: &mut Vec<Handler>,
    models: &mut Vec<Bound>,
    focus_labels: &mut Vec<(NodeId, String, Option<String>)>,
    hidden: &mut Vec<NodeId>,
    opacities: &mut Vec<(NodeId, f32)>,
    scrolls: &mut Vec<ScrollFacts>,
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
    caps: &mut IdMap<f32>,
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
    // What decides where each node paints among the others, in document order.
    // A node's index here is its place in the document, which `collect` sorts
    // on: see `Stack`.
    stacks: &mut Vec<(NodeId, Stack)>,
    // Named anchors and the boxes placed against them. See `Anchoring`.
    anchoring: &mut Anchoring,
    // `content-visibility`: which boxes skip their contents. See `Skipping`.
    skipping: &mut Skipping,
    // What this node was last frame, when one was matched to it. See `Kept`.
    old: Option<Kept>,
    // The viewport, for the root alone: it is forced to fill it, so a `screen`
    // always covers the window, and it takes every out-of-flow box still
    // looking for a containing block.
    root: Option<(f32, f32)>,
    // Taffy nodes nothing matched this frame, removed once the tree is built.
    garbage: &mut Vec<NodeId>,
) -> (NodeId, Kept) {
    // Claimed on the way down, so a parent comes before its children.
    let seq = stacks.len() as u32;
    stacks.push((NodeId::from(u64::MAX), Stack::default()));
    let own_cap = width_cap(&node.style, cap, vp);
    let child_cap = inner_cap(&node.style, own_cap, cap);
    let row = node.key.as_deref().or(row);
    let (old_id, old_cap, old_intrinsic, mut old_children) = match old {
        Some(k) => (Some(k.id), k.cap, k.intrinsic, k.children),
        None => (None, None, None, Vec::new()),
    };
    // Whether this box's contents are laid out at all, and if not the size
    // that stands in for them.
    let skip = match node.style.content_visibility {
        ContentVisibility::Visible => None,
        cv => {
            let near = cv == ContentVisibility::Auto && skipping.shown.get(path).copied().unwrap_or(false);
            (!near).then(|| {
                let ci = node.style.contain_intrinsic.as_deref().copied().unwrap_or_default();
                let last = skipping.sizes.get(path).copied();
                let axis = |a: IntrinsicSize, last: Option<f32>| {
                    a.auto.then_some(last).flatten().or(a.len).unwrap_or(0.0)
                };
                (axis(ci[0], last.map(|l| l.0)), axis(ci[1], last.map(|l| l.1)))
            })
        }
    };
    let mut ts = to_taffy(&node.style, vp);
    let mut child_ids: Vec<NodeId> = Vec::new();
    let mut kept_children: Vec<Kept> = Vec::new();
    // The text's own cap: it wraps inside this box, so its own padding and
    // border come out of the width available to the glyphs.
    let mut text_cap = None;
    if skip.is_some() {
        // Nothing inside is built: no children, no text to measure.
    } else if node.text.is_some() {
        text_cap = child_cap;
    } else if node.tick.is_some() {
    } else if let Some(ic) = &node.image {
        // An image with no CSS size falls back to its intrinsic pixel size, the
        // way a browser sizes an <img>.
        if node.style.width.is_none() {
            ts.size.width = length(ic.intrinsic.0);
        }
        if node.style.height.is_none() {
            ts.size.height = length(ic.intrinsic.1);
        }
    } else if let Some(pc) = &node.path {
        // A path given no CSS size takes the size of its own geometry, the way
        // an <image> takes its intrinsic pixels. That makes the common case
        // (paste some path data, see it) work with no box to write, and the
        // geometry is in the element's own coordinates either way: naming a
        // width does not rescale the drawing, it changes the box the drawing
        // sits in. Scaling is `transform`, which every other element already
        // uses for the same thing.
        let bounds = pc.bounds();
        if node.style.width.is_none() {
            ts.size.width = length(bounds.map_or(0.0, |b| b.2.max(0.0)));
        }
        if node.style.height.is_none() {
            ts.size.height = length(bounds.map_or(0.0, |b| b.3.max(0.0)));
        }
    } else {
        let mark = hoisted.len();
        // Each child is matched to last frame's child with the same key, or,
        // unkeyed, to the unkeyed one at the same index. Rotating a list then
        // reuses every row and changes only the order under its parent.
        let mut olds: Vec<Option<Kept>> = std::mem::take(&mut old_children).into_iter().map(Some).collect();
        let mut by_key: Option<HashMap<u64, usize, std::hash::BuildHasherDefault<IdHasher>>> = None;
        kept_children.reserve(node.children.len());
        for (i, c) in node.children.iter().enumerate() {
            let want = c.key.as_deref().map(key_hash);
            let here = olds.get(i).and_then(Option::as_ref).is_some_and(|o| o.key == want);
            let at = if here {
                Some(i)
            } else if let Some(h) = want {
                let by_key = by_key.get_or_insert_with(|| {
                    olds.iter()
                        .enumerate()
                        .filter_map(|(j, o)| Some((o.as_ref()?.key?, j)))
                        .collect()
                });
                by_key.get(&h).copied()
            } else {
                None
            };
            let prev = at.and_then(|j| olds[j].take());
            let mut cp = path.to_vec();
            cp.push(i);
            // Text in a block box once took no automatic `max-width: 100%`
            // here: Taffy 0.7 resolved the percentage against a parent size
            // from an earlier pass and kept a one-line height for text that
            // then wrapped onto two (watchlist #32). Taffy 0.12 keys its
            // cache on the parent size, and tests/stretch_text.rs, which
            // fails on 0.7 without that exception, passes on 0.15 without
            // it, so the exception is gone.
            let (cid, kept) = build(tree, c, paint, handlers, models, focus_labels, hidden, opacities, scrolls, transforms, states, access, &cp, paths, vp, child_cap, caps, row, stickies, hoisted, stacks, anchoring, skipping, prev, None, garbage);
            kept_children.push(kept);
            match hoists(&c.style) {
                Some(fixed) => hoisted.push((cid, fixed)),
                None => child_ids.push(cid),
            }
        }
        for gone in olds.into_iter().flatten() {
            bury(gone, garbage);
        }
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
        // `content-visibility` other than `visible` contains layout, which
        // makes a containing block for both kinds, as a transform does.
        let transformed =
            node.style.transform.is_some() || node.style.content_visibility != ContentVisibility::Visible;
        if transformed || node.style.position.is_containing_block() {
            let mut i = mark;
            while i < hoisted.len() {
                let carries_on = hoisted[i].1 && !transformed;
                if carries_on {
                    i += 1;
                } else {
                    child_ids.push(hoisted.remove(i).0);
                }
            }
        }
        // The root is the initial containing block, so it takes whatever is
        // still looking for one: every `fixed` box, and any absolute box with
        // no non-static ancestor. It is also the window, which is what makes
        // `fixed` mean fixed, since a box parented to the root is outside every
        // scroller and so is never shifted by one.
        if root.is_some() {
            child_ids.extend(hoisted.drain(..).map(|(id, _)| id));
        }
    }
    // A node that was a box last frame and is a leaf now leaves its children.
    for gone in old_children {
        bury(gone, garbage);
    }
    if let Some((w, h)) = root {
        ts.size = Size {
            width: length(w),
            height: length(h),
        };
    }
    // A `<text>` of pieces is laid out as the one sentence they make.
    let joined = (skip.is_none() && inline::is_sentence(node)).then(|| inline::join(node, path));
    let text = if skip.is_some() { None } else { joined.as_ref().map(|(tc, _)| tc).or(node.text.as_ref()) };
    let id = node_for(tree, old_id, ts, text, &child_ids);
    if old_id.is_some() && (text_cap != old_cap || skip != old_intrinsic) {
        tree.mark_dirty(id).expect("taffy dirty");
    }
    if let Some(sz) = skip {
        skipping.intrinsic.insert(id, sz);
    }
    if node.style.content_visibility != ContentVisibility::Visible {
        skipping.boxes.push((id, skip.is_some()));
    }
    if let Some(c) = text_cap {
        caps.insert(id, c);
    }
    let kept = Kept {
        id,
        key: node.key.as_deref().map(key_hash),
        cap: text_cap,
        intrinsic: skip,
        children: kept_children,
    };
    if let Some(tc) = text {
        // A text node is a box too: its background and border paint under the
        // glyphs. (collect() walks every paint entry for a node, in order.)
        paint.push((
            id,
            PaintKind::Box {
                bg: node.style.background.clone(),
                radius: node.style.radius,
                radius_pct: node.style.radius_pct,
                border: node.style.border,
                border_color: node.style.border_color,
                clip: clips(&node.style, skip.is_some()),
                shadow: node.style.box_shadow,
            },
        ));
        paint.push((id, PaintKind::Text(tc.clone())));
        if let Some((_, parts)) = joined {
            paint.push((id, PaintKind::Inline(std::sync::Arc::new(parts))));
        }
    } else if let Some(color) = node.tick {
        paint.push((id, PaintKind::Tick(color)));
    } else if let Some(ic) = &node.image {
        paint.push((
            id,
            PaintKind::Box {
                bg: node.style.background.clone(),
                radius: node.style.radius,
                radius_pct: node.style.radius_pct,
                border: node.style.border,
                border_color: node.style.border_color,
                clip: clips(&node.style, skip.is_some()),
                shadow: node.style.box_shadow,
            },
        ));
        paint.push((id, PaintKind::Image(ic.clone())));
    } else if let Some(pc) = &node.path {
        paint.push((
            id,
            PaintKind::Box {
                bg: node.style.background.clone(),
                radius: node.style.radius,
                radius_pct: node.style.radius_pct,
                border: node.style.border,
                border_color: node.style.border_color,
                clip: clips(&node.style, skip.is_some()),
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
    } else {
        paint.push((
            id,
            PaintKind::Box {
                bg: node.style.background.clone(),
                radius: node.style.radius,
                radius_pct: node.style.radius_pct,
                // Uniform border for rendering (top width is representative).
                border: node.style.border,
                border_color: node.style.border_color,
                clip: clips(&node.style, skip.is_some()),
                shadow: node.style.box_shadow,
            },
        ));
    }
    // Last of the node's own paint, and read after its children: see
    // `PaintKind::Outline` in `collect`.
    if let Some(outline) = node.style.outline {
        paint.push((id, PaintKind::Outline(outline)));
    }
    // A hit region is needed for anything a pointer can reach, not only for a
    // `@tap`: an element with just `@drag` still has to be found by a hit test.
    if node.on_tap.is_some() || !node.gestures.is_empty() {
        handlers.push((
            id,
            node.on_tap.clone(),
            node.gestures.clone(),
            node.style.cursor,
            node.style.touch_action,
            node.instance.clone(),
            node.focus_path.clone(),
        ));
    }
    // A disabled field is not a field as far as focus is concerned: no region
    // to tap, nothing for Tab to land on, no label that can focus it. What it
    // shows still paints, from the node, like any other box.
    if let Some(model) = node.model.as_ref().filter(|_| !node.field.disabled) {
        models.push(Bound {
            id,
            model: model.clone(),
            row: row.map(str::to_string),
            instance: node.instance.clone(),
            kind: node.kind,
            field: node.field.clone(),
            options: node.options.clone(),
            focus_path: node.focus_path.clone(),
        });
    }
    if let Some(fm) = &node.focus_model {
        focus_labels.push((id, fm.clone(), row.map(str::to_string)));
    }
    // `display: none` reached taffy, which gave the box no size and no slot,
    // but nothing stopped it being *painted*: a text node with no box still
    // drew its glyphs, at its parent's origin, on top of whatever was really
    // there. It takes the same road as `r-show="false"` from here, because
    // "paints nothing, hit-tests as nothing, and takes its subtree with it" is
    // exactly what both mean. The one difference between them, whether a slot
    // is reserved, is already settled in the layout above.
    if node.hidden || node.style.display == Display::None {
        hidden.push(id);
    }
    if node.style.opacity < 1.0 {
        opacities.push((id, node.style.opacity.max(0.0)));
    }
    if let Some(tf) = node.style.transform {
        transforms.push((id, tf));
    }
    if node.style.overflow == Overflow::Scroll {
        scrolls.push(ScrollFacts {
            id,
            rtl: node.style.direction == Direction::Rtl,
            axes: (node.style.scrolls_x(), node.style.scrolls_y()),
            contain: node.style.overscroll_contain,
            bar: Scrollbar {
                width: node.style.scrollbar_width,
                color: node.style.scrollbar_color.as_deref().copied(),
            },
        });
    }
    if node.style.position.is_sticky() {
        // Resolved to px here, where the viewport is in hand, so `collect` can
        // compare them against window coordinates without carrying units.
        let px = |l: Option<Len>| match l {
            None => None,
            Some(Len::Px(v)) => Some(v),
            // A fraction already: `10%` is 0.1. Divided by a hundred again
            // until 2026-10-15, so a sticky `top: 10%` stuck at a thousandth.
            Some(Len::Pct(p)) => Some(vp.1 * p),
            Some(Len::Vw(v)) => Some(vp.0 * v / 100.0),
            Some(Len::Vh(v)) => Some(vp.1 * v / 100.0),
            Some(Len::Vmin(v)) => Some(vp.0.min(vp.1) * v / 100.0),
            Some(Len::Vmax(v)) => Some(vp.0.max(vp.1) * v / 100.0),
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
    if let Some(a) = &node.style.anchor {
        if let Some(name) = &a.name {
            anchoring.names.push((id, name.clone(), seq));
        }
        // `anchor()` belongs to an absolutely positioned box, as in CSS; on
        // any other it means nothing.
        if node.style.position.is_out_of_flow() && a.anchored() {
            anchoring.boxes.push((
                id,
                Anchored {
                    inset: a.inset.clone(),
                    plain: node.style.inset,
                    size: a.size.clone(),
                    default: a.position_anchor.clone(),
                    fixed: node.style.position == Position::Fixed,
                    tries: a.tries.clone(),
                    try_order: a.try_order,
                    path: path.to_vec(),
                },
            ));
        }
    }
    stacks[seq as usize] = (
        id,
        Stack {
            seq,
            z: node.style.z_index,
            position: node.style.position,
            // Contained, as `content-visibility` contains paint: whatever is
            // inside is sorted inside, as with `isolation: isolate`.
            isolate: node.style.isolate || node.style.content_visibility != ContentVisibility::Visible,
        },
    );
    if node.access.role.is_meaningful() {
        access.push((id, node.access.clone(), node.model.clone()));
    }
    (id, kept)
}

/// Whether a box clips what is inside it: `overflow` other than `visible`,
/// or the paint containment `content-visibility` brings, which a box whose
/// contents were skipped has nothing to apply to.
fn clips(style: &Style, skipped: bool) -> bool {
    style.overflow != Overflow::Visible || style.content_visibility != ContentVisibility::Visible && !skipped
}

/// What `content-visibility` needs across one layout: which boxes were near
/// the screen last frame and the size each was last drawn at (both by
/// path), and, filled by `build`, what was skipped.
struct Skipping<'a> {
    shown: &'a HashMap<Vec<usize>, bool>,
    sizes: &'a HashMap<Vec<usize>, (f32, f32)>,
    /// Each skipped box's content size, answered by the measure hook.
    intrinsic: IdMap<(f32, f32)>,
    /// Every box with `content-visibility` other than `visible`, and whether
    /// its contents were skipped.
    boxes: Vec<(NodeId, bool)>,
}

/// A box with `content-visibility` other than `visible`, as one layout
/// found it. See [`LayoutState`].
#[derive(Clone, Debug, PartialEq)]
pub struct Contained {
    pub path: Vec<usize>,
    /// Whether its contents were skipped this frame.
    pub skipped: bool,
    /// Whether any of it is on screen, inside every clip it sits in.
    pub on_screen: bool,
    /// Whether it is near enough to the screen that its contents should be
    /// there: on screen with half the window's size to spare on every side.
    pub near: bool,
    /// Its content box, when its contents were laid out: what
    /// `contain-intrinsic-size: auto` remembers.
    pub size: Option<(f32, f32)>,
}

/// What decides where a node paints among the others: CSS's stacking rules.
///
/// **Paint order is not tree order.** Inside a stacking context CSS paints the
/// context's own box, then the boxes with a negative `z-index`, then the
/// in-flow content, then the positioned boxes with `z-index: auto` or `0` in
/// document order, then the positive ones from lowest up (CSS 2, appendix E).
/// A box with a `z-index` makes a context of its own, and so do `fixed`,
/// `sticky`, `opacity` below 1, `transform` and `isolation: isolate`: whatever
/// is inside one of those is sorted against its neighbours there and is drawn
/// all together, never interleaved with anything outside it.
///
/// Before this was built Rux had two hand-written cases, a positioned box over
/// its in-flow *siblings* and every `fixed` box over everything, and an author
/// had no way to say anything else. A dropdown opened under the next card.
#[derive(Clone, Copy, Debug, Default)]
struct Stack {
    /// The node's place in the document. Ties paint in this order, and Tab
    /// and the accessibility tree are put back in it after painting reorders
    /// them, since neither should follow where a box happens to be drawn.
    seq: u32,
    z: Option<i32>,
    position: Position,
    isolate: bool,
}

/// How a child is painted relative to its stacking context: `None` in flow,
/// in the walk, or `Some((z, makes_its_own))` held back and drawn later.
///
/// `parent_items` is whether the parent lays its children out as flex or grid
/// items, the one case where `z-index` counts on a box that is not positioned.
fn layer(at: &At, parent_items: bool) -> Option<(i32, bool)> {
    let st = at.stack;
    let positioned = st.position != Position::Static;
    let item = parent_items && !st.position.is_out_of_flow();
    let z = st.z.filter(|_| positioned || item);
    let context = z.is_some()
        || matches!(st.position, Position::Fixed | Position::Sticky)
        || at.opacity.is_some()
        || at.transform.is_some()
        || st.isolate;
    (positioned || context).then_some((z.unwrap_or(0), context))
}

/// A clip between a stacking context and a box it paints later, which has to
/// be put back around that box: the box is still inside it.
///
/// Only clips. A `transform` or an `opacity` in between would itself be a
/// stacking context, and the box would belong to that one instead.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Clip {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    radius: Corners,
}

/// A box held back until its stacking context reaches its layer, with
/// everything `collect` would have been called with where it was found.
struct Deferred {
    z: i32,
    seq: u32,
    context: bool,
    id: NodeId,
    origin: (f32, f32),
    holder: (f32, f32, f32, f32),
    inside_scroll: Option<usize>,
    xform: Option<Transform>,
    dim: f32,
    clips: Vec<Clip>,
}

impl Deferred {
    fn key(&self) -> (i32, u32) {
        (self.z, self.seq)
    }
}

impl PartialEq for Deferred {
    fn eq(&self, other: &Self) -> bool {
        self.key() == other.key()
    }
}
impl Eq for Deferred {}
impl PartialOrd for Deferred {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Deferred {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.key().cmp(&other.key())
    }
}

/// The lengths of the lists painting order decides: paints, and every region
/// a pointer finds by taking the topmost (last) one. Scrollers, Tab order and
/// the accessibility tree are not among them, on purpose.
type Marks = [usize; 5];

fn marks(out: &Layout) -> Marks {
    [out.paints.len(), out.hits.len(), out.focuses.len(), out.selects.len(), out.states.len()]
}

/// One stacking context while it is being painted.
struct Stacking {
    /// Where its children start, after its own box: where negative layers go.
    mark: Marks,
    /// How deep the clip stack was there. The clips above it are the ones a
    /// held-back box has to be drawn inside.
    clip_base: usize,
    pending: std::collections::BinaryHeap<std::cmp::Reverse<Deferred>>,
    /// The negative layers already moved under the in-flow content, in order,
    /// with how much of each list they took.
    below: Vec<((i32, u32), Marks)>,
}

impl Stacking {
    fn new(out: &Layout, clip_base: usize) -> Self {
        Stacking {
            mark: marks(out),
            clip_base,
            pending: Default::default(),
            below: Vec::new(),
        }
    }
}

/// CSS anchor positioning: the boxes named as anchors, and the boxes placed
/// against them.
///
/// **Placed at paint time, as `sticky` is**, because where an anchor is on
/// screen is a question about scroll offsets, which the layout does not know.
/// So `collect` runs twice when anything is anchored: once to find where each
/// anchor is drawn, and once to draw, with every anchored box moved against
/// that. The first walk is thrown away. A menu therefore follows its button
/// when the panel under it scrolls, and its hit region goes with it.
///
/// Size is the layout's: `anchor-size()`, and an axis with both insets on the
/// same anchor, are given to it in a second layout pass once the anchors have
/// been measured. See `size_anchored`.
#[derive(Default)]
struct Anchoring {
    /// Every `anchor-name`, with its node's place in the document: when two
    /// boxes share a name the later one is the anchor, as in CSS.
    names: Vec<(NodeId, String, u32)>,
    boxes: Vec<(NodeId, Anchored)>,
}

impl Anchoring {
    /// The node a name means for sizing: the last one in the document.
    fn node(&self, name: &str) -> Option<NodeId> {
        self.names.iter().filter(|n| n.1 == name).max_by_key(|n| n.2).map(|n| n.0)
    }
}

/// An absolutely positioned box with an `anchor()` or `anchor-size()`.
#[derive(Clone, Debug)]
struct Anchored {
    inset: [Option<AnchorRef>; 4],
    /// The insets that are lengths, which the layout placed.
    plain: [Option<Len>; 4],
    size: [Option<AnchorSize>; 2],
    /// `position-anchor`.
    default: Option<String>,
    fixed: bool,
    /// `position-try-fallbacks` and `position-try-order`.
    tries: Vec<PositionTry>,
    try_order: TryOrder,
    /// Where the box is in the document, which is what the place it last
    /// fitted is remembered by. See `LayoutState::tried`.
    path: Vec<usize>,
}

impl Anchored {
    fn name<'a>(&'a self, own: &'a Option<String>) -> Option<&'a str> {
        own.as_deref().or(self.default.as_deref())
    }
}

/// Where an anchor was drawn, in window pixels, and whether any of it was
/// left inside the clips it sits in.
#[derive(Clone, Copy, Debug)]
struct AnchorBox {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    visible: bool,
    seq: u32,
}

/// How far along an anchor of length `len` starting at `start` a side is.
fn anchor_edge(side: AnchorSide, start: f32, len: f32) -> f32 {
    start
        + len
            * match side {
                AnchorSide::Top | AnchorSide::Left => 0.0,
                AnchorSide::Bottom | AnchorSide::Right => 1.0,
                AnchorSide::Center => 0.5,
                AnchorSide::Frac(f) => f,
            }
}

fn len_px(l: Len, of: f32, vp: (f32, f32)) -> f32 {
    match l {
        Len::Px(v) => v,
        Len::Pct(f) => f * of,
        Len::Vw(v) => vp.0 * v / 100.0,
        Len::Vh(v) => vp.1 * v / 100.0,
        Len::Vmin(v) => vp.0.min(vp.1) * v / 100.0,
        Len::Vmax(v) => vp.0.max(vp.1) * v / 100.0,
    }
}

/// The size `a` takes from its anchor on one axis, if it takes one: its
/// `anchor-size()`, or both insets on that axis measured to one anchor.
fn anchored_size(
    a: &Anchored,
    vertical: bool,
    anchoring: &Anchoring,
    tree: &TaffyTree<TextContent>,
    vp: (f32, f32),
) -> Option<f32> {
    let measured = |name: Option<&str>, height: bool| {
        let id = anchoring.node(name?)?;
        let size = tree.layout(id).ok()?.size;
        Some(if height { size.height } else { size.width })
    };
    if let Some(s) = &a.size[vertical as usize] {
        return measured(a.name(&s.name), s.height).or_else(|| {
            s.fallback.map(|f| len_px(f, if vertical { vp.1 } else { vp.0 }, vp))
        });
    }
    let (start, end) = if vertical { (&a.inset[0], &a.inset[2]) } else { (&a.inset[3], &a.inset[1]) };
    let (start, end) = (start.as_ref()?, end.as_ref()?);
    let name = a.name(&start.name)?;
    if a.name(&end.name)? != name {
        return None;
    }
    let len = measured(Some(name), vertical)?;
    Some((anchor_edge(end.side, 0.0, len) - anchor_edge(start.side, 0.0, len)).max(0.0))
}

/// What an anchored box does this frame.
enum Placement {
    /// Drawn with its top-left corner here (each `None` axis where the layout
    /// put it).
    At(Option<f32>, Option<f32>),
    /// Its anchor is entirely clipped away, so the box is not drawn: CSS's
    /// default `position-visibility: anchors-visible`. A menu does not float
    /// on alone after its button has scrolled out of the panel.
    Hidden,
}

/// One side of a place to try: what its inset measures to.
#[derive(Clone, Debug)]
enum Edge {
    Auto,
    Len(Len),
    Anchor(AnchorRef),
}

/// A place to try: the box's own insets and margins, or a fallback's over
/// them, in `inset` order, and the anchor an `anchor()` naming none means.
#[derive(Clone, Debug)]
struct Spot {
    edge: [Edge; 4],
    margin: [f32; 4],
    default: Option<String>,
}

impl Spot {
    /// The box's own place. `margin` is the layout's, already resolved.
    fn own(a: &Anchored, margin: [f32; 4]) -> Spot {
        let edge = std::array::from_fn(|i| match (&a.inset[i], a.plain[i]) {
            (Some(r), _) => Edge::Anchor(r.clone()),
            (None, Some(l)) => Edge::Len(l),
            (None, None) => Edge::Auto,
        });
        Spot { edge, margin, default: a.default.clone() }
    }

    /// `t` over this place: its rule's sides, then its flips.
    fn tried(&self, t: &PositionTry, cb_width: f32) -> Spot {
        let mut s = self.clone();
        for i in 0..4 {
            if let Some(e) = &t.inset[i] {
                s.edge[i] = match e {
                    TryInset::Auto => Edge::Auto,
                    TryInset::Len(l) => Edge::Len(*l),
                    TryInset::Anchor(r) => Edge::Anchor(r.clone()),
                };
            }
            if let Some(f) = t.margin_pct[i] {
                s.margin[i] = f * cb_width;
            } else if let Some(m) = t.margin[i] {
                s.margin[i] = m;
            }
        }
        if t.position_anchor.is_some() {
            s.default = t.position_anchor.clone();
        }
        // In CSS's order: block, then inline, then start.
        if t.flip_block {
            s.swap(0, 2, |side| match side {
                AnchorSide::Top => AnchorSide::Bottom,
                AnchorSide::Bottom => AnchorSide::Top,
                AnchorSide::Frac(f) => AnchorSide::Frac(1.0 - f),
                other => other,
            });
        }
        if t.flip_inline {
            s.swap(3, 1, |side| match side {
                AnchorSide::Left => AnchorSide::Right,
                AnchorSide::Right => AnchorSide::Left,
                AnchorSide::Frac(f) => AnchorSide::Frac(1.0 - f),
                other => other,
            });
        }
        if t.flip_start {
            // Top goes to left and bottom to right, each anchor edge to the
            // matching one on the other axis.
            let turn = |side| match side {
                AnchorSide::Top => AnchorSide::Left,
                AnchorSide::Left => AnchorSide::Top,
                AnchorSide::Bottom => AnchorSide::Right,
                AnchorSide::Right => AnchorSide::Bottom,
                other => other,
            };
            s.swap(0, 3, turn);
            s.swap(2, 1, turn);
        }
        s
    }

    fn swap(&mut self, i: usize, j: usize, side: impl Fn(AnchorSide) -> AnchorSide) {
        self.edge.swap(i, j);
        self.margin.swap(i, j);
        for k in [i, j] {
            if let Edge::Anchor(r) = &mut self.edge[k] {
                r.side = side(r.side);
            }
        }
    }
}

/// Where a place puts the box on one axis, and the room it leaves.
struct Fit {
    /// The box's start, `None` where the layout put it.
    at: Option<f32>,
    /// The inset-modified containing block on this axis: from the start
    /// inset (or the containing block's edge) to the end one.
    room: (f32, f32),
}

/// Where a place puts a box, and the names it asked for that nothing
/// answered.
struct Placed {
    x: Fit,
    y: Fit,
    hidden: bool,
    misses: Vec<String>,
}

impl Placed {
    /// Whether the margin box stays inside the room its insets leave it.
    /// `laid` is where the layout put the box, for an axis the layout places.
    fn fits(&self, s: &Spot, width: f32, height: f32, laid: (f32, f32)) -> bool {
        const SLACK: f32 = 0.5;
        let axis = |f: &Fit, laid: f32, len: f32, m0: f32, m1: f32| {
            let at = f.at.unwrap_or(laid);
            at - m0 >= f.room.0 - SLACK && at + len + m1 <= f.room.1 + SLACK
        };
        axis(&self.x, laid.0, width, s.margin[3], s.margin[1]) && axis(&self.y, laid.1, height, s.margin[0], s.margin[2])
    }
}

/// Where place `s` puts a box laid out `width` x `height`, against the
/// anchors drawn last walk.
fn put(
    s: &Spot,
    width: f32,
    height: f32,
    cb: (f32, f32, f32, f32),
    vp: (f32, f32),
    known: &HashMap<String, AnchorBox>,
) -> Placed {
    let mut hidden = false;
    let mut misses = Vec::new();
    // One axis: `start` is the top or left side, `end` the bottom or right.
    let mut axis = |start: usize, end: usize, size: f32, vertical: bool| -> Fit {
        let (cb_at, cb_len) = if vertical { (cb.1, cb.3) } else { (cb.0, cb.2) };
        // Where a side's inset puts that edge of the box, in window pixels.
        let mut edge = |i: usize, from_end: bool| -> Option<f32> {
            let from_cb = |px: f32| if from_end { cb_at + cb_len - px } else { cb_at + px };
            match &s.edge[i] {
                Edge::Auto => None,
                Edge::Len(l) => Some(from_cb(len_px(*l, cb_len, vp))),
                Edge::Anchor(r) => {
                    let Some(name) = r.name.as_deref().or(s.default.as_deref()) else {
                        misses.push(String::new());
                        return None;
                    };
                    match known.get(name) {
                        Some(b) if !b.visible => {
                            hidden = true;
                            None
                        }
                        Some(b) => {
                            let (at, len) = if vertical { (b.y, b.height) } else { (b.x, b.width) };
                            Some(anchor_edge(r.side, at, len))
                        }
                        None => match r.fallback {
                            Some(fb) => Some(from_cb(len_px(fb, cb_len, vp))),
                            None => {
                                misses.push(name.to_string());
                                None
                            }
                        },
                    }
                }
            }
        };
        let (a, b) = (edge(start, false), edge(end, true));
        let at = match (a, b) {
            (Some(a), _) => Some(a + s.margin[start]),
            (None, Some(b)) => Some(b - s.margin[end] - size),
            (None, None) => None,
        };
        Fit { at, room: (a.unwrap_or(cb_at), b.unwrap_or(cb_at + cb_len)) }
    };
    let x = axis(3, 1, width, false);
    let y = axis(0, 2, height, true);
    Placed { x, y, hidden, misses }
}

/// Where `a` goes this frame, and which of its places that is (0 its own,
/// `n` its `n`th fallback), to be tried first next frame.
///
/// The box's own place wins when it fits. Otherwise the first fallback that
/// does, in `position-try-order`; with none, the one that last fitted, or
/// its own. A place that fitted last frame and still does is kept, so a
/// menu that flipped does not flip back and forth as its panel scrolls.
/// `laid` is where the layout drew the box and `margin` its margins.
#[allow(clippy::too_many_arguments)]
fn place(
    a: &Anchored,
    width: f32,
    height: f32,
    laid: (f32, f32),
    margin: [f32; 4],
    cb: (f32, f32, f32, f32),
    vp: (f32, f32),
    known: &HashMap<String, AnchorBox>,
    last: Option<u8>,
    misses: &mut Vec<String>,
) -> (Placement, u8) {
    let own = Spot::own(a, margin);
    let spots: Vec<Spot> =
        std::iter::once(own.clone()).chain(a.tries.iter().map(|t| own.tried(t, cb.2))).collect();
    let placed: Vec<Placed> = spots.iter().map(|s| put(s, width, height, cb, vp, known)).collect();
    let fits = |i: usize| !placed[i].hidden && placed[i].fits(&spots[i], width, height, laid);
    let chosen = if spots.len() == 1 {
        0
    } else {
        let mut order: Vec<usize> = (0..spots.len()).collect();
        let room = |f: &Fit| f.room.1 - f.room.0;
        match a.try_order {
            TryOrder::Normal => {}
            TryOrder::MostWidth => order.sort_by(|&i, &j| room(&placed[j].x).total_cmp(&room(&placed[i].x))),
            TryOrder::MostHeight => order.sort_by(|&i, &j| room(&placed[j].y).total_cmp(&room(&placed[i].y))),
        }
        let last = last.map(usize::from).filter(|&i| i < spots.len());
        match last.filter(|&i| fits(i)) {
            Some(i) => i,
            None => order.into_iter().find(|&i| fits(i)).or(last).unwrap_or(0),
        }
    };
    let p = &placed[chosen];
    misses.extend(p.misses.iter().cloned());
    if p.hidden {
        return (Placement::Hidden, chosen as u8);
    }
    // The box's own place moves only an axis an `anchor()` places, which the
    // layout held at the containing block's start; the layout's answer for
    // any other axis is already right. A fallback is placed whole.
    let (x, y) = if chosen == 0 {
        let anchored = |i: usize| matches!(own.edge[i], Edge::Anchor(_));
        (p.x.at.filter(|_| anchored(1) || anchored(3)), p.y.at.filter(|_| anchored(0) || anchored(2)))
    } else {
        (p.x.at, p.y.at)
    };
    (Placement::At(x, y), chosen as u8)
}

/// What lives for the whole walk rather than for one node.
#[derive(Default)]
struct Walk {
    /// The clips pushed on the way down to where the walk is now.
    clips: Vec<Clip>,
    /// Each scroller's region, by id. Ids are given out before painting (see
    /// `number_scrollers`), so the order painting reaches them in is free to
    /// differ from their ids.
    scrolls: Vec<Option<ScrollRegion>>,
    /// The `seq` of each entry in `Layout::focusables` and `Layout::access`.
    focus_seq: Vec<u32>,
    access_seq: Vec<u32>,
    /// Where each anchor was drawn by the walk before this one, when
    /// something is anchored. `None` on that first walk itself.
    known: Option<HashMap<String, AnchorBox>>,
    /// Where each anchor is drawn by this walk.
    seen: HashMap<String, AnchorBox>,
    /// Names asked for that nothing on screen answered.
    misses: Vec<String>,
    /// Whether this is the walk that only finds the anchors, whose paints and
    /// regions are thrown away, so it makes none.
    dry: bool,
    /// The fallback each anchored box was drawn at last frame, by its path.
    /// See `place`.
    tried_before: std::rc::Rc<HashMap<Vec<usize>, (u8, Option<String>)>>,
    /// The fallback each is drawn at this frame, where not its own place.
    tried: Vec<(Vec<usize>, u8, Option<String>)>,
    /// The boxes with `content-visibility`, as this walk drew them.
    contained: Vec<Contained>,
    /// Where the parts of each sentence landed, asked of the text engine once
    /// the sentence had a width. See [`TextMeasure::inline`].
    inline: std::rc::Rc<IdMap<std::sync::Arc<InlineGeometry>>>,
}

/// A rectangle as `(x, y, width, height)`.
type Rect4 = (f32, f32, f32, f32);

/// Give every scroller that will be drawn an id, in tree order.
///
/// **Not in the order painting reaches them.** The shell keeps each scroller's
/// offset by id from one frame to the next, and painting order moves whenever
/// a box starts or stops being its own stacking context, which a fading page
/// does twice per route change. Numbered by painting, two scrollers swapped
/// offsets for the length of the fade.
fn number_scrollers(tree: &TaffyTree<TextContent>, id: NodeId, at: &IdMap<At>, ids: &mut IdMap<usize>) {
    let a = at.get(&id).copied().unwrap_or_default();
    if a.hidden {
        return;
    }
    if a.scroll {
        let n = ids.len();
        ids.insert(id, n);
    }
    for kid in tree.children(id).expect("children") {
        number_scrollers(tree, kid, at, ids);
    }
}

/// `items` sorted by `seq`, keeping the order of equal ones.
fn in_order<T>(items: Vec<T>, seq: &[u32]) -> Vec<T> {
    debug_assert_eq!(items.len(), seq.len());
    if seq.windows(2).all(|w| w[0] <= w[1]) {
        return items;
    }
    let mut keyed: Vec<(u32, T)> = seq.iter().copied().zip(items).collect();
    keyed.sort_by_key(|(k, _)| *k);
    keyed.into_iter().map(|(_, t)| t).collect()
}

/// Draw the boxes a stacking context held back, in CSS's order.
///
/// The heap gives them lowest first. A negative one is drawn at the end and
/// then moved down under the in-flow content, which was drawn before it was
/// known: one can be found late, inside a positioned box that is not a
/// context of its own and so is only walked here. Everything else is drawn
/// where it falls.
#[allow(clippy::too_many_arguments)]
fn drain(
    tree: &TaffyTree<TextContent>,
    b: &Built,
    offsets: &[Offset],
    vp: (f32, f32),
    out: &mut Layout,
    walk: &mut Walk,
    ctx: &mut Stacking,
) {
    // The clips put back around the last member drawn, left open while the
    // next one sits inside the same ones: a list of rows that are each a
    // context (`content-visibility`, say) is one clip around them all, not
    // a clip layer per row.
    let mut open: Option<Vec<Clip>> = None;
    let close = |open: &mut Option<Vec<Clip>>, out: &mut Layout, walk: &mut Walk| {
        for _ in open.take().unwrap_or_default() {
            out.paints.push(Paint::PopClip);
            walk.clips.pop();
        }
    };
    while let Some(std::cmp::Reverse(d)) = ctx.pending.pop() {
        // A negative one is moved afterwards, so it carries its own clips.
        let shared = d.z >= 0 && open.as_ref().is_some_and(|c| *c == d.clips);
        if !shared {
            close(&mut open, out, walk);
        }
        let start = marks(out);
        if !shared {
            for c in &d.clips {
                out.paints.push(Paint::PushClip { x: c.x, y: c.y, width: c.width, height: c.height, radius: c.radius });
                walk.clips.push(*c);
            }
            open = Some(d.clips.clone());
        }
        collect(
            tree, d.id, d.origin.0, d.origin.1, b, offsets, vp, d.holder, d.inside_scroll, d.xform, d.dim,
            d.context, out, walk, ctx,
        );
        if d.z >= 0 {
            continue;
        }
        close(&mut open, out, walk);
        let end = marks(out);
        let took: Marks = std::array::from_fn(|i| end[i] - start[i]);
        let at = ctx.below.partition_point(|(k, _)| *k < d.key());
        let before: Marks = std::array::from_fn(|i| ctx.below[..at].iter().map(|(_, m)| m[i]).sum());
        let to: Marks = std::array::from_fn(|i| ctx.mark[i] + before[i]);
        out.paints[to[0]..end[0]].rotate_right(took[0]);
        out.hits[to[1]..end[1]].rotate_right(took[1]);
        out.focuses[to[2]..end[2]].rotate_right(took[2]);
        out.selects[to[3]..end[3]].rotate_right(took[3]);
        out.states[to[4]..end[4]].rotate_right(took[4]);
        ctx.below.insert(at, (d.key(), took));
    }
    close(&mut open, out, walk);
}

#[allow(clippy::too_many_arguments)]
fn collect(
    tree: &TaffyTree<TextContent>,
    id: NodeId,
    origin_x: f32,
    origin_y: f32,
    b: &Built,
    offsets: &[Offset],
    vp: (f32, f32),
    // The box this node sits in, as `(x, y, width, height)` in window
    // coordinates. Only `sticky` reads it, to stop travelling when its parent
    // runs out from under it.
    holder: (f32, f32, f32, f32),
    // The nearest scroller above this node: what holds a focusable, and what
    // holds a scroller, so focus scrolls only the boxes an element is in.
    inside_scroll: Option<usize>,
    // The `transform` and `opacity` accumulated from the ancestors, so anything
    // drawn outside the paint list can be drawn through the same lens.
    xform: Option<Transform>,
    dim: f32,
    // Whether this box is a stacking context: what its positioned descendants
    // are sorted in. When it is not, they go to `ctx`, the one it is inside.
    context: bool,
    out: &mut Layout,
    walk: &mut Walk,
    ctx: &mut Stacking,
) {
    let layout = tree.layout(id).expect("layout");
    let at = b.at(id);
    let mut x = origin_x + layout.location.x;
    let mut y = origin_y + layout.location.y;

    // `sticky` is resolved here rather than in the layout, because it is a
    // question about the scroll offset and the layout does not know one. Done
    // before anything is recorded, so the box's paint, its hit region, its
    // metrics and its children all move together: a sticky header you can see
    // but cannot tap would be worse than one that does not stick.
    if let Some((_, inset)) = at.sticky.map(|i| &b.stickies[i as usize]) {
        // Against the scroller it is inside; with none, against the window,
        // which is what a sticky box in an unscrolled document sits in.
        let view = match inside_scroll.and_then(|sid| walk.scrolls.get(sid)).and_then(Option::as_ref) {
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

    // r-show=false: the node kept its layout slot but paints nothing (nor its
    // subtree, nor its hit regions).
    if at.hidden {
        return;
    }

    // Anchored, so placed against where its anchor was drawn: before anything
    // is recorded, for the same reason as sticky above. See `Anchoring`.
    if let (Some(i), Some(known)) = (at.anchored, walk.known.as_ref()) {
        let a = &b.anchoring.boxes[i as usize].1;
        let cb = if a.fixed { (0.0, 0.0, vp.0, vp.1) } else { holder };
        let m = layout.margin;
        // Forgotten once the box is anchored to something else.
        let last = walk.tried_before.get(&a.path).filter(|(_, d)| *d == a.default).map(|(n, _)| *n);
        let (placement, chosen) = place(
            a,
            layout.size.width,
            layout.size.height,
            (x, y),
            [m.top, m.right, m.bottom, m.left],
            cb,
            vp,
            known,
            last,
            &mut walk.misses,
        );
        if chosen != 0 {
            walk.tried.push((a.path.clone(), chosen, a.default.clone()));
        }
        match placement {
            Placement::Hidden => return,
            Placement::At(nx, ny) => {
                x = nx.unwrap_or(x);
                y = ny.unwrap_or(y);
            }
        }
    }
    let x = x;
    let y = y;

    // `content-visibility`: whether this box is on screen, or near it, for
    // the next pass or frame to decide whether its contents are there.
    if let (Some(i), false) = (at.contained, walk.dry) {
        let (_, skipped) = b.contained[i as usize];
        let path = at.path.map(|p| b.paths[p as usize].1.clone()).unwrap_or_default();
        let (w, h) = (layout.size.width, layout.size.height);
        // What can be seen from here, the window inside every clip, and the
        // same with half the window to spare on every side of each.
        let (mx, my) = (vp.0 / 2.0, vp.1 / 2.0);
        let mut seen = (0.0f32, 0.0f32, vp.0, vp.1);
        let mut near = (-mx, -my, vp.0 + mx, vp.1 + my);
        for c in &walk.clips {
            seen = (seen.0.max(c.x), seen.1.max(c.y), seen.2.min(c.x + c.width), seen.3.min(c.y + c.height));
            near = (
                near.0.max(c.x - mx),
                near.1.max(c.y - my),
                near.2.min(c.x + c.width + mx),
                near.3.min(c.y + c.height + my),
            );
        }
        let meets = |r: (f32, f32, f32, f32)| x <= r.2 && x + w >= r.0 && y <= r.3 && y + h >= r.1 && r.2 > r.0 && r.3 > r.1;
        let (_, _, cw, ch) = content_box(layout);
        walk.contained.push(Contained {
            path,
            skipped,
            on_screen: meets(seen),
            near: meets(near),
            size: (!skipped).then_some((cw, ch)),
        });
    }

    if let Some(i) = at.anchor_name {
        let (_, name, seq) = &b.anchoring.names[i as usize];
        // Visible unless the clips it is inside leave nothing of it.
        let (mut l, mut t, mut r, mut bt) = (x, y, x + layout.size.width, y + layout.size.height);
        for c in &walk.clips {
            l = l.max(c.x);
            t = t.max(c.y);
            r = r.min(c.x + c.width);
            bt = bt.min(c.y + c.height);
        }
        let visible = r > l && bt > t || (layout.size.width == 0.0 || layout.size.height == 0.0) && r >= l && bt >= t;
        let here = AnchorBox { x, y, width: layout.size.width, height: layout.size.height, visible, seq: *seq };
        match walk.seen.get(name) {
            Some(had) if had.seq > *seq => {}
            _ => {
                walk.seen.insert(name.clone(), here);
            }
        }
    }

    // opacity fades this node and everything under it, so the layer opens
    // before the node paints its own background.
    let alpha = at.opacity.map_or(1.0, |i| b.opacities[i as usize].1);
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
    let transform = at.transform.map(|i| b.transforms[i as usize].1);
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
    // The parts of a sentence and where they landed, for their regions.
    let mut placed: Vec<(inline::InlinePart, Vec<Rect4>)> = Vec::new();
    // A node can emit more than one paint (a text node paints its box, then its
    // glyphs), so walk every entry it owns, in order.
    // The walk that only finds where anchors are drawn (see `Anchoring`) needs
    // a box's clip and nothing it paints or answers taps with.
    if walk.dry {
        for (_, kind) in b.paints(id) {
            if let PaintKind::Box { radius, radius_pct, clip: c, .. } = kind {
                clip = *c;
                clip_radius = resolve_radius(*radius, *radius_pct, layout.size.width, layout.size.height);
            }
        }
    } else {
        for (_, kind) in b.paints(id) {
            match kind {
                PaintKind::Box {
                    bg,
                    radius,
                    radius_pct,
                    border,
                    border_color,
                    clip: c,
                    shadow,
                } => {
                    clip = *c;
                    // Here, and not in the cascade, is the first moment a
                    // percentage has a box to be a percentage of.
                    let radius =
                        &resolve_radius(*radius, *radius_pct, layout.size.width, layout.size.height);
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
                    let widest = border.top.max(border.right).max(border.bottom).max(border.left);
                    let has_border = widest > 0.0 && border_color.is_some();
                    if bg.is_some() || has_border {
                        out.paints.push(Paint::Rect(PaintRect {
                            x,
                            y,
                            width: layout.size.width,
                            height: layout.size.height,
                            background: bg.clone(),
                            radius: *radius,
                            border: *border,
                            border_color: *border_color,
                        }));
                    }
                }
                // Glyphs go in the *content* box, inside this node's own padding and
                // border. Painting them at the border box put a padded label flush
                // against the edge of its own background: the box grew, the words
                // did not move. The size matters as much as the origin, since it is
                // what the run is aligned and wrapped within.
                //
                // **`overflow` clips a text's own glyphs, not only its children.**
                // The clip below is pushed after a node's own paints, which is
                // right for a box (it bounds what is inside it) and left a `<text>`
                // with `height` and `overflow: clip` spilling its lines over
                // everything under it: a text has no children, its content *is*
                // the glyphs. Found on the phone, an event log written with
                // exactly that CSS running down over the page.
                PaintKind::Text(tc) => {
                    let (cx, cy, cw, ch) = content_box(layout);
                    if clip {
                        out.paints.push(Paint::PushClip {
                            x,
                            y,
                            width: layout.size.width,
                            height: layout.size.height,
                            radius: clip_radius,
                        });
                    }
                    out.paints.push(Paint::Text(PaintText {
                        x: x + cx,
                        y: y + cy,
                        width: cw,
                        height: ch,
                        content: tc.clone(),
                    }));
                    if clip {
                        out.paints.push(Paint::PopClip);
                    }
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
                // Drawn after the children, below.
                PaintKind::Outline(_) => {}
                // Each box of a sentence where its glyphs left room for it, and
                // each part's rectangles kept for its regions, made below
                // after the sentence's own so the part is found first.
                PaintKind::Inline(parts) => {
                    let (cx, cy, _, _) = content_box(layout);
                    let geo = walk.inline.get(&id).map(|g| &**g);
                    for part in parts.iter() {
                        let rects = inline::place_part(part, x + cx, y + cy, geo, &mut out.paints);
                        placed.push((part.clone(), rects));
                    }
                }
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
    }

    if !walk.dry {
        // A `for=` label targeting a text input: a focus region at the label's box,
        // carrying the *target's* model, so tapping the label focuses that input.
        if let Some((_, model, row)) = at.focus_label.map(|i| &b.focus_labels[i as usize]) {
            // The scope comes from the input the label points at, not from the
            // label: `for=` names a model, and a model only means anything where it
            // was written. A label that finds no such input carries no instance,
            // which is the same answer as before this field existed.
            let target = b.models.iter().find(|m| m.model == *model && m.row == *row);
            let instance = target.and_then(|b| b.instance.clone());
            // The target's attributes too, because the shell reads them off
            // whichever region shares the field's identity, and a label's region
            // can come first. A `readonly` field focused from its label must still
            // refuse the keyboard.
            let field = target.map(|t| t.field.clone()).unwrap_or_default();
            out.focuses.push(FocusRegion {
                x,
                y,
                width: layout.size.width,
                height: layout.size.height,
                model: model.clone(),
                row: row.clone(),
                instance,
                text: None,
                kind: target.map_or(InputKind::Text, |b| b.kind),
                field,
                scroll_id: None,
            });
        }

        // Assistive technology needs the same geometry the pointer uses, so this rides
        // the same walk. `hidden` nodes returned above, so an `r-show="false"` element
        // is absent from the a11y tree too, not merely invisible.
        if let Some((_, node_access, model)) = at.access.map(|i| &b.access[i as usize]) {
            walk.access_seq.push(at.stack.seq);
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
        if let Some((_, path)) = at.path.map(|i| &b.paths[i as usize]) {
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
        if let Some((_, path)) = at.state.map(|i| &b.states[i as usize]) {
            out.states.push(StateRegion {
                x,
                y,
                width: layout.size.width,
                height: layout.size.height,
                path: path.clone(),
            });
        }

        if let Some((_, handler, gestures, cursor, touch_action, instance, _)) =
            at.handler.map(|i| &b.handlers[i as usize])
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
                touch_action: *touch_action,
                instance: instance.clone(),
            });
        }

        let (fw, fh) = (layout.size.width, layout.size.height);
        if let Some(bound) = at.model.map(|i| &b.models[i as usize]) {
            if let Some(options) = &bound.options {
                // A select: no caret, just a tappable box that opens a dropdown.
                out.selects.push(SelectRegion {
                    x,
                    y,
                    width: fw,
                    height: fh,
                    model: bound.model.clone(),
                    row: bound.row.clone(),
                    instance: bound.instance.clone(),
                    options: options.clone(),
                    field: bound.field.clone(),
                });
                walk.focus_seq.push(at.stack.seq);
                out.focusables.push(FocusItem {
                    x,
                    y,
                    width: fw,
                    height: fh,
                    kind: FocusKind::Select {
                        model: bound.model.clone(),
                        row: bound.row.clone(),
                        instance: bound.instance.clone(),
                        options: options.clone(),
                    },
                    path: bound.focus_path.clone(),
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
                        let content = b.paints(kid).iter().find_map(|(_, k)| match k {
                            PaintKind::Text(tc) => Some(tc.clone()),
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
                    instance: bound.instance.clone(),
                    text: text.clone(),
                    kind: bound.kind,
                    field: bound.field.clone(),
                    // Ids are given out before the walk: see `number_scrollers`.
                    scroll_id: b.scroll_ids.get(&id).copied(),
                });
                walk.focus_seq.push(at.stack.seq);
                out.focusables.push(FocusItem {
                    x,
                    y,
                    width: fw,
                    height: fh,
                    kind: FocusKind::Text {
                        model: bound.model.clone(),
                        row: bound.row.clone(),
                        instance: bound.instance.clone(),
                        kind: bound.kind,
                        text,
                    },
                    path: None,
                    scroll: inside_scroll,
                });
            }
        } else if let Some((_, Some(handler), _, _, _, instance, focus_path)) =
            at.handler.map(|i| &b.handlers[i as usize])
        {
            // A button / checkbox / radio (anything with a `@tap` handler) is
            // keyboard-reachable: Space or Enter runs the same handler as a tap.
            //
            // Only `@tap`. A keyboard has no pointer, so an element that declares
            // only `@drag` has nothing a key could stand in for, and offering Enter
            // as a fake drag would be worse than leaving it alone.
            walk.focus_seq.push(at.stack.seq);
            out.focusables.push(FocusItem {
                x,
                y,
                width: fw,
                height: fh,
                kind: FocusKind::Activate { on_tap: handler.clone(), instance: instance.clone() },
                path: focus_path.clone(),
                scroll: inside_scroll,
            });
        }

        // A sentence's parts answer for themselves, after the sentence, so a
        // tappable run inside a tappable sentence is the one a tap finds.
        for (part, rects) in &placed {
            inline::part_regions(part, rects, child_xform, at.stack.seq, inside_scroll, out, walk);
        }
    }

    // overflow: clip/scroll, bound the subtree to this box (following its corners).
    if clip {
        let c = Clip { x, y, width: layout.size.width, height: layout.size.height, radius: clip_radius };
        out.paints.push(Paint::PushClip { x: c.x, y: c.y, width: c.width, height: c.height, radius: c.radius });
        walk.clips.push(c);
    }

    // A scroller shifts its children by the current offset and registers itself
    // so the wheel, the scrollbars and the keyboard can find it.
    let mut shift = Offset::default();
    // What the children are clipped by: this box if it scrolls, otherwise
    // whatever was clipping us.
    let mut child_scroll = inside_scroll;
    if at.scroll {
        let sid = b.scroll_ids[&id];
        child_scroll = Some(sid);
        // **The end padding is part of what scrolls.** Taffy's content size
        // stops at the far edge of the last child, so without this a scroller
        // could not travel far enough to show its own bottom padding, and the
        // last line of a textarea stayed half under it however far it was
        // scrolled. Reported from the phone while typing past the bottom.
        // CSS counts the padding (css-overflow-3, scrollable overflow), and so
        // does every current browser. The border is added because the clip is
        // the border box.
        //
        //
        // Taffy 0.14 on counts it in every layout mode, as CSS does (before,
        // block layout stopped at the last child and this added the padding
        // for block alone). Its rectangle starts at the padding box, so the
        // border goes on both ends here: see `scroll_extent`.
        let (content_width, content_height) = scroll_extent(layout, at.scroll_rtl);
        // An axis that only clips has nowhere to travel, so nothing (wheel,
        // finger, scrollbar) moves it.
        let (sx, sy) = at.scroll_axes;
        let max = Offset {
            x: if sx { (content_width - layout.size.width).max(0.0) } else { 0.0 },
            y: if sy { (content_height - layout.size.height).max(0.0) } else { 0.0 },
        };
        shift = offsets.get(sid).copied().unwrap_or_default().clamp_to(max);
        walk.scrolls[sid] = Some(ScrollRegion {
            transform: child_xform,
            alpha: child_dim,
            id: sid,
            x,
            y,
            width: layout.size.width,
            height: layout.size.height,
            // The same extent `max` was measured against, end padding in, so
            // the scrollbar's thumb and its travel agree.
            content_width,
            content_height,
            max,
            within: inside_scroll,
            rtl: at.scroll_rtl,
            contain: at.scroll_contain,
            bar: at.scroll_bar,
        });
    }

    // **Positioned children are held back**, along with anything that makes a
    // stacking context, and drawn when the context they belong to reaches
    // their layer: see `Stack`. Everything else is drawn here, in tree order.
    //
    // This replaced two partitions of a box's own children, positioned after
    // in-flow and sticky after those. Both were the right answer for siblings,
    // found by looking: a sticky heading drawn before its rows had them scroll
    // straight through it, and a row leaving with `:leave-to { position:
    // absolute }` was drawn under the row sliding into its place (watchlist
    // #42). CSS asks the same question of the whole context rather than of
    // one parent, and both still come out as they did.
    //
    // Hit testing takes the topmost region, which is the last one, so a box
    // drawn later is also the one a tap finds.
    let mut fresh = context.then(|| Stacking::new(out, walk.clips.len()));
    let ctx = match fresh.as_mut() {
        Some(own) => own,
        None => ctx,
    };
    let items = matches!(
        tree.style(id).expect("style").display,
        taffy::Display::Flex | taffy::Display::Grid
    );
    // A right-to-left scroller starts at its right end, and scrolling moves its
    // content rightwards, bringing in what lies to the left.
    let origin = (if at.scroll_rtl { x + shift.x } else { x - shift.x }, y - shift.y);
    // The box this node's children are held by, measured where the scroll
    // has put it.
    //
    // For a **scroller** that is its *content* box, not the visible one: its
    // children live down the whole scrollable length, and clamping a sticky
    // one to the 200px on screen would pin it to a band that is itself
    // sliding, so it would drift instead of sticking. Everywhere else the two
    // are the same box.
    let holder = (
        origin.0,
        origin.1,
        scroll_extent(layout, at.scroll_rtl).0.max(layout.size.width),
        scroll_extent(layout, at.scroll_rtl).1.max(layout.size.height),
    );
    for &child in tree.children(id).expect("children").iter() {
        let ca = b.at(child);
        if ca.hidden {
            continue;
        }
        // **A `fixed` box is not moved by the root's own scroll.** Every fixed
        // box is handed to the root, and when the `<screen>` itself is the
        // scroller its offset moved them all, so a header written there
        // scrolled out of view (watchlist #43). The root is the window, and a
        // window scrolling is CSS's viewport scrolling, which a fixed box
        // ignores. One under a `transform` is that box's child instead, and
        // goes with it, as CSS says.
        let pinned = at.stack.seq == 0 && ca.stack.position == Position::Fixed;
        match layer(&ca, items) {
            Some((z, own)) => ctx.pending.push(std::cmp::Reverse(Deferred {
                z,
                seq: ca.stack.seq,
                context: own,
                id: child,
                origin: if pinned { (x, y) } else { origin },
                holder,
                inside_scroll: if pinned { inside_scroll } else { child_scroll },
                xform: child_xform,
                dim: child_dim,
                clips: walk.clips[ctx.clip_base..].to_vec(),
            })),
            None => collect(
                tree, child, origin.0, origin.1, b, offsets, vp, holder, child_scroll, child_xform,
                child_dim, false, out, walk, ctx,
            ),
        }
    }
    if context {
        drain(tree, b, offsets, vp, out, walk, ctx);
    }
    if clip {
        out.paints.push(Paint::PopClip);
        walk.clips.pop();
    }
    // **The outline goes over everything the element contains**, as CSS paints
    // it, and after its own clip is popped, because an outline sits outside
    // the box and `overflow` on the element does not cut it. It is still
    // inside the element's transform and opacity, and inside every ancestor's
    // clip, so it slides, fades and scrolls out of view with the element: the
    // focus ring used to be drawn by the shell as a separate layer and had to
    // be taught each of those one at a time.
    for (_, kind) in b.paints(id) {
        if let PaintKind::Outline(o) = kind {
            out.paints.push(outline_paint(o, x, y, layout.size.width, layout.size.height, clip_radius));
        }
    }
    if transform.is_some() {
        out.paints.push(Paint::PopTransform);
    }
    if alpha < 1.0 {
        out.paints.push(Paint::PopOpacity);
    }
}

/// An outline as a stroked rect around a `width` x `height` box at `(x, y)`
/// whose corners are `radius`.
///
/// Drawn as a border on the rect `offset + width` outside the box, since a
/// border is drawn inward. Each rounded corner grows by the same distance, so
/// the line stays parallel to the curve; a square corner stays square. An
/// `auto` ring is always a little round, as a platform's focus ring is.
fn outline_paint(o: &Outline, x: f32, y: f32, width: f32, height: f32, radius: Corners) -> Paint {
    let out = o.offset + o.width;
    let corner = |r: f32| {
        let grown = if r > 0.0 { (r + out).max(0.0) } else { 0.0 };
        if o.auto {
            grown.max(4.0)
        } else {
            grown
        }
    };
    Paint::Rect(PaintRect {
        x: x - out,
        y: y - out,
        width: (width + 2.0 * out).max(0.0),
        height: (height + 2.0 * out).max(0.0),
        background: None,
        radius: [corner(radius[0]), corner(radius[1]), corner(radius[2]), corner(radius[3])],
        border: Sides::uniform(o.width),
        border_color: Some(o.color.unwrap_or(Outline::RING)),
    })
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
///
/// Builds the whole Taffy tree and lays it all out. A caller laying out frame
/// after frame keeps a [`LayoutState`] instead.
pub fn layout_scrolled(
    root: &Node,
    avail_w: f32,
    avail_h: f32,
    offsets: &[Offset],
    measure: &mut Measure,
) -> Layout {
    LayoutState::new().lay_out(root, avail_w, avail_h, offsets, &mut SizeOnly(measure))
}

/// The Taffy tree kept from one frame to the next, so a frame lays out only
/// what changed (track (g).4 of `docs/11-next.md`).
///
/// Each frame's nodes are matched to the last frame's (see `Kept`), and a node
/// whose style, text, text cap and children are all as they were keeps its
/// Taffy node and Taffy's cache. Rotating a list of 300 rows then dirties the
/// list and its ancestors, and Taffy answers for every row from its cache.
///
/// Debug builds lay every frame out again from scratch and panic if the two
/// differ in anything, paints, regions or metrics.
pub struct LayoutState {
    tree: TaffyTree<TextContent>,
    kept: Option<Kept>,
    /// Taffy nodes found unmatched while building, removed after.
    garbage: Vec<NodeId>,
    /// Each anchored box drawn at a fallback last frame, by its path. See
    /// [`Layout::position_tries`].
    tried: HashMap<Vec<usize>, (u8, Option<String>)>,
    /// `content-visibility: auto`: whether each such box was near the
    /// screen last frame, and the content size each was last drawn at, for
    /// `contain-intrinsic-size: auto`. By path. See [`Layout::contained`].
    shown: HashMap<Vec<usize>, bool>,
    sizes: HashMap<Vec<usize>, (f32, f32)>,
}

impl Default for LayoutState {
    fn default() -> Self {
        Self::new()
    }
}

impl LayoutState {
    pub fn new() -> Self {
        let mut tree: TaffyTree<TextContent> = TaffyTree::new();
        // Taffy rounds boxes to whole pixels by default, which can shave a
        // fraction off a text box and make paint re-wrap the last word into a
        // line the box has no height for. Keep the exact sizes measure asked for.
        tree.disable_rounding();
        Self {
            tree,
            kept: None,
            garbage: Vec::new(),
            tried: HashMap::new(),
            shown: HashMap::new(),
            sizes: HashMap::new(),
        }
    }

    /// Drop everything kept, so the next frame is laid out whole.
    ///
    /// For when `measure` would now answer differently for the same text, as
    /// it does once a font is registered: Taffy's cache holds the old answers
    /// and has no way to know.
    pub fn forget(&mut self) {
        *self = Self::new();
    }

    /// [`layout_scrolled`], reusing what the last call laid out.
    ///
    /// `measure` must answer the same for the same text, font and width from
    /// one call to the next (see [`LayoutState::forget`]); it is asked again
    /// only about a text that changed in something a size can depend on.
    pub fn layout_scrolled(
        &mut self,
        root: &Node,
        avail_w: f32,
        avail_h: f32,
        offsets: &[Offset],
        measure: &mut Measure,
    ) -> Layout {
        self.lay_out_text(root, avail_w, avail_h, offsets, &mut SizeOnly(measure))
    }

    /// [`LayoutState::layout_scrolled`] with a text engine that can say where
    /// the parts of a sentence land, so a `<text>` with runs and inline boxes
    /// is placed and hit-tested part by part. The shell lays out through this.
    pub fn lay_out_text(
        &mut self,
        root: &Node,
        avail_w: f32,
        avail_h: f32,
        offsets: &[Offset],
        measure: &mut dyn TextMeasure,
    ) -> Layout {
        let out = self.lay_out(root, avail_w, avail_h, offsets, measure);
        #[cfg(debug_assertions)]
        {
            let mut fresh = LayoutState::new();
            fresh.tried = self.tried.clone();
            fresh.shown = self.shown.clone();
            fresh.sizes = self.sizes.clone();
            let whole = fresh.lay_out(root, avail_w, avail_h, offsets, measure);
            let (kept, whole) = (format!("{out:#?}"), format!("{whole:#?}"));
            if kept != whole {
                let at = kept.lines().zip(whole.lines()).position(|(a, b)| a != b);
                let line = |s: &str| at.and_then(|n| s.lines().nth(n)).unwrap_or("").to_string();
                panic!(
                    "a kept layout differs from a whole one at line {at:?}: kept `{}`, whole `{}`",
                    line(&kept),
                    line(&whole)
                );
            }
        }
        self.tried = out.position_tries.iter().map(|(p, n, a)| (p.clone(), (*n, a.clone()))).collect();
        self.shown = out.contained.iter().map(|c| (c.path.clone(), c.near)).collect();
        // A size is remembered while its box is in the document, drawn or not.
        self.sizes.retain(|p, _| out.contained.iter().any(|c| &c.path == p));
        for c in &out.contained {
            if let Some(size) = c.size {
                self.sizes.insert(c.path.clone(), size);
            }
        }
        out
    }

    /// One frame: laid out again while a box whose contents were skipped
    /// turns out to be on screen, which happens when it scrolls in faster
    /// than the margin around the screen can see it coming, and on the
    /// first frame, when nothing is known. Three passes at most; a box near
    /// the screen but not on it waits for the next frame.
    fn lay_out(
        &mut self,
        root: &Node,
        avail_w: f32,
        avail_h: f32,
        offsets: &[Offset],
        measure: &mut dyn TextMeasure,
    ) -> Layout {
        // Last frame's answers, put back untouched: the caller keeps them
        // until it has compared this layout with a whole one.
        let before = std::mem::take(&mut self.shown);
        let mut again: Option<HashMap<Vec<usize>, bool>> = None;
        let mut pass = 0;
        loop {
            let out = self.lay_out_pass(root, avail_w, avail_h, offsets, measure, again.as_ref().unwrap_or(&before));
            pass += 1;
            if pass == 3 || !out.contained.iter().any(|c| c.skipped && c.on_screen) {
                self.shown = before;
                return out;
            }
            let next = again.get_or_insert_with(|| before.clone());
            for c in &out.contained {
                next.insert(c.path.clone(), c.near);
            }
        }
    }

    fn lay_out_pass(
        &mut self,
        root: &Node,
        avail_w: f32,
        avail_h: f32,
        offsets: &[Offset],
        measure: &mut dyn TextMeasure,
        shown: &HashMap<Vec<usize>, bool>,
    ) -> Layout {
        let mut skipping = Skipping { shown, sizes: &self.sizes, intrinsic: IdMap::default(), boxes: Vec::new() };
        let tree = &mut self.tree;
        let tried_before = &self.tried;
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
        let mut caps: IdMap<f32> = IdMap::default();
        let mut stickies: Vec<(NodeId, [Option<f32>; 4])> = Vec::new();
        let mut hoisted: Vec<(NodeId, bool)> = Vec::new();
        let mut stacks: Vec<(NodeId, Stack)> = Vec::new();
        let mut anchoring = Anchoring::default();
        let (root_id, kept) = build(
            tree,
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
            // The root is forced to the viewport, so that is the widest anything
            // can be.
            Some(avail_w),
            &mut caps,
            None, // the root is not inside any row
            &mut stickies,
            &mut hoisted,
            &mut stacks,
            &mut anchoring,
            &mut skipping,
            self.kept.take(),
            Some(vp),
            &mut self.garbage,
        );
        debug_assert!(hoisted.is_empty(), "the root takes every out-of-flow box");
        self.kept = Some(kept);
        // What nothing matched. `bury` lists a subtree from its top down, and
        // removing a node unhooks its children, so a node's parent here is always
        // still in the tree: one this frame kept, whose layout has to be redone
        // without it. Taffy's `remove` marks that parent dirty itself (0.14 on;
        // 0.7 did not, and this loop did it by hand).
        for id in self.garbage.drain(..) {
            tree.remove(id).expect("taffy remove");
        }

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

        // Taffy 0.14 on hands a leaf its whole layout input; `compute_leaf_layout`
        // is the leaf sizing it did before, around the text's own size below.
        let mut measure_fn = |inputs: taffy::LayoutInput,
                              id: NodeId,
                              ctx: Option<&mut TextContent>,
                              style: &taffy::Style| {
            taffy::compute_leaf_layout(inputs, style, |_, _| 0.0, |known: Size<Option<f32>>, available: Size<AvailableSpace>| {
                if let (Some(w), Some(h)) = (known.width, known.height) {
                    return Size { width: w, height: h };
                }
                // A box whose contents were skipped is the size given for them.
                if let Some(&(w, h)) = skipping.intrinsic.get(&id) {
                    return Size { width: known.width.unwrap_or(w), height: known.height.unwrap_or(h) };
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
                        let (w, h) = measure.measure(tc, max);
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
            })
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

        // `anchor-size()`, and an axis with both insets on one anchor: sizes
        // only the anchor's own layout can answer, given once it has one.
        let mut sized = false;
        for (id, a) in &anchoring.boxes {
            for vertical in [false, true] {
                let Some(px) = anchored_size(a, vertical, &anchoring, tree, vp) else { continue };
                let mut st = tree.style(*id).expect("style").clone();
                if vertical {
                    st.size.height = length(px);
                } else {
                    st.size.width = length(px);
                }
                tree.set_style(*id, st).expect("sized from its anchor");
                sized = true;
            }
        }
        if sized {
            tree.compute_layout_with_measure(root_id, space, &mut measure_fn)
                .expect("compute layout with anchor sizes");
        }

        let at = index(
            &paint, &handlers, &models, &focus_labels, &hidden, &opacities, &scrolls, &transforms,
            &states, &access, &paths, &stickies, &stacks, &anchoring,
        );
        let mut at = at;
        for (i, (id, _)) in skipping.boxes.iter().enumerate() {
            at.entry(*id).or_default().contained = Some(i as u32);
        }
        let built = Built {
            paint: &paint,
            handlers: &handlers,
            models: &models,
            focus_labels: &focus_labels,
            transforms: &transforms,
            opacities: &opacities,
            states: &states,
            access: &access,
            paths: &paths,
            stickies: &stickies,
            contained: &skipping.boxes,
            scroll_ids: {
                let mut ids = IdMap::default();
                number_scrollers(&tree, root_id, &at, &mut ids);
                ids
            },
            anchoring: &anchoring,
            at,
        };
        // Where the parts of each sentence landed, now that each has the width
        // it is drawn at. Its `Text` paint comes just before its `Inline` one.
        let mut geometry: IdMap<std::sync::Arc<InlineGeometry>> = IdMap::default();
        let mut last_text: Option<(NodeId, &TextContent)> = None;
        for (id, kind) in &paint {
            match kind {
                PaintKind::Text(tc) => last_text = Some((*id, tc)),
                PaintKind::Inline(_) => {
                    if let Some((tid, tc)) = last_text.filter(|(tid, _)| tid == id) {
                        let (_, _, width, _) = content_box(tree.layout(tid).expect("layout"));
                        geometry.insert(tid, measure.inline(tc, width));
                    }
                }
                _ => {}
            }
        }
        // Shared by the walks, of which there are two when anything is anchored.
        let geometry = std::rc::Rc::new(geometry);
        let tried_before = std::rc::Rc::new(tried_before.clone());
        let walk_once = |known: Option<HashMap<String, AnchorBox>>, dry: bool| {
            let mut out = Layout::default();
            let mut walk = Walk {
                scrolls: vec![None; built.scroll_ids.len()],
                known,
                dry,
                tried_before: std::rc::Rc::clone(&tried_before),
                inline: std::rc::Rc::clone(&geometry),
                ..Walk::default()
            };
            // The root is the outermost stacking context. This one only holds
            // its place: the root makes its own, as every context does.
            let mut outer = Stacking::new(&out, 0);
            collect(
                &tree, root_id, 0.0, 0.0, &built, offsets, vp,
                // The root is held by the window.
                (0.0, 0.0, vp.0, vp.1),
                None, None, 1.0, true, &mut out, &mut walk, &mut outer,
            );
            debug_assert!(outer.pending.is_empty() && walk.clips.is_empty());
            (out, walk)
        };
        // Anything anchored is drawn by a second walk, against where the
        // first one drew its anchors. See `Anchoring`.
        let anchored = !anchoring.boxes.is_empty();
        let (mut out, mut walk) = walk_once(None, anchored);
        if anchored {
            (out, walk) = walk_once(Some(std::mem::take(&mut walk.seen)), false);
            walk.misses.sort();
            walk.misses.dedup();
            out.anchor_misses = std::mem::take(&mut walk.misses);
            walk.tried.sort();
            out.position_tries = std::mem::take(&mut walk.tried);
        }
        out.contained = std::mem::take(&mut walk.contained);
        out.scrolls = walk.scrolls.into_iter().flatten().collect();
        debug_assert!(out.scrolls.iter().enumerate().all(|(i, s)| s.id == i), "every numbered scroller is drawn");
        // Painting order is not reading order. Tab and assistive technology
        // follow the document, whatever is drawn over what.
        out.focusables = in_order(std::mem::take(&mut out.focusables), &walk.focus_seq);
        out.access = in_order(std::mem::take(&mut out.access), &walk.access_seq);
        out
    }
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
            touch_action: TouchAction::default(),
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
            within: None,
            rtl: false,
            contain: (false, false),
            bar: Scrollbar::default(),
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
            within: Some(0),
            rtl: false,
            contain: (false, false),
            bar: Scrollbar::default(),
        });
        let offsets = vec![Offset { x: 0.0, y: 0.0 }, Offset { x: 0.0, y: 0.0 }];
        assert_eq!(containing_scroller(&scrolls, &offsets, 40.0, 500.0), Some(1));
    }
}

#[cfg(test)]
mod mask_tests {
    use super::{mask, masked_offset, unmasked_offset};

    /// **The caret bug, in arithmetic.** A bullet is three bytes in UTF-8 and a
    /// caret is a byte offset into the *real* text, so reading one as the other
    /// puts the caret a third of the way along. Reported from the phone twice:
    /// six characters typed and the caret sat after two bullets, and "the
    /// longer the value, the further forward it moves, about one character for
    /// every three".
    #[test]
    fn a_caret_maps_from_real_bytes_to_bullets() {
        let value = "abcdef";
        assert_eq!(mask(value).len(), 18, "six bullets, three bytes each");
        // What the caret meant before the mapping existed: byte 6 of an
        // 18-byte string, which the painter reads as two bullets in.
        assert_eq!(6 / super::SECRET_BULLET.len_utf8(), 2, "the reported symptom");
        // And what it means now.
        assert_eq!(masked_offset(value, 6), 18, "the end is the end");
        assert_eq!(masked_offset(value, 3), 9, "and half way is half way");
    }

    /// A tap lands on a bullet and has to come back as an offset in the value.
    #[test]
    fn a_tap_maps_from_bullets_back_to_real_bytes() {
        let value = "abcdef";
        for byte in [0usize, 3, 6] {
            assert_eq!(unmasked_offset(value, masked_offset(value, byte)), byte);
        }
    }

    /// Multi-byte text is where clamping and mapping visibly disagree.
    ///
    /// `é` is two bytes and one bullet, so a caret after it is byte 2 in the
    /// value and byte 3 in the mask. Clamping would have left it at 2, which is
    /// not a boundary in a string of three-byte bullets at all.
    #[test]
    fn a_multibyte_character_is_one_bullet() {
        let value = "é";
        assert_eq!(value.len(), 2);
        assert_eq!(mask(value).chars().count(), 1);
        assert_eq!(masked_offset(value, 2), 3);
        assert_eq!(unmasked_offset(value, 3), 2);
    }
}

#[cfg(test)]
mod touch_action_tests {
    use super::TouchAction;

    /// The axis claim in one table. `scroller_may_take` is asked once, at the
    /// slop crossing, and its answer decides whether a `@drag` ever starts.
    ///
    /// The row that matters most is `Auto` with a vertical gesture: before this
    /// existed, a `@drag` took every finger in every direction, which made a
    /// draggable row inside a scrolling list impossible to scroll past.
    #[test]
    fn scroller_may_take_by_axis() {
        // vertical = true means the gesture's dominant axis is vertical.
        assert!(TouchAction::Auto.scroller_may_take(true));
        assert!(TouchAction::Auto.scroller_may_take(false));

        // The opt-out: the element owns the finger on both axes.
        assert!(!TouchAction::None.scroller_may_take(true));
        assert!(!TouchAction::None.scroller_may_take(false));

        // `pan-x` keeps horizontal scrolling and gives the element vertical.
        assert!(!TouchAction::PanX.scroller_may_take(true));
        assert!(TouchAction::PanX.scroller_may_take(false));

        // `pan-y` is the mirror, and is what a horizontal carousel inside a
        // vertical page would declare if it wanted the page to keep vertical.
        assert!(TouchAction::PanY.scroller_may_take(true));
        assert!(!TouchAction::PanY.scroller_may_take(false));
    }

    /// The default is CSS's `auto`, not "the element wins". A change here is a
    /// change to what every existing document does with a finger.
    #[test]
    fn default_is_auto() {
        assert_eq!(TouchAction::default(), TouchAction::Auto);
    }
}

#[cfg(test)]
mod date_tests {
    use super::*;

    /// A day that exists, written as HTML writes one or with the padding
    /// left off; a day that does not is not a date.
    #[test]
    fn a_date_is_a_day_that_exists() {
        assert_eq!(parse_date("2026-09-23"), Some((2026, 9, 23)));
        assert_eq!(parse_date("2026-9-3"), Some((2026, 9, 3)));
        assert_eq!(parse_date("2024-02-29"), Some((2024, 2, 29)), "a leap day");
        for not in ["", "2026", "2026-09", "2025-02-29", "1900-02-29", "2026-13-01", "2026-04-31",
                    "2026-00-10", "26-09-23", "2026-09-23-1", "2026/09/23", "2026-09-2x"] {
            assert_eq!(parse_date(not), None, "{not:?}");
        }
        assert_eq!(format_date((2026, 9, 3)), "2026-09-03");
    }
}
