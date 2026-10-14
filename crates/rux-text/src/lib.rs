//! Rux text engine, milestones M4, plus text-metric/weight/alignment fixes.
//!
//! Wraps `parley` (shaping + line layout over the system fonts) and draws the
//! resulting glyph runs into a `vello::Scene`. It owns the font and layout
//! contexts so they're built once and reused: `measure` feeds `taffy`'s leaf
//! sizing (Stage 4), `draw` paints the glyphs (Stage 5).
//!
//! Text is sized and drawn with **leading trimmed**: a line's box is
//! `ascent + descent` (not the full `line_height`, which adds gap above/below),
//! and the baseline sits at `top + ascent`. This makes text hug its box so
//! `padding` reads equally on all sides.
//!
//! Colour is applied by vello at draw time, not carried through parley. The
//! layout brush is only a run number: 0 for the paragraph's own style, `i + 1`
//! for run `i` of a [`RichText`], which is how a drawn glyph run finds its
//! colour and decorations.
//!
//! **A paragraph with runs** (`Read the <text>terms</text>`, an icon in a
//! sentence) is one parley layout with ranged styles and inline boxes, so it
//! breaks, selects and hit-tests as one text. Its lines are measured here, not
//! by parley: parley stands every inline box on the baseline, and
//! `vertical-align` can put one across it or below it.

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Arc;

use parley::{
    Affinity, Alignment, BreakReason, AlignmentOptions, Cursor, FontContext, FontWeight, InlineBox, InlineBoxKind,
    Layout, LayoutContext, Line, LineHeight, OverflowWrap, PositionedLayoutItem, Selection, StyleProperty,
    TextWrapMode,
};
use parley::style::{FontFamily, FontStyle};
use rux_layout::{InlineGeometry, RichText, RunStyle};
use vello::kurbo::{Affine, Rect};
use vello::peniko::{Color, Fill};
use vello::{Glyph, Scene};

/// Which run a glyph run belongs to: 0 is the paragraph's own style.
type RunId = u32;

/// Caret thickness, in logical pixels.
pub const CARET_WIDTH: f32 = 1.5;

/// Horizontal text alignment within the text box (`text-align`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Align {
    #[default]
    Start,
    Center,
    End,
    Justify,
}

impl Align {
    fn to_parley(self) -> Alignment {
        match self {
            Align::Start => Alignment::Start,
            Align::Center => Alignment::Center,
            Align::End => Alignment::End,
            Align::Justify => Alignment::Justify,
        }
    }
}

/// How a line may break when a word is wider than its box (`overflow-wrap`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Wrap {
    /// Only break between words. A long word overflows, as in CSS.
    #[default]
    Normal,
    /// Break inside a word rather than overflow (`overflow-wrap: break-word`).
    BreakWord,
    /// Break anywhere (`word-break: break-all`).
    Anywhere,
}

impl Wrap {
    fn to_parley(self) -> OverflowWrap {
        match self {
            Wrap::Normal => OverflowWrap::Normal,
            Wrap::BreakWord => OverflowWrap::BreakWord,
            Wrap::Anywhere => OverflowWrap::Anywhere,
        }
    }
}

/// The shaping inputs for a run of text, gathered into one struct so the engine
/// methods don't take a dozen positional arguments. Everything except
/// `font_size`/`weight` is optional and off by default. Lengths are logical px.
#[derive(Clone, Copy, Debug)]
pub struct TextStyle<'a> {
    pub font_size: f32,
    pub weight: u16,
    pub wrap: Wrap,
    /// `font-family` as a raw CSS list; `None` uses the system default.
    pub family: Option<&'a str>,
    /// `letter-spacing` / `word-spacing`, extra px between letters / words.
    pub letter_spacing: Option<f32>,
    pub word_spacing: Option<f32>,
    /// `line-height`, as an absolute pixel value. `None` uses the font's metrics
    /// (leading trimmed, so text hugs its box).
    pub line_height: Option<f32>,
    /// `font-style: italic`.
    pub italic: bool,
    /// `text-decoration: underline` / `line-through`.
    pub underline: bool,
    pub strikethrough: bool,
    /// `white-space: nowrap`: never break lines, even past `max_width`.
    pub nowrap: bool,
    /// The runs and inline boxes of a paragraph that has them.
    pub rich: Option<&'a RichText>,
}

