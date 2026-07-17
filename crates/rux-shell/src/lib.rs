//! Rux runtime shell — milestone M3.
//!
//! Opens a native window (winit), manages the GPU via vello's `RenderContext`,
//! loads a `.rux` document each frame's tree from `rux-runtime`, and paints it
//! (`rux-paint`). A `notify` file watcher wakes the event loop through an
//! `EventLoopProxy` on every save, so edits to the `.rux` file repaint live —
//! the hot-reload path from `docs/04-architecture.md`.

use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use notify::{EventKind, RecursiveMode, Watcher};
use rux_layout::{
    Background, Cursor, FocusItem, FocusKind, FocusRegion, HitRegion, Offset, Paint, PaintRect,
    PaintText, Rgba, ScrollRegion, SelectRegion, TextAlign, TextContent, TextWrap,
};
use rux_runtime::{Document, Focus};
use vello::kurbo::Affine;
use vello::peniko::Color;
use vello::util::{RenderContext, RenderSurface};
use vello::wgpu;
use vello::wgpu::CurrentSurfaceTexture;
use vello::{AaConfig, AaSupport, Renderer, RendererOptions, RenderParams, Scene};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, TouchPhase, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::{CursorIcon, Window, WindowId};

/// Events delivered to the winit loop from outside it.
#[derive(Debug, Clone)]
enum RuxEvent {
    /// The `.rux` file changed on disk.
    Reload,
}

/// Taps closer than this (in physical pixels) between press and release still
/// count as a tap rather than a drag.
const TAP_SLOP: f64 = 6.0;

/// Half the caret blink period: the caret is shown for this long, then hidden
/// for this long. ~530ms matches the platform norm.
const BLINK: Duration = Duration::from_millis(530);

/// Two clicks closer together than this (and within `TAP_SLOP`) are a
/// double-click, which selects a word.
const DOUBLE_CLICK: Duration = Duration::from_millis(500);

/// Rux screen background `#11111b`.
const BG: Color = Color::from_rgb8(0x11, 0x11, 0x1b);

/// Height of one option row in an open `select` dropdown, in logical px.
const DROPDOWN_ROW_H: f32 = 30.0;
/// Gap between the select box and the top of its dropdown panel, in logical px.
const DROPDOWN_GAP: f32 = 4.0;

/// The nth option row of an open dropdown as `(x, y, w, h)` in logical px. Rows
/// stack below the select box (after a small gap). Shared by paint and
/// hit-testing so the dropdown looks and behaves consistently.
fn dropdown_row(sel: &SelectRegion, i: usize) -> (f32, f32, f32, f32) {
    (
        sel.x,
        sel.y + sel.height + DROPDOWN_GAP + i as f32 * DROPDOWN_ROW_H,
        sel.width,
        DROPDOWN_ROW_H,
    )
}

/// Thickness of a scrollbar, in logical px.
const BAR_W: f32 = 8.0;
/// Shortest a thumb may get, however long the content is.
const BAR_MIN_THUMB: f32 = 24.0;
/// One line of scroll travel — the wheel's unit, and the arrow keys'.
const LINE: f32 = 24.0;

/// Which axis a scrollbar (or a drag on one) belongs to.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Axis2 {
    X,
    Y,
}

/// An in-progress drag of a scrollbar thumb.
#[derive(Clone, Copy, Debug)]
struct BarDrag {
    /// The `ScrollRegion::id` being dragged.
    id: usize,
    axis: Axis2,
    /// Pointer position (logical px, on `axis`) when the thumb was grabbed.
    grab: f32,
    /// The region's scroll offset (on `axis`) when the thumb was grabbed.
    start: f32,
}

/// The track a scrollbar runs in, as `(x, y, w, h)` in logical px — an overlay
/// inset along the box's trailing edge. When a box scrolls both ways the tracks
/// stop short of the corner so they never overlap.
fn bar_track(r: &ScrollRegion, axis: Axis2) -> (f32, f32, f32, f32) {
    let corner = if r.max.x > 0.0 && r.max.y > 0.0 { BAR_W } else { 0.0 };
    match axis {
        Axis2::Y => (r.x + r.width - BAR_W, r.y, BAR_W, r.height - corner),
        Axis2::X => (r.x, r.y + r.height - BAR_W, r.width - corner, BAR_W),
    }
}

/// The thumb inside `bar_track`, as `(x, y, w, h)`. `None` when the box doesn't
/// scroll on this axis, so there's nothing to show or grab.
fn bar_thumb(r: &ScrollRegion, offset: Offset, axis: Axis2) -> Option<(f32, f32, f32, f32)> {
    let (max, visible, content) = match axis {
        Axis2::Y => (r.max.y, r.height, r.content_height),
        Axis2::X => (r.max.x, r.width, r.content_width),
    };
    if max <= 0.0 {
        return None;
    }
    let (tx, ty, tw, th) = bar_track(r, axis);
    let track_len = if axis == Axis2::Y { th } else { tw };
    // The thumb is as long a fraction of the track as the box is of the content
    // — the standard proportion — but never so short it can't be grabbed.
    let thumb_len = (track_len * visible / content.max(1.0)).clamp(BAR_MIN_THUMB.min(track_len), track_len);
    let travel = (track_len - thumb_len).max(0.0);
    let pos = match axis {
        Axis2::Y => offset.y,
        Axis2::X => offset.x,
    };
    let along = travel * (pos / max).clamp(0.0, 1.0);
    // The track tuple is (x, y, w, h): its thickness is `tw` on the vertical bar
    // and `th` on the horizontal one — the length is the other component.
    Some(match axis {
        Axis2::Y => (tx, ty + along, tw, thumb_len),
        Axis2::X => (tx + along, ty, thumb_len, th),
    })
}

/// Paint items for every visible scrollbar: a faint track with a lighter thumb,
/// drawn over the content so a scroller's own clip can't eat them.
fn scrollbar_paints(scrolls: &[ScrollRegion], offsets: &[Offset]) -> Vec<Paint> {
    let track_bg = Rgba::new(1.0, 1.0, 1.0, 0.05);
    let thumb_bg = Rgba::new(0.80, 0.84, 0.96, 0.35); // #cdd6f4 at 35%
    let mut out = Vec::new();
    for r in scrolls {
        let offset = offsets.get(r.id).copied().unwrap_or_default();
        for axis in [Axis2::Y, Axis2::X] {
            let Some((thx, thy, thw, thh)) = bar_thumb(r, offset, axis) else {
                continue;
            };
            let (tx, ty, tw, th) = bar_track(r, axis);
            out.push(Paint::Rect(PaintRect {
                x: tx,
                y: ty,
                width: tw,
                height: th,
                background: Some(Background::Color(track_bg)),
                radius: [BAR_W / 2.0; 4],
                border_width: 0.0,
                border_color: None,
            }));
            out.push(Paint::Rect(PaintRect {
                x: thx,
                y: thy,
                width: thw,
                height: thh,
                background: Some(Background::Color(thumb_bg)),
                radius: [BAR_W / 2.0; 4],
                border_width: 0.0,
                border_color: None,
            }));
        }
    }
    out
}

