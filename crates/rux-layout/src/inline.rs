//! A `<text>` with elements inside it, joined into one sentence.
//!
//! The build hands the layout a `<text>` node whose children are its pieces in
//! order: runs of words (a nested `<text>`, or the anonymous run the parser
//! made of the words between two tags) and boxes (`<image>`, `<path>`). They
//! stay separate nodes up to here, because each is an element of its own: it
//! has its own bindings, its own `r-if`, its own `@tap`. Here they become one
//! [`TextContent`] with a [`RichText`], which is what is measured, broken into
//! lines and drawn, and a list of [`InlinePart`]s, which remember which element
//! each run or box came from so it can be placed and answer a tap.
//!
//! Spaces are collapsed across the pieces, with each piece's own
//! `white-space`: a space ending one run and a space starting the next are one
//! space. A box counts as a word, so spaces either side of it stay.

use std::sync::Arc;

use crate::rich::{InlineItem, RichText, RunStyle, TextRun};
use crate::white_space::collapse_runs_into;
use crate::{
    Access, AccessRole, Background, BoxShadow, Corners, Cursor, Display, Gesture, ImageContent, Len, Node,
    PathContent, PathPaint, Rgba, Sides, TextContent, TouchAction, WhiteSpace,
};

/// The character a box stands on while spaces are collapsed. It is never in
/// the text that is shaped: a box is a gap in the line, not a glyph.
const BOX: &str = "\u{FFFC}";

/// One element of a sentence: where it is in the joined text, and what it
/// answers to.
#[derive(Clone, Debug)]
pub(crate) struct InlinePart {
    pub what: PartWhat,
    /// Its tree path, for its metrics (`query()`).
    pub path: Vec<usize>,
    pub on_tap: Option<String>,
    pub gestures: Vec<(Gesture, String)>,
    pub cursor: Cursor,
    pub touch_action: TouchAction,
    pub instance: Option<String>,
    pub focus_path: Option<Vec<usize>>,
    /// The path `:hover` and `:active` are matched by, when a rule could.
    pub state: Option<Vec<usize>>,
    pub access: Access,
}

#[derive(Clone, Debug)]
pub(crate) enum PartWhat {
    /// These runs of the paragraph: one for a run of words, several for a
    /// nested `<text>` that has pieces of its own.
    Runs(std::ops::Range<usize>),
    /// This inline box, drawn as the element it is.
    Item { index: usize, width: f32, height: f32, look: ItemLook },
}

/// What an inline box draws: its own box (background, border), then its
/// content.
#[derive(Clone, Debug)]
pub(crate) struct ItemLook {
    pub bg: Option<Background>,
    pub radius: Corners,
    pub border: Sides,
    pub border_color: Option<Rgba>,
    pub shadow: Option<BoxShadow>,
    pub content: ItemContent,
}

#[derive(Clone, Debug)]
pub(crate) enum ItemContent {
    Image(ImageContent),
    Path(PathContent, PathPaint),
}

/// Whether a `<text>` node is a sentence of pieces rather than plain words.
pub(crate) fn is_sentence(node: &Node) -> bool {
    node.text.is_some() && !node.children.is_empty()
}

/// Join a sentence's pieces into the text the layout measures and draws.
pub(crate) fn join(node: &Node, path: &[usize]) -> (TextContent, Vec<InlinePart>) {
    let mut base = node.text.clone().expect("a sentence is a text");
    let mut pieces: Vec<(&str, WhiteSpace)> = Vec::new();
    let mut kinds: Vec<Piece> = Vec::new();
    let mut parts: Vec<InlinePart> = Vec::new();
    gather(node, path, &mut pieces, &mut kinds, &mut parts);

    // One string, the boxes' stand-ins included, then the stand-ins taken
    // out again: a box is a place in the text, not a character of it.
    let mut joined = String::new();
    let ranges = collapse_runs_into(&pieces, &mut joined);
    let mut text = String::with_capacity(joined.len());
    let mut rich = RichText::default();
    for (range, kind) in ranges.into_iter().zip(kinds) {
        match kind {
            Piece::Run(style) => {
                let start = text.len();
                text.push_str(&joined[range]);
                rich.runs.push(TextRun { range: start..text.len(), style });
            }
            Piece::Item { width, height, align } => {
                rich.items.push(InlineItem { index: text.len(), width, height, align });
            }
        }
    }
    base.text = text;
    base.rich = Some(Arc::new(rich));
    (base, parts)
}

enum Piece {
    Run(RunStyle),
    Item { width: f32, height: f32, align: crate::VerticalAlign },
}