impl<'a> TextStyle<'a> {
    /// A plain run at the given size/weight, no family and no extras.
    pub fn new(font_size: f32, weight: u16, wrap: Wrap) -> Self {
        Self {
            font_size,
            weight,
            wrap,
            family: None,
            letter_spacing: None,
            word_spacing: None,
            line_height: None,
            italic: false,
            underline: false,
            strikethrough: false,
            nowrap: false,
            rich: None,
        }
    }
}

/// One line as it is stacked: its top and height within the text, and where
/// its baseline falls. Lines are stacked with the leading trimmed, see the
/// module doc.
#[derive(Clone, Copy, Debug, PartialEq)]
struct LineBox {
    top: f32,
    height: f32,
    baseline: f32,
}

/// The paragraph's own font, measured: what CSS calls the strut. Every line of
/// a paragraph with runs is at least this tall, and `vertical-align` lines
/// boxes up against it.
#[derive(Clone, Copy, Debug, Default)]
struct Strut {
    ascent: f32,
    descent: f32,
    x_height: f32,
}

/// Owns the reusable font/layout contexts. One per app is plenty.
pub struct TextEngine {
    font_cx: FontContext,
    layout_cx: LayoutContext<RunId>,
    /// What [`measure`](Self::measure) has answered, keyed by everything that
    /// decides the answer. Layout asks for each text box's size several times
    /// (taffy probes min-content, max-content and the final width) and again
    /// every frame, and each ask shaped the text from scratch: in a 300-row
    /// list that was most of the frame. Cleared when a font is registered, and
    /// started over past [`MEASURED_CAP`] entries.
    measured: HashMap<MeasureKey, (f32, f32)>,
    /// What [`draw`](Self::draw) has shaped, aligned, by the same key plus
    /// the alignment. Every text was shaped again each frame to be drawn,
    /// 900 of them in a 300-row list, which was most of the scene's time
    /// (docs/11-next.md, track (g)). Cleared as `measured` is, and started
    /// over past [`SHAPED_CAP`].
    shaped: HashMap<MeasureKey, Layout<RunId>>,
    /// Where the parts of a paragraph with runs landed, by the same key as
    /// `shaped`: asked once per sentence per frame, and the same for the same
    /// sentence. Cleared with it.
    placed: HashMap<MeasureKey, Arc<InlineGeometry>>,
}

/// How many measurements the engine keeps before starting over. A key and its
/// answer are small, so this is a bound on growth, not a memory budget.
const MEASURED_CAP: usize = 16384;

/// How many shaped layouts the engine keeps. A layout holds its glyphs, so
/// this is kept to about what one screen draws, several times over.
const SHAPED_CAP: usize = 2048;

/// Everything [`TextEngine::build`] reads, plus the alignment for a drawn
/// layout (0 for a measurement), as a hashable value. Floats by their bits:
/// two sizes are the same size when they are the same number.
///
/// Generic over its strings so a lookup borrows them: the caches hold
/// `Key<String>` and are asked with a `Key<&str>`, through [`AsKey`]. A
/// lookup used to copy the text into a new `String` first, 6 600 times a
/// frame in a 300-row list (docs/11-next.md, track (g)).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct Key<S, R> {
    text: S,
    font_size: u32,
    weight: u16,
    wrap: u8,
    family: Option<S>,
    letter_spacing: Option<u32>,
    word_spacing: Option<u32>,
    line_height: Option<u32>,
    italic: bool,
    nowrap: bool,
    max_width: Option<u32>,
    align: u8,
    /// A paragraph's runs, `None` for plain text. Compared by value: the
    /// same runs are the same paragraph.
    rich: Option<R>,
}

type MeasureKey = Key<String, RichText>;
type KeyRef<'a> = Key<&'a str, &'a RichText>;

impl<'a> KeyRef<'a> {
    fn new(text: &'a str, style: &TextStyle<'a>, max_width: Option<f32>, align: u8) -> Self {
        Self {
            text,
            font_size: style.font_size.to_bits(),
            weight: style.weight,
            wrap: style.wrap as u8,
            family: style.family,
            letter_spacing: style.letter_spacing.map(f32::to_bits),
            word_spacing: style.word_spacing.map(f32::to_bits),
            line_height: style.line_height.map(f32::to_bits),
            italic: style.italic,
            nowrap: style.nowrap,
            max_width: max_width.map(f32::to_bits),
            align,
            rich: style.rich,
        }
    }