/// A 2px focus ring just outside the focused element's box.
fn focus_ring(item: &FocusItem) -> Vec<Paint> {
    vec![Paint::Rect(PaintRect {
        x: item.x - 2.0,
        y: item.y - 2.0,
        width: item.width + 4.0,
        height: item.height + 4.0,
        background: None,
        radius: [7.0; 4],
        border_width: 2.0,
        border_color: Some(Rgba::new(0.54, 0.71, 0.98, 1.0)), // #89b4fa
    })]
}

/// Paint items for an open dropdown: a single floating panel with a shadow, the
/// selected value picked out as a pill, and thin separators between options.
fn dropdown_paints(sel: &SelectRegion, value: &str) -> Vec<Paint> {
    let panel_bg = Rgba::new(0.19, 0.20, 0.27, 1.0); // #313244
    let border = Rgba::new(0.27, 0.28, 0.35, 1.0); // #45475a
    let selected = Rgba::new(0.35, 0.36, 0.44, 1.0); // #585b70
    let ink = Rgba::new(0.80, 0.84, 0.96, 1.0); // #cdd6f4

    let (px, py, pw, _) = dropdown_row(sel, 0);
    let ph = sel.options.len() as f32 * DROPDOWN_ROW_H;

    let mut out = Vec::with_capacity(sel.options.len() * 2 + 2);
    // A soft shadow so the panel reads as floating above the page.
    out.push(Paint::Shadow {
        x: px,
        y: py + 3.0,
        width: pw,
        height: ph,
        radius: 8.0,
        blur: 16.0,
        color: Rgba::new(0.0, 0.0, 0.0, 0.45),
    });
    // The panel itself: one rounded rect behind all the rows.
    out.push(Paint::Rect(PaintRect {
        x: px,
        y: py,
        width: pw,
        height: ph,
        background: Some(Background::Color(panel_bg)),
        radius: [8.0; 4],
        border_width: 1.0,
        border_color: Some(border),
    }));

    for (i, option) in sel.options.iter().enumerate() {
        let y = py + i as f32 * DROPDOWN_ROW_H;
        if option == value {
            // A rounded pill marks the current choice, inset from the panel edge.
            out.push(Paint::Rect(PaintRect {
                x: px + 4.0,
                y: y + 3.0,
                width: pw - 8.0,
                height: DROPDOWN_ROW_H - 6.0,
                background: Some(Background::Color(selected)),
                radius: [5.0; 4],
                border_width: 0.0,
                border_color: None,
            }));
        } else if i > 0 {
            // A hairline separator between unselected rows.
            out.push(Paint::Rect(PaintRect {
                x: px + 10.0,
                y,
                width: pw - 20.0,
                height: 1.0,
                background: Some(Background::Color(border)),
                radius: [0.0; 4],
                border_width: 0.0,
                border_color: None,
            }));
        }
        out.push(Paint::Text(PaintText {
            x: px + 12.0,
            y: y + (DROPDOWN_ROW_H - 15.0) / 2.0,
            width: pw - 24.0,
            height: DROPDOWN_ROW_H,
            content: TextContent {
                text: option.clone(),
                font_size: 15.0,
                weight: 400,
                color: ink,
                align: TextAlign::Start,
                wrap: TextWrap::Normal,
                font_family: None,
                letter_spacing: None,
                word_spacing: None,
                line_height: None,
                italic: false,
                underline: false,
                strikethrough: false,
                nowrap: true,
                caret: None,
                selection: None,
            },
        }));
    }
    out
}

/// Load a `.rux` document. On failure, log the diagnostic and fall back to an
/// empty screen so the window still opens (stand-in for the dev overlay).
fn load_document(path: &PathBuf) -> Document {
    match Document::load(path) {
        Ok(doc) => doc,
        Err(err) => {
            eprintln!("failed to load {}: {err}", path.display());
            Document::from_source("<template><screen></screen></template>")
                .expect("empty document")
        }
    }
}

/// Per-window render state.
struct RenderState {
    window: Arc<Window>,
    surface: RenderSurface<'static>,
    renderer: Renderer,
    scene: Scene,
}

/// The application: owns the vello render context, the document, the text
/// engine, input state, and (once resumed) one window.
struct App {
    context: RenderContext,
    state: Option<RenderState>,
    path: PathBuf,
    document: Document,
    text: rux_text::TextEngine,
    images: rux_paint::ImageCache,
    /// Hit regions from the most recent layout, for tap dispatch.
    hits: Vec<HitRegion>,
    /// Focusable input regions from the most recent layout.
    focuses: Vec<FocusRegion>,
    /// `type="select"` regions from the most recent layout.
    selects: Vec<SelectRegion>,
    /// Keyboard-focusable elements in Tab order, from the most recent layout.
    focusables: Vec<FocusItem>,
    /// Index into `focusables` of the keyboard-focused element, if any.
    focus_index: Option<usize>,
    /// Whether Shift is held (Shift+Tab reverse traversal; Shift+arrows extend a
    /// selection; Shift+wheel scrolls sideways).
    shift_held: bool,
    /// Whether Ctrl is held (Ctrl+A/C/X/V).
    ctrl_held: bool,
    /// Scrollable regions from the most recent layout.
    scrolls: Vec<ScrollRegion>,
    /// Scroll offset per scrollable box, in tree order. Survives the rebuild
    /// that follows every state change, so a list doesn't jump back to the top
    /// when you tap something in it.
    offsets: Vec<Offset>,
    /// The scrollbar thumb being dragged, if any.
    bar_drag: Option<BarDrag>,
    /// Where the finger last was during a touch drag, in logical px.
    touch: Option<(f32, f32)>,
    /// The `r-model` of the currently focused input, if any.
    focused: Option<String>,
    /// Whether the focused input is a `type="textarea"` (Enter → newline).
    focused_multiline: bool,
    /// The `r-model` of the currently open `select` dropdown, if any. Survives
    /// the rebuild after a state change, like scroll offsets.
    open_select: Option<String>,
    /// Caret position in the focused input, as a byte index into its value.
    caret: usize,
    /// Where the current selection started, as a byte index. Equal to `caret`
    /// when nothing is selected — the selection is the range between them.
    anchor: usize,
    /// Whether the pointer is selecting text by dragging inside an input.
    text_drag: bool,
    /// When and where the last click landed, for double-click word-select.
    last_click: Option<(Instant, f64, f64)>,
    /// The system clipboard. `None` if the platform wouldn't give us one — the
    /// app still runs, copy/paste just does nothing.
    clipboard: Option<arboard::Clipboard>,
    /// Whether the caret is in the visible half of its blink cycle.
    caret_visible: bool,
    /// When the caret next toggles. `None` when no input is focused, so an idle
    /// window stays fully event-driven with no timer.
    blink_deadline: Option<Instant>,
    /// Current pointer position (physical pixels).
    pointer: (f64, f64),
    /// Where the left button was pressed, if it is currently down.
    press: Option<(f64, f64)>,
    /// The cursor icon currently set on the window, so a mouse-move only calls
    /// `set_cursor` when the shape actually changes.
    cursor: CursorIcon,
}

