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

use rux_layout::filter::FilterOp;
use rux_layout::{
    TextLook, Background, BgAttachment, BgBox, BgLayer, BgLen, BgRepeat, BgSize, BgPos, BlendMode, BorderStyle, PaintBackground, PaintBorder, Sides, Corners, FillRule, Gradient, GradientKind, LineCap, LineJoin, Paint, PathCmd, Rgba,
    TextAlign, TextContent, TextWrap,
};
use rux_text::{Align, TextEngine, TextStyle, Wrap};
use vello::kurbo::{Affine, BezPath, Cap, Join, Point, Rect, RoundedRect, RoundedRectRadii, Shape, Stroke, Vec2};
use vello::peniko::{
    BlendMode as PenikoBlend, Blob, Color, ColorStop, Compose, Fill, Extend, Gradient as PenikoGradient, ImageAlphaType, ImageBrush,
    ImageData, ImageFormat, Mix,
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
    let stops = ramp_stops(g);
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

/// A gradient's stops as vello is handed them.
///
/// vello's ramp starts its first stop at 0 whatever its offset says (found in
/// vello 0.9, still so in 0.11: `make_ramp` in vello_encoding's
/// `ramp_cache.rs`; linebender/vello#2002 and #2013), so `red 50%, blue 50%`
/// faded from the left edge instead of turning at the middle. CSS holds the
/// first colour up to the first stop: say so with a stop of that colour at 0.
/// #2002 reports the last stop short of 1 drawn wrong too, so it is held to 1
/// the same way. Harmless once vello is fixed.
fn ramp_stops(g: &Gradient) -> Vec<ColorStop> {
    let mut stops: Vec<ColorStop> = g
        .stops
        .iter()
        .map(|(rgba, off)| ColorStop { offset: *off, color: to_color(*rgba).into() })
        .collect();
    if let Some(first) = stops.first().copied().filter(|s| s.offset > 0.0) {
        stops.insert(0, ColorStop { offset: 0.0, ..first });
    }
    if let Some(last) = stops.last().copied().filter(|s| s.offset < 1.0) {
        stops.push(ColorStop { offset: 1.0, ..last });
    }
    stops
}

/// A background over a box with its corners rounded: a colour, a gradient
/// across the box, or an image covering it.
fn fill_background(
    scene: &mut Scene,
    cur: Affine,
    bg: &Background,
    (x, y, width, height): (f32, f32, f32, f32),
    radius: Corners,
    images: &mut ImageCache,
) {
    let shape = rounded_rect(x, y, width, height, radius, 0.0);
    match bg {
        Background::Color(c) => {
            scene.fill(Fill::NonZero, cur, to_color(*c), None, &shape);
        }
        Background::Gradient(g) => {
            let brush = to_gradient(g, x, y, width, height);
            scene.fill(Fill::NonZero, cur, &brush, None, &shape);
        }
        // Layers on a bare rect, which has no border or padding to place them
        // by: only something built by hand, since layout gives them a paint
        // of their own.
        Background::Layers(layers) => {
            let b = PaintBackground {
                x,
                y,
                width,
                height,
                radius,
                border: Sides::default(),
                padding: Sides::default(),
                view: (x, y, width, height),
                local: (x, y, width, height),
                layers: layers.clone(),
            };
            draw_background(scene, cur, &b, images);
        }
        Background::Image(src) => {
            if let Some(brush) = images.get(src) {
                let (iw, ih) = (brush.image.width as f64, brush.image.height as f64);
                if iw > 0.0 && ih > 0.0 {
                    // `cover`: scale to fill the box, centre, and
                    // clip to the box's rounded corners.
                    let scale = (width as f64 / iw).max(height as f64 / ih);
                    let (sw, sh) = (iw * scale, ih * scale);
                    let ox = x as f64 + (width as f64 - sw) / 2.0;
                    let oy = y as f64 + (height as f64 - sh) / 2.0;
                    let tf = Affine::scale(scale).then_translate(Vec2::new(ox, oy));
                    scene.push_clip_layer(Fill::NonZero, cur, &shape);
                    scene.draw_image(brush, cur * tf);
                    scene.pop_layer();
                }
            }
        }
    }
}

/// One of a box's three boxes, `(x, y, width, height)`, and its corners: the
/// border box's corners, less the border and padding inside them.
fn box_of(b: &PaintBackground, which: BgBox) -> ((f32, f32, f32, f32), Corners) {
    let s = |a: Sides, c: Sides| Sides { top: a.top + c.top, right: a.right + c.right, bottom: a.bottom + c.bottom, left: a.left + c.left };
    let i = match which {
        BgBox::Border => Sides::default(),
        BgBox::Padding => b.border,
        BgBox::Content => s(b.border, b.padding),
    };
    let rect = (b.x + i.left, b.y + i.top, (b.width - i.left - i.right).max(0.0), (b.height - i.top - i.bottom).max(0.0));
    // Each corner loses the thicker of the two sides that meet at it.
    let r = b.radius;
    let radius = [
        (r[0] - i.top.max(i.left)).max(0.0),
        (r[1] - i.top.max(i.right)).max(0.0),
        (r[2] - i.bottom.max(i.right)).max(0.0),
        (r[3] - i.bottom.max(i.left)).max(0.0),
    ];
    (rect, radius)
}

/// The most tiles one layer draws one at a time. A gradient pattern a few
/// pixels across over a large box is the case that reaches it; an image
/// that repeats on both axes is one fill whatever its count.
const MAX_TILES: usize = 4096;

/// A background with image layers: the colour first, cut to the bottom
/// layer's clip, then the layers from the bottom up.
fn draw_background(scene: &mut Scene, cur: Affine, b: &PaintBackground, images: &mut ImageCache) {
    if let Some(c) = b.layers.color {
        let ((x, y, w, h), radius) = box_of(b, b.layers.color_clip());
        scene.fill(Fill::NonZero, cur, to_color(c), None, &rounded_rect(x, y, w, h, radius, 0.0));
    }
    for layer in b.layers.layers.iter().rev() {
        draw_layer(scene, cur, b, layer, images);
    }
}

fn draw_layer(scene: &mut Scene, cur: Affine, b: &PaintBackground, layer: &BgLayer, images: &mut ImageCache) {
    // The positioning area: the origin box, or for a `fixed` layer the
    // window, or for a `local` one the origin box of the scrolled content.
    let area = match layer.attachment {
        BgAttachment::Fixed => b.view,
        BgAttachment::Scroll => box_of(b, layer.origin).0,
        BgAttachment::Local => {
            let (x, y, w, h) = b.local;
            let (bd, pd) = (b.border, b.padding);
            match layer.origin {
                BgBox::Border => (x - bd.left, y - bd.top, w + bd.left + bd.right, h + bd.top + bd.bottom),
                BgBox::Padding => (x, y, w, h),
                BgBox::Content => (x + pd.left, y + pd.top, (w - pd.left - pd.right).max(0.0), (h - pd.top - pd.bottom).max(0.0)),
            }
        }
    };
    let (ax, ay, aw, ah) = area;
    let natural = match &layer.image {
        Background::Image(src) => match images.get(src) {
            Some(brush) if brush.image.width > 0 && brush.image.height > 0 => {
                Some((brush.image.width as f32, brush.image.height as f32))
            }
            // Not drawn, as a browser draws nothing for an image it cannot load.
            _ => return,
        },
        Background::Gradient(_) => None,
        _ => return,
    };
    let (mut tw, mut th) = tile_size(layer.size, natural, (aw, ah));
    // `round`: as many whole tiles as come nearest, stretched to fit. A side
    // left `auto` keeps the image's proportions.
    let auto = match layer.size {
        BgSize::Sized([w, h]) => [w == BgLen::Auto, h == BgLen::Auto],
        _ => [false, false],
    };
    let (rx, ry) = (layer.repeat[0] == BgRepeat::Round, layer.repeat[1] == BgRepeat::Round);
    if rx && tw > 0.0 {
        let n = (aw / tw).round().max(1.0);
        let was = tw;
        tw = aw / n;
        if !ry && auto[1] {
            th *= tw / was;
        }
    }
    if ry && th > 0.0 {
        let n = (ah / th).round().max(1.0);
        let was = th;
        th = ah / n;
        if !rx && auto[0] {
            tw *= th / was;
        }
    }
    if tw <= 0.0 || th <= 0.0 {
        return;
    }
    let ((cx, cy, cw, ch), clip_radius) = box_of(b, layer.clip);
    if cw <= 0.0 || ch <= 0.0 {
        return;
    }
    // Where the tiles start and how far apart they are, per axis.
    let axis = |repeat: BgRepeat, pos: BgPos, start: f32, len: f32, tile: f32, cmin: f32, cmax: f32| -> Vec<f32> {
        let placed = start + pos.pct / 100.0 * (len - tile) + pos.px;
        let (first, step) = match repeat {
            BgRepeat::NoRepeat => return vec![placed],
            BgRepeat::Repeat | BgRepeat::Round => (placed, tile),
            BgRepeat::Space => {
                let n = (len / tile).floor();
                if n < 2.0 {
                    return vec![placed];
                }
                // The first tile and the last touch the area's edges.
                (start, tile + (len - n * tile) / (n - 1.0))
            }
        };
        let back = ((first - cmin) / step).ceil();
        let mut at = first - back * step;
        let mut out = Vec::new();
        while at < cmax && out.len() <= MAX_TILES {
            if at + tile > cmin {
                out.push(at);
            }
            at += step;
        }
        out
    };
    let shape = rounded_rect(cx, cy, cw, ch, clip_radius, 0.0);
    scene.push_clip_layer(Fill::NonZero, cur, &shape);
    match &layer.image {
        // An image repeated on both axes is one fill with a repeating brush,
        // however many tiles that is.
        Background::Image(src)
            if matches!(layer.repeat[0], BgRepeat::Repeat | BgRepeat::Round)
                && matches!(layer.repeat[1], BgRepeat::Repeat | BgRepeat::Round) =>
        {
            let brush = images.get(src).expect("loaded above").clone().with_extend(Extend::Repeat);
            let (iw, ih) = natural.expect("an image has a size");
            let ox = ax + layer.position[0].pct / 100.0 * (aw - tw) + layer.position[0].px;
            let oy = ay + layer.position[1].pct / 100.0 * (ah - th) + layer.position[1].px;
            let tf = Affine::scale_non_uniform((tw / iw) as f64, (th / ih) as f64).then_translate(Vec2::new(ox as f64, oy as f64));
            let rect = Rect::new(cx as f64, cy as f64, (cx + cw) as f64, (cy + ch) as f64);
            scene.fill(Fill::NonZero, cur, &brush, Some(tf), &rect);
        }
        image => {
            let xs = axis(layer.repeat[0], layer.position[0], ax, aw, tw, cx, cx + cw);
            let ys = axis(layer.repeat[1], layer.position[1], ay, ah, th, cy, cy + ch);
            if xs.len() * ys.len() > MAX_TILES {
                warn_tiles();
                scene.pop_layer();
                return;
            }
            for &ty in ys.iter() {
                for &tx in xs.iter() {
                    match image {
                        Background::Gradient(g) => {
                            let brush = to_gradient(g, tx, ty, tw, th);
                            let rect = Rect::new(tx as f64, ty as f64, (tx + tw) as f64, (ty + th) as f64);
                            scene.fill(Fill::NonZero, cur, &brush, None, &rect);
                        }
                        Background::Image(src) => {
                            let brush = images.get(src).expect("loaded above");
                            let (iw, ih) = natural.expect("an image has a size");
                            let tf = Affine::scale_non_uniform((tw / iw) as f64, (th / ih) as f64)
                                .then_translate(Vec2::new(tx as f64, ty as f64));
                            scene.draw_image(brush, cur * tf);
                        }
                        _ => {}
                    }
                }
            }
        }
    }
    scene.pop_layer();
}

/// Said once: a pattern too fine for its box is left undrawn rather than
/// drawn at a cost that stalls the frame.
fn warn_tiles() {
    static SAID: std::sync::Once = std::sync::Once::new();
    SAID.call_once(|| {
        eprintln!("rux: a background repeats more than {MAX_TILES} times in its box and is not drawn; make its tiles larger")
    });
}

/// A layer's tile, from its `background-size`, the image's natural size (a
/// gradient has none) and the positioning area, as CSS sizes it.
fn tile_size(size: BgSize, natural: Option<(f32, f32)>, (aw, ah): (f32, f32)) -> (f32, f32) {
    match (size, natural) {
        (BgSize::Cover | BgSize::Contain, None) => (aw, ah),
        (BgSize::Cover | BgSize::Contain, Some((iw, ih))) => {
            let (sx, sy) = (aw / iw, ah / ih);
            let s = if size == BgSize::Cover { sx.max(sy) } else { sx.min(sy) };
            (iw * s, ih * s)
        }
        (BgSize::Sized([w, h]), _) => {
            let len = |l: BgLen, of: f32| match l {
                BgLen::Auto => None,
                BgLen::Px(v) => Some(v.max(0.0)),
                BgLen::Pct(p) => Some((p / 100.0 * of).max(0.0)),
            };
            match (len(w, aw), len(h, ah), natural) {
                (Some(w), Some(h), _) => (w, h),
                (None, None, Some(n)) => n,
                (None, None, None) => (aw, ah),
                (Some(w), None, Some((iw, ih))) => (w, w * ih / iw),
                (None, Some(h), Some((iw, ih))) => (h * iw / ih, h),
                (Some(w), None, None) => (w, ah),
                (None, Some(h), None) => (aw, h),
            }
        }
    }
}

/// The dash pattern that draws a row of dots `width` across with round caps:
/// a dash too short to see, which the caps round into a dot, every two
/// widths. A dash of length zero would be the exact pattern, and vello draws
/// nothing for it.
fn dots(width: f64) -> [f64; 2] {
    let tiny = width * 0.01;
    [tiny, width * 2.0 - tiny]
}

/// A border that is not plain solid all round: `dashed`, `dotted`, `double`,
/// `groove`, `ridge`, `inset` and `outset`.
///
/// One width and one style on every side is stroked once around the box,
/// following its corners, dashes and dots included. Anything else is drawn a
/// side at a time, each side a band from corner to corner whose ends are cut
/// on the diagonal, as CSS mitres them, and its dashes laid along its middle.
///
/// The 3D styles are shaded from the colour: the side that would be in
/// light keeps it and the one in shade is a third darker. CSS leaves the
/// exact shades to the browser, and browsers differ.
fn draw_border(scene: &mut Scene, cur: Affine, b: &PaintBorder) {
    let w = [b.border.top, b.border.right, b.border.bottom, b.border.left];
    let color = to_color(b.color);
    let dark = to_color(Rgba::new(b.color.r * 2.0 / 3.0, b.color.g * 2.0 / 3.0, b.color.b * 2.0 / 3.0, b.color.a));
    // Which sides are in light: top and left for `outset` and `ridge`'s
    // outer half, bottom and right for `inset` and `groove`'s.
    let lit = |style: BorderStyle, side: usize, outer: bool| -> Color {
        let top_left = side == 0 || side == 3;
        let raised = match style {
            BorderStyle::Outset => true,
            BorderStyle::Inset => false,
            BorderStyle::Ridge => outer,
            BorderStyle::Groove => !outer,
            _ => return color,
        };
        if raised == top_left { color } else { dark }
    };
    let uniform = w.iter().all(|v| *v == w[0]) && b.styles.iter().all(|s| *s == b.styles[0]);
    let style = b.styles[0];
    let shaded = matches!(style, BorderStyle::Groove | BorderStyle::Ridge | BorderStyle::Inset | BorderStyle::Outset);
    if uniform && !shaded && w[0] > 0.0 {
        let width = w[0] as f64;
        let ring = |inset: f64, thick: f64| (rounded_rect(b.x, b.y, b.width, b.height, b.radius, inset), Stroke::new(thick));
        match style {
            BorderStyle::Dashed => {
                let (shape, stroke) = ring(width / 2.0, width);
                let stroke = stroke.with_dashes(0.0, [width * 3.0, width * 3.0]);
                scene.stroke(&stroke, cur, color, None, &shape);
            }
            BorderStyle::Dotted => {
                let (shape, stroke) = ring(width / 2.0, width);
                let stroke = stroke.with_caps(Cap::Round).with_dashes(0.0, dots(width));
                scene.stroke(&stroke, cur, color, None, &shape);
            }
            BorderStyle::Double => {
                for inset in [width / 6.0, width * 5.0 / 6.0] {
                    let (shape, stroke) = ring(inset, width / 3.0);
                    scene.stroke(&stroke, cur, color, None, &shape);
                }
            }
            _ => {
                let (shape, stroke) = ring(width / 2.0, width);
                scene.stroke(&stroke, cur, color, None, &shape);
            }
        }
        return;
    }
    // A side at a time. The box's corners, outer and inner, clockwise from
    // the top left; each side is the band between its two outer corners and
    // the two inner ones.
    let (x0, y0, x1, y1) = (b.x as f64, b.y as f64, (b.x + b.width) as f64, (b.y + b.height) as f64);
    let (t, r, bt, l) = (w[0] as f64, w[1] as f64, w[2] as f64, w[3] as f64);
    let outer = [(x0, y0), (x1, y0), (x1, y1), (x0, y1)];
    let inner = [(x0 + l, y0 + t), (x1 - r, y0 + t), (x1 - r, y1 - bt), (x0 + l, y1 - bt)];
    // With rounded corners a side is a wedge instead: its band carried on
    // past each inner corner as far as the padding box's curve reaches, and
    // cut to its slice of the ring between two rounded rectangles. A corner's
    // radius shrinks by the wider of the two widths either side of it.
    let rounded = b.radius.iter().any(|v| *v > 0.0);
    let across = [t.max(l), t.max(r), bt.max(r), bt.max(l)];
    // The rounded rectangle `f` of the way from the border box (0) to the
    // padding box (1).
    let ring_at = |f: f64| {
        let rad = |i: usize| (b.radius[i] as f64 - across[i] * f).max(0.0);
        RoundedRect::from_rect(
            Rect::new(x0 + l * f, y0 + t * f, x1 - r * f, y1 - bt * f),
            RoundedRectRadii::new(rad(0), rad(1), rad(2), rad(3)),
        )
    };
    let slice = |from: f64, to: f64| {
        let mut path = ring_at(from).to_path(0.1);
        path.extend(ring_at(to).to_path(0.1));
        path
    };
    let toward = [(1.0, 1.0), (-1.0, 1.0), (-1.0, -1.0), (1.0, -1.0)];
    let far: [(f64, f64); 4] = std::array::from_fn(|i| {
        let k = (b.radius[i] as f64 - across[i]).max(0.0);
        (inner[i].0 + toward[i].0 * k, inner[i].1 + toward[i].1 * k)
    });
    for (side, &wide) in w.iter().enumerate() {
        if wide <= 0.0 {
            continue;
        }
        let (a, z) = (side, (side + 1) % 4);
        let band = |from: f64, to: f64| {
            // The part of the band between `from` and `to` of its depth,
            // 0 at the outer edge and 1 at the inner.
            let at = |o: (f64, f64), i: (f64, f64), f: f64| (o.0 + (i.0 - o.0) * f, o.1 + (i.1 - o.1) * f);
            let mut path = BezPath::new();
            path.move_to(at(outer[a], inner[a], from));
            path.line_to(at(outer[z], inner[z], from));
            path.line_to(at(outer[z], inner[z], to));
            path.line_to(at(outer[a], inner[a], to));
            path.close_path();
            path
        };
        let piece = |scene: &mut Scene, from: f64, to: f64, paint: Color| {
            if !rounded {
                scene.fill(Fill::NonZero, cur, paint, None, &band(from, to));
                return;
            }
            let mut wedge = BezPath::new();
            wedge.move_to(outer[a]);
            for p in [outer[z], inner[z], far[z], far[a], inner[a]] {
                wedge.line_to(p);
            }
            wedge.close_path();
            scene.push_clip_layer(Fill::EvenOdd, cur, &slice(from, to));
            scene.fill(Fill::NonZero, cur, paint, None, &wedge);
            scene.pop_layer();
        };
        let style = b.styles[side];
        match style {
            BorderStyle::Dashed | BorderStyle::Dotted => {
                let width = wide as f64;
                let mid = |o: (f64, f64), i: (f64, f64)| ((o.0 + i.0) / 2.0, (o.1 + i.1) / 2.0);
                let mut line = BezPath::new();
                line.move_to(mid(outer[a], inner[a]));
                line.line_to(mid(outer[z], inner[z]));
                let stroke = if style == BorderStyle::Dotted {
                    Stroke::new(width).with_caps(Cap::Round).with_dashes(0.0, dots(width))
                } else {
                    Stroke::new(width).with_caps(Cap::Butt).with_dashes(0.0, [width * 3.0, width * 3.0])
                };
                // Rounded, the straight line is cut to the ring.
                if rounded {
                    scene.push_clip_layer(Fill::EvenOdd, cur, &slice(0.0, 1.0));
                }
                scene.stroke(&stroke, cur, color, None, &line);
                if rounded {
                    scene.pop_layer();
                }
            }
            BorderStyle::Double => {
                piece(scene, 0.0, 1.0 / 3.0, color);
                piece(scene, 2.0 / 3.0, 1.0, color);
            }
            BorderStyle::Groove | BorderStyle::Ridge => {
                piece(scene, 0.0, 0.5, lit(style, side, true));
                piece(scene, 0.5, 1.0, lit(style, side, false));
            }
            _ => piece(scene, 0.0, 1.0, lit(style, side, true)),
        }
    }
}

/// A path from Rux's commands, moved by `(ox, oy)`.
fn bez_path(commands: &[PathCmd], ox: f64, oy: f64) -> BezPath {
    let mut path = BezPath::new();
    for c in commands {
        match *c {
            PathCmd::Move { x, y } => path.move_to((ox + x as f64, oy + y as f64)),
            PathCmd::Curve { x1, y1, x2, y2, x, y } => path.curve_to(
                (ox + x1 as f64, oy + y1 as f64),
                (ox + x2 as f64, oy + y2 as f64),
                (ox + x as f64, oy + y as f64),
            ),
            PathCmd::Close => path.close_path(),
        }
    }
    path
}

/// vello's name for a CSS blend mode.
fn mix_of(blend: BlendMode) -> Mix {
    match blend {
        BlendMode::Normal => Mix::Normal,
        BlendMode::Multiply => Mix::Multiply,
        BlendMode::Screen => Mix::Screen,
        BlendMode::Overlay => Mix::Overlay,
        BlendMode::Darken => Mix::Darken,
        BlendMode::Lighten => Mix::Lighten,
        BlendMode::ColorDodge => Mix::ColorDodge,
        BlendMode::ColorBurn => Mix::ColorBurn,
        BlendMode::HardLight => Mix::HardLight,
        BlendMode::SoftLight => Mix::SoftLight,
        BlendMode::Difference => Mix::Difference,
        BlendMode::Exclusion => Mix::Exclusion,
        BlendMode::Hue => Mix::Hue,
        BlendMode::Saturation => Mix::Saturation,
        BlendMode::Color => Mix::Color,
        BlendMode::Luminosity => Mix::Luminosity,
    }
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
        rich: tc.rich.as_deref(),
        direction: tc.direction,
        look: tc.look.as_deref(),
    }
}