    fn owned(self) -> MeasureKey {
        Key {
            text: self.text.to_string(),
            family: self.family.map(str::to_string),
            font_size: self.font_size,
            weight: self.weight,
            wrap: self.wrap,
            letter_spacing: self.letter_spacing,
            word_spacing: self.word_spacing,
            line_height: self.line_height,
            italic: self.italic,
            nowrap: self.nowrap,
            max_width: self.max_width,
            align: self.align,
            rich: self.rich.cloned(),
        }
    }
}

/// A key seen with borrowed strings, which is what lets a `HashMap` keyed by
/// `Key<String>` be asked with a `Key<&str>`. The two hash alike, since a
/// `String` hashes as the `str` it holds.
trait AsKey {
    fn key(&self) -> KeyRef<'_>;
}

impl AsKey for MeasureKey {
    fn key(&self) -> KeyRef<'_> {
        Key {
            text: &self.text,
            family: self.family.as_deref(),
            font_size: self.font_size,
            weight: self.weight,
            wrap: self.wrap,
            letter_spacing: self.letter_spacing,
            word_spacing: self.word_spacing,
            line_height: self.line_height,
            italic: self.italic,
            nowrap: self.nowrap,
            max_width: self.max_width,
            align: self.align,
            rich: self.rich.as_ref(),
        }
    }
}

impl AsKey for KeyRef<'_> {
    fn key(&self) -> KeyRef<'_> {
        *self
    }
}

impl std::hash::Hash for dyn AsKey + '_ {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.key().hash(state)
    }
}

impl PartialEq for dyn AsKey + '_ {
    fn eq(&self, other: &Self) -> bool {
        self.key() == other.key()
    }
}

impl Eq for dyn AsKey + '_ {}

impl<'a> std::borrow::Borrow<dyn AsKey + 'a> for MeasureKey {
    fn borrow(&self) -> &(dyn AsKey + 'a) {
        self
    }
}

impl Default for TextEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl TextEngine {
    pub fn new() -> Self {
        Self {
            font_cx: FontContext::new(),
            layout_cx: LayoutContext::new(),
            measured: HashMap::new(),
            shaped: HashMap::new(),
            placed: HashMap::new(),
        }
    }

    /// Register an in-memory font file and point the generic families at it.
    ///
    /// Needed on the web, where there is no system font source at all: fontique
    /// discovers nothing, every family query misses, and text measures to zero,
    /// a blank window rather than an error. Registering one font gives the
    /// cascade something to land on.
    ///
    /// The generic families are all aimed at it, `serif` and `monospace`
    /// included. That is deliberately wrong typographically and deliberately
    /// right in practice: an example asking for `monospace` should render in
    /// *something* rather than vanish. Registering a mono font afterwards
    /// overrides that mapping for `monospace` alone.
    ///
    /// Returns false if the data held no usable font.
    pub fn register_font(&mut self, data: Vec<u8>) -> bool {
        use parley::fontique::{Blob, GenericFamily};

        let registered = self.font_cx.collection.register_fonts(Blob::new(Arc::new(data)), None);
        let Some((family, _)) = registered.first() else {
            return false;
        };
        // Every size measured so far was measured in the fonts that were
        // there before this one, and so was every layout kept for drawing.
        self.measured.clear();
        self.shaped.clear();
        self.placed.clear();

        for generic in [
            GenericFamily::SansSerif,
            GenericFamily::Serif,
            GenericFamily::Monospace,
            GenericFamily::SystemUi,
            GenericFamily::UiSansSerif,
        ] {
            self.font_cx.collection.set_generic_families(generic, [*family].into_iter());
        }
        true
    }