impl App {
    fn new(path: PathBuf) -> Self {
        let document = load_document(&path);
        Self {
            context: RenderContext::new(),
            state: None,
            path,
            document,
            text: rux_text::TextEngine::new(),
            images: rux_paint::ImageCache::new(),
            hits: Vec::new(),
            focuses: Vec::new(),
            selects: Vec::new(),
            focusables: Vec::new(),
            focus_index: None,
            shift_held: false,
            ctrl_held: false,
            scrolls: Vec::new(),
            offsets: Vec::new(),
            bar_drag: None,
            touch: None,
            focused: None,
            focused_multiline: false,
            open_select: None,
            caret: 0,
            anchor: 0,
            text_drag: false,
            last_click: None,
            clipboard: arboard::Clipboard::new()
                .map_err(|e| eprintln!("rux: no clipboard ({e}) — copy/paste disabled"))
                .ok(),
            caret_visible: true,
            blink_deadline: None,
            pointer: (0.0, 0.0),
            press: None,
            cursor: CursorIcon::Default,
        }
    }

    /// Re-load the document after a file change. On a parse/load error we keep
    /// the last good document and log the diagnostic, rather than blanking the
    /// window (a first step toward the dev overlay).
    fn reload(&mut self) {
        match Document::load(&self.path) {
            Ok(doc) => {
                self.document = doc;
                eprintln!("reloaded {}", self.path.display());
            }
            Err(err) => eprintln!("reload failed for {}: {err}", self.path.display()),
        }
    }

    /// The window's DPI scale. Layout and hit regions are in logical pixels; the
    /// surface is physical, so the scene is scaled up at paint time.
    fn scale(&self) -> f64 {
        self.state
            .as_ref()
            .map(|s| s.window.scale_factor())
            .unwrap_or(1.0)
    }

    /// The pointer in logical pixels (layout, hit regions and scrollbars all live
    /// in logical space; winit reports physical).
    fn logical(&self, p: (f64, f64)) -> (f32, f32) {
        let scale = self.scale();
        ((p.0 / scale) as f32, (p.1 / scale) as f32)
    }

    /// Scroll the innermost scrollable box under the pointer by `(dx, dy)`
    /// logical pixels. Nothing under the pointer scrolls (or it's already at the
    /// end) → nothing happens, and no repaint is queued.
    fn scroll_at(&mut self, pointer: (f64, f64), dx: f32, dy: f32) {
        let (px, py) = self.logical(pointer);
        // Innermost wins: scrollers are pushed parent-first, so search backwards.
        let Some(region) = self
            .scrolls
            .iter()
            .rev()
            .find(|s| s.contains(px, py) && s.scrollable())
        else {
            return;
        };
        let (id, max) = (region.id, region.max);
        self.scroll_to(
            id,
            Offset {
                x: self.offsets[id].x + dx,
                y: self.offsets[id].y + dy,
            }
            .clamp_to(max),
        );
    }

    /// Move scroller `id` to `next`, repainting only if it actually moved.
    fn scroll_to(&mut self, id: usize, next: Offset) {
        if self.offsets.get(id) != Some(&next) {
            if let Some(slot) = self.offsets.get_mut(id) {
                *slot = next;
                self.request_redraw();
            }
        }
    }

    /// Start a scrollbar drag if the press landed on a thumb. Returns whether it
    /// did — in which case the press is the bar's, not a tap's.
    fn press_scrollbar(&mut self, pointer: (f64, f64)) -> bool {
        let (px, py) = self.logical(pointer);
        // Topmost (innermost) bar wins, as with the wheel.
        for r in self.scrolls.iter().rev() {
            let offset = self.offsets.get(r.id).copied().unwrap_or_default();
            for axis in [Axis2::Y, Axis2::X] {
                let Some((tx, ty, tw, th)) = bar_thumb(r, offset, axis) else {
                    continue;
                };
                if px >= tx && px <= tx + tw && py >= ty && py <= ty + th {
                    self.bar_drag = Some(BarDrag {
                        id: r.id,
                        axis,
                        grab: if axis == Axis2::Y { py } else { px },
                        start: if axis == Axis2::Y { offset.y } else { offset.x },
                    });
                    return true;
                }
            }
        }
        false
    }

    /// Follow a scrollbar thumb drag: the pointer's travel down the *track* maps
    /// to the content's travel through its full scroll range.
    fn drag_scrollbar(&mut self, pointer: (f64, f64)) {
        let Some(drag) = self.bar_drag else { return };
        let Some(r) = self.scrolls.iter().find(|s| s.id == drag.id).cloned() else {
            return;
        };
        let Some((_, _, tw, th)) = bar_thumb(&r, self.offsets[drag.id], drag.axis) else {
            return;
        };
        let (_, _, track_w, track_h) = bar_track(&r, drag.axis);
        let (px, py) = self.logical(pointer);
        let (pos, track_len, thumb_len, max) = match drag.axis {
            Axis2::Y => (py, track_h, th, r.max.y),
            Axis2::X => (px, track_w, tw, r.max.x),
        };
        let travel = (track_len - thumb_len).max(0.0);
        if travel <= 0.0 {
            return;
        }
        let moved = drag.start + (pos - drag.grab) * max / travel;
        let next = match drag.axis {
            Axis2::Y => Offset { x: self.offsets[drag.id].x, y: moved },
            Axis2::X => Offset { x: moved, y: self.offsets[drag.id].y },
        };
        self.scroll_to(drag.id, next.clamp_to(r.max));
    }

    /// Scroll the box under the pointer with the keyboard. Only reached when no
    /// input has focus, so it can't steal a caret key. Returns whether it acted.
    fn scroll_key(&mut self, key: &Key) -> bool {
        let (px, py) = self.logical(self.pointer);
        let Some(r) = self
            .scrolls
            .iter()
            .rev()
            .find(|s| s.contains(px, py) && s.scrollable())
            .cloned()
        else {
            return false;
        };
        // A page is just short of the box, so a landmark stays on screen.
        let page = (r.height * 0.9).max(LINE);
        let here = self.offsets[r.id];
        let next = match key {
            Key::Named(NamedKey::ArrowDown) => Offset { y: here.y + LINE, ..here },
            Key::Named(NamedKey::ArrowUp) => Offset { y: here.y - LINE, ..here },
            Key::Named(NamedKey::ArrowRight) => Offset { x: here.x + LINE, ..here },
            Key::Named(NamedKey::ArrowLeft) => Offset { x: here.x - LINE, ..here },
            Key::Named(NamedKey::PageDown) => Offset { y: here.y + page, ..here },
            Key::Named(NamedKey::PageUp) => Offset { y: here.y - page, ..here },
            Key::Named(NamedKey::Home) => Offset { y: 0.0, ..here },
            Key::Named(NamedKey::End) => Offset { y: r.max.y, ..here },
            _ => return false,
        };
        self.scroll_to(r.id, next.clamp_to(r.max));
        true
    }