/// The text engine as the layout asks it: sizes, and where the runs and boxes
/// of a sentence landed. What the shell lays out through.
pub struct EngineMeasure<'a>(pub &'a mut TextEngine);

impl rux_layout::TextMeasure for EngineMeasure<'_> {
    fn measure(&mut self, tc: &TextContent, max_width: Option<f32>) -> (f32, f32) {
        self.0.measure(&tc.text, &text_style(tc), max_width)
    }

    /// At the width the text is drawn at, aligned as it is drawn, so the
    /// shaped layout is the one `draw` reads from the cache.
    fn inline(&mut self, tc: &TextContent, width: f32) -> std::sync::Arc<rux_layout::InlineGeometry> {
        self.0.placed(&tc.text, &text_style(tc), to_align(tc.align), Some(width))
    }
}

fn to_align(a: TextAlign) -> Align {
    match a {
        TextAlign::Start => Align::Start,
        TextAlign::Center => Align::Center,
        TextAlign::End => Align::End,
        TextAlign::Justify => Align::Justify,
        TextAlign::Left => Align::Left,
        TextAlign::Right => Align::Right,
    }
}

/// What part of the window can still be drawn on, as a box in the window's
/// coordinates and the same box in the coordinates of the current transform.
/// `None` is nowhere: a clip that misses the window, or a transform that
/// collapses everything under it to a line.
#[derive(Clone, Copy)]
struct Reach {
    window: Option<Rect>,
    local: Option<Rect>,
    cur: Affine,
}