    fn build(&mut self, text: &str, style: &TextStyle, max_width: Option<f32>) -> Layout<RunId> {
        let mut builder = self.layout_cx.ranged_builder(&mut self.font_cx, text, 1.0, true);
        builder.push_default(StyleProperty::FontSize(style.font_size));
        builder.push_default(StyleProperty::FontWeight(FontWeight::new(style.weight as f32)));
        builder.push_default(StyleProperty::OverflowWrap(style.wrap.to_parley()));
        // `font-family` is a raw CSS list; `FontFamily::Source` lets parley parse
        // it and run the usual name-matching + fallback. Absent → system default.
        if let Some(family) = style.family.filter(|f| !f.trim().is_empty()) {
            builder.push_default(StyleProperty::FontFamily(FontFamily::Source(Cow::Borrowed(family))));
        }
        if let Some(px) = style.letter_spacing {
            builder.push_default(StyleProperty::LetterSpacing(px));
        }
        if let Some(px) = style.word_spacing {
            builder.push_default(StyleProperty::WordSpacing(px));
        }
        if let Some(px) = style.line_height {
            builder.push_default(StyleProperty::LineHeight(LineHeight::Absolute(px)));
        }
        if style.italic {
            builder.push_default(StyleProperty::FontStyle(FontStyle::Italic));
        }
        if style.nowrap {
            builder.push_default(StyleProperty::TextWrapMode(TextWrapMode::NoWrap));
        }
        if let Some(rich) = style.rich {
            for (i, run) in rich.runs.iter().enumerate() {
                Self::push_run(&mut builder, i as RunId + 1, run.range.clone(), &run.style, style);
            }
            // The height is what parley stands on the baseline, which matters
            // to nothing here: lines are measured in `lines`, and parley's
            // own line metrics are never read for a paragraph with runs.
            for (i, item) in rich.items.iter().enumerate() {
                builder.push_inline_box(InlineBox {
                    id: i as u64,
                    kind: InlineBoxKind::InFlow,
                    index: item.index.min(text.len()),
                    width: item.width,
                    height: item.height,
                });
            }
        }
        let mut layout: Layout<RunId> = builder.build(text);
        layout.break_all_lines(max_width);
        layout
    }

    /// One run's style over its range, every property of it: a run carries
    /// its whole style, inherited already, so nothing is left to fall through
    /// from the paragraph except what the two share.
    fn push_run(
        builder: &mut parley::RangedBuilder<'_, RunId>,
        id: RunId,
        range: std::ops::Range<usize>,
        run: &RunStyle,
        base: &TextStyle,
    ) {
        builder.push(StyleProperty::Brush(id), range.clone());
        builder.push(StyleProperty::FontSize(run.font_size), range.clone());
        builder.push(StyleProperty::FontWeight(FontWeight::new(run.weight as f32)), range.clone());
        builder.push(
            StyleProperty::FontStyle(if run.italic { FontStyle::Italic } else { FontStyle::Normal }),
            range.clone(),
        );
        if let Some(family) = run.family.as_deref().filter(|f| !f.trim().is_empty()) {
            if base.family != Some(family) {
                builder.push(StyleProperty::FontFamily(FontFamily::Source(Cow::Owned(family.to_string()))), range.clone());
            }
        }
        builder.push(StyleProperty::LetterSpacing(run.letter_spacing.unwrap_or(0.0)), range.clone());
        builder.push(StyleProperty::WordSpacing(run.word_spacing.unwrap_or(0.0)), range);
    }

    /// Measure the text block with leading trimmed: `(width, height)` where
    /// height sums each line's `ascent + descent`.
    ///
    /// The width is rounded **up**. Paint re-wraps the text at the box width
    /// layout gave it, so a box even a fraction of a pixel narrower than the
    /// text would break the last word onto a line the box has no height for.
    pub fn measure(&mut self, text: &str, style: &TextStyle, max_width: Option<f32>) -> (f32, f32) {
        let key = KeyRef::new(text, style, max_width, 0);
        if let Some(&size) = self.measured.get(&key as &dyn AsKey) {
            return size;
        }
        let size = self.measure_uncached(text, style, max_width);
        if self.measured.len() >= MEASURED_CAP {
            self.measured.clear();
        }
        self.measured.insert(key.owned(), size);
        size
    }

    fn measure_uncached(&mut self, text: &str, style: &TextStyle, max_width: Option<f32>) -> (f32, f32) {
        let layout = self.build(text, style, max_width);
        // Each line is `line-height` tall when set, else its own leading-trimmed
        // height (ascent + descent) so text hugs its box by default.
        let height: f32 = Self::lines(&layout, text, style).iter().map(|l| l.height).sum();
        (layout.width().ceil(), height.ceil())
    }