    /// Bring the keyboard-focused element into view: if it sits outside a
    /// scroller it belongs to, nudge that scroller just far enough. Tabbing to
    /// something below the fold is otherwise a focus ring you can't see.
    ///
    /// Geometry here is the *painted* (already-shifted) position from the last
    /// layout, so the adjustment is a plain delta; the next layout re-clamps it.
    fn scroll_focus_into_view(&mut self) {
        let Some(item) = self.focus_index.and_then(|i| self.focusables.get(i)).cloned() else {
            return;
        };
        // Outermost first: scrolling an ancestor moves the box inside it, so the
        // inner scroller's own correction must be computed after.
        for r in self.scrolls.clone() {
            if !r.scrollable() {
                continue;
            }
            // Only a scroller the item is horizontally within can own it — a
            // cheap stand-in for a real ancestor test (we don't carry parentage).
            if item.x + item.width < r.x || item.x > r.x + r.width {
                continue;
            }
            let here = self.offsets[r.id];
            let mut next = here;
            if item.y < r.y {
                next.y = here.y - (r.y - item.y);
            } else if item.y + item.height > r.y + r.height {
                next.y = here.y + (item.y + item.height - (r.y + r.height));
            }
            if item.x < r.x {
                next.x = here.x - (r.x - item.x);
            } else if item.x + item.width > r.x + r.width {
                next.x = here.x + (item.x + item.width - (r.x + r.width));
            }
            self.scroll_to(r.id, next.clamp_to(r.max));
        }
    }

    /// The byte index in `region`'s text nearest a point, in logical px. An empty
    /// input is showing its placeholder, not a value, so its caret belongs at 0.
    fn index_in(&mut self, region: &FocusRegion, px: f32, py: f32) -> usize {
        let value = self.document.engine_mut().get_string(&region.model);
        match &region.text {
            Some(t) if !value.is_empty() => self.text.index_at_point(
                &value,
                &rux_paint::text_style(&t.content),
                Some(t.width),
                px - t.x,
                py - t.y,
            ),
            _ => 0,
        }
    }

    /// A press inside an input starts a text selection: it drops the caret (and
    /// the anchor) where you clicked, and a drag from there extends it. A second
    /// click in the same spot selects the word instead.
    ///
    /// Returns whether the press was ours — if so it is *not* also dispatched as a
    /// tap on release, since focusing already happened here.
    fn press_text(&mut self, pointer: (f64, f64)) -> bool {
        // An open dropdown floats over everything and gets first refusal.
        if self.open_select.is_some() {
            return false;
        }
        let (fx, fy) = self.logical(pointer);
        let Some(region) = self.focuses.iter().rev().find(|f| f.contains(fx, fy)).cloned() else {
            return false;
        };

        // A tap also moves keyboard focus, so Tab continues from what you clicked.
        self.focus_index = self.focusables.iter().rposition(|f| f.contains(fx, fy));
        self.focused_multiline = region.multiline;

        let double = self
            .last_click
            .is_some_and(|(at, x, y)| {
                at.elapsed() < DOUBLE_CLICK && (pointer.0 - x).hypot(pointer.1 - y) <= TAP_SLOP
            });
        self.last_click = Some((Instant::now(), pointer.0, pointer.1));

        if double {
            // Double-click selects the word under the pointer.
            let value = self.document.engine_mut().get_string(&region.model);
            if let (Some(t), false) = (&region.text, value.is_empty()) {
                let (start, end) = self.text.word_at_point(
                    &value,
                    &rux_paint::text_style(&t.content),
                    Some(t.width),
                    fx - t.x,
                    fy - t.y,
                );
                self.set_focus_range(Some(Focus { model: region.model, caret: end, anchor: start }));
                return true;
            }
        }

        let caret = self.index_in(&region, fx, fy);
        self.text_drag = true;
        self.set_focus(Some((region.model, caret)));
        true
    }

    /// Extend the selection to the pointer while dragging inside an input: the
    /// anchor stays where the press landed, the caret follows the pointer.
    fn drag_text(&mut self, pointer: (f64, f64)) {
        let Some(model) = self.focused.clone() else { return };
        let Some(region) = self.focuses.iter().find(|f| f.model == model).cloned() else {
            return;
        };
        let (fx, fy) = self.logical(pointer);
        let caret = self.index_in(&region, fx, fy);
        if caret != self.caret {
            let anchor = self.anchor;
            self.set_focus_range(Some(Focus { model, caret, anchor }));
        }
    }

    /// Set the window's cursor from whatever tappable region is under the
    /// pointer (topmost wins, as with tap dispatch). Only touches the window when
    /// the shape changes, so it's cheap to call on every mouse move.
    fn update_cursor(&mut self) {
        let scale = self.scale();
        let (px, py) = ((self.pointer.0 / scale) as f32, (self.pointer.1 / scale) as f32);
        let want = self
            .hits
            .iter()
            .rev()
            .find(|h| h.contains(px, py))
            .map(|h| match h.cursor {
                Cursor::Pointer => CursorIcon::Pointer,
                Cursor::Default => CursorIcon::Default,
            })
            .unwrap_or(CursorIcon::Default);
        if want != self.cursor {
            self.cursor = want;
            if let Some(state) = &self.state {
                state.window.set_cursor(want);
            }
        }
    }

    /// Handle a completed tap at `(px, py)`, in physical pixels: focus an input
    /// if one is under the pointer, otherwise run the topmost `@tap` handler.
    fn dispatch_tap(&mut self, px: f64, py: f64) {
        let scale = self.scale();
        let (px, py) = (px / scale, py / scale);
        let (fx, fy) = (px as f32, py as f32);

        // An open dropdown is on top of everything, so it intercepts taps first:
        // a tap on an option selects it; any other tap just closes the dropdown.
        if let Some(model) = self.open_select.take() {
            if let Some(sel) = self.selects.iter().find(|s| s.model == model).cloned() {
                for (i, option) in sel.options.iter().enumerate() {
                    let (rx, ry, rw, rh) = dropdown_row(&sel, i);
                    if fx >= rx && fx <= rx + rw && fy >= ry && fy <= ry + rh {
                        self.document.engine_mut().set_string(&model, option);
                        self.document.rebuild();
                        self.request_redraw();
                        return;
                    }
                }
            }
            // Closed by taking `open_select`; repaint without the dropdown.
            self.request_redraw();
            return;
        }

        // A tap also moves keyboard focus, so Tab continues from what you clicked
        // (topmost focusable under the pointer, or nothing on empty space).
        self.focus_index = self.focusables.iter().rposition(|f| f.contains(fx, fy));

        // A tap on a closed select opens its dropdown.
        if let Some(sel) = self.selects.iter().find(|s| s.contains(fx, fy)) {
            self.open_select = Some(sel.model.clone());
            self.set_focus(None);
            self.request_redraw();
            return;
        }

        // Inputs are handled at press time (`press_text`), which is where a
        // selection drag has to start — so by here the tap is on something else.
        // Tapping elsewhere drops focus.
        self.set_focus(None);

        // Topmost hit region wins (later in list = drawn on top).
        let handler = self
            .hits
            .iter()
            .rev()
            .find(|h| h.contains(px as f32, py as f32))
            .map(|h| h.on_tap.clone());

        if let Some(src) = handler {
            if self.document.engine_mut().run_handler(&src) {
                self.document.rebuild();
                self.request_redraw();
            }
        }
    }

