//! Rux runtime shell, milestone M3.
//!
//! Opens a native window (winit), manages the GPU via vello's `RenderContext`,
//! loads a `.rux` document each frame's tree from `rux-runtime`, and paints it
//! (`rux-paint`). A `notify` file watcher wakes the event loop through an
//! `EventLoopProxy` on every save, so edits to the `.rux` file repaint live,
//! the hot-reload path from `docs/04-architecture.md`.
//!
//! This is the largest crate in the workspace and the least pure, because it is
//! where the pipeline meets an operating system. Beyond the frame loop it owns
//! all of input: pointer and touch, keyboard and modifiers, focus and tab order,
//! text editing, selection and the clipboard, scrolling, and the dev overlay
//! that puts a load error on screen instead of exiting.
//!
//! It runs in two worlds. [`run`] opens a native window; `start_web` and its
//! neighbours drive the same document against a canvas, and are compiled only
//! for `wasm32`, so they are absent from these docs unless the page was built
//! for that target. The browser has no filesystem, no blocking main thread and no
//! system clipboard, so those three assumptions are not baked into the paths
//! above. Making the shell survive that was most of the v0.5 web work, and it
//! is the reason a phone looks reachable at all.
//!
//! Touch is not the mouse. Routing a finger down the pointer path is the bug
//! v0.5.1 exists to fix: a drag moves the caret where a mouse drag selects, and
//! a long press picks a word. Anything new that reads a position should convert
//! it through the one shared conversion here, not derive a second correct one,
//! because two places doing the same coordinate arithmetic eventually disagree.

use std::num::NonZeroUsize;
use std::path::Path;
#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;
use std::sync::Arc;
// `web_time` re-exports `std::time` verbatim on native, so this is the std type
// everywhere except wasm, where `std::time::Instant` panics on construction and
// `ControlFlow::WaitUntil` wants the browser clock's instant instead. One import
// covers both; there is no cfg and no behavioural difference off the web.
use web_time::{Duration, Instant};

#[cfg(target_arch = "wasm32")]
use std::cell::RefCell;
#[cfg(target_arch = "wasm32")]
use std::rc::Rc;

#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
use notify::{EventKind, RecursiveMode, Watcher};
use rux_layout::{
    Background, Cursor, EnterKey, Field, FocusItem, FocusKind, FocusRegion, HitRegion, InputKind,
    Keyboard,
    Offset, Paint, PaintRect, PaintText, Rgba, ScrollRegion, SelectRegion, Sides, StateRegion,
    TextAlign,
    TextContent, TextWrap,
};
use rux_runtime::{Document, Focus, Insets, InteractionState, Viewport};
use vello::kurbo::Affine;
use vello::peniko::Color;
use vello::util::{RenderContext, RenderSurface};
use vello::wgpu;
use vello::wgpu::CurrentSurfaceTexture;
use vello::{AaConfig, AaSupport, Renderer, RendererOptions, RenderParams, Scene};
#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
use accesskit::{Node as AccessKitNode, NodeId, Role, Toggled, Tree, TreeUpdate};
// Only the accessibility tree uses these, so they are gated with it rather
// than sitting unused in the wasm build.
#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
use rux_layout::{AccessNode, AccessRole};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, Ime, MouseButton, MouseScrollDelta, TouchPhase, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::{CursorIcon, Theme, Window, WindowId};

/// Events delivered to the winit loop from outside it.
#[derive(Debug)]
enum RuxEvent {
    /// The `.rux` file changed on disk.
    #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
    Reload,
    /// The GPU surface finished initialising. Web only: `create_surface` is
    /// async and `resumed` is not, so setup runs as a task and wakes the loop
    /// here. The payload is parked in `App::pending` rather than carried in the
    /// event, because wgpu's types are not `Send` on wasm and the proxy requires
    /// that they be.
    #[cfg(target_arch = "wasm32")]
    SurfaceReady,
    /// New source text from the host page, the playground's replacement for a
    /// file watcher. `String` is `Send`, so this one can travel in the event.
    #[cfg(target_arch = "wasm32")]
    SetSource(String),
    /// The host page's canvas container changed size, in logical pixels. Web
    /// only: on a desktop the window manager drives this, but in a page the
    /// layout does, and the canvas has to be told rather than asked.
    #[cfg(target_arch = "wasm32")]
    Resize(f64, f64),
    /// The browser's soft keyboard edited the focused field. Web only: on a
    /// phone the text does not arrive as key presses at all, it arrives as the
    /// new contents of the hidden `<input>` the shell keeps focused, so this
    /// carries the whole value rather than a keystroke.
    ///
    /// `composing` is the byte length of any in-progress composition at the end
    /// of the caret, `0` when there is none. The browser runs the composition
    /// itself here; the shell only needs to know which tail of the text is still
    /// provisional so it can underline it, exactly as it does natively.
    ///
    /// `anchor` is the other end of the selection, equal to `caret` when nothing
    /// is selected. It is carried because the browser's own copy, cut and
    /// select-all act on the hidden input's selection, so the two have to agree
    /// about what is selected or the phone's clipboard operates on the wrong
    /// text (before v0.5.1, on no text at all).
    #[cfg(target_arch = "wasm32")]
    WebText { value: String, caret: usize, anchor: usize, composing: usize },
    /// The browser's clipboard resolved, carrying what it held. Web only: the
    /// Clipboard API is a promise, so a paste cannot be finished inside the tap
    /// or key press that asked for it.
    #[cfg(target_arch = "wasm32")]
    WebPaste(String),
    /// The user walked the browser's history: its Back or Forward button, a
    /// swipe, or a long-press that jumped several entries at once.
    ///
    /// The payload is the index Rux stamped on that entry when it pushed it,
    /// handed back untouched, which is why this is an index and not a
    /// direction: a browser reports where it landed. `None` is an entry Rux
    /// never pushed, which is the tab's own first entry, so it means the path
    /// has to be read back off the URL instead.
    #[cfg(target_arch = "wasm32")]
    WebRoute(Option<usize>),
    /// Enter from a phone's keyboard, typed into the hidden input. It never
    /// reaches the window as a key, because the canvas is not what has focus,
    /// and an `<input>` does nothing with it on its own: before this a form
    /// could not be submitted from a phone's browser, Next moved nowhere and a
    /// textarea could not take a new line. The web half of `AndroidEnter`.
    #[cfg(target_arch = "wasm32")]
    WebEnter,
    /// An input method on Android edited the focused field.
    ///
    /// Carries the whole editing state rather than a keystroke, for the same
    /// reason [`RuxEvent::WebText`] does: on a phone the text is edited by
    /// something else and reported as a result. Offsets are UTF-16 code units,
    /// which is what the platform counts in, converted on arrival.
    ///
    /// `compose` is the composing range, or `None` when nothing is being
    /// composed.
    #[cfg(target_os = "android")]
    AndroidText { value: String, caret: usize, anchor: usize, compose: Option<(usize, usize)> },
    /// The platform's picker closed, having been opened for a `<select>`.
    ///
    /// `index` is the option chosen, or `None` if the picker was dismissed
    /// without choosing. Carried through the proxy rather than answered inline
    /// because the dialog runs on Android's main thread while the shell loops
    /// on another, and because a picker is not answered in the tap that opened
    /// it: the person may think about it for a while first.
    ///
    /// The select it belongs to is not carried. It is held in
    /// [`App::pending_select`], because the answer has to be matched against
    /// the field as it was when the picker opened, and a rebuild in between
    /// could have moved it.
    #[cfg(any(target_os = "android", target_arch = "wasm32"))]
    PickedSelect { index: Option<usize> },
    /// The platform date picker closed: the day chosen as `YYYY-MM-DD`, or
    /// `None` if it was dismissed. The field is in [`App::pending_date`].
    #[cfg(any(target_os = "android", target_arch = "wasm32"))]
    PickedDate { value: Option<String> },
    /// A choice from the platform's text menu: one of the `MENU_ACTION_`
    /// codes. Share and the `PROCESS_TEXT` apps are Java's to run and never
    /// arrive here.
    #[cfg(target_os = "android")]
    AndroidTextAction(i32),
    /// Enter from the input method, which never reaches the window as a key:
    /// Android drops the event an input method dispatches before winit sees
    /// it. `true` for the action key, which does what its label says; `false`
    /// for a plain Enter, which submits a form.
    #[cfg(target_os = "android")]
    AndroidEnter(bool),
    /// The on-screen keyboard changed height. See `KEYBOARD`.
    #[cfg(target_os = "android")]
    AndroidKeyboard,
    /// `rux run --device` sent the project's files again: each path with its
    /// new contents, or `None` for one deleted. See [`dev_link`].
    #[cfg(target_os = "android")]
    DevFiles(Vec<(String, Option<Vec<u8>>)>),
    /// Autofill filled a field, by its [`autofill_id`].
    #[cfg(target_os = "android")]
    AndroidAutofill { id: i32, value: String },
    /// The platform's text menu closed without Rux asking it to. `collapse`
    /// says whether the selection goes with it: yes for Back or Share, as in
    /// any Android field, and no when an app was handed the text and will
    /// answer through [`RuxEvent::AndroidProcessedText`].
    #[cfg(target_os = "android")]
    AndroidTextMenuClosed { collapse: bool },
    /// An app offered through `PROCESS_TEXT` (a translator, a spell checker)
    /// answered with text to put in place of the selection.
    #[cfg(target_os = "android")]
    AndroidProcessedText(String),
    /// A link opened the app while it was already running, already turned
    /// into a route by [`link_route`].
    #[cfg(target_os = "android")]
    AndroidLink(String),
    /// Assistive technology asked us something (it attached, it wants the
    /// tree, it moved focus). Delivered through the same proxy as hot-reload.
    #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
    Access(accesskit_winit::Event),
}

#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
impl From<accesskit_winit::Event> for RuxEvent {
    fn from(event: accesskit_winit::Event) -> Self {
        Self::Access(event)
    }
}

/// Taps closer than this (in physical pixels) between press and release still
/// count as a tap rather than a drag.
const TAP_SLOP: f64 = 6.0;

/// How long a finger must rest on text before the press takes the word under it.
///
/// This is the gesture a phone uses to start selecting, and it is why a drag is
/// free to mean something else (moving the caret). Roughly the platform
/// convention: much shorter and an ordinary tap starts selecting text, much
/// longer and the field feels unresponsive.
const LONG_PRESS: Duration = Duration::from_millis(500);

/// How fast the finger must still be moving when it lifts for the scroller to
/// carry on, in **logical px per millisecond**. Below it, a lift is a stop.
///
/// Without a floor every scroll ends in a tiny creep, which reads as the list
/// failing to settle rather than as momentum.
const FLING_MIN_V: f32 = 0.05;

/// The time constant of the fling's decay, in milliseconds: velocity falls by
/// `1/e` every `FLING_TAU`.
///
/// The decay is **time-based rather than per-frame**. A per-frame multiplier is
/// the usual shortcut and it makes the distance thrown depend on the refresh
/// rate, so the same flick travels further on a 120 Hz phone than a 60 Hz one.
/// Phones vary their refresh rate while running, so that is not even stable on
/// one device.
const FLING_TAU: f32 = 325.0;

/// How far back to look when measuring the lift velocity, in milliseconds.
///
/// Measuring the **last move alone** is the obvious thing and it is wrong in the
/// case that matters: dragging a list, holding it still, then lifting would
/// throw it, because the final event pair can be a stale delta over a tiny
/// interval. A window over the recent path reports what the hand was actually
/// doing, and reports roughly zero for a finger that had stopped.
const FLING_SAMPLE_MS: f32 = 100.0;

/// One step of the fling's decay: how far it travels in `dt_ms`, and what
/// velocity is left afterwards.
///
/// Split out from the stepping so the property the whole design rests on can be
/// tested: with `v = v0 * e^(-t/tau)`, the distance over a step is the integral,
/// `v0 * tau * (1 - e^(-dt/tau))`, and **one long step travels exactly as far as
/// many short ones**. Multiplying velocity by elapsed time instead is the usual
/// shortcut, and it undershoots by more the longer the frame, so a list thrown
/// on a busy frame would travel less than the same throw on an idle one.
fn fling_step(v: f32, dt_ms: f32) -> (f32, f32) {
    let decay = (-dt_ms / FLING_TAU).exp();
    (v * FLING_TAU * (1.0 - decay), v * decay)
}

/// A scroller still moving after the finger left it.
#[derive(Clone, Copy, Debug)]
struct Fling {
    /// Which scroller is moving. Captured when the drag scrolled, not looked up
    /// again per frame: the finger has gone, so there is no pointer to ask, and
    /// a fling that re-tested position could hand itself to a different box
    /// halfway through.
    id: usize,
    /// Content velocity in logical px per ms, already in the sense `scroll_to`
    /// wants: positive moves the content the way a downward drag does.
    vx: f32,
    vy: f32,
    /// When the last step ran, so a step advances by real elapsed time rather
    /// than by an assumed frame.
    last: Instant,
}

/// The selection toolbar's height and the padding around its labels, in logical
/// px.
const TOOLBAR_H: f32 = 34.0;
const TOOLBAR_PAD: f32 = 12.0;
/// Gap between the toolbar and the field it belongs to.
const TOOLBAR_GAP: f32 = 6.0;

/// What the selection toolbar offers, left to right.
///
/// A phone has no Ctrl+C, and a browser has no system clipboard for Rux to
/// reach, so without this there is no way at all to get text out of a field on
/// either. The desktop app keeps its shortcuts; this is the same four actions
/// with somewhere to tap.
#[derive(Clone, Copy, Debug, PartialEq)]
enum TextAction {
    Copy,
    Cut,
    Paste,
    SelectAll,
}

impl TextAction {
    const ALL: [TextAction; 4] =
        [TextAction::Copy, TextAction::Cut, TextAction::Paste, TextAction::SelectAll];

    /// Whether this action may be offered on a `type="password"`.
    ///
    /// Paste puts text in, Select all moves no text anywhere. Copy and Cut
    /// take it out, which is the one thing a masked field exists to prevent.
    fn allowed_on_secret(self) -> bool {
        matches!(self, TextAction::Paste | TextAction::SelectAll)
    }

    fn label(self) -> &'static str {
        match self {
            TextAction::Copy => "Copy",
            TextAction::Cut => "Cut",
            TextAction::Paste => "Paste",
            TextAction::SelectAll => "Select all",
        }
    }

    /// Button width from the label's length.
    ///
    /// Estimated rather than measured because the geometry is needed for hit
    /// testing as well as painting, and threading the text engine into a hit
    /// test to agree with the painter is how the two end up disagreeing. The
    /// estimate is deliberately generous, so a label sits inside its button
    /// rather than against its edge.
    fn width(self) -> f32 {
        (self.label().chars().count() as f32 * 7.8).round() + TOOLBAR_PAD * 2.0
    }
}

/// Which buttons the toolbar offers, left to right.
///
/// **A password field offers fewer, and the toolbar shrinks to fit.** Copy and
/// Cut are refused on one, and a button that does nothing when tapped teaches
/// the person the app is broken. Every platform's own toolbar drops them there
/// too, so this is what a phone user expects.
///
/// **With nothing selected, only what makes sense at a caret:** Paste, and
/// Select all when there is something to select. That toolbar is opened by a
/// long press that found no word, which is how every phone offers Paste into
/// an empty field. Without it an empty field could not be pasted into by hand.
fn offered_actions(secret: bool, selected: bool, has_text: bool) -> Vec<TextAction> {
    TextAction::ALL
        .into_iter()
        .filter(|a| a.allowed_on_secret() || !secret)
        .filter(|a| {
            selected
                || match a {
                    TextAction::Paste => true,
                    TextAction::SelectAll => has_text,
                    TextAction::Copy | TextAction::Cut => false,
                }
        })
        .collect()
}

/// Where the toolbar sits for a field at `(x, y, w, h)`, and the box of each
/// button, in logical px.
///
/// Above the field when there is room, below it when there is not, and never off
/// the left edge. One function so the painter and the hit test cannot drift.
fn toolbar_layout(
    field: (f32, f32, f32, f32),
    viewport: (f32, f32),
    ceiling: f32,
    offered: &[TextAction],
) -> ((f32, f32, f32, f32), Vec<(TextAction, f32, f32, f32, f32)>) {
    let total: f32 = offered.iter().map(|a| a.width()).sum();
    let (fx, fy, _, fh) = field;
    let x = fx.min(viewport.0 - total).max(0.0);
    // Above by preference: a finger selecting text is usually below the line it
    // is selecting, and a toolbar under the finger is one you cannot read.
    //
    // **"Room" starts below the status bar, not at the top of the window.** A
    // phone draws its clock over the first few dozen pixels of the app, and a
    // strip put there is half under it and hard to hit. Reported from the phone
    // against the first field on the page. `ceiling` is the top safe-area inset.
    let above = fy - TOOLBAR_H - TOOLBAR_GAP;
    let y = if above >= ceiling { above } else { fy + fh + TOOLBAR_GAP };

    let mut buttons = Vec::with_capacity(offered.len());
    let mut bx = x;
    for &action in offered {
        let w = action.width();
        buttons.push((action, bx, y, w, TOOLBAR_H));
        bx += w;
    }
    ((x, y, total, TOOLBAR_H), buttons)
}

/// What the finger currently down is doing to a text field.
///
/// Touch used to share the mouse's press/drag/release path, which meant a drag
/// selected, because that is what a mouse does. A phone expects the three
/// gestures below instead, so touch needs its own small state machine: the same
/// finger movement means different things depending on whether the press has had
/// time to become a long one.
#[derive(Clone, Copy, Debug, PartialEq)]
enum TouchText {
    /// Down on text and not yet resolved. Still becomes `Selecting` if the
    /// finger rests until `deadline`, or `Caret` if it moves first.
    Pending { at: (f64, f64), deadline: Instant },
    /// Moved before the deadline: the caret follows the finger and nothing is
    /// selected.
    Caret,
    /// The long press took a word: further movement extends the selection from
    /// it, which is the only gesture that selects.
    Selecting,
}

/// What a finger `distance` px from where it went down means, given what the
/// press was already doing.
///
/// The whole gesture model is this one decision, so it is a plain function
/// rather than inline in the event arm: a press that moves before it is old
/// enough is a caret drag and can never become a selection afterwards, and one
/// that has already taken a word keeps extending it however far it travels.
fn touch_text_after_move(state: TouchText, distance: f64) -> TouchText {
    match state {
        TouchText::Pending { .. } if distance > TAP_SLOP => TouchText::Caret,
        other => other,
    }
}

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
/// One line of scroll travel, the wheel's unit, and the arrow keys'.
const LINE: f32 = 24.0;

/// A press in progress on an element that declared pointer handlers.
///
/// Held from the press to the release, because every gesture except `@press` is
/// decided by what happens *after* the finger lands: a release is a release, a
/// long press is one that stayed still, a swipe is one that travelled and left,
/// and a drag is one that is still travelling.
#[derive(Clone, Debug)]
struct GesturePress {
    /// The element's top-left in logical px, so every coordinate handed to a
    /// handler is relative to the element it was written on.
    origin: (f32, f32),
    /// The element's width and height, handed over beside the position so a
    /// handler can say how far across it the pointer is.
    size: (f32, f32),
    handlers: Vec<(rux_layout::Gesture, String)>,
    instance: Option<String>,
    /// Where the press landed, in window-global logical px.
    start: (f32, f32),
    /// Where it was on the previous move, so a handler can be told how far it
    /// came *this* event as well as how far in total. Velocity and flick
    /// detection want the former; following a finger wants the latter.
    last: (f32, f32),
    at: Instant,
    /// Whether `@drag` has already been told the drag began. A drag reports
    /// start, then moves, then end, and the start is not the press: a press
    /// that never moves is not a drag at all.
    dragging: bool,
    /// A long press fires once. Without this it would fire on every wake-up
    /// while the finger rested.
    long_fired: bool,
    /// `touch-action` on the element the press landed on, which decides whether
    /// a scroller is allowed to take this gesture instead.
    touch_action: rux_layout::TouchAction,
    /// Whether this press came from a finger rather than a mouse button.
    ///
    /// The axis claim is **touch only**, and not as a simplification: a mouse
    /// does not scroll by dragging, it scrolls by wheel, so the mouse path has
    /// no `scroll_at` at all. Letting a scroller "win" a mouse drag would hand
    /// the gesture to something that does nothing with it, and the drag would
    /// simply stop working on the desktop.
    from_touch: bool,
    /// The axis arbitration, decided once when the finger first passes the slop
    /// threshold and never revisited.
    ///
    /// `None` means the question has not been asked yet. `Some(true)` means a
    /// scroller took this gesture and `@drag` must stay out of it for the rest
    /// of the press; `Some(false)` means the element has it.
    ///
    /// It is settled once rather than re-evaluated per move because a gesture
    /// that changes owner mid-flight is visibly wrong, and because no platform
    /// does it: iOS arbitrates its pan recognizers on the initial translation,
    /// Android intercepts at the slop crossing, and the web decides ahead of
    /// time from `touch-action`.
    scroll_won: Option<bool>,
}

/// The fields a drag or swipe adds: which part of the gesture this is, how far
/// it has come in total, and how far it came this event.
///
/// Two distances rather than one, and named so they cannot be confused.
/// `totalX` is measured from where the press landed, which is what following a
/// finger wants: assign it and the thing tracks the hand with no bookkeeping.
/// `moveX` is measured from the previous move, which is what velocity and flick
/// detection want. Calling either of them `dx` invites each reader to assume the
/// other one.
fn drag_fields(
    name: &str,
    start: (f32, f32),
    last: (f32, f32),
    fx: f32,
    fy: f32,
) -> Vec<(String, rux_reactive::Value)> {
    use rux_reactive::Value;
    vec![
        ("phase".to_string(), Value::Text(name.to_string())),
        ("totalX".to_string(), Value::Number((fx - start.0) as f64)),
        ("totalY".to_string(), Value::Number((fy - start.1) as f64)),
        ("moveX".to_string(), Value::Number((fx - last.0) as f64)),
        ("moveY".to_string(), Value::Number((fy - last.1) as f64)),
    ]
}

/// How far a press has to travel to count as a swipe, in logical px, and how
/// long it may take. A slow drag that ends far away is a drag, not a swipe.
const SWIPE_DISTANCE: f32 = 40.0;
const SWIPE_TIME: Duration = Duration::from_millis(600);

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

/// The track a scrollbar runs in, as `(x, y, w, h)` in logical px, an overlay
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
    //, the standard proportion, but never so short it can't be grabbed.
    let thumb_len = (track_len * visible / content.max(1.0)).clamp(BAR_MIN_THUMB.min(track_len), track_len);
    let travel = (track_len - thumb_len).max(0.0);
    let pos = match axis {
        Axis2::Y => offset.y,
        Axis2::X => offset.x,
    };
    let along = travel * (pos / max).clamp(0.0, 1.0);
    // The track tuple is (x, y, w, h): its thickness is `tw` on the vertical bar
    // and `th` on the horizontal one, the length is the other component.
    Some(match axis {
        Axis2::Y => (tx, ty + along, tw, thumb_len),
        Axis2::X => (tx + along, ty, thumb_len, th),
    })
}

/// A layout `Transform` as a kurbo `Affine`, identity when there is none. The
/// coefficients are already in the same order.
fn to_affine(m: Option<rux_layout::Transform>) -> Affine {
    match m {
        Some(t) => Affine::new([t[0] as f64, t[1] as f64, t[2] as f64, t[3] as f64, t[4] as f64, t[5] as f64]),
        None => Affine::IDENTITY,
    }
}

/// Paint items for every visible scrollbar: a faint track with a lighter thumb,
/// drawn over the content so a scroller's own clip can't eat them.
fn scrollbar_paints(scrolls: &[ScrollRegion], offsets: &[Offset], alpha: f32) -> Vec<Paint> {
    let track_bg = Rgba::new(1.0, 1.0, 1.0, 0.05 * alpha);
    let thumb_bg = Rgba::new(0.80, 0.84, 0.96, 0.35 * alpha); // #cdd6f4 at 35%
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
                border: Sides::ZERO,
                border_color: None,
            }));
            out.push(Paint::Rect(PaintRect {
                x: thx,
                y: thy,
                width: thw,
                height: thh,
                background: Some(Background::Color(thumb_bg)),
                radius: [BAR_W / 2.0; 4],
                border: Sides::ZERO,
                border_color: None,
            }));
        }
    }
    out
}

/// Paint items for an open dropdown: a single floating panel with a shadow, the
/// selected value picked out as a pill, and thin separators between options.
/// The selection toolbar: one rounded strip of actions above (or below) the
/// focused field. Same palette as the dropdown, so the two read as one system.
fn toolbar_paints(
    field: (f32, f32, f32, f32),
    viewport: (f32, f32),
    ceiling: f32,
    offered: &[TextAction],
) -> Vec<Paint> {
    let panel_bg = Rgba::new(0.19, 0.20, 0.27, 1.0); // #313244
    let border = Rgba::new(0.27, 0.28, 0.35, 1.0); // #45475a
    let ink = Rgba::new(0.80, 0.84, 0.96, 1.0); // #cdd6f4
    let divider = Rgba::new(0.35, 0.36, 0.44, 1.0); // #585b70

    let ((x, y, w, h), buttons) = toolbar_layout(field, viewport, ceiling, offered);
    let mut out = Vec::with_capacity(buttons.len() * 2 + 2);
    out.push(Paint::Shadow {
        x,
        y: y + 3.0,
        width: w,
        height: h,
        radius: 8.0,
        blur: 16.0,
        color: Rgba::new(0.0, 0.0, 0.0, 0.45),
    });
    out.push(Paint::Rect(PaintRect {
        x,
        y,
        width: w,
        height: h,
        background: Some(Background::Color(panel_bg)),
        radius: [8.0; 4],
        border: Sides::uniform(1.0),
        border_color: Some(border),
    }));

    for (i, (action, bx, by, bw, bh)) in buttons.iter().enumerate() {
        // A hairline between buttons, so the strip reads as separate targets
        // rather than one wide button.
        if i > 0 {
            out.push(Paint::Rect(PaintRect {
                x: *bx,
                y: by + 7.0,
                width: 1.0,
                height: bh - 14.0,
                background: Some(Background::Color(divider)),
                radius: [0.0; 4],
                border: Sides::ZERO,
                border_color: None,
            }));
        }
        out.push(Paint::Text(PaintText {
            x: *bx,
            y: by + (bh - 17.0) / 2.0,
            width: *bw,
            height: 17.0,
            content: TextContent {
                align: TextAlign::Center,
                ..overlay_text(action.label().to_string(), 14.0, 500, ink)
            },
        }));
    }
    out
}

/// The on-screen keyboard's height in physical pixels, which the layout gives
/// up. Always 0 off Android: a desktop has no such keyboard, and a phone's
/// browser resizes the page itself.
fn keyboard_px() -> u32 {
    #[cfg(target_os = "android")]
    {
        KEYBOARD.load(std::sync::atomic::Ordering::Relaxed)
    }
    #[cfg(not(target_os = "android"))]
    {
        0
    }
}

/// Whether a field's text may be kept for when the app comes back after the
/// platform ended it, which writes it to disk while the process is dead.
///
/// Never a password, as before. Never a card number, its security code or
/// expiry, or a one-time code either: the same text a password manager treats
/// as secret. And never a field whose author said `autocomplete="off"`,
/// which is what that attribute already asks of a browser, whose history does
/// not restore such a field on Back.
#[cfg_attr(not(any(target_os = "android", target_arch = "wasm32")), allow(dead_code))]
fn kept_across_a_kill(kind: InputKind, field: &Field) -> bool {
    const SECRET: &[&str] = &[
        "off",
        "current-password",
        "new-password",
        "one-time-code",
        "cc-number",
        "cc-csc",
        "cc-exp",
        "cc-exp-month",
        "cc-exp-year",
    ];
    kind != InputKind::Password
        && !field
            .autocomplete
            .as_deref()
            .is_some_and(|tokens| tokens.split_whitespace().any(|t| SECRET.contains(&t)))
}

/// Whether typing here is done on an on-screen keyboard: always on Android,
/// on the web when the browser says its pointer is a finger.
fn touch_first() -> bool {
    #[cfg(target_arch = "wasm32")]
    {
        web_is_touch()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        cfg!(target_os = "android")
    }
}

/// The word at byte `caret` in `text`, or the one just before it when the
/// caret sits in spaces or at the end: what Select takes from a caret menu,
/// which is usually opened after the last word. `None` when there is no word
/// before or at the caret.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
fn word_near(text: &str, caret: usize) -> Option<(usize, usize)> {
    let is_word = |c: char| c.is_alphanumeric() || c == '_' || c == '\'';
    let caret = caret.min(text.len());
    let caret = (0..=caret).rev().find(|i| text.is_char_boundary(*i))?;
    // Step back over spaces and punctuation to the word before, unless the
    // caret is already on one.
    let on_word = text[caret..].chars().next().is_some_and(is_word);
    let mut end = caret;
    if !on_word {
        end = text[..caret].char_indices().rev().find(|(_, c)| is_word(*c)).map(|(i, c)| i + c.len_utf8())?;
    } else {
        end += text[caret..].chars().take_while(|c| is_word(*c)).map(char::len_utf8).sum::<usize>();
    }
    let start = text[..end]
        .char_indices()
        .rev()
        .take_while(|(_, c)| is_word(*c))
        .last()
        .map(|(i, _)| i)?;
    Some((start, end))
}

// ── Selection handles ────────────────────────────────────────────────────────
//
// The teardrops a finger drags to change a selection, and the single one under
// a caret that a finger drags to move it. Drawn by Rux on every platform rather
// than borrowed from Android: the platform's handles belong to `TextView`'s
// editor and are not offered to any other view, which is why every app that
// draws its own text (Chrome, Flutter, Compose) draws its own handles too.

/// Radius of a handle's round part, in logical px. Android's own handle is a
/// 22dp square with three corners rounded, so this is that.
const HANDLE_R: f32 = 11.0;

/// How far from a handle's middle a finger still takes it, in logical px. A
/// 48dp target, the size Android asks every touch target to be, around a
/// shape half that.
const HANDLE_REACH: f32 = 24.0;

/// How long the lone caret handle, and the Paste menu that came with it, stay
/// up with nothing touching them. A selection has no such clock: it stays
/// until it is let go of.
const CARET_HANDLE_FADE: Duration = Duration::from_secs(5);

/// The handle colour where neither the field nor the platform names one: the
/// focus-ring blue, opaque.
const HANDLE_DEFAULT: Rgba = Rgba::new(0x89 as f32 / 255.0, 0xb4 as f32 / 255.0, 0xfa as f32 / 255.0, 1.0);

#[derive(Clone, Copy, Debug, PartialEq)]
enum Handle {
    /// The start of a selection, hanging to the left of it.
    Start,
    /// The end of a selection, hanging to the right.
    End,
    /// Under a caret with nothing selected, pointing straight up at it.
    Caret,
}

impl Handle {
    /// The middle of the round part, for a handle hanging from the text point
    /// `(x, y)`: the bottom of the line at its end of the selection.
    fn centre(self, (x, y): (f32, f32)) -> (f32, f32) {
        match self {
            Handle::Start => (x - HANDLE_R, y + HANDLE_R),
            Handle::End => (x + HANDLE_R, y + HANDLE_R),
            Handle::Caret => (x, y + HANDLE_R * std::f32::consts::SQRT_2),
        }
    }

    /// How far `(fx, fy)` is from this handle's middle, or `None` when it is
    /// out of a finger's reach.
    fn reach(self, point: (f32, f32), (fx, fy): (f32, f32)) -> Option<f32> {
        let (cx, cy) = self.centre(point);
        let distance = (fx - cx).hypot(fy - cy);
        (distance <= HANDLE_REACH).then_some(distance)
    }
}

/// A handle hanging from `point`, in logical px.
///
/// Each is a square with the corner that touches the text left sharp and the
/// other three rounded to a circle, which is the whole of Android's teardrop.
/// The caret's is the same square turned 45 degrees, so its point is on top.
fn handle_paints(handle: Handle, (x, y): (f32, f32), color: Rgba) -> Vec<Paint> {
    let d = HANDLE_R * 2.0;
    let r = HANDLE_R;
    let square = |x: f32, radius: [f32; 4]| {
        Paint::Rect(PaintRect {
            x,
            y,
            width: d,
            height: d,
            background: Some(Background::Color(color)),
            radius,
            border: Sides::ZERO,
            border_color: None,
        })
    };
    match handle {
        // Radii run top-left, top-right, bottom-right, bottom-left.
        Handle::Start => vec![square(x - d, [r, 0.0, r, r])],
        Handle::End => vec![square(x, [0.0, r, r, r])],
        Handle::Caret => {
            // The End shape turned about its sharp corner, which carries the
            // corner-to-corner diagonal from pointing down-right to pointing
            // straight down.
            let (s, c) = std::f32::consts::FRAC_PI_4.sin_cos();
            vec![
                Paint::PushTransform([c, s, -s, c, x - c * x + s * y, y - s * x - c * y]),
                square(x, [0.0, r, r, r]),
                Paint::PopTransform,
            ]
        }
    }
}

/// One end of the focused field's selection as the last frame drew it, in
/// logical px: its x, the bottom of its line, the line's height, and whether
/// the point is inside the field's box rather than scrolled out of it.
///
/// Measured in [`App::render`], from the text paint the frame actually draws,
/// and kept for the touch that follows, so a handle is hit where it is seen.
#[derive(Clone, Copy, Debug)]
struct SelectionEnd {
    x: f32,
    bottom: f32,
    height: f32,
    visible: bool,
}

/// A handle a finger is holding.
#[derive(Clone, Copy, Debug)]
struct HandleDrag {
    handle: Handle,
    /// The other end of the selection, which stays put. The caret's own index
    /// for the caret handle.
    fixed: usize,
    /// From the finger to the point the handle hangs from, so the handle does
    /// not jump to put its tip under the finger when the drag starts.
    grab: (f32, f32),
    /// Where the finger went down, physical px.
    from: (f64, f64),
    /// Past the tap slop. A caret handle that never moved was tapped, which
    /// opens the menu.
    moved: bool,
}

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
        border: Sides::uniform(1.0),
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
                border: Sides::ZERO,
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
                border: Sides::ZERO,
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
                preedit: None,
                selection_style: Default::default(),
            },
        }));
    }
    out
}

// ── Accessibility ───────────────────────────────────────────────────────────

/// The accessibility tree's root. Element ids follow it, offset by one, so an
/// element's id is stable for a given position in document order.
#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
const ACCESS_ROOT: NodeId = NodeId(0);

#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
fn to_accesskit_role(role: AccessRole) -> Role {
    match role {
        AccessRole::Label => Role::Label,
        AccessRole::Heading => Role::Heading,
        AccessRole::Button => Role::Button,
        AccessRole::CheckBox => Role::CheckBox,
        AccessRole::RadioButton => Role::RadioButton,
        AccessRole::Switch => Role::Switch,
        AccessRole::Slider => Role::Slider,
        AccessRole::DateInput => Role::DateInput,
        AccessRole::TextInput => Role::TextInput,
        AccessRole::MultilineTextInput => Role::MultilineTextInput,
        AccessRole::ComboBox => Role::ComboBox,
        AccessRole::Image => Role::Image,
        AccessRole::Link => Role::Link,
        AccessRole::ScrollView => Role::ScrollView,
        // A grouping the author marked with `role=`, and the unreachable None.
        AccessRole::Group | AccessRole::None => Role::Group,
    }
}

/// Build the accessibility tree for the current frame: a window root with one
/// child per meaningful element, carrying its role, name, value, checked state
/// and on-screen bounds.
///
/// Rebuilt per frame rather than diffed, at these tree sizes it is cheap, and
/// the alternative (tracking node identity across reconciles) is exactly the kind
/// of parallel bookkeeping that goes stale. Geometry is in *physical* pixels,
/// which is what the platform expects.
#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
fn access_tree(nodes: &[AccessNode], focused_model: Option<&str>, scale: f64, title: &str) -> TreeUpdate {
    let mut root = AccessKitNode::new(Role::Window);
    root.set_label(title.to_string());

    let mut updates = Vec::with_capacity(nodes.len() + 1);
    let mut children = Vec::with_capacity(nodes.len());
    let mut focus = ACCESS_ROOT;

    for (i, node) in nodes.iter().enumerate() {
        let id = NodeId(i as u64 + 1);
        children.push(id);

        let mut ak = AccessKitNode::new(to_accesskit_role(node.access.role));
        if let Some(label) = node.access.name() {
            // Static text is the exception: accesskit reads a `Role::Label`'s
            // name from its *value* (`label_comes_from_value`), so setting the
            // label there leaves it nameless, which is what a UIA client saw
            // before this line existed.
            if node.access.role == AccessRole::Label {
                ak.set_value(label.to_string());
            } else {
                ak.set_label(label.to_string());
            }
        }
        if let Some(value) = &node.access.value {
            ak.set_value(value.clone());
        }
        if let Some(checked) = node.access.checked {
            ak.set_toggled(if checked { Toggled::True } else { Toggled::False });
        }
        // Bounds let a screen reader's cursor track the element on screen.
        ak.set_bounds(accesskit::Rect {
            x0: node.x as f64 * scale,
            y0: node.y as f64 * scale,
            x1: (node.x + node.width) as f64 * scale,
            y1: (node.y + node.height) as f64 * scale,
        });
        // Anything a user can operate is reachable; static text is not a stop.
        if matches!(
            node.access.role,
            AccessRole::Button
                | AccessRole::Link
                | AccessRole::CheckBox
                | AccessRole::RadioButton
                | AccessRole::Switch
                | AccessRole::Slider
                | AccessRole::DateInput
                | AccessRole::TextInput
                | AccessRole::MultilineTextInput
                | AccessRole::ComboBox
        ) {
            ak.add_action(accesskit::Action::Focus);
            ak.add_action(accesskit::Action::Click);
        }
        // Keep the platform's focus in step with ours, so a screen reader follows
        // the caret instead of announcing a stale element.
        if let (Some(model), Some(focused)) = (&node.model, focused_model) {
            if model == focused {
                focus = id;
            }
        }
        updates.push((id, ak));
    }

    root.set_children(children);
    let mut tree = Tree::new(ACCESS_ROOT);
    tree.toolkit_name = Some("Rux".into());
    tree.toolkit_version = Some(env!("CARGO_PKG_VERSION").into());
    let mut tree_update = TreeUpdate {
        nodes: vec![(ACCESS_ROOT, root)],
        tree: Some(tree),
        // We publish one window-level tree, never a subtree graft.
        tree_id: accesskit::TreeId::ROOT,
        focus,
    };
    tree_update.nodes.extend(updates);
    tree_update
}

// ── Dev overlay ─────────────────────────────────────────────────────────────

const OVERLAY_PAD: f32 = 16.0;
const OVERLAY_LINE_H: f32 = 20.0;
const OVERLAY_TITLE_H: f32 = 26.0;
/// Warnings listed before the panel stops and says how many are left.
const OVERLAY_MAX_WARNINGS: usize = 6;
/// Printed lines get their own cap, so a chatty `print` in a binding cannot
/// crowd out the warnings that share the panel with it.
const OVERLAY_MAX_PRINTS: usize = 6;

/// Paint items for the dev overlay: what is wrong with the document, drawn over
/// the app.
///
/// This is the whole point of the feature, a broken `.rux` file used to show an
/// empty window with one line on a stderr nobody running a GUI is watching. An
/// error takes a red panel and says the screen is stale; warnings take a quieter
/// amber one, since the app underneath is fine.
/// The painted overlay, and where it ended up.
struct Overlay {
    paints: Vec<Paint>,
    /// The panel's box in logical px, so a tap on it can dismiss it. Kept beside
    /// the paints rather than recomputed, since a hit region that disagrees with
    /// what was drawn is the kind of bug that only shows up under a resize.
    rect: (f32, f32, f32, f32),
}

fn overlay_paints(diag: &rux_runtime::Diagnostics, path: &Path, width: f32) -> Option<Overlay> {
    if diag.is_empty() {
        return None;
    }
    let error_bg = Rgba::new(0.24, 0.09, 0.13, 0.97); // deep red
    let error_edge = Rgba::new(0.95, 0.35, 0.42, 1.0); // #f38ba8-ish
    let warn_bg = Rgba::new(0.20, 0.17, 0.10, 0.97); // deep amber
    let warn_edge = Rgba::new(0.98, 0.70, 0.35, 1.0); // #fab387-ish
    let ink = Rgba::new(0.95, 0.95, 0.97, 1.0);
    let muted = Rgba::new(0.78, 0.78, 0.84, 1.0);

    // A panel that is only carrying `print` output is not reporting a problem,
    // so it does not wear a problem's colours. Amber for a document with nothing
    // wrong would train the eye to ignore amber.
    let print_bg = Rgba::new(0.11, 0.14, 0.22, 0.97); // deep slate
    let print_edge = Rgba::new(0.53, 0.71, 0.98, 1.0); // #89b4fa-ish
    let print_ink = Rgba::new(0.72, 0.82, 0.99, 1.0);

    let failed_to_load = diag.error.is_some();
    // The sink carries a level per entry, and the overlay used to read only
    // `diag.error`, which is the *load* failing. So a finding the runtime calls
    // an error — a route naming a view nothing imported, a tag naming nothing —
    // was counted and coloured here as a warning, while `rux check` and the
    // editor's squiggle called the same line an error. One finding, two
    // verdicts, depending on which window you were looking at. Reported
    // 2026-09-15 from exactly that pair of screenshots.
    let errors = diag.warnings.iter().filter(|w| w.is_error()).count();
    let warnings = diag.warnings.len() - errors;
    let (bg, edge) = if failed_to_load || errors > 0 {
        (error_bg, error_edge)
    } else if diag.warnings.is_empty() {
        (print_bg, print_edge)
    } else {
        (warn_bg, warn_edge)
    };

    // Wrap the message text to the panel width so a long error is readable
    // rather than clipped at the edge.
    let panel_w = (width - OVERLAY_PAD * 2.0).max(120.0);
    let text_w = panel_w - OVERLAY_PAD * 2.0;
    let mut lines: Vec<(String, Rgba)> = Vec::new();
    if let Some(error) = &diag.error {
        lines.extend(wrap_overlay(error, text_w).into_iter().map(|l| (l, ink)));
        if diag.stale {
            lines.push((
                "showing the last version that loaded, fix the file and save".to_string(),
                muted,
            ));
        }
    }
    // A document can easily have a dozen unhonored properties; an unbounded panel
    // would grow past the window and hide the app it is describing.
    let shown = diag.warnings.len().min(OVERLAY_MAX_WARNINGS);
    for warning in &diag.warnings[..shown] {
        // A warning raised inside an imported component belongs to that
        // component, and its line number is a line of *that* file. Showing
        // `line 9` under a panel headed with the document being run points
        // confidently at the wrong file, which is worse than saying nothing:
        // the reader trusts it and goes to look. The file is named whenever it
        // is not the one in the title.
        let origin = warning
            .file
            .as_deref()
            .map(file_name)
            .filter(|name| *name != file_name(path));
        let text = match origin {
            Some(name) => format!("• {name} {warning}"),
            None => format!("• {warning}"),
        };
        lines.extend(
            wrap_overlay(&text, text_w)
                .into_iter()
                .map(|l| (l, if failed_to_load { muted } else { ink })),
        );
    }
    if diag.warnings.len() > shown {
        lines.push((
            format!("… and {} more (full list on stderr)", diag.warnings.len() - shown),
            muted,
        ));
    }

    // `print(…)` output, last, so a real problem is never pushed off the top of
    // the panel by debugging chatter. Marked with `›` rather than the warnings'
    // `•`, and inked differently, because the two lists mean opposite things:
    // one is the document telling you it is broken, the other is you asking it a
    // question.
    let prints_shown = diag.prints.len().min(OVERLAY_MAX_PRINTS);
    for line in &diag.prints[..prints_shown] {
        lines.extend(
            wrap_overlay(&format!("› {line}"), text_w).into_iter().map(|l| (l, print_ink)),
        );
    }
    if diag.prints.len() > prints_shown {
        lines.push((
            format!("… and {} more printed (full list on stderr)", diag.prints.len() - prints_shown),
            muted,
        ));
    }

    // Errors and warnings are counted apart, so the panel agrees with what
    // `rux check` prints and with what the editor draws. "1 warning(s)" over a
    // finding the checker calls an error is the whole reason this is a match on
    // two numbers rather than one.
    let counted = match (errors, warnings) {
        (0, 0) => String::new(),
        (0, n) => format!("{n} warning(s)"),
        (n, 0) => format!("{n} error(s)"),
        (e, w) => format!("{e} error(s), {w} warning(s)"),
    };
    let title = match (&diag.error, counted.is_empty(), diag.prints.len()) {
        (Some(_), true, _) => format!("rux: {} failed to load", file_name(path)),
        (Some(_), false, _) => {
            format!("rux: {} failed to load  ·  {counted}", file_name(path))
        }
        (None, true, p) => format!("rux: {p} printed from {}", file_name(path)),
        (None, false, 0) => format!("rux: {counted} in {}", file_name(path)),
        (None, false, p) => format!("rux: {counted}, {p} printed in {}", file_name(path)),
    };
    // The panel covers the app it is describing, and there was no way to move it
    // out of the way. It says so rather than leaving the gesture to be guessed
    // at, and it comes back by itself the moment the diagnostics change.
    lines.push(("tap this panel to dismiss it".to_string(), muted));

    let panel_h = OVERLAY_TITLE_H + lines.len() as f32 * OVERLAY_LINE_H + OVERLAY_PAD * 1.5;
    let x = OVERLAY_PAD;
    let y = OVERLAY_PAD;

    let mut out = Vec::with_capacity(lines.len() + 3);
    out.push(Paint::Shadow {
        x,
        y: y + 3.0,
        width: panel_w,
        height: panel_h,
        radius: 10.0,
        blur: 20.0,
        color: Rgba::new(0.0, 0.0, 0.0, 0.5),
    });
    out.push(Paint::Rect(PaintRect {
        x,
        y,
        width: panel_w,
        height: panel_h,
        background: Some(Background::Color(bg)),
        radius: [10.0; 4],
        border: Sides::uniform(2.0),
        border_color: Some(edge),
    }));
    out.push(Paint::Text(PaintText {
        x: x + OVERLAY_PAD,
        y: y + OVERLAY_PAD * 0.6,
        width: text_w,
        height: OVERLAY_TITLE_H,
        content: overlay_text(title, 15.0, 700, edge),
    }));
    for (i, (line, color)) in lines.into_iter().enumerate() {
        out.push(Paint::Text(PaintText {
            x: x + OVERLAY_PAD,
            y: y + OVERLAY_TITLE_H + OVERLAY_PAD * 0.4 + i as f32 * OVERLAY_LINE_H,
            width: text_w,
            height: OVERLAY_LINE_H,
            content: overlay_text(line, 14.0, 400, color),
        }));
    }
    Some(Overlay { paints: out, rect: (x, y, panel_w, panel_h) })
}

/// Whether the overlay should be on screen: there is something to say, and it
/// has not been dismissed *for these particular diagnostics*.
///
/// Comparing the whole `Diagnostics` rather than holding a flag is what makes
/// the panel come back on its own. Dismissing "3 warnings" and then introducing
/// a parse error must not leave the window silent about it, which a boolean
/// would do until the next restart.
fn overlay_visible(
    diag: &rux_runtime::Diagnostics,
    dismissed: Option<&rux_runtime::Diagnostics>,
) -> bool {
    !diag.is_empty() && dismissed != Some(diag)
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// Break `text` into lines that fit `width`, by character estimate. The overlay
/// paints each line itself (rather than handing one block to the text engine)
/// so the panel's height is known before it is drawn.
fn wrap_overlay(text: &str, width: f32) -> Vec<String> {
    // ~0.52em per character at this size, a deliberate under-estimate, since a
    // slightly short line is invisible and an overlong one is clipped.
    let max_chars = ((width / 7.3) as usize).max(20);
    let mut lines = Vec::new();
    for paragraph in text.split('\n') {
        let mut line = String::new();
        for word in paragraph.split_whitespace() {
            if !line.is_empty() && line.chars().count() + 1 + word.chars().count() > max_chars {
                lines.push(std::mem::take(&mut line));
            }
            if !line.is_empty() {
                line.push(' ');
            }
            line.push_str(word);
        }
        lines.push(line);
    }
    lines
}

fn overlay_text(text: String, font_size: f32, weight: u16, color: Rgba) -> TextContent {
    TextContent {
        text,
        font_size,
        weight,
        color,
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
        preedit: None,
        selection_style: Default::default(),
    }
}

/// Load a `.rux` document. On failure the window still opens, but now it opens
/// showing the error, instead of a blank screen with a line on stderr.
#[cfg(not(target_arch = "wasm32"))]
fn load_document(path: &PathBuf) -> Document {
    match Document::load(path) {
        Ok(doc) => doc,
        Err(err) => {
            eprintln!("rux: failed to load {}: {err}", path.display());
            let mut doc = Document::from_source("<template><screen></screen></template>")
                .expect("empty document");
            doc.set_load_error(err);
            // Nothing was ever shown, so the empty screen isn't "stale", it is
            // simply all there is.
            doc.clear_stale();
            doc
        }
    }
}

/// Per-window render state.
struct RenderState {
    window: Arc<Window>,
    surface: RenderSurface<'static>,
    renderer: Renderer,
    scene: Scene,
    /// Publishes the accessibility tree to the platform (UI Automation on
    /// Windows, AT-SPI on Linux, NSAccessibility on macOS). It only does work
    /// while assistive technology is actually attached, so this costs nothing in
    /// the common case.
    #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
    access: accesskit_winit::Adapter,
}

/// An IME composition in flight: the text between pressing a dead key (or
/// starting to spell a CJK word) and choosing what it becomes.
///
/// The composed text is written straight into the bound signal, so it renders
/// through the ordinary text path and needs no second string that the layout and
/// painter would have to be taught about. A browser does the same thing to an
/// `<input>`'s value while you compose, so an `@input` handler seeing provisional
/// text is the behaviour people already expect.
///
/// What must be remembered separately is how to take it back out again, because
/// a composition can be abandoned as well as committed.
#[derive(Clone, Debug)]
struct Preedit {
    /// Byte offset in the value where the composition starts.
    at: usize,
    /// Byte length of the composed text currently sitting in the value.
    len: usize,
    /// Whatever the composition replaced when it began (composing over a
    /// selection is allowed), put back if it is cancelled rather than committed.
    replaced: String,
}

/// The application: owns the vello render context, the document, the text
/// engine, input state, and (once resumed) one window.
/// Where the surface-setup task leaves its result for `user_event` to collect.
/// A shared cell rather than an event payload because wgpu's handles are `!Send`
/// on wasm while `EventLoopProxy` requires `Send`.
#[cfg(target_arch = "wasm32")]
type Pending = Rc<RefCell<Option<(RenderContext, RenderState)>>>;

struct App {
    context: RenderContext,
    state: Option<RenderState>,
    /// A renderer set aside while the app is suspended, with the device id it
    /// was built for.
    ///
    /// Only Android suspends, but the field is not gated: `resumed` is shared
    /// with the desktop, and one branch there that reads a field which does not
    /// exist on half the targets is worse than a field that is always `None` on
    /// those targets.
    spare_renderer: Option<(usize, Renderer)>,
    /// Proxy for events raised outside the loop: the file watcher and the
    /// accessibility adapter both deliver through it.
    #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
    proxy: winit::event_loop::EventLoopProxy<RuxEvent>,
    /// Set while the async surface setup is in flight, so `resumed` firing twice
    /// doesn't start a second one.
    #[cfg(target_arch = "wasm32")]
    pending: Pending,
    #[cfg(target_arch = "wasm32")]
    starting: bool,
    /// The file behind the document. Native only, it titles the window and is
    /// what the watcher re-reads; on the web there is no file, and new source
    /// arrives as text.
    #[cfg(not(target_arch = "wasm32"))]
    path: PathBuf,
    /// The device this window is pretending to be, from `--preview`.
    ///
    /// It supplies the two answers a desktop window cannot give honestly: the
    /// density of a screen nobody is looking at, and the insets of a display
    /// with something in front of it. The viewport is **not** taken from it:
    /// that stays the window's own logical size, so `@media (max-width: …)`
    /// keeps telling the truth and dragging the window edge still works.
    preview: Option<DeviceProfile>,
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
    /// Whether the last thing the person did was on the keyboard rather than
    /// with a pointer or a finger. It decides whether a focused button shows
    /// its focus: see [`App::sync_focus_state`].
    keyboard_modality: bool,
    /// Whether Shift is held (Shift+Tab reverse traversal; Shift+arrows extend a
    /// selection; Shift+wheel scrolls sideways).
    shift_held: bool,
    /// Whether Ctrl is held (Ctrl+A/C/X/V).
    ctrl_held: bool,
    /// Whether Alt is held (Alt+Left/Right walk the router's history).
    alt_held: bool,
    /// Scrollable regions from the most recent layout.
    scrolls: Vec<ScrollRegion>,
    /// Boxes styled by `:hover`/`:active`, from the most recent layout. Empty
    /// unless the document actually uses a pointer-state rule.
    states: Vec<StateRegion>,
    /// Every laid-out node's box, keyed by tree path, from the most recent
    /// layout. What turns a `query()` handle into a point, so `tap()` can be
    /// dispatched through the same coordinate path a finger takes.
    metrics: Vec<rux_layout::NodeMetrics>,
    /// Scroll offset per scrollable box, in tree order. Survives the rebuild
    /// that follows every state change, so a list doesn't jump back to the top
    /// when you tap something in it.
    offsets: Vec<Offset>,
    /// The scrollbar thumb being dragged, if any.
    bar_drag: Option<BarDrag>,
    /// Where the finger last was during a touch drag, in logical px.
    touch: Option<(f32, f32)>,
    /// Recent finger positions with the time each arrived, for the lift
    /// velocity. Trimmed to `FLING_SAMPLE_MS`, so it stays a handful of entries
    /// and never grows with the length of a drag.
    touch_track: Vec<(Instant, (f32, f32))>,
    /// Which scroller the current touch drag actually moved, if any.
    ///
    /// Recorded rather than recomputed at lift because by then the finger is
    /// gone. It is also the test for whether a fling is allowed at all: a lift
    /// that never scrolled anything has nothing to throw.
    scroll_target: Option<usize>,
    /// A scroller coasting after the finger left. `None` when nothing is.
    fling: Option<Fling>,
    /// The pointer handlers of whatever is being pressed, and what has happened
    /// to that press so far. `None` when nothing is held, or when the press
    /// landed on something with no pointer handlers at all.
    gesture: Option<GesturePress>,
    /// When the held press becomes a long one, if anything is listening.
    gesture_deadline: Option<Instant>,
    /// Every finger currently down, in logical px, in the order they landed.
    ///
    /// A list rather than one position, because a hand has more than one finger
    /// and a handler is handed all of them. The mouse contributes a single point
    /// with id 0 while a button is held, so a handler written for a phone reads
    /// the same on a desktop.
    points: Vec<(u64, (f32, f32))>,
    /// The `r-model` of the currently focused input, if any.
    focused: Option<String>,
    /// The `r-key` of the row that input is in, when it is inside an `r-for`.
    /// The model repeats across a list's rows, so this is the half that says
    /// which row, and it is what keeps the caret with its row when the list is
    /// reordered.
    focused_row: Option<String>,
    /// The component instance that input was written in, when it is inside one.
    ///
    /// The scope its model is read and written in. Without it every keystroke
    /// was assigned to a document signal of the same name, which in a component
    /// is nothing at all: the field stayed empty and typing did nothing.
    focused_instance: Option<String>,
    /// Which text field has focus: a textarea takes Enter as a newline, a
    /// password refuses copy and cut. See [`InputKind`].
    focused_kind: InputKind,
    /// The focused field's attributes and handlers, taken when it gained
    /// focus. Kept rather than looked up, because `@blur` and `@change` belong
    /// to the field being *left*, whose region may be gone by then.
    focused_field: Field,
    /// The focused field's value when it was focused or last committed:
    /// `@change` fires when the value being committed differs from this.
    committed: Option<String>,
    /// The focused field's value when it was focused, so leaving it can tell
    /// whether the person changed it. See `:user-invalid`.
    focus_value: Option<String>,
    /// The window changed size while a field had focus: reveal it after the
    /// next layout. See watchlist #19.
    reveal_focus: bool,
    /// What the last reveal did to its scroller: `(scroller id, where it was,
    /// where the reveal put it)`. Undone when the keyboard closes, if nobody
    /// has scrolled since, so the page goes back to where the person had it.
    keyboard_reveal: Option<(usize, rux_layout::Offset, rux_layout::Offset)>,
    /// A `type="number"` or `type="date"` part way through being typed: the
    /// text it shows, and the bound value as text when that was written. See
    /// [`App::focused_value`].
    typed_draft: Option<(String, String)>,
    /// The person's decimal separator, `,` or `.`, which decides what a
    /// number field's comma means. See [`parse_number`].
    decimal: char,
    /// `@input`, `@change`, `@focus` and `@blur` waiting to run: handler,
    /// instance, and the `event` it is handed. See [`App::flush_field_events`].
    field_events: std::collections::VecDeque<(String, Option<String>, rux_reactive::Value)>,
    /// The `autofocus` fields that were in the last frame, so one takes focus
    /// when it *appears* and not on every frame it stays. See
    /// [`App::adopt_autofocus`].
    autofocus_seen: Vec<(String, Option<String>, Option<String>)>,
    /// The focus Android kept when it killed the app, waiting for the first
    /// frame that has fields to put it in: `Some(None)` when nothing had
    /// focus. See [`App::adopt_restored_focus`].
    #[cfg(any(target_os = "android", target_arch = "wasm32"))]
    restored_focus: Option<Option<rux_runtime::SavedFocus>>,
    /// The text Android kept for every other field, waiting for the same
    /// frame. See [`App::adopt_restored_fields`].
    #[cfg(any(target_os = "android", target_arch = "wasm32"))]
    restored_fields: Vec<rux_runtime::SavedField>,
    /// A dev build's documents, as the app last loaded them, for hot reload
    /// to patch. `None` in a release build, which never reloads.
    #[cfg(target_os = "android")]
    dev_files: Option<rux_runtime::MemorySource>,
    /// What the input method's connection holds for the focused field: its
    /// text, caret and anchor, as bytes. See [`App::sync_android_ime`].
    #[cfg(target_os = "android")]
    ime_mirror: Option<(String, usize, usize)>,
    /// The toolbar is up at a caret, with nothing selected: a long press found
    /// no word to take. Cleared by the next change of focus or caret.
    caret_menu: bool,
    /// The selection handles belong to this focus: a finger placed its caret
    /// or made its selection. A mouse press, or a change of field, puts them
    /// away, because a handle under a mouse pointer is something to aim at for
    /// no reason.
    handles: bool,
    /// When the lone caret handle goes, if it is up. See [`CARET_HANDLE_FADE`].
    caret_handle_until: Option<Instant>,
    /// The handle a finger is dragging, if one is.
    handle_drag: Option<HandleDrag>,
    /// The word a long press or double tap took, as byte offsets, while the
    /// finger that took it is still down. See [`App::extend_from_word`].
    pressed_word: Option<(usize, usize)>,
    /// The focused field's selection start and end as last drawn, the caret
    /// twice when nothing is selected. See [`SelectionEnd`].
    selection_ends: Option<[SelectionEnd; 2]>,
    /// The colour handles take when the field names none: the platform's accent
    /// where there is one to read.
    handle_accent: Rgba,
    /// Whether the platform's highlight and accent have been read yet.
    #[cfg(target_os = "android")]
    theme_read: bool,
    /// The platform text menu as last asked of Java, so it is asked again only
    /// when something about it changed. See [`App::sync_text_menu`].
    #[cfg(target_os = "android")]
    text_menu_sent: Option<TextMenu>,
    /// The selection the platform text menu was closed over without the
    /// selection going, as `(anchor, caret)`: it stays closed until the
    /// selection moves.
    #[cfg(target_os = "android")]
    text_menu_dismissed: Option<(usize, usize)>,
    /// The currently open `select` dropdown, as `(r-model, row key)`. Survives
    /// the rebuild after a state change, like scroll offsets.
    ///
    /// The row is half the identity, for the same reason an input needs one: the
    /// model is recorded as written, so every row of an `r-for` carries the same
    /// one. Keyed by the model alone, tapping row three's select opened row
    /// one's dropdown, drew it over row one, hit-tested the options against row
    /// one's box, and wrote the chosen option into row one.
    open_select: Option<(String, Option<String>, Option<String>)>,
    /// The `<select>` a platform picker is currently open for.
    ///
    /// Android only, and separate from [`App::open_select`] because the two are
    /// different things: `open_select` is a dropdown Rux is drawing and can hit
    /// test, this is a dialog the platform owns and will answer later. Held so
    /// the answer reaches the field that asked, since the tree may have been
    /// rebuilt while the picker was up.
    #[cfg(any(target_os = "android", target_arch = "wasm32"))]
    pending_select: Option<(String, Option<String>, Option<String>, Vec<String>)>,
    /// The `type="date"` a platform date picker is open for: its model, row,
    /// instance and `@change`, held for the same reason as `pending_select`.
    #[cfg(any(target_os = "android", target_arch = "wasm32"))]
    pending_date: Option<(String, Option<String>, Option<String>, Option<String>)>,
    /// Caret position in the focused input, as a byte index into its value.
    caret: usize,
    /// Where the current selection started, as a byte index. Equal to `caret`
    /// when nothing is selected, the selection is the range between them.
    anchor: usize,
    /// The diagnostics whose overlay has been dismissed, if any. Held as the
    /// diagnostics themselves rather than a flag so that the panel reappears the
    /// moment what is wrong with the document changes: dismissing "3 warnings"
    /// must not also hide the error you introduce next.
    overlay_dismissed: Option<rux_runtime::Diagnostics>,
    /// Where the overlay was drawn last frame, in logical px, for hit testing.
    /// `None` when it is not on screen.
    overlay_rect: Option<(f32, f32, f32, f32)>,
    /// The IME composition in flight, if any. `None` covers every keyboard that
    /// commits directly, which is most of them most of the time.
    preedit: Option<Preedit>,
    /// Whether the pointer is selecting text by dragging inside an input.
    text_drag: bool,
    /// The touch text gesture in progress, if a finger is down on a field. The
    /// mouse does not use this: it keeps `text_drag`, since drag-to-select is
    /// the right model with a pointer.
    touch_text: Option<TouchText>,
    /// How far the focused *single-line* input's text is scrolled left, in
    /// logical px.
    ///
    /// A textarea is `overflow: scroll` and gets a real scroll region, which is
    /// what `scroll_caret_into_view` moves. An input is `overflow: clip`: it has
    /// no scroll region, so nothing ever kept its caret inside the box and the
    /// caret was simply clipped away past the right edge. This is the offset
    /// that was missing. Held for the focused field only, and reset when focus
    /// moves.
    text_scroll: f32,
    /// When and where the last click landed, for double-click word-select.
    last_click: Option<(Instant, f64, f64)>,
    /// The system clipboard. `None` if the platform wouldn't give us one, the
    /// app still runs, copy/paste just does nothing. Absent on the web, where
    /// the clipboard is async and permission-gated; same "copy/paste does
    /// nothing" outcome, reached without a field.
    #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
    clipboard: Option<arboard::Clipboard>,
    /// Whether the caret is in the visible half of its blink cycle.
    caret_visible: bool,
    /// When the caret next toggles. `None` when no input is focused, so an idle
    /// window stays fully event-driven with no timer.
    blink_deadline: Option<Instant>,
    /// Per-node memory of what each transitioned property is showing, and the
    /// only thing that knows a style change should be walked rather than jumped.
    ///
    /// It lives here rather than in the document for the reason every other
    /// per-frame fact does: a build throws the tree away and rebuilds it, so
    /// anything remembering the *previous* frame has to outlive the tree.
    anim: rux_runtime::Animator,
    /// When the next animation frame is due. `None` when nothing is animating,
    /// which is what lets an idle app go back to waiting on real events.
    anim_deadline: Option<Instant>,
    /// The zero the animator's clock counts from. The animator takes a plain
    /// `f64` of milliseconds rather than an `Instant` so it stays testable and
    /// works unchanged on the web.
    epoch: Instant,
    /// Current pointer position (physical pixels).
    pointer: (f64, f64),
    /// Where the left button was pressed, if it is currently down.
    press: Option<(f64, f64)>,
    /// The cursor icon currently set on the window, so a mouse-move only calls
    /// `set_cursor` when the shape actually changes.
    cursor: CursorIcon,
    /// The history position the browser's URL bar was last told about, as
    /// `(index, route)`.
    ///
    /// Kept so the shell can tell a move it made itself from one the user made
    /// with the browser's own Back button. Applying a `popstate` updates this
    /// too, which is what stops the echo: after it, the document and the URL
    /// already agree, so there is nothing left to write.
    #[cfg(target_arch = "wasm32")]
    mirrored: Option<(usize, String)>,
}

impl App {
    /// Build the app. Native loads the document from `path`; the web is handed
    /// one already parsed, because it has no filesystem to load it from.
    fn new(
        #[cfg(not(target_arch = "wasm32"))] path: PathBuf,
        #[cfg(not(any(target_arch = "wasm32", target_os = "android")))] proxy: winit::event_loop::EventLoopProxy<RuxEvent>,
        #[cfg(target_arch = "wasm32")] document: Document,
    ) -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        let document = load_document(&path);
        Self {
            context: RenderContext::new(),
            state: None,
            spare_renderer: None,
            #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
            proxy,
            #[cfg(target_arch = "wasm32")]
            pending: Rc::new(RefCell::new(None)),
            #[cfg(target_arch = "wasm32")]
            starting: false,
            #[cfg(not(target_arch = "wasm32"))]
            path,
            preview: None,
            document,
            text: rux_text::TextEngine::new(),
            images: rux_paint::ImageCache::new(),
            hits: Vec::new(),
            focuses: Vec::new(),
            selects: Vec::new(),
            focusables: Vec::new(),
            focus_index: None,
            keyboard_modality: false,
            shift_held: false,
            ctrl_held: false,
            alt_held: false,
            scrolls: Vec::new(),
            offsets: Vec::new(),
            bar_drag: None,
            gesture: None,
            gesture_deadline: None,
            points: Vec::new(),
            touch: None,
            touch_track: Vec::new(),
            scroll_target: None,
            fling: None,
            focused: None,
            focused_row: None,
            focused_instance: None,
            focused_kind: InputKind::Text,
            focused_field: Field::default(),
            committed: None,
            focus_value: None,
            reveal_focus: false,
            keyboard_reveal: None,
            typed_draft: None,
            decimal: '.',
            field_events: std::collections::VecDeque::new(),
            autofocus_seen: Vec::new(),
            #[cfg(any(target_os = "android", target_arch = "wasm32"))]
            restored_focus: None,
            #[cfg(any(target_os = "android", target_arch = "wasm32"))]
            restored_fields: Vec::new(),
            #[cfg(target_os = "android")]
            dev_files: None,
            #[cfg(target_os = "android")]
            ime_mirror: None,
            caret_menu: false,
            handles: false,
            caret_handle_until: None,
            handle_drag: None,
            pressed_word: None,
            selection_ends: None,
            handle_accent: HANDLE_DEFAULT,
            #[cfg(target_os = "android")]
            theme_read: false,
            #[cfg(target_os = "android")]
            text_menu_sent: None,
            #[cfg(target_os = "android")]
            text_menu_dismissed: None,
            open_select: None,
            #[cfg(any(target_os = "android", target_arch = "wasm32"))]
            pending_select: None,
            #[cfg(any(target_os = "android", target_arch = "wasm32"))]
            pending_date: None,
            caret: 0,
            anchor: 0,
            overlay_dismissed: None,
            overlay_rect: None,
            preedit: None,
            text_drag: false,
            touch_text: None,
            text_scroll: 0.0,
            last_click: None,
            #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
            clipboard: arboard::Clipboard::new()
                .map_err(|e| eprintln!("rux: no clipboard ({e}), so copy/paste is disabled"))
                .ok(),
            caret_visible: true,
            blink_deadline: None,
            anim: rux_runtime::Animator::new(),
            anim_deadline: None,
            epoch: Instant::now(),
            pointer: (0.0, 0.0),
            press: None,
            cursor: CursorIcon::Default,
            states: Vec::new(),
            metrics: Vec::new(),
            #[cfg(target_arch = "wasm32")]
            mirrored: None,
        }
    }

    /// Keep the browser's URL bar showing where the document actually is.
    ///
    /// Called once per frame rather than at each place that can navigate, so a
    /// handler that navigates more than once produces the single move it
    /// amounts to instead of one per call.
    ///
    /// Which of the three moves it is falls out of comparing the document's
    /// history position with the last one written:
    ///
    /// - **further along** means a new page was visited: push an entry.
    /// - **further back** means the app went back or forward itself (Alt+Left,
    ///   a mouse side button, a `back()` in a handler): walk the browser by the
    ///   same number of entries, so its own Back button stays in step. The
    ///   `popstate` that answers is recognised as an echo and does nothing.
    /// - **the same index, a different route** means a navigation that replaced
    ///   where we were, which is what going back and then somewhere new looks
    ///   like once the two moves are collapsed into one frame.
    #[cfg(target_arch = "wasm32")]
    fn sync_url(&mut self) {
        if WEB_BASE.with(|b| b.borrow().is_none()) {
            return;
        }
        let (index, _) = self.document.history_position();
        let route = self.document.location().to_string();
        let Some((was, ref was_route)) = self.mirrored else {
            // The tab's first entry is one the browser made, not us. Rewrite it
            // in place so it carries an index like every other entry, or a Back
            // that lands on it would arrive with nothing to say where it is.
            web_write_history(index, &route, true);
            self.mirrored = Some((index, route));
            return;
        };
        if was == index {
            if *was_route != route {
                web_write_history(index, &route, true);
                self.mirrored = Some((index, route));
            }
            return;
        }
        if index > was {
            web_write_history(index, &route, false);
        } else if let Some(window) = web_sys::window() {
            if let Ok(history) = window.history() {
                let _ = history.go_with_delta(index as i32 - was as i32);
            }
        }
        self.mirrored = Some((index, route));
    }

    /// The browser's Back or Forward moved the tab; move the document to match.
    ///
    /// The index is the one Rux stamped on that entry, so a jump of any size is
    /// one call. An entry with no index is one Rux never pushed, which happens
    /// when the tab's own first entry is reached; the URL is then the only
    /// statement of where we are, so it is read back off the location.
    #[cfg(target_arch = "wasm32")]
    fn apply_web_route(&mut self, index: Option<usize>) {
        let moved = match index {
            Some(index) => self.document.go_to(index),
            None => match web_route_now() {
                Some(route) => self.document.open_link(&route, false),
                None => false,
            },
        };
        // Recorded whether or not anything moved: the browser is where it is
        // either way, and the point of this is that the next frame agrees with
        // it instead of trying to correct it back.
        self.mirrored =
            Some((self.document.history_position().0, self.document.location().to_string()));
        if moved {
            self.request_redraw();
        }
    }

    /// Re-load the document after a file change. On a parse/load error the last
    /// good tree stays on screen and the dev overlay reports the error, so a typo
    /// mid-edit neither blanks the window nor passes unnoticed.
    ///
    /// Android reloads too, from files `rux run --device` pushed; see
    /// [`RuxEvent::DevFiles`].
    #[cfg(not(target_arch = "wasm32"))]
    fn reload(&mut self) {
        match Document::load(&self.path) {
            Ok(doc) => {
                // Keeps the window's own state (viewport, hover) and drops the
                // previous error, so fixing the file clears the overlay.
                let was = self.document.location().to_string();
                self.document.replace_with(doc);
                // A reloaded document starts at `/`, so without this, saving a
                // file while looking at a page other than the first one sent
                // the window home and the edit could not be seen. The history
                // behind it is genuinely gone: the reloaded document is a new
                // one, and claiming it was visited would be a lie.
                if was != rux_runtime::ROOT_PATH {
                    self.document.start_at(&was);
                }
                eprintln!("reloaded {}", self.path.display());
            }
            Err(err) => {
                eprintln!("rux: reload failed for {}: {err}", self.path.display());
                // The last good tree stays on screen; the overlay explains why it
                // is no longer what the file says.
                self.document.set_load_error(err);
            }
        }
    }

    /// Rebuild from new source text, the web's equivalent of a file save.
    ///
    /// A parse error keeps the previous document on screen rather than blanking
    /// the canvas, which matters in a playground where the source is mid-edit
    /// most of the time. The error goes to the console for now; surfacing it in
    /// the page is what v0.4's dev overlay is for.
    #[cfg(target_arch = "wasm32")]
    fn set_source(&mut self, source: String) {
        match Document::from_source(&source) {
            Ok(doc) => {
                self.document = doc;
                self.focused = None;
                self.focus_index = None;
                self.open_select = None;
            }
            Err(err) => web_sys::console::error_1(&format!("rux: {err}").into()),
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
        // Remembered for the fling: at lift there is no finger left to hit-test
        // with, and the box that was being scrolled is the only one that may be
        // thrown.
        self.scroll_target = Some(id);
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

    /// Record where the finger is now, for the lift velocity, and drop samples
    /// that have aged out of the window.
    fn track_touch(&mut self, here: (f32, f32)) {
        let now = Instant::now();
        self.touch_track.push((now, here));
        let window = Duration::from_secs_f32(FLING_SAMPLE_MS / 1000.0);
        // Keep one sample older than the window so a slow drag, which may have
        // only two samples in 100 ms, still has a pair to measure between.
        let cut = self
            .touch_track
            .iter()
            .rposition(|(t, _)| now.duration_since(*t) > window);
        if let Some(i) = cut {
            self.touch_track.drain(..i);
        }
    }

    /// Start a fling if the finger left fast enough, having actually scrolled
    /// something. Called once, as the finger lifts.
    fn start_fling(&mut self) {
        let Some(id) = self.scroll_target.take() else { return };
        let (Some((t0, p0)), Some((t1, p1))) =
            (self.touch_track.first().copied(), self.touch_track.last().copied())
        else {
            return;
        };
        let ms = t1.duration_since(t0).as_secs_f32() * 1000.0;
        // Two samples at the same instant say nothing about speed, and dividing
        // by that interval is how a flick becomes infinitely fast.
        if ms < 1.0 {
            return;
        }
        // The content moves opposite to the finger, which is the same sense the
        // drag itself used: `scroll_at` is handed `last - now`.
        let (vx, vy) = ((p0.0 - p1.0) / ms, (p0.1 - p1.1) / ms);
        if vx.hypot(vy) < FLING_MIN_V {
            return;
        }
        self.fling = Some(Fling { id, vx, vy, last: Instant::now() });
    }

    /// Advance a running fling. Returns whether anything moved, so the caller
    /// knows whether to ask for another frame.
    ///
    /// The integral of the decay is used rather than "velocity times elapsed",
    /// so a long frame travels exactly as far as several short ones would: with
    /// `v = v0 * e^(-t/tau)`, the distance over a step is
    /// `v0 * tau * (1 - e^(-dt/tau))`.
    fn step_fling(&mut self) -> bool {
        let Some(mut f) = self.fling else { return false };
        let now = Instant::now();
        let dt = now.duration_since(f.last).as_secs_f32() * 1000.0;
        if dt <= 0.0 {
            return false;
        }
        let (dx, vx) = fling_step(f.vx, dt);
        let (dy, vy) = fling_step(f.vy, dt);
        f.vx = vx;
        f.vy = vy;
        f.last = now;

        let Some(max) = self.scrolls.iter().find(|s| s.id == f.id).map(|s| s.max) else {
            // The tree rebuilt and this scroller is gone. Nothing to move, and
            // nothing to keep waking up for.
            self.fling = None;
            return false;
        };
        let before = self.offsets.get(f.id).copied().unwrap_or_default();
        let next = Offset { x: before.x + dx, y: before.y + dy }.clamp_to(max);
        self.scroll_to(f.id, next);

        // Stop on any of three: too slow to see, or run into an edge on the
        // axis that was carrying it. Hitting the end is a stop rather than a
        // bounce because there is no overscroll to bounce into yet.
        let stalled = f.vx.hypot(f.vy) < FLING_MIN_V;
        let stuck = (next.x - before.x).abs() < f32::EPSILON
            && (next.y - before.y).abs() < f32::EPSILON;
        if stalled || stuck {
            self.fling = None;
            return !stuck;
        }
        self.fling = Some(f);
        true
    }

    /// Whether some scroller under `at` can actually travel on the axis a
    /// gesture is heading down, which is what decides the axis claim.
    ///
    /// `at` is the point the press landed, in logical px, and not where the
    /// finger is now: the question is which box the gesture started inside, and
    /// a fast move can already be outside it by the time the slop is crossed.
    ///
    /// "Can travel" is deliberately stricter than "is a scroller": a box whose
    /// content fits has `max` zero on that axis, and taking a gesture it cannot
    /// use would swallow the drag and then do nothing, which is the worst of
    /// both answers.
    fn scroller_can_take(&self, at: (f32, f32), vertical: bool) -> bool {
        self.scrolls
            .iter()
            .rev()
            .find(|s| s.contains(at.0, at.1) && s.scrollable())
            .is_some_and(|s| if vertical { s.max.y > 0.0 } else { s.max.x > 0.0 })
    }

    /// Start a scrollbar drag if the press landed on a thumb. Returns whether it
    /// did, in which case the press is the bar's, not a tap's.
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
    /// **Only the scrollers the element is inside move.** This used to be
    /// guessed from geometry, any scroller the element overlapped sideways,
    /// and a button above a list scrolled the list back to its top when it
    /// took focus. The layout records which scroller holds each element and
    /// each scroller, so the chain is known rather than guessed.
    ///
    /// Geometry here is the *painted* (already-shifted) position from the last
    /// layout, so the adjustment is a plain delta; the next layout re-clamps it.
    fn scroll_focus_into_view(&mut self) {
        let Some(item) = self.focus_index.and_then(|i| self.focusables.get(i)).cloned() else {
            return;
        };
        let mut chain = Vec::new();
        let mut at = item.scroll;
        while let Some(id) = at.filter(|id| !chain.contains(id)) {
            chain.push(id);
            at = self.scrolls.iter().find(|r| r.id == id).and_then(|r| r.within);
        }
        // Outermost first: scrolling an ancestor moves the box inside it, so the
        // inner scroller's own correction must be computed after. A scroller is
        // recorded before the ones inside it, so tree order is that order.
        for r in self.scrolls.clone() {
            if !r.scrollable() || !chain.contains(&r.id) {
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
        let value = self.document.value_in(
            &region.model,
            region.row.as_deref(),
            region.instance.as_deref(),
        );
        match region.text.as_ref() {
            Some(t) if !value.is_empty() => {
                let (tx, ty) = self.text_point(region, t, px, py);
                // A tap lands on a bullet in a masked field, so it is resolved
                // against the painted string and converted back. See
                // `shown_text`.
                let (shown, _) = self.shown_text(&value, 0);
                let hit = self.text.index_at_point(
                    &shown,
                    &rux_paint::text_style(&t.content),
                    Some(t.width),
                    tx,
                    ty,
                );
                self.real_caret(&value, hit)
            }
            _ => 0,
        }
    }

    /// A pointer position in the text's own coordinates, with the field's
    /// horizontal scroll applied.
    ///
    /// Every mapping from a pointer onto a string goes through here: a caret in
    /// [`index_in`](Self::index_in), a word in
    /// [`select_word_at`](Self::select_word_at). They were originally written
    /// separately and one of them missed the scroll, so a long press in a
    /// scrolled field took the word one scroll-distance behind the finger. A
    /// single conversion cannot disagree with itself.
    ///
    /// **A textarea scrolls itself, and that offset is not in `t`.** The layout
    /// records the text box before it shifts the field's own children, so `t.y`
    /// is where the text would be at the top of its scroll. Leaving the offset
    /// out put every tap in a scrolled textarea one scroll-distance *above*
    /// the finger. Reported from the phone: selecting took the line above, and
    /// placing the caret needed aiming, but only once the field had scrolled,
    /// so it seemed to come and go.
    fn text_point(
        &self,
        region: &FocusRegion,
        t: &rux_layout::PaintText,
        px: f32,
        py: f32,
    ) -> (f32, f32) {
        let own = self.own_scroll(region);
        (px - t.x + self.text_scroll_for(region) + own.x, py - t.y + own.y)
    }

    /// Whether the focused field has more text than fits, top to bottom.
    fn focused_scrolls_vertically(&self) -> bool {
        let Some(sid) = self.focused_region().and_then(|r| r.scroll_id) else { return false };
        self.scrolls.iter().any(|s| s.id == sid && s.max.y > 0.0)
    }

    /// How far a scrolling field has scrolled its own text, clamped the way the
    /// layout clamps it when painting. Zero for a field that does not scroll.
    fn own_scroll(&self, region: &FocusRegion) -> Offset {
        let Some(sid) = region.scroll_id else { return Offset::default() };
        let Some(max) = self.scrolls.iter().find(|s| s.id == sid).map(|s| s.max) else {
            return Offset::default();
        };
        self.offsets.get(sid).copied().unwrap_or_default().clamp_to(max)
    }

    /// Update the focused single-line input's horizontal offset so its caret is
    /// inside the visible box, and return the offset to paint with.
    ///
    /// The offset only moves when the caret would otherwise fall outside, which
    /// is what stops the text sliding under a caret that is already visible. It
    /// is also clamped so the field never scrolls past the start, and never
    /// leaves blank space after the end once the text is short enough to fit.
    fn track_caret_x(
        layout: &rux_layout::Layout,
        focused: Option<&str>,
        focused_row: Option<&str>,
        focused_instance: Option<&str>,
        caret: usize,
        scroll: &mut f32,
        text: &mut rux_text::TextEngine,
        document: &mut rux_runtime::Document,
        secret: bool,
    ) -> f32 {
        let Some(model) = focused else {
            *scroll = 0.0;
            return 0.0;
        };
        let Some(region) = layout
            .focuses
            .iter()
            .find(|f| {
                f.model == model
                    && f.row.as_deref() == focused_row
                    && f.instance.as_deref() == focused_instance
            })
        else {
            return *scroll;
        };
        // A textarea has a real scroll region and is handled by
        // `scroll_caret_into_view`; this is only for the clipped single line.
        let (false, Some(t)) = (region.kind.multiline(), region.text.as_ref()) else {
            *scroll = 0.0;
            return 0.0;
        };
        let value = document.value_in(model, focused_row, focused_instance);
        let style = rux_paint::text_style(&t.content);
        // The painted string, for the same reason as everywhere else: a bullet
        // is not as wide as the letter it stands for.
        let (shown, shown_caret) = if secret {
            (rux_layout::mask(&value), rux_layout::masked_offset(&value, caret))
        } else {
            (value.clone(), caret.min(value.len()))
        };
        let (cx, _, _) = text.caret_geometry(&shown, &style, Some(t.width), shown_caret);

        // The text starts inset from the box by its padding and border. Mirroring
        // that inset on the right gives the span actually visible, without the
        // layout having to report a content box it does not currently carry.
        let inset = (t.x - region.x).max(0.0);
        let visible = (region.width - inset * 2.0).max(1.0);

        if cx < *scroll {
            *scroll = cx;
        } else if cx > *scroll + visible {
            *scroll = cx - visible;
        }
        // `None` for the width: the caret is tracked against the text's true
        // length, not a re-wrap at the box width.
        let full = text.measure(&shown, &style, None).0;
        *scroll = scroll.clamp(0.0, (full - visible).max(0.0));
        *scroll
    }

    /// The focused field's box, when it has a selection worth offering actions
    /// on. `None` means no toolbar: nothing focused, or nothing selected.
    ///
    /// Tied to the selection rather than to focus so the strip is not sitting
    /// over the page the whole time an input has a caret in it.
    fn toolbar_field(&self) -> Option<(f32, f32, f32, f32)> {
        // Android has the platform's own. See `App::sync_text_menu`.
        if cfg!(target_os = "android") {
            return None;
        }
        if self.caret == self.anchor && !self.caret_menu {
            return None;
        }
        let region = self.focused_region()?;
        Some((region.x, region.y, region.width, region.height))
    }

    /// The buttons for the focused field as it stands. See [`offered_actions`].
    fn toolbar_actions(&mut self) -> Vec<TextAction> {
        let length = self.focused_value().len();
        let has_text = length > 0;
        let readonly = self.live_field().readonly;
        // Select all when all of it is selected already is a button that does
        // nothing, and Android's own fields leave it out.
        let all = self.selection() == (0, length);
        offered_actions(self.focused_kind.secret(), self.caret != self.anchor, has_text)
            .into_iter()
            .filter(|a| !(all && *a == TextAction::SelectAll))
            // A read-only field can be copied from and nothing else: Cut and
            // Paste would both be edits it is going to refuse.
            .filter(|a| !readonly || matches!(a, TextAction::Copy | TextAction::SelectAll))
            .collect()
    }

    /// The action under `(fx, fy)` in logical px, if the toolbar is up and the
    /// point is on one of its buttons.
    fn toolbar_action_at(&mut self, fx: f32, fy: f32) -> Option<TextAction> {
        let field = self.toolbar_field()?;
        let (_, buttons) = toolbar_layout(field, self.logical_size(), self.safe_top(), &self.toolbar_actions());
        buttons
            .into_iter()
            .find(|(_, bx, by, bw, bh)| fx >= *bx && fx <= bx + bw && fy >= *by && fy <= by + bh)
            .map(|(action, ..)| action)
    }

    /// Whether the toolbar covers `(fx, fy)`, so a press there is not also a
    /// press on whatever is underneath. The same rule the dev overlay follows.
    fn toolbar_covers(&mut self, fx: f32, fy: f32) -> bool {
        let Some(field) = self.toolbar_field() else { return false };
        let ((x, y, w, h), _) = toolbar_layout(field, self.logical_size(), self.safe_top(), &self.toolbar_actions());
        fx >= x && fx <= x + w && fy >= y && fy <= y + h
    }

    /// Run a toolbar action against the focused field.
    fn run_text_action(&mut self, action: TextAction) {
        let Some(model) = self.focused.clone() else { return };
        match action {
            TextAction::Copy => self.copy_selection(),
            TextAction::Cut => self.cut_selection(&model),
            TextAction::Paste => self.request_paste(&model),
            TextAction::SelectAll => self.select_all_text(&model),
        }
        // Copy leaves the selection up, which is what every platform does: you
        // may want to cut what you just copied. The others change it themselves.
        self.request_redraw();
    }

    /// The top safe-area inset in logical px: the status bar on a phone, the
    /// named device's under `--preview`, zero anywhere else. The same inset the
    /// stylesheet reads as `env(safe-area-inset-top)`.
    fn safe_top(&self) -> f32 {
        #[cfg(target_os = "android")]
        {
            let scale = self.state.as_ref().map_or(1.0, |s| s.window.scale_factor());
            android_safe_area(scale).top
        }
        #[cfg(not(target_os = "android"))]
        {
            self.preview.map_or(0.0, |p| p.safe_area.top)
        }
    }

    /// The window in logical px, which the toolbar is kept inside.
    fn logical_size(&self) -> (f32, f32) {
        let Some(state) = self.state.as_ref() else { return (0.0, 0.0) };
        let scale = state.window.scale_factor();
        let size = state.window.inner_size();
        ((size.width as f64 / scale) as f32, (size.height as f64 / scale) as f32)
    }

    /// The horizontal offset in force for `region`, which is zero for anything
    /// but the focused single-line input. A textarea scrolls through its own
    /// scroll region instead, and an unfocused field is never scrolled.
    fn text_scroll_for(&self, region: &FocusRegion) -> f32 {
        let focused = self.focused.as_deref() == Some(region.model.as_str())
            && self.focused_row.as_deref() == region.row.as_deref()
            && self.focused_instance.as_deref() == region.instance.as_deref();
        if focused && !region.kind.multiline() { self.text_scroll } else { 0.0 }
    }

    /// A press inside an input starts a text selection: it drops the caret (and
    /// the anchor) where you clicked, and a drag from there extends it. A second
    /// click in the same spot selects the word instead.
    ///
    /// Returns whether the press was ours, if so it is *not* also dispatched as a
    /// tap on release, since focusing already happened here.
    fn press_text(&mut self, pointer: (f64, f64)) -> bool {
        // An open dropdown floats over everything and gets first refusal.
        if self.open_select.is_some() {
            return false;
        }
        // A press on the toolbar must not move the caret: collapsing the
        // selection is exactly what the button is about to act on. The tap is
        // handled on release, in `dispatch_tap`.
        let (fx, fy) = self.logical(pointer);
        if self.toolbar_covers(fx, fy) {
            return false;
        }
        let Some(region) = self.focuses.iter().rev().find(|f| f.contains(fx, fy)).cloned() else {
            return false;
        };
        // A date on a phone is not typed into: the tap opens the platform's
        // picker, in `dispatch_tap`. Nor in a phone's browser.
        if region.kind == InputKind::Date && touch_first() {
            return false;
        }

        // A tap also moves keyboard focus, so Tab continues from what you clicked.
        self.focus_index = self.focusables.iter().rposition(|f| f.contains(fx, fy));
        self.focused_kind = region.kind;

        let double = self
            .last_click
            .is_some_and(|(at, x, y)| {
                at.elapsed() < DOUBLE_CLICK && (pointer.0 - x).hypot(pointer.1 - y) <= TAP_SLOP
            });
        self.last_click = Some((Instant::now(), pointer.0, pointer.1));

        // Double-click, and double-tap, select the word under the pointer.
        if double && self.select_word_at(pointer) {
            return true;
        }

        let caret = self.index_in(&region, fx, fy);
        self.text_drag = true;
        self.set_focus(Some((region.model, region.row, region.instance, caret)));
        true
    }

    /// Select the word under `pointer`, in whichever field it lands in.
    ///
    /// Shared by double-click and by the touch long press: both mean "take the
    /// word here", and having one implementation is what keeps them agreeing
    /// about where a word ends. Returns whether a word was actually taken, which
    /// is false for an empty field or a press outside any text.
    fn select_word_at(&mut self, pointer: (f64, f64)) -> bool {
        let (fx, fy) = self.logical(pointer);
        let Some(region) = self.focuses.iter().rev().find(|f| f.contains(fx, fy)).cloned() else {
            return false;
        };
        let value = self.document.value_in(
            &region.model,
            region.row.as_deref(),
            region.instance.as_deref(),
        );
        let (Some(t), false) = (&region.text, value.is_empty()) else {
            return false;
        };
        let (tx, ty) = self.text_point(&region, t, fx, fy);
        // **A password is taken whole.** Split at its spaces, a long press
        // showed where the spaces were, which is the one thing a masked field
        // must not tell anyone. Android's own password fields select all
        // here for the same reason.
        let (start, end) = if region.kind.secret() {
            (0, value.len())
        } else {
            self.text.word_at_point(&value, &rux_paint::text_style(&t.content), Some(t.width), tx, ty)
        };
        self.set_focus_range(Some(Focus {
            model: region.model,
            row: region.row,
            instance: region.instance,
            caret: end,
            anchor: start,
            preedit: None,
        }));
        true
    }

    /// Press on text from a *finger*. Unlike the mouse, this does not start a
    /// selection: it moves the caret and arms the long press, so that what the
    /// finger does next decides between dragging the caret and selecting.
    fn press_text_touch(&mut self, pointer: (f64, f64)) -> bool {
        if !self.press_text(pointer) {
            return false;
        }
        // `press_text` set this for the mouse's model; touch resolves the drag
        // itself and must not also be dragging a selection.
        self.text_drag = false;
        // A finger placed this caret, so the handles are the finger's. A plain
        // tap shows none: the caret handle comes with a long press on empty
        // space. See `App::handle_set`.
        self.handles = true;
        // A double-tap has already taken a word, so there is nothing pending.
        self.pressed_word = (self.anchor != self.caret).then(|| self.selection());
        self.touch_text = Some(if self.anchor == self.caret {
            TouchText::Pending { at: pointer, deadline: Instant::now() + LONG_PRESS }
        } else {
            TouchText::Selecting
        });
        true
    }

    /// Which handles are up, before asking whether each end is on screen.
    ///
    /// Both ends while something is selected. With nothing selected, the lone
    /// caret handle, from a long press on empty space until
    /// [`CARET_HANDLE_FADE`] of idleness later, and for as long as a finger
    /// holds it.
    ///
    /// **Not on a plain tap.** It was, as Android's own fields do on some
    /// versions, and the user asked for it gone: a tap puts the caret where
    /// the finger is, and a handle nobody asked for sits over the next field.
    fn handle_set(&mut self) -> Vec<Handle> {
        if !self.handles || self.focused.is_none() {
            return Vec::new();
        }
        if self.caret != self.anchor {
            return vec![Handle::Start, Handle::End];
        }
        let held = self.handle_drag.is_some_and(|d| d.handle == Handle::Caret);
        let up = held || self.caret_handle_until.is_some_and(|until| Instant::now() < until);
        if up {
            vec![Handle::Caret]
        } else {
            Vec::new()
        }
    }

    /// A finger landing on a handle takes it. Returns whether it did, in which
    /// case the press is the handle's and nothing else's.
    ///
    /// When two are in reach, which happens on a one-letter selection, the
    /// nearer wins.
    fn press_handle(&mut self, pointer: (f64, f64)) -> bool {
        let Some(ends) = self.selection_ends else { return false };
        let (fx, fy) = self.logical(pointer);
        let best = self
            .handle_set()
            .into_iter()
            .filter_map(|handle| {
                let end = if handle == Handle::End { ends[1] } else { ends[0] };
                if !end.visible {
                    return None;
                }
                handle.reach((end.x, end.bottom), (fx, fy)).map(|d| (d, handle, end))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0));
        let Some((_, handle, end)) = best else { return false };
        let (start, stop) = self.selection();
        let fixed = match handle {
            Handle::Start => stop,
            Handle::End => start,
            Handle::Caret => self.caret,
        };
        self.handle_drag = Some(HandleDrag {
            handle,
            fixed,
            // Aimed at the middle of the handle's line, not its bottom edge:
            // the bottom of one line is the top of the next, and a lookup
            // there lands on whichever the rounding favours.
            grab: (end.x - fx, end.bottom - end.height / 2.0 - fy),
            from: pointer,
            moved: false,
        });
        self.request_redraw();
        true
    }

    /// Move the end of the selection a finger is holding.
    ///
    /// The other end stays put, and the held end becomes the caret, so a
    /// field that scrolls to keep its caret in view follows the finger. The
    /// ends may cross, which swaps which handle is which, as on Android. They
    /// may not meet: a selection dragged to nothing would take its handles
    /// with it, under the finger that was still holding one.
    fn drag_handle(&mut self, pointer: (f64, f64)) {
        let Some(mut drag) = self.handle_drag else { return };
        if !drag.moved {
            if (pointer.0 - drag.from.0).hypot(pointer.1 - drag.from.1) <= TAP_SLOP {
                return;
            }
            drag.moved = true;
            self.handle_drag = Some(drag);
            self.request_redraw();
        }
        let Some(region) = self.focused_region().cloned() else { return };
        let (fx, fy) = self.logical(pointer);
        let index = self.index_in(&region, fx + drag.grab.0, fy + drag.grab.1);
        let (caret, anchor) = match drag.handle {
            Handle::Caret => (index, index),
            _ if index == drag.fixed => return,
            _ => (index, drag.fixed),
        };
        if caret == self.caret && anchor == self.anchor {
            return;
        }
        self.set_focus_range(Some(Focus {
            model: region.model,
            row: region.row,
            instance: region.instance,
            caret,
            anchor,
            preedit: None,
        }));
    }

    /// The finger holding a handle lifted. Returns whether one was held.
    ///
    /// A caret handle that was tapped rather than dragged opens the Paste and
    /// Select all menu, and a second tap closes it, which is what a caret
    /// handle does on Android.
    fn release_handle(&mut self) -> bool {
        let Some(drag) = self.handle_drag.take() else { return false };
        if drag.handle == Handle::Caret {
            if !drag.moved {
                self.caret_menu = !self.caret_menu;
            }
            self.caret_handle_until = Some(Instant::now() + CARET_HANDLE_FADE);
        }
        self.request_redraw();
        true
    }

    /// Whether a long press at `pointer` is on a word rather than on empty
    /// space: past the end of a line, after the last word, or on the spaces
    /// between two words.
    ///
    /// `word_at_point` answers with the nearest word wherever the finger is,
    /// which is right for a double click and wrong here: a long press after
    /// the last word selected that word, where the person was asking for
    /// Paste at the end of the text. So the finger has to be inside the word's
    /// own box.
    fn press_on_word(&mut self, pointer: (f64, f64)) -> bool {
        let (fx, fy) = self.logical(pointer);
        let Some(region) = self.focuses.iter().rev().find(|f| f.contains(fx, fy)).cloned() else {
            return false;
        };
        let value =
            self.document.value_in(&region.model, region.row.as_deref(), region.instance.as_deref());
        let Some(t) = region.text.as_ref() else { return false };
        if value.is_empty() {
            return false;
        }
        let style = rux_paint::text_style(&t.content);
        let (tx, ty) = self.text_point(&region, t, fx, fy);
        // A masked field is one run of bullets: on it is on the "word", past
        // it is empty space, exactly as in any other field.
        if region.kind.secret() {
            let shown = rux_layout::mask(&value);
            let (sx, sy, sh) = self.text.caret_geometry(&shown, &style, Some(t.width), 0);
            let (ex, _, _) = self.text.caret_geometry(&shown, &style, Some(t.width), shown.len());
            return tx >= sx && tx <= ex && ty >= sy && ty <= sy + sh;
        }
        let (start, end) = self.text.word_at_point(&value, &style, Some(t.width), tx, ty);
        if value.get(start..end).is_none_or(|w| w.trim().is_empty()) {
            return false;
        }
        let (sx, sy, sh) = self.text.caret_geometry(&value, &style, Some(t.width), start);
        let (ex, ey, _) = self.text.caret_geometry(&value, &style, Some(t.width), end);
        // A word that wraps is taken anywhere on its lines.
        if (ey - sy).abs() > 0.5 {
            return true;
        }
        tx >= sx && tx <= ex && ty >= sy && ty <= sy + sh
    }

    /// Extend a selection that began as a word, from a finger still down.
    ///
    /// **The word stays whole.** This was `drag_text`, which moves the caret
    /// to the finger and keeps the anchor at the word's start, so the first
    /// movement after a long press, however small, cut the word back to
    /// wherever the finger sat inside it. A finger resting on "brown" selected
    /// "br". Driven on the phone with `adb shell input swipe`, whose held
    /// press reports moves of zero distance, and a real finger's tremor does
    /// the same. Now the finger only extends past the word, from its far end,
    /// which is what a browser does after a double click and a drag.
    fn extend_from_word(&mut self, pointer: (f64, f64)) {
        let Some((start, end)) = self.pressed_word else {
            self.drag_text(pointer);
            return;
        };
        let Some(region) = self.focused_region().cloned() else { return };
        let (fx, fy) = self.logical(pointer);
        let index = self.index_in(&region, fx, fy);
        let (anchor, caret) = if index > end {
            (start, index)
        } else if index < start {
            (end, index)
        } else {
            (start, end)
        };
        if (anchor, caret) == (self.anchor, self.caret) {
            return;
        }
        self.set_focus_range(Some(Focus {
            model: region.model,
            row: region.row,
            instance: region.instance,
            caret,
            anchor,
            preedit: None,
        }));
    }

    /// Move the caret to the pointer *without* selecting: the anchor follows it,
    /// so the range stays empty. This is what a finger dragging on text does on
    /// a phone, where selecting is what the long press is for.
    fn drag_caret(&mut self, pointer: (f64, f64)) {
        let Some(region) = self.focused_region().cloned() else { return };
        let (fx, fy) = self.logical(pointer);
        let caret = self.index_in(&region, fx, fy);
        if caret != self.caret || self.anchor != caret {
            self.set_focus_range(Some(Focus {
                model: region.model,
                row: region.row,
                instance: region.instance,
                caret,
                anchor: caret,
                preedit: None,
            }));
        }
    }

    /// Extend the selection to the pointer while dragging inside an input: the
    /// anchor stays where the press landed, the caret follows the pointer.
    fn drag_text(&mut self, pointer: (f64, f64)) {
        let Some(region) = self.focused_region().cloned() else { return };
        let (fx, fy) = self.logical(pointer);
        let caret = self.index_in(&region, fx, fy);
        if caret != self.caret {
            let anchor = self.anchor;
            self.set_focus_range(Some(Focus {
                model: region.model,
                row: region.row,
                instance: region.instance,
                caret,
                anchor,
                preedit: None,
            }));
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

    /// Push the current pointer state into the document so `:hover` and `:active`
    /// restyle. The topmost state region under the pointer wins, as with tap
    /// dispatch; `:active` additionally requires the button to be down on it.
    ///
    /// Cheap to call on every mouse move: with no pointer-state rules in the
    /// document there are no regions, and the document declines any state it is
    /// already in without touching the tree.
    fn update_pointer_state(&mut self) {
        if self.states.is_empty() && self.document.interaction().hovered.is_none() {
            return;
        }
        let scale = self.scale();
        let (px, py) = ((self.pointer.0 / scale) as f32, (self.pointer.1 / scale) as f32);
        let hovered = self
            .states
            .iter()
            .rev()
            .find(|r| r.contains(px, py))
            .map(|r| r.path.clone());
        // Pressing and then dragging off the element drops `:active`, the way a
        // button un-presses when the pointer leaves it.
        let active = self.press.is_some().then(|| hovered.clone()).flatten();
        // Everything but the pointer is kept as the document has it: focus and
        // which fields have been touched are not the pointer's to change.
        let next = InteractionState { hovered, active, ..self.document.interaction().clone() };
        if self.document.set_interaction(next) {
            self.request_redraw();
        }
    }

    /// Tell the document the window's *logical* size, so `@media` queries are
    /// evaluated against the same units the stylesheet is written in. The document
    /// only re-cascades if a query actually changed answer, so calling this on
    /// every resize event is cheap.
    fn update_viewport(&mut self) {
        let Some(state) = self.state.as_ref() else { return };
        let scale = state.window.scale_factor();
        let viewport = Viewport {
            width: (state.surface.config.width as f64 / scale) as f32,
            height: (state.surface.config.height.saturating_sub(keyboard_px()) as f64 / scale)
                as f32,
        };
        let environment = rux_runtime::Environment {
            viewport,
            reduced_motion: Self::os_reduced_motion(),
            color_scheme: Self::window_color_scheme(&state.window),
            // The window's scale factor is what a stylesheet would call density,
            // and it is the one environment answer this shell has always known
            // and never passed on. A preview answers for the device instead,
            // since the monitor's scale factor says nothing about a phone's.
            density: match self.preview {
                Some(profile) => profile.density,
                None => scale as f32,
            },
            // Zero on a desktop, which is the honest answer and also a useless
            // one to develop against: a window that owns its whole surface has
            // no unsafe edges, and every phone does. A preview answers with a
            // named device's insets; Android answers with the real ones.
            #[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
            safe_area: self.preview.map(|p| p.safe_area).unwrap_or_default(),
            // What the browser says, which is zero unless the page asked to
            // draw under the notch (`viewport-fit=cover`, as a built app's
            // page does). See [`web_safe_area`].
            #[cfg(target_arch = "wasm32")]
            safe_area: web_safe_area(),
            // Read here rather than delivered, for the reason on `SAFE_AREA`:
            // the insets arrive on another thread and there is no event to
            // carry them. So they are picked up wherever the environment is
            // rebuilt, which is startup and every resize. A rotation is a
            // resize, and it is the case that moves an inset from one edge to
            // another, so the one that matters is covered.
            #[cfg(target_os = "android")]
            safe_area: android_safe_area(scale),
            ..Default::default()
        };
        if self.document.set_environment(environment) {
            self.request_redraw();
        }
    }

    /// Whether the operating system says to reduce motion.
    ///
    /// Asked here because winit 0.30 exposes nothing for it and every platform
    /// answers differently. Windows is `SPI_GETCLIENTAREAANIMATION`, which
    /// reports whether animation is *enabled*, so reducing motion is its
    /// negation. Everywhere else the honest answer for now is "no preference
    /// stated", which is what a platform that has not been taught to ask should
    /// say rather than guessing.
    ///
    /// **Known limit:** this is read when the environment is next rebuilt,
    /// which is at startup and on a resize. Turning the setting off while an
    /// app is running is not noticed until then, because Windows announces it
    /// with `WM_SETTINGCHANGE` and winit does not forward that event.
    #[cfg(windows)]
    fn os_reduced_motion() -> bool {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            SystemParametersInfoW, SPI_GETCLIENTAREAANIMATION,
        };
        // A Win32 BOOL. Defaulted to "animations on" so that a failed call
        // leaves motion exactly as it was rather than silently stilling an app.
        let mut animations_enabled: i32 = 1;
        let ok = unsafe {
            SystemParametersInfoW(
                SPI_GETCLIENTAREAANIMATION,
                0,
                (&mut animations_enabled) as *mut i32 as *mut core::ffi::c_void,
                0,
            )
        };
        ok != 0 && animations_enabled == 0
    }

    /// Every platform that has not been taught to ask. Saying no preference
    /// is honest; guessing would be worse than the gap.
    /// A browser answers the media query itself, so a web app asks it.
    #[cfg(target_arch = "wasm32")]
    fn os_reduced_motion() -> bool {
        web_sys::window()
            .and_then(|w| w.match_media("(prefers-reduced-motion: reduce)").ok().flatten())
            .is_some_and(|m| m.matches())
    }

    #[cfg(not(any(windows, target_arch = "wasm32")))]
    fn os_reduced_motion() -> bool {
        false
    }

    /// The size to open a preview window at, which is the device's own size
    /// unless the monitor is too small for it.
    ///
    /// **A phone is taller than a laptop screen, in the units that matter.** A
    /// 393 by 852 window is 1278 physical pixels tall on a 1.5x display, and a
    /// monitor 960 pixels high cannot show it: the window opens with its lower
    /// half off the bottom of the desktop, which looks like the app having
    /// drawn nothing there. So it is capped to what fits, and the cap is said
    /// out loud, because a window that is not the size it was asked for is
    /// worth one line of explanation.
    ///
    /// The viewport follows the window, so a capped preview is an honest
    /// smaller device rather than a lie about a big one: `@media` answers for
    /// the window that exists, and only density and the insets come from the
    /// profile.
    #[cfg(not(target_arch = "wasm32"))]
    fn preview_size(event_loop: &ActiveEventLoop, profile: DeviceProfile) -> (f64, f64) {
        let (want_w, want_h) = (profile.width as f64, profile.height as f64);
        let Some(monitor) = event_loop.primary_monitor() else { return (want_w, want_h) };
        let scale = monitor.scale_factor();
        let size = monitor.size();
        // Nine tenths, because the monitor's full height is not available to a
        // window: a title bar sits above the client area and a taskbar usually
        // sits below the desktop. winit does not report the work area, and
        // guessing low is the failure that is easy to recover from by dragging.
        let (max_w, max_h) =
            (size.width as f64 / scale * 0.9, size.height as f64 / scale * 0.9);
        let (w, h) = (want_w.min(max_w), want_h.min(max_h));
        if w < want_w || h < want_h {
            eprintln!(
                "rux: this monitor fits {} by {} logical pixels, not the {} by {} of `{}`, \
                 so the window is that much of it. Density and the safe area are still the \
                 device's.",
                w as i32, h as i32, want_w as i32, want_h as i32, profile.name
            );
        }
        (w, h)
    }

    /// Whether the operating system is asking for light or dark surfaces.
    ///
    /// Unlike reduced motion this needs no per-platform code: winit asks the
    /// window itself, and every platform that has an answer gives one here.
    /// `None` means the platform has no notion of a theme, and light is the
    /// honest reading of that: it is what a window with no preference stated
    /// has always been drawn as.
    ///
    /// This does **not** share reduced motion's known limit. A theme changed
    /// mid-run arrives as [`WindowEvent::ThemeChanged`], so the environment is
    /// rebuilt the moment the person flips the setting.
    fn window_color_scheme(window: &Window) -> rux_runtime::ColorScheme {
        match window.theme() {
            Some(Theme::Dark) => rux_runtime::ColorScheme::Dark,
            Some(Theme::Light) | None => rux_runtime::ColorScheme::Light,
        }
    }

    /// The pointer left the window: nothing is hovered or pressed any more.
    ///
    /// This needs its own event because the pointer leaving produces `CursorLeft`,
    /// not a `CursorMoved` to somewhere outside, so without it a `:hover` style
    /// stays lit after the pointer is long gone.
    fn clear_pointer_state(&mut self) {
        let mut next = self.document.interaction().clone();
        if next.hovered.is_none() && next.active.is_none() {
            return;
        }
        next.hovered = None;
        next.active = None;
        if self.document.set_interaction(next) {
            self.request_redraw();
        }
    }

    /// Tell the document which element that is not a text field has focus, and
    /// whether to show it, so `:focus` and `:focus-visible` match it and the
    /// default stylesheet's ring is drawn on it.
    ///
    /// **Shown after the keyboard, not after a tap**, which is the rule
    /// browsers settled on: a button pressed with a finger or a mouse does not
    /// grow a ring, and the same button reached with Tab does, because only
    /// then is the ring the one way to see where you are. A text field shows
    /// its focus either way, and is not decided here (see
    /// `rux_style::ElemStates::focus_visible`). The ring used to be drawn by
    /// the shell on every focus, tapped or not, and nothing could restyle it.
    ///
    /// Run once per wake-up rather than at each place focus moves: Tab, a tap,
    /// Escape, a `focus()` and a rebuild that lost the element all move it,
    /// and this catches every one.
    fn sync_focus_state(&mut self) {
        let path = self.focus_index.and_then(|i| self.focusables.get(i)).and_then(|f| f.path.clone());
        // Whether it would be shown only matters while something is focused
        // this way, and leaving it false otherwise saves a re-cascade every
        // time the person goes from typing to clicking.
        let visible = path.is_some() && self.keyboard_modality;
        let mut next = self.document.interaction().clone();
        if next.focused_path == path && next.focus_visible == visible {
            return;
        }
        next.focused_path = path;
        next.focus_visible = visible;
        if self.document.set_interaction(next) {
            self.request_redraw();
        }
    }

    /// Tell the document which input has focus, so `:focus` rules match it.
    fn update_focus_state(
        &mut self,
        model: Option<String>,
        row: Option<String>,
        instance: Option<String>,
        left: Option<(String, Option<String>, Option<String>)>,
    ) {
        let mut next = self.document.interaction().clone();
        if next.focused_model == model
            && next.focused_row == row
            && next.focused_instance == instance
        {
            return;
        }
        // The field being left has now been through, for `:user-invalid`.
        // Recorded in the same change as the focus move, so the two cost one
        // re-cascade between them.
        if let Some(left) = left.filter(|l| !next.touched.contains(l)) {
            next.touched.push(left);
        }
        next.focused_model = model;
        next.focused_row = row;
        next.focused_instance = instance;
        if self.document.set_interaction(next) {
            self.request_redraw();
        }
    }

    /// Handle a completed tap at `(px, py)`, in physical pixels: focus an input
    /// Hide the dev overlay if `(fx, fy)` in logical px is on it. Returns whether
    /// it acted, so the tap is not also delivered to the app underneath.
    ///
    /// The dismissal is remembered against the current diagnostics, so it lasts
    /// exactly as long as the document's problems are the same ones.
    fn dismiss_overlay_at(&mut self, fx: f32, fy: f32) -> bool {
        if !self.overlay_covers(fx, fy) {
            return false;
        }
        self.overlay_dismissed = Some(self.document.diagnostics().clone());
        self.overlay_rect = None;
        self.request_redraw();
        true
    }

    /// Whether the overlay is on screen and covers `(fx, fy)` in logical px.
    fn overlay_covers(&self, fx: f32, fy: f32) -> bool {
        self.overlay_rect
            .is_some_and(|(x, y, w, h)| fx >= x && fx <= x + w && fy >= y && fy <= y + h)
    }

    /// The same test against a physical-pixel pointer position, which is what
    /// the press handlers have. A press landing on the panel must not reach the
    /// app underneath: starting a text selection inside a field you cannot see
    /// is exactly the confusion the panel is there to prevent.
    fn overlay_covers_physical(&self, (px, py): (f64, f64)) -> bool {
        let scale = self.scale();
        self.overlay_covers((px / scale) as f32, (py / scale) as f32)
    }

    /// if one is under the pointer, otherwise run the topmost `@tap` handler.
    fn dispatch_tap(&mut self, px: f64, py: f64) {
        let scale = self.scale();
        let (px, py) = (px / scale, py / scale);
        let (fx, fy) = (px as f32, py as f32);

        // The dev overlay is painted above everything, including a dropdown, so
        // it takes the tap first. Anything else would have the panel swallow
        // taps meant for it while passing them to whatever it is covering.
        if self.dismiss_overlay_at(fx, fy) {
            return;
        }

        // The selection toolbar sits above the page like the dropdown, so it
        // takes the tap before anything under it. Checked before the dropdown
        // because the two are never up together: opening a select drops focus.
        if let Some(action) = self.toolbar_action_at(fx, fy) {
            self.run_text_action(action);
            return;
        }

        // An open dropdown is on top of everything, so it intercepts taps first:
        // a tap on an option selects it; any other tap just closes the dropdown.
        if let Some((model, row, instance)) = self.open_select.take() {
            if let Some(sel) = self
                .selects
                .iter()
                .find(|s| s.model == model && s.row == row && s.instance == instance)
                .cloned()
            {
                for (i, option) in sel.options.iter().enumerate() {
                    let (rx, ry, rw, rh) = dropdown_row(&sel, i);
                    if fx >= rx && fx <= rx + rw && fy >= ry && fy <= ry + rh {
                        self.choose_option(&model, row, instance, option, sel.field.on_change);
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

        // A tap on a date on a phone opens the platform's date picker. On the
        // tap rather than the press (see `press_text`), so a scroll that
        // happens to start on the field scrolls. A read-only date shows its
        // day and offers no picker, as a read-only field offers no keyboard.
        // A phone's browser is a phone here too: its own picker, not a field
        // typed into.
        #[cfg(any(target_os = "android", target_arch = "wasm32"))]
        if let Some(region) = self
            .focuses
            .iter()
            .rev()
            .find(|f| f.kind == InputKind::Date && f.contains(fx, fy) && touch_first())
            .cloned()
        {
            if !region.field.readonly {
                let value =
                    self.document.value_in(&region.model, region.row.as_deref(), region.instance.as_deref());
                let (min, max) = (region.field.min, region.field.max);
                #[cfg(target_arch = "wasm32")]
                let rect = (region.x, region.y, region.width, region.height);
                self.pending_date =
                    Some((region.model, region.row, region.instance, region.field.on_change));
                self.set_focus(None);
                #[cfg(target_os = "android")]
                android_open_date(&value, min, max);
                #[cfg(target_arch = "wasm32")]
                web_open_date(&value, min, max, rect);
            }
            return;
        }

        // A tap on a closed select opens its dropdown.
        if let Some(sel) = self.selects.iter().find(|s| s.contains(fx, fy)) {
            // **On a phone the platform owns this control**, which the spec has
            // asked for since before there was a phone to run it on. A drawn
            // dropdown is a browser emulation: it does not scroll, dismiss,
            // announce or look like every other picker the person has used, and
            // on a small screen those are most of what a picker is.
            #[cfg(any(target_os = "android", target_arch = "wasm32"))]
            if touch_first() {
                // Cloned out before anything else touches `self`: the search
                // above borrows `self.selects`, and reading the field's current
                // value needs the document mutably.
                let (model, row, instance, options) = (
                    sel.model.clone(),
                    sel.row.clone(),
                    sel.instance.clone(),
                    sel.options.clone(),
                );
                let chosen = self.document.value_in(&model, row.as_deref(), instance.as_deref());
                let at = options.iter().position(|o| *o == chosen);
                #[cfg(target_arch = "wasm32")]
                let rect = (sel.x, sel.y, sel.width, sel.height);
                self.pending_select = Some((model, row, instance, options.clone()));
                self.set_focus(None);
                #[cfg(target_os = "android")]
                android_open_select(&options, at);
                #[cfg(target_arch = "wasm32")]
                web_open_select(&options, at, rect);
                return;
            }
            self.open_select = Some((sel.model.clone(), sel.row.clone(), sel.instance.clone()));
            self.set_focus(None);
            self.request_redraw();
            return;
        }

        // Inputs are handled at press time (`press_text`), which is where a
        // selection drag has to start, so by here the tap is on something else.
        // Tapping elsewhere drops focus.
        self.set_focus(None);

        // Topmost hit region wins (later in list = drawn on top).
        let handler = self
            .hits
            .iter()
            .rev()
            // Only regions that actually have a `@tap`: an element with just
            // `@drag` is hit-testable but is not a tap target, and letting it
            // swallow the tap would hide the button underneath it.
            .find(|h| h.on_tap.is_some() && h.contains(px as f32, py as f32))
            .map(|h| {
                let rect = ((h.x, h.y), (h.width, h.height));
                (h.on_tap.clone().unwrap_or_default(), h.instance.clone(), rect)
            });

        if let Some((src, instance, (origin, size))) = handler {
            // Patch in place when the change is display-only; rebuild only when it
            // touches structure/attributes/input values. Either way, repaint.
            //
            // The instance travels with the handler because two instances of one
            // component carry identical handler text: the string alone cannot
            // say whose state to run it against.
            let event = self.pointer_event(fx, fy, origin, size);
            if self.document.apply_handler_with_event(&src, instance.as_deref(), &event) {
                self.request_redraw();
            }
            self.adopt_element_requests();
        }
    }

    /// Begin tracking a press for the pointer handlers, and fire `@press`.
    ///
    /// Called from both the mouse and the touchscreen, with the position already
    /// in logical pixels, so the two cannot drift apart in what a gesture means.
    fn begin_gesture(&mut self, fx: f32, fy: f32, from_touch: bool) {
        let found = self
            .hits
            .iter()
            .rev()
            .find(|h| !h.gestures.is_empty() && h.contains(fx, fy))
            .map(|h| {
                let rect = ((h.x, h.y), (h.width, h.height));
                (h.gestures.clone(), h.instance.clone(), rect, h.touch_action)
            });
        let Some((handlers, instance, (origin, size), touch_action)) = found else {
            self.gesture = None;
            self.gesture_deadline = None;
            return;
        };
        let listening_long = handlers.iter().any(|(g, _)| *g == rux_layout::Gesture::LongPress);
        self.gesture = Some(GesturePress {
            origin,
            size,
            handlers,
            instance,
            start: (fx, fy),
            last: (fx, fy),
            at: Instant::now(),
            dragging: false,
            long_fired: false,
            touch_action,
            from_touch,
            scroll_won: None,
        });
        // Only armed when something is listening, so a page full of ordinary
        // buttons still sleeps between events.
        self.gesture_deadline = listening_long.then(|| Instant::now() + LONG_PRESS);
        self.fire_gesture(rux_layout::Gesture::Press, fx, fy, Vec::new());
    }

    /// Follow a press that is moving: start or continue a drag, and give up on
    /// the long press once the finger has wandered.
    fn move_gesture(&mut self, fx: f32, fy: f32) {
        let Some(press) = self.gesture.clone() else { return };
        let travelled = (fx - press.start.0).hypot(fy - press.start.1);
        if travelled > TAP_SLOP as f32 {
            // It has moved, so it is no longer resting.
            self.gesture_deadline = None;
        }
        if !press.handlers.iter().any(|(g, _)| *g == rux_layout::Gesture::Drag) {
            return;
        }
        // The axis claim. Asked once, the moment the finger first passes the
        // slop threshold, because before that the dominant axis is noise and
        // after that the answer must not change under the hand.
        if press.from_touch && press.scroll_won.is_none() && travelled > TAP_SLOP as f32 {
            let (dx, dy) = (fx - press.start.0, fy - press.start.1);
            let vertical = dy.abs() >= dx.abs();
            // A scroller only wins an axis it can actually scroll. That is what
            // lets a horizontal carousel inside a vertical page take horizontal
            // drags with no CSS written at all, while a vertical thumb-swipe
            // over the same carousel still scrolls the page.
            let scroll_won = press.touch_action.scroller_may_take(vertical)
                && self.scroller_can_take(press.start, vertical);
            if let Some(g) = self.gesture.as_mut() {
                g.scroll_won = Some(scroll_won);
            }
            if scroll_won {
                return;
            }
        }
        // The scroller took this gesture at the slop crossing, so `@drag` stays
        // out of it for the rest of the press.
        if press.scroll_won == Some(true) {
            return;
        }
        if !press.dragging {
            if travelled <= TAP_SLOP as f32 {
                return;
            }
            if let Some(g) = self.gesture.as_mut() {
                g.dragging = true;
            }
            self.fire_gesture(
                rux_layout::Gesture::Drag,
                fx,
                fy,
                drag_fields("start", press.start, press.last, fx, fy),
            );
        } else {
            self.fire_gesture(
                rux_layout::Gesture::Drag,
                fx,
                fy,
                drag_fields("move", press.start, press.last, fx, fy),
            );
        }
        // After the dispatch, so this event's `moveX` is measured against where
        // the pointer was when the *previous* one ran.
        if let Some(g) = self.gesture.as_mut() {
            g.last = (fx, fy);
        }
    }

    /// End a press: `@release` always, then the one thing it turned out to be.
    fn end_gesture(&mut self, fx: f32, fy: f32) {
        let Some(press) = self.gesture.take() else {
            self.gesture_deadline = None;
            return;
        };
        self.gesture_deadline = None;
        // Put it back for the duration of the dispatches below, so a handler
        // reading the element's origin still gets the right frame.
        self.gesture = Some(press.clone());

        self.fire_gesture(rux_layout::Gesture::Release, fx, fy, Vec::new());

        let (dx, dy) = (fx - press.start.0, fy - press.start.1);
        if press.dragging {
            self.fire_gesture(
                rux_layout::Gesture::Drag,
                fx,
                fy,
                drag_fields("end", press.start, press.last, fx, fy),
            );
        }
        // A drag that ended as a flick is **also** a swipe. They are not rivals:
        // a page that follows the finger still has to be told, at the end,
        // whether the hand meant to throw it. Making them exclusive meant an
        // element with both handlers could never fire the swipe, which is
        // exactly what happened the first time this was tried by hand.
        if dx.hypot(dy) >= SWIPE_DISTANCE && press.at.elapsed() <= SWIPE_TIME {
            // The dominant axis decides, which is what makes a slightly diagonal
            // flick still mean what the hand meant.
            let direction = if dx.abs() > dy.abs() {
                if dx > 0.0 { "right" } else { "left" }
            } else if dy > 0.0 {
                "down"
            } else {
                "up"
            };
            let mut extra = drag_fields("end", press.start, press.last, fx, fy);
            // A swipe has no phases: it happens once, when it is already over.
            extra.retain(|(k, _)| k != "phase");
            extra.push(("direction".to_string(), rux_reactive::Value::Text(direction.to_string())));
            self.fire_gesture(rux_layout::Gesture::Swipe, fx, fy, extra);
        }
        self.gesture = None;
    }

    /// Run the handler for one gesture, if the pressed element declared it.
    fn fire_gesture(
        &mut self,
        kind: rux_layout::Gesture,
        fx: f32,
        fy: f32,
        extra: Vec<(String, rux_reactive::Value)>,
    ) {
        let Some(press) = self.gesture.clone() else { return };
        let Some((_, body)) = press.handlers.iter().find(|(g, _)| *g == kind) else { return };
        let mut event = self.pointer_event(fx, fy, press.origin, press.size);
        if let rux_reactive::Value::Map(fields) = &mut event {
            fields.extend(extra);
        }
        if self.document.apply_handler_with_event(body, press.instance.as_deref(), &event) {
            self.request_redraw();
        }
        self.adopt_element_requests();
    }

    /// What a handler is handed as `event`: where the pointer is, and every
    /// finger that is down.
    ///
    /// Coordinates are **relative to the element the handler is on**, in logical
    /// pixels, because that is the frame an author is thinking in: half way
    /// across a card is `event.x > width / 2`, whatever the card's position on
    /// screen. `touches` is the whole hand, in the same frame, so a finger
    /// outside the element is simply negative rather than missing.
    ///
    /// A list even when there is one finger, and a mouse counted as one finger
    /// with id 0. The shape does not change when a second finger arrives, which
    /// is the point: pinch and rotate can be added later without rewriting what
    /// every existing handler reads.
    ///
    /// `width` and `height` are the element's own, so "how far across" is
    /// `event.x / event.width` with nothing else to look up. A slider is
    /// built on exactly that.
    fn pointer_event(
        &self,
        fx: f32,
        fy: f32,
        origin: (f32, f32),
        size: (f32, f32),
    ) -> rux_reactive::Value {
        use rux_reactive::Value;
        let touches: Vec<Value> = self
            .points
            .iter()
            .map(|(id, (x, y))| {
                Value::Map(vec![
                    ("id".to_string(), Value::Number(*id as f64)),
                    ("x".to_string(), Value::Number((x - origin.0) as f64)),
                    ("y".to_string(), Value::Number((y - origin.1) as f64)),
                ])
            })
            .collect();
        Value::Map(vec![
            ("x".to_string(), Value::Number((fx - origin.0) as f64)),
            ("y".to_string(), Value::Number((fy - origin.1) as f64)),
            // The same point in the window's own frame. Needed whenever the
            // element cannot be the frame of reference: a drag whose element is
            // moving under the finger, or anything comparing two elements.
            ("pageX".to_string(), Value::Number(fx as f64)),
            ("pageY".to_string(), Value::Number(fy as f64)),
            ("width".to_string(), Value::Number(size.0 as f64)),
            ("height".to_string(), Value::Number(size.1 as f64)),
            ("touches".to_string(), Value::List(touches)),
        ])
    }

    /// Apply a `focus()` or `blur()` a handler asked for.
    ///
    /// Through `set_focus_range`, the shell's one focus funnel, rather than by
    /// letting the document set its own focus directly. The document owns the
    /// caret; the shell separately owns which input keystrokes reach, the IME
    /// state, the blink timer and the text scroll. Setting only the first paints
    /// a caret in an input that then ignores every key, which is exactly what
    /// shipped until the window was driven by hand.
    fn adopt_focus_request(&mut self) {
        if let Some(request) = self.document.take_focus_request() {
            self.set_focus_range(request);
            self.request_redraw();
        }
    }

    /// Apply everything a handler asked to do to an element: any `tap()` first,
    /// then the focus it settled on.
    ///
    /// Taps are drained in a bounded loop because a synthetic tap runs a
    /// handler, and that handler may tap something else. A button that taps
    /// itself is a loop with no exit, so it is cut off with the same reasoning
    /// (and the same wording) as the `emit` chain limit rather than hanging the
    /// window.
    ///
    /// Focus is adopted *after* the taps, because a tap moves focus itself: a
    /// handler that taps something and then focuses an input means the focus,
    /// and applying them the other way round would let the tap overwrite it.
    fn adopt_element_requests(&mut self) {
        const MAX_TAP_DEPTH: usize = 8;
        for round in 0.. {
            let taps = self.document.take_taps();
            if taps.is_empty() {
                break;
            }
            if round >= MAX_TAP_DEPTH {
                rux_runtime::warn_script(format!(
                    "a tap() chain is still going after {MAX_TAP_DEPTH} rounds and has been \
                     stopped; an element is probably tapping something that taps it back"
                ));
                break;
            }
            for path in taps {
                self.synthesize_tap(&path);
            }
        }
        self.adopt_focus_request();
        // Last, so a refused submission's focus on the first bad field is the
        // focus the person ends up with.
        for form in self.document.take_submits() {
            self.submit_form(&form);
        }
    }

    /// Submit the form at `path`: its `@submit` with `event.values` when every
    /// field passes, and otherwise `@invalid` with `event.errors`, the caret in
    /// the first field that failed and `:user-invalid` on every field that did.
    ///
    /// Refused as HTML refuses, so a form's `@submit` only ever sees values that
    /// passed. Both handlers are queued like a field's, and run once the event
    /// that submitted has finished with the focus.
    fn submit_form(&mut self, path: &[usize]) {
        use rux_reactive::Value;
        let Some(report) = self.document.check_form(path) else { return };
        let values = Value::Map(report.values);
        if report.failures.is_empty() {
            // What was typed can now be offered for saving: this is the moment
            // a password manager asks "save password?".
            #[cfg(target_os = "android")]
            android_autofill_commit();
            if let Some(body) = report.form.on_submit {
                let event = Value::Map(vec![("values".to_string(), values)]);
                self.field_events.push_back((body, report.instance, event));
            }
            return;
        }
        let mut next = self.document.interaction().clone();
        if !next.attempted.iter().any(|f| f == path) {
            next.attempted.push(path.to_vec());
            self.document.set_interaction(next);
        }
        let errors = report
            .failures
            .iter()
            .map(|f| (f.name.clone(), Value::Text(f.message.clone())))
            .collect();
        if let Some(body) = report.form.on_invalid {
            let event = Value::Map(vec![
                ("errors".to_string(), Value::Map(errors)),
                ("values".to_string(), values),
            ]);
            self.field_events.push_back((body, report.instance, event));
        }
        // The first field that failed takes the caret, if it is one that can.
        // A checkbox cannot, and the keyboard goes down so it can be seen.
        match report.failures.into_iter().next().and_then(|f| f.focus) {
            Some((model, row, instance)) => {
                let index = self.focusables.iter().position(|item| {
                    matches!(&item.kind, FocusKind::Text { model: m, row: r, instance: i, .. }
                        if *m == model && *r == row && *i == instance)
                });
                match index {
                    Some(i) => self.set_keyboard_focus(Some(i)),
                    None => {
                        let caret = self.document.value_in(&model, row.as_deref(), instance.as_deref()).len();
                        self.set_focus(Some((model, row, instance, caret)));
                    }
                }
            }
            None => self.set_focus(None),
        }
        self.request_redraw();
    }

    /// The focused field's action key, with the default worked out.
    ///
    /// What the author wrote wins. A textarea keeps its Enter. Otherwise a
    /// field in a form says Next, and the form's last field says Go, which
    /// submits it; outside a form a phone's keyboard says Done, which closes
    /// it, where before it said Done and did nothing.
    fn enter_key(&self) -> EnterKey {
        let field = self.live_field();
        if field.enter_key != EnterKey::Default || self.focused_kind.multiline() {
            return field.enter_key;
        }
        match &field.form {
            Some(form) if self.form_neighbour(form, false).is_some() => EnterKey::Next,
            Some(_) => EnterKey::Go,
            // A phone's keyboard outside a form: its key closes the field, in
            // an app or in a phone's browser alike.
            None if touch_first() => EnterKey::Done,
            None => EnterKey::Default,
        }
    }

    /// The typing field after (or before) the focused one in `form`, as an
    /// index into `focusables`. Buttons, toggles and selects are passed over,
    /// as a phone's Next passes over them, and so is a date, which a phone
    /// answers with a picker rather than a keyboard.
    fn form_neighbour(&self, form: &[usize], backward: bool) -> Option<usize> {
        let focused = self.focused.as_deref()?;
        fn typing(item: &FocusItem) -> Option<(&str, Option<&str>, Option<&str>)> {
            match &item.kind {
                FocusKind::Text { model, row, instance, kind, .. } => (*kind != InputKind::Date)
                    .then_some((model.as_str(), row.as_deref(), instance.as_deref())),
                _ => None,
            }
        }
        let here = self.focusables.iter().position(|item| {
            typing(item) == Some((focused, self.focused_row.as_deref(), self.focused_instance.as_deref()))
        })?;
        let in_form = |i: &usize| {
            typing(&self.focusables[*i]).is_some_and(|(model, row, instance)| {
                self.focuses.iter().any(|r| {
                    r.model == model
                        && r.row.as_deref() == row
                        && r.instance.as_deref() == instance
                        && r.field.form.as_deref() == Some(form)
                })
            })
        };
        if backward {
            (0..here).rev().find(in_form)
        } else {
            (here + 1..self.focusables.len()).find(in_form)
        }
    }

    /// Press an element as a finger would, at the centre of its box.
    ///
    /// Routed through the *same* `press_text` + `dispatch_tap` a real pointer
    /// takes, rather than reaching for the element's `@tap` body directly. A
    /// press is not one action: it follows a link, toggles a checkbox, opens a
    /// select, moves keyboard focus and puts the caret in a text input, and
    /// every one of those lives in a different place. Going through the pointer
    /// path means a synthetic tap cannot drift from a real one, because it is a
    /// real one.
    ///
    /// It hit-tests like a real tap too, so the topmost element at that point
    /// wins. Tapping something covered by a dropdown taps the dropdown, exactly
    /// as a finger would, and an element with no box in the last frame (hidden,
    /// or never laid out) cannot be tapped at all.
    fn synthesize_tap(&mut self, path: &[usize]) {
        let Some(m) = self.metrics.iter().find(|e| e.path == path).cloned() else {
            rux_runtime::warn_script(
                "tap() was called on an element with no box on screen, so nothing was tapped"
                    .to_string(),
            );
            return;
        };
        // `dispatch_tap` takes physical pixels and divides by the scale; the
        // metrics are logical, so they have to be scaled back up on the way in.
        let scale = self.scale();
        let x = (m.x + m.width / 2.0) as f64 * scale;
        let y = (m.y + m.height / 2.0) as f64 * scale;
        // Press first, which is where a tap on a text input is handled and
        // where a selection would start, then release, which is what runs a
        // handler. Both halves, or tapping an input would move no caret.
        self.press_text((x, y));
        self.dispatch_tap(x, y);
        self.request_redraw();
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

        let mut value = self.focused_value();
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
            // than moving, that's what every text field does.
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
            Key::Named(NamedKey::ArrowUp | NamedKey::ArrowDown) if self.focused_kind.multiline() => {
                if let Some(t) = self
                    .focused_region()
                    .and_then(|f| f.text.clone())
                {
                    let style = rux_paint::text_style(&t.content);
                    let (shown, shown_caret) = self.shown_text(&value, caret);
                    let (cx, cy, ch) =
                        self.text.caret_geometry(&shown, &style, Some(t.width), shown_caret);
                    let dir = if matches!(key, Key::Named(NamedKey::ArrowUp)) { -1.0 } else { 1.0 };
                    let target_y = cy + ch / 2.0 + dir * ch;
                    let hit = self.text.index_at_point(&shown, &style, Some(t.width), cx, target_y);
                    new_caret = self.real_caret(&value, hit);
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
            // Enter inserts a newline in a textarea. In a one-line field it
            // commits, and does what `enterkeyhint` says.
            Key::Named(NamedKey::Enter) if self.focused_kind.multiline() => {
                new_caret = replace_selection(&mut value, "\n");
                edited = true;
            }
            Key::Named(NamedKey::Enter) => {
                self.enter_in_field(false);
                return;
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
            // Patch the input's value in place (no rebuild) unless `model` is also
            // structural; then set the caret on the resulting tree. A field that
            // refuses the edit (`readonly`) leaves the caret where it was.
            if edited {
                match self.write_focused(&value, new_caret, false) {
                    Some((written, caret)) => {
                        value = written;
                        new_caret = caret;
                    }
                    None => return,
                }
            }
            // Shift+movement keeps the anchor, extending the selection; anything
            // else collapses it to the caret.
            let new_anchor = if moved && extend { self.anchor } else { new_caret };
            self.scroll_caret_into_view(&value, new_caret);
            self.set_focus_range(Some(Focus {
                model,
                // Still the same field being typed into.
                row: self.focused_row.clone(),
                instance: self.focused_instance.clone(),
                caret: new_caret,
                anchor: new_anchor,
                preedit: None,
            }));
        }
    }

    /// Ctrl chords inside a focused input: select all, copy, cut, paste. Returns
    /// whether the key was one of them, so it isn't also typed as a character:
    /// Ctrl+V arrives as `Key::Character("v")`.
    fn text_shortcut(&mut self, key: &Key, model: &str) -> bool {
        let Key::Character(s) = key else { return false };
        // The bodies live in named methods because the selection toolbar runs
        // the same four actions from a tap. Two implementations of "cut" would
        // drift the moment one of them learned about something the other did
        // not.
        match s.to_lowercase().as_str() {
            "a" => self.select_all_text(model),
            "c" => self.copy_selection(),
            "x" => self.cut_selection(model),
            "v" => self.request_paste(model),
            _ => return false,
        }
        true
    }

    /// A caret at `caret` in the focused field, the whole of its identity kept.
    ///
    /// **Not `Focus::at`, which is a field outside any list or component.** Cut,
    /// paste and committed composition all refocused through it, so in an
    /// input inside an `r-for` or a component each one moved focus to a field
    /// with no row and no instance: the field blurred, committed and lost its
    /// caret in the middle of an edit.
    fn focus_here(&self, model: &str, caret: usize) -> Focus {
        Focus::at_row_in(model, self.focused_row.clone(), self.focused_instance.clone(), caret)
    }

    /// Let go of the selection, leaving the caret at its end.
    #[cfg(target_os = "android")]
    fn collapse_selection(&mut self) {
        let Some(model) = self.focused.clone() else { return };
        let (_, end) = self.selection();
        self.set_focus_range(Some(self.focus_here(&model, end)));
    }

    /// Ask Java for the text menu the field wants now, when that differs from
    /// what it was last asked for. See [`App::wanted_text_menu`].
    #[cfg(target_os = "android")]
    fn sync_text_menu(&mut self) {
        let want = self.wanted_text_menu();
        if want == self.text_menu_sent {
            return;
        }
        android_text_menu(want.as_ref());
        self.text_menu_sent = want;
    }

    /// The platform text menu this moment calls for, or `None` for none.
    ///
    /// **The platform's menu, not the drawn one.** It is the one every other
    /// app on the phone shows: its shape, its overflow, and the apps that
    /// register for `PROCESS_TEXT` (Translate, a dictionary), which no drawn
    /// strip can reach. Up while something is selected, or while a caret has
    /// asked for Paste; held back while a finger is on the glass, since
    /// Android's own steps aside for a drag or a scroll and returns on lift.
    #[cfg(target_os = "android")]
    fn wanted_text_menu(&mut self) -> Option<TextMenu> {
        self.focused.as_ref()?;
        let selected = self.caret != self.anchor;
        if !selected && !self.caret_menu {
            self.text_menu_dismissed = None;
            return None;
        }
        if !self.points.is_empty() || self.handle_drag.is_some() || self.touch_text.is_some() {
            return None;
        }
        if let Some(closed) = self.text_menu_dismissed {
            if closed == (self.anchor, self.caret) {
                return None;
            }
            self.text_menu_dismissed = None;
        }
        let [a, b] = self.selection_ends?;
        if !a.visible && !b.visible {
            return None;
        }
        let secret = self.focused_kind.secret();
        let mut flags = 0;
        for action in self.toolbar_actions() {
            flags |= match action {
                TextAction::Copy => MENU_COPY,
                TextAction::Cut => MENU_CUT,
                TextAction::Paste => MENU_PASTE,
                TextAction::SelectAll => MENU_SELECT_ALL,
            };
        }
        // Handing the text to another app is taking it out of the field,
        // which a password refuses for the reason it refuses Copy.
        if selected && !secret {
            flags |= MENU_SHARE | MENU_PROCESS;
        }
        // At a caret, Select takes the word there. Not in a password, which
        // has no words to show.
        if !selected && !secret && !self.focused_value().is_empty() {
            flags |= MENU_SELECT;
        }
        // At a caret, Autofill, as any other field on the phone offers it.
        // Java drops it when no autofill service is switched on.
        if !selected && self.live_field().autofill() {
            flags |= MENU_AUTOFILL;
        }
        if flags == 0 {
            return None;
        }
        if self.live_field().readonly {
            flags |= MENU_READONLY;
        }
        let region = self.focused_region()?.clone();
        // The box the menu floats beside: the selection on one line, the
        // field's width when it runs over several, and down past the handles
        // so the menu never lands on one.
        let top = (a.bottom - a.height).min(b.bottom - b.height).max(region.y);
        let (left, right) = if (a.bottom - b.bottom).abs() < 0.5 {
            (a.x.min(b.x), a.x.max(b.x))
        } else {
            (region.x, region.x + region.width)
        };
        let reach = if self.handles { HANDLE_R * 2.0 } else { 0.0 };
        let bottom = a.bottom.max(b.bottom).min(region.y + region.height) + reach;
        let scale = self.scale() as f32;
        let px = |v: f32| (v * scale).round() as i32;
        let text = if selected && !secret { self.selected_text().unwrap_or_default() } else { String::new() };
        Some(TextMenu {
            rect: [px(left), px(top), px(right).max(px(left) + 1), px(bottom)],
            flags,
            text,
        })
    }

    fn select_all_text(&mut self, model: &str) {
        let value = self.focused_value();
        self.set_focus_range(Some(Focus {
            model: model.to_string(),
            row: self.focused_row.clone(),
            instance: self.focused_instance.clone(),
            caret: value.len(),
            anchor: 0,
            preedit: None,
        }));
    }

    /// The string the focused field actually paints, and a caret inside it.
    ///
    /// **Geometry has to be measured against what is drawn, not what is
    /// stored.** A `type="password"` paints bullets, and a bullet's advance is
    /// nothing like the average letter's, so measuring the real text puts the
    /// caret wherever the real glyphs would have ended. Reported from the
    /// phone: typing into a password field left the caret a third of the way
    /// along, and it could never reach the end.
    ///
    /// Returns the value unchanged for every ordinary field, so the callers
    /// read the same either way.
    fn shown_text(&self, value: &str, caret: usize) -> (String, usize) {
        if self.focused_kind.secret() {
            (rux_layout::mask(value), rux_layout::masked_offset(value, caret))
        } else {
            (value.to_string(), caret.min(value.len()))
        }
    }

    /// Turn a caret found in the painted string back into one in the value.
    ///
    /// The other direction of [`Self::shown_text`], for a tap: it lands on a
    /// bullet, and what has to be recorded is an offset in the real text.
    fn real_caret(&self, value: &str, shown_caret: usize) -> usize {
        if self.focused_kind.secret() {
            rux_layout::unmasked_offset(value, shown_caret)
        } else {
            shown_caret.min(value.len())
        }
    }

    /// Whether the focused field will let its text leave by the clipboard.
    ///
    /// **A mask that can be copied is decoration.** Select all, Copy, and the
    /// password is in the clipboard in plain text, where the next paste
    /// anywhere reveals it. Every platform's own password field refuses this,
    /// and the refusal is the reason masking means anything at all. Paste
    /// *into* the field stays allowed: text arriving is not text leaving.
    fn clipboard_may_read_field(&self) -> bool {
        !self.focused_kind.secret()
    }

    fn copy_selection(&mut self) {
        if !self.clipboard_may_read_field() {
            return;
        }
        if let Some(text) = self.selected_text() {
            self.clipboard_write(&text);
        }
    }

    fn cut_selection(&mut self, model: &str) {
        // Refused outright rather than degraded to a delete: Cut on a password
        // is asking for the text, and quietly destroying it instead would be a
        // different surprise, not a smaller one.
        // A read-only field refuses it before the clipboard is touched, or Cut
        // would quietly become Copy.
        if !self.clipboard_may_read_field() || self.live_field().readonly {
            return;
        }
        let Some(text) = self.selected_text() else { return };
        // Nothing is removed until the text is safe on the clipboard.
        if !self.clipboard_write(&text) {
            return;
        }
        let value = self.focused_value();
        let (start, end) = self.selection();
        let mut value = value;
        value.replace_range(start.min(value.len())..end.min(value.len()), "");
        if self.write_focused(&value, start, false).is_none() {
            return;
        }
        self.set_focus_range(Some(self.focus_here(model, start)));
    }

    /// Ask for the clipboard's contents and paste them.
    ///
    /// Native reads it here and pastes immediately. The web cannot: the Clipboard
    /// API is a promise, and permission may even be prompted for, so the read is
    /// started here and the paste happens later, when [`RuxEvent::WebPaste`]
    /// arrives. Both ends meet in [`apply_paste`](Self::apply_paste).
    // Native, Android included: the body is only `clipboard_read` plus
    // `apply_paste`, and Android's read is synchronous like the desktop's, so
    // the paste path stays one path rather than growing a third.
    #[cfg(not(target_arch = "wasm32"))]
    fn request_paste(&mut self, model: &str) {
        if let Some(pasted) = self.clipboard_read() {
            self.apply_paste(model, &pasted);
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn request_paste(&mut self, _model: &str) {
        use wasm_bindgen_futures::JsFuture;

        let Some(clipboard) = web_clipboard() else { return };
        let promise = clipboard.read_text();
        wasm_bindgen_futures::spawn_local(async move {
            // A rejection is the ordinary case when the user declines the
            // permission prompt, so it is silent rather than a warning: refusing
            // to paste is not an error in the document.
            let Ok(value) = JsFuture::from(promise).await else { return };
            let Some(text) = value.as_string() else { return };
            WEB_PROXY.with(|p| {
                if let Some(proxy) = p.borrow().as_ref() {
                    let _ = proxy.send_event(RuxEvent::WebPaste(text));
                }
            });
        });
    }

    /// Replace the selection with `pasted`, or insert it at the caret.
    fn apply_paste(&mut self, model: &str, pasted: &str) {
        // A single-line input takes the first line only, pasting a block
        // of text into a one-line field shouldn't smuggle newlines in.
        let pasted = if self.focused_kind.multiline() {
            pasted.replace("\r\n", "\n")
        } else {
            pasted.lines().next().unwrap_or("").to_string()
        };
        let value = self.focused_value();
        let (start, end) = self.selection();
        let mut value = value;
        let (start, end) = (start.min(value.len()), end.min(value.len()));
        value.replace_range(start..end, &pasted);
        let Some((value, caret)) = self.write_focused(&value, start + pasted.len(), false) else {
            return;
        };
        self.scroll_caret_into_view(&value, caret);
        self.set_focus_range(Some(self.focus_here(model, caret)));
    }

    /// Keep the caret visible in a scrolling textarea: adjust its scroll offset
    /// so the caret *line* sits inside the box. No-op for a single-line input,
    /// which has no scroll region; its horizontal equivalent is
    /// [`track_caret_x`](Self::track_caret_x), applied once per frame.
    /// Takes no model: the field is whichever one has focus, and a model on its
    /// own cannot say which row of a list that is.
    fn scroll_caret_into_view(&mut self, value: &str, caret: usize) {
        let Some(region) = self.focused_region().cloned() else {
            return;
        };
        let (Some(sid), Some(t)) = (region.scroll_id, &region.text) else {
            return;
        };
        let style = rux_paint::text_style(&t.content);
        // Measured against what is painted; see `shown_text`.
        let (shown, shown_caret) = self.shown_text(value, caret);
        let (_, cy, ch) = self.text.caret_geometry(&shown, &style, Some(t.width), shown_caret);
        // **The text sits inside the padding, and so does the space it may be
        // seen in.** `cy` is measured from the top of the text, which is
        // `inset` below the box's edge; scrolling only until the line's bottom
        // met the *box's* bottom left it under the bottom padding and border,
        // clipped. Reported from the phone: typing past the last visible line
        // kept that line half hidden. Mirrored at the bottom, as
        // `track_caret_x` mirrors it on the right.
        let inset = (t.y - region.y).max(0.0);
        let visible = (region.height - inset * 2.0).max(ch);
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
        // Alt+Left / Alt+Right walk the history, the platform's own shortcut for
        // it. Checked before anything else, including the focused input: it is a
        // chord, so it cannot be text, and a caret in a field is exactly when
        // someone wants to leave a page they typed into by mistake.
        if self.alt_held {
            let moved = match key {
                Key::Named(NamedKey::ArrowLeft) => self.document.back(),
                Key::Named(NamedKey::ArrowRight) => self.document.forward(),
                _ => false,
            };
            if moved {
                self.request_redraw();
                return;
            }
        }
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

    /// Answer a Back. `false` means there was nowhere to go and the app should
    /// close.
    ///
    /// **On a phone this is the only place Back can be answered, and that is
    /// not a preference.** A `NativeActivity` takes the input queue, so a key
    /// reaches native code before the view hierarchy, and `winit` reports every
    /// key it decodes back to Android as handled (only the volume keys are
    /// excepted). The platform therefore never runs the stage that would call
    /// `onBackPressed`, so overriding that, or `onKeyDown`, or registering an
    /// `OnBackInvokedCallback` would each wait for a call that never arrives.
    /// Found on a phone rather than reasoned about: before this existed Back
    /// did nothing at all, and the app could only be left through the task
    /// switcher.
    ///
    /// **Leaving is a real outcome, not a fallback.** Back at the root of the
    /// history closes the app everywhere else on Android, and one that swallows
    /// it instead is the first thing a person notices.
    ///
    /// **A guard's refusal is not "nowhere to go".** `Document::back` answers
    /// `false` both when the history is at its first page and when a route
    /// guard cancelled the step, and treating those alike would turn an
    /// "unsaved changes" guard into an app that quits on the Back it was
    /// written to block. So the history is asked first, and only a genuinely
    /// empty one is reported as a close.
    fn on_back(&mut self) -> bool {
        // A selection is let go of before anything else, as in every Android
        // field: Back with text selected closes the text menu, not the app.
        #[cfg(target_os = "android")]
        if self.focused.is_some() && (self.caret != self.anchor || self.caret_menu) {
            self.collapse_selection();
            return true;
        }
        if !self.document.can_back() {
            return false;
        }
        if self.document.back() {
            self.request_redraw();
        }
        // Refused, but by a guard, which is the app answering rather than
        // declining to. Nothing more to do and nothing to close.
        true
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
    /// the caret at the end); anything else is only focused, and shows it
    /// through `:focus-visible`.
    fn set_keyboard_focus(&mut self, index: Option<usize>) {
        self.focus_index = index;
        match index.and_then(|i| self.focusables.get(i)).map(|f| f.kind.clone()) {
            Some(FocusKind::Text { model, row, instance, kind, .. }) => {
                // Read against the field being moved *to*, not the one being
                // left: focus has not moved yet, so `focused_value` is still the
                // old field and Tab would drop the caret at its length. In the
                // field's own scope, or tabbing into an input inside a component
                // reads an empty string and drops the caret at 0.
                let caret =
                    self.document.value_in(&model, row.as_deref(), instance.as_deref()).len();
                self.focused_kind = kind;
                self.set_focus(Some((model, row, instance, caret)));
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
            Some(FocusKind::Activate { on_tap, instance }) => {
                self.document.apply_handler_in(&on_tap, instance.as_deref());
                self.adopt_element_requests();
                self.request_redraw();
            }
            Some(FocusKind::Select { model, row, instance, .. }) => {
                self.open_select = Some((model, row, instance));
                self.request_redraw();
            }
            _ => {}
        }
    }

    /// Focus an input (or clear focus) and tell the document, so the caret and
    /// selection paint. Collapses the selection to the caret.
    ///
    /// The row is the `r-key` of the `r-for` row the input is in, and `None`
    /// outside a list. It is half the identity: every row of a list is bound to
    /// the same `r-model` text, so the model alone cannot say which one.
    fn set_focus(&mut self, focus: Option<(String, Option<String>, Option<String>, usize)>) {
        match focus {
            Some((model, row, instance, caret)) => {
                self.set_focus_range(Some(Focus::at_row_in(model, row, instance, caret)))
            }
            None => self.set_focus_range(None),
        }
    }

    /// The focused input's value, read in its own row's scope.
    ///
    /// Every read and write of the edited text goes through this pair, because
    /// an `r-model` inside a list can mention the loop variable and means
    /// nothing without it. Reading it raw returned an empty string and a
    /// `Variable not found` warning, which is how a row's field looked editable
    /// and swallowed every keystroke.
    fn focused_value(&mut self) -> String {
        let value = self.focused_signal_text();
        // A number or date field's draft is its text for as long as the signal still
        // holds what the draft wrote. A handler that writes the signal has
        // said what the field holds, and the draft gives way to it.
        match self.typed_draft.take() {
            Some((draft, written)) if written == value => {
                self.typed_draft = Some((draft.clone(), written));
                draft
            }
            Some(_) => {
                self.document.set_draft(None);
                value
            }
            None => value,
        }
    }

    /// The focused field's bound value, as text, whatever the field shows.
    fn focused_signal_text(&mut self) -> String {
        let Some(model) = self.focused.clone() else { return String::new() };
        let row = self.focused_row.clone();
        let instance = self.focused_instance.clone();
        self.document.value_in(&model, row.as_deref(), instance.as_deref())
    }

    /// Whether the focused field is a `type="number"`.
    fn focused_is_number(&self) -> bool {
        self.focused_region().is_some_and(|r| r.kind == InputKind::Number)
    }

    /// Write the focused input's value back, in that same scope, as far as the
    /// field allows, and say what was written and where the caret now belongs.
    ///
    /// **The one place a field's constraints are enforced**, because every way
    /// text reaches a field (a key, a paste, a cut, a composition, a phone's
    /// keyboard, a browser's) ends here. `None` means nothing was written: no
    /// field, or a `readonly` one. `maxlength` cuts the *inserted* text short
    /// rather than the end of the value, so typing in the middle of a full
    /// field does not eat its last character; a value already over the limit
    /// may still shrink, as HTML allows. It is not applied mid-composition,
    /// where cutting the word being composed would fight the input method; the
    /// commit that ends the composition is cut instead.
    ///
    /// A change is `@input`, queued to run once the edit is complete.
    fn write_focused(
        &mut self,
        value: &str,
        caret: usize,
        composing: bool,
    ) -> Option<(String, usize)> {
        let model = self.focused.clone()?;
        let field = self.live_field();
        if field.readonly {
            return None;
        }
        let row = self.focused_row.clone();
        let instance = self.focused_instance.clone();
        let old = self.document.value_in(&model, row.as_deref(), instance.as_deref());
        // What the field shows, which `maxlength` measures: a number's draft.
        let shown = self.focused_value();
        // Typing puts the caret handle away, as it does in every phone field.
        // Only a real change: an input method reports unchanged text as it
        // attaches, and that must not take the handle a tap just put up.
        if value != shown {
            self.caret_handle_until = None;
        }
        let (value, caret) = match field.maxlength.filter(|_| !composing) {
            Some(max) => fit_length(&shown, value, caret, max),
            None => (value.to_string(), caret),
        };
        if let Some(kind) = self.focused_region().map(|r| r.kind).filter(|k| k.typed()) {
            // Only a number, or a date, reaches the signal. Whatever was
            // typed stays in the field as its draft, valid or not, so no
            // keystroke is lost.
            if let Some(typed) = typed_value(kind, &value, &field, self.decimal) {
                self.document.apply_value_in(&model, row.as_deref(), instance.as_deref(), &typed);
            }
            let written = self.document.value_in(&model, row.as_deref(), instance.as_deref());
            if written != old {
                self.queue_field_event(field.on_input.as_deref(), &written);
            }
            self.typed_draft = Some((value.clone(), written));
            self.document.set_draft(Some(value.clone()));
            return Some((value, caret));
        }
        self.document.apply_edit_in(&model, row.as_deref(), instance.as_deref(), &value);
        if value != old {
            self.queue_field_event(field.on_input.as_deref(), &value);
        }
        Some((value, caret))
    }

    /// The focused field's attributes as the last layout has them, so a bound
    /// `:readonly` that changed while it had focus is honoured. Falls back to
    /// what it had when focused, for a field whose region is not laid out yet.
    fn live_field(&self) -> Field {
        self.focused_region().map_or_else(|| self.focused_field.clone(), |r| r.field.clone())
    }

    /// Queue a field handler, if there is one, handed `event.value`. Runs in
    /// the focused field's instance.
    ///
    /// A number field hands over its number, not the text it came from, so
    /// `event.value` is what the signal holds.
    fn queue_field_event(&mut self, body: Option<&str>, value: &str) {
        if body.is_none() {
            return;
        }
        let value = match self.focused.clone() {
            Some(model) if self.focused_is_number() => self
                .document
                .typed_value_in(&model, self.focused_row.as_deref(), self.focused_instance.as_deref())
                .unwrap_or_else(|| rux_reactive::Value::Text(value.to_string())),
            _ => rux_reactive::Value::Text(value.to_string()),
        };
        self.queue_field_event_in(body, self.focused_instance.clone(), value);
    }

    fn queue_field_event_in(
        &mut self,
        body: Option<&str>,
        instance: Option<String>,
        value: rux_reactive::Value,
    ) {
        let Some(body) = body else { return };
        let event = rux_reactive::Value::Map(vec![("value".to_string(), value)]);
        self.field_events.push_back((body.to_string(), instance, event));
    }

    /// Run the field handlers the last batch of events queued.
    ///
    /// **Queued and run afterwards, never from inside the edit or the focus
    /// change that caused them.** A handler may write the field's own signal,
    /// focus another field or blur this one, and every one of those re-enters
    /// the code that was still half way through deciding where the caret goes.
    /// Run from `about_to_wait`, the shell has finished with the event, and a
    /// handler sees the field exactly as the person does.
    ///
    /// Bounded, because a `@blur` that focuses the field and a `@focus` that
    /// blurs it is a loop with no exit; cut off the way a `tap()` chain is.
    fn flush_field_events(&mut self) {
        const MAX_FIELD_EVENTS: usize = 64;
        if self.field_events.is_empty() {
            return;
        }
        let before = self.focused.is_some().then(|| self.focused_value());
        let mut ran = 0;
        while let Some((body, instance, event)) = self.field_events.pop_front() {
            if ran == MAX_FIELD_EVENTS {
                self.field_events.clear();
                rux_runtime::warn_script(format!(
                    "field events were still firing after {MAX_FIELD_EVENTS} handlers and have \
                     been stopped; a @focus or @blur is probably moving focus back and forth"
                ));
                break;
            }
            ran += 1;
            if self.document.apply_handler_with_event(&body, instance.as_deref(), &event) {
                self.request_redraw();
            }
            self.adopt_element_requests();
        }
        // **A handler that rewrote the focused field** (an `@input` that
        // upper-cases, one that strips spaces) changed it behind the input
        // method's back, which is exactly the stale copy that doubled a paste
        // in phase 2. Put the caret back inside the new text and focus the
        // same field again: that goes through `set_ime_enabled`, which brings
        // the keyboard's copy back in step. Only on a change, because a
        // re-focus abandons a desktop composition in progress.
        let Some(model) = self.focused.clone() else { return };
        let value = self.focused_value();
        if before.as_deref() == Some(value.as_str()) {
            return;
        }
        let clamp = |mut i: usize| {
            i = i.min(value.len());
            while !value.is_char_boundary(i) {
                i -= 1;
            }
            i
        };
        let (caret, anchor) = (clamp(self.caret), clamp(self.anchor));
        self.set_focus_range(Some(Focus {
            model,
            row: self.focused_row.clone(),
            instance: self.focused_instance.clone(),
            caret,
            anchor,
            preedit: None,
        }));
    }

    /// Write a select's chosen option, and `@change` if that changed it. A
    /// select commits as it is chosen, as HTML's does: there is no typing to
    /// wait out.
    fn choose_option(
        &mut self,
        model: &str,
        row: Option<String>,
        instance: Option<String>,
        option: &str,
        on_change: Option<String>,
    ) {
        let before = self.document.value_in(model, row.as_deref(), instance.as_deref());
        self.document.apply_edit_in(model, row.as_deref(), instance.as_deref(), option);
        if before != option {
            let option = rux_reactive::Value::Text(option.to_string());
            self.queue_field_event_in(on_change.as_deref(), instance, option);
        }
    }

    /// Commit the focused field: `@change`, if its value differs from what it
    /// held when it was focused or last committed.
    fn commit_focused(&mut self) {
        if self.focused.is_none() {
            return;
        }
        // The bound value, not what the field shows: a number field's draft
        // `1.` commits the `1` it holds.
        let value = self.focused_signal_text();
        if self.committed.as_deref() != Some(value.as_str()) {
            let change = self.focused_field.on_change.clone();
            self.queue_field_event(change.as_deref(), &value);
            self.committed = Some(value);
        }
    }

    /// Enter in a one-line field: commit it, then do what the action key said.
    ///
    /// `next` and `previous` move focus as Tab does and `done` closes the
    /// field, because those three name something the field itself can do. The
    /// others (`go`, `search`, `send`) name what the *app* will do, and are
    /// answered by `@change`.
    fn enter_in_field(&mut self, action_key: bool) {
        // A word still being composed is accepted as it stands, as the
        // keyboard itself accepts it before acting. Left as a composition,
        // moving focus below would abandon it and put the field back the way
        // it was before the word, which is what `cancel_preedit` is for.
        self.preedit = None;
        self.commit_focused();
        let form = self.live_field().form;
        // A keyboard's Enter in a form's field submits it, from any field, as
        // HTML's does; Tab is what moves between them. Only a phone's action
        // key does what its label says, because the label is all a person
        // tapping it has to go on.
        if !action_key {
            if let Some(form) = form {
                self.submit_form(&form);
                return;
            }
        }
        match self.enter_key() {
            // Inside a form, Next and Previous stay among its typing fields.
            // Outside one, or past its ends, they move as Tab does.
            key @ (EnterKey::Next | EnterKey::Previous) => {
                let backward = key == EnterKey::Previous;
                match form.as_deref().and_then(|f| self.form_neighbour(f, backward)) {
                    Some(i) => self.set_keyboard_focus(Some(i)),
                    None => self.move_focus(backward),
                }
            }
            EnterKey::Done => self.set_focus(None),
            // Every other key in a form's field submits it, as Enter in a
            // one-line field submits an HTML form.
            _ => {
                if let Some(form) = form {
                    self.submit_form(&form);
                }
            }
        }
    }

    /// Focus an `autofocus` field when it appears, if nothing has focus.
    ///
    /// On appearance rather than at load, because a route view or an `r-if`
    /// can bring a field in long after the document loaded, and that is when
    /// the author meant it. Nothing is taken from a field that already has
    /// focus: the person put the caret there, and a field arriving elsewhere
    /// on the page is not a reason to move it.
    fn adopt_autofocus(&mut self) {
        let present: Vec<(String, Option<String>, Option<String>)> = self
            .focuses
            .iter()
            .filter(|f| f.field.autofocus && f.text.is_some())
            .map(|f| (f.model.clone(), f.row.clone(), f.instance.clone()))
            .collect();
        let arrived = present.iter().find(|id| !self.autofocus_seen.contains(id)).cloned();
        self.autofocus_seen = present;
        if self.focused.is_some() {
            return;
        }
        let Some((model, row, instance)) = arrived else { return };
        if let Some(region) = self
            .focuses
            .iter()
            .find(|f| f.model == model && f.row == row && f.instance == instance)
        {
            self.focused_kind = region.kind;
        }
        let caret = self.document.value_in(&model, row.as_deref(), instance.as_deref()).len();
        self.set_focus(Some((model, row, instance, caret)));
    }

    /// Put back the field that had focus when Android killed the app, its
    /// text and its caret, once there is a laid-out frame to find it in.
    ///
    /// Tried on one frame only. A field that is not in the first frame is
    /// on a page still waiting for its data, and focus arriving a second later,
    /// in the middle of whatever the person has started doing, is worse than
    /// focus not arriving.
    ///
    /// The text goes through [`App::write_focused`], so the field's `@input`
    /// runs with it: whatever the app derives from the field (a search's
    /// results, a counter) is rebuilt from the text rather than disagreeing
    /// with it.
    #[cfg(any(target_os = "android", target_arch = "wasm32"))]
    fn adopt_restored_focus(&mut self) {
        if self.state.is_none() {
            return;
        }
        let fields = std::mem::take(&mut self.restored_fields);
        self.adopt_restored_fields(fields);
        let Some(saved) = self.restored_focus.take() else { return };
        let Some(saved) = saved else {
            // Nothing had focus, so an `autofocus` field must not take it now:
            // counting them as already seen is what stops it.
            self.autofocus_seen = self
                .focuses
                .iter()
                .filter(|f| f.field.autofocus && f.text.is_some())
                .map(|f| (f.model.clone(), f.row.clone(), f.instance.clone()))
                .collect();
            return;
        };
        let Some(region) = self.focuses.iter().find(|f| {
            f.text.is_some() && f.model == saved.model && f.row == saved.row && f.instance == saved.instance
        }) else {
            return;
        };
        self.focused_kind = region.kind;
        let model = saved.model.clone();
        self.set_focus(Some((model.clone(), saved.row, saved.instance, 0)));
        if let Some(text) = saved.text {
            if text != self.focused_value() {
                self.write_focused(&text, text.len(), false);
            }
        }
        let value = self.focused_value();
        let fit = |at: usize| {
            let mut at = at.min(value.len());
            while !value.is_char_boundary(at) {
                at -= 1;
            }
            at
        };
        let mut focus = self.focus_here(&model, fit(saved.caret));
        focus.anchor = fit(saved.anchor);
        self.set_focus_range(Some(focus));
    }

    /// Put back the text Android kept for the fields that did not have focus,
    /// on the same frame as focus and before it.
    ///
    /// Written straight to each field's signal, not through focus: focusing
    /// every field in turn would run every `@focus` and `@blur` on the page
    /// for a person who touched none of them. Each field's `@input` does run,
    /// for the same reason as the focused field's: whatever the app derives
    /// from a field is rebuilt from its text.
    ///
    /// A field whose text already matches is left alone, so its `@input` does
    /// not run for nothing, and a `readonly` one is left alone because its
    /// text is the app's, which the rebuilt page has already put there.
    #[cfg(any(target_os = "android", target_arch = "wasm32"))]
    fn adopt_restored_fields(&mut self, fields: Vec<rux_runtime::SavedField>) {
        for saved in fields {
            let Some(region) = self.focuses.iter().find(|f| {
                f.text.is_some()
                    && f.kind != InputKind::Password
                    && f.model == saved.model
                    && f.row == saved.row
                    && f.instance == saved.instance
            }) else {
                self.adopt_restored_select(saved);
                continue;
            };
            let (kind, field) = (region.kind, region.field.clone());
            if field.readonly {
                continue;
            }
            let (model, row, instance) = (saved.model.as_str(), saved.row.as_deref(), saved.instance.as_deref());
            if self.document.value_in(model, row, instance) == saved.text {
                continue;
            }
            let value = if kind.typed() {
                // Read back as it was written: a number's text as the signal
                // gave it, with a point whatever the person's language.
                let Some(value) = typed_value(kind, &saved.text, &field, '.') else { continue };
                self.document.apply_value_in(model, row, instance, &value);
                value
            } else {
                self.document.apply_edit_in(model, row, instance, &saved.text);
                rux_reactive::Value::Text(saved.text.clone())
            };
            self.queue_field_event_in(field.on_input.as_deref(), saved.instance.clone(), value);
        }
    }

    /// Put back a `<select>`'s choice, as a native spinner keeps its own:
    /// only if it is still one of the options, and with `@change`, the one
    /// event a select has, so what the app derives from it is rebuilt as a
    /// text field's `@input` rebuilds it.
    #[cfg(any(target_os = "android", target_arch = "wasm32"))]
    fn adopt_restored_select(&mut self, saved: rux_runtime::SavedField) {
        let Some(select) = self
            .selects
            .iter()
            .find(|s| s.model == saved.model && s.row == saved.row && s.instance == saved.instance)
            .cloned()
        else {
            return;
        };
        if select.field.readonly || !select.options.contains(&saved.text) {
            return;
        }
        let (model, row, instance) = (saved.model.as_str(), saved.row.as_deref(), saved.instance.as_deref());
        if self.document.value_in(model, row, instance) == saved.text {
            return;
        }
        self.document.apply_edit_in(model, row, instance, &saved.text);
        let value = rux_reactive::Value::Text(saved.text.clone());
        self.queue_field_event_in(select.field.on_change.as_deref(), saved.instance.clone(), value);
    }

    /// Leave where the app is for `onSaveInstanceState`. See [`SAVED_STATE`].
    /// On the web, in the tab's session storage, for the tab a phone's browser
    /// discards: see [`web_save_state`].
    ///
    /// Every text field on the page keeps its text, as every native
    /// `EditText` does, within one [`SAVED_TEXT_MAX`] for them all. A password
    /// is never kept: the platform writes the state to disk, and the field
    /// comes back empty, as a native one would.
    #[cfg(any(target_os = "android", target_arch = "wasm32"))]
    fn publish_saved_state(&mut self) {
        let mut state = self.document.saved_state();
        let mut budget = SAVED_TEXT_MAX;
        if let Some(model) = self.focused.clone() {
            let text = kept_across_a_kill(self.focused_kind, &self.live_field())
                .then(|| self.focused_value())
                .filter(|t| t.len() <= budget);
            budget -= text.as_ref().map_or(0, String::len);
            state.focus = Some(rux_runtime::SavedFocus {
                model,
                row: self.focused_row.clone(),
                instance: self.focused_instance.clone(),
                caret: self.caret,
                anchor: self.anchor,
                text,
            });
        }
        let others: Vec<(String, Option<String>, Option<String>)> = self
            .focuses
            .iter()
            .filter(|f| f.text.is_some() && kept_across_a_kill(f.kind, &f.field) && !f.field.readonly)
            .filter(|f| {
                self.focused.as_deref() != Some(f.model.as_str())
                    || f.row != self.focused_row
                    || f.instance != self.focused_instance
            })
            .map(|f| (f.model.clone(), f.row.clone(), f.instance.clone()))
            .collect();
        for (model, row, instance) in others {
            if state.fields.iter().any(|f| f.model == model && f.row == row && f.instance == instance) {
                continue;
            }
            let text = self.document.value_in(&model, row.as_deref(), instance.as_deref());
            if text.len() > budget {
                continue;
            }
            budget -= text.len();
            state.fields.push(rux_runtime::SavedField { model, row, instance, text });
        }
        // And every select's choice, as a native spinner keeps its own.
        let selects: Vec<(String, Option<String>, Option<String>)> = self
            .selects
            .iter()
            .filter(|s| !s.field.readonly && kept_across_a_kill(InputKind::Text, &s.field))
            .map(|s| (s.model.clone(), s.row.clone(), s.instance.clone()))
            .collect();
        for (model, row, instance) in selects {
            if state.fields.iter().any(|f| f.model == model && f.row == row && f.instance == instance) {
                continue;
            }
            let text = self.document.value_in(&model, row.as_deref(), instance.as_deref());
            if text.len() > budget {
                continue;
            }
            budget -= text.len();
            state.fields.push(rux_runtime::SavedField { model, row, instance, text });
        }
        let encoded = state.encode();
        #[cfg(target_os = "android")]
        if let Ok(mut slot) = SAVED_STATE.lock() {
            if *slot != encoded {
                *slot = encoded;
            }
        }
        #[cfg(target_arch = "wasm32")]
        web_save_state(encoded);
    }

    /// The focused input's region, matched on both halves of its identity.
    fn focused_region(&self) -> Option<&FocusRegion> {
        let model = self.focused.as_deref()?;
        self.focuses.iter().find(|f| {
            f.model == model
                && f.row.as_deref() == self.focused_row.as_deref()
                && f.instance.as_deref() == self.focused_instance.as_deref()
        })
    }

    /// The full-fidelity focus setter: caret, selection anchor *and* composition.
    ///
    /// Any caller that is not the IME leaves `preedit` at `None`, which is taken
    /// as "whatever was being composed is abandoned": clicking into another
    /// field, tabbing away or pressing Escape mid-composition all put the field
    /// back the way it was, rather than stranding half-typed text nobody chose.
    fn set_focus_range(&mut self, focus: Option<Focus>) {
        // The caret menu closes when the caret moves, not whenever focus is
        // restated. A long press on empty space focuses the field, the
        // keyboard attaches and reports the unchanged text straight back, and
        // closing the menu on that report closed it before it ever showed.
        // Driven on the phone: the handle came up, the menu never did.
        let restated = focus.as_ref().is_some_and(|f| {
            f.is(
                self.focused.as_deref().unwrap_or(""),
                self.focused_row.as_deref(),
                self.focused_instance.as_deref(),
            ) && f.caret == self.caret
                && f.anchor == self.anchor
        });
        if !restated {
            self.caret_menu = false;
        }
        if focus.as_ref().and_then(|f| f.preedit).is_none() {
            self.cancel_preedit();
        }
        // A different field starts unscrolled: the offset belongs to the text
        // being edited, and carrying it over would show the new field's value
        // already scrolled to somewhere the caret is not.
        let same_field = focus
            .as_ref()
            .is_some_and(|f| {
                f.is(
                    self.focused.as_deref().unwrap_or(""),
                    self.focused_row.as_deref(),
                    self.focused_instance.as_deref(),
                )
            });
        // Only a field whose value the person changed counts as having been
        // through, as browsers decide `:user-invalid`: tabbing past an empty
        // required field is not getting it wrong.
        let left = (!same_field)
            .then(|| {
                let model = self.focused.clone()?;
                let now = self.focused_signal_text();
                let changed = self.focus_value.as_deref() != Some(now.as_str());
                changed.then(|| (model, self.focused_row.clone(), self.focused_instance.clone()))
            })
            .flatten();
        if !same_field {
            self.text_scroll = 0.0;
            self.handles = false;
            self.caret_handle_until = None;
            self.handle_drag = None;
            // The field being left commits and blurs, in that order, as HTML
            // has it, and with its own handlers and its own instance.
            if self.focused.is_some() {
                self.commit_focused();
                let blur = self.focused_field.on_blur.clone();
                let value = self.focused_signal_text();
                self.queue_field_event(blur.as_deref(), &value);
            }
            // A draft ends with its field. The document drops its own copy
            // when its focus moves.
            self.typed_draft = None;
        }
        self.focused = focus.as_ref().map(|f| f.model.clone());
        self.focused_row = focus.as_ref().and_then(|f| f.row.clone());
        self.focused_instance = focus.as_ref().and_then(|f| f.instance.clone());
        self.caret = focus.as_ref().map(|f| f.caret).unwrap_or(0);
        self.anchor = focus.as_ref().map(|f| f.anchor).unwrap_or(0);
        self.document.set_focus(focus);
        // `:focus` matches on the focused input, so the document needs both
        // halves of its identity, or every row of a list matches at once.
        let model = self.focused.clone();
        let row = self.focused_row.clone();
        let instance = self.focused_instance.clone();
        self.update_focus_state(model, row, instance, left);
        if !same_field {
            self.focused_field = self.focused_region().map(|r| r.field.clone()).unwrap_or_default();
            // Autofill follows focus: the field entered is where a password
            // manager puts its suggestions. The field's own box, not a label's.
            #[cfg(target_os = "android")]
            {
                let scale = self.scale();
                let entered = self.focused.as_deref().and_then(|model| {
                    self.focuses.iter().find(|f| {
                        f.text.is_some()
                            && f.model == model
                            && f.row == self.focused_row
                            && f.instance == self.focused_instance
                            && f.field.autofill()
                    })
                });
                match entered {
                    Some(f) => {
                        let px = |v: f32| (v as f64 * scale).round() as i32;
                        let id = autofill_id(&f.model, f.row.as_deref(), f.instance.as_deref());
                        android_autofill_focus(id, [px(f.x), px(f.y), px(f.width), px(f.height)]);
                    }
                    None => android_autofill_focus(0, [0; 4]),
                }
            }
            if self.focused_is_number() {
                self.decimal = os_decimal_separator();
            }
            self.committed = None;
            if self.focused.is_some() {
                let value = self.focused_signal_text();
                let focus = self.focused_field.on_focus.clone();
                self.queue_field_event(focus.as_deref(), &value);
                self.focus_value = Some(value.clone());
                self.committed = Some(value);
            }
        }
        // A read-only field, or one whose keyboard is `none`, is focused
        // without one: the caret and the selection work, the keyboard stays
        // down.
        let keyboard =
            !self.focused_field.readonly && self.focused_field.keyboard != Keyboard::None;
        self.set_ime_enabled(self.focused.is_some() && keyboard);
        self.reset_blink();
        self.request_redraw();
    }

    /// Tell the platform whether to route composition at us.
    ///
    /// Off by default in winit, which is why Rux had no dead keys and no CJK
    /// input on any desktop: the events exist, nothing had ever asked for them.
    /// It is toggled with focus rather than left on, because while it is on the
    /// compositor may swallow plain keystrokes that the rest of the UI wants.
    fn set_ime_enabled(&mut self, on: bool) {
        // **Android goes first, and the order is the whole of why this works.**
        // `set_ime_allowed` below is what raises the keyboard, and raising it
        // makes the input method ask the focused view whether it is a text
        // editor. That view answers out of `WANTS_TEXT`. Setting the flag after
        // the call means the answer is still "no" when the question is asked,
        // so the keyboard does not appear and nothing asks again. Driven: with
        // this block last, tapping a field did nothing at all.
        #[cfg(target_os = "android")]
        {
            // Android's equivalent of the web's hidden input. An input method
            // reads this the moment it attaches, so it has to be written before
            // the keyboard opens rather than at the next edit. Cleared on blur
            // so a stale value cannot seed the next field.
            let value = if on { self.focused_value() } else { String::new() };
            // Where the selection is, in the connection's units, so a new
            // connection starts with the caret where Rux has it rather than at
            // the end. Anchor in the high half, caret in the low.
            let caret = self.caret.min(value.len());
            let anchor = self.anchor.min(value.len());
            FOCUSED_SELECTION.store(
                ((byte_to_utf16_index(&value, anchor) as i64) << 32)
                    | byte_to_utf16_index(&value, caret) as i64,
                std::sync::atomic::Ordering::Relaxed,
            );
            if let Ok(mut text) = FOCUSED_TEXT.lock() {
                *text = value.clone();
            }
            // What kind of field it is, which decides the keyboard Android
            // raises for it. See [`FOCUSED_KIND`].
            let kind = match (on, self.focused_kind) {
                (false, _) | (true, InputKind::Text) => KIND_TEXT,
                (true, InputKind::Textarea) => KIND_TEXTAREA,
                (true, InputKind::Password) => KIND_PASSWORD,
                (true, InputKind::Search) => KIND_SEARCH,
                (true, InputKind::Number) => KIND_NUMBER,
                // Opened as a picker, never typed into on a phone; a text
                // keyboard if a hardware Tab lands on one.
                (true, InputKind::Date) => KIND_TEXT,
            };
            // `inputmode` and `enterkeyhint` ride in the upper bytes, so one
            // native call still answers everything the `EditorInfo` needs.
            // Exhaustive for the same reason the kind is.
            let keyboard = match self.focused_field.keyboard {
                Keyboard::Text => 0,
                Keyboard::Numeric => 1,
                Keyboard::Decimal => 2,
                Keyboard::Tel => 3,
                Keyboard::Email => 4,
                Keyboard::Url => 5,
                Keyboard::Search => 6,
                // Never raised, see `set_focus_range`; a text keyboard if it is.
                Keyboard::None => 0,
            };
            // What the key will do, not only what was written: Next and Go in
            // a form, Done outside one.
            let enter = match self.enter_key() {
                EnterKey::Default => 0,
                EnterKey::Enter => 1,
                EnterKey::Done => 2,
                EnterKey::Go => 3,
                EnterKey::Next => 4,
                EnterKey::Previous => 5,
                EnterKey::Search => 6,
                EnterKey::Send => 7,
            };
            let (keyboard, enter) = if on { (keyboard, enter) } else { (0, 0) };
            FOCUSED_KIND.store(kind | keyboard << 8 | enter << 16, std::sync::atomic::Ordering::Relaxed);
            // Written after the text, so an input method that reads both in the
            // same breath cannot see "yes, and it is empty" for a field that
            // has contents.
            WANTS_TEXT.store(on, std::sync::atomic::Ordering::Relaxed);
            // And then the keyboard is raised or dropped from the Java side.
            // It cannot be done from here: an input method only serves the
            // focused view, and once Rux's own view took focus, the decor view
            // that `set_ime_allowed` asks for stopped being served.
            //
            // **Which field, not just whether there is one.** Moving from one
            // input to another is two "yes" answers in a row, and an input
            // method has to be told about the second or it keeps editing the
            // first. See [`android_set_text_input`].
            let field = on.then(|| {
                (
                    self.focused.clone().unwrap_or_default(),
                    self.focused_row.clone(),
                    self.focused_instance.clone(),
                )
            });
            // Bumped before the restart is asked for, so that anything the
            // outgoing connection says from here on carries a token that no
            // longer matches and is dropped. See [`FIELD_TOKEN`].
            if IME_FIELD.lock().map(|current| *current != field).unwrap_or(false) {
                FIELD_TOKEN.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
            if android_set_text_input(field) {
                // A new connection is being built from exactly what was
                // published above, so that is what it holds.
                self.ime_mirror = on.then(|| (value, caret, anchor));
            } else if on {
                self.sync_android_ime(value, caret, anchor);
            }
        }
        let Some(state) = self.state.as_ref() else { return };
        state.window.set_ime_allowed(on);
        if on {
            self.update_ime_area();
        }
        #[cfg(target_arch = "wasm32")]
        self.sync_web_ime();
    }


    /// Bring the input method's copy of the focused field back in step, when
    /// Rux changed the field without it.
    ///
    /// **An input connection holds its own copy of the text, and only the
    /// keyboard edits it.** Everything Rux does to a field by itself (Cut,
    /// Paste and Select all from the toolbar, a tap that moves the caret, a
    /// handler that writes the signal) left that copy as it was. The keyboard
    /// then edited the stale copy and reported the result, which Rux applied
    /// over the real one. Reported from the phone: select all, Cut from the
    /// toolbar, Paste from Gboard's clipboard, and the field held the text
    /// twice, because Gboard's copy still had all of it with the caret at the
    /// end. The web shell has always done this, in `sync_web_ime`.
    ///
    /// Only on drift, measured against the mirror of what the connection was
    /// last known to hold, and that is what keeps this from fighting the
    /// keyboard: an edit the keyboard made is recorded as it arrives, so
    /// applying it finds nothing to send back.
    ///
    /// **Changed text rebuilds the connection; a moved selection updates it.**
    /// Rewriting another object's `Editable` under a keyboard that caches the
    /// text around the caret is the kind of fix that works on one keyboard, so
    /// a text change goes the way a change of field already goes, and the
    /// connection is built fresh from what Rux holds. A new token drops
    /// anything the old one says on its way out.
    #[cfg(target_os = "android")]
    fn sync_android_ime(&mut self, value: String, caret: usize, anchor: usize) {
        let Some((held, held_caret, held_anchor)) = self.ime_mirror.as_ref() else {
            return;
        };
        if *held != value {
            FIELD_TOKEN.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            self.ime_mirror = Some((value, caret, anchor));
            android_restart_input();
        } else if (*held_caret, *held_anchor) != (caret, anchor) {
            let (caret16, anchor16) =
                (byte_to_utf16_index(&value, caret), byte_to_utf16_index(&value, anchor));
            self.ime_mirror = Some((value, caret, anchor));
            android_sync_selection(anchor16 as i32, caret16 as i32);
        }
    }

    /// Keep the hidden `<input>` in step with the focused field, and focus or
    /// blur it so the phone's keyboard opens and closes with the caret.
    ///
    /// Only on a touch device: see [`web_is_touch`]. Focusing it has to happen
    /// while the browser still considers a user gesture to be in progress, which
    /// is why this hangs off the focus change a tap causes rather than off a
    /// later frame.
    #[cfg(target_arch = "wasm32")]
    fn sync_web_ime(&mut self) {
        if !web_is_touch() {
            return;
        }
        // Nothing focused means nothing to type into, so the keyboard goes away.
        let Some(el) = (if self.focused.is_none() {
            web_ime_current()
        } else {
            web_ime_element(self.focused_kind.multiline())
        }) else {
            return;
        };
        if self.focused.is_none() {
            let _ = el.blur();
            return;
        }
        let value = self.focused_value();
        // Only touch it when it has actually drifted, which means the change
        // came from Rux (a handler, a tap moving the caret) rather than from the
        // keyboard. Writing the value or the selection back on every edit would
        // fight the browser for the caret mid-word, and the browser is the one
        // holding the composition.
        let caret16 = byte_to_utf16_index(&value, self.caret.min(value.len())) as u32;
        let anchor16 = byte_to_utf16_index(&value, self.anchor.min(value.len())) as u32;
        let (start, end, direction) = browser_selection(anchor16, caret16);
        if el.value() != value {
            el.set_value(&value);
            el.set_selection(start, end, direction);
        } else if el.selection_start() != Some(start) || el.selection_end() != Some(end) {
            // The text is unchanged but the selection moved on our side: a drag
            // across the canvas, a double-tap on a word, a handler selecting
            // all. The browser has to be told, because its own copy, cut and
            // select-all read the hidden input's selection and nothing else.
            // Leaving this out is what made copy on a phone act on no text.
            el.set_selection(start, end, direction);
        }
        // The keyboard the field asks for, said to the browser in the words it
        // already reads, so a phone's keyboard matches the one Android raises.
        // A number field with no `inputmode` of its own asks for `decimal`,
        // the closest a browser has to a number keyboard on a text input.
        let keyboard = match self.focused_field.keyboard {
            Keyboard::Text if self.focused_is_number() => Keyboard::Decimal,
            other => other,
        };
        let _ = el.set_attribute("inputmode", keyboard.name());
        match self.enter_key().name() {
            Some(hint) => {
                let _ = el.set_attribute("enterkeyhint", hint);
            }
            None => {
                let _ = el.remove_attribute("enterkeyhint");
            }
        }
        // What the field holds, for the browser's own autofill. The hidden
        // input stands in for every field in turn, so it is told each time.
        match self.focused_field.autocomplete.as_deref() {
            Some(tokens) => {
                let _ = el.set_attribute("autocomplete", tokens);
            }
            None => {
                let _ = el.remove_attribute("autocomplete");
            }
        }
        // A password is typed as one: the keyboard neither suggests nor learns
        // it, and the browser's password manager knows what it is looking at.
        // What Android's password input type does for a native field.
        if let WebIme::Line(input) = &el {
            let kind = if self.focused_kind == InputKind::Password { "password" } else { "text" };
            if input.type_() != kind {
                input.set_type(kind);
            }
        }
        let _ = el.focus();
        self.position_web_ime();
    }

    /// Lay the hidden input over the field it is editing, so that when the
    /// keyboard opens the browser scrolls to the right place and any native UI
    /// it anchors (the composition popup, the selection handles) lands on the
    /// text rather than in the corner of the page.
    #[cfg(target_arch = "wasm32")]
    fn position_web_ime(&mut self) {
        let Some(el) = web_ime_current() else { return };
        let Some(canvas) = WEB_CANVAS.with(|c| c.borrow().clone()) else { return };
        let Some(region) = self.focused_region() else { return };
        // Rux's logical pixels are CSS pixels, and the input is the canvas's
        // sibling, so the field's box offsets straight off the canvas's own.
        let (ox, oy) = (canvas.offset_left() as f32, canvas.offset_top() as f32);
        let style = el.style();
        let _ = style.set_property("left", &format!("{}px", ox + region.x));
        let _ = style.set_property("top", &format!("{}px", oy + region.y));
        let _ = style.set_property("width", &format!("{}px", region.width.max(1.0)));
        let _ = style.set_property("height", &format!("{}px", region.height.max(1.0)));
    }

    /// Apply an edit an Android input method made.
    ///
    /// The Android half of [`Self::apply_web_text`], and deliberately the same
    /// shape: the field's value is replaced outright, because the editing
    /// happened somewhere else and arrives as a result rather than a keystroke.
    /// It differs only in taking the composing range as a pair, which is what
    /// the platform reports, rather than as a length back from the caret.
    ///
    /// The snapshot an input method starts from is refreshed here too, so the
    /// next one to attach begins from what is actually in the field.
    #[cfg(target_os = "android")]
    fn apply_soft_keyboard_text(
        &mut self,
        value: String,
        caret: usize,
        anchor: usize,
        compose: Option<(usize, usize)>,
    ) {
        let Some(model) = self.focused.clone() else { return };
        let value = if self.focused_kind.multiline() {
            value.replace("\r\n", "\n")
        } else {
            value.replace(['\n', '\r'], "")
        };
        let caret = floor_char_boundary(&value, caret.min(value.len()));
        let anchor = floor_char_boundary(&value, anchor.min(value.len()));
        let preedit = compose.map(|(start, end)| {
            (
                floor_char_boundary(&value, start.min(value.len())),
                floor_char_boundary(&value, end.min(value.len())),
            )
        });
        // The input method is running the composition, so the shell's own
        // composition state stays empty and must not be restored over this.
        self.preedit = None;
        // A field that refused or cut the edit leaves the keyboard's copy
        // ahead of it; the focus below puts the caret back on what was kept,
        // and the drift it leaves is what brings the keyboard back in step.
        let (value, caret, anchor, preedit) =
            match self.write_focused(&value, caret, preedit.is_some()) {
                Some((written, _)) if written == value => (written, caret, anchor, preedit),
                Some((written, at)) => (written, at, at, None),
                None => {
                    let kept = self.focused_value();
                    let (caret, anchor) = (self.caret.min(kept.len()), self.anchor.min(kept.len()));
                    (kept, caret, anchor, None)
                }
            };
        self.scroll_caret_into_view(&value, caret);
        if let Ok(mut text) = FOCUSED_TEXT.lock() {
            *text = value.clone();
        }
        let row = self.focused_row.clone();
        let instance = self.focused_instance.clone();
        self.set_focus_range(Some(Focus { model, row, instance, caret, anchor, preedit }));
    }

    /// Apply an edit the browser's soft keyboard made.
    ///
    /// On a phone the text never arrives as key presses: the browser owns the
    /// editing, the composition and the autocorrect, and reports the result as
    /// the hidden input's new contents. So this replaces the field's value
    /// outright rather than applying a keystroke to it.
    #[cfg(target_arch = "wasm32")]
    fn apply_web_text(&mut self, value: String, caret: usize, anchor: usize, composing: usize) {
        let Some(model) = self.focused.clone() else { return };
        // A one-line field never takes a newline, the rule paste already follows.
        let value = if self.focused_kind.multiline() {
            value.replace("\r\n", "\n")
        } else {
            value.replace(['\n', '\r'], "")
        };
        let caret = floor_char_boundary(&value, caret.min(value.len()));
        let anchor = floor_char_boundary(&value, anchor.min(value.len()));
        let preedit = (composing > 0 && composing <= caret)
            .then(|| (floor_char_boundary(&value, caret - composing), caret));
        // The browser is running the composition, so the shell's own
        // composition state stays empty and must not be restored over this.
        self.preedit = None;
        // A field that refused or cut the edit leaves the keyboard's copy
        // ahead of it; the focus below puts the caret back on what was kept,
        // and the drift it leaves is what brings the keyboard back in step.
        let (value, caret, anchor, preedit) =
            match self.write_focused(&value, caret, preedit.is_some()) {
                Some((written, _)) if written == value => (written, caret, anchor, preedit),
                Some((written, at)) => (written, at, at, None),
                None => {
                    let kept = self.focused_value();
                    let (caret, anchor) = (self.caret.min(kept.len()), self.anchor.min(kept.len()));
                    (kept, caret, anchor, None)
                }
            };
        self.scroll_caret_into_view(&value, caret);
        // The row and the instance travel with the model: an input inside an
        // `r-for` is identified by both, and one inside a component by the
        // instance as well. Dropping either here would put the caret in every
        // row of the list, or in every instance of the component, at once.
        let row = self.focused_row.clone();
        let instance = self.focused_instance.clone();
        self.set_focus_range(Some(Focus { model, row, instance, caret, anchor, preedit }));
    }

    /// Park the candidate window under the caret instead of at the window's
    /// top-left, so the list of characters to choose from does not cover the text
    /// it is being chosen for.
    fn update_ime_area(&mut self) {
        let Some(window) = self.state.as_ref().map(|s| s.window.clone()) else { return };
        let scale = window.scale_factor();
        // The guard is that *something* is focused; the field itself comes
        // from ocused_region, which knows about rows.
        if self.focused.is_none() { return; }
        let Some(region) = self.focused_region().cloned() else {
            return;
        };
        let Some(t) = region.text.as_ref() else { return };
        let value = self.focused_value();
        let style = rux_paint::text_style(&t.content);
        let caret = self.caret.min(value.len());
        let (shown, shown_caret) = self.shown_text(&value, caret);
        let (cx, cy, ch) = self.text.caret_geometry(&shown, &style, Some(t.width), shown_caret);
        window.set_ime_cursor_area(
            winit::dpi::LogicalPosition::new((t.x + cx) as f64, (t.y + cy) as f64)
                .to_physical::<f64>(scale),
            winit::dpi::LogicalSize::new(rux_text::CARET_WIDTH as f64, ch as f64)
                .to_physical::<f64>(scale),
        );
    }

    /// Route a composition event from the platform's input method.
    ///
    /// This is the path that makes dead keys, accents and CJK work. Before it
    /// existed the shell read `KeyboardInput` only, so `´` then `e` produced two
    /// characters instead of `é`, and there was no way at all to type a language
    /// that spells one character out of several keystrokes.
    fn on_ime(&mut self, ime: &Ime) {
        match ime {
            // The method is attached. Nothing to do until text arrives.
            Ime::Enabled => {}
            Ime::Preedit(text, cursor) => self.set_preedit(text, *cursor),
            Ime::Commit(text) => self.commit_text(text),
            // The method detached (the window lost focus, the user switched
            // keyboards). Half-composed text was never chosen, so it goes back.
            Ime::Disabled => {
                self.cancel_preedit();
                self.request_redraw();
            }
        }
    }

    /// Show the text being composed, replacing whatever the last preedit showed.
    ///
    /// `cursor` is the platform's caret *within* the composition, as a byte
    /// range; we take its start, which is where compositors put the insertion
    /// point. `None` means it wants the caret after the whole thing.
    fn set_preedit(&mut self, text: &str, cursor: Option<(usize, usize)>) {
        let Some(model) = self.focused.clone() else { return };
        let mut value = self.focused_value();

        // Starting a composition lifts out whatever it is going to sit on top
        // of, so that abandoning it can put that back.
        let composing = match self.preedit.clone() {
            Some(p) => p,
            None => {
                let (start, end) = self.selection();
                let (start, end) = (start.min(value.len()), end.min(value.len()));
                let replaced = value[start..end].to_string();
                value.replace_range(start..end, "");
                Preedit { at: start, len: 0, replaced }
            }
        };

        let at = composing.at.min(value.len());
        let end = (at + composing.len).min(value.len());
        value.replace_range(at..end, text);

        // An empty preedit is how a compositor says the composition ended with
        // nothing chosen, which is a cancel, not a commit of "".
        if text.is_empty() {
            value.insert_str(at, &composing.replaced);
            let caret = at + composing.replaced.len();
            self.preedit = None;
            let caret = self.write_focused(&value, caret, false).map_or(self.caret, |(_, c)| c);
            self.set_focus_range(Some(self.focus_here(&model, caret)));
            return;
        }

        let caret = at + cursor.map(|(s, _)| s.min(text.len())).unwrap_or(text.len());
        // A read-only field takes no composition at all.
        if self.write_focused(&value, caret, true).is_none() {
            return;
        }
        self.preedit = Some(Preedit { at, len: text.len(), replaced: composing.replaced });
        self.scroll_caret_into_view(&value, caret);
        self.set_focus_range(Some(Focus {
            model,
            row: self.focused_row.clone(),
            instance: self.focused_instance.clone(),
            caret,
            anchor: caret,
            preedit: Some((at, at + text.len())),
        }));
        self.update_ime_area();
    }

    /// Accept composed text into the field for good.
    ///
    /// Also the path a plain keystroke takes on platforms whose input method
    /// stays in the loop even when nothing is being composed, so it has to
    /// behave like typing when there is no composition to replace.
    fn commit_text(&mut self, text: &str) {
        let Some(model) = self.focused.clone() else { return };
        let mut value = self.focused_value();
        let (start, end) = match self.preedit.take() {
            Some(p) => {
                let at = p.at.min(value.len());
                (at, (at + p.len).min(value.len()))
            }
            None => {
                let (s, e) = self.selection();
                (s.min(value.len()), e.min(value.len()))
            }
        };
        // A one-line input never takes a newline, the rule paste already follows.
        let text = if self.focused_kind.multiline() {
            text.replace("\r\n", "\n")
        } else {
            text.lines().next().unwrap_or("").to_string()
        };
        value.replace_range(start..end, &text);
        let Some((value, caret)) = self.write_focused(&value, start + text.len(), false) else {
            return;
        };
        self.scroll_caret_into_view(&value, caret);
        self.set_focus_range(Some(self.focus_here(&model, caret)));
        self.update_ime_area();
    }

    /// Abandon a composition, putting the field back exactly as it was before it
    /// started. A no-op when nothing is being composed, which is the usual case.
    fn cancel_preedit(&mut self) {
        let Some(p) = self.preedit.take() else { return };
        if self.focused.is_none() { return; }
        let mut value = self.focused_value();
        let at = p.at.min(value.len());
        let end = (at + p.len).min(value.len());
        value.replace_range(at..end, &p.replaced);
        self.write_focused(&value, at, false);
    }

    /// The focused input's selected byte range, low to high. Empty when there's
    /// no selection (`start == end`).
    fn selection(&self) -> (usize, usize) {
        (self.caret.min(self.anchor), self.caret.max(self.anchor))
    }

    /// The focused input's selected text, if any.
    fn selected_text(&mut self) -> Option<String> {
        self.focused.as_ref()?;
        let (start, end) = self.selection();
        if start == end {
            return None;
        }
        let value = self.focused_value();
        value.get(start.min(value.len())..end.min(value.len())).map(str::to_string)
    }

    /// Put `text` on the system clipboard, and say whether it got there.
    ///
    /// **The answer is for Cut**, which removes the text only once it is safe
    /// somewhere else. A Cut that deletes after a failed write is a delete with
    /// no undo, and that is exactly what the first Android build shipped.
    #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
    fn clipboard_write(&mut self, text: &str) -> bool {
        let Some(cb) = self.clipboard.as_mut() else { return false };
        match cb.set_text(text.to_string()) {
            Ok(()) => true,
            Err(e) => {
                eprintln!("rux: clipboard copy failed: {e}");
                false
            }
        }
    }

    /// Read the system clipboard. `None` when it's empty, holds non-text, or
    /// there's no clipboard at all.
    #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
    fn clipboard_read(&mut self) -> Option<String> {
        self.clipboard.as_mut()?.get_text().ok()
    }

    // On the web the clipboard is asynchronous and permission-gated. Writing can
    // be fired and forgotten; reading cannot, so it does not go through
    // `clipboard_read` at all: see `request_paste`.
    #[cfg(target_arch = "wasm32")]
    fn clipboard_write(&mut self, text: &str) -> bool {
        let Some(clipboard) = web_clipboard() else { return false };
        // The promise is deliberately dropped. A rejection (no permission, not a
        // secure context) means the copy did not happen, and there is nothing
        // useful to do about it in a UI with no place to report it.
        //
        // So a started write counts as a write. Waiting for the promise would
        // make Cut asynchronous for a failure that, in a secure context with
        // the page focused, the browser does not produce.
        let _ = clipboard.write_text(text);
        true
    }

    #[cfg(target_arch = "wasm32")]
    fn clipboard_read(&mut self) -> Option<String> {
        // Unreachable in practice: `request_paste` takes the async path on the
        // web. Kept so the native and web shells present the same surface.
        None
    }

    // Android's clipboard is `ClipboardManager`, reached through the activity
    // the same way the select picker is. See `ruxClipboardWrite` in the Java.
    //
    // **These were stubs, and the stubs were a data-loss bug.** The first APK
    // made copy and paste do nothing, reasoning that nothing was better than
    // half working. Then the drawn toolbar reached the phone with Cut on it,
    // and Cut removed the selection after a write that stored it nowhere.
    #[cfg(target_os = "android")]
    fn clipboard_write(&mut self, text: &str) -> bool {
        android_clipboard_write(text)
    }

    #[cfg(target_os = "android")]
    fn clipboard_read(&mut self) -> Option<String> {
        android_clipboard_read()
    }

    /// Show the caret solid and (re)start the blink cycle. Called on focus and on
    /// every edit, so the caret is steady while you type and only blinks at rest.
    /// Clearing focus stops the timer entirely, an idle window stays event-driven.
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
        // The phone's own highlight and accent, so a Rux field selects like
        // every other field on it. Retried until the activity has been handed
        // over, which can be after the first frame.
        #[cfg(target_os = "android")]
        if !self.theme_read {
            if let Some((highlight, accent)) = android_theme_colors() {
                rux_paint::set_default_selection(highlight);
                self.handle_accent = accent;
                self.theme_read = true;
            }
        }
        // Catches the first frame and any resize that arrived without an event
        // (hot-reload, scale change); a no-op unless a breakpoint moved.
        self.update_viewport();
        // Every navigation ends in a repaint, so this is the one place that
        // sees all of them, wherever they came from: a link, a handler, a key,
        // or a mouse button.
        #[cfg(target_arch = "wasm32")]
        self.sync_url();
        // Transitions are folded in here, after the build that changed the
        // styles and before the layout that measures them, so a box animating
        // its width is laid out at the width it is actually showing. The
        // animator hands back when it next needs a frame, or `None` to say the
        // window can go back to sleep.
        let now = self.epoch.elapsed().as_secs_f64() * 1000.0;
        // Enter/leave first: a swap that has just finished stops its element
        // being built, and there is nothing to interpolate into a node that is
        // about to leave the tree. It rebuilds when one commits, which is also
        // where the departing element's `unmounted` fires.
        let swap_next = self.document.advance_swaps(now);
        let next = match (self.anim.apply(&mut self.document.root, now), swap_next) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        self.anim_deadline = next.map(|ms| {
            Instant::now() + Duration::from_secs_f64(ms.max(rux_runtime::FRAME_MS) / 1000.0)
        });
        let caret_visible = self.caret_visible;
        let safe_top = self.safe_top();
        let caret_menu = self.caret_menu;
        let toolbar_actions = self.toolbar_actions();
        let handle_set = self.handle_set();
        let handle_color = self
            .focused_region()
            .and_then(|r| r.text.as_ref())
            .and_then(|t| t.content.selection_style.handle)
            .unwrap_or(self.handle_accent);
        // A masked field paints bullets, so its ends are measured in the
        // bullets' offsets. See `shown_text`.
        let (sel_start, sel_end) = self.selection();
        let (sel_start, sel_end) = if self.focused_kind.secret() {
            let value = self.focused_value();
            (rux_layout::masked_offset(&value, sel_start), rux_layout::masked_offset(&value, sel_end))
        } else {
            (sel_start, sel_end)
        };
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
            states,
            metrics,
            overlay_dismissed,
            overlay_rect,
            caret,
            anchor,
            text_scroll,
            focused,
            focused_row,
            focused_instance,
            focused_kind,
            selection_ends,
            reveal_focus,
            keyboard_reveal,
            #[cfg(not(target_arch = "wasm32"))]
            path,
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
        // Laid out in the part of the window the keyboard leaves, as
        // `adjustResize` promises and no longer delivers. See `keyboard_px`.
        let logical = (width as f64 / scale, height.saturating_sub(keyboard_px()) as f64 / scale);

        // A navigation has chosen where the arriving page should sit: the top
        // for one being opened, wherever it was left for one being returned to.
        // Taken before the layout so this frame is already laid out there,
        // rather than drawn in the wrong place and corrected on the next one.
        if let Some(restored) = document.take_scroll() {
            *offsets = restored;
        }

        // Layout (text sized via the engine's measure), then paint. Cache the
        // hit regions for tap dispatch.
        let mut layout = {
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
        // Remember where this page is, so returning to it can come back here.
        // Recorded once a frame rather than at each place that scrolls, and
        // after the clamp, so what is stored is a position that exists.
        document.record_scroll(offsets);

        // Hand this frame's geometry back, so a handler that runs before the
        // next one can ask where things are. One frame stale by construction:
        // these are the boxes currently on screen, which is what a script
        // asking "where is this" means.
        document.set_metrics(layout.metrics.clone());

        // What autofill would see if it asked now. Kept ready, because the
        // question arrives on Android's main thread and must be answered
        // before it returns.
        #[cfg(target_os = "android")]
        publish_autofill(&layout, document, scale, focused.as_deref(), focused_row, focused_instance);

        // `scrollIntoView()`: nudge the containing scroller until the element is
        // inside it. Applied here because the offsets are the shell's, and taken
        // after `set_metrics` so a reveal asked for by the handler that just ran
        // is resolved against the frame it was looking at.
        let mut revealed = false;
        for path in document.take_reveals() {
            let Some(m) = layout.metrics.iter().find(|e| e.path == path) else {
                continue;
            };
            // The innermost scroller containing it, matched against the
            // scroller's *content* rather than the part of it on screen. See
            // `containing_scroller`: the visible-rect test failed for exactly
            // the element a reveal is usually asked about, the one below the
            // fold, so a list that scrolled itself to its newest row did
            // nothing at all.
            let Some(at) =
                rux_layout::containing_scroller(&layout.scrolls, offsets, m.x, m.y)
            else {
                continue;
            };
            let region = &layout.scrolls[at];
            // The rect is already shifted by the current offset, so the
            // correction is the overhang, not an absolute position.
            let off = &mut offsets[region.id];
            if m.y < region.y {
                off.y -= region.y - m.y;
            } else if m.y + m.height > region.y + region.height {
                off.y += (m.y + m.height) - (region.y + region.height);
            }
            *off = off.clamp_to(region.max);
            revealed = true;
        }
        // The window changed size under a focused field, which is what a
        // phone's keyboard opening does to it: `adjustResize` shrinks the
        // window, and the field that raised the keyboard can end up beneath
        // it. Bring it back into view, as Android's own apps do. Watchlist #19.
        if std::mem::take(reveal_focus) {
            let field = focused.as_deref().and_then(|model| {
                layout.focuses.iter().find(|f| {
                    f.text.is_some()
                        && f.model == model
                        && f.row == *focused_row
                        && f.instance == *focused_instance
                })
            });
            if let Some(f) = field {
                if let Some(at) = rux_layout::containing_scroller(&layout.scrolls, offsets, f.x, f.y) {
                    let region = &layout.scrolls[at];
                    let off = &mut offsets[region.id];
                    let before = *off;
                    if f.y < region.y {
                        off.y -= region.y - f.y;
                    } else if f.y + f.height > region.y + region.height {
                        off.y += (f.y + f.height) - (region.y + region.height);
                    }
                    *off = off.clamp_to(region.max);
                    if *off != before {
                        // The first place the page was, kept across a
                        // second reveal, so closing the keyboard goes back
                        // to where the person had it.
                        let first = keyboard_reveal
                            .filter(|(id, ..)| *id == region.id)
                            .map_or(before, |(_, first, _)| first);
                        *keyboard_reveal = Some((region.id, first, *off));
                    }
                    revealed = true;
                }
            }
        }
        // The corrected offset takes effect on the next layout, so ask for one.
        // Through `state` rather than `self.request_redraw()`: `self` is already
        // split-borrowed here so the text engine and the render state can be
        // held at once.
        if revealed {
            state.window.request_redraw();
        }

        // Keep the focused single-line input's caret inside its box.
        //
        // Done here, once per frame, rather than at each place the caret moves:
        // typing, arrows, Home/End, a tap, a drag, an IME commit and the
        // browser's own keyboard all end up here, and one rule covers them all
        // where six call sites would eventually disagree.
        let shift = Self::track_caret_x(
            &layout,
            focused.as_deref(),
            focused_row.as_deref(),
            focused_instance.as_deref(),
            *caret,
            text_scroll,
            text,
            document,
            focused_kind.secret(),
        );
        if shift != 0.0 {
            // Only the focused input has a caret, so this finds exactly one text
            // paint. Everything the painter draws for it (glyphs, caret,
            // selection, preedit) is placed from this single x, so moving it
            // moves them together, and the box's own clip hides the rest.
            for paint in layout.paints.iter_mut() {
                if let Paint::Text(t) = paint {
                    if t.content.caret.is_some() {
                        t.x -= shift;
                    }
                }
            }
        }

        // Where the selection's two ends are drawn this frame, for the handles
        // below and for the touch that follows. Measured on the text paint as
        // it will be drawn, scroll and all, so a handle cannot sit one frame
        // behind the text it belongs to while a list scrolls.
        *selection_ends = focused.as_deref().and_then(|m| {
            let region = layout.focuses.iter().find(|f| {
                f.model == m
                    && f.row.as_deref() == focused_row.as_deref()
                    && f.instance.as_deref() == focused_instance.as_deref()
            })?;
            let t = layout.paints.iter().find_map(|p| match p {
                Paint::Text(t) if t.content.caret.is_some() => Some(t),
                _ => None,
            })?;
            let style = rux_paint::text_style(&t.content);
            let mut end = |index: usize| {
                let (cx, cy, ch) = text.caret_geometry(&t.content.text, &style, Some(t.width), index);
                let (x, bottom) = (t.x + cx, t.y + cy + ch);
                let visible = x >= region.x - 1.0
                    && x <= region.x + region.width + 1.0
                    && bottom > region.y
                    && bottom <= region.y + region.height + 1.0;
                SelectionEnd { x, bottom, height: ch, visible }
            };
            Some([end(sel_start), end(sel_end)])
        });

        let content = rux_paint::build_scene(&layout.paints, text, images, caret_visible);
        state.scene.reset();
        state
            .scene
            .append(&content, Some(Affine::scale(scale)));

        // Scrollbars go over the content: they're an overlay on the box's own
        // trailing edge, and a scroller clips its children, so they can't be
        // painted as part of the subtree.
        //
        // Being outside the subtree, they are also outside its `transform` and
        // `opacity`, and that showed: a page transitioning at `opacity: 0` drew
        // its scrollbar at full strength over the page it was leaving, and a
        // scrollbar on a sliding page stayed where the page used to be. So each
        // region is drawn through the lens its own box is drawn through, one
        // bar at a time, since two scrollers can be under different transforms.
        for region in &layout.scrolls {
            if region.alpha <= 0.001 {
                continue;
            }
            let bars = scrollbar_paints(std::slice::from_ref(region), offsets, region.alpha);
            if bars.is_empty() {
                continue;
            }
            let scene = rux_paint::build_scene(&bars, text, images, false);
            state.scene.append(&scene, Some(Affine::scale(scale) * to_affine(region.transform)));
        }

        // The selection toolbar, over the content while something is selected.
        // It is the only route to copy and paste on a phone, and on the web at
        // all, so it is drawn above the page rather than inside it.
        if (*caret != *anchor || caret_menu) && !cfg!(target_os = "android") {
            if let Some(r) = focused.as_deref().and_then(|m| {
                layout
                    .focuses
                    .iter()
                    .find(|f| {
                        f.model == m
                            && f.row.as_deref() == focused_row.as_deref()
                            && f.instance.as_deref() == focused_instance.as_deref()
                    })
            }) {
                let strip = toolbar_paints(
                    (r.x, r.y, r.width, r.height),
                    (logical.0 as f32, logical.1 as f32),
                    safe_top,
                    &toolbar_actions,
                );
                let scene = rux_paint::build_scene(&strip, text, images, false);
                state.scene.append(&scene, Some(Affine::scale(scale)));
            }
        }

        // The selection handles, over the page and the toolbar, under a
        // dropdown. Only an end that is inside its field gets one: a handle
        // pointing at text scrolled out of sight points at nothing.
        if let Some(ends) = *selection_ends {
            let mut marks = Vec::new();
            for &handle in &handle_set {
                let end = if handle == Handle::End { ends[1] } else { ends[0] };
                if end.visible {
                    marks.extend(handle_paints(handle, (end.x, end.bottom), handle_color));
                }
            }
            if !marks.is_empty() {
                let scene = rux_paint::build_scene(&marks, text, images, false);
                state.scene.append(&scene, Some(Affine::scale(scale)));
            }
        }

        // An open `select` draws its dropdown on top of everything else.
        if let Some((model, row, instance)) = open_select.clone() {
            if let Some(sel) = layout
                .selects
                .iter()
                .find(|s| s.model == model && s.row == row && s.instance == instance)
            {
                let value = document.value_in(&model, row.as_deref(), instance.as_deref());
                let overlay = dropdown_paints(sel, &value);
                let scene = rux_paint::build_scene(&overlay, text, images, false);
                state.scene.append(&scene, Some(Affine::scale(scale)));
            }
        }

        // The dev overlay goes last, above everything including a dropdown: if the
        // document is broken, that is the most important thing on screen.
        let diagnostics = document.diagnostics();
        // Dismissal is remembered against the diagnostics it was for, so fixing
        // one thing and breaking another brings the panel straight back rather
        // than leaving it hidden until restart.
        *overlay_rect = None;
        if overlay_visible(diagnostics, overlay_dismissed.as_ref()) {
            #[cfg(not(target_arch = "wasm32"))]
            let panel = overlay_paints(diagnostics, path, logical.0 as f32);
            // No file on the web, so the overlay titles itself after the editor.
            #[cfg(target_arch = "wasm32")]
            let panel =
                overlay_paints(diagnostics, Path::new("playground.rux"), logical.0 as f32);
            if let Some(panel) = panel {
                let scene = rux_paint::build_scene(&panel.paints, text, images, false);
                state.scene.append(&scene, Some(Affine::scale(scale)));
                *overlay_rect = Some(panel.rect);
            }
        }

        // Publish the accessibility tree for this frame. `update_if_active` skips
        // the work entirely unless assistive technology is attached, so the common
        // case pays only for the (already computed) node list.
        // Native only: the web already has an accessibility tree of its own, and
        // accesskit_winit has no adapter for it.
        #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
        {
            let window_title = state.window.title();
            state.access.update_if_active(|| {
                access_tree(&layout.access, focused.as_deref(), scale, &window_title)
            });
        }

        *hits = layout.hits;
        *focuses = layout.focuses;
        *metrics = layout.metrics.clone();
        // A field that is no longer in the tree must not stay focused. Nothing
        // dropped focus when its input went away: only a web source reload ever
        // cleared it, so navigating off a page you had been typing on left the
        // shell believing that field was still there.
        //
        // It shows up worst on the web, where the hidden `<input>` holds real
        // DOM focus and a phone's on-screen keyboard would stay up over the
        // page you just moved to. Identity is `(model, row, instance)`, the same
        // three the caret uses, or one row of a list would answer for another,
        // and so would one instance of a component for its twin.
        if let Some(model) = focused.clone() {
            let still_here = focuses.iter().any(|f| {
                f.model == model
                    && f.row.as_deref() == focused_row.as_deref()
                    && f.instance.as_deref() == focused_instance.as_deref()
            });
            if !still_here {
                *focused = None;
                *focused_row = None;
                *focused_instance = None;
                *text_scroll = 0.0;
                #[cfg(target_arch = "wasm32")]
                if let Some(el) = web_ime_current() {
                    let _ = el.blur();
                }
            }
        }
        *selects = layout.selects;
        // Keep the focus index in range if the new layout has fewer focusables.
        if focus_index.map(|i| i >= layout.focusables.len()).unwrap_or(false) {
            *focus_index = None;
        }
        *focusables = layout.focusables;
        *scrolls = layout.scrolls;
        *states = layout.states;

        let device_handle = &context.devices[state.surface.dev_id];
        // wgpu 29 reports acquisition as a status enum. A timeout/occluded frame
        // is normal (minimized window, compositor hiccup), skip it and repaint
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

        // After the present and not before: the splash is covering an empty
        // surface, and telling Android to take it away while the frame is still
        // queued would put the black back.
        #[cfg(target_os = "android")]
        android_first_frame();

        // The hidden input is placed from `self.focuses`, which only becomes the
        // *current* layout here. Placing it during the focus change instead
        // would use the previous frame's geometry, so it sat one edit behind
        // whenever an edit moved the field it covers.
        #[cfg(target_arch = "wasm32")]
        self.position_web_ime();
    }
}

/// Build the vello renderer for a freshly created surface. Shared by both
/// platforms so they cannot drift in their renderer options.
fn make_renderer(context: &RenderContext, surface: &RenderSurface<'static>) -> Renderer {
    Renderer::new(
        &context.devices[surface.dev_id].device,
        RendererOptions {
            use_cpu: false,
            antialiasing_support: AaSupport::area_only(),
            num_init_threads: NonZeroUsize::new(1),
            pipeline_cache: None,
        },
    )
    .expect("create renderer")
}

impl ApplicationHandler<RuxEvent> for App {
    #[cfg(not(target_arch = "wasm32"))]
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.state.is_some() {
            return;
        }

        let title = format!(
            "Rux · {}",
            self.path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "M2".into())
        );
        // Created hidden: the accessibility adapter must exist before the window
        // is first shown, or it panics. Revealed again once the adapter is up.
        // A preview opens at the device's own logical size; everything else at
        // the size this shell has always used.
        let (open_w, open_h) = match self.preview {
            Some(profile) => Self::preview_size(event_loop, profile),
            None => (420.0, 640.0),
        };
        let attributes = Window::default_attributes()
            .with_title(title)
            .with_visible(false)
            .with_inner_size(winit::dpi::LogicalSize::new(open_w, open_h));
        // Android ignores the title, the size and the visibility above: an
        // activity gets the display it is given. They are set anyway rather than
        // branched around, because winit accepts them everywhere and a second
        // code path here would be two ways to open a window and one of them
        // rarely exercised.
        let window = Arc::new(event_loop.create_window(attributes).expect("create window"));
        #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
        let access = accesskit_winit::Adapter::with_event_loop_proxy(
            event_loop,
            &window,
            self.proxy.clone(),
        );
        window.set_visible(true);

        let size = window.inner_size();
        let surface = pollster::block_on(self.context.create_surface(
            window.clone(),
            size.width.max(1),
            size.height.max(1),
            wgpu::PresentMode::AutoVsync,
        ))
        .expect("create surface");

        // A renderer outlives the surface it was made for: it is built from the
        // *device*, and the device survives a suspend. Building one compiles
        // shaders, which is most of the delay when coming back to an app, so
        // the one taken apart in `suspended` is put back rather than remade.
        // Only for the same device, since that is the one thing it is tied to.
        let renderer = match self.spare_renderer.take() {
            Some((dev, renderer)) if dev == surface.dev_id => renderer,
            _ => make_renderer(&self.context, &surface),
        };
        self.state = Some(RenderState {
            window,
            surface,
            renderer,
            scene: Scene::new(),
            #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
            access,
        });
        self.request_redraw();
    }

    /// Android takes the activity's surface away whenever the app leaves the
    /// screen, and hands back a **different** one on the way in. The old one is
    /// dead, so the render state built on it has to go with it.
    ///
    /// Without this the app comes back **black**, and looks like it crashed
    /// while in fact it is drawing perfectly into a surface nobody is showing.
    /// `resumed` returns early when `state` is already `Some`, which is correct
    /// on a desktop, where it fires once for the life of the process. On Android
    /// it fires again on every return, so leaving the dead state in place is
    /// what makes the guard skip the rebuild. Dropping it here is what turns
    /// that same guard back into "build the state when there is none".
    ///
    /// **Nothing the person sees is lost.** The document, the signals and the
    /// scroll offsets live on `App`, not in `RenderState`, so the app comes back
    /// where it was; only the window, surface, renderer and scene are rebuilt.
    ///
    /// Gated to Android on purpose. A desktop window is never suspended, and a
    /// handler that threw the surface away there would be a way to lose a window
    /// that nothing ever exercises.
    /// **The renderer is kept.** Dropping the whole of `RenderState` is correct
    /// and was visibly slow: rebuilding a renderer compiles shaders, and the app
    /// showed up to three seconds of black on every return. It is tied to the
    /// device, not to the surface, and the device survives, so it is set aside
    /// here and picked up again in `resumed`.
    #[cfg(target_os = "android")]
    fn suspended(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(state) = self.state.take() {
            self.spare_renderer = Some((state.surface.dev_id, state.renderer));
        }
    }

    /// The web version of the same thing. `create_surface` is async and there is
    /// no blocking on a browser's main thread, so setup runs as a task: it builds
    /// its own `RenderContext` (cheap, and sidesteps borrowing `self` across an
    /// await), parks the result in `self.pending`, and wakes the loop with
    /// `SurfaceReady`.
    #[cfg(target_arch = "wasm32")]
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        use winit::platform::web::WindowAttributesExtWebSys;

        if self.state.is_some() || self.starting {
            return;
        }
        self.starting = true;

        let canvas = WEB_CANVAS.with(|c| c.borrow().clone());
        let (lw, lh) = WEB_SIZE.with(|s| *s.borrow());
        let attributes = Window::default_attributes()
            .with_canvas(canvas)
            .with_inner_size(winit::dpi::LogicalSize::new(lw, lh));
        let window = Arc::new(event_loop.create_window(attributes).expect("create window"));

        let pending = self.pending.clone();
        let proxy = WEB_PROXY.with(|p| p.borrow().clone()).expect("event loop proxy");

        // `inner_size()` is 0×0 until the resize observer has fired at least
        // once, which has usually not happened yet. Fall back to the size we
        // just asked for rather than configuring a 1×1 surface.
        let mut size = window.inner_size();
        if size.width == 0 || size.height == 0 {
            size = winit::dpi::LogicalSize::new(lw, lh).to_physical(window.scale_factor());
        }
        web_sys::console::log_1(
            &format!(
                "rux: canvas {lw}x{lh} css, surface {}x{} physical, dpr {}",
                size.width,
                size.height,
                window.scale_factor()
            )
            .into(),
        );

        wasm_bindgen_futures::spawn_local(async move {
            let mut context = RenderContext::new();
            let surface = context
                .create_surface(
                    window.clone(),
                    size.width.max(1),
                    size.height.max(1),
                    wgpu::PresentMode::AutoVsync,
                )
                .await
                .expect("create surface");
            let renderer = make_renderer(&context, &surface);

            *pending.borrow_mut() = Some((
                context,
                RenderState { window, surface, renderer, scene: Scene::new() },
            ));
            let _ = proxy.send_event(RuxEvent::SurfaceReady);
        });
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: RuxEvent) {
        match event {
            #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
            RuxEvent::Reload => self.reload(),

            // The device that owns the surface lives in the context the task
            // built, so that context replaces the placeholder one here.
            #[cfg(target_arch = "wasm32")]
            RuxEvent::SurfaceReady => {
                if let Some((context, state)) = self.pending.borrow_mut().take() {
                    self.context = context;
                    self.state = Some(state);
                    self.starting = false;
                }
            }

            #[cfg(target_arch = "wasm32")]
            RuxEvent::SetSource(source) => self.set_source(source),

            #[cfg(target_arch = "wasm32")]
            RuxEvent::WebText { value, caret, anchor, composing } => {
                self.apply_web_text(value, caret, anchor, composing)
            }

            // The same thing the web does, by a different road. Both platforms
            // let something else own the editing and report the result whole.
            #[cfg(target_os = "android")]
            RuxEvent::AndroidText { value, caret, anchor, compose } => {
                // Recorded as the connection reported it, before Rux applies
                // it. If applying changes anything (a newline stripped from a
                // one-line field), the two differ and the sync sends it back.
                self.ime_mirror = Some((value.clone(), caret, anchor));
                self.apply_soft_keyboard_text(value, caret, anchor, compose)
            }

            // The platform picker closed. Taken rather than read, so a second
            // answer for a picker that is no longer open cannot arrive and edit
            // a field nobody was looking at.
            #[cfg(any(target_os = "android", target_arch = "wasm32"))]
            RuxEvent::PickedSelect { index } => {
                if let Some((model, row, instance, options)) = self.pending_select.take() {
                    // `None` is a dismissal, which keeps what the field had.
                    // Out of range would mean Java and Rux disagreed about the
                    // list, which is worth ignoring rather than guessing at.
                    if let Some(option) = index.and_then(|i| options.get(i)) {
                        let change = self
                            .selects
                            .iter()
                            .find(|s| s.model == model && s.row == row && s.instance == instance)
                            .and_then(|s| s.field.on_change.clone());
                        self.choose_option(&model, row, instance, option, change);
                    }
                    self.request_redraw();
                }
            }

            // A date is committed as it is chosen, as a select is, so the
            // same write serves it: `@change` if the day moved.
            #[cfg(any(target_os = "android", target_arch = "wasm32"))]
            RuxEvent::PickedDate { value } => {
                if let Some((model, row, instance, change)) = self.pending_date.take() {
                    if let Some(value) = value {
                        self.choose_option(&model, row, instance, &value, change);
                    }
                    self.request_redraw();
                }
            }

            // Enter from the input method: its action key (`true`) or a plain
            // Enter in a one-line field. See `nativeEnter`.
            // The page now has less room (or all of it back): lay it out
            // again, and bring the focused field above the keyboard.
            // Hot reload: patch the documents and load again, only if the
            // files differ from what is loaded. The first batch on every
            // connection is the whole project, and it usually matches.
            #[cfg(target_os = "android")]
            RuxEvent::DevFiles(changes) => {
                if let Some(files) = self.dev_files.as_mut() {
                    if apply_dev_changes(files, changes) {
                        rux_runtime::set_source(std::rc::Rc::new(files.clone()));
                        self.reload();
                        android_log("hot reload: reloaded");
                        self.request_redraw();
                    }
                }
            }

            #[cfg(target_os = "android")]
            RuxEvent::AndroidKeyboard => {
                self.update_viewport();
                if keyboard_px() == 0 {
                    // Closed: put back what the reveal moved, unless the
                    // person has scrolled since, in which case where they
                    // scrolled to is where they want to be.
                    if let Some((id, before, after)) = self.keyboard_reveal.take() {
                        if self.offsets.get(id) == Some(&after) {
                            self.offsets[id] = before;
                        }
                    }
                } else {
                    self.reveal_focus = self.focused.is_some();
                }
                self.request_redraw();
            }

            // Autofill filled a field. HTML fires `input` and then `change` on
            // a field autofill fills; a focused field gets `@change` when it is
            // left, as for typing, so it is not told twice.
            #[cfg(target_os = "android")]
            RuxEvent::AndroidAutofill { id, value } => {
                let known = AUTOFILL
                    .lock()
                    .ok()
                    .and_then(|a| a.0.iter().find(|(i, _)| *i == id).map(|(_, f)| f.clone()));
                if let Some((model, row, instance)) = known {
                    let focused = self.focused.as_deref() == Some(model.as_str())
                        && self.focused_row == row
                        && self.focused_instance == instance;
                    let field = self
                        .focuses
                        .iter()
                        .find(|f| f.model == model && f.row == row && f.instance == instance)
                        .map(|f| f.field.clone())
                        .unwrap_or_default();
                    if focused {
                        // The keyboard's copy is brought up to date by the next
                        // frame's `sync_android_ime`, with a new token. A
                        // restart from here kept the old token, and the old
                        // connection's empty report then wiped the fill.
                        // It queues the field's `@input` itself, as for typing,
                        // and hands back where the caret goes: after the text.
                        if let Some((_, caret)) = self.write_focused(&value, value.len(), false) {
                            self.set_focus_range(Some(self.focus_here(&model, caret)));
                        }
                    } else {
                        self.document.apply_edit_in(&model, row.as_deref(), instance.as_deref(), &value);
                        let text = rux_reactive::Value::Text(value);
                        self.queue_field_event_in(field.on_input.as_deref(), instance.clone(), text.clone());
                        self.queue_field_event_in(field.on_change.as_deref(), instance, text);
                    }
                    self.request_redraw();
                }
            }

            #[cfg(target_os = "android")]
            RuxEvent::AndroidEnter(action) => {
                if self.focused.is_some() && !self.focused_kind.multiline() {
                    self.enter_in_field(action);
                    self.request_redraw();
                }
            }

            #[cfg(target_os = "android")]
            RuxEvent::AndroidTextAction(MENU_ACTION_SELECT_WORD) => {
                let value = self.focused_value();
                if let (Some(model), Some((start, end))) =
                    (self.focused.clone(), word_near(&value, self.caret))
                {
                    self.set_focus_range(Some(Focus {
                        model,
                        row: self.focused_row.clone(),
                        instance: self.focused_instance.clone(),
                        caret: end,
                        anchor: start,
                        preedit: None,
                    }));
                    self.handles = true;
                }
            }

            #[cfg(target_os = "android")]
            RuxEvent::AndroidTextAction(code) => {
                let action = match code {
                    MENU_ACTION_COPY => Some(TextAction::Copy),
                    MENU_ACTION_CUT => Some(TextAction::Cut),
                    MENU_ACTION_PASTE => Some(TextAction::Paste),
                    MENU_ACTION_SELECT_ALL => Some(TextAction::SelectAll),
                    _ => None,
                };
                if let Some(action) = action {
                    self.run_text_action(action);
                    match action {
                        // Android's own fields let go of a selection once it
                        // is copied, leaving the caret at its end. The drawn
                        // toolbar keeps it, which is what a desktop does.
                        TextAction::Copy => self.collapse_selection(),
                        TextAction::SelectAll => self.handles = true,
                        _ => {}
                    }
                }
            }

            #[cfg(target_os = "android")]
            RuxEvent::AndroidTextMenuClosed { collapse } => {
                // Java's menu is gone either way, so the next one is asked
                // for fresh rather than compared against one that is not up.
                self.text_menu_sent = None;
                if collapse {
                    self.collapse_selection();
                } else {
                    self.text_menu_dismissed = Some((self.anchor, self.caret));
                }
                self.request_redraw();
            }

            // The answer lands on whatever is selected now, which is what
            // was selected when the app was opened: the token Java checked
            // says the field is the same, and nothing else could move the
            // selection while the other app was in front.
            #[cfg(target_os = "android")]
            RuxEvent::AndroidProcessedText(text) => {
                if let Some(model) = self.focused.clone() {
                    self.apply_paste(&model, &text);
                    self.request_redraw();
                }
            }

            // A link to an app that was already open. **Pushed, not started
            // at**: the person was somewhere, and Back should return them
            // there rather than out of the app. Guards run as for any tap.
            #[cfg(target_os = "android")]
            RuxEvent::AndroidLink(route) => {
                android_log(&format!("link: {route}"));
                if self.document.open_link(&route, true) {
                    self.request_redraw();
                }
            }

            #[cfg(target_arch = "wasm32")]
            RuxEvent::WebRoute(index) => self.apply_web_route(index),

            // The key is the phone keyboard's action key, labelled from
            // `enterkeyhint`, so it does what its label says, as on Android. A
            // textarea's hidden twin takes its Enter as a new line itself.
            #[cfg(target_arch = "wasm32")]
            RuxEvent::WebEnter => {
                if self.focused.is_some() && !self.focused_kind.multiline() {
                    self.enter_in_field(true);
                    self.sync_web_ime();
                    self.request_redraw();
                }
            }

            // The clipboard read started by a paste has come back. The field may
            // have lost focus in the meantime, in which case there is nowhere to
            // put it and dropping it is right.
            #[cfg(target_arch = "wasm32")]
            RuxEvent::WebPaste(text) => {
                if let Some(model) = self.focused.clone() {
                    self.apply_paste(&model, &text);
                    self.sync_web_ime();
                    self.request_redraw();
                }
            }

            // Asking winit to resize restyles the canvas and then reports a
            // `Resized`, which reconfigures the surface through the same path a
            // desktop window resize takes. Going through winit rather than
            // setting CSS directly is what keeps the canvas's displayed size and
            // its surface size equal: taps are hit-tested against that geometry,
            // so any divergence misaligns every tap by the ratio.
            #[cfg(target_arch = "wasm32")]
            RuxEvent::Resize(w, h) => {
                if let Some(state) = self.state.as_ref() {
                    let _ = state
                        .window
                        .request_inner_size(winit::dpi::LogicalSize::new(w.max(1.0), h.max(1.0)));
                }
            }

            #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
            RuxEvent::Access(event) => {
                match event.window_event {
                    // Assistive technology just attached: it needs the whole tree,
                    // which the next frame publishes.
                    accesskit_winit::WindowEvent::InitialTreeRequested => {}
                    // It asked to focus or activate something. Our own focus model
                    // drives the app, so the tree is simply re-published; wiring
                    // these to real actions is the next slice.
                    accesskit_winit::WindowEvent::ActionRequested(_) => {}
                    accesskit_winit::WindowEvent::AccessibilityDeactivated => {}
                }
                self.request_redraw();
                return;
            }
        }
        self.request_redraw();
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _id: WindowId,
        event: WindowEvent,
    ) {
        // The adapter needs to see window events (focus, resize) to keep the
        // platform's view of the window in step. It observes; we still handle
        // every event ourselves below.
        #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
        if let Some(state) = self.state.as_mut() {
            state.access.process_event(&state.window, &event);
        }
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
                self.update_viewport();
                // Revealed once the next layout says where the field now is.
                self.reveal_focus = self.focused.is_some();
                self.request_redraw();
            }
            // The person changed light or dark mode while the app was running.
            // The window's own theme is already updated by the time this
            // arrives, so rebuilding the environment from it is enough, and
            // the document re-cascades only if a query changed answer.
            WindowEvent::ThemeChanged(_) => {
                self.update_viewport();
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
                // Shift+wheel scrolls horizontally, the platform convention for a
                // wheel with only one axis.
                let (dx, dy) = if self.shift_held && dx == 0.0 { (dy, 0.0) } else { (dx, dy) };
                self.scroll_at(self.pointer, -dx, -dy);
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.pointer = (position.x, position.y);
                let scale = self.scale();
                let here = ((position.x / scale) as f32, (position.y / scale) as f32);
                if let Some(p) = self.points.iter_mut().find(|(id, _)| *id == 0) {
                    p.1 = here;
                }
                self.move_gesture(here.0, here.1);
                if self.bar_drag.is_some() {
                    self.drag_scrollbar(self.pointer);
                } else if self.text_drag {
                    self.drag_text(self.pointer);
                } else {
                    self.update_cursor();
                    self.update_pointer_state();
                }
            }
            // The pointer left the window entirely, no CursorMoved follows, so
            // hover/active have to be dropped here or they stay lit.
            WindowEvent::CursorLeft { .. } => self.clear_pointer_state(),
            // Touch follows the same path as the mouse: press, drag, release.
            // It used to only scroll, which meant a finger could never tap
            // anything. That went unnoticed because there was no touch hardware
            // to try it on, and it is the first thing someone on a phone does.
            //
            // The one behaviour touch does *not* share: dragging on content that
            // is neither a scrollbar nor text scrolls that content directly. The
            // finger stays on the pixel it grabbed, so the content follows it and
            // the offset moves the other way.
            WindowEvent::Touch(touch) => {
                let at = (touch.location.x, touch.location.y);
                let scale = self.scale();
                let here = ((at.0 / scale) as f32, (at.1 / scale) as f32);
                match touch.phase {
                    TouchPhase::Started => {
                        self.keyboard_modality = false;
                        // There is no hover on a touchscreen, so the pointer only
                        // exists while a finger is down and has to be set here.
                        // Every helper below reads it.
                        self.pointer = at;
                        self.touch = Some(here);
                        // A finger landing on a list that is still coasting
                        // stops it, which is how every phone behaves and is the
                        // only way to catch a long throw. It happens before
                        // anything else in this arm so the press lands on the
                        // content where the finger actually met it.
                        self.fling = None;
                        self.scroll_target = None;
                        self.touch_track.clear();
                        self.track_touch(here);
                        // Recorded before anything decides what this press means,
                        // so a handler sees every finger that is down whether or
                        // not this one turned out to be a tap.
                        self.points.retain(|(id, _)| *id != touch.id);
                        self.points.push((touch.id, here));
                        // A selection handle is the shell's and is above the
                        // page, so a finger on one takes it before anything
                        // under it, the app's own gestures included.
                        if self.points.len() == 1 && self.press_handle(at) {
                            return;
                        }
                        // First finger down owns the gesture. A second one joins
                        // the `touches` list every handler reads, rather than
                        // starting a competing press.
                        if self.points.len() == 1 {
                            self.begin_gesture(here.0, here.1, true);
                        }
                        // Same order as the mouse: the dev overlay is above
                        // everything, so a finger on it arms a dismiss rather
                        // than reaching the app it is covering. The short-circuit
                        // is load-bearing, `press_scrollbar` and `press_text`
                        // start a drag as a side effect and must not run when the
                        // panel took the press.
                        if self.overlay_covers_physical(at)
                            || (!self.press_scrollbar(at) && !self.press_text_touch(at))
                        {
                            self.press = Some(at);
                        }
                    }
                    TouchPhase::Moved => {
                        self.pointer = at;
                        if let Some(p) = self.points.iter_mut().find(|(id, _)| *id == touch.id) {
                            p.1 = here;
                        }
                        self.track_touch(here);
                        if self.handle_drag.is_some() {
                            self.drag_handle(at);
                            return;
                        }
                        self.move_gesture(here.0, here.1);
                        // The axis claim. An element that declared `@drag` may
                        // take the finger, but only on an axis no scroller under
                        // it can travel, and the winner was decided once when
                        // this gesture passed the slop threshold. See
                        // `TouchAction` and `move_gesture`.
                        if self.gesture.as_ref().is_some_and(|g| g.dragging) {
                            return;
                        }
                        if self.bar_drag.is_some() {
                            self.drag_scrollbar(at);
                        } else if let Some(state) = self.touch_text {
                            // The finger is on text. Which of the three gestures
                            // this is depends on whether the press had time to
                            // become a long one before it moved.
                            let from = match state {
                                TouchText::Pending { at, .. } => at,
                                _ => at,
                            };
                            let moved = (at.0 - from.0).hypot(at.1 - from.1);
                            let next = touch_text_after_move(state, moved);
                            // **A drag in a field with text to scroll scrolls
                            // it.** Dragging the caret was the answer for every
                            // field, and in a textarea it left the scrollbar as
                            // the only way through the text, which no phone
                            // offers: its own fields scroll under the finger and
                            // move the caret by a handle. Reported from the
                            // phone. So the gesture is handed to the ordinary
                            // scroll path below, which finds the innermost
                            // scroller (this field) and throws it on lift.
                            if matches!(state, TouchText::Pending { .. })
                                && next == TouchText::Caret
                                && self.focused_scrolls_vertically()
                            {
                                self.touch_text = None;
                                self.touch = Some(here);
                                return;
                            }
                            self.touch_text = Some(next);
                            match next {
                                TouchText::Selecting => self.extend_from_word(at),
                                TouchText::Caret => self.drag_caret(at),
                                // Still resting inside the slop: the press has
                                // not decided yet, so nothing moves.
                                TouchText::Pending { .. } => {}
                            }
                        } else if let Some((lx, ly)) = self.touch.replace(here) {
                            self.scroll_at(at, lx - here.0, ly - here.1);
                        }
                    }
                    TouchPhase::Ended => {
                        self.pointer = at;
                        self.touch = None;
                        // Lifted *after* the dispatch below would be wrong: the
                        // finger that caused the event is part of the event, so
                        // it is removed once, at the end of this arm.
                        let lifted = touch.id;
                        if self.release_handle() {
                            self.points.retain(|(id, _)| *id != lifted);
                            return;
                        }
                        self.end_gesture(here.0, here.1);
                        // Each of the three below leaves early, and a finger
                        // that is not removed on every path out of here is one
                        // the next event still believes is down.
                        if self.bar_drag.take().is_some() {
                            self.points.retain(|(id, _)| *id != lifted);
                            return;
                        }
                        if std::mem::take(&mut self.text_drag) {
                            self.points.retain(|(id, _)| *id != lifted);
                            return;
                        }
                        // A finger lifting off text has already had its effect,
                        // whichever gesture it turned out to be, and must not
                        // also reach the app as a tap.
                        if let Some(state) = self.touch_text.take() {
                            // A plain tap on the field that has focus brings the
                            // keyboard back if Back put it away, as a native field
                            // does. Nothing else would: focus did not change, so
                            // nothing asked for it.
                            #[cfg(target_os = "android")]
                            if matches!(state, TouchText::Pending { .. })
                                && self.focused.is_some()
                                && !self.live_field().readonly
                            {
                                android_show_keyboard();
                            }
                            let _ = state;
                            self.points.retain(|(id, _)| *id != lifted);
                            return;
                        }
                        // The throw. Harmless after a tap: nothing was scrolled,
                        // so there is no target and no fling starts.
                        self.start_fling();
                        self.touch_track.clear();
                        // A finger wanders more than a mouse, but the slop that
                        // separates a tap from a drag is the same idea.
                        if let Some((sx, sy)) = self.press.take() {
                            if (at.0 - sx).hypot(at.1 - sy) <= TAP_SLOP {
                                self.dispatch_tap(at.0, at.1);
                            }
                        }
                        self.points.retain(|(id, _)| *id != lifted);
                    }
                    TouchPhase::Cancelled => {
                        self.touch = None;
                        self.press = None;
                        // A cancelled gesture is not a throw: the system took
                        // the finger, and the hand never let go of anything.
                        self.scroll_target = None;
                        self.touch_track.clear();
                        self.points.retain(|(id, _)| *id != touch.id);
                        // A cancelled touch is not a release: nothing fires, and
                        // the press is simply forgotten.
                        self.gesture = None;
                        self.gesture_deadline = None;
                        self.bar_drag = None;
                        self.text_drag = false;
                        // Dropping this also disarms a pending long press, so a
                        // cancelled touch cannot select a word after the fact.
                        self.touch_text = None;
                        self.handle_drag = None;
                    }
                }
            }
            WindowEvent::ModifiersChanged(mods) => {
                self.shift_held = mods.state().shift_key();
                self.ctrl_held = mods.state().control_key();
                self.alt_held = mods.state().alt_key();
            }
            // The side buttons on a mouse are the back and forward buttons
            // everywhere else, and a router that ignored them would be the one
            // app on the machine that does.
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: button @ (MouseButton::Back | MouseButton::Forward),
                ..
            } => {
                let moved = if button == MouseButton::Back {
                    self.document.back()
                } else {
                    self.document.forward()
                };
                if moved {
                    self.request_redraw();
                }
            }
            WindowEvent::Ime(ime) => self.on_ime(&ime),
            WindowEvent::KeyboardInput { event, .. } => {
                // While a composition is running the input method owns the
                // keyboard: the same keystrokes also arrive here, and acting on
                // them would type the letters twice, once raw and once composed.
                if event.state == ElementState::Pressed {
                    self.keyboard_modality = true;
                }
                if event.state == ElementState::Pressed && self.preedit.is_none() {
                    // **Android's Back button, and it closes the app the same
                    // way a desktop window does.** winit decodes it as
                    // `BrowserBack` and hands it over like any other key; there
                    // is no separate lifecycle event for it. Answered here
                    // rather than inside `on_key` so that a focused text field
                    // cannot eat it, and so that leaving is `event_loop.exit()`
                    // and nothing more exotic.
                    //
                    // **Exiting the loop is what closes the activity, not a
                    // `finish()` over JNI.** `android-activity` calls
                    // `ANativeActivity_finish` itself once `android_main`
                    // returns, and it is the return that matters: winit 0.30
                    // leaves `MainEvent::Destroy` as a `warn!("TODO")`, so a
                    // `finish()` asked for any other way destroys the Java
                    // activity while this loop runs on. Driven: `onDestroy`
                    // then waits ten seconds, logs "Activity destroy timeout",
                    // and the next launch reuses the wedged process and sits on
                    // the splash screen.
                    if let Key::Named(NamedKey::BrowserBack) = event.logical_key {
                        if !self.on_back() {
                            event_loop.exit();
                        }
                        return;
                    }
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
                // content under it. A press on the dev overlay is none of those,
                // it just arms the tap that dismisses it.
                let scale = self.scale();
                let here = (
                    (self.pointer.0 / scale) as f32,
                    (self.pointer.1 / scale) as f32,
                );
                self.keyboard_modality = false;
                // A mouse is one finger with id 0 for as long as its button is
                // held, so a handler written for a phone reads the same here.
                self.points.clear();
                self.points.push((0, here));
                // The pointer handlers see the press even when it also starts a
                // selection or a scrollbar drag: those are the shell's, this is
                // the app's, and an element that asked for `@press` asked for
                // every press on it.
                self.begin_gesture(here.0, here.1, false);
                self.handles = false;
                if self.overlay_covers_physical(self.pointer) {
                    self.press = Some(self.pointer);
                } else if !self.press_scrollbar(self.pointer) && !self.press_text(self.pointer) {
                    self.press = Some(self.pointer);
                    // `:active` holds from press to release.
                    self.update_pointer_state();
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
                    // Release ends `:active`, before the tap runs, so a handler
                    // that restructures the tree doesn't leave a pressed node behind.
                    self.update_pointer_state();
                    let (px, py) = self.pointer;
                    if (px - sx).hypot(py - sy) <= TAP_SLOP {
                        self.dispatch_tap(px, py);
                    }
                }
                let scale = self.scale();
                let here = (
                    (self.pointer.0 / scale) as f32,
                    (self.pointer.1 / scale) as f32,
                );
                self.end_gesture(here.0, here.1);
                // After the tap and the gestures, because the button that caused
                // them is part of them.
                self.points.clear();
            }
            // Event-driven: we only paint in response to a redraw request, which
            // is issued on resume, resize, reload, and tap, not every frame.
            WindowEvent::RedrawRequested => {
                self.render();
                #[cfg(target_os = "android")]
                self.sync_text_menu();
                #[cfg(any(target_os = "android", target_arch = "wasm32"))]
                self.publish_saved_state();
                // A `tap()` or `focus()` asked for by something that is not an
                // input event has nowhere else to be picked up: the only other
                // drain runs after a handler the shell itself dispatched. A
                // `mounted` hook that focuses a field at startup queued its
                // request before the window had ever seen an event, and it sat
                // in the queue forever. Here rather than at load, because both
                // requests are answered against a laid-out frame: before the
                // first layout there are no focusables to focus and no hit
                // regions to tap.
                // Before anything else can take focus: the app is being put
                // back as it was, and that includes which field had it.
                #[cfg(any(target_os = "android", target_arch = "wasm32"))]
                self.adopt_restored_focus();
                self.adopt_element_requests();
                // After any `focus()`, which is the more specific request: an
                // `autofocus` only takes a field nobody else has put focus in.
                self.adopt_autofocus();
            }
            _ => {}
        }
    }

    /// The only clock in an otherwise event-driven loop: while an input is
    /// focused, wake every `BLINK` to toggle the caret. With no focus the
    /// deadline is `None`, so we wait indefinitely for the next real event.
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // Whatever `@input`, `@change`, `@focus` and `@blur` the events since
        // the last wake-up queued, now that the shell is done with them.
        self.flush_field_events();

        self.sync_focus_state();

        // A finger lifting does not always draw a frame, and the text menu
        // waits on the lift.
        #[cfg(target_os = "android")]
        self.sync_text_menu();

        // A resting finger is the second clock, and the reason this is not just
        // the blink any more: nothing arrives to say a press has gone on long
        // enough, so the deadline has to be waited on and checked here.
        if let Some(TouchText::Pending { at, deadline }) = self.touch_text {
            if Instant::now() >= deadline {
                // Whether or not a word was there to take, the press has
                // resolved: it must not stay pending and fire again later.
                self.touch_text = Some(TouchText::Selecting);
                if self.press_on_word(at) && self.select_word_at(at) {
                    self.pressed_word = Some(self.selection());
                    self.request_redraw();
                } else if self.focused.is_some() {
                    // Empty space: the caret goes where the finger is, with
                    // its handle and the menu of what a caret can do.
                    self.drag_caret(at);
                    self.touch_text = Some(TouchText::Caret);
                    self.caret_menu = true;
                    self.handles = true;
                    self.caret_handle_until = Some(Instant::now() + CARET_HANDLE_FADE);
                    self.request_redraw();
                }
            }
        }

        if let Some(until) = self.caret_handle_until {
            if Instant::now() >= until {
                self.caret_handle_until = None;
                // The Paste menu goes with its handle. A selection's menu has
                // no clock, so only a caret's is closed.
                if self.caret == self.anchor {
                    self.caret_menu = false;
                }
                self.request_redraw();
            }
        }

        if let Some(deadline) = self.blink_deadline {
            if Instant::now() >= deadline {
                self.caret_visible = !self.caret_visible;
                self.blink_deadline = Some(Instant::now() + BLINK);
                self.request_redraw();
            }
        }

        // A press that has rested long enough, on an element that asked. The
        // deadline is cleared either way, so it fires once and a finger that
        // stays down does not fire it again on every wake-up.
        if let Some(deadline) = self.gesture_deadline {
            if Instant::now() >= deadline {
                self.gesture_deadline = None;
                let resting = self.gesture.as_ref().map(|g| (g.start, g.long_fired));
                if let Some((start, false)) = resting {
                    if let Some(g) = self.gesture.as_mut() {
                        g.long_fired = true;
                    }
                    self.fire_gesture(rux_layout::Gesture::LongPress, start.0, start.1, Vec::new());
                }
            }
        }

        // The third clock, and the only one that is a frame rather than a state
        // change: something is mid-transition and the next frame is due. The
        // deadline is cleared here because `render` sets the following one, and
        // sets none once the animation has landed.
        if let Some(deadline) = self.anim_deadline {
            if Instant::now() >= deadline {
                self.anim_deadline = None;
                self.request_redraw();
            }
        }

        // The fifth clock: a scroller still coasting after the finger left. It
        // is the shell's own animation rather than the document's, because what
        // moves is a scroll offset and not a style, and the animator works on
        // the tree.
        if self.fling.is_some() && self.step_fling() {
            self.request_redraw();
        }

        // An entering element has now been painted wearing `:enter-from`, so it
        // can let go of it and the animator has somewhere to walk from. This
        // happens *here*, after the frame, rather than in `render` beside the
        // commit: doing it there replaces the `:enter-from` build before it is
        // ever drawn, and the element simply appears at its destination.
        if self.document.settle_swaps() {
            self.request_redraw();
        }

        // The fourth clock: an interval a script started. Time is passed in as
        // milliseconds rather than read here, the same contract the animator
        // has, so the runtime stays testable without a window and correct on the
        // web where `Instant` is not what the event loop runs on.
        let now = self.epoch.elapsed().as_secs_f64() * 1000.0;
        if self.document.fire_timers(now) {
            self.request_redraw();
        }
        let timer = self.document.timer_deadline(now).map(|due| {
            // Back into the event loop's own clock. The floor keeps a period
            // that has already slipped past from asking to wait a negative time.
            Instant::now() + Duration::from_secs_f64(((due - now) / 1000.0).max(0.0))
        });

        // Wake for whichever clock is due first. With none running, wait
        // indefinitely for a real event, as before.
        let long_press = match self.touch_text {
            Some(TouchText::Pending { deadline, .. }) => Some(deadline),
            _ => None,
        };
        // A running fling wants the next frame, and nothing else will ask for
        // it: the finger has gone, so no event is coming to wake the loop.
        let fling = self
            .fling
            .is_some()
            .then(|| Instant::now() + Duration::from_secs_f64(rux_runtime::FRAME_MS / 1000.0));
        match [
            self.blink_deadline,
            self.caret_handle_until,
            long_press,
            self.anim_deadline,
            timer,
            self.gesture_deadline,
            fling,
        ]
        .into_iter()
        .flatten()
        .min()
        {
            Some(next) => event_loop.set_control_flow(ControlFlow::WaitUntil(next)),
            None => event_loop.set_control_flow(ControlFlow::Wait),
        }
    }
}

// ── The URL bar as the router's address bar ──────────────────────────────────
//
// Two functions, and they are deliberately not gated to wasm: the arithmetic
// between a served base path and a Rux route is where this goes wrong, and it
// is worth being able to test it without a browser.
//
// A Rux app served at the root of a domain has base `/`, and its routes are the
// URL's path. One served from a subdirectory (which is what `rux build` output
// dropped into an existing site looks like, and what the docs site does) has
// base `/app/`, and the same route `/settings` is the URL `/app/settings`. The
// app is written the same way either way, which is the point: a route is the
// app's own address, not its address on somebody's server.

/// The route named by a browser path, with the app's base subtracted.
///
/// Anything that is not under the base is treated as the root rather than
/// passed through: it means the page is served from somewhere the base does not
/// describe, and a route the app cannot match would land on its fallback page
/// with no way to tell why.
///
/// Only the wasm build calls it. It is compiled everywhere anyway so that its
/// tests run in the ordinary `cargo test`, which is the whole reason it is a
/// separate function.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
fn route_from_path(base: &str, pathname: &str) -> String {
    let base = base.trim_end_matches('/');
    let rest = match pathname.strip_prefix(base) {
        Some(rest) => rest,
        // Serving at `/app/` and asked about `/app` exactly: the base itself,
        // which is the app's root.
        None if base.trim_start_matches('/') == pathname.trim_start_matches('/') => "",
        None => "",
    };
    if rest.is_empty() || !rest.starts_with('/') {
        return rux_runtime::ROOT_PATH.to_string();
    }
    rest.to_string()
}

/// The browser path a route lives at, the inverse of [`route_from_path`].
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
fn path_for_route(base: &str, route: &str) -> String {
    let base = base.trim_end_matches('/');
    if route == rux_runtime::ROOT_PATH {
        // A bare base with no trailing slash is a valid URL and the one a user
        // would type, but `/` is what the root of a site is spelled.
        return if base.is_empty() { rux_runtime::ROOT_PATH.to_string() } else { base.to_string() };
    }
    format!("{base}{route}")
}

#[cfg(test)]
mod overlay_attribution {
    use super::{overlay_paints, Paint};
    use std::path::{Path, PathBuf};

    /// Every string the overlay would paint.
    fn painted(diag: &rux_runtime::Diagnostics, running: &str) -> String {
        let panel = overlay_paints(diag, Path::new(running), 900.0).expect("a panel");
        panel
            .paints
            .iter()
            .filter_map(|p| match p {
                Paint::Text(t) => Some(t.content.text.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn warning(message: &str, line: Option<usize>, file: Option<&str>) -> rux_reactive::Warning {
        rux_reactive::Warning {
            message: message.to_string(),
            line,
            file: file.map(PathBuf::from),
            level: rux_reactive::Level::Warning,
        }
    }

    /// A warning from an imported component names that component.
    ///
    /// The panel is headed with the document being *run*, so `line 9` under a
    /// title reading `app.rux` pointed confidently at a line of the wrong file.
    /// Unplaced is vague; placed and wrong is a trap, which is the same lesson
    /// the line numbers themselves taught.
    #[test]
    fn a_warning_from_another_file_says_which() {
        let diag = rux_runtime::Diagnostics {
            error: None,
            stale: false,
            warnings: vec![warning("`draft` is not defined", Some(9), Some("pages/home.rux"))],
            prints: vec![],
        };
        let text = painted(&diag, "app.rux");
        assert!(text.contains("home.rux"), "names the file it is really in: {text}");
        assert!(text.contains("line 9"), "and keeps the line: {text}");
    }

    /// A warning from the document being run is not labelled, because repeating
    /// the title on every line is noise.
    #[test]
    fn a_warning_from_the_running_document_is_not_labelled() {
        let diag = rux_runtime::Diagnostics {
            error: None,
            stale: false,
            warnings: vec![warning("`draft` is not defined", Some(9), Some("app.rux"))],
            prints: vec![],
        };
        let text = painted(&diag, "app.rux");
        let body: Vec<&str> = text.lines().filter(|l| l.starts_with('•')).collect();
        assert!(!body.is_empty(), "a warning line was painted: {text}");
        assert!(
            body.iter().all(|l| !l.contains("app.rux")),
            "its own file is not repeated on the line: {body:?}"
        );
    }

    /// A warning that carries no file at all is still shown, unlabelled.
    #[test]
    fn a_warning_with_no_file_is_still_shown() {
        let diag = rux_runtime::Diagnostics {
            error: None,
            stale: false,
            warnings: vec![warning("a css property is not honored", None, None)],
            prints: vec![],
        };
        let text = painted(&diag, "app.rux");
        assert!(text.contains("not honored"), "shown: {text}");
    }
}

#[cfg(test)]
mod url_routes {
    use super::{path_for_route, route_from_path};

    /// Served at the root of a domain: the URL path is the route, unchanged.
    #[test]
    fn at_the_root_a_path_is_a_route() {
        assert_eq!(route_from_path("/", "/"), "/");
        assert_eq!(route_from_path("/", "/settings"), "/settings");
        assert_eq!(route_from_path("/", "/user/7"), "/user/7");
    }

    /// Served from a subdirectory: the base comes off, and the app never sees
    /// where it was deployed.
    #[test]
    fn a_base_is_subtracted() {
        assert_eq!(route_from_path("/app/", "/app/settings"), "/settings");
        assert_eq!(route_from_path("/app", "/app/user/7"), "/user/7");
        assert_eq!(route_from_path("/app/", "/app/"), "/");
        assert_eq!(route_from_path("/app/", "/app"), "/");
    }

    /// A path outside the base means the page is not where the base says. The
    /// app's root beats a route it could only answer with its fallback.
    #[test]
    fn a_path_outside_the_base_is_the_root() {
        assert_eq!(route_from_path("/app/", "/other/page"), "/");
        // `/application` starts with `/app` as *text* and is a different place.
        assert_eq!(route_from_path("/app", "/application"), "/");
    }

    /// Round trip: every route the app can be on maps to a URL that maps back.
    #[test]
    fn a_route_survives_the_round_trip() {
        for base in ["/", "/app", "/app/"] {
            for route in ["/", "/settings", "/user/7"] {
                let path = path_for_route(base, route);
                assert_eq!(
                    route_from_path(base, &path),
                    route,
                    "base {base}, route {route}, path {path}"
                );
            }
        }
    }
}

// ── Web entry point ──────────────────────────────────────────────────────────
//
// The browser drives the same `App` as the desktop: same input handling, same
// focus and caret logic, same painter. Only the three things a browser does not
// have are different, no file watcher (the host page pushes source instead), no
// blocking on the main thread (surface setup is a task), and no OS clipboard.
//
// Two values have to outlive the call that creates them and be reachable from
// inside `resumed` and from later JS calls, so they live in thread-locals. That
// is sound here in a way it would not be natively: wasm is single-threaded, and
// `spawn_app` hands the loop to the browser rather than returning.

#[cfg(target_arch = "wasm32")]
thread_local! {
    /// The canvas the host page gave us, taken by `resumed`.
    static WEB_CANVAS: RefCell<Option<web_sys::HtmlCanvasElement>> = const { RefCell::new(None) };
    /// Kept so the surface task, and `set_source`, can wake the event loop.
    static WEB_PROXY: RefCell<Option<winit::event_loop::EventLoopProxy<RuxEvent>>> =
        const { RefCell::new(None) };
    /// The canvas's CSS size at boot, in logical pixels.
    ///
    /// Not a convenience, it is load-bearing. winit's web backend leaves a
    /// window's `current_size` at **zero** until a `ResizeObserver` fires, and it
    /// only styles the canvas at all when `inner_size` was requested. Ask a
    /// freshly created window for its size and you get 0×0, configure a surface
    /// at that, and wgpu sets the canvas backing store to 1×1, which collapses
    /// the element to a one-pixel strip that then never resizes, because there is
    /// no longer any size change to observe. So the size is captured from the DOM
    /// up front and used for both the window attributes and the first surface.
    static WEB_SIZE: RefCell<(f64, f64)> = const { RefCell::new((420.0, 640.0)) };
    /// The hidden `<input>` that exists purely to be focusable.
    ///
    /// A browser raises a phone's on-screen keyboard for a focused editable DOM
    /// element and for nothing else. Rux's fields are painted inside a
    /// `<canvas>`, which the browser knows nothing about, so before this there
    /// was no way to type into one on a phone at all: tapping a field focused it
    /// inside the runtime and the keyboard never came up.
    ///
    /// It is a real input holding the real text rather than a bare event sink,
    /// because that hands composition, autocorrect, dictation and the keyboard's
    /// own backspace to the browser, which already does all of it properly. The
    /// shell reads the value back out and copies it into the bound signal.
    static WEB_IME: RefCell<Option<WebIme>> = const { RefCell::new(None) };
    /// Its multi-line twin, a hidden `<textarea>`. See [`WebIme`].
    static WEB_IME_AREA: RefCell<Option<WebIme>> = const { RefCell::new(None) };
    /// Which of the two the focused field is using.
    static WEB_IME_MULTILINE: RefCell<bool> = const { RefCell::new(false) };
    /// Byte length of the composition in flight in that input, `0` when none.
    static WEB_COMPOSING: RefCell<usize> = const { RefCell::new(0) };
    /// The path the app is served under, and the switch that turns URL routing
    /// on at all.
    ///
    /// `None` means leave the URL bar alone, and it is the default for a
    /// reason: the playground runs *other people's documents* on a page of
    /// ruxlang.dev, and a document with a router in it must not be able to
    /// rewrite the address of the site hosting it. A page that wants its URL
    /// to be its app's address says so by passing a base to `start`.
    static WEB_BASE: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// The route the browser's URL currently names, or `None` when URL routing is
/// off.
#[cfg(target_arch = "wasm32")]
fn web_route_now() -> Option<String> {
    let base = WEB_BASE.with(|b| b.borrow().clone())?;
    let location = web_sys::window()?.location();
    let route = route_from_path(&base, &location.pathname().ok()?);
    // The query rides along, so opening `/search?q=rust` opens the search
    // showing what was searched for. `search()` already includes the `?`, and
    // is empty when there is none.
    let query = location.search().unwrap_or_default();
    Some(format!("{route}{query}"))
}

/// Write the document's position into the browser's history.
///
/// `replace` rewrites the entry the tab is on; otherwise a new one is added.
/// The index travels as the entry's state, and comes back on `popstate`, which
/// is what lets a jump of several entries be applied in one move.
#[cfg(target_arch = "wasm32")]
fn web_write_history(index: usize, route: &str, replace: bool) {
    let Some(base) = WEB_BASE.with(|b| b.borrow().clone()) else { return };
    let Some(history) = web_sys::window().and_then(|w| w.history().ok()) else { return };
    let url = path_for_route(&base, route);
    let state = wasm_bindgen::JsValue::from_f64(index as f64);
    // A number is structured-cloneable, so the state needs no object and this
    // needs no `js-sys`. The title argument is ignored by every browser.
    let wrote = if replace {
        history.replace_state_with_url(&state, "", Some(&url))
    } else {
        history.push_state_with_url(&state, "", Some(&url))
    };
    if wrote.is_err() {
        // Cross-origin, or a sandboxed frame without `allow-top-navigation`.
        // The app keeps working, the URL bar simply stops following it, so this
        // is said once rather than on every navigation.
        web_sys::console::warn_1(
            &"rux: this page may not change its URL, so the address bar will not follow the router"
                .into(),
        );
        WEB_BASE.with(|b| *b.borrow_mut() = None);
    }
}

/// Listen for the browser's Back and Forward, once.
#[cfg(target_arch = "wasm32")]
fn web_watch_history() {
    use wasm_bindgen::JsCast;
    use wasm_bindgen::prelude::Closure;

    let Some(window) = web_sys::window() else { return };
    let on_pop = Closure::<dyn FnMut(web_sys::PopStateEvent)>::new(
        move |event: web_sys::PopStateEvent| {
            let index = event.state().as_f64().map(|n| n as usize);
            WEB_PROXY.with(|p| {
                if let Some(proxy) = p.borrow().as_ref() {
                    let _ = proxy.send_event(RuxEvent::WebRoute(index));
                }
            });
        },
    );
    let _ = window
        .add_event_listener_with_callback("popstate", on_pop.as_ref().unchecked_ref());
    on_pop.forget();
}

/// Whether this is a touch-first device, where the keyboard has to be summoned.
///
/// The hidden input is deliberately *not* used on a pointer-driven browser: it
/// takes DOM focus away from the canvas, and winit's web backend listens for
/// keys on the canvas, so focusing it there would trade a working desktop
/// keyboard for one that is not needed.
#[cfg(target_arch = "wasm32")]
fn web_is_touch() -> bool {
    web_sys::window()
        .and_then(|w| w.match_media("(pointer: coarse)").ok().flatten())
        .map(|m| m.matches())
        .unwrap_or(false)
}

/// The browser's clipboard, when there is one.
///
/// Absent outside a secure context, which is also where WebGPU is absent, so in
/// practice this only fails on a page that could not have rendered anyway.
#[cfg(target_arch = "wasm32")]
fn web_clipboard() -> Option<web_sys::Clipboard> {
    Some(web_sys::window()?.navigator().clipboard())
}

/// The element standing in for the focused field: an `<input>` for a
/// one-line field, a `<textarea>` for a textarea.
///
/// **Two, because an `<input>` cannot hold a new line.** Its value is
/// sanitised: a line feed written into it is dropped, so a textarea's text
/// went into it without its line breaks, and the next letter typed reported
/// the flattened text back over the real one. Driven in Edge under touch
/// emulation: Enter in a textarea, then a letter, and `note`, new line, `x`
/// came back as `notex`. A `<textarea>` keeps them, and takes Enter as a new
/// line itself.
#[cfg(target_arch = "wasm32")]
#[derive(Clone)]
enum WebIme {
    Line(web_sys::HtmlInputElement),
    Area(web_sys::HtmlTextAreaElement),
}

#[cfg(target_arch = "wasm32")]
impl std::ops::Deref for WebIme {
    type Target = web_sys::HtmlElement;
    fn deref(&self) -> &web_sys::HtmlElement {
        match self {
            WebIme::Line(el) => el,
            WebIme::Area(el) => el,
        }
    }
}

#[cfg(target_arch = "wasm32")]
impl WebIme {
    /// The element an event came from, if it is one of the two.
    fn of(target: Option<web_sys::EventTarget>) -> Option<WebIme> {
        use wasm_bindgen::JsCast;
        match target?.dyn_into::<web_sys::HtmlInputElement>() {
            Ok(el) => Some(WebIme::Line(el)),
            Err(target) => target.dyn_into::<web_sys::HtmlTextAreaElement>().ok().map(WebIme::Area),
        }
    }
    fn value(&self) -> String {
        match self {
            WebIme::Line(el) => el.value(),
            WebIme::Area(el) => el.value(),
        }
    }
    fn set_value(&self, value: &str) {
        match self {
            WebIme::Line(el) => el.set_value(value),
            WebIme::Area(el) => el.set_value(value),
        }
    }
    fn selection_start(&self) -> Option<u32> {
        match self {
            WebIme::Line(el) => el.selection_start().ok().flatten(),
            WebIme::Area(el) => el.selection_start().ok().flatten(),
        }
    }
    fn selection_end(&self) -> Option<u32> {
        match self {
            WebIme::Line(el) => el.selection_end().ok().flatten(),
            WebIme::Area(el) => el.selection_end().ok().flatten(),
        }
    }
    fn backward(&self) -> bool {
        let direction = match self {
            WebIme::Line(el) => el.selection_direction().ok().flatten(),
            WebIme::Area(el) => el.selection_direction().ok().flatten(),
        };
        direction.as_deref() == Some("backward")
    }
    fn set_selection(&self, start: u32, end: u32, direction: &str) {
        let _ = match self {
            WebIme::Line(el) => el.set_selection_range_with_direction(start, end, direction),
            WebIme::Area(el) => el.set_selection_range_with_direction(start, end, direction),
        };
    }
}

/// The hidden element in use now, if one has been made.
#[cfg(target_arch = "wasm32")]
fn web_ime_current() -> Option<WebIme> {
    if WEB_IME_MULTILINE.with(|m| *m.borrow()) {
        WEB_IME_AREA.with(|c| c.borrow().clone())
    } else {
        WEB_IME.with(|c| c.borrow().clone())
    }
}

/// The hidden element for a one-line or a multi-line field, created and wired
/// on first use, and remembered as the one in use.
#[cfg(target_arch = "wasm32")]
fn web_ime_element(multiline: bool) -> Option<WebIme> {
    use wasm_bindgen::JsCast;
    use wasm_bindgen::prelude::Closure;

    let slot = if multiline { &WEB_IME_AREA } else { &WEB_IME };
    let other = if multiline { &WEB_IME } else { &WEB_IME_AREA };
    if WEB_IME_MULTILINE.with(|m| m.replace(multiline)) != multiline {
        // Left focused, the other one would go on taking what is typed.
        if let Some(el) = other.with(|c| c.borrow().clone()) {
            let _ = el.blur();
        }
    }
    if let Some(el) = slot.with(|c| c.borrow().clone()) {
        return Some(el);
    }
    let canvas = WEB_CANVAS.with(|c| c.borrow().clone())?;
    let document = web_sys::window()?.document()?;
    let el = if multiline {
        WebIme::Area(document.create_element("textarea").ok()?.dyn_into().ok()?)
    } else {
        let el: web_sys::HtmlInputElement = document.create_element("input").ok()?.dyn_into().ok()?;
        el.set_type("text");
        WebIme::Line(el)
    };
    // Turn off every helper that would rewrite what is typed behind our back.
    // Autocorrect on a phone is welcome inside a text field, but capitalising
    // the first letter of a password or a code is not, and Rux has no way yet
    // to say which a field is.
    let _ = el.set_attribute("autocomplete", "off");
    let _ = el.set_attribute("autocapitalize", "off");
    let _ = el.set_attribute("autocorrect", "off");
    let _ = el.set_attribute("spellcheck", "false");
    let _ = el.set_attribute("aria-hidden", "true");
    // Invisible, but genuinely present and laid out over the field it is
    // editing: `display: none` or `visibility: hidden` cannot take focus, and an
    // element parked off-screen makes the browser scroll to it when the keyboard
    // opens. `pointer-events: none` keeps taps going to the canvas, so tapping
    // to move the caret still works; focus is only ever set programmatically.
    // The 16px floor is what stops iOS Safari zooming the page in on focus.
    let _ = el.set_attribute(
        "style",
        "position: absolute; opacity: 0; pointer-events: none; z-index: 1; \
         border: 0; padding: 0; margin: 0; background: transparent; \
         color: transparent; caret-color: transparent; font-size: 16px; \
         width: 1px; height: 1px; left: 0; top: 0;",
    );

    // The canvas's parent is the positioned box the canvas itself sits in, so
    // placing the input there lets both be positioned in the same coordinates.
    let parent = canvas.parent_element()?;
    parent.append_child(&el).ok()?;

    // Every path that changes the text ends in an `input` event, including
    // composition, dictation, autocorrect and the keyboard's own backspace, so
    // one listener covers all of them and no key mapping is needed.
    let on_input = Closure::<dyn FnMut(web_sys::Event)>::new(move |event: web_sys::Event| {
        if let Some(target) = WebIme::of(event.target()) {
            web_send_text(&target);
        }
    });
    let _ = el.add_event_listener_with_callback("input", on_input.as_ref().unchecked_ref());
    on_input.forget();

    // Composition needs its own listeners only to know how much of the tail is
    // still provisional, so the runtime can underline it the way the desktop
    // does. The text itself already arrives through `input`.
    let on_comp = Closure::<dyn FnMut(web_sys::CompositionEvent)>::new(
        move |event: web_sys::CompositionEvent| {
            let composing = match event.type_().as_str() {
                "compositionend" => 0,
                _ => event.data().unwrap_or_default().len(),
            };
            WEB_COMPOSING.with(|c| *c.borrow_mut() = composing);
            if let Some(target) = WebIme::of(event.target()) {
                web_send_text(&target);
            }
        },
    );
    for name in ["compositionstart", "compositionupdate", "compositionend"] {
        let _ = el.add_event_listener_with_callback(name, on_comp.as_ref().unchecked_ref());
    }
    on_comp.forget();

    // Enter is the one key the `input` event never carries: an `<input>`
    // takes no new line and, outside a `<form>`, does nothing with it at all.
    // Default prevented so a browser that would act on it (an implicit submit
    // of some enclosing page form) does not. A textarea takes Enter as a new
    // line, which reaches `input` like any other text, so it is not listened to.
    let on_key = Closure::<dyn FnMut(web_sys::KeyboardEvent)>::new(
        move |event: web_sys::KeyboardEvent| {
            if event.key() != "Enter" {
                return;
            }
            event.prevent_default();
            WEB_PROXY.with(|p| {
                if let Some(proxy) = p.borrow().as_ref() {
                    let _ = proxy.send_event(RuxEvent::WebEnter);
                }
            });
        },
    );
    if !multiline {
        let _ = el.add_event_listener_with_callback("keydown", on_key.as_ref().unchecked_ref());
    }
    on_key.forget();

    slot.with(|c| *c.borrow_mut() = Some(el.clone()));
    Some(el)
}

#[cfg(target_arch = "wasm32")]
thread_local! {
    /// The hidden `<select>` a phone's browser shows its own picker for.
    static WEB_SELECT: RefCell<Option<web_sys::HtmlSelectElement>> = const { RefCell::new(None) };
    /// The hidden `<input type="date">`, the same for a day.
    static WEB_DATE: RefCell<Option<web_sys::HtmlInputElement>> = const { RefCell::new(None) };
}

/// A hidden control beside the canvas, laid over the field it stands in for
/// so the browser anchors its picker there, and answering through `answer`
/// when the person chooses. Rendered, which `showPicker` requires, and
/// invisible, as the hidden input is.
#[cfg(target_arch = "wasm32")]
fn web_picker_element(tag: &str, answer: fn(&web_sys::Element) -> RuxEvent) -> Option<web_sys::Element> {
    use wasm_bindgen::JsCast;
    use wasm_bindgen::prelude::Closure;

    let canvas = WEB_CANVAS.with(|c| c.borrow().clone())?;
    let document = web_sys::window()?.document()?;
    let el = document.create_element(tag).ok()?;
    let _ = el.set_attribute("aria-hidden", "true");
    let _ = el.set_attribute("tabindex", "-1");
    let _ = el.set_attribute(
        "style",
        "position: absolute; opacity: 0; pointer-events: none; z-index: 1; \
         border: 0; padding: 0; margin: 0; font-size: 16px; \
         width: 1px; height: 1px; left: 0; top: 0;",
    );
    canvas.parent_element()?.append_child(&el).ok()?;
    let on_change = Closure::<dyn FnMut(web_sys::Event)>::new(move |event: web_sys::Event| {
        let Some(target) = event.target().and_then(|t| t.dyn_into::<web_sys::Element>().ok()) else {
            return;
        };
        let event = answer(&target);
        WEB_PROXY.with(|p| {
            if let Some(proxy) = p.borrow().as_ref() {
                let _ = proxy.send_event(event);
            }
        });
    });
    let _ = el.add_event_listener_with_callback("change", on_change.as_ref().unchecked_ref());
    on_change.forget();
    Some(el)
}

/// Lay a picker over the field's box, then ask the browser to open it.
///
/// `showPicker` is looked up rather than called through a binding because it
/// is new (a select's arrived in 2024) and a browser without it still opens a
/// focused control's picker on a phone, which is what the fallback does.
#[cfg(target_arch = "wasm32")]
fn web_show_picker(el: &web_sys::HtmlElement, (x, y, width, height): (f32, f32, f32, f32)) {
    use wasm_bindgen::JsCast;

    if let Some(canvas) = WEB_CANVAS.with(|c| c.borrow().clone()) {
        let (ox, oy) = (canvas.offset_left() as f32, canvas.offset_top() as f32);
        let style = el.style();
        let _ = style.set_property("left", &format!("{}px", ox + x));
        let _ = style.set_property("top", &format!("{}px", oy + y));
        let _ = style.set_property("width", &format!("{}px", width.max(1.0)));
        let _ = style.set_property("height", &format!("{}px", height.max(1.0)));
    }
    let shown = js_sys::Reflect::get(el, &"showPicker".into())
        .ok()
        .and_then(|f| f.dyn_into::<js_sys::Function>().ok())
        .is_some_and(|f| f.call0(el).is_ok());
    if !shown {
        let _ = el.focus();
    }
}

/// The browser's own picker for a `<select>`, on a phone. Answered by
/// [`RuxEvent::PickedSelect`]; a dismissal sends nothing, which leaves the
/// field as it was.
#[cfg(target_arch = "wasm32")]
fn web_open_select(options: &[String], selected: Option<usize>, rect: (f32, f32, f32, f32)) {
    use wasm_bindgen::JsCast;

    let el = WEB_SELECT.with(|c| c.borrow().clone()).or_else(|| {
        let el: web_sys::HtmlSelectElement = web_picker_element("select", |target| {
            let index = target
                .dyn_ref::<web_sys::HtmlSelectElement>()
                .map(|s| s.selected_index())
                .filter(|i| *i >= 0)
                .map(|i| i as usize);
            RuxEvent::PickedSelect { index }
        })?
        .dyn_into()
        .ok()?;
        WEB_SELECT.with(|c| *c.borrow_mut() = Some(el.clone()));
        Some(el)
    });
    let Some(el) = el else { return };
    el.set_inner_html("");
    let Some(document) = web_sys::window().and_then(|w| w.document()) else { return };
    for option in options {
        if let Ok(o) = document.create_element("option") {
            o.set_text_content(Some(option));
            let _ = el.append_child(&o);
        }
    }
    // Nothing chosen yet shows as no option, so choosing the first one is a
    // change the browser reports.
    el.set_selected_index(selected.map_or(-1, |i| i as i32));
    web_show_picker(&el, rect);
}

/// The browser's own date picker, on a phone. Answered by
/// [`RuxEvent::PickedDate`] with the day as `YYYY-MM-DD`, which is how a
/// date input writes one; cleared in the picker reads as no answer.
#[cfg(target_arch = "wasm32")]
fn web_open_date(
    value: &str,
    min: Option<(i32, u32, u32)>,
    max: Option<(i32, u32, u32)>,
    rect: (f32, f32, f32, f32),
) {
    use wasm_bindgen::JsCast;

    let el = WEB_DATE.with(|c| c.borrow().clone()).or_else(|| {
        let el: web_sys::HtmlInputElement = web_picker_element("input", |target| {
            let value = target
                .dyn_ref::<web_sys::HtmlInputElement>()
                .map(|i| i.value())
                .filter(|v| !v.is_empty());
            RuxEvent::PickedDate { value }
        })?
        .dyn_into()
        .ok()?;
        el.set_type("date");
        WEB_DATE.with(|c| *c.borrow_mut() = Some(el.clone()));
        Some(el)
    });
    let Some(el) = el else { return };
    for (name, bound) in [("min", min), ("max", max)] {
        match bound {
            Some(day) => {
                let _ = el.set_attribute(name, &rux_layout::format_date(day));
            }
            None => {
                let _ = el.remove_attribute(name);
            }
        }
    }
    el.set_value(value);
    web_show_picker(&el, rect);
}

#[cfg(target_arch = "wasm32")]
thread_local! {
    /// What [`web_save_state`] last wrote, so an unchanged frame writes nothing.
    static WEB_SAVED: RefCell<String> = const { RefCell::new(String::new()) };
}

/// Where the app is, kept in the tab's session storage, under the path the
/// app is served from.
///
/// **A phone's browser discards a background tab as Android kills a
/// background app**, and brings it back by loading the page again. A browser
/// puts a form's fields and the page's scroll back when it does; a canvas
/// has neither for it to put back, so the app came back on its route (the URL
/// has that) with every field empty and every list at the top. This is the
/// same [`rux_runtime::SavedState`] Android keeps, and nothing new is kept:
/// never a password, and session storage is the tab's own and goes with it.
///
/// Only for a page that owns its address, as `start`'s `base` says. The
/// playground runs whatever is typed into it, and one document's fields
/// have no business turning up in the next. **That gate is also what keeps
/// the playground safe to share**: it runs other people's documents on the
/// ruxlang.dev origin, and must not reach that origin's storage. Lifting it
/// means moving the playground into a sandboxed frame first (watchlist #31).
#[cfg(target_arch = "wasm32")]
fn web_save_state(encoded: String) {
    let Some(base) = WEB_BASE.with(|b| b.borrow().clone()) else { return };
    if WEB_SAVED.with(|s| *s.borrow() == encoded) {
        return;
    }
    let Some(storage) = web_sys::window().and_then(|w| w.session_storage().ok().flatten()) else {
        return;
    };
    // Full, or refused in a private window: the app goes on, and a discarded
    // tab comes back as a fresh one, as it always did.
    if storage.set_item(&web_state_key(&base), &encoded).is_ok() {
        WEB_SAVED.with(|s| *s.borrow_mut() = encoded);
    }
}

#[cfg(target_arch = "wasm32")]
fn web_state_key(base: &str) -> String {
    format!("rux:saved:{base}")
}

/// What [`web_save_state`] kept, when this load is the tab coming back rather
/// than someone opening the page: a reload, Back or Forward onto it, or the
/// browser restoring a tab it discarded. A link followed or an address typed
/// is a new visit and starts fresh, as it would for a form.
///
/// And only when the entry on screen then is still the one the URL names, so
/// an address edited by hand is taken at its word.
#[cfg(target_arch = "wasm32")]
fn web_restored_state() -> Option<rux_runtime::SavedState> {
    use wasm_bindgen::JsCast;

    let window = web_sys::window()?;
    let base = WEB_BASE.with(|b| b.borrow().clone())?;
    let get = |target: &wasm_bindgen::JsValue, name: &str| {
        js_sys::Reflect::get(target, &name.into()).ok()
    };
    let discarded = window
        .document()
        .and_then(|d| get(&d, "wasDiscarded"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    // `performance.getEntriesByType("navigation")[0].type`, looked up rather
    // than bound: two features and a type for one string.
    let kind = get(&window, "performance")
        .and_then(|p| {
            let find = get(&p, "getEntriesByType")?.dyn_into::<js_sys::Function>().ok()?;
            find.call1(&p, &"navigation".into()).ok()
        })
        .and_then(|list| get(&list, "0"))
        .and_then(|entry| get(&entry, "type"))
        .and_then(|t| t.as_string())
        .unwrap_or_default();
    if !discarded && kind != "reload" && kind != "back_forward" {
        return None;
    }
    let storage = window.session_storage().ok().flatten()?;
    let state = rux_runtime::SavedState::decode(&storage.get_item(&web_state_key(&base)).ok()??)?;
    let here = web_route_now()?;
    (state.entries.get(state.at)?.0 == here).then_some(state)
}

/// The page's safe-area insets, in CSS pixels, which are Rux's logical ones.
///
/// **Only CSS can read them**: there is no script API for
/// `env(safe-area-inset-*)`. So an invisible probe is padded with the four and
/// its computed padding read back. Rebuilt with the environment, at startup
/// and on every resize, and a rotation is a resize.
#[cfg(target_arch = "wasm32")]
fn web_safe_area() -> Insets {
    use wasm_bindgen::JsCast;

    thread_local! {
        static PROBE: RefCell<Option<web_sys::HtmlElement>> = const { RefCell::new(None) };
    }
    let Some(window) = web_sys::window() else { return Default::default() };
    let probe = PROBE.with(|c| c.borrow().clone()).or_else(|| {
        let document = window.document()?;
        let el: web_sys::HtmlElement = document.create_element("div").ok()?.dyn_into().ok()?;
        let _ = el.set_attribute("aria-hidden", "true");
        let _ = el.set_attribute(
            "style",
            "position: fixed; visibility: hidden; pointer-events: none; left: 0; top: 0; \
             padding: env(safe-area-inset-top) env(safe-area-inset-right) \
             env(safe-area-inset-bottom) env(safe-area-inset-left);",
        );
        document.body()?.append_child(&el).ok()?;
        PROBE.with(|c| *c.borrow_mut() = Some(el.clone()));
        Some(el)
    });
    let Some(style) = probe.and_then(|el| window.get_computed_style(&el).ok().flatten()) else {
        return Default::default();
    };
    let px = |name: &str| {
        style
            .get_property_value(name)
            .ok()
            .and_then(|v| v.trim().trim_end_matches("px").parse::<f32>().ok())
            .unwrap_or(0.0)
    };
    Insets {
        top: px("padding-top"),
        right: px("padding-right"),
        bottom: px("padding-bottom"),
        left: px("padding-left"),
    }
}

/// Push the hidden input's contents at the event loop.
#[cfg(target_arch = "wasm32")]
fn web_send_text(el: &WebIme) {
    let value = el.value();
    // `selection_start` is in UTF-16 code units, which is not where Rux counts
    // from: it indexes strings by byte. Converting through the prefix keeps a
    // caret after an emoji or a CJK character in the right place instead of
    // several bytes short.
    let start16 = el.selection_start().unwrap_or(0) as usize;
    let end16 = el.selection_end().map_or(start16, |v| v as usize);
    // `selectionStart`/`End` are ordered, so on their own they cannot say which
    // end the caret is at. `selectionDirection` is what distinguishes a
    // selection dragged leftwards from the same range dragged rightwards, and
    // getting it wrong makes Shift+arrow extend from the wrong end afterwards.
    let backward = el.backward();
    let (anchor16, caret16) = rux_selection(start16, end16, backward);
    let caret = utf16_to_byte_index(&value, caret16);
    let anchor = utf16_to_byte_index(&value, anchor16);
    let composing = WEB_COMPOSING.with(|c| *c.borrow()).min(caret);
    WEB_PROXY.with(|p| {
        if let Some(proxy) = p.borrow().as_ref() {
            let _ = proxy.send_event(RuxEvent::WebText { value, caret, anchor, composing });
        }
    });
}

// The caret arithmetic between a browser and Rux, kept out of the wasm cfg so
// it can be tested on any target. A browser counts a caret in UTF-16 code units
// and Rux indexes strings by bytes, and the two only agree on pure ASCII: an
// emoji is 4 bytes and 2 code units, a CJK character 3 bytes and 1. Getting this
// wrong does not misplace the caret slightly, it panics on the first slice that
// lands inside a character, so it is worth testing directly.
//
// Compiled for the web, which is the only caller, and for tests, which are the
// reason it is not simply inside the wasm module.

/// Rux's `(anchor, caret)` as the browser's `(start, end, direction)`.
///
/// Rux stores a selection as two ends where the caret is the moving one. A DOM
/// input stores an ordered range plus a direction, so the caret's end is only
/// recoverable from `selectionDirection`. Mapping the two is pure arithmetic and
/// lives here so it can be tested without a browser.
#[cfg(any(target_arch = "wasm32", test))]
fn browser_selection(anchor: u32, caret: u32) -> (u32, u32, &'static str) {
    if anchor <= caret {
        (anchor, caret, "forward")
    } else {
        (caret, anchor, "backward")
    }
}

/// The inverse: the browser's ordered range and direction as Rux's ends.
///
/// A collapsed range is reported `"none"` rather than a direction, which lands
/// on the forward arm and gives `anchor == caret`, meaning nothing selected.
/// That is the same thing Rux means by it.
#[cfg(any(target_arch = "wasm32", test))]
fn rux_selection(start: usize, end: usize, backward: bool) -> (usize, usize) {
    if backward {
        (end, start)
    } else {
        (start, end)
    }
}

/// Byte index of the character boundary at or before `units` UTF-16 code units
/// into `s`.
///
/// "At or before" matters for the one index that has no byte equivalent: the
/// middle of a surrogate pair. Rounding down puts the caret in front of the
/// character, which is the same direction [`floor_char_boundary`] rounds, so a
/// caret can never appear to jump over an emoji depending on which conversion it
/// happened to go through.
#[cfg(any(target_arch = "wasm32", test))]
fn utf16_to_byte_index(s: &str, units: usize) -> usize {
    let mut seen = 0;
    for (byte, ch) in s.char_indices() {
        if seen >= units {
            return byte;
        }
        let next = seen + ch.len_utf16();
        if next > units {
            return byte;
        }
        seen = next;
    }
    s.len()
}

/// The inverse: how many UTF-16 code units precede byte index `byte` in `s`.
#[cfg(any(target_arch = "wasm32", target_os = "android", test))]
fn byte_to_utf16_index(s: &str, byte: usize) -> usize {
    s[..floor_char_boundary(s, byte)].chars().map(char::len_utf16).sum()
}

/// Round `index` down to a character boundary, so a caret that arrives inside a
/// character is pulled back to its start rather than left to panic a later slice.
#[cfg(any(target_arch = "wasm32", target_os = "android", test))]
fn floor_char_boundary(s: &str, mut index: usize) -> usize {
    index = index.min(s.len());
    while index > 0 && !s.is_char_boundary(index) {
        index -= 1;
    }
    index
}

/// What a number or date field's text writes to its signal, if it is one
/// yet: a number, or a date inside the field's `min` and `max`, written back
/// the way HTML writes one. `decimal` is the person's decimal separator.
fn typed_value(
    kind: InputKind,
    text: &str,
    field: &Field,
    decimal: char,
) -> Option<rux_reactive::Value> {
    match kind {
        InputKind::Number => parse_number(text, decimal).map(rux_reactive::Value::Number),
        InputKind::Date => rux_layout::parse_date(text)
            .filter(|d| field.min.is_none_or(|min| *d >= min))
            .filter(|d| field.max.is_none_or(|max| *d <= max))
            .map(|d| rux_reactive::Value::Text(rux_layout::format_date(d))),
        _ => None,
    }
}

/// What a `type="number"` field's text means as a number, if it is one yet,
/// read in a language whose decimal separator is `decimal`.
///
/// **The separator is the person's, not a guess.** The first version read a
/// comma as the point whenever there was no dot, so `24,000` typed on an
/// English phone held 24. Driven on the phone, and it caught the user out.
/// Now the language decides: the other of comma and dot is a thousands
/// separator, and is dropped. That is more forgiving than HTML or Android,
/// which both refuse `24,000` in a number field, and it keeps the rule that
/// nothing typed is thrown away. Thousands separators, and spaces, count only
/// before the decimal point, so `1.234,5` on an English phone is not a number
/// yet rather than a surprising one.
///
/// Not Rust's parse alone either, which also reads `inf`, `NaN` and
/// `infinity`: a field is typed into, and those are words.
fn parse_number(text: &str, decimal: char) -> Option<f64> {
    let text = text.trim();
    let group = if decimal == ',' { '.' } else { ',' };
    let space = |c: char| matches!(c, ' ' | '\u{a0}' | '\u{202f}');
    let numeric = |c: char| {
        matches!(c, '0'..='9' | '.' | ',' | '-' | '+' | 'e' | 'E') || space(c)
    };
    if text.is_empty() || !text.chars().all(numeric) {
        return None;
    }
    let (whole, fraction) = match text.find(decimal) {
        Some(at) => (&text[..at], Some(&text[at + decimal.len_utf8()..])),
        None => (text, None),
    };
    let whole: String = whole.chars().filter(|&c| c != group && !space(c)).collect();
    let number = match fraction {
        Some(f) if f.contains(group) || f.chars().any(space) => return None,
        Some(f) => format!("{whole}.{f}"),
        None => whole,
    };
    number.parse::<f64>().ok().filter(|n| n.is_finite())
}

/// The character the person's language writes a decimal point with: `,` or
/// `.`. Any other answer (Arabic's `٫`) is read as `.`, which is what such a
/// keyboard's number pad also offers.
///
/// Asked when a number field takes focus rather than once at startup, so a
/// language changed while the app runs is noticed at the next field.
#[cfg(windows)]
fn os_decimal_separator() -> char {
    use windows_sys::Win32::Globalization::{GetLocaleInfoEx, LOCALE_SDECIMAL};
    let mut buf = [0u16; 8];
    // A null locale name is the user's default locale.
    let len = unsafe {
        GetLocaleInfoEx(std::ptr::null(), LOCALE_SDECIMAL, buf.as_mut_ptr(), buf.len() as i32)
    };
    decimal_from(char::decode_utf16(buf[..len.max(1) as usize - 1].iter().copied()).next().and_then(Result::ok))
}

#[cfg(target_os = "android")]
fn os_decimal_separator() -> char {
    decimal_from(android_decimal_separator())
}

#[cfg(target_arch = "wasm32")]
fn os_decimal_separator() -> char {
    let language = web_sys::window().and_then(|w| w.navigator().language()).unwrap_or_default();
    let shown: String = js_sys::Number::from(1.5).to_locale_string(&language).into();
    decimal_from(shown.chars().find(|c| !c.is_ascii_digit()))
}

/// A desktop that has not been taught to ask: the dot, which is what a
/// number is written with in code and in HTML's own value.
#[cfg(not(any(windows, target_os = "android", target_arch = "wasm32")))]
fn os_decimal_separator() -> char {
    '.'
}

fn decimal_from(c: Option<char>) -> char {
    if c == Some(',') { ',' } else { '.' }
}

/// Cut an edit short so the field holds at most `max` UTF-16 code units, the
/// unit HTML's `maxlength` counts in. Returns the value and where `caret` (a
/// byte index into `new`) lands in it.
///
/// **What is cut is what was inserted**, found as whatever lies between the
/// longest common prefix and suffix of the old and new values. Cutting the end
/// of the value instead would make typing in the middle of a full field delete
/// its last character. The insertion is cut at a whole character, never half
/// way through a surrogate pair. An edit that does not lengthen the value is
/// let through untouched, so a value that was already too long can still be
/// shortened.
fn fit_length(old: &str, new: &str, caret: usize, max: usize) -> (String, usize) {
    let units = |s: &str| s.chars().map(char::len_utf16).sum::<usize>();
    let new_units = units(new);
    if new_units <= max || new_units <= units(old) {
        return (new.to_string(), caret);
    }
    let prefix = old
        .char_indices()
        .zip(new.chars())
        .take_while(|((_, a), b)| a == b)
        .last()
        .map_or(0, |((i, a), _)| i + a.len_utf8());
    let suffix = old[prefix..]
        .chars()
        .rev()
        .zip(new[prefix..].chars().rev())
        .take_while(|(a, b)| a == b)
        .map(|(a, _)| a.len_utf8())
        .sum::<usize>();
    let inserted = &new[prefix..new.len() - suffix];
    let mut room = max.saturating_sub(new_units - units(inserted));
    let kept_len = inserted
        .chars()
        .take_while(|c| {
            let fits = c.len_utf16() <= room;
            if fits {
                room -= c.len_utf16();
            }
            fits
        })
        .map(char::len_utf8)
        .sum::<usize>();
    let cut = inserted.len() - kept_len;
    let value = format!("{}{}{}", &new[..prefix], &inserted[..kept_len], &new[new.len() - suffix..]);
    let caret = if caret <= prefix {
        caret
    } else if caret >= new.len() - suffix {
        caret - cut
    } else {
        prefix + (caret - prefix).min(kept_len)
    };
    (value, caret)
}

#[cfg(test)]
mod dev_auth_tests {
    use super::{dev_handshake_app, dev_mac, dev_nonce, siphash24};
    use std::io::{BufRead, Write};

    /// The reference vector from the SipHash paper: key 00..0f, message 00..0e.
    #[test]
    fn siphash_matches_the_reference() {
        let message: Vec<u8> = (0..15).collect();
        assert_eq!(siphash24(0x0706050403020100, 0x0f0e0d0c0b0a0908, &message), 0xa129ca6149be45e5);
    }

    /// `rux run`'s side, as it is in `rux-cli`, over a real loopback socket.
    fn host(token: String) -> (u16, std::thread::JoinHandle<bool>) {
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let side = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let ours = dev_nonce();
            let mut writer = &stream;
            writeln!(writer, "rux-hello {ours}").unwrap();
            let mut reader = std::io::BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            let mut words = line.trim().strip_prefix("rux-auth ").unwrap().split(' ');
            let (answer, theirs) = (words.next().unwrap(), words.next().unwrap());
            if answer != dev_mac(&token, &format!("app {ours}")) {
                return false;
            }
            writeln!(writer, "rux-ok {}", dev_mac(&token, &format!("host {theirs}"))).unwrap();
            writeln!(writer, "put 2 app.rux").unwrap();
            true
        });
        (port, side)
    }

    #[test]
    fn both_ends_prove_the_token_and_nothing_after_is_lost() {
        let token = dev_nonce();
        let (port, side) = host(token.clone());
        let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        let mut reader = std::io::BufReader::new(stream.try_clone().unwrap());
        assert_eq!(dev_handshake_app(&mut reader, &mut stream, &token), Ok(()));
        assert!(side.join().unwrap(), "the host accepted the app");
        let mut next = String::new();
        reader.read_line(&mut next).unwrap();
        assert_eq!(next.trim(), "put 2 app.rux", "what the host sent after its answer is still there");
    }

    #[test]
    fn a_listener_without_the_token_is_refused() {
        let (port, side) = host(dev_nonce());
        let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        let mut reader = std::io::BufReader::new(stream.try_clone().unwrap());
        let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(5)));
        assert!(dev_handshake_app(&mut reader, &mut stream, &dev_nonce()).is_err());
        assert!(!side.join().unwrap(), "and it refused the app in turn");
    }
}

#[cfg(test)]
mod saved_text_tests {
    use super::{kept_across_a_kill, Field, InputKind};

    /// What an app keeps of a field while the platform holds it on disk
    /// (watchlist #30): ordinary text yes; a password, a card, a one-time
    /// code, or a field its author marked `autocomplete="off"`, no.
    #[test]
    fn secrets_are_not_kept_across_a_kill() {
        let with = |tokens: Option<&str>| Field {
            autocomplete: tokens.map(str::to_string),
            ..Default::default()
        };
        assert!(kept_across_a_kill(InputKind::Text, &with(None)));
        assert!(kept_across_a_kill(InputKind::Text, &with(Some("email"))));
        assert!(!kept_across_a_kill(InputKind::Password, &with(None)));
        assert!(!kept_across_a_kill(InputKind::Text, &with(Some("cc-number"))));
        assert!(!kept_across_a_kill(InputKind::Text, &with(Some("billing cc-csc"))));
        assert!(!kept_across_a_kill(InputKind::Text, &with(Some("one-time-code"))));
        assert!(!kept_across_a_kill(InputKind::Textarea, &with(Some("off"))));
    }
}

#[cfg(test)]
mod fit_length_tests {
    use super::fit_length;

    #[test]
    fn typing_past_the_limit_is_refused() {
        assert_eq!(fit_length("abc", "abcd", 4, 3), ("abc".to_string(), 3));
    }

    #[test]
    fn a_paste_is_cut_short_and_the_caret_follows() {
        assert_eq!(fit_length("ab", "ab12345", 7, 4), ("ab12".to_string(), 4));
    }

    /// Typing in the middle of a full field must not eat the end of it.
    #[test]
    fn an_insertion_in_the_middle_is_what_gets_cut() {
        assert_eq!(fit_length("abcd", "abXYcd", 4, 5), ("abXcd".to_string(), 3));
    }

    #[test]
    fn a_value_already_too_long_may_shrink() {
        assert_eq!(fit_length("abcdef", "abcde", 5, 3), ("abcde".to_string(), 5));
    }

    /// Counted in UTF-16 as HTML counts, and never cut through a pair.
    #[test]
    fn an_emoji_is_two_units_and_is_kept_or_dropped_whole() {
        assert_eq!(fit_length("ab", "ab😀", 6, 3), ("ab".to_string(), 2));
        assert_eq!(fit_length("ab", "ab😀", 6, 4), ("ab😀".to_string(), 6));
    }

    /// A repeated letter must not confuse where the insertion is.
    #[test]
    fn a_repeated_character_is_still_found_as_the_insertion() {
        assert_eq!(fit_length("aa", "aaaa", 4, 3), ("aaa".to_string(), 3));
    }
}

#[cfg(test)]
mod fling {
    use super::{FLING_MIN_V, FLING_TAU, fling_step};

    /// The property the decay exists for: **one long frame travels exactly as
    /// far as many short ones**, so how far a list is thrown does not depend on
    /// the refresh rate or on whether a frame was slow.
    ///
    /// This is what a per-frame multiplier gets wrong, and the failure is
    /// invisible on the machine it was tuned on.
    #[test]
    fn distance_does_not_depend_on_frame_size() {
        let v0 = 2.0_f32;

        let (one_step, _) = fling_step(v0, 100.0);

        let (mut v, mut total) = (v0, 0.0);
        for _ in 0..10 {
            let (d, next) = fling_step(v, 10.0);
            total += d;
            v = next;
        }
        assert!(
            (one_step - total).abs() < 0.001,
            "one 100ms step travelled {one_step}, ten 10ms steps travelled {total}"
        );

        // And against an uneven run, since real frames are not uniform either.
        let (mut v, mut total) = (v0, 0.0);
        for dt in [3.0, 41.0, 9.0, 17.0, 30.0] {
            let (d, next) = fling_step(v, dt);
            total += d;
            v = next;
        }
        assert!((one_step - total).abs() < 0.001, "uneven frames travelled {total}");
    }

    /// Velocity decays towards zero and never reverses, so a fling cannot crawl
    /// backwards at the end.
    #[test]
    fn velocity_decays_towards_zero_and_keeps_its_sign() {
        let mut v = 1.5_f32;
        for _ in 0..200 {
            let (_, next) = fling_step(v, 16.0);
            assert!(next.abs() <= v.abs(), "velocity grew: {v} -> {next}");
            assert!(next >= 0.0, "velocity changed sign: {next}");
            v = next;
        }
        assert!(v < FLING_MIN_V, "a fling that never stalls never stops: {v}");

        // The same, thrown the other way.
        let mut v = -1.5_f32;
        for _ in 0..200 {
            let (_, next) = fling_step(v, 16.0);
            assert!(next <= 0.0, "velocity changed sign: {next}");
            v = next;
        }
        assert!(v.abs() < FLING_MIN_V);
    }

    /// One time constant sheds `1/e` of the speed, which is what `FLING_TAU`
    /// means. A change to the constant is a change to how the scroll feels, so
    /// it should have to be made on purpose.
    #[test]
    fn tau_is_the_time_constant() {
        let (_, left) = fling_step(1.0, FLING_TAU);
        assert!((left - std::f32::consts::E.recip()).abs() < 0.0001, "{left}");
    }

    /// A step of no time moves nothing and costs no speed. The stepper guards
    /// this too, and it should not depend on that guard.
    #[test]
    fn a_zero_step_is_a_no_op() {
        let (d, left) = fling_step(3.0, 0.0);
        assert_eq!(d, 0.0);
        assert_eq!(left, 3.0);
    }
}

#[cfg(test)]
mod caret_index {
    use super::{
        Instant, TAP_SLOP, TouchText, browser_selection, byte_to_utf16_index, toolbar_layout,
        floor_char_boundary, rux_selection, touch_text_after_move, utf16_to_byte_index,
    };

    /// ASCII is the case where the two agree, and the one every other case is
    /// measured against.
    #[test]
    fn ascii_indices_are_the_same_in_both_counts() {
        let s = "hello";
        for i in 0..=s.len() {
            assert_eq!(utf16_to_byte_index(s, i), i);
            assert_eq!(byte_to_utf16_index(s, i), i);
        }
    }

    /// A caret after a CJK character: 1 code unit, 3 bytes.
    #[test]
    fn a_cjk_caret_converts_both_ways() {
        let s = "日本語";
        assert_eq!(utf16_to_byte_index(s, 0), 0);
        assert_eq!(utf16_to_byte_index(s, 1), 3);
        assert_eq!(utf16_to_byte_index(s, 3), 9);
        assert_eq!(byte_to_utf16_index(s, 3), 1);
        assert_eq!(byte_to_utf16_index(s, 9), 3);
    }

    /// An emoji is a surrogate pair: 2 code units, 4 bytes. A caret between the
    /// two halves is not a position Rux can represent, so it comes back as the
    /// start of the character rather than as an index inside it.
    #[test]
    fn a_surrogate_pair_never_yields_an_index_inside_a_character() {
        let s = "a🙂b";
        assert_eq!(utf16_to_byte_index(s, 1), 1);
        assert_eq!(utf16_to_byte_index(s, 2), 1, "mid-surrogate falls back to the start");
        assert_eq!(utf16_to_byte_index(s, 3), 5);
        assert_eq!(byte_to_utf16_index(s, 5), 3);
        for i in 0..=s.len() {
            assert!(s.is_char_boundary(utf16_to_byte_index(s, i)));
        }
    }

    /// Past the end clamps rather than panicking: a stale caret can outlive the
    /// text it pointed into, because the value is replaced wholesale.
    #[test]
    fn indices_past_the_end_clamp() {
        let s = "ab";
        assert_eq!(utf16_to_byte_index(s, 99), 2);
        assert_eq!(byte_to_utf16_index(s, 99), 2);
        assert_eq!(floor_char_boundary(s, 99), 2);
        assert_eq!(floor_char_boundary("é", 1), 0);
    }

    /// A DOM input stores an ordered range and a direction; Rux stores two ends
    /// with the caret as the moving one. A selection dragged leftwards is the
    /// same range as one dragged rightwards, so the direction is the only thing
    /// carrying which end the caret is at.
    #[test]
    fn a_selection_keeps_which_end_the_caret_is_at() {
        assert_eq!(browser_selection(2, 7), (2, 7, "forward"));
        assert_eq!(browser_selection(7, 2), (2, 7, "backward"), "dragged leftwards");
        assert_eq!(browser_selection(4, 4), (4, 4, "forward"), "collapsed");

        assert_eq!(rux_selection(2, 7, false), (2, 7));
        assert_eq!(rux_selection(2, 7, true), (7, 2), "caret at the left end");
        // A collapsed range reports "none", which is not "backward", so it takes
        // the forward arm and means nothing is selected.
        assert_eq!(rux_selection(4, 4, false), (4, 4));
    }

    #[test]
    fn the_toolbar_offers_what_the_field_can_do() {
        use super::{offered_actions, TextAction::*};
        assert_eq!(offered_actions(false, true, true), vec![Copy, Cut, Paste, SelectAll]);
        // A password never lets its text out.
        assert_eq!(offered_actions(true, true, true), vec![Paste, SelectAll]);
        // At a caret: Paste, and Select all only when there is text to select.
        assert_eq!(offered_actions(false, false, true), vec![Paste, SelectAll]);
        assert_eq!(offered_actions(false, false, false), vec![Paste]);
        assert_eq!(offered_actions(true, false, false), vec![Paste]);
    }

    /// The painter draws the toolbar from this and the hit test reads it, so a
    /// button's box must be exactly where it is painted, and the strip must stay
    /// on screen for a field at either edge.
    #[test]
    fn the_toolbar_sits_where_its_buttons_are_hit() {
        let viewport = (400.0, 800.0);
        let ((x, y, w, h), buttons) = toolbar_layout((20.0, 300.0, 200.0, 40.0), viewport, 0.0, &super::TextAction::ALL);

        // Buttons tile the strip exactly: no gap to fall through, no overlap.
        assert_eq!(buttons.len(), 4);
        assert!((buttons[0].1 - x).abs() < f32::EPSILON, "first starts at the panel");
        let mut edge = x;
        for (_, bx, by, bw, bh) in &buttons {
            assert!((bx - edge).abs() < 0.001, "buttons are contiguous");
            assert_eq!((*by, *bh), (y, h), "all share the strip's line");
            edge += bw;
        }
        assert!((edge - (x + w)).abs() < 0.001, "and fill it exactly");

        // Above the field, since there is room above it.
        assert!(y + h < 300.0, "sits above the field: {y}");

        // A field at the top has no room above, so the strip goes below it.
        let ((_, below_y, _, _), _) = toolbar_layout((20.0, 0.0, 200.0, 40.0), viewport, 0.0, &super::TextAction::ALL);
        assert!(below_y >= 40.0, "drops below the field instead: {below_y}");

        // Room under a status bar is not room: a field 60px down with a 40px
        // inset has 20px above it that nobody can use.
        let ((_, under_bar_y, _, _), _) =
            toolbar_layout((20.0, 60.0, 200.0, 40.0), viewport, 40.0, &super::TextAction::ALL);
        assert!(under_bar_y >= 100.0, "goes below rather than under the bar: {under_bar_y}");

        // A field against the right edge must not push the strip off screen.
        let ((right_x, _, right_w, _), _) = toolbar_layout((380.0, 300.0, 200.0, 40.0), viewport, 0.0, &super::TextAction::ALL);
        assert!(right_x >= 0.0, "never off the left edge");
        assert!(right_x + right_w <= viewport.0 + 0.001, "nor off the right: {right_x}");
    }

    /// A finger drag on text moved the caret on a phone only after v0.5.1;
    /// before that it selected, because touch was routed down the mouse's path.
    /// These are the transitions that separate the two.
    #[test]
    fn a_finger_that_moves_before_the_long_press_drags_the_caret() {
        let pending = TouchText::Pending { at: (0.0, 0.0), deadline: Instant::now() };

        // Inside the slop the press has not decided: it can still become a
        // selection if the finger stays put.
        assert_eq!(touch_text_after_move(pending, 0.0), pending);
        assert_eq!(touch_text_after_move(pending, TAP_SLOP), pending);

        // Past it, the gesture is a caret drag, and cannot become a selection
        // later however long the finger then rests.
        assert_eq!(touch_text_after_move(pending, TAP_SLOP + 0.1), TouchText::Caret);
        assert_eq!(touch_text_after_move(TouchText::Caret, 0.0), TouchText::Caret);
        assert_eq!(touch_text_after_move(TouchText::Caret, 500.0), TouchText::Caret);

        // Once a word has been taken, every further movement extends it. This
        // is the only path that selects.
        assert_eq!(touch_text_after_move(TouchText::Selecting, 0.0), TouchText::Selecting);
        assert_eq!(touch_text_after_move(TouchText::Selecting, 500.0), TouchText::Selecting);
    }

    /// The two directions are inverses. Round-tripping is what catches a
    /// direction bug: pushing a backward selection to the browser and reading it
    /// straight back must not silently flip the caret to the other end, which is
    /// what makes a later Shift+arrow extend the wrong way.
    #[test]
    fn pushing_a_selection_and_reading_it_back_is_lossless() {
        for (anchor, caret) in [(0u32, 0u32), (0, 5), (5, 0), (3, 9), (9, 3), (4, 4)] {
            let (start, end, direction) = browser_selection(anchor, caret);
            let backward = direction == "backward";
            let (back_anchor, back_caret) = rux_selection(start as usize, end as usize, backward);
            assert_eq!(
                (back_anchor as u32, back_caret as u32),
                (anchor, caret),
                "round trip changed ({anchor}, {caret})"
            );
        }
    }
}

/// Boot Rux onto an existing `<canvas>`, rendering `source`.
///
/// `font` is a font file's bytes, and is not optional in practice: a browser
/// exposes no system font source, so without it every family query misses and
/// the app renders as silent blank boxes. See `TextEngine::register_font`.
///
/// Returns immediately: `spawn_app` gives the event loop to the browser instead
/// of blocking, so the caller keeps running. Errors in `source` are reported and
/// replaced with an empty document, matching what the native loader does with an
/// unreadable file.
///
/// `base` is the path the app is served under, and giving one is what makes the
/// URL bar the app's address bar: the document opens on the route the URL
/// names, navigating pushes a history entry, and the browser's Back and Forward
/// walk the app. Without it the URL is left alone entirely and the document
/// opens at `/`, which is what the playground needs: it runs documents written
/// by other people on a page of somebody else's site.
#[cfg(target_arch = "wasm32")]
pub fn start_web(
    canvas: web_sys::HtmlCanvasElement,
    source: String,
    font: Vec<u8>,
    base: Option<String>,
) {
    use winit::platform::web::EventLoopExtWebSys;

    let mut document = match Document::from_source(&source) {
        Ok(doc) => doc,
        Err(err) => {
            web_sys::console::error_1(&format!("rux: {err}").into());
            Document::from_source("<template><screen></screen></template>").expect("empty document")
        }
    };

    // Before the first frame: `start_at` replaces the history rather than
    // adding to it, so it has to happen while there is nothing to replace.
    if let Some(base) = base {
        WEB_BASE.with(|b| *b.borrow_mut() = Some(base));
        if let Some(route) = web_route_now() {
            document.open_link(&route, false);
        }
        web_watch_history();
    }

    let event_loop = EventLoop::<RuxEvent>::with_user_event()
        .build()
        .expect("create event loop");
    event_loop.set_control_flow(ControlFlow::Wait);

    // Prefer the laid-out CSS size; fall back to the element's width/height
    // attributes, then to a phone-ish default. See WEB_SIZE for why this cannot
    // be left to winit.
    let (mut lw, mut lh) = (canvas.client_width() as f64, canvas.client_height() as f64);
    if lw <= 0.0 || lh <= 0.0 {
        lw = canvas.width() as f64;
        lh = canvas.height() as f64;
    }
    if lw > 0.0 && lh > 0.0 {
        WEB_SIZE.with(|s| *s.borrow_mut() = (lw, lh));
    }

    WEB_CANVAS.with(|c| *c.borrow_mut() = Some(canvas));
    WEB_PROXY.with(|p| *p.borrow_mut() = Some(event_loop.create_proxy()));

    let mut app = App::new(document);
    // Before the first frame, as on Android: the tab is coming back.
    if let Some(state) = web_restored_state() {
        app.document.restore_state(&state);
        app.restored_focus = Some(state.focus);
        app.restored_fields = state.fields;
    }
    if !app.text.register_font(font) {
        web_sys::console::error_1(&"rux: the supplied font had no usable faces, so text will not render".into());
    }
    event_loop.spawn_app(app);
}

/// A font to render with when the platform supplies none.
///
/// **Only on the web, and that is the whole reason it exists.** A desktop or a
/// phone has system fonts; a browser canvas has nothing, so a web build that
/// shipped no font would draw no text at all. Inter, variable, under the SIL
/// Open Font License, with the licence beside it in `assets/`.
///
/// It lives here rather than in `rux-web` because it is the *shell* that needs
/// a font when there is nothing to ask, and both the playground and an app
/// built by `rux build --target web` need the same one. Two copies of 876 KB
/// would be two copies to keep in step.
///
/// **Not gated to wasm, deliberately.** It was, and that broke the test which
/// checks these bytes actually parse, which runs on the host on purpose: a font
/// that yields no faces makes every string measure to zero and the canvas
/// render blank, and diagnosing that through a wasm bundle is miserable.
/// Leaving it ungated costs nothing where it is unused, because this is a
/// `const` and not a `static`: it is materialised at its use sites, so a
/// desktop binary that never mentions it carries none of it.
pub const DEFAULT_FONT: &[u8] = include_bytes!("../assets/Inter-Variable.ttf");

/// Run a whole project in a canvas, loading it through the installed
/// [`rux_runtime::Source`].
///
/// The difference from [`start_web`] is the difference between a playground and
/// an app. `start_web` takes one document as text and
/// `Document::from_source` **discards its imports**: no components, no
/// `<style src>`, no pages behind a router. That is right for a page where
/// somebody is typing a single document into an editor, and useless for
/// `rux build --target web`, where the whole point is a project of many files.
///
/// So this loads by path instead, and the caller installs a `MemorySource`
/// holding every file first, exactly as a desktop release build does. The
/// import graph then resolves out of memory with no filesystem anywhere, which
/// is what the source provider was built for.
#[cfg(target_arch = "wasm32")]
pub fn start_web_app(
    canvas: web_sys::HtmlCanvasElement,
    entry: String,
    font: Vec<u8>,
    base: Option<String>,
) {
    use winit::platform::web::EventLoopExtWebSys;

    let mut document = match Document::load(std::path::Path::new(&entry)) {
        Ok(doc) => doc,
        Err(err) => {
            web_sys::console::error_1(&format!("rux: {err}").into());
            Document::from_source("<template><screen></screen></template>").expect("empty document")
        }
    };

    // A built app owns its address bar, unlike the playground, which is why
    // `base` is threaded through at all: a `<router>` in an app someone
    // deployed should put its routes in the URL.
    if let Some(base) = base {
        WEB_BASE.with(|b| *b.borrow_mut() = Some(base));
        if let Some(route) = web_route_now() {
            document.open_link(&route, false);
        }
        web_watch_history();
    }

    let event_loop = EventLoop::<RuxEvent>::with_user_event()
        .build()
        .expect("create event loop");
    event_loop.set_control_flow(ControlFlow::Wait);

    let (mut lw, mut lh) = (canvas.client_width() as f64, canvas.client_height() as f64);
    if lw <= 0.0 || lh <= 0.0 {
        lw = canvas.width() as f64;
        lh = canvas.height() as f64;
    }
    if lw > 0.0 && lh > 0.0 {
        WEB_SIZE.with(|s| *s.borrow_mut() = (lw, lh));
    }

    WEB_CANVAS.with(|c| *c.borrow_mut() = Some(canvas));
    WEB_PROXY.with(|p| *p.borrow_mut() = Some(event_loop.create_proxy()));

    let mut app = App::new(document);
    // Before the first frame, as on Android: the tab is coming back.
    if let Some(state) = web_restored_state() {
        app.document.restore_state(&state);
        app.restored_focus = Some(state.focus);
        app.restored_fields = state.fields;
    }
    if !app.text.register_font(font) {
        web_sys::console::error_1(
            &"rux: the supplied font had no usable faces, so text will not render".into(),
        );
    }
    event_loop.spawn_app(app);
}

/// Resize the canvas to `w` x `h` logical pixels. No-op before `start_web`.
///
/// The host page owns the layout, so it has to push the size in. Everything
/// downstream (canvas styling, surface reconfigure, re-layout at the new
/// viewport, `@media` re-evaluation once v0.4 lands) follows from winit's
/// `Resized`.
#[cfg(target_arch = "wasm32")]
pub fn resize_web(w: f64, h: f64) {
    WEB_SIZE.with(|s| *s.borrow_mut() = (w, h));
    WEB_PROXY.with(|p| {
        if let Some(proxy) = p.borrow().as_ref() {
            let _ = proxy.send_event(RuxEvent::Resize(w, h));
        }
    });
}

/// Replace the running document's source, returning a parse error if the source
/// is not loadable. No-op before `start_web`.
///
/// The source is checked here rather than in the event handler so the caller
/// gets a *synchronous* answer it can put on screen. The running app parses it
/// again when the event arrives; parsing is cheap next to a frame, and the
/// alternative, plumbing a result back out through the event loop, would be
/// far more machinery for the same outcome.
#[cfg(target_arch = "wasm32")]
pub fn set_web_source(source: String) -> Option<String> {
    if let Err(err) = Document::from_source(&source) {
        return Some(err.to_string());
    }
    WEB_PROXY.with(|p| {
        if let Some(proxy) = p.borrow().as_ref() {
            let _ = proxy.send_event(RuxEvent::SetSource(source));
        }
    });
    None
}

/// Replace the running document and report **everything** wrong with it, as
/// JSON: `{"error": {"message", "line", "column"} | null, "warnings": [...]}`.
///
/// [`set_web_source`] returns only an error message, which is all the playground
/// could ever show: no line to jump to, and no warnings at all, while the
/// desktop window had both. This is the same call with the diagnostics the
/// runtime already computes actually handed over.
///
/// The document is built twice, once here to inspect and once on the event loop
/// to display. That is not new and not avoidable cheaply: a `Document` is not
/// `Send`, and the proxy that wakes the loop requires that it be, so the source
/// text is what travels. Both builds run the same code over the same input, so
/// the diagnostics reported are the diagnostics shown.
#[cfg(target_arch = "wasm32")]
pub fn diagnose_web_source(source: String) -> String {
    let (error, warnings) = match Document::from_source_checked(&source) {
        Err(err) => {
            let line = err.line.map(|l| l.to_string()).unwrap_or_else(|| "null".into());
            let column = err.column.map(|c| c.to_string()).unwrap_or_else(|| "null".into());
            let error = format!(
                "{{\"message\": {}, \"line\": {line}, \"column\": {column}}}",
                rux_runtime::json_string(&err.message)
            );
            (error, String::from("[]"))
        }
        Ok(doc) => {
            let warnings: Vec<String> =
                doc.diagnostics().warnings.iter().map(|w| w.to_json()).collect();
            // Only a document that builds gets displayed: a broken one leaves the
            // last good tree on screen, which is what the desktop does too.
            WEB_PROXY.with(|p| {
                if let Some(proxy) = p.borrow().as_ref() {
                    let _ = proxy.send_event(RuxEvent::SetSource(source));
                }
            });
            (String::from("null"), format!("[{}]", warnings.join(", ")))
        }
    };
    format!("{{\"error\": {error}, \"warnings\": {warnings}}}")
}

/// A device to pretend to be, so that mobile layout can be worked on with no
/// device in the room.
///
/// **The desktop window is the only honest thing here, and it is honest about
/// nothing that matters on a phone.** Every inset is zero, the density is
/// whatever the monitor says, and the window is whatever size it was dragged
/// to. So `env(safe-area-inset-bottom)` resolves to zero on every machine a Rux
/// app is written on and to 34 on the device it ships to, and the first time
/// anyone sees the difference is on the device. A profile closes that: the
/// window is sized to a device and the answers the operating system would give
/// are filled in by hand.
///
/// **These are nominal, not measurements of one handset.** The point is to be
/// *a* phone rather than *the* phone: a notch that takes a strip off the top, a
/// home indicator that takes one off the bottom, a display narrower than any
/// desktop window anyone would drag. A real device reports its own values and
/// they arrive by the same road, through [`Environment`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DeviceProfile {
    /// What `--preview` is given.
    pub name: &'static str,
    /// What it stands for, for the listing when a name is wrong.
    pub about: &'static str,
    /// Logical pixels, portrait. The window is opened at this size.
    pub width: f32,
    pub height: f32,
    /// Physical pixels per logical pixel, as the device reports it.
    pub density: f32,
    /// The edges the display will not let the app use, portrait.
    pub safe_area: Insets,
}

/// The profiles `--preview` knows.
///
/// Four, on purpose. A list long enough to need scrolling is a list nobody
/// reads, and the differences that matter to a layout are the ones between
/// these: a tall notched display, a short one with none, an Android bar at both
/// ends, and something wide enough to be a tablet.
pub const DEVICE_PROFILES: &[DeviceProfile] = &[
    DeviceProfile {
        name: "phone",
        about: "a modern full-screen phone: a notch at the top, a home indicator at the bottom",
        width: 393.0,
        height: 852.0,
        density: 3.0,
        safe_area: Insets { top: 59.0, right: 0.0, bottom: 34.0, left: 0.0 },
    },
    DeviceProfile {
        name: "phone-small",
        about: "an older, smaller phone with a physical home button and a plain status bar",
        width: 375.0,
        height: 667.0,
        density: 2.0,
        safe_area: Insets { top: 20.0, right: 0.0, bottom: 0.0, left: 0.0 },
    },
    DeviceProfile {
        name: "phone-android",
        about: "an Android phone with a status bar above and a gesture bar below",
        width: 412.0,
        height: 915.0,
        density: 2.625,
        safe_area: Insets { top: 24.0, right: 0.0, bottom: 24.0, left: 0.0 },
    },
    DeviceProfile {
        name: "tablet",
        about: "a tablet: wide enough that a phone layout stops being the right one",
        width: 820.0,
        height: 1180.0,
        density: 2.0,
        safe_area: Insets { top: 24.0, right: 0.0, bottom: 20.0, left: 0.0 },
    },
];

impl DeviceProfile {
    /// Look one up by the name written on the command line.
    pub fn by_name(name: &str) -> Option<Self> {
        DEVICE_PROFILES.iter().copied().find(|p| p.name == name)
    }

    /// What to print when the name was not one of them.
    ///
    /// The list, with what each one is for, because a bare "unknown profile" is
    /// an error that makes the reader go looking for documentation to answer a
    /// question the program could have answered.
    pub fn names_with_descriptions() -> String {
        let width = DEVICE_PROFILES.iter().map(|p| p.name.len()).max().unwrap_or(0);
        DEVICE_PROFILES
            .iter()
            .map(|p| {
                format!(
                    "  {:width$}  {} by {} at {}x, {}",
                    p.name,
                    p.width as i32,
                    p.height as i32,
                    p.density,
                    p.about,
                    width = width
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// Open the Rux window for the given `.rux` file and run the frame loop until the
/// window closes. Watches the file and repaints on change.
///
/// Native only: it takes a filesystem path and installs a file watcher, neither
/// of which a browser has. The web build drives the same `App` from source text
/// supplied by the playground editor.
#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
pub fn run(path: PathBuf) {
    run_at(path, None)
}

/// The same, opening the document on `route` instead of on `/`.
///
/// This is a deep link arriving on the desktop, and it is what makes one
/// testable at all: a page reached only by tapping through the app cannot be
/// checked on its own, and on a phone the same call is what an `myapp://` URL
/// eventually turns into.
#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
pub fn run_at(path: PathBuf, route: Option<String>) {
    run_previewing(path, route, None)
}

/// The same, with the window pretending to be a device.
///
/// See [`DeviceProfile`]. This is the whole of "develop without a phone" that
/// costs nothing: no emulator, no NDK, the desktop loop and hot reload intact,
/// and the layout mistakes that a notch and a 393-pixel-wide display cause are
/// the mistakes it catches, which is most of them.
#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
pub fn run_previewing(path: PathBuf, route: Option<String>, preview: Option<DeviceProfile>) {
    let event_loop = EventLoop::<RuxEvent>::with_user_event()
        .build()
        .expect("create event loop");
    event_loop.set_control_flow(ControlFlow::Wait);

    // Watch the file's directory *recursively* so edits to imported components
    // (which live in subdirectories) also trigger a reload. Reload on any `.rux`
    // change, `Document::load` re-reads the main file and its components.
    //
    // `.css` counts too, since `<style src="…">` means a document's styling can
    // live in a file that is not a `.rux` at all. Hot reload that covers most of
    // a document is worse than none: it teaches you to trust the window, and
    // then quietly stops telling the truth for one kind of edit.
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
        let touches_source = event
            .paths
            .iter()
            .any(|p| p.extension().is_some_and(|e| e == "rux" || e == "css"));
        if touches_source {
            let _ = proxy.send_event(RuxEvent::Reload);
        }
    })
    .expect("create watcher");
    watcher
        .watch(&watch_dir, RecursiveMode::Recursive)
        .expect("watch directory");

    let mut app = App::new(path, event_loop.create_proxy());
    // Before `resumed`, which is where the window is sized from it.
    app.preview = preview;
    // Before the first frame, and before the watcher can reload: `start_at`
    // replaces the history, so it has to be the first thing that touches it.
    // `--route` stands in for a link, so the guards are told it is one: it is
    // how an author sees what a deep link to that page would do.
    if let Some(route) = route {
        app.document.open_link(&route, false);
    }
    event_loop.run_app(&mut app).expect("run app");

    drop(watcher); // keep the watcher alive for the loop's lifetime
}

/// `android_activity`, re-exported so a generated wrapper needs one dependency.
///
/// `rux build --target android` writes a crate whose whole content is an
/// `android_main` taking an [`AndroidApp`](android_activity::AndroidApp). That
/// type has to be named in the signature, and it has to be the *same* type this
/// crate was built against, so naming the crate again in the generated manifest
/// would be one more version to keep in step for no benefit.
#[cfg(target_os = "android")]
pub use android_activity;

/// The safe-area insets a phone last reported, in physical pixels.
///
/// A static rather than an event, because on Android there is nothing to send an
/// event *to*: the insets arrive on Android's main thread from a Java callback,
/// while the shell runs its loop on another, and `RuxEvent` has no variants here
/// (see `user_event`). An atomic is the whole of the synchronisation needed for
/// four numbers that are only ever written together and read together.
///
/// Packed into one `u64` so a reader cannot catch a top from one dispatch and a
/// bottom from the next. Insets are small: 16 bits each is 65535 physical
/// pixels, and a display with a 65535-pixel status bar is not a phone.
#[cfg(target_os = "android")]
static SAFE_AREA: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Android hands the shell its safe-area insets, from `RuxActivity`.
///
/// Named for the JVM rather than for Rust: this is what
/// `dev.ruxlang.shell.RuxActivity.nativeSafeArea` resolves to, so the name is
/// load-bearing and has to match the class and method exactly. Changing the
/// package or the method name in the Java means changing it here, and the
/// failure is an `UnsatisfiedLinkError` at the first inset dispatch rather than
/// anything at build time.
///
/// Takes the two leading JNI arguments as raw pointers and ignores them, which
/// is what lets this be a plain `extern "system"` function with no `jni` types
/// in its signature: the values are primitives and nothing has to be converted.
///
/// # Safety
///
/// Called by the JVM, with the signature declared in `RuxActivity.java`.
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "system" fn Java_dev_ruxlang_shell_RuxActivity_nativeSafeArea(
    _env: *mut std::ffi::c_void,
    _class: *mut std::ffi::c_void,
    top: i32,
    right: i32,
    bottom: i32,
    left: i32,
) {
    let clamp = |v: i32| (v.clamp(0, u16::MAX as i32) as u64) & 0xffff;
    let packed = (clamp(top) << 48) | (clamp(right) << 32) | (clamp(bottom) << 16) | clamp(left);
    SAFE_AREA.store(packed, std::sync::atomic::Ordering::Relaxed);
}

/// How tall the on-screen keyboard is, in physical pixels; 0 when it is down.
///
/// **`adjustResize` does not resize the window any more.** From Android 11 the
/// keyboard is reported as an inset, which an ordinary view pads itself for,
/// and the window keeps the whole display: driven on the Spark 20, the frame
/// stayed `[0,0][720,1612]` with the keyboard up. A native surface pads for
/// nothing, so a field low on the page was drawn under the keyboard and
/// nothing moved it (watchlist #19). So the keyboard's inset is read here and
/// the page is laid out in the space above it, which is what the manifest
/// asked for.
#[cfg(target_os = "android")]
static KEYBOARD: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// The keyboard's height, with the keyboard up. See [`KEYBOARD`].
///
/// # Safety
///
/// As [`Java_dev_ruxlang_shell_RuxActivity_nativeSafeArea`].
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "system" fn Java_dev_ruxlang_shell_RuxActivity_nativeKeyboard(
    _env: *mut std::ffi::c_void,
    _class: *mut std::ffi::c_void,
    bottom: i32,
) {
    let bottom = bottom.max(0) as u32;
    if KEYBOARD.swap(bottom, std::sync::atomic::Ordering::Relaxed) != bottom {
        if let Ok(proxy) = PROXY.lock() {
            if let Some(proxy) = proxy.as_ref() {
                let _ = proxy.send_event(RuxEvent::AndroidKeyboard);
            }
        }
    }
}

/// How the JNI callbacks reach the event loop.
///
/// An input method calls in on Android's main thread, and the shell runs its
/// loop on another, so the text has to travel as an event exactly as it does in
/// a browser. A proxy is `Send`, so it can be left here for a caller that has
/// no other way to find the loop.
#[cfg(target_os = "android")]
static PROXY: std::sync::Mutex<Option<winit::event_loop::EventLoopProxy<RuxEvent>>> =
    std::sync::Mutex::new(None);

/// What the focused field holds, for an input method that is about to start.
///
/// Kept beside the loop rather than asked of it, because the question arrives
/// on the wrong thread and has to be answered synchronously: `onCreateInputConnection`
/// needs the text to seed its editable before it returns, and cannot wait for a
/// frame. The shell writes it whenever focus or content changes, so it is a
/// snapshot that is never more than one edit stale.
#[cfg(target_os = "android")]
static FOCUSED_TEXT: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());

/// Whether a text field is focused, asked by the view that receives typing.
///
/// Android treats a focused text editor as a reason to raise the keyboard, and
/// the view exists for the whole life of the app, so it cannot simply answer
/// yes: the keyboard would be up before anything was tapped. It answers this
/// instead, which is the same thing Rux means by a field being focused.
#[cfg(target_os = "android")]
static WANTS_TEXT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Android asks whether anything wants typing.
///
/// # Safety
///
/// Called by the JVM, with the signature declared in `RuxActivity.java`.
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "system" fn Java_dev_ruxlang_shell_RuxActivity_nativeWantsText(
    _env: *mut std::ffi::c_void,
    _class: *mut std::ffi::c_void,
) -> jni::sys::jboolean {
    WANTS_TEXT.load(std::sync::atomic::Ordering::Relaxed)
}

/// Android asks what the focused field currently holds.
///
/// # Safety
///
/// Called by the JVM, with the signature declared in `RuxActivity.java`.
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "system" fn Java_dev_ruxlang_shell_RuxActivity_nativeFocusedText<'frame>(
    mut env: jni::EnvUnowned<'frame>,
    _class: jni::objects::JClass<'frame>,
) -> jni::sys::jstring {
    // The pointer travels back as a `usize` and is cast at the end, because
    // `resolve` requires a type with a `Default` and a raw pointer has none.
    // Zero is null, which is what the Java side already reads as "no text", so
    // the failure path degrades to an empty field rather than a crash.
    let pointer = env
        .with_env(|env| {
            let text = FOCUSED_TEXT.lock().map(|t| t.clone()).unwrap_or_default();
            Ok::<usize, jni::errors::Error>(env.new_string(&text)?.into_raw() as usize)
        })
        .resolve::<jni::errors::LogErrorAndDefault>();
    pointer as jni::sys::jstring
}

/// An input method edited the focused field.
///
/// Offsets arrive as UTF-16 code units, which is what Java counts in, and are
/// converted here rather than on the Java side. The conversion belongs to
/// whichever side indexes the string, and that is this one.
///
/// # Safety
///
/// Called by the JVM, with the signature declared in `RuxActivity.java`.
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "system" fn Java_dev_ruxlang_shell_RuxActivity_nativeTextChanged<'frame>(
    mut env: jni::EnvUnowned<'frame>,
    _class: jni::objects::JClass<'frame>,
    token: i64,
    text: jni::objects::JString<'frame>,
    caret: i32,
    anchor: i32,
    compose_start: i32,
    compose_end: i32,
) {
    // **An edit belonging to a field that no longer has focus is dropped.** The
    // connection that sent it was built for an earlier focus and is on its way
    // out, and applying it writes the old field's text into the new one. See
    // `RuxInputConnection.token` in the Java, and note that this is only ever
    // visible for about half a second, so it is the eye that catches it.
    if token != FIELD_TOKEN.load(std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    env.with_env(|env| {
        let value: String = text.try_to_string(env)?;
        let at = |units: i32| utf16_to_byte(&value, units);
        // -1 from `getComposingSpanStart` means nothing is being composed, and
        // the two ends are either both set or both absent.
        let compose = (compose_start >= 0 && compose_end >= 0)
            .then(|| (at(compose_start.min(compose_end)), at(compose_start.max(compose_end))));
        let event = RuxEvent::AndroidText { caret: at(caret), anchor: at(anchor), compose, value };
        if let Ok(proxy) = PROXY.lock() {
            if let Some(proxy) = proxy.as_ref() {
                // A closed loop is the ordinary case while the app is going
                // away, and there is nothing to do about it.
                let _ = proxy.send_event(event);
            }
        }
        Ok::<(), jni::errors::Error>(())
    })
    .resolve::<jni::errors::LogErrorAndDefault>();
}

/// Where the app is, as `onSaveInstanceState` will store it: the last frame's
/// [`rux_runtime::SavedState`], encoded. Kept ready for the same reason as
/// [`FOCUSED_TEXT`]: Android asks on its main thread and wants the answer
/// before the call returns.
#[cfg(target_os = "android")]
static SAVED_STATE: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());

/// What `onCreate` was handed back after Android killed the app, waiting for
/// the loop to start. Set before `super.onCreate`, which is what starts it.
#[cfg(target_os = "android")]
static RESTORED_STATE: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

/// Android is about to stop the activity and asks where the app is.
///
/// # Safety
///
/// Called by the JVM, with the signature declared in `RuxActivity.java`.
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "system" fn Java_dev_ruxlang_shell_RuxActivity_nativeSaveState<'frame>(
    mut env: jni::EnvUnowned<'frame>,
    _class: jni::objects::JClass<'frame>,
) -> jni::sys::jstring {
    // As in `nativeFocusedText`: zero is null, which Java reads as "nothing
    // to keep", so a failure starts the app fresh next time.
    let pointer = env
        .with_env(|env| {
            let state = SAVED_STATE.lock().map(|s| s.clone()).unwrap_or_default();
            if state.is_empty() {
                return Ok::<usize, jni::errors::Error>(0);
            }
            Ok(env.new_string(&state)?.into_raw() as usize)
        })
        .resolve::<jni::errors::LogErrorAndDefault>();
    pointer as jni::sys::jstring
}

/// The activity was created again with what [`nativeSaveState`] kept.
///
/// [`nativeSaveState`]: Java_dev_ruxlang_shell_RuxActivity_nativeSaveState
///
/// # Safety
///
/// Called by the JVM, with the signature declared in `RuxActivity.java`.
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "system" fn Java_dev_ruxlang_shell_RuxActivity_nativeRestoreState<'frame>(
    mut env: jni::EnvUnowned<'frame>,
    _class: jni::objects::JClass<'frame>,
    state: jni::objects::JString<'frame>,
) {
    env.with_env(|env| {
        let state: String = state.try_to_string(env)?;
        if let Ok(mut slot) = RESTORED_STATE.lock() {
            *slot = Some(state);
        }
        Ok::<(), jni::errors::Error>(())
    })
    .resolve::<jni::errors::LogErrorAndDefault>();
}

/// A link that arrived before the loop was there to take it, as a route.
///
/// Set from `onCreate` on a cold start, before `super.onCreate` starts the
/// loop, and taken once before the first frame. Also set when a link arrives
/// through `onNewIntent` in the moment between Android recreating a killed
/// activity and the loop starting, which is why the loop checks it after
/// restoring rather than instead.
#[cfg(target_os = "android")]
static PENDING_LINK: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

/// A link opened the activity: `myapp://settings` or `https://host/settings`.
///
/// Java decides whether an intent is a link to act on (a VIEW with data, not a
/// relaunch from Recents carrying the old one); this only turns it into a route
/// and delivers it: to the running loop when there is one, and otherwise to
/// [`PENDING_LINK`] for the loop to open on.
///
/// # Safety
///
/// Called by the JVM, with the signature declared in `RuxActivity.java`.
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "system" fn Java_dev_ruxlang_shell_RuxActivity_nativeLink<'frame>(
    mut env: jni::EnvUnowned<'frame>,
    _class: jni::objects::JClass<'frame>,
    uri: jni::objects::JString<'frame>,
) {
    env.with_env(|env| {
        let uri: String = uri.try_to_string(env)?;
        let Some(route) = link_route(&uri) else {
            android_log(&format!("link: {uri} names no route, ignored"));
            return Ok(());
        };
        if let Ok(proxy) = PROXY.lock() {
            if let Some(proxy) = proxy.as_ref() {
                let _ = proxy.send_event(RuxEvent::AndroidLink(route));
                return Ok(());
            }
        }
        if let Ok(mut slot) = PENDING_LINK.lock() {
            *slot = Some(route);
        }
        Ok::<(), jni::errors::Error>(())
    })
    .resolve::<jni::errors::LogErrorAndDefault>();
}

/// The route a link names, query included, or `None` when it is not a URL.
///
/// **A custom scheme's host is the first segment of the route.** In
/// `myapp://settings/profile` the URL grammar makes `settings` the authority,
/// but nobody writing that link means a server called `settings`: they mean
/// `/settings/profile`, and every app platform reads it that way. `myapp:///x`
/// and `myapp:x` are accepted as `/x` too, because all three get written.
///
/// An https link's host is the site, already matched by the intent filter, so
/// only its path is the route.
///
/// The fragment is dropped, as a browser drops it before a request: it names a
/// place in a page, and the router has no such thing. Percent-escapes are kept,
/// as `location.pathname` keeps them, so a route parameter reads the same from
/// a link as from the web build.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub(crate) fn link_route(uri: &str) -> Option<String> {
    let (scheme, rest) = uri.split_once(':')?;
    if scheme.is_empty() || !scheme.chars().all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c)) {
        return None;
    }
    let rest = rest.split('#').next().unwrap_or_default();
    let web = scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https");
    let path = match rest.strip_prefix("//") {
        Some(after) => {
            let end = after.find(['/', '?']).unwrap_or(after.len());
            let (authority, tail) = after.split_at(end);
            if web || authority.is_empty() {
                tail.to_string()
            } else {
                format!("/{authority}{tail}")
            }
        }
        None if web => return None,
        None => rest.to_string(),
    };
    Some(if path.starts_with('/') { path } else { format!("/{path}") })
}

/// The most field text kept across a kill, every field's together. The
/// platform refuses a saved state over about a megabyte and takes the app
/// down with it, and a person who pasted a book into a field loses the book
/// rather than the app.
#[cfg(any(target_os = "android", target_arch = "wasm32"))]
const SAVED_TEXT_MAX: usize = 64 * 1024;

/// The fields autofill can see, as the last frame laid them out: each one's
/// id and identity, and the same list packed for Java. See [`App::publish_autofill`].
#[cfg(target_os = "android")]
static AUTOFILL: std::sync::Mutex<(Vec<(i32, FieldId)>, String)> =
    std::sync::Mutex::new((Vec::new(), String::new()));

/// A field's three-part identity: `r-model`, row, instance.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
type FieldId = (String, Option<String>, Option<String>);

/// A field's autofill id: a virtual view id Android keeps between asking what
/// the fields are and handing values back, so it has to name the *field*
/// rather than its place in a list that a rebuild can reorder. A hash of the
/// identity, positive and never 0, which Android reads as the host view.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
fn autofill_id(model: &str, row: Option<&str>, instance: Option<&str>) -> i32 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    (model, row, instance).hash(&mut hasher);
    ((hasher.finish() as u32) & 0x7fff_ffff).max(1) as i32
}

/// What autofill can fill, asked by Java when Android wants the structure.
/// One record per field, `\u{2}` between records and `\u{1}` between fields:
/// id, hints (comma separated), 1 for a password, left, top, width, height in
/// window pixels, and the value.
///
/// # Safety
///
/// As [`Java_dev_ruxlang_shell_RuxActivity_nativeFocusedText`].
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "system" fn Java_dev_ruxlang_shell_RuxActivity_nativeAutofillFields<'frame>(
    mut env: jni::EnvUnowned<'frame>,
    _class: jni::objects::JClass<'frame>,
) -> jni::sys::jstring {
    let pointer = env
        .with_env(|env| {
            let text = AUTOFILL.lock().map(|a| a.1.clone()).unwrap_or_default();
            Ok::<usize, jni::errors::Error>(env.new_string(&text)?.into_raw() as usize)
        })
        .resolve::<jni::errors::LogErrorAndDefault>();
    pointer as jni::sys::jstring
}

/// Autofill filled field `id` with `text`.
///
/// # Safety
///
/// As [`Java_dev_ruxlang_shell_RuxActivity_nativeTextChanged`].
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "system" fn Java_dev_ruxlang_shell_RuxActivity_nativeAutofill<'frame>(
    mut env: jni::EnvUnowned<'frame>,
    _class: jni::objects::JClass<'frame>,
    id: i32,
    text: jni::objects::JString<'frame>,
) {
    env.with_env(|env| {
        let value: String = text.try_to_string(env)?;
        if let Ok(proxy) = PROXY.lock() {
            if let Some(proxy) = proxy.as_ref() {
                let _ = proxy.send_event(RuxEvent::AndroidAutofill { id, value });
            }
        }
        Ok::<(), jni::errors::Error>(())
    })
    .resolve::<jni::errors::LogErrorAndDefault>();
}

/// Pack the laid-out typing fields for [`AUTOFILL`]. Only a field autofill
/// may see ([`Field::autofill`]); a field with a label has two regions, and
/// only the one with the field's own text box counts.
///
/// **Only the focused field's form.** Offered every field on the screen,
/// Google's service filled a sign-in address into a sign-up form's name and
/// email further down as well. A form is what a browser hands autofill, so a
/// form is what this hands it; a field in no form goes with the others in no
/// form.
#[cfg(target_os = "android")]
fn publish_autofill(
    layout: &rux_layout::Layout,
    document: &mut rux_runtime::Document,
    scale: f64,
    focused: Option<&str>,
    focused_row: &Option<String>,
    focused_instance: &Option<String>,
) {
    let scope = focused.and_then(|model| {
        layout
            .focuses
            .iter()
            .find(|f| {
                f.text.is_some()
                    && f.model == model
                    && f.row == *focused_row
                    && f.instance == *focused_instance
            })
            .map(|f| f.field.form.clone())
    });
    let mut ids: Vec<(i32, FieldId)> = Vec::new();
    let mut packed = String::new();
    for f in &layout.focuses {
        if f.text.is_none() || !f.field.autofill() {
            continue;
        }
        if scope.as_ref().is_some_and(|form| *form != f.field.form) {
            continue;
        }
        let id = autofill_id(&f.model, f.row.as_deref(), f.instance.as_deref());
        if ids.iter().any(|(i, _)| *i == id) {
            continue;
        }
        let px = |v: f32| (v as f64 * scale).round() as i32;
        let value = document.value_in(&f.model, f.row.as_deref(), f.instance.as_deref());
        if !packed.is_empty() {
            packed.push('\u{2}');
        }
        packed.push_str(&format!(
            "{id}\u{1}{}\u{1}{}\u{1}{}\u{1}{}\u{1}{}\u{1}{}\u{1}{value}",
            rux_layout::autofill_hints(&f.field, f.kind).join(","),
            u8::from(f.kind == InputKind::Password),
            px(f.x),
            px(f.y),
            px(f.width),
            px(f.height),
        ));
        ids.push((id, (f.model.clone(), f.row.clone(), f.instance.clone())));
    }
    if let Ok(mut slot) = AUTOFILL.lock() {
        if slot.1 != packed {
            *slot = (ids, packed);
        }
    }
}

/// Tell Android's autofill which field has focus (`id` 0 for none) and where
/// it is in the window, which is what puts a password manager's suggestions
/// under it.
#[cfg(target_os = "android")]
fn android_autofill_focus(id: i32, rect: [i32; 4]) {
    let Ok(activity) = ACTIVITY.lock() else { return };
    let Some(activity) = activity.as_ref() else { return };
    let ctx = ndk_context::android_context();
    // Safety: as in `call_set_text_input`, which records the reasoning.
    let vm = unsafe { jni::JavaVM::from_raw(ctx.vm().cast()) };
    let _ = vm.attach_current_thread(|env| {
        env.call_method(
            activity,
            jni::jni_str!("ruxAutofillFocus"),
            jni::jni_sig!("(IIIII)V"),
            &[
                jni::JValue::Int(id),
                jni::JValue::Int(rect[0]),
                jni::JValue::Int(rect[1]),
                jni::JValue::Int(rect[2]),
                jni::JValue::Int(rect[3]),
            ],
        )?;
        Ok::<(), jni::errors::Error>(())
    });
}

/// A form was submitted and passed: let autofill offer to save what was
/// typed, a new password above all.
#[cfg(target_os = "android")]
fn android_autofill_commit() {
    let Ok(activity) = ACTIVITY.lock() else { return };
    let Some(activity) = activity.as_ref() else { return };
    let ctx = ndk_context::android_context();
    // Safety: as in `call_set_text_input`, which records the reasoning.
    let vm = unsafe { jni::JavaVM::from_raw(ctx.vm().cast()) };
    let _ = vm.attach_current_thread(|env| {
        env.call_method(activity, jni::jni_str!("ruxAutofillCommit"), jni::jni_sig!("()V"), &[])?;
        Ok::<(), jni::errors::Error>(())
    });
}

/// A UTF-16 offset, as a byte offset into the same text.
///
/// Java counts a string in UTF-16 code units and Rust indexes bytes, and the
/// two agree only while the text is ASCII. They diverge at the first accented
/// letter and wildly at the first emoji, which is a surrogate pair: two code
/// units to Java, four bytes to Rust. Every offset an input method sends has to
/// come through here, or the caret lands mid-character and the next edit
/// panics on a byte that is not a boundary.
#[cfg(target_os = "android")]
fn utf16_to_byte(text: &str, units: i32) -> usize {
    if units <= 0 {
        return 0;
    }
    let wanted = units as usize;
    let mut seen = 0;
    for (offset, c) in text.char_indices() {
        if seen >= wanted {
            return offset;
        }
        seen += c.len_utf16();
    }
    text.len()
}

/// Which field Android has already been told about, if any.
///
/// A field's identity, not a yes/no: the model it binds, the row it sits in and
/// the component instance it belongs to, which is what tells two inputs apart
/// when they are the same element in different rows. Guards the call below
/// against being made again with the answer it already has, and only that. See
/// [`android_set_text_input`].
#[cfg(target_os = "android")]
static IME_FIELD: std::sync::Mutex<Option<(String, Option<String>, Option<String>)>> =
    std::sync::Mutex::new(None);

/// Which focus an input method's edits belong to.
///
/// Bumped whenever the focused field changes, read by Java when it builds an
/// input connection, and handed back with every edit that connection reports.
/// An edit carrying an old token is one from a connection that has not caught
/// up yet, and is dropped rather than applied to whatever has focus now.
#[cfg(target_os = "android")]
static FIELD_TOKEN: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(0);

/// Java asks which focus it is building a connection for.
///
/// # Safety
///
/// Called by the JVM, with the signature declared in `RuxActivity.java`.
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "system" fn Java_dev_ruxlang_shell_RuxActivity_nativeFieldToken<'frame>(
    _env: jni::EnvUnowned<'frame>,
    _class: jni::objects::JClass<'frame>,
) -> i64 {
    FIELD_TOKEN.load(std::sync::atomic::Ordering::Relaxed)
}

/// Enter from the input method. `action` is a JNI `jboolean`: 1 for the
/// action key (`performEditorAction`), 0 for a plain Enter in a one-line field.
///
/// **A key dispatched by an input method never reaches winit on this phone.**
/// `BaseInputConnection` answers the action key by dispatching an Enter from
/// no device (source 0, device -1), and it goes nowhere: driven with Gboard,
/// Java logged the Enter going out and the window never received it, while
/// `adb shell input` keys arrived. So Next did nothing, phase 3's note of
/// "should, not seen" was the accurate one, and Enter is handed over here
/// instead of as a key.
///
/// # Safety
///
/// As [`Java_dev_ruxlang_shell_RuxActivity_nativeTextAction`].
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "system" fn Java_dev_ruxlang_shell_RuxActivity_nativeEnter(
    _env: *mut std::ffi::c_void,
    _class: *mut std::ffi::c_void,
    action: u8,
) {
    if let Ok(proxy) = PROXY.lock() {
        if let Some(proxy) = proxy.as_ref() {
            let _ = proxy.send_event(RuxEvent::AndroidEnter(action != 0));
        }
    }
}

/// A one-line text field. See [`FOCUSED_KIND`].
#[cfg(target_os = "android")]
const KIND_TEXT: i32 = 0;

/// `type="textarea"`. See [`FOCUSED_KIND`].
#[cfg(target_os = "android")]
const KIND_TEXTAREA: i32 = 1;

/// `type="password"`: no autocorrect, no learning, and masked by the IME too.
/// See [`FOCUSED_KIND`].
#[cfg(target_os = "android")]
const KIND_PASSWORD: i32 = 2;

/// `type="search"`: a Search key where the action key sits. See [`FOCUSED_KIND`].
#[cfg(target_os = "android")]
const KIND_SEARCH: i32 = 3;

/// `type="number"`: digits, a sign and a decimal point. See [`FOCUSED_KIND`].
#[cfg(target_os = "android")]
const KIND_NUMBER: i32 = 4;

/// What kind of field has focus, so Android can raise the right keyboard.
///
/// **An `EditorInfo` is the only thing an app ever tells an input method about
/// what is being edited**, and Rux used to fill one in with a constant: a
/// one-line text field, with Done as its action key, for every input there is.
/// So a `type="textarea"` got a keyboard with no newline key on it, and the
/// documented behaviour of a textarea, that Enter inserts a newline, was true
/// on a desktop and false on a phone.
///
/// Driven on the phone: type `ab`, press the action key, type `c`, and the
/// field reads `abc` on one line. Nothing was inserted, because Gboard sent an
/// editor action rather than a key, which is exactly what it was asked to do.
///
/// A number rather than a bool because this is the seam every other input type
/// arrives through: one constant per [`InputKind`], matched exhaustively where
/// it is stored, so a new kind cannot reach Android without a keyboard.
///
/// Packed: the kind in the low byte, `inputmode` in the next ([`Keyboard`],
/// 0 text, 1 numeric, 2 decimal, 3 tel, 4 email, 5 url, 6 search) and
/// `enterkeyhint` in the third ([`EnterKey`], 0 unset, then enter, done, go,
/// next, previous, search, send). `RuxActivity.java` unpacks the same layout.
#[cfg(target_os = "android")]
static FOCUSED_KIND: std::sync::atomic::AtomicI32 =
    std::sync::atomic::AtomicI32::new(KIND_TEXT);

/// Java asks what kind of field it is building a connection for.
///
/// # Safety
///
/// Called by the JVM, with the signature declared in `RuxActivity.java`.
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "system" fn Java_dev_ruxlang_shell_RuxActivity_nativeFieldKind<'frame>(
    _env: jni::EnvUnowned<'frame>,
    _class: jni::objects::JClass<'frame>,
) -> i32 {
    FOCUSED_KIND.load(std::sync::atomic::Ordering::Relaxed)
}

/// The activity, handed over by Java so Rust can call back into it.
///
/// **Not `ndk_context`'s, and that distinction cost an afternoon.**
/// `ndk_context` is filled in by `android-activity`, and what it stores as the
/// "context" is the **Application** object, not the Activity. Calling an
/// activity method on it fails with `NoSuchMethodError`, which the error policy
/// logs and swallows, so the symptom is a keyboard that never opens and a log
/// with nothing in it.
///
/// So the activity arrives the only way that is unambiguous: it passes itself
/// in. A global reference, because a local one dies when `onCreate` returns.
#[cfg(target_os = "android")]
static ACTIVITY: std::sync::Mutex<Option<jni::objects::Global<jni::objects::JObject<'static>>>> =
    std::sync::Mutex::new(None);

/// Java hands over the activity, once, from `onCreate`.
///
/// **Once per process is a guarantee, not an assumption**, and it is the
/// process exit at the end of [`run_android`] that makes it one. Everything
/// this module keeps in a `static` describes one activity, so a second activity
/// built inside a surviving process would read the first one's state; winit
/// would refuse the second event loop anyway. See that function for what was
/// driven.
///
/// # Safety
///
/// Called by the JVM, with the signature declared in `RuxActivity.java`.
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "system" fn Java_dev_ruxlang_shell_RuxActivity_nativeActivityCreated<'frame>(
    mut env: jni::EnvUnowned<'frame>,
    _class: jni::objects::JClass<'frame>,
    activity: jni::objects::JObject<'frame>,
) {
    env.with_env(|env| {
        let global = env.new_global_ref(&activity)?;
        if let Ok(mut slot) = ACTIVITY.lock() {
            *slot = Some(global);
        }
        Ok::<(), jni::errors::Error>(())
    })
    .resolve::<jni::errors::LogErrorAndDefault>();
}

/// Tell Android that what it knows about the focused field is out of date.
///
/// **The one call that goes from Rust into Java, and it is not avoidable.** An
/// input method asks a view once whether it is a text editor and then caches
/// the answer for as long as that view keeps focus. Rux's view keeps focus for
/// the life of the app, so the first answer, taken before anything was tapped,
/// would be the only one: tapping a field would set the flag and no keyboard
/// would ever appear. Driven and confirmed, not assumed.
///
/// `restartInput` is what throws that cache away, and it can only be asked for
/// in Java. So this reaches the activity through `ndk_context`, which
/// `android-activity` has already filled in, and calls a method on it.
///
/// **`field` is which input is focused, and `None` is none.** It was a `bool`
/// first, and that is the whole of a defect the emulator could not show. An
/// input connection is built once per restart and holds its own copy of the
/// text; moving from one field to another asked for the keyboard twice in a
/// row, so a guard on "is it wanted" saw no change and skipped the restart.
/// Gboard then carried on editing the connection built for the *previous*
/// field. Driven on a phone: type `abc` in one input, tap the next, type one
/// character, and the second input reads `abcs` — the first field's contents,
/// including a letter already deleted from it. Backspace in that state does
/// nothing at all, because the offsets an input method is deleting at no longer
/// mean anything in the text it is deleting from: four presses, no change.
///
/// Failure is logged by the policy and otherwise ignored: the consequence is a
/// keyboard that does not open, not a broken app, and there is nothing useful
/// to do about it from here.
///
/// Returns whether it asked, which is to say whether a new connection is on
/// its way. See [`App::sync_android_ime`].
#[cfg(target_os = "android")]
fn android_set_text_input(field: Option<(String, Option<String>, Option<String>)>) -> bool {
    // **Only on a change, and this is not an optimisation.** Focus state is
    // recomputed on every edit, so without this guard the sequence is: an edit
    // arrives, the field is rewritten, focus is recomputed, the input is
    // restarted, the restart builds a fresh connection, the fresh connection
    // reports its contents as an edit, and around again. Driven: 178 reports of
    // an empty field from one tap, and a field that could never hold a
    // character because the loop overwrote it faster than typing could fill it.
    //
    // Typing does not change which field is focused, so that loop is still cut
    // here; only moving between fields gets through, which is once per tap.
    let on = field.is_some();
    {
        let Ok(mut current) = IME_FIELD.lock() else { return false };
        if *current == field {
            return false;
        }
        *current = field;
    }
    call_set_text_input(on);
    true
}

/// Rebuild the input connection for the field that already has focus, because
/// its text changed under it. See [`App::sync_android_ime`].
///
/// Not the Java call a change of field makes, which also shows the keyboard,
/// and without the guard in [`android_set_text_input`], which exists to stop
/// that call being repeated for a field that has not changed. Here the field
/// has not changed and its text has, which the guard cannot see.
/// Ask for the keyboard for the field that already has focus. See the
/// plain tap on text in the touch handler.
#[cfg(target_os = "android")]
fn android_show_keyboard() {
    let Ok(activity) = ACTIVITY.lock() else { return };
    let Some(activity) = activity.as_ref() else { return };
    let ctx = ndk_context::android_context();
    // Safety: as in `call_set_text_input`.
    let vm = unsafe { jni::JavaVM::from_raw(ctx.vm().cast()) };
    let _ = vm.attach_current_thread(|env| {
        env.call_method(activity, jni::jni_str!("ruxShowKeyboard"), jni::jni_sig!("()V"), &[])?;
        Ok::<(), jni::errors::Error>(())
    });
}

#[cfg(target_os = "android")]
fn android_restart_input() {
    // The connection only: the keyboard stays as the person left it.
    // Watchlist #20: through `ruxSetTextInput` this also raised the keyboard.
    let Ok(activity) = ACTIVITY.lock() else { return };
    let Some(activity) = activity.as_ref() else { return };
    let ctx = ndk_context::android_context();
    // Safety: as in `call_set_text_input`.
    let vm = unsafe { jni::JavaVM::from_raw(ctx.vm().cast()) };
    let _ = vm.attach_current_thread(|env| {
        env.call_method(activity, jni::jni_str!("ruxRestartInput"), jni::jni_sig!("()V"), &[])?;
        Ok::<(), jni::errors::Error>(())
    });
}

#[cfg(target_os = "android")]
fn call_set_text_input(on: bool) {
    let Ok(activity) = ACTIVITY.lock() else { return };
    let Some(activity) = activity.as_ref() else { return };
    let ctx = ndk_context::android_context();
    // Safety: `ndk_context` is filled in by `android-activity` before any Rux
    // code runs, and the VM pointer is valid for the life of the process. Only
    // the VM is taken from here; the activity comes from `ACTIVITY` above, for
    // the reason recorded on it.
    let vm = unsafe { jni::JavaVM::from_raw(ctx.vm().cast()) };
    let _ = vm.attach_current_thread(|env| {
        env.call_method(
            activity,
            jni::jni_str!("ruxSetTextInput"),
            jni::jni_sig!("(Z)V"),
            &[jni::JValue::Bool(on)],
        )?;
        Ok::<(), jni::errors::Error>(())
    });
}

/// Move the selection in the input method's copy of the focused field, in
/// UTF-16 units. See [`App::sync_android_ime`].
///
/// Carries the current token, so a sync that reaches Java after focus has moved
/// on is ignored there rather than moving a caret in the next field.
#[cfg(target_os = "android")]
fn android_sync_selection(anchor: i32, caret: i32) {
    let Ok(activity) = ACTIVITY.lock() else { return };
    let Some(activity) = activity.as_ref() else { return };
    let ctx = ndk_context::android_context();
    // Safety: as in `android_set_text_input`, which records the reasoning.
    let vm = unsafe { jni::JavaVM::from_raw(ctx.vm().cast()) };
    let token = FIELD_TOKEN.load(std::sync::atomic::Ordering::Relaxed);
    let _ = vm.attach_current_thread(|env| {
        env.call_method(
            activity,
            jni::jni_str!("ruxSyncSelection"),
            jni::jni_sig!("(JII)V"),
            &[jni::JValue::Long(token), jni::JValue::Int(anchor), jni::JValue::Int(caret)],
        )?;
        Ok::<(), jni::errors::Error>(())
    });
}

/// The focused field's selection in UTF-16 units, anchor in the high 32 bits
/// and caret in the low, published beside [`FOCUSED_TEXT`] so a new input
/// connection starts with the caret where Rux has it.
///
/// **It started at the end of the text, always.** Harmless while a connection
/// was only ever built by a tap that also put the caret at the end; wrong the
/// moment one is rebuilt because Cut or Paste changed the text in the middle.
#[cfg(target_os = "android")]
static FOCUSED_SELECTION: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(0);

/// Java asks where the selection is, for a connection it is building.
///
/// # Safety
///
/// Called by the JVM, with the signature declared in `RuxActivity.java`.
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "system" fn Java_dev_ruxlang_shell_RuxActivity_nativeFocusedSelection<'frame>(
    _env: jni::EnvUnowned<'frame>,
    _class: jni::objects::JClass<'frame>,
) -> i64 {
    FOCUSED_SELECTION.load(std::sync::atomic::Ordering::Relaxed)
}

/// Tell the activity that a frame has reached the surface, once.
///
/// **The splash screen is waiting on this.** A `NativeActivity` reports a first
/// frame to the platform as soon as its surface exists, which is about a second
/// before the shell has rendered anything, so the activity holds the splash
/// until this says the picture is real. See `keepSplashUntilDrawn` in the Java.
///
/// Called from the render path on every frame and guarded by an atomic rather
/// than by the caller, so that the "once" cannot be lost if the draw path grows
/// a second exit. The cost after the first frame is one relaxed load.
///
/// Failure is logged by the policy and otherwise ignored, as with
/// [`android_set_text_input`]: the consequence is a splash that stays up until
/// the activity's own deadline takes it away, not a broken app.
#[cfg(target_os = "android")]
fn android_first_frame() {
    if FIRST_FRAME.swap(true, std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    let Ok(activity) = ACTIVITY.lock() else { return };
    let Some(activity) = activity.as_ref() else { return };
    let ctx = ndk_context::android_context();
    // Safety: as in `android_set_text_input`, which records the reasoning.
    let vm = unsafe { jni::JavaVM::from_raw(ctx.vm().cast()) };
    let _ = vm.attach_current_thread(|env| {
        env.call_method(activity, jni::jni_str!("ruxFirstFrame"), jni::jni_sig!("()V"), &[])?;
        Ok::<(), jni::errors::Error>(())
    });
}

/// Whether [`android_first_frame`] has already reported.
#[cfg(target_os = "android")]
static FIRST_FRAME: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Ask the platform to pick one of `options`, with `selected` already chosen.
///
/// **The dialog is the platform's, and so is the thread it runs on.** This hops
/// to Android's main thread inside the Java, and the answer comes back through
/// [`Java_dev_ruxlang_shell_RuxActivity_nativeSelectChosen`] rather than from
/// this call, which returns as soon as the picker has been asked for.
///
/// Failure is logged by the policy and otherwise ignored, as with the other
/// calls into the activity: the consequence is a select that does not open, not
/// a broken app.
#[cfg(target_os = "android")]
fn android_open_select(options: &[String], selected: Option<usize>) {
    let Ok(activity) = ACTIVITY.lock() else { return };
    let Some(activity) = activity.as_ref() else { return };
    let ctx = ndk_context::android_context();
    // Safety: as in `android_set_text_input`, which records the reasoning.
    let vm = unsafe { jni::JavaVM::from_raw(ctx.vm().cast()) };
    let _ = vm.attach_current_thread(|env| {
        // A Java `String[]`, built one element at a time because there is no
        // bulk path for object arrays.
        let class = env.find_class(jni::jni_str!("java/lang/String"))?;
        let empty = env.new_string("")?;
        let array = env.new_object_array(options.len() as i32, &class, &empty)?;
        for (i, option) in options.iter().enumerate() {
            let value = env.new_string(option.as_str())?;
            array.set_element(env, i, &value)?;
        }
        env.call_method(
            activity,
            jni::jni_str!("ruxOpenSelect"),
            jni::jni_sig!("([Ljava/lang/String;I)V"),
            &[
                jni::JValue::Object(&array),
                // -1 rather than an Option, because this crosses into Java and
                // a primitive is the honest shape there.
                jni::JValue::Int(selected.map(|i| i as i32).unwrap_or(-1)),
            ],
        )?;
        Ok::<(), jni::errors::Error>(())
    });
}

/// Ask the platform for a day, starting on `value` (`YYYY-MM-DD`, or empty
/// for today) and limited to `min` and `max`. Answered through
/// [`Java_dev_ruxlang_shell_RuxActivity_nativeDateChosen`], like the select.
#[cfg(target_os = "android")]
fn android_open_date(value: &str, min: Option<(i32, u32, u32)>, max: Option<(i32, u32, u32)>) {
    let Ok(activity) = ACTIVITY.lock() else { return };
    let Some(activity) = activity.as_ref() else { return };
    let ctx = ndk_context::android_context();
    // Safety: as in `android_set_text_input`, which records the reasoning.
    let vm = unsafe { jni::JavaVM::from_raw(ctx.vm().cast()) };
    let limit = |d: Option<(i32, u32, u32)>| d.map(rux_layout::format_date).unwrap_or_default();
    let _ = vm.attach_current_thread(|env| {
        let value = env.new_string(value)?;
        let min = env.new_string(limit(min))?;
        let max = env.new_string(limit(max))?;
        env.call_method(
            activity,
            jni::jni_str!("ruxOpenDate"),
            jni::jni_sig!("(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)V"),
            &[jni::JValue::Object(&value), jni::JValue::Object(&min), jni::JValue::Object(&max)],
        )?;
        Ok::<(), jni::errors::Error>(())
    });
}

/// The phone's decimal separator, from its language. `None` if the activity
/// could not be asked, which the caller reads as a dot.
#[cfg(target_os = "android")]
fn android_decimal_separator() -> Option<char> {
    let Ok(activity) = ACTIVITY.lock() else { return None };
    let Some(activity) = activity.as_ref() else { return None };
    let ctx = ndk_context::android_context();
    // Safety: as in `android_set_text_input`, which records the reasoning.
    let vm = unsafe { jni::JavaVM::from_raw(ctx.vm().cast()) };
    vm.attach_current_thread(|env| {
        env.call_method(activity, jni::jni_str!("ruxDecimalSeparator"), jni::jni_sig!("()C"), &[])?
            .c()
    })
    .ok()
    .and_then(|unit| char::from_u32(unit as u32))
}

/// What the platform text menu is asked to show: where, which items, and the
/// selected text for Share and `PROCESS_TEXT`.
#[cfg(target_os = "android")]
#[derive(Clone, Debug, PartialEq)]
struct TextMenu {
    /// Left, top, right, bottom of the selection, physical px in the window.
    rect: [i32; 4],
    /// `MENU_` bits.
    flags: i32,
    /// Empty for a password, which shares nothing.
    text: String,
}

// The items the menu offers, as bits. The same numbers are in RuxActivity.java.
#[cfg(target_os = "android")]
const MENU_COPY: i32 = 1;
#[cfg(target_os = "android")]
const MENU_CUT: i32 = 2;
#[cfg(target_os = "android")]
const MENU_PASTE: i32 = 4;
#[cfg(target_os = "android")]
const MENU_SELECT_ALL: i32 = 8;
#[cfg(target_os = "android")]
const MENU_SHARE: i32 = 16;
#[cfg(target_os = "android")]
const MENU_PROCESS: i32 = 32;
/// Not an item: tells `PROCESS_TEXT` apps their answer will not be used.
#[cfg(target_os = "android")]
const MENU_READONLY: i32 = 64;
/// Select, at a caret: take the word there.
#[cfg(target_os = "android")]
const MENU_SELECT: i32 = 128;
/// Autofill, at a caret: ask the autofill service for this field.
#[cfg(target_os = "android")]
const MENU_AUTOFILL: i32 = 256;

// What Java reports back when an item is chosen. Also in RuxActivity.java.
#[cfg(target_os = "android")]
const MENU_ACTION_COPY: i32 = 0;
#[cfg(target_os = "android")]
const MENU_ACTION_CUT: i32 = 1;
#[cfg(target_os = "android")]
const MENU_ACTION_PASTE: i32 = 2;
#[cfg(target_os = "android")]
const MENU_ACTION_SELECT_ALL: i32 = 3;
#[cfg(target_os = "android")]
const MENU_ACTION_SELECT_WORD: i32 = 4;

/// Show, move or close the platform text menu. `None` closes it.
///
/// Failure is logged by the policy and otherwise ignored, as with the other
/// calls into the activity: the consequence is a menu that does not appear,
/// and the selection and its handles still work.
#[cfg(target_os = "android")]
fn android_text_menu(menu: Option<&TextMenu>) {
    let Ok(activity) = ACTIVITY.lock() else { return };
    let Some(activity) = activity.as_ref() else { return };
    let ctx = ndk_context::android_context();
    // Safety: as in `android_set_text_input`, which records the reasoning.
    let vm = unsafe { jni::JavaVM::from_raw(ctx.vm().cast()) };
    let (show, [l, t, r, b], flags, text) = match menu {
        Some(m) => (true, m.rect, m.flags, m.text.as_str()),
        None => (false, [0; 4], 0, ""),
    };
    let _ = vm.attach_current_thread(|env| {
        let text = env.new_string(text)?;
        env.call_method(
            activity,
            jni::jni_str!("ruxTextMenu"),
            jni::jni_sig!("(ZIIIIILjava/lang/String;)V"),
            &[
                jni::JValue::Bool(show),
                jni::JValue::Int(l),
                jni::JValue::Int(t),
                jni::JValue::Int(r),
                jni::JValue::Int(b),
                jni::JValue::Int(flags),
                jni::JValue::Object(&text),
            ],
        )?;
        Ok::<(), jni::errors::Error>(())
    });
}

/// The theme's text highlight and accent, as the phone has them. `None` when
/// the activity could not be asked yet.
#[cfg(target_os = "android")]
fn android_theme_colors() -> Option<(Rgba, Rgba)> {
    let Ok(activity) = ACTIVITY.lock() else { return None };
    let Some(activity) = activity.as_ref() else { return None };
    let ctx = ndk_context::android_context();
    // Safety: as in `android_set_text_input`, which records the reasoning.
    let vm = unsafe { jni::JavaVM::from_raw(ctx.vm().cast()) };
    let packed = vm
        .attach_current_thread(|env| {
            env.call_method(activity, jni::jni_str!("ruxThemeColors"), jni::jni_sig!("()J"), &[])?
                .j()
        })
        .ok()?;
    // Two ARGB ints, highlight in the high half.
    let argb = |v: u32| {
        let at = |shift: u32| ((v >> shift) & 0xff) as f32 / 255.0;
        Rgba::new(at(16), at(8), at(0), at(24))
    };
    let (highlight, accent) = ((packed >> 32) as u32, packed as u32);
    // A theme that names neither says 0, which is transparent and would hide
    // the selection entirely. Only what was actually said is used.
    if highlight == 0 || accent == 0 {
        return None;
    }
    Some((argb(highlight), argb(accent)))
}

/// Put `text` on Android's clipboard, and say whether it got there.
///
/// `false` for any failure, the activity not yet handed over included, because
/// the caller that cares is Cut, and to Cut every failure means the same thing:
/// do not delete. The Java catches its own exceptions, so a `false` from it is
/// an answer and not a pending exception left on this thread.
#[cfg(target_os = "android")]
fn android_clipboard_write(text: &str) -> bool {
    let Ok(activity) = ACTIVITY.lock() else { return false };
    let Some(activity) = activity.as_ref() else { return false };
    let ctx = ndk_context::android_context();
    // Safety: as in `android_set_text_input`, which records the reasoning.
    let vm = unsafe { jni::JavaVM::from_raw(ctx.vm().cast()) };
    vm.attach_current_thread(|env| {
        let text = env.new_string(text)?;
        env.call_method(
            activity,
            jni::jni_str!("ruxClipboardWrite"),
            jni::jni_sig!("(Ljava/lang/String;)Z"),
            &[jni::JValue::Object(&text)],
        )?
        .z()
    })
    .unwrap_or(false)
}

/// Android's clipboard as text, or `None` when it holds none.
///
/// Empty and unreachable read the same, which is what every other platform's
/// clipboard says in both cases: there is nothing to paste.
#[cfg(target_os = "android")]
fn android_clipboard_read() -> Option<String> {
    let Ok(activity) = ACTIVITY.lock() else { return None };
    let Some(activity) = activity.as_ref() else { return None };
    let ctx = ndk_context::android_context();
    // Safety: as in `android_set_text_input`, which records the reasoning.
    let vm = unsafe { jni::JavaVM::from_raw(ctx.vm().cast()) };
    vm.attach_current_thread(|env| {
        let text = env
            .call_method(
                activity,
                jni::jni_str!("ruxClipboardRead"),
                jni::jni_sig!("()Ljava/lang/String;"),
                &[],
            )?
            .l()?;
        if text.is_null() {
            return Ok(None);
        }
        let text = env.cast_local::<jni::objects::JString>(text)?;
        Ok::<_, jni::errors::Error>(Some(text.try_to_string(env)?))
    })
    .ok()
    .flatten()
}

/// The platform picker closed. Called from Java, on Android's main thread.
///
/// `index` is `-1` when the picker was dismissed without a choice, which is a
/// real outcome and not a failure: a person who opens a picker and changes
/// their mind expects the field to keep what it had.
#[cfg(target_os = "android")]
#[no_mangle]
/// # Safety
///
/// Called by the JVM, with the signature declared in `RuxActivity.java`.
///
/// **Raw pointers, not `jni::Env`.** The first version took `Env` by value, the
/// compiler warned that it is not FFI-safe, and the warning was right: the
/// argument slots shift, `index` reads the wrong one, and the picker's answer
/// arrives as a number no option has. Nothing fails loudly. The dialog opens,
/// the choice is made, Java logs the correct index, and the field does not
/// change, which reads as a missing event rather than a mangled argument.
pub extern "system" fn Java_dev_ruxlang_shell_RuxActivity_nativeSelectChosen(
    _env: *mut std::ffi::c_void,
    _class: *mut std::ffi::c_void,
    index: i32,
) {
    let index = (index >= 0).then_some(index as usize);
    if let Ok(proxy) = PROXY.lock() {
        if let Some(proxy) = proxy.as_ref() {
            let _ = proxy.send_event(RuxEvent::PickedSelect { index });
        }
    }
}

/// The platform date picker closed. Called from Java, on Android's main
/// thread, with the month counting from 1, or `-1` in all three for a
/// dismissal.
///
/// # Safety
///
/// Called by the JVM, with the signature declared in `RuxActivity.java`.
/// Raw pointers for the reason [`Java_dev_ruxlang_shell_RuxActivity_nativeSelectChosen`]
/// records.
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "system" fn Java_dev_ruxlang_shell_RuxActivity_nativeDateChosen(
    _env: *mut std::ffi::c_void,
    _class: *mut std::ffi::c_void,
    year: i32,
    month: i32,
    day: i32,
) {
    let value = (year > 0 && month > 0 && day > 0)
        .then(|| rux_layout::format_date((year, month as u32, day as u32)));
    if let Ok(proxy) = PROXY.lock() {
        if let Some(proxy) = proxy.as_ref() {
            let _ = proxy.send_event(RuxEvent::PickedDate { value });
        }
    }
}

/// An item was chosen from the platform text menu. Called from Java, on
/// Android's main thread, with one of the `MENU_ACTION_` codes.
///
/// # Safety
///
/// Called by the JVM, with the signature declared in `RuxActivity.java`.
/// Raw pointers for the reason [`Java_dev_ruxlang_shell_RuxActivity_nativeSelectChosen`]
/// records.
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "system" fn Java_dev_ruxlang_shell_RuxActivity_nativeTextAction(
    _env: *mut std::ffi::c_void,
    _class: *mut std::ffi::c_void,
    action: i32,
) {
    if let Ok(proxy) = PROXY.lock() {
        if let Some(proxy) = proxy.as_ref() {
            let _ = proxy.send_event(RuxEvent::AndroidTextAction(action));
        }
    }
}

/// The platform text menu closed without Rux asking it to. `collapse` is a
/// JNI `jboolean`, one byte.
///
/// # Safety
///
/// As [`Java_dev_ruxlang_shell_RuxActivity_nativeTextAction`].
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "system" fn Java_dev_ruxlang_shell_RuxActivity_nativeTextMenuClosed(
    _env: *mut std::ffi::c_void,
    _class: *mut std::ffi::c_void,
    collapse: u8,
) {
    if let Ok(proxy) = PROXY.lock() {
        if let Some(proxy) = proxy.as_ref() {
            let _ = proxy.send_event(RuxEvent::AndroidTextMenuClosed { collapse: collapse != 0 });
        }
    }
}

/// An app handed the selection through `PROCESS_TEXT` answered.
///
/// `token` is the focus the text was taken from; an answer for a field that
/// has since lost focus is dropped rather than written into whichever field
/// has it now, the rule [`Java_dev_ruxlang_shell_RuxActivity_nativeTextChanged`]
/// follows for the same reason.
///
/// # Safety
///
/// Called by the JVM, with the signature declared in `RuxActivity.java`.
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "system" fn Java_dev_ruxlang_shell_RuxActivity_nativeProcessedText<'frame>(
    mut env: jni::EnvUnowned<'frame>,
    _class: jni::objects::JClass<'frame>,
    token: i64,
    text: jni::objects::JString<'frame>,
) {
    if token != FIELD_TOKEN.load(std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    env.with_env(|env| {
        let value: String = text.try_to_string(env)?;
        if let Ok(proxy) = PROXY.lock() {
            if let Some(proxy) = proxy.as_ref() {
                let _ = proxy.send_event(RuxEvent::AndroidProcessedText(value));
            }
        }
        Ok::<(), jni::errors::Error>(())
    })
    .resolve::<jni::errors::LogErrorAndDefault>();
}

/// One batch of hot-reload changes, read off the connection to `rux run`.
///
/// Where `rux run --device` leaves the token a dev build proves itself with,
/// in the app's own files directory. See [`dev_mac`].
pub const DEV_TOKEN_FILE: &str = "rux-dev-token";

/// The keyed hash both ends of hot reload answer a challenge with.
///
/// **Why there is a handshake at all.** A dev build dials a port on the
/// phone's own loopback, and when `rux run` is not there, any other app can
/// listen on that port and push documents that would then run inside this
/// one. On the computer, any local process could connect and be sent the
/// project. So each side proves it knows a secret, and the secret never
/// crosses the wire: `rux run --device` writes a fresh one into the app's
/// private files directory with `adb shell run-as` (which only works on a
/// debuggable build, and which no other app can read), and each side answers
/// the other's random challenge with this hash of it. A secret baked into the
/// APK would not do: any app can read another app's APK.
///
/// SipHash-2-4, the keyed hash designed for exactly this size of job, written
/// out rather than taken from a crate: forty lines, no dependency, and both
/// ends are this function.
pub fn dev_mac(token: &str, message: &str) -> String {
    let key = |at: usize| {
        token
            .get(at..at + 16)
            .and_then(|hex| u64::from_str_radix(hex, 16).ok())
            .unwrap_or(0)
    };
    format!("{:016x}", siphash24(key(0), key(16), message.as_bytes()))
}

fn siphash24(k0: u64, k1: u64, data: &[u8]) -> u64 {
    let mut v = [
        k0 ^ 0x736f_6d65_7073_6575,
        k1 ^ 0x646f_7261_6e64_6f6d,
        k0 ^ 0x6c79_6765_6e65_7261,
        k1 ^ 0x7465_6462_7974_6573,
    ];
    fn round(v: &mut [u64; 4]) {
        v[0] = v[0].wrapping_add(v[1]);
        v[1] = v[1].rotate_left(13) ^ v[0];
        v[0] = v[0].rotate_left(32);
        v[2] = v[2].wrapping_add(v[3]);
        v[3] = v[3].rotate_left(16) ^ v[2];
        v[0] = v[0].wrapping_add(v[3]);
        v[3] = v[3].rotate_left(21) ^ v[0];
        v[2] = v[2].wrapping_add(v[1]);
        v[1] = v[1].rotate_left(17) ^ v[2];
        v[2] = v[2].rotate_left(32);
    }
    let mut chunks = data.chunks_exact(8);
    for chunk in &mut chunks {
        let m = u64::from_le_bytes(chunk.try_into().expect("eight bytes"));
        v[3] ^= m;
        round(&mut v);
        round(&mut v);
        v[0] ^= m;
    }
    let mut last = [0u8; 8];
    last[..chunks.remainder().len()].copy_from_slice(chunks.remainder());
    last[7] = data.len() as u8;
    let m = u64::from_le_bytes(last);
    v[3] ^= m;
    round(&mut v);
    round(&mut v);
    v[0] ^= m;
    v[2] ^= 0xff;
    for _ in 0..4 {
        round(&mut v);
    }
    v[0] ^ v[1] ^ v[2] ^ v[3]
}

/// A fresh random challenge, as 32 hex characters. The standard library's
/// hasher keys come from the operating system's random source, which is
/// what makes two of them unguessable; nothing here is secret, only fresh.
pub fn dev_nonce() -> String {
    use std::hash::{BuildHasher, Hasher};
    let half = || {
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u128(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos()),
        );
        h.finish()
    };
    format!("{:016x}{:016x}", half(), half())
}

/// The app's half of the handshake: answer `rux run`'s challenge, set one of
/// its own, and check the answer. Reads through the same buffered reader the
/// batches will be read from, so nothing the host sends after its answer is
/// lost in a buffer thrown away.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
fn dev_handshake_app(
    reader: &mut impl std::io::BufRead,
    writer: &mut impl std::io::Write,
    token: &str,
) -> Result<(), String> {
    let mut line = String::new();
    reader.read_line(&mut line).map_err(|e| e.to_string())?;
    let theirs = line.trim().strip_prefix("rux-hello ").ok_or("it did not say hello")?.to_string();
    let ours = dev_nonce();
    writeln!(writer, "rux-auth {} {ours}", dev_mac(token, &format!("app {theirs}")))
        .and_then(|()| writer.flush())
        .map_err(|e| e.to_string())?;
    line.clear();
    reader.read_line(&mut line).map_err(|e| e.to_string())?;
    let answer = line.trim().strip_prefix("rux-ok ").ok_or("it did not answer")?;
    if answer != dev_mac(token, &format!("host {ours}")) {
        return Err("it does not know this run's token".into());
    }
    Ok(())
}

/// The protocol is lines, with file contents inline:
///
/// ```text
/// put <length> <path>
/// <length bytes>
/// del <path>
/// reload
/// ```
///
/// A batch ends at `reload`. `Ok(None)` is the other end closing cleanly
/// between batches. Plain functions rather than methods, so the desktop tests
/// cover what only ever runs on a phone.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
fn read_dev_batch(
    reader: &mut impl std::io::BufRead,
) -> std::io::Result<Option<Vec<(String, Option<Vec<u8>>)>>> {
    use std::io::{Error, ErrorKind};
    let bad = |what: &str| Error::new(ErrorKind::InvalidData, what.to_string());
    let mut batch = Vec::new();
    let mut line = String::new();
    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            return if batch.is_empty() { Ok(None) } else { Err(bad("closed mid-batch")) };
        }
        let text = line.trim_end_matches(['\r', '\n']);
        if text == "reload" {
            return Ok(Some(batch));
        }
        if let Some(path) = text.strip_prefix("del ") {
            batch.push((path.to_string(), None));
        } else if let Some(rest) = text.strip_prefix("put ") {
            let (length, path) = rest.split_once(' ').ok_or_else(|| bad("put without a path"))?;
            let length: usize = length.parse().map_err(|_| bad("put without a length"))?;
            let mut bytes = vec![0; length];
            reader.read_exact(&mut bytes)?;
            batch.push((path.to_string(), Some(bytes)));
        } else {
            return Err(bad("unknown line"));
        }
    }
}

/// Apply a batch, and say whether anything actually changed: a file that
/// arrives with the contents already held is not a reason to reload.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
fn apply_dev_changes(
    files: &mut rux_runtime::MemorySource,
    changes: Vec<(String, Option<Vec<u8>>)>,
) -> bool {
    let mut changed = false;
    for (path, contents) in changes {
        match contents {
            Some(bytes) if files.file(&path) != Some(bytes.as_slice()) => {
                files.insert(&path, bytes);
                changed = true;
            }
            None if files.file(&path).is_some() => {
                files.remove(&path);
                changed = true;
            }
            _ => {}
        }
    }
    changed
}

/// Stay connected to `rux run --device` for as long as the app runs.
///
/// **The app dials out, the host listens.** `adb reverse` makes the host's
/// port a port on the device's own loopback, which is the one direction that
/// works the same on an emulator and on a phone over USB or wireless adb.
/// Nobody listening is the ordinary case (the app was opened without
/// `rux run`), so a refused connection is retried quietly, once a second.
#[cfg(target_os = "android")]
fn dev_link(port: u16, token_file: Option<PathBuf>) {
    loop {
        // Read afresh on every attempt: `rux run --device` writes a new one
        // each run, possibly after this app started.
        let token = token_file
            .as_ref()
            .and_then(|f| std::fs::read_to_string(f).ok())
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty());
        let connected = token.as_deref().and_then(|token| {
            let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).ok()?;
            let mut reader = std::io::BufReader::new(stream.try_clone().ok()?);
            // A listener that says nothing cannot hold the link.
            let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(5)));
            let checked = dev_handshake_app(&mut reader, &mut stream, token);
            let _ = stream.set_read_timeout(None);
            match checked {
                Ok(()) => Some(reader),
                Err(why) => {
                    android_log(&format!("hot reload: refused whoever is on port {port}: {why}"));
                    None
                }
            }
        });
        if let Some(mut reader) = connected {
            android_log(&format!("hot reload: connected to rux on port {port}"));
            while let Ok(Some(batch)) = read_dev_batch(&mut reader) {
                if let Ok(proxy) = PROXY.lock() {
                    if let Some(proxy) = proxy.as_ref() {
                        let _ = proxy.send_event(RuxEvent::DevFiles(batch));
                    }
                }
            }
            android_log("hot reload: rux went away");
        }
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
}

/// A line in logcat under the tag `rux`, which is where a dev reads what the
/// app is doing: `adb logcat -s rux`.
#[cfg(target_os = "android")]
fn android_log(message: &str) {
    #[link(name = "log")]
    extern "C" {
        fn __android_log_write(
            priority: i32,
            tag: *const std::ffi::c_char,
            text: *const std::ffi::c_char,
        ) -> i32;
    }
    const INFO: i32 = 4;
    let Ok(text) = std::ffi::CString::new(message.replace(char::from(0), "")) else { return };
    // Safety: both pointers are to NUL-terminated strings that outlive the call.
    unsafe { __android_log_write(INFO, c"rux".as_ptr(), text.as_ptr()) };
}

/// The last reported insets, in logical pixels.
///
/// Divided by the scale factor on the way out, because Android counts insets in
/// physical pixels while `env(safe-area-inset-*)` is a CSS length like every
/// other. The device profiles behind `--preview` are written in logical pixels
/// for the same reason, so both roads reach the stylesheet in one unit.
#[cfg(target_os = "android")]
fn android_safe_area(scale: f64) -> Insets {
    let packed = SAFE_AREA.load(std::sync::atomic::Ordering::Relaxed);
    let at = |shift: u32| ((packed >> shift) & 0xffff) as f32 / scale.max(0.01) as f32;
    Insets { top: at(48), right: at(32), bottom: at(16), left: at(0) }
}

/// Run a document as an Android activity.
///
/// The Android counterpart of [`run`], and deliberately the smaller function.
/// Everything that makes the desktop entry long is absent here:
///
/// - **No watcher.** An APK's documents are not files on a disk anyone can
///   watch. Reloading on Android is driven from the other end, over `adb`.
/// - **No window sizing, and no `--preview`.** An activity is given the
///   display. The device is the device.
/// - **No route.** A deep link arrives as an `Intent`, which is a later piece of
///   work than the first APK; until then an Android app starts at its entry
///   document exactly as a desktop one does with no `--route`.
///
/// What is left is the same `App`, the same event loop and the same frame path
/// the desktop has always used, which is the point: this is not a second shell.
///
/// `path` names the entry document *inside the source that is already
/// installed*, not a file on a filesystem. The caller installs a
/// [`rux_runtime::Source`] first (a generated release wrapper embeds every
/// document and installs a `MemorySource`), so by the time this runs, loading a
/// document is a lookup and never touches Android's asset machinery.
///
/// Called from the `android_main` of a generated cdylib, which is how an Android
/// app is entered: there is no `fn main` on the other side.
#[cfg(target_os = "android")]
pub fn run_android(app: android_activity::AndroidApp, path: PathBuf) {
    run_android_with(app, path, None);
}

/// [`run_android`] for a dev build: the same app, which also keeps a
/// connection to `rux run --device` on `port` and reloads when it sends files.
///
/// `files` are the documents the build embedded, which this installs as the
/// source and then patches. A release build never calls this, so it carries
/// no socket and asks for no network permission.
#[cfg(target_os = "android")]
pub fn run_android_dev(
    app: android_activity::AndroidApp,
    path: PathBuf,
    files: rux_runtime::MemorySource,
    port: u16,
) {
    run_android_with(app, path, Some((files, port)));
}

#[cfg(target_os = "android")]
fn run_android_with(
    app: android_activity::AndroidApp,
    path: PathBuf,
    dev: Option<(rux_runtime::MemorySource, u16)>,
) {
    use winit::platform::android::EventLoopBuilderExtAndroid;

    // Where hot reload's token is, read before the app is handed on.
    let token_file = app.internal_data_path().map(|dir| dir.join(DEV_TOKEN_FILE));
    let event_loop = EventLoop::<RuxEvent>::with_user_event()
        .with_android_app(app)
        .build()
        .expect("create event loop");
    // Wait, not Poll, for the same reason as the desktop: an idle window should
    // cost nothing. It matters more here, where the cost is someone's battery.
    event_loop.set_control_flow(ControlFlow::Wait);

    // Left where the JNI callbacks can find it. They run on Android's main
    // thread and have no other way to reach this loop.
    if let Ok(mut slot) = PROXY.lock() {
        *slot = Some(event_loop.create_proxy());
    }

    // `App` itself takes no proxy here. The only thing outside this loop with
    // anything to say is a JNI callback, and it reaches the proxy above rather
    // than going through the app.
    // Installed before the app loads its entry document, so the first load
    // and every reload read the same copy.
    let dev = dev.map(|(files, port)| {
        rux_runtime::set_source(std::rc::Rc::new(files.clone()));
        std::thread::spawn(move || dev_link(port, token_file));
        files
    });
    let mut app = App::new(path);
    app.dev_files = dev;
    // Before the first frame, like a deep link: Android killed the app and is
    // bringing it back where it was.
    let restored = RESTORED_STATE.lock().ok().and_then(|mut s| s.take());
    let restored = restored.as_deref().and_then(rux_runtime::SavedState::decode);
    let was_restored = restored.is_some();
    if let Some(state) = restored {
        app.document.restore_state(&state);
        app.restored_focus = Some(state.focus);
        app.restored_fields = state.fields;
    }
    // After the restore, so a link that arrives as Android brings a killed app
    // back is pushed on top of where the person was, the same as a link to a
    // running app. Otherwise this is a cold start and the link is the first
    // page, as `--route` makes it on the desktop.
    if let Some(route) = PENDING_LINK.lock().ok().and_then(|mut s| s.take()) {
        android_log(&format!("link: {route}, before the first frame"));
        app.document.open_link(&route, was_restored);
    }
    event_loop.run_app(&mut app).expect("run app");

    // **Returning from here is not enough: the process has to end with it.**
    // The loop is only left when the app has asked to close, and Android does
    // not end a process just because its activity finished. It keeps it cached
    // and builds the next activity inside it, which calls this function a
    // second time, and winit refuses a second `EventLoop` in one process.
    //
    // Driven, and the failure is not subtle: after closing with Back, the next
    // launch panicked with `create event loop: RecreationAttempt` and the app
    // bounced straight back to the launcher. Ending the process makes the next
    // launch a cold start, which is the only kind that works.
    //
    // After `run_app`, so everything the loop owns has already been dropped.
    // The activity is finished by `android-activity` when `android_main`
    // returns, and it does not get the chance here; Android tears down an
    // activity whose process is gone, which is the same outcome by a shorter
    // road.
    std::process::exit(0);
}

#[cfg(test)]
mod tests {
    use super::*;
    use rux_runtime::{Diagnostics, Warning};

    /// A custom scheme's host is the route's first segment, because that is
    /// what anyone writing `myapp://settings` means.
    #[test]
    fn a_custom_scheme_link_names_its_route() {
        assert_eq!(link_route("tasks://settings/profile").as_deref(), Some("/settings/profile"));
        assert_eq!(link_route("tasks://user/7?tab=2").as_deref(), Some("/user/7?tab=2"));
        assert_eq!(link_route("tasks:///settings").as_deref(), Some("/settings"));
        assert_eq!(link_route("tasks:settings").as_deref(), Some("/settings"));
        assert_eq!(link_route("tasks://").as_deref(), Some("/"));
        assert_eq!(link_route("tasks://?q=1").as_deref(), Some("/?q=1"));
    }

    /// An https link's host is the site, already matched by the filter.
    #[test]
    fn a_web_link_names_only_its_path() {
        assert_eq!(link_route("https://example.com/user/7").as_deref(), Some("/user/7"));
        assert_eq!(link_route("https://example.com").as_deref(), Some("/"));
        assert_eq!(link_route("https://example.com?x=1").as_deref(), Some("/?x=1"));
        assert_eq!(link_route("https://example.com/a#section").as_deref(), Some("/a"));
    }

    /// Kept escaped, as `location.pathname` keeps it, so a parameter reads
    /// the same from a link as from the web build.
    #[test]
    fn a_link_keeps_its_percent_escapes() {
        assert_eq!(link_route("tasks://user/J%C3%BCrgen").as_deref(), Some("/user/J%C3%BCrgen"));
    }

    #[test]
    fn a_link_that_is_not_a_url_names_no_route() {
        assert_eq!(link_route("settings"), None);
        assert_eq!(link_route(":x"), None);
        assert_eq!(link_route("https:nothing"), None);
    }

    /// A batch from `rux run --device`: files with their bytes, a deletion,
    /// and the line that ends it. Contents may hold anything, a line break
    /// included, because they are counted rather than read as lines.
    #[test]
    fn a_hot_reload_batch_reads_back() {
        let wire = b"put 12 app.rux\n<x>\nline</x>del old.css\nreload\nput 1 a\nbreload\n";
        let mut reader = std::io::BufReader::new(&wire[..]);
        let first = read_dev_batch(&mut reader).unwrap().unwrap();
        assert_eq!(
            first,
            vec![("app.rux".to_string(), Some(b"<x>\nline</x>".to_vec())), ("old.css".to_string(), None)]
        );
        let second = read_dev_batch(&mut reader).unwrap().unwrap();
        assert_eq!(second, vec![("a".to_string(), Some(b"b".to_vec()))]);
        assert!(read_dev_batch(&mut reader).unwrap().is_none(), "a clean close");

        let mut cut = std::io::BufReader::new(&b"put 5 a\nab"[..]);
        assert!(read_dev_batch(&mut cut).is_err(), "a file cut short is not a file");
        let mut junk = std::io::BufReader::new(&b"hello\n"[..]);
        assert!(read_dev_batch(&mut junk).is_err());
    }

    /// The first batch on every connection is the whole project, and a
    /// reload is only worth its cost when something in it differs.
    #[test]
    fn a_hot_reload_changes_only_what_differs() {
        let mut files = rux_runtime::MemorySource::new().with("app.rux", "one").with("x.css", "c");
        let same = vec![("app.rux".to_string(), Some(b"one".to_vec()))];
        assert!(!apply_dev_changes(&mut files, same), "the same bytes are no change");
        assert!(!apply_dev_changes(&mut files, vec![("gone.rux".to_string(), None)]));

        let edit = vec![("app.rux".to_string(), Some(b"two".to_vec())), ("x.css".to_string(), None)];
        assert!(apply_dev_changes(&mut files, edit));
        assert_eq!(files.file("app.rux"), Some(&b"two"[..]));
        assert_eq!(files.file("x.css"), None);
    }

    /// Android keeps the id between asking for the structure and filling it,
    /// so it has to name the field, be positive, and never be 0.
    #[test]
    fn an_autofill_id_names_the_field() {
        let a = autofill_id("email", None, None);
        assert_eq!(a, autofill_id("email", None, None), "stable");
        assert!(a > 0);
        assert_ne!(a, autofill_id("email", Some("2"), None), "another row");
        assert_ne!(a, autofill_id("email", None, Some("c1")), "another instance");
    }

    #[test]
    fn autofill_hints_from_autocomplete_or_the_field() {
        use rux_layout::{autofill_hints, Field, InputKind, Keyboard};
        let with = |ac: &str| Field { autocomplete: Some(ac.into()), ..Field::default() };
        assert_eq!(autofill_hints(&with("username"), InputKind::Text), ["username"]);
        assert_eq!(autofill_hints(&with("section-a shipping postal-code"), InputKind::Text), ["postalCode"]);
        assert_eq!(autofill_hints(&with("new-password"), InputKind::Password), ["newPassword"]);
        assert!(autofill_hints(&with("off"), InputKind::Password).is_empty());
        assert!(!with("off").autofill());
        assert_eq!(autofill_hints(&Field::default(), InputKind::Password), ["password"]);
        let email = Field { keyboard: Keyboard::Email, ..Field::default() };
        assert_eq!(autofill_hints(&email, InputKind::Text), ["emailAddress"]);
        assert!(autofill_hints(&Field::default(), InputKind::Text).is_empty());
    }

    fn warned(message: &str) -> Diagnostics {
        Diagnostics { warnings: vec![Warning::new(message)], ..Diagnostics::default() }
    }

    /// What a number field makes of its text: a number once it is one, and
    /// nothing while it is on the way to one. The words Rust would also
    /// read as numbers are words here.
    #[test]
    fn a_number_field_reads_numbers_and_only_numbers() {
        let dot = |t: &str| parse_number(t, '.');
        assert_eq!(dot("42"), Some(42.0));
        assert_eq!(dot(" -3.5 "), Some(-3.5));
        assert_eq!(dot("1."), Some(1.0));
        assert_eq!(dot(".5"), Some(0.5));
        assert_eq!(dot("1e3"), Some(1000.0));
        for draft in ["", "-", ".", ",", "+", "1e", "abc", "inf", "NaN", "1.2.3", "1,000.5,"] {
            assert_eq!(dot(draft), None, "{draft:?} is not a number yet");
        }
    }

    /// The comma means what the person's language says it means. `24,000`
    /// on an English phone is twenty-four thousand: typed on the phone and
    /// read as 24 by the version that guessed.
    #[test]
    fn a_comma_means_what_the_language_says() {
        let english = |t: &str| parse_number(t, '.');
        assert_eq!(english("24,000"), Some(24000.0));
        assert_eq!(english("1,234,567.5"), Some(1234567.5));
        assert_eq!(english("2,5"), Some(25.0), "a thousands separator, dropped");
        assert_eq!(english("1.234,5"), None, "no thousands after the point");
        assert_eq!(english("1\u{a0}000"), Some(1000.0), "a space groups too");

        let german = |t: &str| parse_number(t, ',');
        assert_eq!(german("24,000"), Some(24.0));
        assert_eq!(german("2,5"), Some(2.5));
        assert_eq!(german("1.234,5"), Some(1234.5));
        assert_eq!(german(",5"), Some(0.5));
        assert_eq!(german("1,2.3"), None);
    }

    /// A date field writes a day only once it is one, inside its range,
    /// padded the way HTML writes it.
    #[test]
    fn a_date_field_writes_only_real_days_in_range() {
        let field = Field { min: Some((2026, 1, 1)), max: Some((2026, 12, 31)), ..Field::default() };
        let date = |text: &str| typed_value(InputKind::Date, text, &field, '.');
        assert_eq!(date("2026-9-3"), Some(rux_reactive::Value::Text("2026-09-03".into())));
        assert_eq!(date("2026-09"), None, "not a day yet");
        assert_eq!(date("2025-12-31"), None, "before min");
        assert_eq!(date("2027-01-01"), None, "after max");
        assert_eq!(typed_value(InputKind::Text, "2026-09-03", &field, '.'), None, "text is not typed");
    }

    /// The names on the command line are the names in the table, and a wrong
    /// one is answered with the list rather than with "unknown".
    #[test]
    fn a_device_profile_is_found_by_the_name_that_is_typed() {
        assert_eq!(DeviceProfile::by_name("phone").map(|p| p.width), Some(393.0));
        assert!(DeviceProfile::by_name("iphone-42").is_none());

        let listing = DeviceProfile::names_with_descriptions();
        for profile in DEVICE_PROFILES {
            assert!(listing.contains(profile.name), "{} is missing from the listing", profile.name);
        }
    }

    /// Every profile has a safe area worth previewing for, and a phone-sized
    /// display. A profile that took nothing off any edge would be a desktop
    /// window with a different width, which is not what this is for.
    #[test]
    fn every_profile_takes_something_off_an_edge() {
        for profile in DEVICE_PROFILES {
            let insets = profile.safe_area;
            assert!(
                insets.top + insets.right + insets.bottom + insets.left > 0.0,
                "{} reports no safe area at all",
                profile.name
            );
            assert!(profile.density >= 2.0, "{} is not a device density", profile.name);
        }
    }

    /// The overlay covers the app it is describing, so it has to be dismissable.
    #[test]
    fn dismissing_the_overlay_hides_it() {
        let diag = warned("float does nothing");
        assert!(overlay_visible(&diag, None), "shown before it is dismissed");
        assert!(!overlay_visible(&diag, Some(&diag)), "hidden after");
    }

    /// And it must come back on its own when what is wrong changes, or
    /// dismissing a warning would silence the error you write next.
    #[test]
    fn a_dismissed_overlay_returns_when_the_diagnostics_change() {
        let dismissed = warned("float does nothing");

        let another_warning = warned("`:nope` is not supported");
        assert!(overlay_visible(&another_warning, Some(&dismissed)));

        let now_broken = Diagnostics {
            error: Some("parse error".into()),
            stale: true,
            warnings: dismissed.warnings.clone(),
            prints: Vec::new(),
        };
        assert!(
            overlay_visible(&now_broken, Some(&dismissed)),
            "an error arriving after a dismissed warning must show"
        );
    }

    /// Fixing everything hides the panel whether or not it was dismissed, and a
    /// stale dismissal must not make an empty document look dismissed-into-silence.
    #[test]
    fn nothing_wrong_means_no_overlay() {
        let clean = Diagnostics::default();
        assert!(!overlay_visible(&clean, None));
        assert!(!overlay_visible(&clean, Some(&warned("old"))));
    }

    /// A 200x200 box holding 500px-tall content: it scrolls down, not sideways.
    fn tall() -> ScrollRegion {
        ScrollRegion {
            transform: None,
            alpha: 1.0,
            id: 0,
            x: 0.0,
            y: 0.0,
            width: 200.0,
            height: 200.0,
            content_width: 200.0,
            content_height: 500.0,
            max: Offset { x: 0.0, y: 300.0 },
            within: None,
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
    /// box, invisible to every test that only looked at the vertical bar.)
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

    /// At the end of the content the thumb is at the end of its track, the
    /// bottom of the thumb meets the bottom of the box.
    #[test]
    fn thumb_reaches_the_end_of_the_track() {
        let r = tall();
        let (_, y, _, h) = bar_thumb(&r, Offset { x: 0.0, y: 300.0 }, Axis2::Y).expect("a thumb");
        assert_eq!(y + h, r.height);
    }

    /// The negative case: an axis with no travel has no thumb, nothing to draw,
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

#[cfg(test)]
mod selection_handles {
    use super::{handle_paints, Handle, Paint, HANDLE_R, HANDLE_REACH};

    /// Each handle hangs below the text point: the start's to the left, the
    /// end's to the right, and the caret's straight down, so the two ends of
    /// a one-letter selection do not cover each other.
    #[test]
    fn handles_hang_below_their_point_on_their_own_side() {
        let point = (100.0, 50.0);
        let (sx, sy) = Handle::Start.centre(point);
        let (ex, ey) = Handle::End.centre(point);
        let (cx, cy) = Handle::Caret.centre(point);
        assert!(sx < 100.0 && sy > 50.0);
        assert!(ex > 100.0 && ey > 50.0);
        assert!((cx - 100.0).abs() < 1e-4 && cy > 50.0);
    }

    #[test]
    fn a_finger_reaches_a_handle_only_near_it() {
        let point = (100.0, 50.0);
        let (cx, cy) = Handle::End.centre(point);
        assert!(Handle::End.reach(point, (cx, cy)).is_some());
        assert!(Handle::End.reach(point, (cx + HANDLE_REACH - 1.0, cy)).is_some());
        assert!(Handle::End.reach(point, (cx + HANDLE_REACH + 1.0, cy)).is_none());
    }

    /// The caret's teardrop is the end's square turned about its point: the
    /// square's far corner must come out straight below the point, a
    /// diagonal's length away. A turn the wrong way puts it off to the side.
    #[test]
    fn the_caret_handle_points_straight_up() {
        let (x, y) = (100.0f32, 50.0f32);
        let paints = handle_paints(Handle::Caret, (x, y), super::HANDLE_DEFAULT);
        let Paint::PushTransform(m) = paints[0] else { panic!("turned: {paints:?}") };
        let far = (x + 2.0 * HANDLE_R, y + 2.0 * HANDLE_R);
        let tx = m[0] * far.0 + m[2] * far.1 + m[4];
        let ty = m[1] * far.0 + m[3] * far.1 + m[5];
        assert!((tx - x).abs() < 1e-3, "x {tx}");
        assert!((ty - (y + 2.0 * HANDLE_R * std::f32::consts::SQRT_2)).abs() < 1e-3, "y {ty}");
        // The point itself does not move.
        let (px, py) = (m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5]);
        assert!((px - x).abs() < 1e-3 && (py - y).abs() < 1e-3);
    }

    /// The sharp corner is the one touching the text: top-right for the
    /// start, top-left for the end.
    #[test]
    fn the_sharp_corner_touches_the_text() {
        let point = (100.0, 50.0);
        let Paint::Rect(start) = &handle_paints(Handle::Start, point, super::HANDLE_DEFAULT)[0] else {
            panic!()
        };
        assert_eq!((start.x + start.width, start.y), point);
        assert_eq!(start.radius[1], 0.0);
        let Paint::Rect(end) = &handle_paints(Handle::End, point, super::HANDLE_DEFAULT)[0] else {
            panic!()
        };
        assert_eq!((end.x, end.y), point);
        assert_eq!(end.radius[0], 0.0);
    }
}

#[cfg(test)]
mod word_near_tests {
    use super::word_near;

    #[test]
    fn select_takes_the_word_at_or_before_the_caret() {
        let text = "Gold highlight, dark letters";
        // After the last word, the usual place a caret menu is opened.
        assert_eq!(word_near(text, text.len()), Some((21, 28)));
        // Inside a word, and at its start.
        assert_eq!(word_near(text, 7), Some((5, 14)));
        assert_eq!(word_near(text, 5), Some((5, 14)));
        // In the gap after a comma, the word before it.
        assert_eq!(word_near(text, 15), Some((5, 14)));
        assert_eq!(word_near("   ", 2), None);
        assert_eq!(word_near("", 0), None);
    }
}