/// The overlap of two boxes, `None` when they do not overlap.
fn overlap(a: Rect, b: Rect) -> Option<Rect> {
    let r = Rect::new(a.x0.max(b.x0), a.y0.max(b.y0), a.x1.min(b.x1), a.y1.min(b.y1));
    (r.x0 < r.x1 && r.y0 < r.y1).then_some(r)
}

/// Whether anything inside `bounds` can land in `reach`.
///
/// Within [`EDGE`] counts: an edge is anti-aliased, a clip's edge too, so a
/// box that ends exactly where a clip begins still shades the pixel the two
/// share whenever that edge is not on a pixel boundary. Seen at 1.5x scale as
/// a one-pixel line along the top of a scroller when the card scrolled just
/// out of it was left out (examples/scroll.rux).
fn reaches(reach: Option<Rect>, bounds: Rect) -> bool {
    reach.is_some_and(|v| {
        bounds.x0 < v.x1 + EDGE && v.x0 - EDGE < bounds.x1 && bounds.y0 < v.y1 + EDGE && v.y0 - EDGE < bounds.y1
    })
}

/// How far outside what can be seen a paint still counts as seen, in logical
/// pixels: more than one physical pixel at any scale at or above 1x.
const EDGE: f64 = 2.0;

fn rect_of(x: f32, y: f32, width: f32, height: f32) -> Rect {
    Rect::new(x as f64, y as f64, (x + width) as f64, (y + height) as f64)
}