    /// Apply a key to the focused input's bound signal, then rebuild + repaint.
    ///
    /// Indices are byte offsets into the value, always on a char boundary (we
    /// only ever step by whole characters, and parley returns boundaries), so
    /// slicing is safe.
    ///
    /// Selection rules, which are the platform's everywhere: **Shift** + a
    /// movement extends (the anchor stays put); a movement without it collapses;
    /// and anything that inserts or deletes replaces the selection first.
    fn edit_focused(&mut self, key: &Key) {
        let Some(model) = self.focused.clone() else {
            return;
        };
        // Ctrl chords are select-all / copy / cut / paste, not text.
        if self.ctrl_held && self.text_shortcut(key, &model) {
            return;
        }

        let mut value = self.document.engine_mut().get_string(&model);
        let caret = self.caret.min(value.len());
        let (sel_start, sel_end) = {
            let (s, e) = self.selection();
            (s.min(value.len()), e.min(value.len()))
        };
        let has_selection = sel_start != sel_end;
        let extend = self.shift_held;

        // How far the previous / next character is, in bytes.
        let prev = value[..caret].chars().next_back().map(char::len_utf8);
        let next = value[caret..].chars().next().map(char::len_utf8);

        let mut edited = false;
        let mut moved = false;
        let mut new_caret = caret;
        // Replace whatever is selected with `text`, leaving the caret after it.
        let replace_selection = |value: &mut String, text: &str| {
            value.replace_range(sel_start..sel_end, text);
            sel_start + text.len()
        };

        match key {
            Key::Named(NamedKey::Backspace) => {
                if has_selection {
                    new_caret = replace_selection(&mut value, "");
                    edited = true;
                } else if let Some(len) = prev {
                    value.replace_range(caret - len..caret, "");
                    new_caret = caret - len;
                    edited = true;
                }
            }
            Key::Named(NamedKey::Delete) => {
                if has_selection {
                    new_caret = replace_selection(&mut value, "");
                    edited = true;
                } else if let Some(len) = next {
                    value.replace_range(caret..caret + len, "");
                    edited = true;
                }
            }
            // A plain arrow with a selection collapses to its near edge rather
            // than moving — that's what every text field does.
            Key::Named(NamedKey::ArrowLeft) => {
                if has_selection && !extend {
                    new_caret = sel_start;
                    moved = true;
                } else if let Some(len) = prev {
                    new_caret = caret - len;
                    moved = true;
                }
            }
            Key::Named(NamedKey::ArrowRight) => {
                if has_selection && !extend {
                    new_caret = sel_end;
                    moved = true;
                } else if let Some(len) = next {
                    new_caret = caret + len;
                    moved = true;
                }
            }
            // Up/Down move the caret between lines of a textarea: find the byte
            // index at the same x on the line above/below the current caret.
            Key::Named(NamedKey::ArrowUp | NamedKey::ArrowDown) if self.focused_multiline => {
                if let Some(t) = self
                    .focuses
                    .iter()
                    .find(|f| f.model == model)
                    .and_then(|f| f.text.clone())
                {
                    let style = rux_paint::text_style(&t.content);
                    let (cx, cy, ch) = self.text.caret_geometry(&value, &style, Some(t.width), caret);
                    let dir = if matches!(key, Key::Named(NamedKey::ArrowUp)) { -1.0 } else { 1.0 };
                    let target_y = cy + ch / 2.0 + dir * ch;
                    new_caret = self.text.index_at_point(&value, &style, Some(t.width), cx, target_y);
                    moved = new_caret != caret;
                }
            }
            Key::Named(NamedKey::Home) => {
                new_caret = 0;
                moved = true;
            }
            Key::Named(NamedKey::End) => {
                new_caret = value.len();
                moved = true;
            }
            Key::Named(NamedKey::Escape) => {
                self.set_focus(None);
                return;
            }
            Key::Named(NamedKey::Space) => {
                new_caret = replace_selection(&mut value, " ");
                edited = true;
            }
            // Enter inserts a newline in a textarea; single-line inputs ignore it.
            Key::Named(NamedKey::Enter) if self.focused_multiline => {
                new_caret = replace_selection(&mut value, "\n");
                edited = true;
            }
            Key::Character(s) => {
                let typed: String = s.chars().filter(|c| !c.is_control()).collect();
                if !typed.is_empty() {
                    new_caret = replace_selection(&mut value, &typed);
                    edited = true;
                }
            }
            _ => {}
        }

        if edited || moved {
            // Shift+movement keeps the anchor, extending the selection; anything
            // else collapses it to the caret.
            let new_anchor = if moved && extend { self.anchor } else { new_caret };
            self.scroll_caret_into_view(&model, &value, new_caret);
            if edited {
                self.document.engine_mut().set_string(&model, &value);
            }
            self.set_focus_range(Some(Focus {
                model,
                caret: new_caret,
                anchor: new_anchor,
            }));
            if edited {
                self.document.rebuild();
            }
        }
    }

    /// Ctrl chords inside a focused input: select all, copy, cut, paste. Returns
    /// whether the key was one of them (so it isn't also typed as a character —
    /// Ctrl+V arrives as `Key::Character("v")`).
    fn text_shortcut(&mut self, key: &Key, model: &str) -> bool {
        let Key::Character(s) = key else { return false };
        let value = self.document.engine_mut().get_string(model);
        match s.to_lowercase().as_str() {
            "a" => {
                self.set_focus_range(Some(Focus {
                    model: model.to_string(),
                    caret: value.len(),
                    anchor: 0,
                }));
            }
            "c" => {
                if let Some(text) = self.selected_text() {
                    self.clipboard_write(&text);
                }
            }
            "x" => {
                if let Some(text) = self.selected_text() {
                    self.clipboard_write(&text);
                    let (start, end) = self.selection();
                    let mut value = value;
                    value.replace_range(start.min(value.len())..end.min(value.len()), "");
                    self.document.engine_mut().set_string(model, &value);
                    self.set_focus_range(Some(Focus::at(model, start)));
                    self.document.rebuild();
                }
            }
            "v" => {
                let Some(pasted) = self.clipboard_read() else {
                    return true;
                };
                // A single-line input takes the first line only — pasting a block
                // of text into a one-line field shouldn't smuggle newlines in.
                let pasted = if self.focused_multiline {
                    pasted.replace("\r\n", "\n")
                } else {
                    pasted.lines().next().unwrap_or("").to_string()
                };
                let (start, end) = self.selection();
                let mut value = value;
                let (start, end) = (start.min(value.len()), end.min(value.len()));
                value.replace_range(start..end, &pasted);
                let caret = start + pasted.len();
                self.document.engine_mut().set_string(model, &value);
                self.scroll_caret_into_view(model, &value, caret);
                self.set_focus_range(Some(Focus::at(model, caret)));
                self.document.rebuild();
            }
            _ => return false,
        }
        true
    }

