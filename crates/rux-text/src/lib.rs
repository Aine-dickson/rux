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
    TextWrapMode, IndentOptions,
};
use parley::style::{FontFamily, FontFeatures, FontStyle, FontVariations, FontWidth};
use rux_layout::{DecorationStyle, Direction, InlineGeometry, RichText, RunStyle, TextLook, TextTransform};
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
    /// `left` and `right` are sides of the box, whatever the direction:
    /// `start` and `end` are the ones that follow it.
    Left,
    Right,
}

impl Align {
    fn to_parley(self) -> Alignment {
        match self {
            Align::Start => Alignment::Start,
            Align::Center => Alignment::Center,
            Align::End => Alignment::End,
            Align::Justify => Alignment::Justify,
            Align::Left => Alignment::Left,
            Align::Right => Alignment::Right,
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
    /// The paragraph's base direction; `None` takes it from its first letter
    /// with a direction (`dir="auto"`).
    pub direction: Option<Direction>,
    /// The rarely set text properties: transform, indent, font features and
    /// width, and what decorations look like.
    pub look: Option<&'a TextLook>,
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
            direction: None,
            look: None,
        }
    }
}

/// The direction of the first letter that has one, `None` for text of digits,
/// spaces and punctuation alone. Right-to-left is the Hebrew, Arabic, Syriac,
/// Thaana, N'Ko and later right-to-left scripts' blocks, less the Arabic
/// digits and number signs, which have no direction of their own.
/// The text as it is shaped: `text-transform` applied, the paragraph's, or
/// in a paragraph with runs, each run's own over its range. Borrowed when
/// nothing changes it.
fn transformed<'t>(text: &'t str, style: &TextStyle) -> Cow<'t, str> {
    let own = style.look.map_or(TextTransform::None, |l| l.transform);
    let Some(rich) = style.rich.filter(|r| r.runs.iter().any(|run| run.style.look.is_some())) else {
        return own.apply(text);
    };
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    for run in &rich.runs {
        let (start, end) = (run.range.start.min(text.len()), run.range.end.min(text.len()));
        if start > at {
            out.push_str(&own.apply(&text[at..start]));
        }
        let t = run.style.look.as_deref().map_or(own, |l| l.transform);
        out.push_str(&t.apply(&text[start..end]));
        at = at.max(end);
    }
    out.push_str(&own.apply(&text[at..]));
    Cow::Owned(out)
}

fn first_strong(text: &str) -> Option<Direction> {
    // What an isolate holds does not count (UAX #9, P2): a sentence opening
    // with a set-apart Hebrew name is still the sentence it is.
    let mut isolated = 0usize;
    for c in text.chars() {
        let u = c as u32;
        match c {
            '\u{2066}' | '\u{2067}' | '\u{2068}' => {
                isolated += 1;
                continue;
            }
            '\u{2069}' => {
                isolated = isolated.saturating_sub(1);
                continue;
            }
            _ if isolated > 0 => continue,
            '\u{200E}' => return Some(Direction::Ltr),
            '\u{200F}' => return Some(Direction::Rtl),
            _ => {}
        }
        let weak_arabic = matches!(u, 0x0600..=0x0605 | 0x0660..=0x066C | 0x06F0..=0x06F9);
        let rtl = matches!(u, 0x0590..=0x08FF | 0xFB1D..=0xFDFF | 0xFE70..=0xFEFF | 0x10800..=0x10FFF | 0x1E800..=0x1EFFF);
        if rtl && !weak_arabic {
            return Some(Direction::Rtl);
        }
        if !rtl && c.is_alphabetic() {
            return Some(Direction::Ltr);
        }
    }
    None
}