/// The window box seen from inside a transform: the smallest box in its own
/// coordinates that covers everything it can show.
fn local_reach(window: Option<Rect>, cur: Affine) -> Option<Rect> {
    let det = cur.determinant();
    if !det.is_finite() || det.abs() < 1e-12 {
        return None;
    }
    window.map(|w| cur.inverse().transform_rect_bbox(w))
}

/// Whether a text's glyphs can land in `reach`.
///
/// Glyphs are not held by their box: a long word or `nowrap` runs past its
/// right edge, centred or end-aligned text past its left, and a box given a
/// height shorter than its lines lets them fall out of the bottom. They never
/// rise above the box's top or, aligned to the start, begin left of it, so a
/// box wholly below the window (the usual case, a long list) or wholly right
/// of it is answered without measuring. Only a box that misses the window in
/// another direction is measured, at the width it is drawn at.
fn text_reaches(
    reach: Option<Rect>,
    t: &rux_layout::PaintText,
    measure: &mut impl FnMut(&TextContent, f32) -> (f32, f32),
) -> bool {
    let Some(v) = reach else {
        return false;
    };
    // Room for a glyph's overhang: italics, accents, a line taller than its
    // font. A whole line of it, which is generous.
    let slack = t.content.font_size.max(t.content.line_height.unwrap_or(0.0)) as f64;
    let (x, y, w, h) = (t.x as f64, t.y as f64, t.width as f64, t.height as f64);
    if y - slack >= v.y1 + EDGE {
        return false;
    }
    // Anchored at its left edge, so nothing of it reaches left of `x`. A
    // right-to-left text's start is its right edge, and it overflows leftwards.
    let rtl = t.content.direction == Some(rux_layout::Direction::Rtl);
    let from_start = match t.content.align {
        TextAlign::Left => true,
        TextAlign::Start | TextAlign::Justify => !rtl,
        _ => false,
    };
    if from_start && x - slack >= v.x1 + EDGE {
        return false;
    }
    if reaches(reach, Rect::new(x - slack, y - slack, x + w + slack, y + h + slack)) {
        return true;
    }
    let (mw, mh) = measure(&t.content, t.width);
    let (mw, mh) = (mw as f64, mh as f64);
    reaches(
        reach,
        Rect::new(x + (w - mw).min(0.0) - slack, y - slack, x + w.max(mw) + slack, y + h.max(mh) + slack),
    )
}