    /// Keep the caret visible in a scrolling textarea: adjust its scroll offset so
    /// the caret line sits inside the box. No-op for non-scrolling inputs.
    fn scroll_caret_into_view(&mut self, model: &str, value: &str, caret: usize) {
        let Some(region) = self.focuses.iter().find(|f| f.model == model).cloned() else {
            return;
        };
        let (Some(sid), Some(t)) = (region.scroll_id, &region.text) else {
            return;
        };
        let style = rux_paint::text_style(&t.content);
        let (_, cy, ch) = self.text.caret_geometry(value, &style, Some(t.width), caret);
        let visible = region.height;
        let mut off = self.offsets.get(sid).copied().unwrap_or_default();
        if cy < off.y {
            off.y = cy;
        } else if cy + ch > off.y + visible {
            off.y = cy + ch - visible;
        }
        // The next layout re-clamps this to the content's real max offset.
        if let Some(slot) = self.offsets.get_mut(sid) {
            slot.y = off.y.max(0.0);
        }
    }

    /// Route a key press. Tab always moves keyboard focus; otherwise a focused
    /// text input edits, and a focused button/checkbox/radio/select activates on
    /// Space/Enter.
    fn on_key(&mut self, key: &Key) {
        if let Key::Named(NamedKey::Tab) = key {
            self.move_focus(self.shift_held);
            return;
        }
        if self.focused.is_some() {
            self.edit_focused(key);
            return;
        }
        if let Some(idx) = self.focus_index {
            match key {
                Key::Named(NamedKey::Space | NamedKey::Enter) => {
                    self.activate_focused(idx);
                    return;
                }
                Key::Named(NamedKey::Escape) => {
                    self.focus_index = None;
                    self.request_redraw();
                    return;
                }
                _ => {}
            }
        }
        // Nothing focused wants this key: let it scroll the box under the pointer.
        self.scroll_key(key);
    }

    /// Move keyboard focus to the next (or previous) focusable, wrapping around.
    fn move_focus(&mut self, backward: bool) {
        let n = self.focusables.len();
        if n == 0 {
            return;
        }
        let next = match self.focus_index {
            Some(i) if backward => (i + n - 1) % n,
            Some(i) => (i + 1) % n,
            None if backward => n - 1,
            None => 0,
        };
        self.set_keyboard_focus(Some(next));
    }

    /// Point keyboard focus at `index`. A text input also gets caret editing (with
    /// the caret at the end); anything else just gets the focus ring.
    fn set_keyboard_focus(&mut self, index: Option<usize>) {
        self.focus_index = index;
        match index.and_then(|i| self.focusables.get(i)).map(|f| f.kind.clone()) {
            Some(FocusKind::Text { model, multiline, .. }) => {
                let caret = self.document.engine_mut().get_string(&model).len();
                self.focused_multiline = multiline;
                self.set_focus(Some((model, caret)));
            }
            _ => self.set_focus(None),
        }
        // Tabbing to something below the fold must bring it into view.
        self.scroll_focus_into_view();
        self.request_redraw();
    }

    /// Activate the focused element by keyboard: run a button/toggle's handler, or
    /// open a select's dropdown.
    fn activate_focused(&mut self, index: usize) {
        match self.focusables.get(index).map(|f| f.kind.clone()) {
            Some(FocusKind::Activate { on_tap }) => {
                if self.document.engine_mut().run_handler(&on_tap) {
                    self.document.rebuild();
                }
                self.request_redraw();
            }
            Some(FocusKind::Select { model, .. }) => {
                self.open_select = Some(model);
                self.request_redraw();
            }
            _ => {}
        }
    }

    /// Focus an input (or clear focus) and tell the document, so the caret and
    /// selection paint. Collapses the selection to the caret.
    fn set_focus(&mut self, focus: Option<(String, usize)>) {
        match focus {
            Some((model, caret)) => self.set_focus_range(Some(Focus::at(model, caret))),
            None => self.set_focus_range(None),
        }
    }

    /// The full-fidelity focus setter: caret *and* selection anchor.
    fn set_focus_range(&mut self, focus: Option<Focus>) {
        self.focused = focus.as_ref().map(|f| f.model.clone());
        self.caret = focus.as_ref().map(|f| f.caret).unwrap_or(0);
        self.anchor = focus.as_ref().map(|f| f.anchor).unwrap_or(0);
        self.document.set_focus(focus);
        self.reset_blink();
        self.request_redraw();
    }

    /// The focused input's selected byte range, low to high. Empty when there's
    /// no selection (`start == end`).
    fn selection(&self) -> (usize, usize) {
        (self.caret.min(self.anchor), self.caret.max(self.anchor))
    }

    /// The focused input's selected text, if any.
    fn selected_text(&mut self) -> Option<String> {
        let model = self.focused.clone()?;
        let (start, end) = self.selection();
        if start == end {
            return None;
        }
        let value = self.document.engine_mut().get_string(&model);
        value.get(start.min(value.len())..end.min(value.len())).map(str::to_string)
    }

    /// Put `text` on the system clipboard.
    fn clipboard_write(&mut self, text: &str) {
        if let Some(cb) = self.clipboard.as_mut() {
            if let Err(e) = cb.set_text(text.to_string()) {
                eprintln!("rux: clipboard copy failed: {e}");
            }
        }
    }

    /// Read the system clipboard. `None` when it's empty, holds non-text, or
    /// there's no clipboard at all.
    fn clipboard_read(&mut self) -> Option<String> {
        self.clipboard.as_mut()?.get_text().ok()
    }

    /// Show the caret solid and (re)start the blink cycle. Called on focus and on
    /// every edit, so the caret is steady while you type and only blinks at rest.
    /// Clearing focus stops the timer entirely — an idle window stays event-driven.
    fn reset_blink(&mut self) {
        self.caret_visible = true;
        self.blink_deadline = self.focused.is_some().then(|| Instant::now() + BLINK);
    }

    fn request_redraw(&self) {
        if let Some(state) = self.state.as_ref() {
            state.window.request_redraw();
        }
    }

