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

#[cfg(not(target_arch = "wasm32"))]
use notify::{EventKind, RecursiveMode, Watcher};
use rux_layout::{
    Background, Cursor, FocusItem, FocusKind, FocusRegion, HitRegion,
    Offset, Paint, PaintRect, PaintText, Rgba, ScrollRegion, SelectRegion, StateRegion, TextAlign,
    TextContent, TextWrap,
};
use rux_runtime::{Document, Focus, InteractionState, Viewport};
use vello::kurbo::Affine;
use vello::peniko::Color;
use vello::util::{RenderContext, RenderSurface};
use vello::wgpu;
use vello::wgpu::CurrentSurfaceTexture;
use vello::{AaConfig, AaSupport, Renderer, RendererOptions, RenderParams, Scene};
#[cfg(not(target_arch = "wasm32"))]
use accesskit::{Node as AccessKitNode, NodeId, Role, Toggled, Tree, TreeUpdate};
// Only the accessibility tree uses these, so they are gated with it rather
// than sitting unused in the wasm build.
#[cfg(not(target_arch = "wasm32"))]
use rux_layout::{AccessNode, AccessRole};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, Ime, MouseButton, MouseScrollDelta, TouchPhase, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::{CursorIcon, Window, WindowId};

/// Events delivered to the winit loop from outside it.
#[derive(Debug)]
enum RuxEvent {
    /// The `.rux` file changed on disk.
    #[cfg(not(target_arch = "wasm32"))]
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
    /// Assistive technology asked us something (it attached, it wants the
    /// tree, it moved focus). Delivered through the same proxy as hot-reload.
    #[cfg(not(target_arch = "wasm32"))]
    Access(accesskit_winit::Event),
}

#[cfg(not(target_arch = "wasm32"))]
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

/// Where the toolbar sits for a field at `(x, y, w, h)`, and the box of each
/// button, in logical px.
///
/// Above the field when there is room, below it when there is not, and never off
/// the left edge. One function so the painter and the hit test cannot drift.
fn toolbar_layout(
    field: (f32, f32, f32, f32),
    viewport: (f32, f32),
) -> ((f32, f32, f32, f32), Vec<(TextAction, f32, f32, f32, f32)>) {
    let total: f32 = TextAction::ALL.iter().map(|a| a.width()).sum();
    let (fx, fy, _, fh) = field;
    let x = fx.min(viewport.0 - total).max(0.0);
    // Above by preference: a finger selecting text is usually below the line it
    // is selecting, and a toolbar under the finger is one you cannot read.
    let above = fy - TOOLBAR_H - TOOLBAR_GAP;
    let y = if above >= 0.0 { above } else { fy + fh + TOOLBAR_GAP };

    let mut buttons = Vec::with_capacity(TextAction::ALL.len());
    let mut bx = x;
    for action in TextAction::ALL {
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
///
/// `within` is the scroller the item sits in, if any. The ring is painted as
/// its own scene after the document's, so it never passes through the
/// `PushClip` a scroller emits around its children; without clipping it here, a
/// ring on a row scrolled out of a list draws over whatever is above the list.
/// The ring is allowed the 2px it sits outside its element by, so a focused row
/// flush with the top of its container still shows one.
fn focus_ring(item: &FocusItem, within: Option<&ScrollRegion>) -> Vec<Paint> {
    let ring = Paint::Rect(PaintRect {
        x: item.x - 2.0,
        y: item.y - 2.0,
        width: item.width + 4.0,
        height: item.height + 4.0,
        background: None,
        radius: [7.0; 4],
        border_width: 2.0,
        border_color: Some(Rgba::new(0.54, 0.71, 0.98, 1.0)), // #89b4fa
    });
    let Some(r) = within else { return vec![ring] };
    // Scrolled entirely out of view: draw nothing rather than a ring clipped to
    // a sliver at the edge, which reads as a rendering fault.
    if item.y + item.height < r.y || item.y > r.y + r.height {
        return Vec::new();
    }
    vec![
        Paint::PushClip { x: r.x - 2.0, y: r.y - 2.0, width: r.width + 4.0, height: r.height + 4.0, radius: [0.0; 4] },
        ring,
        Paint::PopClip,
    ]
}

/// Paint items for an open dropdown: a single floating panel with a shadow, the
/// selected value picked out as a pill, and thin separators between options.
/// The selection toolbar: one rounded strip of actions above (or below) the
/// focused field. Same palette as the dropdown, so the two read as one system.
fn toolbar_paints(field: (f32, f32, f32, f32), viewport: (f32, f32)) -> Vec<Paint> {
    let panel_bg = Rgba::new(0.19, 0.20, 0.27, 1.0); // #313244
    let border = Rgba::new(0.27, 0.28, 0.35, 1.0); // #45475a
    let ink = Rgba::new(0.80, 0.84, 0.96, 1.0); // #cdd6f4
    let divider = Rgba::new(0.35, 0.36, 0.44, 1.0); // #585b70

    let ((x, y, w, h), buttons) = toolbar_layout(field, viewport);
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
        border_width: 1.0,
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
                border_width: 0.0,
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
                preedit: None,
            },
        }));
    }
    out
}

