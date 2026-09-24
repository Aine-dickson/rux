//! Rux painter, milestones M1–M4.
//!
//! Turns the `Paint` items from `rux-layout` into a `vello::Scene`: filled
//! rounded rectangles for boxes, glyph runs (via `rux-text`) for text. Stage 5
//! of `docs/04-architecture.md`.
//!
//! [`build_scene`] is the whole surface. Everything reaching it is already
//! resolved: absolute rects, concrete colours, the text already shaped. This
//! crate decides nothing about what a document should look like, which is
//! deliberate. It is the one stage that can be re-pointed at a different
//! backend without the layers above noticing, and keeping every judgement call
//! upstream of it is what preserves that.
//!
//! What it knows how to draw, beyond the two basics: per-corner radii, per-side
//! borders, linear and radial gradients, box shadows, images, scrollbar ticks,
//! the selection highlight and the rule under an in-progress IME composition.
//!
//! Two pieces of state live here rather than upstream, both for the same
//! reason, that they are pure caches of expensive work and no one above needs
//! to know they exist:
//!
//! - [`ImageCache`], because decoding is the slow part and the shell repaints
//!   on every event, so a `src` is read from disk at most once. A decode
//!   failure is remembered as a miss and not retried.
//! - The text engine's font and layout contexts, which `rux-text` owns and this
//!   crate borrows.
//!
//! The selection highlight is `::selection` when the author wrote one, and the
//! platform's highlight otherwise, which the shell hands over through
//! [`set_default_selection`]. The composition underline is the text's colour.

use std::collections::HashMap;

use rux_layout::{
    Background, Corners, FillRule, Gradient, GradientKind, LineCap, LineJoin, Paint, PathCmd, Rgba,
    TextAlign, TextContent, TextWrap,
};
use rux_text::{Align, TextEngine, TextStyle, Wrap};
use vello::kurbo::{Affine, BezPath, Cap, Join, Point, Rect, RoundedRect, RoundedRectRadii, Stroke, Vec2};
use vello::peniko::{
    Blob, Color, ColorStop, Fill, Gradient as PenikoGradient, ImageAlphaType, ImageBrush, ImageData,
    ImageFormat, Mix,
};
use vello::Scene;

/// The selection highlight when neither the author nor the platform has said
/// one: `#89b4fa` at 45%, the focus-ring blue.
const SELECTION: Rgba = Rgba::new(0x89 as f32 / 255.0, 0xb4 as f32 / 255.0, 0xfa as f32 / 255.0, 0x73 as f32 / 255.0);

/// The platform's highlight, packed RGBA8, or 0 for none yet.
///
/// A process-wide value rather than an argument, because it is a fact about
/// the device, like the font list, and every caller of [`build_scene`] would
/// otherwise have to carry it to reach the one place it is read.
static DEFAULT_SELECTION: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// The highlight a selection gets where no `::selection` names one. The shell
/// calls this with the system's own on a platform that has one, so a Rux field
/// highlights like every other field on the device. See CSS's `Highlight`
/// system colour, which is the same idea.
pub fn set_default_selection(color: Rgba) {
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u32;
    let packed = (byte(color.r) << 24) | (byte(color.g) << 16) | (byte(color.b) << 8) | byte(color.a);
    DEFAULT_SELECTION.store(packed, std::sync::atomic::Ordering::Relaxed);
}

/// The highlight for text with no `::selection` background.
pub fn default_selection() -> Rgba {
    match DEFAULT_SELECTION.load(std::sync::atomic::Ordering::Relaxed) {
        0 => SELECTION,
        p => {
            let at = |shift: u32| ((p >> shift) & 0xff) as f32 / 255.0;
            Rgba::new(at(24), at(16), at(8), at(0))
        }
    }
}

/// Thickness of the rule under an in-progress IME composition, in logical px.
/// Deliberately the caret's width, so the two read as the same pen.
const PREEDIT_RULE: f32 = rux_text::CARET_WIDTH;

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
    // Bytes first, path second. The runtime installs a reader that resolves a
    // `src` the same way it resolves a component or a stylesheet, which is what
    // lets an embedded build draw at all: inside an executable there is no file
    // to open. Without a reader this is the filesystem read it always was, so a
    // bare `rux-paint` test and `rux run` both keep working.
    let bytes = match rux_layout::read_image_bytes(src) {
        Some(bytes) => bytes,
        None => std::fs::read(src)
            .map_err(|e| eprintln!("rux: cannot load image {src}: {e}"))
            .ok()?,
    };
    let decoded = decode_bounded(&bytes)
        .map_err(|e| eprintln!("rux: cannot decode image {src}: {e}"))
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