    /// Where the caret sits for a byte index into `text`: `(x, y, height)`
    /// relative to the text's top-left. Used to draw the caret in an input.
    pub fn caret_geometry(
        &mut self,
        text: &str,
        style: &TextStyle,
        max_width: Option<f32>,
        index: usize,
    ) -> (f32, f32, f32) {
        let layout = self.build(text, style, max_width);
        let cursor = Cursor::from_byte_index(&layout, index.min(text.len()), Affinity::Downstream);
        let bounds = cursor.geometry(&layout, CARET_WIDTH);
        (bounds.x0 as f32, bounds.y0 as f32, (bounds.y1 - bounds.y0) as f32)
    }

    /// The height of each line, in order, under our leading-trim convention:
    /// `line-height` when set, else the line's own `ascent + descent`. `draw` and
    /// `measure` step by exactly this, so anything positioning itself against the
    /// drawn glyphs must too.
    ///
    /// **A last line of nothing but spaces, left by a soft wrap, is not a
    /// line.** parley hangs a space that overflows the width, as CSS does,
    /// then starts a new line anyway, which the next space lands on; so a text
    /// ending in a space (`Tap to add {{ x }}` with `x` empty) measured two
    /// lines tall at exactly its own width: a button twice its height with its
    /// label at the top. In CSS trailing spaces hang and never wrap. Taffy
    /// 0.15 asks at that width where 0.7 reused an earlier answer, which is
    /// what showed it. A last line after a newline (`abc\n` in a textarea) is
    /// real and stays.
    fn lines(layout: &Layout<RunId>, text: &str, style: &TextStyle) -> Vec<LineBox> {
        let lines: Vec<_> = layout.lines().collect();
        let hung = lines.len() > 1
            && text
                .get(lines[lines.len() - 1].text_range())
                .is_some_and(|rest| rest.chars().all(|c| c == ' ' || c == '\u{a0}' || c == '\t'))
            && !lines[lines.len() - 1].items().any(|i| matches!(i, PositionedLayoutItem::InlineBox(_)))
            && matches!(
                lines[lines.len() - 2].break_reason(),
                BreakReason::Regular | BreakReason::Emergency
            );
        let counted = lines.len() - usize::from(hung);
        let strut = Self::strut(layout, style);
        let mut top = 0.0;
        lines
            .into_iter()
            .take(counted)
            .map(|l| {
                let line = Self::line_box(&l, style, strut, top);
                top += line.height;
                line
            })
            .collect()
    }

    /// Where one line stands, its top given.
    ///
    /// The text on it reaches `ascent` up and `descent` down from the baseline,
    /// and `line-height` adds (or takes) the difference half above and half
    /// below, which centres the text in a line taller than it. For plain text
    /// that is the whole story, and it is the arithmetic this engine has always
    /// used. A paragraph with runs adds each box on the line, placed by its
    /// `vertical-align`, and the line grows to hold the furthest reach either
    /// side of the baseline: CSS's line box.
    fn line_box(line: &Line<'_, RunId>, style: &TextStyle, strut: Strut, top: f32) -> LineBox {
        let m = line.metrics();
        let Some(rich) = style.rich else {
            let height = style.line_height.unwrap_or(m.ascent + m.descent);
            let half_leading = (height - (m.ascent + m.descent)) / 2.0;
            return LineBox { top, height, baseline: top + half_leading + m.ascent };
        };
        let (mut ascent, mut descent) = (strut.ascent, strut.descent);
        // The furthest any box on the line reaches above and below.
        let (mut box_above, mut box_below) = (f32::MIN, f32::MIN);
        for item in line.items() {
            match item {
                PositionedLayoutItem::GlyphRun(g) => {
                    let rm = g.run().metrics();
                    ascent = ascent.max(rm.ascent);
                    descent = descent.max(rm.descent);
                }
                PositionedLayoutItem::InlineBox(b) => {
                    if let Some(it) = rich.items.get(b.id as usize) {
                        let (a, d) = it.align.extent(it.height, strut.ascent, strut.descent, strut.x_height);
                        box_above = box_above.max(a);
                        box_below = box_below.max(d);
                    }
                }
            }
        }
        let half_leading = style.line_height.map_or(0.0, |lh| (lh - (ascent + descent)) / 2.0);
        let above = (ascent + half_leading).max(box_above);
        let below = (descent + half_leading).max(box_below);
        LineBox { top, height: above + below, baseline: top + above }
    }

