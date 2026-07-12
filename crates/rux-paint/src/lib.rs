//! Rux painter — milestone M1.
//!
//! Turns the absolute `PaintRect`s from `rux-layout` into a `vello::Scene`:
//! one filled, rounded rectangle per rect. This is the smallest slice of
//! Stage 5 in `docs/04-architecture.md` — no text, gradients, or shadows yet.

use rux_layout::{PaintRect, Rgba};
use vello::kurbo::{Affine, RoundedRect};
use vello::peniko::{Color, Fill};
use vello::Scene;

fn to_color(c: Rgba) -> Color {
    Color::rgba(c.r as f64, c.g as f64, c.b as f64, c.a as f64)
}

/// Build a fresh scene containing every rect, painted in list order (parents
/// first, so children draw on top).
pub fn build_scene(rects: &[PaintRect]) -> Scene {
    let mut scene = Scene::new();
    for r in rects {
        let shape = RoundedRect::new(
            r.x as f64,
            r.y as f64,
            (r.x + r.width) as f64,
            (r.y + r.height) as f64,
            r.radius as f64,
        );
        scene.fill(
            Fill::NonZero,
            Affine::IDENTITY,
            to_color(r.color),
            None,
            &shape,
        );
    }
    scene
}