fn gather<'a>(
    node: &'a Node,
    path: &[usize],
    pieces: &mut Vec<(&'a str, WhiteSpace)>,
    kinds: &mut Vec<Piece>,
    parts: &mut Vec<InlinePart>,
) {
    for (i, c) in node.children.iter().enumerate() {
        // Not drawn, not in the sentence: `r-show="false"` takes its words
        // out, as `display: none` does.
        if c.hidden || c.style.display == Display::None {
            continue;
        }
        let mut cp = path.to_vec();
        cp.push(i);
        let first_run = count_runs(kinds);
        let what = if let Some(tc) = &c.text {
            if c.children.is_empty() {
                pieces.push((tc.text.as_str(), tc.white_space));
                kinds.push(Piece::Run(run_style(tc, c)));
            } else {
                gather(c, &cp, pieces, kinds, parts);
            }
            PartWhat::Runs(first_run..count_runs(kinds))
        } else if let Some(ic) = &c.image {
            let (width, height) = size(c, ic.intrinsic);
            let index = count_items(kinds);
            pieces.push((BOX, WhiteSpace::Normal));
            kinds.push(Piece::Item { width, height, align: c.style.vertical_align });
            PartWhat::Item { index, width, height, look: look(c, ItemContent::Image(ic.clone())) }
        } else if let Some(pc) = &c.path {
            let bounds = pc.bounds().map_or((0.0, 0.0), |b| (b.2.max(0.0), b.3.max(0.0)));
            let (width, height) = size(c, bounds);
            let index = count_items(kinds);
            pieces.push((BOX, WhiteSpace::Normal));
            kinds.push(Piece::Item { width, height, align: c.style.vertical_align });
            let paint = PathPaint {
                fill: c.style.fill,
                fill_rule: c.style.fill_rule,
                stroke: c.style.stroke,
                stroke_width: c.style.stroke_width,
                cap: c.style.stroke_linecap,
                join: c.style.stroke_linejoin,
            };
            PartWhat::Item { index, width, height, look: look(c, ItemContent::Path(pc.clone(), paint)) }
        } else {
            // Nothing else can be in a sentence: the build refuses it.
            continue;
        };
        parts.push(InlinePart {
            what,
            path: cp,
            on_tap: c.on_tap.clone(),
            gestures: c.gestures.clone(),
            cursor: c.style.cursor,
            touch_action: c.style.touch_action,
            instance: c.instance.clone(),
            focus_path: c.focus_path.clone(),
            state: c.state_path.clone(),
            access: c.access.clone(),
        });
    }
}

fn count_runs(kinds: &[Piece]) -> usize {
    kinds.iter().filter(|k| matches!(k, Piece::Run(_))).count()
}

fn count_items(kinds: &[Piece]) -> usize {
    kinds.iter().filter(|k| matches!(k, Piece::Item { .. })).count()
}

/// A run's style: everything its text carries, and a solid `background` as
/// the highlight behind its words.
fn run_style(tc: &TextContent, node: &Node) -> RunStyle {
    RunStyle {
        font_size: tc.font_size,
        weight: tc.weight,
        italic: tc.italic,
        family: tc.font_family.clone(),
        letter_spacing: tc.letter_spacing,
        word_spacing: tc.word_spacing,
        underline: tc.underline,
        strikethrough: tc.strikethrough,
        color: tc.color,
        background: match &node.style.background {
            Some(Background::Color(c)) => Some(*c),
            _ => None,
        },
    }
}

/// A box's size in the line: its CSS `width` and `height` in pixels, else its
/// own (an image's pixels, a path's geometry). A percentage has no box to be a
/// percentage of inside a line, so it falls back too.
fn size(node: &Node, own: (f32, f32)) -> (f32, f32) {
    let px = |l: Option<Len>| match l {
        Some(Len::Px(v)) => Some(v),
        _ => None,
    };
    let (w, h) = (px(node.style.width), px(node.style.height));
    match (w, h) {
        (Some(w), Some(h)) => (w, h),
        // One side given keeps the other in proportion, as an `<img>` does.
        (Some(w), None) if own.0 > 0.0 => (w, own.1 * w / own.0),
        (None, Some(h)) if own.1 > 0.0 => (own.0 * h / own.1, h),
        (w, h) => (w.unwrap_or(own.0), h.unwrap_or(own.1)),
    }
}

fn look(node: &Node, content: ItemContent) -> ItemLook {
    ItemLook {
        bg: node.style.background.clone(),
        radius: node.style.radius,
        border: node.style.border,
        border_color: node.style.border_color,
        shadow: node.style.box_shadow,
        content,
    }
}

impl InlinePart {
    /// Whether a pointer can reach it, so it needs a hit region.
    fn reachable(&self) -> bool {
        self.on_tap.is_some() || !self.gestures.is_empty()
    }
}