    fn render(&mut self) {
        let caret_visible = self.caret_visible;
        // Split borrows so the text engine (used both to measure during layout
        // and to draw during paint) doesn't conflict with the render state.
        let App {
            context,
            state,
            document,
            text,
            images,
            hits,
            focuses,
            selects,
            focusables,
            focus_index,
            open_select,
            scrolls,
            offsets,
            ..
        } = self;
        let Some(state) = state.as_mut() else {
            return;
        };
        let width = state.surface.config.width;
        let height = state.surface.config.height;

        // Lay out in *logical* pixels so a `16px` font is the same physical size
        // on every display, then scale the scene up to the physical surface.
        // Without this, everything renders half-size on a 2x screen.
        let scale = state.window.scale_factor();
        let logical = (width as f64 / scale, height as f64 / scale);

        // Layout (text sized via the engine's measure), then paint. Cache the
        // hit regions for tap dispatch.
        let layout = {
            let mut measure = |tc: &rux_layout::TextContent, mw: Option<f32>| {
                text.measure(&tc.text, &rux_paint::text_style(tc), mw)
            };
            rux_layout::layout_scrolled(
                &document.root,
                logical.0 as f32,
                logical.1 as f32,
                offsets,
                &mut measure,
            )
        };
        // Keep offsets in step with the scrollers the new layout actually has, and
        // re-clamp them (the content may have shrunk under us). `collect` clamps
        // the shift it applies the same way, so doing this before the scrollbars
        // are drawn is what keeps a thumb where its content actually is.
        offsets.resize(layout.scrolls.len(), Offset::default());
        for region in &layout.scrolls {
            offsets[region.id] = offsets[region.id].clamp_to(region.max);
        }

        let content = rux_paint::build_scene(&layout.paints, text, images, caret_visible);
        state.scene.reset();
        state
            .scene
            .append(&content, Some(Affine::scale(scale)));

        // Scrollbars go over the content: they're an overlay on the box's own
        // trailing edge, and a scroller clips its children, so they can't be
        // painted as part of the subtree.
        let bars = scrollbar_paints(&layout.scrolls, offsets);
        if !bars.is_empty() {
            let scene = rux_paint::build_scene(&bars, text, images, false);
            state.scene.append(&scene, Some(Affine::scale(scale)));
        }

        // A keyboard focus ring, drawn over the content (but under a dropdown).
        if let Some(item) = focus_index.and_then(|i| layout.focusables.get(i)) {
            let ring = rux_paint::build_scene(&focus_ring(item), text, images, false);
            state.scene.append(&ring, Some(Affine::scale(scale)));
        }

        // An open `select` draws its dropdown on top of everything else.
        if let Some(model) = open_select.clone() {
            if let Some(sel) = layout.selects.iter().find(|s| s.model == model) {
                let value = document.engine_mut().get_string(&model);
                let overlay = dropdown_paints(sel, &value);
                let scene = rux_paint::build_scene(&overlay, text, images, false);
                state.scene.append(&scene, Some(Affine::scale(scale)));
            }
        }

        *hits = layout.hits;
        *focuses = layout.focuses;
        *selects = layout.selects;
        // Keep the focus index in range if the new layout has fewer focusables.
        if focus_index.map(|i| i >= layout.focusables.len()).unwrap_or(false) {
            *focus_index = None;
        }
        *focusables = layout.focusables;
        *scrolls = layout.scrolls;

        let device_handle = &context.devices[state.surface.dev_id];
        // wgpu 29 reports acquisition as a status enum. A timeout/occluded frame
        // is normal (minimized window, compositor hiccup) — skip it and repaint
        // on the next event rather than tearing the app down.
        let surface_texture = match state.surface.surface.get_current_texture() {
            CurrentSurfaceTexture::Success(t) | CurrentSurfaceTexture::Suboptimal(t) => t,
            other => {
                eprintln!("rux: skipping frame ({other:?})");
                return;
            }
        };
        // vello renders with a compute shader, so it can't write the surface
        // texture directly (the surface is Bgra8, the storage target Rgba8).
        // render_to_surface used to hide this; in 0.9 we render into the
        // RenderSurface's intermediate target and blit that onto the surface.
        state
            .renderer
            .render_to_texture(
                &device_handle.device,
                &device_handle.queue,
                &state.scene,
                &state.surface.target_view,
                &RenderParams {
                    base_color: BG,
                    width,
                    height,
                    antialiasing_method: AaConfig::Area,
                },
            )
            .expect("render to texture");

        let mut encoder = device_handle
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("rux: blit to surface"),
            });
        let view = surface_texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        state
            .surface
            .blitter
            .copy(&device_handle.device, &mut encoder, &state.surface.target_view, &view);
        device_handle.queue.submit([encoder.finish()]);

        surface_texture.present();
    }
}

impl ApplicationHandler<RuxEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.state.is_some() {
            return;
        }

        let title = format!(
            "Rux — {}",
            self.path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "M2".into())
        );
        let attributes = Window::default_attributes()
            .with_title(title)
            .with_inner_size(winit::dpi::LogicalSize::new(420.0, 640.0));
        let window = Arc::new(event_loop.create_window(attributes).expect("create window"));

        let size = window.inner_size();
        let surface = pollster::block_on(self.context.create_surface(
            window.clone(),
            size.width.max(1),
            size.height.max(1),
            wgpu::PresentMode::AutoVsync,
        ))
        .expect("create surface");

        let device_handle = &self.context.devices[surface.dev_id];
        let renderer = Renderer::new(
            &device_handle.device,
            RendererOptions {
                use_cpu: false,
                antialiasing_support: AaSupport::area_only(),
                num_init_threads: NonZeroUsize::new(1),
                pipeline_cache: None,
            },
        )
        .expect("create renderer");

        self.state = Some(RenderState {
            window,
            surface,
            renderer,
            scene: Scene::new(),
        });
        self.request_redraw();
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: RuxEvent) {
        match event {
            RuxEvent::Reload => self.reload(),
        }
        self.request_redraw();
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(state) = self.state.as_mut() {
                    self.context.resize_surface(
                        &mut state.surface,
                        size.width.max(1),
                        size.height.max(1),
                    );
                }
                self.request_redraw();
            }
            WindowEvent::MouseWheel { delta, .. } => {
                // A line of wheel travel is ~ one line of text.
                let (dx, dy) = match delta {
                    MouseScrollDelta::LineDelta(x, y) => (x * LINE, y * LINE),
                    MouseScrollDelta::PixelDelta(p) => {
                        let scale = self.scale();
                        ((p.x / scale) as f32, (p.y / scale) as f32)
                    }
                };
                // Shift+wheel scrolls horizontally — the platform convention for a
                // wheel with only one axis.
                let (dx, dy) = if self.shift_held && dx == 0.0 { (dy, 0.0) } else { (dx, dy) };
                self.scroll_at(self.pointer, -dx, -dy);
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.pointer = (position.x, position.y);
                if self.bar_drag.is_some() {
                    self.drag_scrollbar(self.pointer);
                } else if self.text_drag {
                    self.drag_text(self.pointer);
                } else {
                    self.update_cursor();
                }
            }
            // Touch drags the content itself: the finger stays on the pixel it
            // grabbed, so the content follows it and the offset moves the other way.
            WindowEvent::Touch(touch) => {
                let scale = self.scale();
                let here = ((touch.location.x / scale) as f32, (touch.location.y / scale) as f32);
                match touch.phase {
                    TouchPhase::Started => self.touch = Some(here),
                    TouchPhase::Moved => {
                        if let Some((lx, ly)) = self.touch.replace(here) {
                            let at = (touch.location.x, touch.location.y);
                            self.scroll_at(at, lx - here.0, ly - here.1);
                        }
                    }
                    TouchPhase::Ended | TouchPhase::Cancelled => self.touch = None,
                }
            }
            WindowEvent::ModifiersChanged(mods) => {
                self.shift_held = mods.state().shift_key();
                self.ctrl_held = mods.state().control_key();
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state == ElementState::Pressed {
                    self.on_key(&event.logical_key);
                }
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => {
                // A press on a scrollbar thumb belongs to the bar, and a press in
                // an input starts a text selection: neither becomes a tap on the
                // content under it.
                if !self.press_scrollbar(self.pointer) && !self.press_text(self.pointer) {
                    self.press = Some(self.pointer);
                }
            }
            WindowEvent::MouseInput {
                state: ElementState::Released,
                button: MouseButton::Left,
                ..
            } => {
                if self.bar_drag.take().is_some() {
                    self.update_cursor();
                    return;
                }
                if std::mem::take(&mut self.text_drag) {
                    return;
                }
                if let Some((sx, sy)) = self.press.take() {
                    let (px, py) = self.pointer;
                    if (px - sx).hypot(py - sy) <= TAP_SLOP {
                        self.dispatch_tap(px, py);
                    }
                }
            }
            // Event-driven: we only paint in response to a redraw request, which
            // is issued on resume, resize, reload, and tap — not every frame.
            WindowEvent::RedrawRequested => self.render(),
            _ => {}
        }
    }

    /// The only clock in an otherwise event-driven loop: while an input is
    /// focused, wake every `BLINK` to toggle the caret. With no focus the
    /// deadline is `None`, so we wait indefinitely for the next real event.
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        match self.blink_deadline {
            Some(deadline) => {
                if Instant::now() >= deadline {
                    self.caret_visible = !self.caret_visible;
                    self.blink_deadline = Some(Instant::now() + BLINK);
                    self.request_redraw();
                }
                event_loop.set_control_flow(ControlFlow::WaitUntil(self.blink_deadline.unwrap()));
            }
            None => event_loop.set_control_flow(ControlFlow::Wait),
        }
    }
}

