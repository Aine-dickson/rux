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

use parley::style::{FontWeight, StyleProperty};
use parley::{FontContext, Layout, LayoutContext, PositionedLayoutItem};
use vello::kurbo::Affine;
use vello::peniko::{Color, Fill};
use vello::skrifa::raw::types::F2Dot14;
use vello::{Glyph, Scene};

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
    fn to_parley(self) -> parley::Alignment {
        match self {
            Align::Start => parley::Alignment::Start,
            Align::Center => parley::Alignment::Middle,
            Align::End => parley::Alignment::End,
            Align::Justify => parley::Alignment::Justified,
        }
    }
}

/// Owns the reusable font/layout contexts. One per app is plenty.
pub struct TextEngine {
    font_cx: FontContext,
    layout_cx: LayoutContext<Color>,
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
        color: Color,
        max_width: Option<f32>,
    ) -> Layout<Color> {
        let mut builder = self.layout_cx.ranged_builder(&mut self.font_cx, text, 1.0);
        builder.push_default(StyleProperty::FontSize(font_size));
        builder.push_default(StyleProperty::FontWeight(FontWeight::new(weight as f32)));
        builder.push_default(StyleProperty::Brush(color));
        let mut layout: Layout<Color> = builder.build(text);
        layout.break_all_lines(max_width);
        layout
    }

    /// Measure the text block with leading trimmed: `(width, height)` where
    /// height sums each line's `ascent + descent`.
    pub fn measure(
        &mut self,
        text: &str,
        font_size: f32,
        weight: u16,
        max_width: Option<f32>,
    ) -> (f32, f32) {
        let layout = self.build(text, font_size, weight, Color::WHITE, max_width);
        let height: f32 = layout
            .lines()
            .map(|l| {
                let m = l.metrics();
                m.ascent + m.descent
            })
            .sum();
        (layout.width(), height)
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
        max_width: Option<f32>,
    ) {
        let mut layout = self.build(text, font_size, weight, color, max_width);
        layout.align(max_width, align.to_parley());

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
                let font = run.font();
                let run_size = run.font_size();
                let coords: Vec<F2Dot14> = run
                    .normalized_coords()
                    .iter()
                    .copied()
                    .map(F2Dot14::from_bits)
                    .collect();

                scene
                    .draw_glyphs(font)
                    .brush(color)
                    .transform(Affine::IDENTITY)
                    .font_size(run_size)
                    .normalized_coords(&coords)
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