/// Paint one part of a sentence whose content box starts at `(ox, oy)`, and
/// say where it is: one rectangle for a box, one per line for a run. A part
/// the engine gave no place to has none, and a box with no place sits at the
/// sentence's corner rather than nowhere, which is where it is easiest to see
/// that something is off.
pub(crate) fn place_part(
    part: &InlinePart,
    ox: f32,
    oy: f32,
    geo: Option<&crate::InlineGeometry>,
    paints: &mut Vec<crate::Paint>,
) -> Vec<crate::Rect4> {
    use crate::{Paint, PaintImage, PaintPath, PaintRect};
    match &part.what {
        PartWhat::Runs(runs) => {
            let Some(g) = geo else { return Vec::new() };
            runs.clone()
                .filter_map(|r| g.runs.get(r))
                .flatten()
                .map(|&(x, y, w, h)| (ox + x, oy + y, w, h))
                .collect()
        }
        PartWhat::Item { index, width, height, look } => {
            let (ix, iy) = geo.and_then(|g| g.items.get(*index)).copied().unwrap_or((0.0, 0.0));
            let (x, y, w, h) = (ox + ix, oy + iy, *width, *height);
            if let Some(sh) = look.shadow.filter(|s| !s.inset) {
                paints.push(Paint::Shadow {
                    x: x + sh.dx - sh.spread,
                    y: y + sh.dy - sh.spread,
                    width: w + 2.0 * sh.spread,
                    height: h + 2.0 * sh.spread,
                    radius: look.radius.iter().copied().fold(0.0, f32::max),
                    blur: sh.blur,
                    color: sh.color,
                });
            }
            let widest = look.border.top.max(look.border.right).max(look.border.bottom).max(look.border.left);
            if look.bg.is_some() || (widest > 0.0 && look.border_color.is_some()) {
                paints.push(Paint::Rect(PaintRect {
                    x,
                    y,
                    width: w,
                    height: h,
                    background: look.bg.clone(),
                    radius: look.radius,
                    border: look.border,
                    border_color: look.border_color,
                }));
            }
            match &look.content {
                ItemContent::Image(ic) => {
                    paints.push(Paint::Image(PaintImage { x, y, width: w, height: h, content: ic.clone() }))
                }
                ItemContent::Path(pc, pp) => {
                    paints.push(Paint::Path(PaintPath { x, y, content: pc.clone(), paint: *pp }))
                }
            }
            vec![(x, y, w, h)]
        }
    }
}

/// What a part answers to, at the rectangles it was placed in: a tap, a
/// hover, Tab, a screen reader and `query()`, each the way the same element
/// would as a box of its own.
#[allow(clippy::too_many_arguments)]
pub(crate) fn part_regions(
    part: &InlinePart,
    rects: &[crate::Rect4],
    transform: Option<crate::Transform>,
    seq: u32,
    scroll: Option<usize>,
    out: &mut crate::Layout,
    walk: &mut crate::Walk,
) {
    use crate::{AccessNode, FocusItem, FocusKind, HitRegion, NodeMetrics, StateRegion};
    let Some(bound) = bounding(rects) else { return };
    let (bx, by, bw, bh) = bound;
    if part.reachable() {
        for &(x, y, width, height) in rects {
            out.hits.push(HitRegion {
                transform,
                x,
                y,
                width,
                height,
                on_tap: part.on_tap.clone(),
                gestures: part.gestures.clone(),
                cursor: part.cursor,
                touch_action: part.touch_action,
                instance: part.instance.clone(),
            });
        }
    }
    if let Some(path) = &part.state {
        for &(x, y, width, height) in rects {
            out.states.push(StateRegion { x, y, width, height, path: path.clone() });
        }
    }
    // A link in a sentence is a Tab stop, as it is in a browser: Enter runs
    // it. Its focus ring is drawn around all of it.
    if let Some(on_tap) = &part.on_tap {
        walk.focus_seq.push(seq);
        out.focusables.push(FocusItem {
            x: bx,
            y: by,
            width: bw,
            height: bh,
            kind: FocusKind::Activate { on_tap: on_tap.clone(), instance: part.instance.clone() },
            path: part.focus_path.clone(),
            scroll,
        });
    }
    if part.access.role != AccessRole::None {
        walk.access_seq.push(seq);
        out.access.push(AccessNode { x: bx, y: by, width: bw, height: bh, access: part.access.clone(), model: None });
    }
    out.metrics.push(NodeMetrics { path: part.path.clone(), x: bx, y: by, width: bw, height: bh });
}

/// The smallest rectangle around all of them, `None` for none.
fn bounding(rects: &[crate::Rect4]) -> Option<crate::Rect4> {
    let first = rects.first()?;
    let (mut l, mut t, mut r, mut b) = (first.0, first.1, first.0 + first.2, first.1 + first.3);
    for &(x, y, w, h) in &rects[1..] {
        l = l.min(x);
        t = t.min(y);
        r = r.max(x + w);
        b = b.max(y + h);
    }
    Some((l, t, r - l, b - t))
}