// ── Accessibility ───────────────────────────────────────────────────────────

/// The accessibility tree's root. Element ids follow it, offset by one, so an
/// element's id is stable for a given position in document order.
#[cfg(not(target_arch = "wasm32"))]
const ACCESS_ROOT: NodeId = NodeId(0);

#[cfg(not(target_arch = "wasm32"))]
fn to_accesskit_role(role: AccessRole) -> Role {
    match role {
        AccessRole::Label => Role::Label,
        AccessRole::Heading => Role::Heading,
        AccessRole::Button => Role::Button,
        AccessRole::CheckBox => Role::CheckBox,
        AccessRole::RadioButton => Role::RadioButton,
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
#[cfg(not(target_arch = "wasm32"))]
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

    let is_error = diag.error.is_some();
    let (bg, edge) = if is_error {
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
        lines.extend(
            wrap_overlay(&format!("• {warning}"), text_w)
                .into_iter()
                .map(|l| (l, if is_error { muted } else { ink })),
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

    let title = match (&diag.error, diag.warnings.len(), diag.prints.len()) {
        (Some(_), 0, _) => format!("rux: {} failed to load", file_name(path)),
        (Some(_), n, _) => format!("rux: {} failed to load  ·  {n} warning(s)", file_name(path)),
        (None, 0, p) => format!("rux: {p} printed from {}", file_name(path)),
        (None, n, 0) => format!("rux: {n} warning(s) in {}", file_name(path)),
        (None, n, p) => format!("rux: {n} warning(s), {p} printed in {}", file_name(path)),
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
        border_width: 2.0,
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
    #[cfg(not(target_arch = "wasm32"))]
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
    /// Proxy for events raised outside the loop: the file watcher and the
    /// accessibility adapter both deliver through it.
    #[cfg(not(target_arch = "wasm32"))]
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
    /// The `r-model` of the currently focused input, if any.
    focused: Option<String>,
    /// The `r-key` of the row that input is in, when it is inside an `r-for`.
    /// The model repeats across a list's rows, so this is the half that says
    /// which row, and it is what keeps the caret with its row when the list is
    /// reordered.
    focused_row: Option<String>,
    /// Whether the focused input is a `type="textarea"` (Enter → newline).
    focused_multiline: bool,
    /// The currently open `select` dropdown, as `(r-model, row key)`. Survives
    /// the rebuild after a state change, like scroll offsets.
    ///
    /// The row is half the identity, for the same reason an input needs one: the
    /// model is recorded as written, so every row of an `r-for` carries the same
    /// one. Keyed by the model alone, tapping row three's select opened row
    /// one's dropdown, drew it over row one, hit-tested the options against row
    /// one's box, and wrote the chosen option into row one.
    open_select: Option<(String, Option<String>)>,
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
    #[cfg(not(target_arch = "wasm32"))]
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
        #[cfg(not(target_arch = "wasm32"))] proxy: winit::event_loop::EventLoopProxy<RuxEvent>,
        #[cfg(target_arch = "wasm32")] document: Document,
    ) -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        let document = load_document(&path);
        Self {
            context: RenderContext::new(),
            state: None,
            #[cfg(not(target_arch = "wasm32"))]
            proxy,
            #[cfg(target_arch = "wasm32")]
            pending: Rc::new(RefCell::new(None)),
            #[cfg(target_arch = "wasm32")]
            starting: false,
            #[cfg(not(target_arch = "wasm32"))]
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
            alt_held: false,
            scrolls: Vec::new(),
            offsets: Vec::new(),
            bar_drag: None,
            touch: None,
            focused: None,
            focused_row: None,
            focused_multiline: false,
            open_select: None,
            caret: 0,
            anchor: 0,
            overlay_dismissed: None,
            overlay_rect: None,
            preedit: None,
            text_drag: false,
            touch_text: None,
            text_scroll: 0.0,
            last_click: None,
            #[cfg(not(target_arch = "wasm32"))]
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
                Some(route) => self.document.start_at(&route),
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
            // Only a scroller the item is horizontally within can own it, a
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
        let value = self.document.value_in(&region.model, region.row.as_deref());
        match region.text.as_ref() {
            Some(t) if !value.is_empty() => {
                let (tx, ty) = self.text_point(region, t, px, py);
                self.text.index_at_point(
                    &value,
                    &rux_paint::text_style(&t.content),
                    Some(t.width),
                    tx,
                    ty,
                )
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
    fn text_point(
        &self,
        region: &FocusRegion,
        t: &rux_layout::PaintText,
        px: f32,
        py: f32,
    ) -> (f32, f32) {
        (px - t.x + self.text_scroll_for(region), py - t.y)
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
        caret: usize,
        scroll: &mut f32,
        text: &mut rux_text::TextEngine,
        document: &mut rux_runtime::Document,
    ) -> f32 {
        let Some(model) = focused else {
            *scroll = 0.0;
            return 0.0;
        };
        let Some(region) = layout
            .focuses
            .iter()
            .find(|f| f.model == model && f.row.as_deref() == focused_row)
        else {
            return *scroll;
        };
        // A textarea has a real scroll region and is handled by
        // `scroll_caret_into_view`; this is only for the clipped single line.
        let (false, Some(t)) = (region.multiline, region.text.as_ref()) else {
            *scroll = 0.0;
            return 0.0;
        };
        let value = document.value_in(model, focused_row);
        let style = rux_paint::text_style(&t.content);
        let (cx, _, _) = text.caret_geometry(&value, &style, Some(t.width), caret.min(value.len()));

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
        let full = text.measure(&value, &style, None).0;
        *scroll = scroll.clamp(0.0, (full - visible).max(0.0));
        *scroll
    }

    /// The focused field's box, when it has a selection worth offering actions
    /// on. `None` means no toolbar: nothing focused, or nothing selected.
    ///
    /// Tied to the selection rather than to focus so the strip is not sitting
    /// over the page the whole time an input has a caret in it.
    fn toolbar_field(&self) -> Option<(f32, f32, f32, f32)> {
        if self.caret == self.anchor {
            return None;
        }
        let region = self.focused_region()?;
        Some((region.x, region.y, region.width, region.height))
    }

    /// The action under `(fx, fy)` in logical px, if the toolbar is up and the
    /// point is on one of its buttons.
    fn toolbar_action_at(&self, fx: f32, fy: f32) -> Option<TextAction> {
        let field = self.toolbar_field()?;
        let (_, buttons) = toolbar_layout(field, self.logical_size());
        buttons
            .into_iter()
            .find(|(_, bx, by, bw, bh)| fx >= *bx && fx <= bx + bw && fy >= *by && fy <= by + bh)
            .map(|(action, ..)| action)
    }

    /// Whether the toolbar covers `(fx, fy)`, so a press there is not also a
    /// press on whatever is underneath. The same rule the dev overlay follows.
    fn toolbar_covers(&self, fx: f32, fy: f32) -> bool {
        let Some(field) = self.toolbar_field() else { return false };
        let ((x, y, w, h), _) = toolbar_layout(field, self.logical_size());
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
            && self.focused_row.as_deref() == region.row.as_deref();
        if focused && !region.multiline { self.text_scroll } else { 0.0 }
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

        // A tap also moves keyboard focus, so Tab continues from what you clicked.
        self.focus_index = self.focusables.iter().rposition(|f| f.contains(fx, fy));
        self.focused_multiline = region.multiline;

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
        self.set_focus(Some((region.model, region.row, caret)));
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
        let value = self.document.value_in(&region.model, region.row.as_deref());
        let (Some(t), false) = (&region.text, value.is_empty()) else {
            return false;
        };
        let (tx, ty) = self.text_point(&region, t, fx, fy);
        let (start, end) = self.text.word_at_point(
            &value,
            &rux_paint::text_style(&t.content),
            Some(t.width),
            tx,
            ty,
        );
        self.set_focus_range(Some(Focus {
            model: region.model,
            row: region.row,
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
        // A double-tap has already taken a word, so there is nothing pending.
        self.touch_text = Some(if self.anchor == self.caret {
            TouchText::Pending { at: pointer, deadline: Instant::now() + LONG_PRESS }
        } else {
            TouchText::Selecting
        });
        true
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
        let next = InteractionState {
            hovered,
            active,
            focused_model: self.document.interaction().focused_model.clone(),
            focused_row: self.document.interaction().focused_row.clone(),
        };
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
            height: (state.surface.config.height as f64 / scale) as f32,
        };
        if self.document.set_viewport(viewport) {
            self.request_redraw();
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

    /// Tell the document which input has focus, so `:focus` rules match it.
    fn update_focus_state(&mut self, model: Option<String>, row: Option<String>) {
        let mut next = self.document.interaction().clone();
        if next.focused_model == model && next.focused_row == row {
            return;
        }
        next.focused_model = model;
        next.focused_row = row;
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
        if let Some((model, row)) = self.open_select.take() {
            if let Some(sel) = self
                .selects
                .iter()
                .find(|s| s.model == model && s.row == row)
                .cloned()
            {
                for (i, option) in sel.options.iter().enumerate() {
                    let (rx, ry, rw, rh) = dropdown_row(&sel, i);
                    if fx >= rx && fx <= rx + rw && fy >= ry && fy <= ry + rh {
                        self.document.apply_edit_in(&model, row.as_deref(), option);
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
            self.open_select = Some((sel.model.clone(), sel.row.clone()));
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
            .find(|h| h.contains(px as f32, py as f32))
            .map(|h| (h.on_tap.clone(), h.instance.clone()));

        if let Some((src, instance)) = handler {
            // Patch in place when the change is display-only; rebuild only when it
            // touches structure/attributes/input values. Either way, repaint.
            //
            // The instance travels with the handler because two instances of one
            // component carry identical handler text: the string alone cannot
            // say whose state to run it against.
            if self.document.apply_handler_in(&src, instance.as_deref()) {
                self.request_redraw();
            }
            self.adopt_element_requests();
        }
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
            Key::Named(NamedKey::ArrowUp | NamedKey::ArrowDown) if self.focused_multiline => {
                if let Some(t) = self
                    .focused_region()
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
            self.scroll_caret_into_view(&value, new_caret);
            // Patch the input's value in place (no rebuild) unless `model` is also
            // structural; then set the caret on the resulting tree.
            if edited {
                self.write_focused(&value);
            }
            self.set_focus_range(Some(Focus {
                model,
                // Still the same field being typed into.
                row: self.focused_row.clone(),
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

    fn select_all_text(&mut self, model: &str) {
        let value = self.focused_value();
        self.set_focus_range(Some(Focus {
            model: model.to_string(),
            row: self.focused_row.clone(),
            caret: value.len(),
            anchor: 0,
            preedit: None,
        }));
    }

    fn copy_selection(&mut self) {
        if let Some(text) = self.selected_text() {
            self.clipboard_write(&text);
        }
    }

    fn cut_selection(&mut self, model: &str) {
        let Some(text) = self.selected_text() else { return };
        self.clipboard_write(&text);
        let value = self.focused_value();
        let (start, end) = self.selection();
        let mut value = value;
        value.replace_range(start.min(value.len())..end.min(value.len()), "");
        self.write_focused(&value);
        self.set_focus_range(Some(Focus::at(model, start)));
    }

    /// Ask for the clipboard's contents and paste them.
    ///
    /// Native reads it here and pastes immediately. The web cannot: the Clipboard
    /// API is a promise, and permission may even be prompted for, so the read is
    /// started here and the paste happens later, when [`RuxEvent::WebPaste`]
    /// arrives. Both ends meet in [`apply_paste`](Self::apply_paste).
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
        let pasted = if self.focused_multiline {
            pasted.replace("\r\n", "\n")
        } else {
            pasted.lines().next().unwrap_or("").to_string()
        };
        let value = self.focused_value();
        let (start, end) = self.selection();
        let mut value = value;
        let (start, end) = (start.min(value.len()), end.min(value.len()));
        value.replace_range(start..end, &pasted);
        let caret = start + pasted.len();
        self.write_focused(&value);
        self.scroll_caret_into_view(&value, caret);
        self.set_focus_range(Some(Focus::at(model, caret)));
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
            Some(FocusKind::Text { model, row, multiline, .. }) => {
                // Read against the field being moved *to*, not the one being
                // left: focus has not moved yet, so `focused_value` is still the
                // old field and Tab would drop the caret at its length.
                let caret = self.document.value_in(&model, row.as_deref()).len();
                self.focused_multiline = multiline;
                self.set_focus(Some((model, row, caret)));
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
            Some(FocusKind::Select { model, row, .. }) => {
                self.open_select = Some((model, row));
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
    fn set_focus(&mut self, focus: Option<(String, Option<String>, usize)>) {
        match focus {
            Some((model, row, caret)) => self.set_focus_range(Some(Focus::at_row(model, row, caret))),
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
        let Some(model) = self.focused.clone() else { return String::new() };
        let row = self.focused_row.clone();
        self.document.value_in(&model, row.as_deref())
    }

    /// Write the focused input's value back, in that same scope.
    fn write_focused(&mut self, value: &str) {
        let Some(model) = self.focused.clone() else { return };
        let row = self.focused_row.clone();
        self.document.apply_edit_in(&model, row.as_deref(), value);
    }

    /// The focused input's region, matched on both halves of its identity.
    fn focused_region(&self) -> Option<&FocusRegion> {
        let model = self.focused.as_deref()?;
        self.focuses
            .iter()
            .find(|f| f.model == model && f.row.as_deref() == self.focused_row.as_deref())
    }

    /// The full-fidelity focus setter: caret, selection anchor *and* composition.
    ///
    /// Any caller that is not the IME leaves `preedit` at `None`, which is taken
    /// as "whatever was being composed is abandoned": clicking into another
    /// field, tabbing away or pressing Escape mid-composition all put the field
    /// back the way it was, rather than stranding half-typed text nobody chose.
    fn set_focus_range(&mut self, focus: Option<Focus>) {
        if focus.as_ref().and_then(|f| f.preedit).is_none() {
            self.cancel_preedit();
        }
        // A different field starts unscrolled: the offset belongs to the text
        // being edited, and carrying it over would show the new field's value
        // already scrolled to somewhere the caret is not.
        let same_field = focus
            .as_ref()
            .is_some_and(|f| f.is(self.focused.as_deref().unwrap_or(""), self.focused_row.as_deref()));
        if !same_field {
            self.text_scroll = 0.0;
        }
        self.focused = focus.as_ref().map(|f| f.model.clone());
        self.focused_row = focus.as_ref().and_then(|f| f.row.clone());
        self.caret = focus.as_ref().map(|f| f.caret).unwrap_or(0);
        self.anchor = focus.as_ref().map(|f| f.anchor).unwrap_or(0);
        self.document.set_focus(focus);
        // `:focus` matches on the focused input, so the document needs both
        // halves of its identity, or every row of a list matches at once.
        let model = self.focused.clone();
        let row = self.focused_row.clone();
        self.update_focus_state(model, row);
        self.set_ime_enabled(self.focused.is_some());
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
        let Some(state) = self.state.as_ref() else { return };
        state.window.set_ime_allowed(on);
        if on {
            self.update_ime_area();
        }
        #[cfg(target_arch = "wasm32")]
        self.sync_web_ime();
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
        let Some(el) = web_ime_element() else { return };
        // Nothing focused means nothing to type into, so the keyboard goes away.
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
            let _ = el.set_selection_range_with_direction(start, end, direction);
        } else if el.selection_start().ok().flatten() != Some(start)
            || el.selection_end().ok().flatten() != Some(end)
        {
            // The text is unchanged but the selection moved on our side: a drag
            // across the canvas, a double-tap on a word, a handler selecting
            // all. The browser has to be told, because its own copy, cut and
            // select-all read the hidden input's selection and nothing else.
            // Leaving this out is what made copy on a phone act on no text.
            let _ = el.set_selection_range_with_direction(start, end, direction);
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
        let Some(el) = WEB_IME.with(|c| c.borrow().clone()) else { return };
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
        let value = if self.focused_multiline {
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
        self.write_focused(&value);
        self.scroll_caret_into_view(&value, caret);
        // The row travels with the model: an input inside an `r-for` is
        // identified by both, and dropping it here would put the caret in every
        // row of the list at once.
        let row = self.focused_row.clone();
        self.set_focus_range(Some(Focus { model, row, caret, anchor, preedit }));
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
        let (cx, cy, ch) = self.text.caret_geometry(&value, &style, Some(t.width), caret);
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
            self.write_focused(&value);
            self.set_focus_range(Some(Focus::at(model, caret)));
            return;
        }

        let caret = at + cursor.map(|(s, _)| s.min(text.len())).unwrap_or(text.len());
        self.preedit = Some(Preedit { at, len: text.len(), replaced: composing.replaced });
        self.write_focused(&value);
        self.scroll_caret_into_view(&value, caret);
        self.set_focus_range(Some(Focus {
            model,
            row: self.focused_row.clone(),
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
        let text = if self.focused_multiline {
            text.replace("\r\n", "\n")
        } else {
            text.lines().next().unwrap_or("").to_string()
        };
        value.replace_range(start..end, &text);
        let caret = start + text.len();
        self.write_focused(&value);
        self.scroll_caret_into_view(&value, caret);
        self.set_focus_range(Some(Focus::at(model, caret)));
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
        self.write_focused(&value);
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

    /// Put `text` on the system clipboard.
    #[cfg(not(target_arch = "wasm32"))]
    fn clipboard_write(&mut self, text: &str) {
        if let Some(cb) = self.clipboard.as_mut() {
            if let Err(e) = cb.set_text(text.to_string()) {
                eprintln!("rux: clipboard copy failed: {e}");
            }
        }
    }

    /// Read the system clipboard. `None` when it's empty, holds non-text, or
    /// there's no clipboard at all.
    #[cfg(not(target_arch = "wasm32"))]
    fn clipboard_read(&mut self) -> Option<String> {
        self.clipboard.as_mut()?.get_text().ok()
    }

    // On the web the clipboard is asynchronous and permission-gated. Writing can
    // be fired and forgotten; reading cannot, so it does not go through
    // `clipboard_read` at all: see `request_paste`.
    #[cfg(target_arch = "wasm32")]
    fn clipboard_write(&mut self, text: &str) {
        let Some(clipboard) = web_clipboard() else { return };
        // The promise is deliberately dropped. A rejection (no permission, not a
        // secure context) means the copy did not happen, and there is nothing
        // useful to do about it in a UI with no place to report it.
        let _ = clipboard.write_text(text);
    }

    #[cfg(target_arch = "wasm32")]
    fn clipboard_read(&mut self) -> Option<String> {
        // Unreachable in practice: `request_paste` takes the async path on the
        // web. Kept so the native and web shells present the same surface.
        None
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
        let next = self.anim.apply(&mut self.document.root, now);
        self.anim_deadline = next.map(|ms| {
            Instant::now() + Duration::from_secs_f64(ms.max(rux_runtime::FRAME_MS) / 1000.0)
        });
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
            states,
            metrics,
            overlay_dismissed,
            overlay_rect,
            caret,
            anchor,
            text_scroll,
            focused,
            focused_row,
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
        let logical = (width as f64 / scale, height as f64 / scale);

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

        // `scrollIntoView()`: nudge the containing scroller until the element is
        // inside it. Applied here because the offsets are the shell's, and taken
        // after `set_metrics` so a reveal asked for by the handler that just ran
        // is resolved against the frame it was looking at.
        let mut revealed = false;
        for path in document.take_reveals() {
            let Some(m) = layout.metrics.iter().find(|e| e.path == path) else {
                continue;
            };
            // The innermost scroller containing it. `scrolls` is in tree order,
            // so the last match is the deepest one, and that is the one whose
            // offset actually moves this element.
            let Some(region) = layout.scrolls.iter().rfind(|s| {
                m.x >= s.x && m.y >= s.y && m.x <= s.x + s.width && m.y <= s.y + s.height
            }) else {
                continue;
            };
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
            *caret,
            text_scroll,
            text,
            document,
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
            let within = item.scroll.and_then(|s| layout.scrolls.get(s));
            let ring = rux_paint::build_scene(&focus_ring(item, within), text, images, false);
            state.scene.append(&ring, Some(Affine::scale(scale)));
        }

        // The selection toolbar, over the content while something is selected.
        // It is the only route to copy and paste on a phone, and on the web at
        // all, so it is drawn above the page rather than inside it.
        if *caret != *anchor {
            if let Some(r) = focused.as_deref().and_then(|m| {
                layout
                    .focuses
                    .iter()
                    .find(|f| f.model == m && f.row.as_deref() == focused_row.as_deref())
            }) {
                let strip = toolbar_paints(
                    (r.x, r.y, r.width, r.height),
                    (logical.0 as f32, logical.1 as f32),
                );
                let scene = rux_paint::build_scene(&strip, text, images, false);
                state.scene.append(&scene, Some(Affine::scale(scale)));
            }
        }

        // An open `select` draws its dropdown on top of everything else.
        if let Some((model, row)) = open_select.clone() {
            if let Some(sel) = layout.selects.iter().find(|s| s.model == model && s.row == row) {
                let value = document.value_in(&model, row.as_deref());
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
        #[cfg(not(target_arch = "wasm32"))]
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
        // page you just moved to. Identity is `(model, row)`, the same pair the
        // caret uses, or one row of a list would answer for another.
        if let Some(model) = focused.clone() {
            let still_here = focuses
                .iter()
                .any(|f| f.model == model && f.row.as_deref() == focused_row.as_deref());
            if !still_here {
                *focused = None;
                *focused_row = None;
                *text_scroll = 0.0;
                #[cfg(target_arch = "wasm32")]
                if let Some(el) = web_ime_element() {
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
        let attributes = Window::default_attributes()
            .with_title(title)
            .with_visible(false)
            .with_inner_size(winit::dpi::LogicalSize::new(420.0, 640.0));
        let window = Arc::new(event_loop.create_window(attributes).expect("create window"));
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

        let renderer = make_renderer(&self.context, &surface);
        self.state = Some(RenderState {
            window,
            surface,
            renderer,
            scene: Scene::new(),
            access,
        });
        self.request_redraw();
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
            #[cfg(not(target_arch = "wasm32"))]
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

            #[cfg(target_arch = "wasm32")]
            RuxEvent::WebRoute(index) => self.apply_web_route(index),

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

            #[cfg(not(target_arch = "wasm32"))]
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
        #[cfg(not(target_arch = "wasm32"))]
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
                        // There is no hover on a touchscreen, so the pointer only
                        // exists while a finger is down and has to be set here.
                        // Every helper below reads it.
                        self.pointer = at;
                        self.touch = Some(here);
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
                            self.touch_text = Some(next);
                            match next {
                                TouchText::Selecting => self.drag_text(at),
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
                        if self.bar_drag.take().is_some() {
                            return;
                        }
                        if std::mem::take(&mut self.text_drag) {
                            return;
                        }
                        // A finger lifting off text has already had its effect,
                        // whichever gesture it turned out to be, and must not
                        // also reach the app as a tap.
                        if self.touch_text.take().is_some() {
                            return;
                        }
                        // A finger wanders more than a mouse, but the slop that
                        // separates a tap from a drag is the same idea.
                        if let Some((sx, sy)) = self.press.take() {
                            if (at.0 - sx).hypot(at.1 - sy) <= TAP_SLOP {
                                self.dispatch_tap(at.0, at.1);
                            }
                        }
                    }
                    TouchPhase::Cancelled => {
                        self.touch = None;
                        self.press = None;
                        self.bar_drag = None;
                        self.text_drag = false;
                        // Dropping this also disarms a pending long press, so a
                        // cancelled touch cannot select a word after the fact.
                        self.touch_text = None;
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
                if event.state == ElementState::Pressed && self.preedit.is_none() {
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
            }
            // Event-driven: we only paint in response to a redraw request, which
            // is issued on resume, resize, reload, and tap, not every frame.
            WindowEvent::RedrawRequested => {
                self.render();
                // A `tap()` or `focus()` asked for by something that is not an
                // input event has nowhere else to be picked up: the only other
                // drain runs after a handler the shell itself dispatched. A
                // `mounted` hook that focuses a field at startup queued its
                // request before the window had ever seen an event, and it sat
                // in the queue forever. Here rather than at load, because both
                // requests are answered against a laid-out frame: before the
                // first layout there are no focusables to focus and no hit
                // regions to tap.
                self.adopt_element_requests();
            }
            _ => {}
        }
    }

    /// The only clock in an otherwise event-driven loop: while an input is
    /// focused, wake every `BLINK` to toggle the caret. With no focus the
    /// deadline is `None`, so we wait indefinitely for the next real event.
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // A resting finger is the second clock, and the reason this is not just
        // the blink any more: nothing arrives to say a press has gone on long
        // enough, so the deadline has to be waited on and checked here.
        if let Some(TouchText::Pending { at, deadline }) = self.touch_text {
            if Instant::now() >= deadline {
                // Whether or not a word was there to take, the press has
                // resolved: it must not stay pending and fire again later.
                self.touch_text = Some(TouchText::Selecting);
                if self.select_word_at(at) {
                    self.request_redraw();
                }
            }
        }

        if let Some(deadline) = self.blink_deadline {
            if Instant::now() >= deadline {
                self.caret_visible = !self.caret_visible;
                self.blink_deadline = Some(Instant::now() + BLINK);
                self.request_redraw();
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

        // Wake for whichever clock is due first. With none running, wait
        // indefinitely for a real event, as before.
        let long_press = match self.touch_text {
            Some(TouchText::Pending { deadline, .. }) => Some(deadline),
            _ => None,
        };
        match [self.blink_deadline, long_press, self.anim_deadline].into_iter().flatten().min() {
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
    static WEB_IME: RefCell<Option<web_sys::HtmlInputElement>> = const { RefCell::new(None) };
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

/// The hidden input, created and wired on first use.
#[cfg(target_arch = "wasm32")]
fn web_ime_element() -> Option<web_sys::HtmlInputElement> {
    use wasm_bindgen::JsCast;
    use wasm_bindgen::prelude::Closure;

    if let Some(el) = WEB_IME.with(|c| c.borrow().clone()) {
        return Some(el);
    }
    let canvas = WEB_CANVAS.with(|c| c.borrow().clone())?;
    let document = web_sys::window()?.document()?;
    let el: web_sys::HtmlInputElement =
        document.create_element("input").ok()?.dyn_into().ok()?;

    el.set_type("text");
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
        if let Some(target) = event.target().and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok()) {
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
            if let Some(target) =
                event.target().and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok())
            {
                web_send_text(&target);
            }
        },
    );
    for name in ["compositionstart", "compositionupdate", "compositionend"] {
        let _ = el.add_event_listener_with_callback(name, on_comp.as_ref().unchecked_ref());
    }
    on_comp.forget();

    WEB_IME.with(|c| *c.borrow_mut() = Some(el.clone()));
    Some(el)
}

/// Push the hidden input's contents at the event loop.
#[cfg(target_arch = "wasm32")]
fn web_send_text(el: &web_sys::HtmlInputElement) {
    let value = el.value();
    // `selection_start` is in UTF-16 code units, which is not where Rux counts
    // from: it indexes strings by byte. Converting through the prefix keeps a
    // caret after an emoji or a CJK character in the right place instead of
    // several bytes short.
    let start16 = el.selection_start().ok().flatten().unwrap_or(0) as usize;
    let end16 = el.selection_end().ok().flatten().map_or(start16, |v| v as usize);
    // `selectionStart`/`End` are ordered, so on their own they cannot say which
    // end the caret is at. `selectionDirection` is what distinguishes a
    // selection dragged leftwards from the same range dragged rightwards, and
    // getting it wrong makes Shift+arrow extend from the wrong end afterwards.
    let backward = el.selection_direction().ok().flatten().as_deref() == Some("backward");
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
#[cfg(any(target_arch = "wasm32", test))]
fn byte_to_utf16_index(s: &str, byte: usize) -> usize {
    s[..floor_char_boundary(s, byte)].chars().map(char::len_utf16).sum()
}

/// Round `index` down to a character boundary, so a caret that arrives inside a
/// character is pulled back to its start rather than left to panic a later slice.
#[cfg(any(target_arch = "wasm32", test))]
fn floor_char_boundary(s: &str, mut index: usize) -> usize {
    index = index.min(s.len());
    while index > 0 && !s.is_char_boundary(index) {
        index -= 1;
    }
    index
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

    /// The painter draws the toolbar from this and the hit test reads it, so a
    /// button's box must be exactly where it is painted, and the strip must stay
    /// on screen for a field at either edge.
    #[test]
    fn the_toolbar_sits_where_its_buttons_are_hit() {
        let viewport = (400.0, 800.0);
        let ((x, y, w, h), buttons) = toolbar_layout((20.0, 300.0, 200.0, 40.0), viewport);

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
        let ((_, below_y, _, _), _) = toolbar_layout((20.0, 0.0, 200.0, 40.0), viewport);
        assert!(below_y >= 40.0, "drops below the field instead: {below_y}");

        // A field against the right edge must not push the strip off screen.
        let ((right_x, _, right_w, _), _) = toolbar_layout((380.0, 300.0, 200.0, 40.0), viewport);
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
            document.start_at(&route);
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
    if !app.text.register_font(font) {
        web_sys::console::error_1(&"rux: the supplied font had no usable faces, so text will not render".into());
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

/// Open the Rux window for the given `.rux` file and run the frame loop until the
/// window closes. Watches the file and repaints on change.
///
/// Native only: it takes a filesystem path and installs a file watcher, neither
/// of which a browser has. The web build drives the same `App` from source text
/// supplied by the playground editor.
#[cfg(not(target_arch = "wasm32"))]
pub fn run(path: PathBuf) {
    run_at(path, None)
}

/// The same, opening the document on `route` instead of on `/`.
///
/// This is a deep link arriving on the desktop, and it is what makes one
/// testable at all: a page reached only by tapping through the app cannot be
/// checked on its own, and on a phone the same call is what an `myapp://` URL
/// eventually turns into.
#[cfg(not(target_arch = "wasm32"))]
pub fn run_at(path: PathBuf, route: Option<String>) {
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
    // Before the first frame, and before the watcher can reload: `start_at`
    // replaces the history, so it has to be the first thing that touches it.
    if let Some(route) = route {
        app.document.start_at(&route);
    }
    event_loop.run_app(&mut app).expect("run app");

    drop(watcher); // keep the watcher alive for the loop's lifetime
}

#[cfg(test)]
mod tests {
    use super::*;
    use rux_runtime::{Diagnostics, Warning};

    fn warned(message: &str) -> Diagnostics {
        Diagnostics { warnings: vec![Warning::new(message)], ..Diagnostics::default() }
    }

    fn focusable(y: f32, scroll: Option<usize>) -> FocusItem {
        FocusItem {
            x: 40.0,
            y,
            width: 200.0,
            height: 50.0,
            kind: FocusKind::Activate { on_tap: String::new(), instance: None },
            scroll,
        }
    }

    fn scroller() -> ScrollRegion {
        ScrollRegion {
            id: 0,
            x: 30.0,
            y: 100.0,
            width: 220.0,
            height: 220.0,
            content_width: 220.0,
            content_height: 600.0,
            max: Offset { x: 0.0, y: 380.0 },
        }
    }

    /// Outside a scroller there is nothing to clip against, so the ring is one
    /// plain rectangle, as it always was.
    #[test]
    fn a_focus_ring_outside_a_scroller_is_unclipped() {
        assert_eq!(focus_ring(&focusable(150.0, None), None).len(), 1);
    }

    /// The ring is painted as its own scene after the document's, so it never
    /// passes through the clip a scroller puts around its children. It has to
    /// carry its own, or a ring on a row scrolled up out of a list draws over
    /// whatever sits above the list. That is a real defect, seen in
    /// `examples/router.rux`: the crew list drew a ring over the paragraph
    /// above it.
    #[test]
    fn a_focus_ring_inside_a_scroller_is_clipped_to_it() {
        let paints = focus_ring(&focusable(150.0, Some(0)), Some(&scroller()));
        assert_eq!(paints.len(), 3, "a clip, the ring, and the matching pop");
        assert!(matches!(paints[0], Paint::PushClip { .. }), "{:?}", paints[0]);
        assert!(matches!(paints[2], Paint::PopClip), "{:?}", paints[2]);
    }

    /// Scrolled fully out of view it draws nothing at all. A ring clipped to a
    /// sliver at the container's edge reads as a rendering fault rather than as
    /// a focused element that happens to be off-screen.
    #[test]
    fn a_focus_ring_scrolled_out_of_view_is_not_drawn() {
        let above = focus_ring(&focusable(-90.0, Some(0)), Some(&scroller()));
        assert!(above.is_empty(), "scrolled off the top: {above:?}");
        let below = focus_ring(&focusable(400.0, Some(0)), Some(&scroller()));
        assert!(below.is_empty(), "scrolled off the bottom: {below:?}");
        // And one straddling the edge is still drawn, clipped.
        let edge = focus_ring(&focusable(90.0, Some(0)), Some(&scroller()));
        assert_eq!(edge.len(), 3, "partly visible, so still drawn: {edge:?}");
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