/// Which of `items` can show any of themselves in a window `view` wide and
/// high, one flag per item (track (g).6 of `docs/11-next.md`).
///
/// A long list paints every row, and all but the few on screen were encoded
/// and handed to the GPU for nothing. Each item's bounds are tested against
/// what the clips and transforms above it leave of the window; the bounds are
/// generous (a shadow's blur, a stroke's miter, a text's overflow, see
/// [`text_reaches`]) and a rounded clip counts as its whole box, so an item is
/// dropped only when none of it can be seen. Transform and opacity markers
/// are always kept, so the layers still pair up; a clip that leaves nothing
/// of the window is dropped with its pop, since nothing inside it can show,
/// and a clip layer costs the GPU even when it is empty. A long list of
/// `content-visibility` rows has one per row near the screen.
///
/// `measure` sizes a text at a given width, as it will be drawn.
pub fn visible_paints(
    items: &[Paint],
    view: (f32, f32),
    mut measure: impl FnMut(&TextContent, f32) -> (f32, f32),
) -> Vec<bool> {
    let full = Rect::new(0.0, 0.0, view.0 as f64, view.1 as f64);
    let mut reach = Reach { window: Some(full), local: Some(full), cur: Affine::IDENTITY };
    // Each marker's reach before it, and whether it was kept: a pop goes
    // with its push.
    let mut saved: Vec<(Reach, bool)> = Vec::new();
    let mut shown = Vec::with_capacity(items.len());
    for item in items {
        let keep = match item {
            Paint::PushClip { x, y, width, height, .. } => {
                let before = reach;
                let clip = rect_of(*x, *y, *width, *height);
                reach.window = reach.window.and_then(|w| overlap(w, reach.cur.transform_rect_bbox(clip)));
                reach.local = reach.local.and_then(|l| overlap(l, clip));
                let keep = reach.window.is_some();
                saved.push((before, keep));
                keep
            }
            Paint::PushTransform(m) => {
                saved.push((reach, true));
                let mat = Affine::new([m[0] as f64, m[1] as f64, m[2] as f64, m[3] as f64, m[4] as f64, m[5] as f64]);
                reach.cur = reach.cur * mat;
                reach.local = local_reach(reach.window, reach.cur);
                true
            }
            // A shape is inside its box, so it narrows nothing a culled
            // subtree would miss; kept as a layer like opacity.
            Paint::PushOpacity { .. } | Paint::PushShapeClip { .. } => {
                saved.push((reach, true));
                true
            }
            // Its layer's content is already in or out by the paints above.
            Paint::MaskWith { .. } => true,
            Paint::PushFilter { .. } => {
                saved.push((reach, true));
                true
            }
            Paint::Backdrop { .. } => true,
            Paint::PopClip | Paint::PopTransform | Paint::PopOpacity | Paint::PopFilter => match saved.pop() {
                Some((back, kept)) => {
                    reach = back;
                    kept
                }
                None => true,
            },
            Paint::Rect(r) => reaches(reach.local, rect_of(r.x, r.y, r.width, r.height)),
            Paint::Background(r) => reaches(reach.local, rect_of(r.x, r.y, r.width, r.height)),
            Paint::Border(r) => reaches(reach.local, rect_of(r.x, r.y, r.width, r.height)),
            Paint::Image(i) => reaches(reach.local, rect_of(i.x, i.y, i.width, i.height)),
            // vello's blur reaches about three standard deviations, and the
            // standard deviation is half the CSS blur.
            Paint::Shadow { x, y, width, height, blur, .. } => {
                reaches(reach.local, rect_of(*x, *y, *width, *height).inflate(2.0 * *blur as f64 + 1.0, 2.0 * *blur as f64 + 1.0))
            }
            // Inside its padding box, whatever its blur.
            Paint::InsetShadow { x, y, width, height, .. } => reaches(reach.local, rect_of(*x, *y, *width, *height)),
            Paint::Tick(t) => {
                let pad = (t.height * 0.16).max(1.5) as f64 + 1.0;
                reaches(reach.local, rect_of(t.x, t.y, t.width, t.height).inflate(pad, pad))
            }
            Paint::Path(p) => {
                let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
                let mut take = |x: f32, y: f32| {
                    x0 = x0.min(x);
                    y0 = y0.min(y);
                    x1 = x1.max(x);
                    y1 = y1.max(y);
                };
                // A curve stays inside its control points, so they bound it.
                for c in &p.content.commands {
                    match *c {
                        PathCmd::Move { x, y } => take(x, y),
                        PathCmd::Curve { x1, y1, x2, y2, x, y } => {
                            take(x1, y1);
                            take(x2, y2);
                            take(x, y);
                        }
                        PathCmd::Close => {}
                    }
                }
                if x0 > x1 {
                    false
                } else {
                    // A miter can reach several stroke widths past a corner.
                    let pad = if p.paint.stroke.is_some() { p.paint.stroke_width as f64 * 4.0 + 1.0 } else { 1.0 };
                    let bounds = Rect::new(
                        (p.x + x0) as f64,
                        (p.y + y0) as f64,
                        (p.x + x1) as f64,
                        (p.y + y1) as f64,
                    );
                    reaches(reach.local, bounds.inflate(pad, pad))
                }
            }
            Paint::Text(t) | Paint::Ink(t, _) => text_reaches(reach.local, t, &mut measure),
        };
        shown.push(keep);
    }
    shown
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
    build(items, text, images, caret_visible, None, None)
}

