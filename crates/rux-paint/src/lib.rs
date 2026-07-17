//! Rux painter — milestones M1–M4.
//!
//! Turns the `Paint` items from `rux-layout` into a `vello::Scene`: filled
//! rounded rectangles for boxes, glyph runs (via `rux-text`) for text. Stage 5
//! of `docs/04-architecture.md`.

use std::collections::HashMap;

use rux_layout::{Background, Corners, Gradient, GradientKind, Paint, Rgba, TextAlign, TextContent, TextWrap};
use rux_text::{Align, TextEngine, TextStyle, Wrap};
use vello::kurbo::{Affine, BezPath, Cap, Join, Point, Rect, RoundedRect, RoundedRectRadii, Stroke, Vec2};
use vello::peniko::{
    Blob, Color, ColorStop, Fill, Gradient as PenikoGradient, ImageAlphaType, ImageBrush, ImageData,
    ImageFormat, Mix,
};
use vello::Scene;

/// The selection highlight, `#89b4fa` at 45% — the focus-ring blue. Not
/// author-controlled: Rux has no `::selection` yet.
const SELECTION: Color = Color::from_rgba8(0x89, 0xb4, 0xfa, 0x73);

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

/// Build a peniko gradient brush for a box at `(x, y, w, h)`. A linear gradient's
/// endpoints follow the CSS gradient-line formula for its angle; a radial one is
/// a centred circle to the nearest edge.
fn to_gradient(g: &Gradient, x: f32, y: f32, w: f32, h: f32) -> PenikoGradient {
    let stops: Vec<ColorStop> = g
        .stops
        .iter()
        .map(|(rgba, off)| ColorStop { offset: *off, color: to_color(*rgba).into() })
        .collect();
    let (cx, cy) = ((x + w / 2.0) as f64, (y + h / 2.0) as f64);
    let grad = match g.kind {
        GradientKind::Linear { angle } => {
            // CSS: 0rad points to the top, clockwise. In screen space (y down)
            // that progression direction is (sin, -cos).
            let (sin, cos) = (angle.sin() as f64, angle.cos() as f64);
            let (dx, dy) = (sin, -cos);
            let len = ((w as f64 * sin).abs() + (h as f64 * cos).abs()).max(1.0);
            let p0 = Point::new(cx - dx * len / 2.0, cy - dy * len / 2.0);
            let p1 = Point::new(cx + dx * len / 2.0, cy + dy * len / 2.0);
            PenikoGradient::new_linear(p0, p1)
        }
        GradientKind::Radial => {
            let radius = (w.min(h) / 2.0).max(1.0);
            PenikoGradient::new_radial((cx, cy), radius)
        }
    };
    grad.with_stops(stops.as_slice())
}

/// A rounded rect with independent corner radii, optionally inset on all sides
/// by `inset` (used to sit a stroked border inside the box; corner radii shrink
/// by the same amount, floored at 0). `radius` is CSS order: TL, TR, BR, BL.
fn rounded_rect(x: f32, y: f32, w: f32, h: f32, radius: Corners, inset: f64) -> RoundedRect {
    let rect = Rect::new(
        x as f64 + inset,
        y as f64 + inset,
        (x + w) as f64 - inset,
        (y + h) as f64 - inset,
    );
    let r = |i: usize| (radius[i] as f64 - inset).max(0.0);
    RoundedRect::from_rect(
        rect,
        RoundedRectRadii::new(r(0), r(1), r(2), r(3)),
    )
}

pub fn to_wrap(w: TextWrap) -> Wrap {
    match w {
        TextWrap::Normal => Wrap::Normal,
        TextWrap::BreakWord => Wrap::BreakWord,
        TextWrap::Anywhere => Wrap::Anywhere,
    }
}