/// Open the Rux window for the given `.rux` file and run the frame loop until the
/// window closes. Watches the file and repaints on change.
pub fn run(path: PathBuf) {
    let event_loop = EventLoop::<RuxEvent>::with_user_event()
        .build()
        .expect("create event loop");
    event_loop.set_control_flow(ControlFlow::Wait);

    // Watch the file's directory *recursively* so edits to imported components
    // (which live in subdirectories) also trigger a reload. Reload on any `.rux`
    // change — `Document::load` re-reads the main file and its components.
    let proxy = event_loop.create_proxy();
    let watch_dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));

    let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        let Ok(event) = res else { return };
        if !matches!(event.kind, EventKind::Modify(_) | EventKind::Create(_)) {
            return;
        }
        let touches_rux = event
            .paths
            .iter()
            .any(|p| p.extension().is_some_and(|e| e == "rux"));
        if touches_rux {
            let _ = proxy.send_event(RuxEvent::Reload);
        }
    })
    .expect("create watcher");
    watcher
        .watch(&watch_dir, RecursiveMode::Recursive)
        .expect("watch directory");

    let mut app = App::new(path);
    event_loop.run_app(&mut app).expect("run app");

    drop(watcher); // keep the watcher alive for the loop's lifetime
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 200x200 box holding 500px-tall content: it scrolls down, not sideways.
    fn tall() -> ScrollRegion {
        ScrollRegion {
            id: 0,
            x: 0.0,
            y: 0.0,
            width: 200.0,
            height: 200.0,
            content_width: 200.0,
            content_height: 500.0,
            max: Offset { x: 0.0, y: 300.0 },
        }
    }

    /// The thumb is the box's fraction of the content, and sits at the top when
    /// unscrolled.
    #[test]
    fn thumb_is_proportional_to_the_content() {
        let (x, y, w, h) = bar_thumb(&tall(), Offset::default(), Axis2::Y).expect("a thumb");
        assert_eq!(h, 80.0, "200/500 of a 200px track");
        assert_eq!(y, 0.0, "unscrolled thumb starts at the top of the track");
        assert_eq!(w, BAR_W);
        assert_eq!(x, 200.0 - BAR_W, "the bar hugs the box's right edge");
    }

    /// The horizontal thumb is the mirror of the vertical one: it runs *along* the
    /// bottom edge and is only `BAR_W` thick. (Getting the track tuple's length
    /// and thickness the wrong way round here painted a thumb as tall as the whole
    /// box — invisible to every test that only looked at the vertical bar.)
    #[test]
    fn horizontal_thumb_lies_along_the_bottom_edge() {
        let mut wide = tall();
        wide.content_height = 200.0;
        wide.content_width = 500.0;
        wide.max = Offset { x: 300.0, y: 0.0 };

        let (x, y, w, h) = bar_thumb(&wide, Offset::default(), Axis2::X).expect("a thumb");
        assert_eq!(h, BAR_W, "a horizontal thumb is BAR_W *thick*, not BAR_W long");
        assert_eq!(w, 80.0, "200/500 of a 200px track");
        assert_eq!(x, 0.0);
        assert_eq!(y, 200.0 - BAR_W, "it sits on the box's bottom edge");
    }

    /// At the end of the content the thumb is at the end of its track — the
    /// bottom of the thumb meets the bottom of the box.
    #[test]
    fn thumb_reaches_the_end_of_the_track() {
        let r = tall();
        let (_, y, _, h) = bar_thumb(&r, Offset { x: 0.0, y: 300.0 }, Axis2::Y).expect("a thumb");
        assert_eq!(y + h, r.height);
    }

    /// The negative case: an axis with no travel has no thumb — nothing to draw,
    /// and nothing to grab. (A bar you can drag on a box that can't scroll was the
    /// easy bug here.)
    #[test]
    fn no_thumb_on_an_axis_that_does_not_scroll() {
        assert!(bar_thumb(&tall(), Offset::default(), Axis2::X).is_none());

        let mut fits = tall();
        fits.content_height = 200.0;
        fits.max = Offset::default();
        assert!(bar_thumb(&fits, Offset::default(), Axis2::Y).is_none());
        assert!(!fits.scrollable());
    }

    /// However long the content, the thumb stays big enough to grab.
    #[test]
    fn thumb_has_a_floor() {
        let mut huge = tall();
        huge.content_height = 100_000.0;
        huge.max = Offset { x: 0.0, y: 99_800.0 };
        let (_, _, _, h) = bar_thumb(&huge, Offset::default(), Axis2::Y).expect("a thumb");
        assert_eq!(h, BAR_MIN_THUMB);
    }

    /// When both axes scroll, the tracks stop short of the corner so they don't
    /// cross each other.
    #[test]
    fn tracks_leave_the_corner_free() {
        let mut both = tall();
        both.content_width = 500.0;
        both.max.x = 300.0;

        let (_, _, _, vh) = bar_track(&both, Axis2::Y);
        let (_, _, hw, _) = bar_track(&both, Axis2::X);
        assert_eq!(vh, both.height - BAR_W);
        assert_eq!(hw, both.width - BAR_W);

        // …and with one axis only, the track runs the full length.
        let (_, _, _, full) = bar_track(&tall(), Axis2::Y);
        assert_eq!(full, 200.0);
    }
}