/// A part of the frame drawn apart, to be put through `ops` by the renderer
/// and drawn back in its place, which the frame's scene holds as `image`:
/// a `filter`ed subtree, or a `backdrop-filter`'s backdrop.
///
/// `scene` is in logical pixels, as a frame's is, and covers the whole
/// window; `image` is the window's size in device pixels. The renderer
/// draws `scene` scaled, runs the steps over it, and binds the result to
/// `image` (vello's `Renderer::override_image`) before it draws the frame.
pub struct Offscreen {
    pub scene: Scene,
    /// Where the image's top left corner is, in logical pixels: the window's
    /// corner, or for a part drawn in bounds, the corner of those.
    pub origin: (f32, f32),
    pub ops: std::sync::Arc<[FilterOp]>,
    pub image: ImageData,
    /// A backdrop, drawn over the window's background colour; a filtered
    /// subtree is drawn over nothing.
    pub backdrop: bool,
}

/// [`build_scene_within`], with each `filter` and `backdrop-filter` drawn
/// apart, in the order the renderer must make them: a part nested in
/// another comes first. `scale` is device pixels per logical pixel.
pub fn build_scene_apart(
    items: &[Paint],
    text: &mut TextEngine,
    images: &mut ImageCache,
    caret_visible: bool,
    view: (f32, f32),
    scale: f32,
) -> (Scene, Vec<Offscreen>) {
    let shown = visible_paints(items, view, |tc, width| text.measure(&tc.text, &text_style(tc), Some(width)));
    let mut apart = Apart { view, scale, jobs: Vec::new() };
    let scene = build(items, text, images, caret_visible, Some(&shown), Some(&mut apart));
    (scene, apart.jobs)
}

