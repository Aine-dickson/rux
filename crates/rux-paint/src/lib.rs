//! Rux painter — milestones M1–M4.
//!
//! Turns the `Paint` items from `rux-layout` into a `vello::Scene`: filled
//! rounded rectangles for boxes, glyph runs (via `rux-text`) for text. Stage 5
//! of `docs/04-architecture.md`.

use rux_layout::{Paint, Rgba, TextAlign};
use rux_text::{Align, TextEngine};
use vello::kurbo::{Affine, Rect, RoundedRect, Stroke};
use vello::peniko::{Color, Fill, Mix};
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
                if let Some(bg) = r.background {
                    let shape = RoundedRect::new(
                        r.x as f64,
                        r.y as f64,
                        (r.x + r.width) as f64,
                        (r.y + r.height) as f64,
                        r.radius as f64,
                    );
                    scene.fill(Fill::NonZero, Affine::IDENTITY, to_color(bg), None, &shape);
                }
                // Border: stroke inset by half its width so it sits inside the box.
                if let Some(bc) = r.border_color {
                    if r.border_width > 0.0 {
                        let half = (r.border_width / 2.0) as f64;
                        let inner = RoundedRect::new(
                            r.x as f64 + half,
                            r.y as f64 + half,
                            (r.x + r.width) as f64 - half,
                            (r.y + r.height) as f64 - half,
                            (r.radius as f64 - half).max(0.0),
                        );
                        scene.stroke(
                            &Stroke::new(r.border_width as f64),
                            Affine::IDENTITY,
                            to_color(bc),
                            None,
                            &inner,
                        );
                    }
                }
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
            Paint::PushClip {
                x,
                y,
                width,
                height,
                ..
            } => {
                let rect = Rect::new(
                    *x as f64,
                    *y as f64,
                    (*x + *width) as f64,
                    (*y + *height) as f64,
                );
                scene.push_layer(Mix::Clip, 1.0, Affine::IDENTITY, &rect);
            }
            Paint::PopClip => scene.pop_layer(),
        }
    }
    scene
}
