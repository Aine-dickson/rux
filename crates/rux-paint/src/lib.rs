//! Rux painter — milestones M1–M4.
//!
//! Turns the `Paint` items from `rux-layout` into a `vello::Scene`: filled
//! rounded rectangles for boxes, glyph runs (via `rux-text`) for text. Stage 5
//! of `docs/04-architecture.md`.

use std::collections::HashMap;

use rux_layout::{Paint, Rgba, TextAlign, TextWrap};
use rux_text::{Align, TextEngine, Wrap};
use vello::kurbo::{Affine, BezPath, Cap, Join, Rect, RoundedRect, Stroke, Vec2};
use vello::peniko::{Blob, Color, Fill, ImageAlphaType, ImageBrush, ImageData, ImageFormat, Mix};
use vello::Scene;

/// Decoded images, keyed by the `src` path. Decoding is the expensive part, and
/// we repaint on every event, so an image is read from disk at most once. A src
/// that fails to decode is remembered as a miss and not retried.
#[derive(Default)]
pub struct ImageCache {
    images: HashMap<String, Option<ImageBrush>>,
}

impl ImageCache {
    pub fn new() -> Self {
        Self::default()
    }

    fn get(&mut self, src: &str) -> Option<&ImageBrush> {
        self.images
            .entry(src.to_string())
            .or_insert_with(|| decode(src))
            .as_ref()
    }
}

fn decode(src: &str) -> Option<ImageBrush> {
    let decoded = image::open(src)
        .map_err(|e| eprintln!("rux: cannot load image {src}: {e}"))
        .ok()?
        .into_rgba8();
    let (width, height) = decoded.dimensions();
    Some(ImageBrush::new(ImageData {
        data: Blob::new(std::sync::Arc::new(decoded.into_raw())),
        format: ImageFormat::Rgba8,
        alpha_type: ImageAlphaType::Alpha,
        width,
        height,
    }))
}

fn to_color(c: Rgba) -> Color {
    Color::new([c.r, c.g, c.b, c.a])
}

pub fn to_wrap(w: TextWrap) -> Wrap {
    match w {
        TextWrap::Normal => Wrap::Normal,
        TextWrap::BreakWord => Wrap::BreakWord,
        TextWrap::Anywhere => Wrap::Anywhere,
    }
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
pub fn build_scene(items: &[Paint], text: &mut TextEngine, images: &mut ImageCache) -> Scene {
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
                    to_wrap(t.content.wrap),
                    Some(t.width),
                );
                // The focused input's caret, drawn on top of its own text.
                if let Some(index) = t.content.caret {
                    let (cx, cy, ch) = text.caret_geometry(
                        &t.content.text,
                        t.content.font_size,
                        t.content.weight,
                        to_wrap(t.content.wrap),
                        Some(t.width),
                        index,
                    );
                    let caret = Rect::new(
                        (t.x + cx) as f64,
                        (t.y + cy) as f64,
                        (t.x + cx + rux_text::CARET_WIDTH) as f64,
                        (t.y + cy + ch) as f64,
                    );
                    scene.fill(
                        Fill::NonZero,
                        Affine::IDENTITY,
                        to_color(t.content.color),
                        None,
                        &caret,
                    );
                }
            }
            // A checkmark: two strokes, round caps and joins, proportioned to the
            // box. A stroked path rather than a ✓ glyph — the glyph is whatever
            // the system font ships and reads as text, not as a control mark.
            Paint::Tick(t) => {
                let (x, y, w, h) = (t.x as f64, t.y as f64, t.width as f64, t.height as f64);
                let mut path = BezPath::new();
                path.move_to((x + 0.14 * w, y + 0.53 * h));
                path.line_to((x + 0.40 * w, y + 0.78 * h));
                path.line_to((x + 0.86 * w, y + 0.24 * h));
                let stroke = Stroke::new((h * 0.16).max(1.5))
                    .with_caps(Cap::Round)
                    .with_join(Join::Round);
                scene.stroke(
                    &stroke,
                    Affine::IDENTITY,
                    to_color(t.color),
                    None,
                    &path,
                );
            }
            // Scale the decoded pixels to fill the box layout gave the element.
            Paint::Image(img) => {
                let Some(decoded) = images.get(&img.content.src) else {
                    continue;
                };
                let (iw, ih) = (decoded.image.width as f64, decoded.image.height as f64);
                if iw <= 0.0 || ih <= 0.0 {
                    continue;
                }
                let transform = Affine::scale_non_uniform(img.width as f64 / iw, img.height as f64 / ih)
                    .then_translate(Vec2::new(img.x as f64, img.y as f64));
                scene.draw_image(decoded, transform);
            }
            Paint::PushClip {
                x,
                y,
                width,
                height,
                radius,
            } => {
                let shape = RoundedRect::new(
                    *x as f64,
                    *y as f64,
                    (*x + *width) as f64,
                    (*y + *height) as f64,
                    *radius as f64,
                );
                scene.push_clip_layer(Fill::NonZero, Affine::IDENTITY, &shape);
            }
            Paint::PopClip => scene.pop_layer(),
            // Fade the subtree. The layer covers the viewport so it only blends,
            // never clips — an overflowing child still shows through.
            Paint::PushOpacity {
                alpha,
                width,
                height,
            } => {
                let shape = Rect::new(0.0, 0.0, *width as f64, *height as f64);
                scene.push_layer(Fill::NonZero, Mix::Normal, *alpha, Affine::IDENTITY, &shape);
            }
            Paint::PopOpacity => scene.pop_layer(),
        }
    }
    scene
}