/// Gather a text node's shaping inputs into a [`TextStyle`] for the text engine.
/// Shared by the painter and the shell (measure + hit-testing) so they always
/// shape text the same way.
pub fn text_style(tc: &TextContent) -> TextStyle<'_> {
    TextStyle {
        font_size: tc.font_size,
        weight: tc.weight,
        wrap: to_wrap(tc.wrap),
        family: tc.font_family.as_deref(),
        letter_spacing: tc.letter_spacing,
        word_spacing: tc.word_spacing,
        line_height: tc.line_height,
        italic: tc.italic,
        underline: tc.underline,
        strikethrough: tc.strikethrough,
        nowrap: tc.nowrap,
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
/// `caret_visible` gates the focused input's caret: the shell toggles it on a
/// timer so the caret blinks, without rebuilding or re-laying-out the tree.
pub fn build_scene(
    items: &[Paint],
    text: &mut TextEngine,
    images: &mut ImageCache,
    caret_visible: bool,
) -> Scene {
    let mut scene = Scene::new();
    // Active `transform` stack. Every draw uses the accumulated transform so a
    // transformed element carries its subtree with it. (Hit regions are computed
    // untransformed, so they don't follow — a documented limitation.)
    let mut tstack: Vec<Affine> = Vec::new();
    for item in items {
        let cur = tstack.last().copied().unwrap_or(Affine::IDENTITY);
        match item {
            Paint::Rect(r) => {
                if let Some(bg) = &r.background {
                    let shape = rounded_rect(r.x, r.y, r.width, r.height, r.radius, 0.0);
                    match bg {
                        Background::Color(c) => {
                            scene.fill(Fill::NonZero, cur, to_color(*c), None, &shape);
                        }
                        Background::Gradient(g) => {
                            let brush = to_gradient(g, r.x, r.y, r.width, r.height);
                            scene.fill(Fill::NonZero, cur, &brush, None, &shape);
                        }
                        Background::Image(src) => {
                            if let Some(brush) = images.get(src) {
                                let (iw, ih) = (brush.image.width as f64, brush.image.height as f64);
                                if iw > 0.0 && ih > 0.0 {
                                    // `cover`: scale to fill the box, centre, and
                                    // clip to the box's rounded corners.
                                    let scale = (r.width as f64 / iw).max(r.height as f64 / ih);
                                    let (sw, sh) = (iw * scale, ih * scale);
                                    let ox = r.x as f64 + (r.width as f64 - sw) / 2.0;
                                    let oy = r.y as f64 + (r.height as f64 - sh) / 2.0;
                                    let tf = Affine::scale(scale).then_translate(Vec2::new(ox, oy));
                                    scene.push_clip_layer(Fill::NonZero, cur, &shape);
                                    scene.draw_image(brush, cur * tf);
                                    scene.pop_layer();
                                }
                            }
                        }
                    }
                }
                // Border: stroke inset by half its width so it sits inside the box.
                if let Some(bc) = r.border_color {
                    if r.border_width > 0.0 {
                        let half = (r.border_width / 2.0) as f64;
                        let inner = rounded_rect(r.x, r.y, r.width, r.height, r.radius, half);
                        scene.stroke(
                            &Stroke::new(r.border_width as f64),
                            cur,
                            to_color(bc),
                            None,
                            &inner,
                        );
                    }
                }
            }
            Paint::Shadow {
                x,
                y,
                width,
                height,
                radius,
                blur,
                color,
            } => {
                let rect = Rect::new(
                    *x as f64,
                    *y as f64,
                    (*x + *width) as f64,
                    (*y + *height) as f64,
                );
                // CSS blur radius is ~2σ of the gaussian; vello wants σ.
                let std_dev = (*blur as f64 / 2.0).max(0.0);
                scene.draw_blurred_rounded_rect(cur, rect, to_color(*color), *radius as f64, std_dev);
            }
            Paint::Text(t) => {
                // The selection highlight goes behind the glyphs. There is no
                // `::selection` in Rux yet, so the colour is ours, not the
                // author's — the focus-ring blue, faded enough to read through.
                if let Some((start, end)) = t.content.selection {
                    let rects = text.selection_rects(
                        &t.content.text,
                        &text_style(&t.content),
                        Some(t.width),
                        start,
                        end,
                    );
                    for (sx, sy, sw, sh) in rects {
                        let rect = Rect::new(
                            (t.x + sx) as f64,
                            (t.y + sy) as f64,
                            (t.x + sx + sw) as f64,
                            (t.y + sy + sh) as f64,
                        );
                        scene.fill(Fill::NonZero, cur, SELECTION, None, &rect);
                    }
                }
                text.draw(
                    &mut scene,
                    t.x,
                    t.y,
                    &t.content.text,
                    &text_style(&t.content),
                    to_color(t.content.color),
                    to_align(t.content.align),
                    Some(t.width),
                    cur,
                );
                // The focused input's caret, drawn on top of its own text —
                // only in the visible half of the blink cycle.
                if let (true, Some(index)) = (caret_visible, t.content.caret) {
                    let (cx, cy, ch) = text.caret_geometry(
                        &t.content.text,
                        &text_style(&t.content),
                        Some(t.width),
                        index,
                    );
                    let caret = Rect::new(
                        (t.x + cx) as f64,
                        (t.y + cy) as f64,
                        (t.x + cx + rux_text::CARET_WIDTH) as f64,
                        (t.y + cy + ch) as f64,
                    );
                    scene.fill(Fill::NonZero, cur, to_color(t.content.color), None, &caret);
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
                scene.stroke(&stroke, cur, to_color(t.color), None, &path);
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
                scene.draw_image(decoded, cur * transform);
            }
            Paint::PushClip {
                x,
                y,
                width,
                height,
                radius,
            } => {
                let shape = rounded_rect(*x, *y, *width, *height, *radius, 0.0);
                scene.push_clip_layer(Fill::NonZero, cur, &shape);
            }
            Paint::PopClip => scene.pop_layer(),
            Paint::PushTransform(m) => {
                let mat = Affine::new([
                    m[0] as f64, m[1] as f64, m[2] as f64, m[3] as f64, m[4] as f64, m[5] as f64,
                ]);
                tstack.push(cur * mat);
            }
            Paint::PopTransform => {
                tstack.pop();
            }
            // Fade the subtree. The layer covers the viewport so it only blends,
            // never clips — an overflowing child still shows through.
            Paint::PushOpacity {
                alpha,
                width,
                height,
            } => {
                let shape = Rect::new(0.0, 0.0, *width as f64, *height as f64);
                scene.push_layer(Fill::NonZero, Mix::Normal, *alpha, cur, &shape);
            }
            Paint::PopOpacity => scene.pop_layer(),
        }
    }
    scene
}