/// Decode an image, refusing one whose header claims more than
/// [`rux_layout::MAX_IMAGE_PIXELS`] before a single pixel is decoded.
///
/// The header is read on its own first because that is the only moment the
/// size is known and nothing has been allocated for it. The decoder's own
/// limits are set as well, for a format whose header and body disagree.
fn decode_bounded(bytes: &[u8]) -> Result<image::DynamicImage, String> {
    let reader = || {
        image::ImageReader::new(std::io::Cursor::new(bytes))
            .with_guessed_format()
            .map_err(|e| e.to_string())
    };
    let (width, height) = reader()?.into_dimensions().map_err(|e| e.to_string())?;
    if let Some(why) = rux_layout::image_too_large(width, height) {
        return Err(why);
    }
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(rux_layout::MAX_IMAGE_SIDE);
    limits.max_image_height = Some(rux_layout::MAX_IMAGE_SIDE);
    // The decoded pixels at the widest format a decoder produces here (16-bit
    // RGBA, 8 bytes a pixel), which the RGBA8 copy afterwards stays inside.
    limits.max_alloc = Some(rux_layout::MAX_IMAGE_PIXELS * 8);
    let mut decoder = reader()?;
    decoder.limits(limits);
    decoder.decode().map_err(|e| e.to_string())
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
    // untransformed, so they don't follow, a documented limitation.)
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
                // Border. Two ways to draw one, because a rounded corner and an
                // uneven edge want different things.
                //
                // **Uniform**: one stroke inset by half its width, so it sits
                // inside the box and follows the corner radius. This was the
                // only path, reading a single width taken from `border.top`, so
                // a `border-bottom: 6px` drew nothing and a `border-top: 6px`
                // drew a box on all four sides.
                //
                // **Uneven**: each edge filled as its own rectangle. A stroke
                // cannot vary along its length, and `border-bottom` under an
                // input is the case this exists for. The corners are square
                // where two thicknesses meet; CSS mitres them diagonally, which
                // is only visible when two *different* non-zero sides share a
                // corner and is not worth a path builder here.
                if let Some(bc) = r.border_color {
                    let b = r.border;
                    let widest = b.top.max(b.right).max(b.bottom).max(b.left);
                    if widest > 0.0 {
                        if b.is_uniform() {
                            let half = (b.top / 2.0) as f64;
                            let inner = rounded_rect(r.x, r.y, r.width, r.height, r.radius, half);
                            scene.stroke(
                                &Stroke::new(b.top as f64),
                                cur,
                                to_color(bc),
                                None,
                                &inner,
                            );
                        } else {
                            let (x, y, w, h) = (r.x as f64, r.y as f64, r.width as f64, r.height as f64);
                            let color = to_color(bc);
                            // Top and bottom span the full width; the sides take
                            // what is left between them, so a corner is painted
                            // once rather than twice over.
                            let edges = [
                                (x, y, w, b.top as f64),
                                (x, y + h - b.bottom as f64, w, b.bottom as f64),
                                (x, y + b.top as f64, b.left as f64, h - (b.top + b.bottom) as f64),
                                (
                                    x + w - b.right as f64,
                                    y + b.top as f64,
                                    b.right as f64,
                                    h - (b.top + b.bottom) as f64,
                                ),
                            ];
                            for (ex, ey, ew, eh) in edges {
                                if ew <= 0.0 || eh <= 0.0 {
                                    continue;
                                }
                                scene.fill(
                                    Fill::NonZero,
                                    cur,
                                    color,
                                    None,
                                    &Rect::new(ex, ey, ex + ew, ey + eh),
                                );
                            }
                        }
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
                // The selection highlight goes behind the glyphs: the author's
                // `::selection` background, or the platform's highlight.
                let mut selected = Vec::new();
                if let Some((start, end)) = t.content.selection {
                    let rects = text.selection_rects(
                        &t.content.text,
                        &text_style(&t.content),
                        Some(t.width),
                        start,
                        end,
                    );
                    let fill = to_color(
                        t.content.selection_style.background.unwrap_or_else(default_selection),
                    );
                    for (sx, sy, sw, sh) in rects {
                        let rect = Rect::new(
                            (t.x + sx) as f64,
                            (t.y + sy) as f64,
                            (t.x + sx + sw) as f64,
                            (t.y + sy + sh) as f64,
                        );
                        scene.fill(Fill::NonZero, cur, fill, None, &rect);
                        selected.push(rect);
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
                // `::selection { color }`: the selected glyphs drawn a second
                // time in that colour, clipped to the highlight, so a glyph cut
                // by the selection's edge is two colours exactly as it is in a
                // browser.
                if let Some(ink) = t.content.selection_style.color {
                    for rect in &selected {
                        scene.push_clip_layer(Fill::NonZero, cur, rect);
                        text.draw(
                            &mut scene,
                            t.x,
                            t.y,
                            &t.content.text,
                            &text_style(&t.content),
                            to_color(ink),
                            to_align(t.content.align),
                            Some(t.width),
                            cur,
                        );
                        scene.pop_layer();
                    }
                }
                // An in-progress IME composition, underlined so it reads as
                // provisional. Reuses the selection geometry, which already
                // returns one rect per line a range spans, and puts a rule along
                // the bottom of each in the text's own colour.
                if let Some((start, end)) = t.content.preedit {
                    let rects = text.selection_rects(
                        &t.content.text,
                        &text_style(&t.content),
                        Some(t.width),
                        start,
                        end,
                    );
                    for (sx, sy, sw, sh) in rects {
                        let rule = Rect::new(
                            (t.x + sx) as f64,
                            (t.y + sy + sh - PREEDIT_RULE) as f64,
                            (t.x + sx + sw) as f64,
                            (t.y + sy + sh) as f64,
                        );
                        scene.fill(Fill::NonZero, cur, to_color(t.content.color), None, &rule);
                    }
                }
                // The focused input's caret, drawn on top of its own text,
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
            // box. A stroked path rather than a ✓ glyph, the glyph is whatever
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
            // <path>: the geometry, offset to where layout put the element,
            // filled then stroked. Fill first because that is the order SVG
            // paints in and it is the one that looks right: a stroke is a
            // border on the shape and belongs over its own fill, not under it.
            Paint::Path(p) => {
                let mut path = BezPath::new();
                let (ox, oy) = (p.x as f64, p.y as f64);
                for c in &p.content.commands {
                    match *c {
                        PathCmd::Move { x, y } => path.move_to((ox + x as f64, oy + y as f64)),
                        PathCmd::Curve {
                            x1,
                            y1,
                            x2,
                            y2,
                            x,
                            y,
                        } => path.curve_to(
                            (ox + x1 as f64, oy + y1 as f64),
                            (ox + x2 as f64, oy + y2 as f64),
                            (ox + x as f64, oy + y as f64),
                        ),
                        PathCmd::Close => path.close_path(),
                    }
                }
                if path.is_empty() {
                    continue;
                }
                if let Some(c) = p.paint.fill {
                    let rule = match p.paint.fill_rule {
                        FillRule::NonZero => Fill::NonZero,
                        FillRule::EvenOdd => Fill::EvenOdd,
                    };
                    scene.fill(rule, cur, to_color(c), None, &path);
                }
                // A zero width is not a hairline. CSS says a zero-width border
                // is no border, and a stroke should not be the one place where
                // asking for nothing draws something.
                if let (Some(c), true) = (p.paint.stroke, p.paint.stroke_width > 0.0) {
                    let stroke = Stroke::new(p.paint.stroke_width as f64)
                        .with_caps(match p.paint.cap {
                            LineCap::Butt => Cap::Butt,
                            LineCap::Round => Cap::Round,
                            LineCap::Square => Cap::Square,
                        })
                        .with_join(match p.paint.join {
                            LineJoin::Miter => Join::Miter,
                            LineJoin::Round => Join::Round,
                            LineJoin::Bevel => Join::Bevel,
                        });
                    scene.stroke(&stroke, cur, to_color(c), None, &path);
                }
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
            // never clips, an overflowing child still shows through.
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

#[cfg(test)]
mod tests {
    use super::decode_bounded;

    fn png(width: u32, height: u32) -> Vec<u8> {
        let img = image::GrayImage::new(width, height);
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    }

    /// A decompression bomb: 49 megapixels of one colour is a few kilobytes of
    /// PNG, and would be 196 MB decoded to RGBA. Refused from the header.
    #[test]
    fn an_image_over_the_pixel_limit_is_refused_before_decoding() {
        let bomb = png(7000, 7000);
        assert!(bomb.len() < 1_000_000, "the point is that the file is small: {}", bomb.len());
        let why = decode_bounded(&bomb).expect_err("refused");
        assert!(why.contains("megapixel"), "{why}");
    }

    #[test]
    fn an_image_over_the_side_limit_is_refused() {
        let why = decode_bounded(&png(20_000, 2)).expect_err("refused");
        assert!(why.contains("16384"), "{why}");
    }

    #[test]
    fn an_ordinary_image_decodes() {
        let img = decode_bounded(&png(640, 480)).expect("decodes");
        assert_eq!((img.width(), img.height()), (640, 480));
    }
}
