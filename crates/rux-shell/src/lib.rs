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
    Background, Cursor, FocusItem, FocusKind, FocusRegion, HitRegion, Paint, PaintRect, PaintText,
    Rgba, ScrollRegion, SelectRegion, TextAlign, TextContent, TextWrap,
};
use rux_runtime::Document;
use vello::kurbo::Affine;
use vello::peniko::Color;
use vello::util::{RenderContext, RenderSurface};
use vello::wgpu;
use vello::wgpu::CurrentSurfaceTexture;
use vello::{AaConfig, AaSupport, Renderer, RendererOptions, RenderParams, Scene};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
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
    /// Whether Shift is held (for Shift+Tab reverse traversal).
    shift_held: bool,
    /// Scrollable regions from the most recent layout.
    scrolls: Vec<ScrollRegion>,
    /// Scroll offset per scrollable box, in tree order. Survives the rebuild
    /// that follows every state change, so a list doesn't jump back to the top
    /// when you tap something in it.
    offsets: Vec<f32>,
    /// The `r-model` of the currently focused input, if any.
    focused: Option<String>,
    /// Whether the focused input is a `type="textarea"` (Enter → newline).
    focused_multiline: bool,
    /// The `r-model` of the currently open `select` dropdown, if any. Survives
    /// the rebuild after a state change, like scroll offsets.
    open_select: Option<String>,
    /// Caret position in the focused input, as a byte index into its value.
    caret: usize,
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
            scrolls: Vec::new(),
            offsets: Vec::new(),
            focused: None,
            focused_multiline: false,
            open_select: None,
            caret: 0,
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

    /// Scroll the innermost scrollable box under the pointer by `dy` logical
    /// pixels. Nothing under the pointer scrolls (or it's already at the end) →
    /// nothing happens, and no repaint is queued.
    fn scroll_at(&mut self, pointer: (f64, f64), dy: f32) {
        let scale = self.scale();
        let (px, py) = ((pointer.0 / scale) as f32, (pointer.1 / scale) as f32);

        // Innermost wins: scrollers are pushed parent-first, so search backwards.
        let Some(region) = self
            .scrolls
            .iter()
            .rev()
            .find(|s| s.contains(px, py) && s.max_offset > 0.0)
        else {
            return;
        };
        let (id, max) = (region.id, region.max_offset);

        let current = self.offsets[id];
        let next = (current + dy).clamp(0.0, max);
        if next != current {
            self.offsets[id] = next;
            self.request_redraw();
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

        // Focus takes precedence: an input under the pointer becomes focused,
        // with the caret at the character you tapped.
        if let Some(region) = self
            .focuses
            .iter()
            .rev()
            .find(|f| f.contains(fx, fy))
            .cloned()
        {
            // Map the tap into the text box's own coordinates to find the
            // character. An empty input is showing its placeholder, not a value,
            // so its caret belongs at 0.
            let value = self.document.engine_mut().get_string(&region.model);
            let caret = match &region.text {
                Some(t) if !value.is_empty() => self.text.index_at_point(
                    &value,
                    &rux_paint::text_style(&t.content),
                    Some(t.width),
                    fx - t.x,
                    fy - t.y,
                ),
                _ => 0,
            };
            self.focused_multiline = region.multiline;
            self.set_focus(Some((region.model, caret)));
            return;
        }
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
    /// Edit the focused input at the caret. Returns whether anything changed.
    ///
    /// Indices are byte offsets into the value, always on a char boundary (we
    /// only ever step by whole characters), so slicing is safe.
    fn edit_focused(&mut self, key: &Key) {
        let Some(model) = self.focused.clone() else {
            return;
        };
        let mut value = self.document.engine_mut().get_string(&model);
        let caret = self.caret.min(value.len());

        // How far the previous / next character is, in bytes.
        let prev = value[..caret].chars().next_back().map(char::len_utf8);
        let next = value[caret..].chars().next().map(char::len_utf8);

        let mut edited = false;
        let mut moved = false;
        let mut new_caret = caret;

        match key {
            Key::Named(NamedKey::Backspace) => {
                if let Some(len) = prev {
                    value.replace_range(caret - len..caret, "");
                    new_caret = caret - len;
                    edited = true;
                }
            }
            Key::Named(NamedKey::Delete) => {
                if let Some(len) = next {
                    value.replace_range(caret..caret + len, "");
                    edited = true;
                }
            }
            Key::Named(NamedKey::ArrowLeft) => {
                if let Some(len) = prev {
                    new_caret = caret - len;
                    moved = true;
                }
            }
            Key::Named(NamedKey::ArrowRight) => {
                if let Some(len) = next {
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
                value.insert(caret, ' ');
                new_caret = caret + 1;
                edited = true;
            }
            // Enter inserts a newline in a textarea; single-line inputs ignore it.
            Key::Named(NamedKey::Enter) if self.focused_multiline => {
                value.insert(caret, '\n');
                new_caret = caret + 1;
                edited = true;
            }
            Key::Character(s) => {
                for c in s.chars().filter(|c| !c.is_control()) {
                    value.insert(new_caret, c);
                    new_caret += c.len_utf8();
                    edited = true;
                }
            }
            _ => {}
        }

        if edited {
            self.caret = new_caret;
            self.document.engine_mut().set_string(&model, &value);
            self.scroll_caret_into_view(&model, &value, new_caret);
            self.document.set_focus(Some((model, new_caret)));
            self.document.rebuild();
            self.reset_blink();
            self.request_redraw();
        } else if moved {
            self.caret = new_caret;
            self.scroll_caret_into_view(&model, &value, new_caret);
            self.document.set_focus(Some((model, new_caret)));
            self.reset_blink();
            self.request_redraw();
        }
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
        let mut off = self.offsets.get(sid).copied().unwrap_or(0.0);
        if cy < off {
            off = cy;
        } else if cy + ch > off + visible {
            off = cy + ch - visible;
        }
        // The next layout re-clamps this to the content's real max offset.
        if let Some(slot) = self.offsets.get_mut(sid) {
            *slot = off.max(0.0);
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
        let Some(idx) = self.focus_index else { return };
        match key {
            Key::Named(NamedKey::Space | NamedKey::Enter) => self.activate_focused(idx),
            Key::Named(NamedKey::Escape) => {
                self.focus_index = None;
                self.request_redraw();
            }
            _ => {}
        }
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

    /// Focus an input (or clear focus) and tell the document, so the caret paints.
    fn set_focus(&mut self, focus: Option<(String, usize)>) {
        self.focused = focus.as_ref().map(|(m, _)| m.clone());
        self.caret = focus.as_ref().map(|(_, c)| *c).unwrap_or(0);
        self.document.set_focus(focus);
        self.reset_blink();
        self.request_redraw();
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
        let content = rux_paint::build_scene(&layout.paints, text, images, caret_visible);
        state.scene.reset();
        state
            .scene
            .append(&content, Some(Affine::scale(scale)));

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
        // Keep offsets in step with the scrollers the new layout actually has,
        // and re-clamp them (the content may have shrunk under us).
        offsets.resize(layout.scrolls.len(), 0.0);
        for region in &layout.scrolls {
            offsets[region.id] = offsets[region.id].clamp(0.0, region.max_offset);
        }
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
                const LINE: f32 = 24.0;
                let dy = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y * LINE,
                    MouseScrollDelta::PixelDelta(p) => (p.y / self.scale()) as f32,
                };
                self.scroll_at(self.pointer, -dy);
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.pointer = (position.x, position.y);
                self.update_cursor();
            }
            WindowEvent::ModifiersChanged(mods) => {
                self.shift_held = mods.state().shift_key();
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
                self.press = Some(self.pointer);
            }
            WindowEvent::MouseInput {
                state: ElementState::Released,
                button: MouseButton::Left,
                ..
            } => {
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
