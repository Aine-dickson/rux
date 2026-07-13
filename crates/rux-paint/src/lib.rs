//! Rux painter — milestones M1–M4.
//!
//! Turns the `Paint` items from `rux-layout` into a `vello::Scene`: filled
//! rounded rectangles for boxes, glyph runs (via `rux-text`) for text. Stage 5
//! of `docs/04-architecture.md`.

use rux_layout::{Paint, Rgba, TextAlign};
use rux_text::{Align, TextEngine};
use vello::kurbo::{Affine, RoundedRect};
use vello::peniko::{Color, Fill};
use vello::Scene;

fn to_color(c: Rgba) -> Color {
    Color::rgba(c.r as f64, c.g as f64, c.b as f64, c.a as f64)
}

fn to_align(a: TextAlign) -> Align {
    match a {
        TextAlign::Start => Align::Start,
        TextAlign::Center => Align::Center,
        TextAlign::End => Align::End,
        TextAlign::Justify => Align::Justify,
    }
}

/// Build a fresh scene from paint items, in list order (parents first).
pub fn build_scene(items: &[Paint], text: &mut TextEngine) -> Scene {
    let mut scene = Scene::new();
    for item in items {
        match item {
            Paint::Rect(r) => {
                let shape = RoundedRect::new(
                    r.x as f64,
                    r.y as f64,
                    (r.x + r.width) as f64,
                    (r.y + r.height) as f64,
                    r.radius as f64,
                );
                scene.fill(Fill::NonZero, Affine::IDENTITY, to_color(r.color), None, &shape);
            }
            Paint::Text(t) => {
                text.draw(
                    &mut scene,
                    t.x,
                    t.y,
                    &t.content.text,
                    t.content.font_size,
                    t.content.weight,
                    to_color(t.content.color),
                    to_align(t.content.align),
                    Some(t.width),
                );
            }
        }
    }
    scene
}
