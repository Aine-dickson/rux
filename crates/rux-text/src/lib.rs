//! Rux text engine — milestones M4, plus text-metric/weight/alignment fixes.
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
//! Colour is applied by vello at draw time, not carried through parley, so the
//! layout brush is `()`.

use parley::{
    Affinity, Alignment, AlignmentOptions, Cursor, FontContext, FontWeight, Layout, LayoutContext,
    OverflowWrap, PositionedLayoutItem, StyleProperty,
};
use vello::kurbo::Affine;
use vello::peniko::{Color, Fill};
use vello::{Glyph, Scene};

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

/// Owns the reusable font/layout contexts. One per app is plenty.
pub struct TextEngine {
    font_cx: FontContext,
    layout_cx: LayoutContext<()>,
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
        }
    }

    fn build(
        &mut self,
        text: &str,
        font_size: f32,
        weight: u16,
        wrap: Wrap,
        max_width: Option<f32>,
    ) -> Layout<()> {
        let mut builder = self.layout_cx.ranged_builder(&mut self.font_cx, text, 1.0, true);
        builder.push_default(StyleProperty::FontSize(font_size));
        builder.push_default(StyleProperty::FontWeight(FontWeight::new(weight as f32)));
        builder.push_default(StyleProperty::OverflowWrap(wrap.to_parley()));
        let mut layout: Layout<()> = builder.build(text);
        layout.break_all_lines(max_width);
        layout
    }

    /// Measure the text block with leading trimmed: `(width, height)` where
    /// height sums each line's `ascent + descent`.
    ///
    /// The width is rounded **up**. Paint re-wraps the text at the box width
    /// layout gave it, so a box even a fraction of a pixel narrower than the
    /// text would break the last word onto a line the box has no height for.
    pub fn measure(
        &mut self,
        text: &str,
        font_size: f32,
        weight: u16,
        wrap: Wrap,
        max_width: Option<f32>,
    ) -> (f32, f32) {
        let layout = self.build(text, font_size, weight, wrap, max_width);
        let height: f32 = layout
            .lines()
            .map(|l| {
                let m = l.metrics();
                m.ascent + m.descent
            })
            .sum();
        (layout.width().ceil(), height.ceil())
    }

    /// Where the caret sits for a byte index into `text`: `(x, y, height)`
    /// relative to the text's top-left. Used to draw the caret in an input.
    #[allow(clippy::too_many_arguments)]
    pub fn caret_geometry(
        &mut self,
        text: &str,
        font_size: f32,
        weight: u16,
        wrap: Wrap,
        max_width: Option<f32>,
        index: usize,
    ) -> (f32, f32, f32) {
        let layout = self.build(text, font_size, weight, wrap, max_width);
        let cursor = Cursor::from_byte_index(&layout, index.min(text.len()), Affinity::Downstream);
        let bounds = cursor.geometry(&layout, CARET_WIDTH);
        (bounds.x0 as f32, bounds.y0 as f32, (bounds.y1 - bounds.y0) as f32)
    }

    /// The byte index nearest a point, for click-to-position. `(x, y)` is
    /// relative to the text's top-left.
    #[allow(clippy::too_many_arguments)]
    pub fn index_at_point(
        &mut self,
        text: &str,
        font_size: f32,
        weight: u16,
        wrap: Wrap,
        max_width: Option<f32>,
        x: f32,
        y: f32,
    ) -> usize {
        let layout = self.build(text, font_size, weight, wrap, max_width);
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
        font_size: f32,
        weight: u16,
        color: Color,
        align: Align,
        wrap: Wrap,
        max_width: Option<f32>,
    ) {
        let mut layout = self.build(text, font_size, weight, wrap, max_width);
        layout.align(align.to_parley(), AlignmentOptions::default());

        let mut line_top = y;
        for line in layout.lines() {
            let m = line.metrics();
            let baseline = line_top + m.ascent;
            for item in line.items() {
                let PositionedLayoutItem::GlyphRun(glyph_run) = item else {
                    continue;
                };
                let mut pen_x = x + glyph_run.offset();
                let run = glyph_run.run();

                scene
                    .draw_glyphs(run.font())
                    .brush(color)
                    .transform(Affine::IDENTITY)
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
            }
            line_top += m.ascent + m.descent;
        }
    }
}