struct Apart {
    view: (f32, f32),
    scale: f32,
    jobs: Vec<Offscreen>,
}

impl Apart {
    /// An image for a part drawn apart, `size` logical pixels, which
    /// nothing has drawn yet: its pixels come from the renderer. An empty
    /// blob, so each one is a new identity to bind.
    fn placeholder(&self, size: (f32, f32)) -> ImageData {
        let device = |v: f32| ((v * self.scale).ceil() as u32).max(1);
        ImageData {
            data: Blob::new(std::sync::Arc::new(Vec::<u8>::new())),
            format: ImageFormat::Rgba8,
            alpha_type: ImageAlphaType::Alpha,
            width: device(size.0),
            height: device(size.1),
        }
    }

    /// The placeholder drawn into a logical scene at `origin`: device
    /// pixels, scaled back.
    fn draw(&self, scene: &mut Scene, image: &ImageData, origin: (f32, f32)) {
        let at = Affine::scale(1.0 / self.scale as f64).then_translate(Vec2::new(origin.0 as f64, origin.1 as f64));
        scene.draw_image(&ImageBrush::new(image.clone()), at);
    }
}

/// [`build_scene`] for a window `view` wide and high, leaving out what cannot
/// be seen in it (see [`visible_paints`]).
pub fn build_scene_within(
    items: &[Paint],
    text: &mut TextEngine,
    images: &mut ImageCache,
    caret_visible: bool,
    view: (f32, f32),
) -> Scene {
    let shown = visible_paints(items, view, |tc, width| text.measure(&tc.text, &text_style(tc), Some(width)));
    build(items, text, images, caret_visible, Some(&shown), None)
}