    /// The paragraph's own font, measured from its first glyph run in its own
    /// style (any run's when none is). Plain text never needs it.
    fn strut(layout: &Layout<RunId>, style: &TextStyle) -> Strut {
        if style.rich.is_none() {
            return Strut::default();
        }
        let mut first = None;
        for line in layout.lines() {
            for item in line.items() {
                if let PositionedLayoutItem::GlyphRun(g) = item {
                    let own = g.style().brush == 0;
                    if own || first.is_none() {
                        let run = g.run();
                        let rm = run.metrics();
                        first = Some(Strut {
                            ascent: rm.ascent,
                            descent: rm.descent,
                            x_height: rm.x_height.unwrap_or(run.font_size() * 0.5),
                        });
                    }
                    if own {
                        return first.unwrap_or_default();
                    }
                }
            }
        }
        first.unwrap_or_default()
    }

    /// The rectangles covering the byte range `start..end`, as `(x, y, w, h)`
    /// relative to the text's top-left, one per line the selection spans.
    ///
    /// parley reports each rect's line index alongside it, and we take only the
    /// **horizontal** extent from parley: its vertical coords come from its own
    /// line pitch, but we step lines with the leading trimmed (see `draw`). Taking
    /// parley's `y` would drift a line further out of place with every wrapped
    /// line. So the `y` is recomputed from our own stepping, by line index.
    pub fn selection_rects(
        &mut self,
        text: &str,
        style: &TextStyle,
        max_width: Option<f32>,
        start: usize,
        end: usize,
    ) -> Vec<(f32, f32, f32, f32)> {
        if start == end {
            return Vec::new();
        }
        let layout = self.build(text, style, max_width);
        let (from, to) = (start.min(end), start.max(end));
        let selection = Selection::new(
            Cursor::from_byte_index(&layout, from.min(text.len()), Affinity::Downstream),
            Cursor::from_byte_index(&layout, to.min(text.len()), Affinity::Downstream),
        );

        let lines = Self::lines(&layout, text, style);
        selection
            .geometry(&layout)
            .into_iter()
            .filter_map(|(rect, line)| {
                let line = lines.get(line)?;
                Some((rect.x0 as f32, line.top, (rect.x1 - rect.x0) as f32, line.height))
            })
            .collect()
    }

    /// Shape and align the text into the draw cache unless it is there, so
    /// `draw` and `inline_boxes` read one layout.
    fn ensure_shaped(&mut self, key: KeyRef<'_>, text: &str, style: &TextStyle, max_width: Option<f32>, align: Align) {
        if !self.shaped.contains_key(&key as &dyn AsKey) {
            let mut layout = self.build(text, style, max_width);
            layout.align(align.to_parley(), AlignmentOptions::default());
            if self.shaped.len() >= SHAPED_CAP {
                self.shaped.clear();
                self.placed.clear();
            }
            self.shaped.insert(key.owned(), layout);
        }
    }

    /// [`inline_boxes`](Self::inline_boxes) and [`run_rects`](Self::run_rects)
    /// together, from a cache: what the layout asks of every sentence on every
    /// frame.
    pub fn placed(&mut self, text: &str, style: &TextStyle, align: Align, max_width: Option<f32>) -> Arc<InlineGeometry> {
        let key = KeyRef::new(text, style, max_width, align as u8);
        if let Some(p) = self.placed.get(&key as &dyn AsKey) {
            return Arc::clone(p);
        }
        let placed = Arc::new(InlineGeometry {
            items: self.inline_boxes(text, style, align, max_width),
            runs: self.run_rects(text, style, align, max_width),
        });
        if self.placed.len() >= SHAPED_CAP {
            self.placed.clear();
        }
        self.placed.insert(key.owned(), Arc::clone(&placed));
        placed
    }

