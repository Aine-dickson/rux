//! A `<text>` made of more than one run: `Read the <text class="b">terms</text>`.
//!
//! The text of the whole sentence stays in [`TextContent::text`](crate::TextContent),
//! one string, because that is what a line breaker, a selection and bidi all
//! need: one paragraph, not a row of boxes. What this adds is where the runs
//! are in it and how each is styled, and the boxes that sit in its lines
//! (`<image>` and `<path>` inside a `<text>`).
//!
//! Every run carries its whole style, already inherited from the text around
//! it, and runs never overlap: a run inside a run is flattened into the pieces
//! either side of it. The text engine then only ever applies one style to one
//! range, with no question of which of two wins.

use std::hash::{Hash, Hasher};
use std::ops::Range;

use crate::Rgba;

/// The runs and inline boxes of one paragraph. Text outside every run is
/// drawn in the paragraph's own style.
#[derive(Clone, Debug, Default)]
pub struct RichText {
    /// In order of their ranges, never overlapping.
    pub runs: Vec<TextRun>,
    /// In order of their positions in the text.
    pub items: Vec<InlineItem>,
}

/// One styled range of the paragraph's text, in bytes.
#[derive(Clone, Debug)]
pub struct TextRun {
    pub range: Range<usize>,
    pub style: RunStyle,
}

/// What a run can change. Everything that moves glyphs, plus the two that
/// only paint: `color` and the highlight behind the run.
#[derive(Clone, Debug)]
pub struct RunStyle {
    pub font_size: f32,
    pub weight: u16,
    pub italic: bool,
    pub family: Option<String>,
    pub letter_spacing: Option<f32>,
    pub word_spacing: Option<f32>,
    pub underline: bool,
    pub strikethrough: bool,
    pub color: Rgba,
    /// `background` on the run: drawn behind its glyphs, line by line.
    pub background: Option<Rgba>,
}

/// A box in a line of text, as wide and tall as the element it stands for,
/// at a byte position in the text. It moves with the words around it and
/// wraps with them, and is never broken.
#[derive(Clone, Copy, Debug)]
pub struct InlineItem {
    /// Where in the text it sits, a byte index on a character boundary.
    pub index: usize,
    pub width: f32,
    pub height: f32,
    pub align: VerticalAlign,
}

/// `vertical-align` for a box in a line: where it sits against the text.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum VerticalAlign {
    /// Its bottom edge on the baseline, CSS's default. An icon as tall as the
    /// text sits visibly high, which is why `middle` exists.
    #[default]
    Baseline,
    /// Its middle at the baseline plus half the font's x-height.
    Middle,
    /// Its top at the top of the text.
    TextTop,
    /// Its bottom at the bottom of the text.
    TextBottom,
}

impl VerticalAlign {
    pub fn parse(value: &str) -> Option<Self> {
        Some(match value.trim() {
            "baseline" => VerticalAlign::Baseline,
            "middle" => VerticalAlign::Middle,
            "text-top" => VerticalAlign::TextTop,
            "text-bottom" => VerticalAlign::TextBottom,
            _ => return None,
        })
    }

    /// How far a box `height` tall reaches above and below the baseline,
    /// against text whose ascent, descent and x-height are given.
    pub fn extent(self, height: f32, ascent: f32, descent: f32, x_height: f32) -> (f32, f32) {
        match self {
            VerticalAlign::Baseline => (height, 0.0),
            VerticalAlign::Middle => {
                let mid = x_height / 2.0;
                (height / 2.0 + mid, height / 2.0 - mid)
            }
            VerticalAlign::TextTop => (ascent, height - ascent),
            VerticalAlign::TextBottom => (height - descent, descent),
        }
    }
}

// Equality and hashing by value, floats by their bits, so a paragraph can key
// the text engine's caches: the same runs are the same paragraph.

fn bits(v: f32) -> u32 {
    v.to_bits()
}

fn rgba_bits(c: &Rgba) -> [u32; 4] {
    [c.r.to_bits(), c.g.to_bits(), c.b.to_bits(), c.a.to_bits()]
}

impl PartialEq for RunStyle {
    fn eq(&self, o: &Self) -> bool {
        bits(self.font_size) == bits(o.font_size)
            && self.weight == o.weight
            && self.italic == o.italic
            && self.family == o.family
            && self.letter_spacing.map(bits) == o.letter_spacing.map(bits)
            && self.word_spacing.map(bits) == o.word_spacing.map(bits)
            && self.underline == o.underline
            && self.strikethrough == o.strikethrough
            && rgba_bits(&self.color) == rgba_bits(&o.color)
            && self.background.as_ref().map(rgba_bits) == o.background.as_ref().map(rgba_bits)
    }
}

impl Eq for RunStyle {}

impl Hash for RunStyle {
    fn hash<H: Hasher>(&self, h: &mut H) {
        bits(self.font_size).hash(h);
        self.weight.hash(h);
        self.italic.hash(h);
        self.family.hash(h);
        self.letter_spacing.map(bits).hash(h);
        self.word_spacing.map(bits).hash(h);
        self.underline.hash(h);
        self.strikethrough.hash(h);
        rgba_bits(&self.color).hash(h);
        self.background.as_ref().map(rgba_bits).hash(h);
    }
}

impl PartialEq for TextRun {
    fn eq(&self, o: &Self) -> bool {
        self.range == o.range && self.style == o.style
    }
}

impl Eq for TextRun {}

impl Hash for TextRun {
    fn hash<H: Hasher>(&self, h: &mut H) {
        self.range.hash(h);
        self.style.hash(h);
    }
}

impl PartialEq for InlineItem {
    fn eq(&self, o: &Self) -> bool {
        self.index == o.index
            && bits(self.width) == bits(o.width)
            && bits(self.height) == bits(o.height)
            && self.align == o.align
    }
}

impl Eq for InlineItem {}

impl Hash for InlineItem {
    fn hash<H: Hasher>(&self, h: &mut H) {
        self.index.hash(h);
        bits(self.width).hash(h);
        bits(self.height).hash(h);
        self.align.hash(h);
    }
}

impl PartialEq for RichText {
    fn eq(&self, o: &Self) -> bool {
        self.runs == o.runs && self.items == o.items
    }
}

impl Eq for RichText {}

impl Hash for RichText {
    fn hash<H: Hasher>(&self, h: &mut H) {
        self.runs.hash(h);
        self.items.hash(h);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extents_put_each_alignment_where_css_does() {
        // A 20px box against text 16 up, 4 down, x-height 8.
        let (a, d, x) = (16.0, 4.0, 8.0);
        assert_eq!(VerticalAlign::Baseline.extent(20.0, a, d, x), (20.0, 0.0));
        assert_eq!(VerticalAlign::Middle.extent(20.0, a, d, x), (14.0, 6.0));
        assert_eq!(VerticalAlign::TextTop.extent(20.0, a, d, x), (16.0, 4.0));
        assert_eq!(VerticalAlign::TextBottom.extent(20.0, a, d, x), (16.0, 4.0));
        // Shorter than the text, text-top hangs from the top.
        assert_eq!(VerticalAlign::TextTop.extent(10.0, a, d, x), (16.0, -6.0));
    }
}
