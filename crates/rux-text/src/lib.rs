//! Rux text engine — milestone M4.
//!
//! Wraps `parley` (shaping + line layout over the system fonts) and draws the
//! resulting glyph runs into a `vello::Scene`. It owns the font and layout
//! contexts so they're built once and reused: `measure` feeds `taffy`'s leaf
//! sizing (Stage 4), `draw` paints the glyphs (Stage 5).

use parley::{FontContext, Layout, LayoutContext, PositionedLayoutItem};
use parley::style::StyleProperty;
use vello::kurbo::Affine;
use vello::peniko::{Color, Fill};
use vello::skrifa::raw::types::F2Dot14;
use vello::{Glyph, Scene};

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

    /// Build a broken-into-lines layout for `text` at `font_size`, wrapped to
    /// `max_width` if given.
    fn build(&mut self, text: &str, font_size: f32, color: Color, max_width: Option<f32>) -> Layout<Color> {
        let mut builder = self.layout_cx.ranged_builder(&mut self.font_cx, text, 1.0);
        builder.push_default(StyleProperty::FontSize(font_size));
        builder.push_default(StyleProperty::Brush(color));
        let mut layout: Layout<Color> = builder.build(text);
        layout.break_all_lines(max_width);
        layout
    }

    /// Measure the text block: returns `(width, height)` in pixels.
    pub fn measure(&mut self, text: &str, font_size: f32, max_width: Option<f32>) -> (f32, f32) {
        let layout = self.build(text, font_size, Color::WHITE, max_width);
        (layout.width(), layout.height())
    }

    /// Draw the text with its top-left at `(x, y)`.
    pub fn draw(
        &mut self,
        scene: &mut Scene,
        x: f32,
        y: f32,
        text: &str,
        font_size: f32,
        color: Color,
        max_width: Option<f32>,
    ) {
        let layout = self.build(text, font_size, color, max_width);
        for line in layout.lines() {
            for item in line.items() {
                let PositionedLayoutItem::GlyphRun(glyph_run) = item else {
                    continue;
                };
                let mut pen_x = x + glyph_run.offset();
                let pen_y = y + glyph_run.baseline();
                let run = glyph_run.run();
                let font = run.font();
                let run_size = run.font_size();
                // parley yields raw i16 coords; vello wants F2Dot14 (same bits).
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
                            let gy = pen_y - g.y;
                            pen_x += g.advance;
                            Glyph {
                                id: g.id as u32,
                                x: gx,
                                y: gy,
                            }
                        }),
                    );
            }
        }
    }
}