    /// Where each inline box of a paragraph sits, `(x, y)` from the text's
    /// top-left, in the order of [`RichText::items`]. A box that did not make
    /// it into a line (none should) is at the origin.
    pub fn inline_boxes(
        &mut self,
        text: &str,
        style: &TextStyle,
        align: Align,
        max_width: Option<f32>,
    ) -> Vec<(f32, f32)> {
        let Some(rich) = style.rich else { return Vec::new() };
        let key = KeyRef::new(text, style, max_width, align as u8);
        self.ensure_shaped(key, text, style, max_width, align);
        let layout = &self.shaped[&key as &dyn AsKey];
        let strut = Self::strut(layout, style);
        let mut out = vec![(0.0, 0.0); rich.items.len()];
        let mut top = 0.0;
        for line in layout.lines() {
            let lb = Self::line_box(&line, style, strut, top);
            for item in line.items() {
                if let PositionedLayoutItem::InlineBox(b) = item {
                    let i = b.id as usize;
                    if let Some(it) = rich.items.get(i) {
                        let (above, _) = it.align.extent(it.height, strut.ascent, strut.descent, strut.x_height);
                        out[i] = (b.x, lb.baseline - above);
                    }
                }
            }
            top += lb.height;
        }
        out
    }

    /// Where each run of a paragraph is drawn: the rectangles `(x, y, w, h)`
    /// it covers from the text's top-left, one per line it is on, in the order
    /// of [`RichText::runs`]. Aligned as drawn, so a tap on a centred link
    /// lands on the link.
    pub fn run_rects(
        &mut self,
        text: &str,
        style: &TextStyle,
        align: Align,
        max_width: Option<f32>,
    ) -> Vec<Vec<(f32, f32, f32, f32)>> {
        let Some(rich) = style.rich else { return Vec::new() };
        let key = KeyRef::new(text, style, max_width, align as u8);
        self.ensure_shaped(key, text, style, max_width, align);
        let layout = &self.shaped[&key as &dyn AsKey];
        let lines = Self::lines(layout, text, style);
        rich.runs
            .iter()
            .map(|run| {
                if run.range.is_empty() {
                    return Vec::new();
                }
                let selection = Selection::new(
                    Cursor::from_byte_index(layout, run.range.start.min(text.len()), Affinity::Downstream),
                    Cursor::from_byte_index(layout, run.range.end.min(text.len()), Affinity::Upstream),
                );
                selection
                    .geometry(layout)
                    .into_iter()
                    .filter_map(|(r, line)| {
                        let lb = lines.get(line)?;
                        Some((r.x0 as f32, lb.top, (r.x1 - r.x0) as f32, lb.height))
                    })
                    .collect()
            })
            .collect()
    }

    /// The byte range of the word under a point, for double-click-to-select.
    /// `(x, y)` is relative to the text's top-left.
    pub fn word_at_point(
        &mut self,
        text: &str,
        style: &TextStyle,
        max_width: Option<f32>,
        x: f32,
        y: f32,
    ) -> (usize, usize) {
        let layout = self.build(text, style, max_width);
        let range = Selection::word_from_point(&layout, x, y).text_range();
        (range.start, range.end)
    }

    /// The byte index nearest a point, for click-to-position. `(x, y)` is
    /// relative to the text's top-left.
    pub fn index_at_point(
        &mut self,
        text: &str,
        style: &TextStyle,
        max_width: Option<f32>,
        x: f32,
        y: f32,
    ) -> usize {
        let layout = self.build(text, style, max_width);
        Cursor::from_point(&layout, x, y).index()
    }