/// The invisible mark put in front of a paragraph whose first letter would
/// give it the wrong base direction.
///
/// parley 0.11 takes a paragraph's base direction from its first letter that
/// has one, always: there is no way to tell it otherwise. So a paragraph that
/// is right-to-left but starts with a Latin word, or left-to-right but starts
/// with an Arabic one, starts with a mark of the direction it should have
/// (U+200F or U+200E), which is the first letter with a direction, has no
/// width and draws nothing. Every byte index in and out of the engine is
/// moved by its length, so nothing outside ever sees it. Text whose first
/// letter agrees, nearly all of it, gets no mark.
fn mark(text: &str, direction: Option<Direction>) -> Option<&'static str> {
    let want = direction?;
    (first_strong(text).unwrap_or(Direction::Ltr) != want).then_some(match want {
        Direction::Ltr => "\u{200E}",
        Direction::Rtl => "\u{200F}",
    })
}

/// Whether any letter of `text` is written right to left, which is when a
/// caret's steps through it stop being its order in memory.
pub fn has_rtl(text: &str) -> bool {
    text.chars().any(|c| first_strong(c.encode_utf8(&mut [0; 4])) == Some(Direction::Rtl))
}

/// How far the engine's byte indices are from the caller's: the mark's length.
fn shift(text: &str, style: &TextStyle) -> usize {
    mark(text, style.direction).map_or(0, str::len)
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
    /// Each font file drawn so far, by its blob's id, and the copy of it
    /// vello is handed instead, when it needs one. See [`outlines_only`].
    drawn_fonts: HashMap<(u64, u32), Option<parley::FontData>>,
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
struct Key<S, R, L> {
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
    direction: Option<Direction>,
    /// The rarely set text properties, compared by value like the runs.
    look: Option<L>,
}

type MeasureKey = Key<String, RichText, TextLook>;
type KeyRef<'a> = Key<&'a str, &'a RichText, &'a TextLook>;

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
            direction: style.direction,
            look: style.look,
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
            direction: self.direction,
            look: self.look.cloned(),
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
            direction: self.direction,
            look: self.look.as_ref(),
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
            drawn_fonts: HashMap::new(),
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
        // `text-transform` changes letters, never where they are, so every
        // index into the text still holds for what is shaped.
        let shown = transformed(text, style);
        let text = &*shown;
        let marked: Cow<str> = match mark(text, style.direction) {
            Some(m) => Cow::Owned(format!("{m}{text}")),
            None => Cow::Borrowed(text),
        };
        let off = marked.len() - text.len();
        let text = &*marked;
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
        if let Some(look) = style.look {
            Self::push_look(&mut builder, look, None);
        }
        if let Some(rich) = style.rich {
            for (i, run) in rich.runs.iter().enumerate() {
                let range = run.range.start + off..run.range.end + off;
                Self::push_run(&mut builder, i as RunId + 1, range, &run.style, style);
            }
            // The height is what parley stands on the baseline, which matters
            // to nothing here: lines are measured in `lines`, and parley's
            // own line metrics are never read for a paragraph with runs.
            for (i, item) in rich.items.iter().enumerate() {
                builder.push_inline_box(InlineBox {
                    id: i as u64,
                    kind: InlineBoxKind::InFlow,
                    index: (item.index + off).min(text.len()),
                    width: item.width,
                    height: item.height,
                });
            }
        }
        let mut layout: Layout<RunId> = builder.build(text);
        // `text-indent`: a percentage is of the width the text is set in.
        if let Some(look) = style.look {
            let amount = look.indent + look.indent_pct.map_or(0.0, |p| p / 100.0 * max_width.unwrap_or(0.0));
            if amount != 0.0 {
                let options = IndentOptions { each_line: look.indent_each_line, hanging: look.indent_hanging };
                layout.set_text_indent(amount, options);
            }
        }
        layout.break_all_lines(max_width);
        layout
    }

    /// What a look changes in shaping: font features, variations and width.
    /// Over the whole paragraph, or one run's range.
    fn push_look(builder: &mut parley::RangedBuilder<'_, RunId>, look: &TextLook, range: Option<std::ops::Range<usize>>) {
        let mut put = |p: StyleProperty<'static, RunId>| match &range {
            Some(r) => builder.push(p, r.clone()),
            None => builder.push_default(p),
        };
        if let Some(features) = look.features() {
            put(StyleProperty::FontFeatures(FontFeatures::Source(Cow::Owned(features))));
        }
        if let Some(v) = &look.variation_settings {
            put(StyleProperty::FontVariations(FontVariations::Source(Cow::Owned(v.clone()))));
        }
        if let Some(pct) = look.stretch {
            put(StyleProperty::FontWidth(FontWidth::from_percentage(pct)));
        }
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
        builder.push(StyleProperty::WordSpacing(run.word_spacing.unwrap_or(0.0)), range.clone());
        if let Some(look) = run.look.as_deref() {
            Self::push_look(builder, look, Some(range));
        }
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
        let off = shift(text, style);
        let cursor = Cursor::from_byte_index(&layout, index.min(text.len()) + off, Affinity::Downstream);
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
        let off = shift(text, style);
        let in_text = |r: std::ops::Range<usize>| r.start.saturating_sub(off)..r.end.saturating_sub(off);
        let hung = lines.len() > 1
            && text
                .get(in_text(lines[lines.len() - 1].text_range()))
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
        let off = shift(text, style);
        let (from, to) = (start.min(end), start.max(end));
        let selection = Selection::new(
            Cursor::from_byte_index(&layout, from.min(text.len()) + off, Affinity::Downstream),
            Cursor::from_byte_index(&layout, to.min(text.len()) + off, Affinity::Downstream),
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
        let off = shift(text, style);
        rich.runs
            .iter()
            .map(|run| {
                if run.range.is_empty() {
                    return Vec::new();
                }
                let selection = Selection::new(
                    Cursor::from_byte_index(layout, run.range.start.min(text.len()) + off, Affinity::Downstream),
                    Cursor::from_byte_index(layout, run.range.end.min(text.len()) + off, Affinity::Upstream),
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
        let off = shift(text, style);
        let range = Selection::word_from_point(&layout, x, y).text_range();
        (range.start.saturating_sub(off), range.end.saturating_sub(off))
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
        let off = shift(text, style);
        Cursor::from_point(&layout, x, y).index().saturating_sub(off).min(text.len())
    }

    /// The caret one step to the left or right of `index` as the text is
    /// drawn, which in right-to-left words is the other way through the text:
    /// Left arrow in Arabic moves on to the next letter. What an arrow key
    /// does in a browser's field.
    pub fn visual_step(
        &mut self,
        text: &str,
        style: &TextStyle,
        max_width: Option<f32>,
        index: usize,
        rightward: bool,
    ) -> usize {
        let layout = self.build(text, style, max_width);
        let off = shift(text, style);
        let here = Cursor::from_byte_index(&layout, index.min(text.len()) + off, Affinity::Downstream);
        let next = if rightward { here.next_visual(&layout) } else { here.previous_visual(&layout) };
        next.index().saturating_sub(off).min(text.len())
    }

    /// The caret one word to the left or right of `index` as the text is
    /// drawn: Ctrl+Left and Ctrl+Right (Option on macOS). In right-to-left
    /// words it goes the other way through the text, as [`Self::visual_step`]
    /// does.
    pub fn word_step(
        &mut self,
        text: &str,
        style: &TextStyle,
        max_width: Option<f32>,
        index: usize,
        rightward: bool,
    ) -> usize {
        let layout = self.build(text, style, max_width);
        let off = shift(text, style);
        let here = Cursor::from_byte_index(&layout, index.min(text.len()) + off, Affinity::Downstream);
        let next = if rightward { here.next_visual_word(&layout) } else { here.previous_visual_word(&layout) };
        next.index().saturating_sub(off).min(text.len())
    }

    /// Where the word before `index`, or the one after it, ends in the text's
    /// own order: what Ctrl+Backspace and Ctrl+Delete delete up to, which is
    /// the same whichever way the words are drawn.
    pub fn word_edge(&mut self, text: &str, style: &TextStyle, max_width: Option<f32>, index: usize, forward: bool) -> usize {
        let layout = self.build(text, style, max_width);
        let off = shift(text, style);
        let here = Cursor::from_byte_index(&layout, index.min(text.len()) + off, Affinity::Downstream);
        let to = if forward { here.next_logical_word(&layout) } else { here.previous_logical_word(&layout) };
        to.index().saturating_sub(off).min(text.len())
    }

    /// The start or end of the line `index` is on, as the text is wrapped:
    /// Home and End in a field of several lines.
    pub fn line_edge(&mut self, text: &str, style: &TextStyle, max_width: Option<f32>, index: usize, end: bool) -> usize {
        let layout = self.build(text, style, max_width);
        let off = shift(text, style);
        let here = Selection::from(Cursor::from_byte_index(&layout, index.min(text.len()) + off, Affinity::Downstream));
        let to = if end { here.line_end(&layout, false) } else { here.line_start(&layout, false) };
        let to = to.focus().index().saturating_sub(off).min(text.len());
        if !end || to == text.len() || text[to..].starts_with('\n') {
            return to;
        }
        // A line that wraps ends after the space it wraps at, which is where
        // the next line starts: a caret there is drawn on the next line, and
        // what is typed joins its first word. End stays on this line.
        let kept = text[..to].trim_end_matches([' ', '\t']).len();
        if kept > index.min(text.len()) { kept } else { to }
    }

    /// The line under a point, as it is wrapped, or with `paragraph` the whole
    /// paragraph it is part of, as byte ranges: what a triple click takes.
    pub fn line_at_point(
        &mut self,
        text: &str,
        style: &TextStyle,
        max_width: Option<f32>,
        x: f32,
        y: f32,
        paragraph: bool,
    ) -> (usize, usize) {
        let layout = self.build(text, style, max_width);
        let off = shift(text, style);
        let s = if paragraph {
            Selection::hard_line_from_point(&layout, x, y)
        } else {
            Selection::line_from_point(&layout, x, y)
        };
        let range = s.text_range();
        let clamp = |i: usize| i.saturating_sub(off).min(text.len());
        (clamp(range.start), clamp(range.end))
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

    /// For `text-overflow: ellipsis`, when the text is one line too wide for
    /// `max_width`: the longest start of it that fits with `…` after it, and
    /// the look to draw that with (no ellipsis, so it is not cut again).
    /// `None` when nothing is cut, which is always for text that wraps, has
    /// runs, or has lines of its own.
    fn ellipsized(&mut self, text: &str, style: &TextStyle, max_width: Option<f32>) -> Option<(String, TextLook)> {
        let look = style.look.filter(|l| l.ellipsis)?;
        let max = max_width?;
        if !style.nowrap || style.rich.is_some() || text.contains('\n') {
            return None;
        }
        let whole = TextStyle { look: None, ..*style };
        if self.measure(text, &whole, None).0 <= max.ceil() {
            return None;
        }
        // The most characters that fit, found by halving.
        let ends: Vec<usize> = text.char_indices().map(|(i, _)| i).chain(std::iter::once(text.len())).collect();
        let fits = |engine: &mut Self, n: usize| {
            let cut = format!("{}\u{2026}", text[..ends[n]].trim_end());
            engine.measure(&cut, &whole, None).0 <= max.ceil()
        };
        let (mut lo, mut hi) = (0, ends.len() - 1);
        while lo < hi {
            let mid = (lo + hi).div_ceil(2);
            if fits(self, mid) {
                lo = mid;
            } else {
                hi = mid - 1;
            }
        }
        let cut = format!("{}\u{2026}", text[..ends[lo]].trim_end());
        Some((cut, TextLook { ellipsis: false, ..look.clone() }))
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
        // `text-overflow: ellipsis`: a line too long for its box is drawn cut
        // short, ending in `…`. Only drawing changes; it is measured, hit and
        // edited as the whole text.
        if let Some((cut, look)) = self.ellipsized(text, style, max_width) {
            let style = TextStyle { look: Some(&look), ..*style };
            return self.draw_inked(scene, x, y, &cut, &style, color, ink, align, max_width, transform);
        }
        let key = KeyRef::new(text, style, max_width, align as u8);
        self.ensure_shaped(key, text, style, max_width, align);
        let layout = &self.shaped[&key as &dyn AsKey];
        let strut = Self::strut(layout, style);

        // A run's own colour and decorations, or the paragraph's.
        let look = |id: RunId| -> (Color, bool, bool, Option<&TextLook>) {
            match style.rich.and_then(|r| r.runs.get((id as usize).wrapping_sub(1))) {
                Some(run) if id > 0 => {
                    let c = run.style.color;
                    let fill = if ink { color } else { Color::new([c.r, c.g, c.b, c.a]) };
                    (fill, run.style.underline, run.style.strikethrough, run.style.look.as_deref())
                }
                _ => (color, style.underline, style.strikethrough, style.look),
            }
        };

        // Highlights first, under every glyph: `background` on a run covers its
        // part of each line it is on, the full height of that line.
        if let Some(rich) = style.rich.filter(|r| !ink && r.runs.iter().any(|run| run.style.background.is_some())) {
            let off = shift(text, style);
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
                    Cursor::from_byte_index(layout, run.range.start.min(text.len()) + off, Affinity::Downstream),
                    Cursor::from_byte_index(layout, run.range.end.min(text.len()) + off, Affinity::Upstream),
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
                let (color, underline, strikethrough, run_look) = look(glyph_run.style().brush);

                let font = run.font();
                let font = self
                    .drawn_fonts
                    .entry((font.data.id(), font.index))
                    .or_insert_with(|| outlines_only(font.data.data(), font.index).map(|bytes| {
                        parley::FontData::new(parley::fontique::Blob::new(Arc::new(bytes)), font.index)
                    }))
                    .as_ref()
                    .unwrap_or(font);
                scene
                    .draw_glyphs(font)
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
                // `text-decoration-color`, `-style` and `-thickness`, and
                // where `text-underline-offset` puts the underline: that far
                // below the baseline.
                let deco = run_look.cloned().unwrap_or_default();
                let ink_color = match (ink, deco.decoration_color) {
                    (false, Some(c)) => Color::new([c.r, c.g, c.b, c.a]),
                    _ => color,
                };
                let size = |font: f32| deco.decoration_thickness.unwrap_or(font).max(0.5);
                let span = (run_x0, pen_x);
                if underline {
                    let top = match deco.underline_offset {
                        Some(o) => baseline + o,
                        None => baseline - rm.underline_offset,
                    };
                    decoration(scene, transform, ink_color, deco.decoration_style, span, top, size(rm.underline_size), true);
                }
                if deco.overline {
                    let s = size(rm.underline_size);
                    decoration(scene, transform, ink_color, deco.decoration_style, span, baseline - rm.ascent, s, false);
                }
                if strikethrough {
                    let s = size(rm.strikethrough_size);
                    let top = baseline - rm.strikethrough_offset;
                    decoration(scene, transform, ink_color, deco.decoration_style, span, top, s, false);
                }
            }
            line_top += lb.height;
        }
    }
}

/// One text decoration line, `size` thick with its top at `top`, across
/// `span`, in a `text-decoration-style`. A wavy line swings `size` either
/// side of the middle of where a straight one would be, so it keeps clear of
/// the glyphs above an underline (`below`) as much as it can.
#[allow(clippy::too_many_arguments)]
fn decoration(
    scene: &mut Scene,
    transform: Affine,
    color: Color,
    style: DecorationStyle,
    (x0, x1): (f32, f32),
    top: f32,
    size: f32,
    below: bool,
) {
    use vello::kurbo::{BezPath, Cap, Stroke};
    let (x0, x1, top, size) = (x0 as f64, x1 as f64, top as f64, size as f64);
    if x1 <= x0 {
        return;
    }
    let mid = top + size / 2.0;
    let line = |y: f64| {
        let mut p = BezPath::new();
        p.move_to((x0, y));
        p.line_to((x1, y));
        p
    };
    match style {
        DecorationStyle::Solid => {
            scene.fill(Fill::NonZero, transform, color, None, &Rect::new(x0, top, x1, top + size));
        }
        DecorationStyle::Double => {
            // Two lines a line apart; an underline's second goes down, away
            // from the glyphs.
            let second = if below { top + size * 2.0 } else { top - size * 2.0 };
            for t in [top, second] {
                scene.fill(Fill::NonZero, transform, color, None, &Rect::new(x0, t, x1, t + size));
            }
        }
        DecorationStyle::Dashed => {
            let stroke = Stroke::new(size).with_dashes(0.0, [size * 3.0, size * 2.0]);
            scene.stroke(&stroke, transform, color, None, &line(mid));
        }
        DecorationStyle::Dotted => {
            // Round dots a dot apart, as `border-style: dotted` draws them.
            let stroke = Stroke::new(size).with_caps(Cap::Round).with_dashes(0.0, [size * 0.01, size * 2.0 - size * 0.01]);
            scene.stroke(&stroke, transform, color, None, &line(mid));
        }
        DecorationStyle::Wavy => {
            let amp = size.max(1.0);
            let half = amp * 2.0;
            let mid = if below { mid + amp } else { mid };
            let mut p = BezPath::new();
            p.move_to((x0, mid));
            let mut x = x0;
            let mut up = true;
            while x < x1 {
                let nx = x + half;
                let peak = if up { mid - amp } else { mid + amp };
                p.quad_to((x + half / 2.0, peak), (nx, mid));
                x = nx;
                up = !up;
            }
            // Cut to the span: the last half wave may run past it.
            scene.push_clip_layer(Fill::NonZero, transform, &Rect::new(x0, mid - amp * 2.0, x1, mid + amp * 2.0));
            scene.stroke(&Stroke::new(size), transform, color, None, &p);
            scene.pop_layer();
        }
    }
}

#[cfg(test)]
mod ellipsis_tests {
    use super::{TextEngine, TextStyle, Wrap};
    use rux_layout::TextLook;

    /// A line too wide for its box is drawn as the most of it that fits with
    /// `…` after it; one that fits, or text that wraps, is drawn whole.
    #[test]
    fn a_line_too_wide_ends_in_an_ellipsis() {
        let mut te = TextEngine::new();
        let look = TextLook { ellipsis: true, ..Default::default() };
        let style = TextStyle { look: Some(&look), nowrap: true, ..TextStyle::new(16.0, 400, Wrap::Normal) };
        let text = "a line much too long for a narrow box";
        let (cut, drawn) = te.ellipsized(text, &style, Some(120.0)).expect("cut");
        assert!(cut.ends_with('\u{2026}') && text.starts_with(cut.trim_end_matches('\u{2026}')), "{cut}");
        assert!(!drawn.ellipsis, "not cut again");
        let plain = TextStyle { look: None, ..style };
        assert!(te.measure(&cut, &plain, None).0 <= 120.0);
        assert!(te.ellipsized("short", &style, Some(120.0)).is_none());
        let wraps = TextStyle { nowrap: false, ..style };
        assert!(te.ellipsized(text, &wraps, Some(120.0)).is_none());
    }
}

/// A copy of a font with its black-and-white embedded bitmaps hidden, when it
/// has outlines as well; `None` when the font is fine as it is.
///
/// Calibri, Cambria and other fonts of the ClearType era carry `EBLC`/`EBDT`
/// strikes, small bitmaps meant for GDI at a few pixel sizes. vello 0.9 takes
/// any font with a bitmap strike down its emoji path, glyph by glyph, and a
/// glyph whose mask it cannot unpack is skipped, outline and all: Calibri
/// drew three glyphs out of a sentence (watchlist #45). A browser draws such
/// a font from its outlines, and so does Rux now. Renaming the two tags in
/// the face's table directory hides the strikes from vello and leaves every
/// other table where it was; the new names keep the directory sorted. Colour
/// bitmaps (`CBLC`, `sbix`) are emoji and are left alone.
fn outlines_only(data: &[u8], index: u32) -> Option<Vec<u8>> {
    let u16_at = |at: usize| Some(u16::from_be_bytes(data.get(at..at + 2)?.try_into().ok()?));
    let u32_at = |at: usize| Some(u32::from_be_bytes(data.get(at..at + 4)?.try_into().ok()?));
    let dir = if data.get(0..4)? == b"ttcf" {
        if index >= u32_at(8)? {
            return None;
        }
        u32_at(12 + 4 * index as usize)? as usize
    } else {
        0
    };
    let tables = u16_at(dir + 4)? as usize;
    let tag_at = |i: usize| data.get(dir + 12 + 16 * i..dir + 16 + 16 * i);
    let has = |tag: &[u8]| (0..tables).any(|i| tag_at(i) == Some(tag));
    let outlined = has(b"glyf") || has(b"CFF ") || has(b"CFF2");
    if !outlined || !has(b"EBLC") || has(b"CBLC") || has(b"sbix") {
        return None;
    }
    let mut copy = data.to_vec();
    for i in 0..tables {
        let at = dir + 12 + 16 * i;
        let renamed: Option<&[u8; 4]> = match tag_at(i)? {
            b"EBLC" => Some(b"EBLB"),
            b"EBDT" => Some(b"EBDS"),
            _ => None,
        };
        if let Some(tag) = renamed {
            copy[at..at + 4].copy_from_slice(tag);
        }
    }
    Some(copy)
}

#[cfg(test)]
mod outline_tests {
    use super::outlines_only;

    /// A font file's table directory with these tags and nothing in them.
    fn font(tags: &[&[u8; 4]]) -> Vec<u8> {
        let mut f = vec![0, 1, 0, 0];
        f.extend((tags.len() as u16).to_be_bytes());
        f.extend([0; 6]);
        for tag in tags {
            f.extend(*tag);
            f.extend([0; 12]);
        }
        f
    }

    fn tags(f: &[u8]) -> Vec<&[u8]> {
        (0..u16::from_be_bytes([f[4], f[5]]) as usize).map(|i| &f[12 + 16 * i..16 + 16 * i]).collect()
    }

    /// Calibri's shape: outlines and a black-and-white strike. The strike is
    /// hidden from vello, the rest stays put and in order (watchlist #45).
    #[test]
    fn a_bitmap_strike_beside_outlines_is_hidden() {
        let calibri = font(&[b"EBDT", b"EBLC", b"GDEF", b"glyf"]);
        let copy = outlines_only(&calibri, 0).expect("a copy");
        let t = tags(&copy);
        assert_eq!(t, [&b"EBDS"[..], b"EBLB", b"GDEF", b"glyf"]);
        assert!(t.windows(2).all(|w| w[0] < w[1]), "still sorted");
        assert_eq!(copy.len(), calibri.len());
    }

    /// No strike, colour bitmaps (emoji), or bitmaps with no outline to fall
    /// back on: left as they are.
    #[test]
    fn other_fonts_are_left_alone() {
        assert!(outlines_only(&font(&[b"GDEF", b"glyf"]), 0).is_none());
        assert!(outlines_only(&font(&[b"CBDT", b"CBLC", b"EBDT", b"EBLC", b"glyf"]), 0).is_none());
        assert!(outlines_only(&font(&[b"EBDT", b"EBLC"]), 0).is_none());
        assert!(outlines_only(b"junk", 0).is_none());
    }
}