fn build(
    items: &[Paint],
    text: &mut TextEngine,
    images: &mut ImageCache,
    caret_visible: bool,
    shown: Option<&[bool]>,
    mut apart: Option<&mut Apart>,
) -> Scene {
    let mut scene = Scene::new();
    // Each `filter` group being drawn apart: the scene it interrupted, the
    // layers open in that scene when it did, and the steps it will get.
    let mut levels: Vec<(Scene, usize, std::sync::Arc<[FilterOp]>, Option<(f32, f32, f32, f32)>)> = Vec::new();
    // Layers open in `scene`, which a backdrop's copy of it has to close.
    let mut depth = 0usize;
    // Active `transform` stack. Every draw uses the accumulated transform so a
    // transformed element carries its subtree with it. (Hit regions are computed
    // untransformed, so they don't follow, a documented limitation.)
    let mut tstack: Vec<Affine> = Vec::new();
    for (i, item) in items.iter().enumerate() {
        if shown.is_some_and(|s| !s[i]) {
            continue;
        }
        let cur = tstack.last().copied().unwrap_or(Affine::IDENTITY);
        match item {
            Paint::PushClip { .. } | Paint::PushShapeClip { .. } | Paint::PushOpacity { .. } => depth += 1,
            Paint::PopClip | Paint::PopOpacity => depth = depth.saturating_sub(1),
            _ => {}
        }
        match item {
            // Drawn apart only for a renderer that can filter; otherwise the
            // subtree is drawn as it is, unfiltered.
            Paint::PushFilter { ops, bounds } => {
                if apart.is_some() {
                    levels.push((std::mem::take(&mut scene), depth, ops.clone(), *bounds));
                    depth = 0;
                }
            }
            Paint::PopFilter => {
                if let (Some(a), Some((parent, open, ops, bounds))) = (apart.as_deref_mut(), levels.pop()) {
                    let part = std::mem::replace(&mut scene, parent);
                    depth = open;
                    // Under a transform the bounds are not where it lands, so
                    // only an untransformed part is drawn in them.
                    let (x, y, w, h) = bounds
                        .filter(|_| cur == Affine::IDENTITY)
                        .unwrap_or((0.0, 0.0, a.view.0, a.view.1));
                    let image = a.placeholder((w, h));
                    a.draw(&mut scene, &image, (x, y));
                    a.jobs.push(Offscreen { scene: part, origin: (x, y), ops, image, backdrop: false });
                }
            }
            // Everything drawn so far, each level of it with its open layers
            // closed, drawn apart; the result shows inside this box.
            Paint::Backdrop { x, y, width, height, radius, ops } => {
                if let Some(a) = apart.as_deref_mut() {
                    let mut behind = Scene::new();
                    let closed = |s: &Scene, open: usize| {
                        let mut s = s.clone();
                        (0..open).for_each(|_| s.pop_layer());
                        s
                    };
                    for (s, open, _, _) in &levels {
                        behind.append(&closed(s, *open), None);
                    }
                    behind.append(&closed(&scene, depth), None);
                    let image = a.placeholder(a.view);
                    let shape = rounded_rect(*x, *y, *width, *height, *radius, 0.0);
                    scene.push_clip_layer(Fill::NonZero, cur, &shape);
                    a.draw(&mut scene, &image, (0.0, 0.0));
                    scene.pop_layer();
                    let job = Offscreen { scene: behind, origin: (0.0, 0.0), ops: ops.clone(), image, backdrop: true };
                    a.jobs.push(job);
                }
            }
            Paint::Rect(r) => {
                if let Some(bg) = &r.background {
                    fill_background(&mut scene, cur, bg, (r.x, r.y, r.width, r.height), r.radius, images);
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
            // The colour fills the padding box, and a blurred hole is cut
            // out of it: a layer clipped to the box, then the hole drawn
            // into it with `DestOut`, which keeps what is there only where
            // the hole is not.
            Paint::InsetShadow { x, y, width, height, radius, hole, hole_radius, blur, color } => {
                let shape = rounded_rect(*x, *y, *width, *height, *radius, 0.0);
                scene.push_layer(Fill::NonZero, Mix::Normal, 1.0, cur, &shape);
                scene.fill(Fill::NonZero, cur, to_color(*color), None, &shape);
                let (hx, hy, hw, hh) = *hole;
                if hw > 0.0 && hh > 0.0 {
                    scene.push_layer(Fill::NonZero, PenikoBlend::new(Mix::Normal, Compose::DestOut), 1.0, cur, &shape);
                    let rect = Rect::new(hx as f64, hy as f64, (hx + hw) as f64, (hy + hh) as f64);
                    let std_dev = (*blur as f64 / 2.0).max(0.0);
                    scene.draw_blurred_rounded_rect(cur, rect, Color::BLACK, *hole_radius as f64, std_dev);
                    scene.pop_layer();
                }
                scene.pop_layer();
            }
            Paint::Background(b) => draw_background(&mut scene, cur, b, images),
            Paint::Border(b) => draw_border(&mut scene, cur, b),
            Paint::Ink(t, color) => {
                text.draw_ink(
                    &mut scene,
                    t.x,
                    t.y,
                    &t.content.text,
                    &text_style(&t.content),
                    to_color(*color),
                    to_align(t.content.align),
                    Some(t.width),
                    cur,
                );
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
                // A field being edited shows all its text, as a browser's
                // does: `text-overflow` cuts it only when it is not.
                let editing = t.content.caret.is_some() || t.content.selection.is_some();
                let whole = t.content.look.as_deref().filter(|l| editing && l.ellipsis).map(|l| TextLook {
                    ellipsis: false,
                    ..l.clone()
                });
                let style = match &whole {
                    Some(look) => TextStyle { look: Some(look), ..text_style(&t.content) },
                    None => text_style(&t.content),
                };
                text.draw(
                    &mut scene,
                    t.x,
                    t.y,
                    &t.content.text,
                    &style,
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
                        text.draw_ink(
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
                    // `caret-color`, else the text's.
                    let color = t.content.look.as_ref().and_then(|l| l.caret_color).unwrap_or(t.content.color);
                    scene.fill(Fill::NonZero, cur, to_color(color), None, &caret);
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
                let path = bez_path(&p.content.commands, p.x as f64, p.y as f64);
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
            // The layer so far is kept only where the mask is: drawn over the
            // whole window, so outside the box, where the mask is transparent,
            // nothing is kept. An alpha mask is a `DestIn` layer; vello's
            // luminance layer is already a mask by lightness, laid onto what
            // is under it the same way.
            Paint::MaskWith { x, y, width, height, image, luminance, view } => {
                let all = Rect::new(0.0, 0.0, view.0 as f64, view.1 as f64);
                if *luminance {
                    scene.push_luminance_mask_layer(Fill::NonZero, 1.0, Affine::IDENTITY, &all);
                } else {
                    let keep = PenikoBlend::new(Mix::Normal, Compose::DestIn);
                    scene.push_layer(Fill::NonZero, keep, 1.0, Affine::IDENTITY, &all);
                }
                fill_background(&mut scene, cur, image, (*x, *y, *width, *height), [0.0; 4], images);
                scene.pop_layer();
            }
            Paint::PushShapeClip { commands, evenodd } => {
                let rule = if *evenodd { Fill::EvenOdd } else { Fill::NonZero };
                scene.push_clip_layer(rule, cur, &bez_path(commands, 0.0, 0.0));
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
            // Fade the subtree, and blend it. The layer covers the viewport so
            // it only fades and blends, never clips: an overflowing child
            // still shows through.
            Paint::PushOpacity {
                alpha,
                blend,
                width,
                height,
            } => {
                let shape = Rect::new(0.0, 0.0, *width as f64, *height as f64);
                scene.push_layer(Fill::NonZero, mix_of(*blend), *alpha, cur, &shape);
            }
            Paint::PopOpacity => scene.pop_layer(),
        }
    }
    scene
}

#[cfg(test)]
mod tests {
    use super::decode_bounded;

    /// A gradient whose first stop is past 0 holds its colour up to it, as
    /// CSS does, rather than vello fading into it from 0.
    #[test]
    fn a_gradient_holds_its_first_colour_up_to_its_first_stop() {
        use rux_layout::{Gradient, GradientKind, Rgba};
        let (red, blue) = (Rgba::new(1.0, 0.0, 0.0, 1.0), Rgba::new(0.0, 0.0, 1.0, 1.0));
        let g = Gradient { kind: GradientKind::Linear { angle: 0.0 }, stops: vec![(red, 0.5), (blue, 0.5)] };
        let stops = super::ramp_stops(&g);
        let at: Vec<f32> = stops.iter().map(|s| s.offset).collect();
        assert_eq!(at, [0.0, 0.5, 0.5, 1.0]);
        assert_eq!(stops[0].color, stops[1].color, "the added stop is the first colour");
        assert_eq!(stops[3].color, stops[2].color, "and the last");
        // A gradient that starts at 0 is handed over as written.
        let g = Gradient { kind: GradientKind::Linear { angle: 0.0 }, stops: vec![(red, 0.0), (blue, 1.0)] };
        assert_eq!(super::ramp_stops(&g).len(), 2);
    }

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