    /// Draw the text with its top-left at `(x, y)`, leading trimmed and aligned
    /// within `max_width` (when given).
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        scene: &mut Scene,
        x: f32,
        y: f32,
        text: &str,
        style: &TextStyle,
        color: Color,
        align: Align,
        max_width: Option<f32>,
        transform: Affine,
    ) {
        self.draw_inked(scene, x, y, text, style, color, false, align, max_width, transform);
    }

    /// [`draw`](Self::draw) with every glyph and decoration in `color`, runs
    /// included, and no run highlights: the second pass `::selection { color }`
    /// paints over the selected part.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_ink(
        &mut self,
        scene: &mut Scene,
        x: f32,
        y: f32,
        text: &str,
        style: &TextStyle,
        color: Color,
        align: Align,
        max_width: Option<f32>,
        transform: Affine,
    ) {
        self.draw_inked(scene, x, y, text, style, color, true, align, max_width, transform);
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_inked(
        &mut self,
        scene: &mut Scene,
        x: f32,
        y: f32,
        text: &str,
        style: &TextStyle,
        color: Color,
        ink: bool,
        align: Align,
        max_width: Option<f32>,
        transform: Affine,
    ) {
        let key = KeyRef::new(text, style, max_width, align as u8);
        self.ensure_shaped(key, text, style, max_width, align);
        let layout = &self.shaped[&key as &dyn AsKey];
        let strut = Self::strut(layout, style);

        // A run's own colour and decorations, or the paragraph's.
        let look = |id: RunId| -> (Color, bool, bool) {
            match style.rich.and_then(|r| r.runs.get((id as usize).wrapping_sub(1))) {
                Some(run) if id > 0 => {
                    let c = run.style.color;
                    let fill = if ink { color } else { Color::new([c.r, c.g, c.b, c.a]) };
                    (fill, run.style.underline, run.style.strikethrough)
                }
                _ => (color, style.underline, style.strikethrough),
            }
        };

        // Highlights first, under every glyph: `background` on a run covers its
        // part of each line it is on, the full height of that line.
        if let Some(rich) = style.rich.filter(|r| !ink && r.runs.iter().any(|run| run.style.background.is_some())) {
            let lines: Vec<LineBox> = {
                let mut top = 0.0;
                layout
                    .lines()
                    .map(|l| {
                        let lb = Self::line_box(&l, style, strut, top);
                        top += lb.height;
                        lb
                    })
                    .collect()
            };
            for run in &rich.runs {
                let Some(bg) = run.style.background else { continue };
                let selection = Selection::new(
                    Cursor::from_byte_index(layout, run.range.start.min(text.len()), Affinity::Downstream),
                    Cursor::from_byte_index(layout, run.range.end.min(text.len()), Affinity::Upstream),
                );
                for (rect, line) in selection.geometry(layout) {
                    let Some(lb) = lines.get(line) else { continue };
                    let r = Rect::new(
                        (x as f64) + rect.x0,
                        (y + lb.top) as f64,
                        (x as f64) + rect.x1,
                        (y + lb.top + lb.height) as f64,
                    );
                    scene.fill(Fill::NonZero, transform, Color::new([bg.r, bg.g, bg.b, bg.a]), None, &r);
                }
            }
        }

        let mut line_top = y;
        for line in layout.lines() {
            // With `line-height`, the line box is that tall and the text sits
            // centred in it (half the extra leading above the ascent).
            let lb = Self::line_box(&line, style, strut, line_top);
            let baseline = lb.baseline;
            for item in line.items() {
                let PositionedLayoutItem::GlyphRun(glyph_run) = item else {
                    continue;
                };
                let run_x0 = x + glyph_run.offset();
                let mut pen_x = run_x0;
                let run = glyph_run.run();
                let (color, underline, strikethrough) = look(glyph_run.style().brush);

                scene
                    .draw_glyphs(run.font())
                    .brush(color)
                    .transform(transform)
                    .font_size(run.font_size())
                    .normalized_coords(run.normalized_coords())
                    .draw(
                        Fill::NonZero,
                        glyph_run.glyphs().map(|g| {
                            let gx = pen_x + g.x;
                            let gy = baseline - g.y;
                            pen_x += g.advance;
                            Glyph {
                                id: g.id as u32,
                                x: gx,
                                y: gy,
                            }
                        }),
                    );

                // Decorations are drawn as filled rects across the run, parley
                // doesn't draw them, but its `RunMetrics` give us the placement
                // (offset is the top of the line, from the baseline, in px).
                let rm = run.metrics();
                if underline {
                    let top = baseline - rm.underline_offset;
                    let rect = Rect::new(run_x0 as f64, top as f64, pen_x as f64, (top + rm.underline_size) as f64);
                    scene.fill(Fill::NonZero, transform, color, None, &rect);
                }
                if strikethrough {
                    let top = baseline - rm.strikethrough_offset;
                    let rect = Rect::new(run_x0 as f64, top as f64, pen_x as f64, (top + rm.strikethrough_size) as f64);
                    scene.fill(Fill::NonZero, transform, color, None, &rect);
                }
            }
            line_top += lb.height;
        }
    }
}
