//! `[window] title-bar = "overlay"`: the system's frame stays, and the app's
//! content extends up into the title row beside the window buttons.
//!
//! **Windows.** The window is opened decorated, so it keeps its border, its
//! shadow, its rounded corners, its resize edges and its snap behaviour. A
//! subclass on its window procedure then does the two things winit has no hook
//! for: `WM_NCCALCSIZE` gives the caption's height back to the client area, and
//! `WM_NCHITTEST` says where the buttons and the top resize edge are.
//!
//! The system cannot draw its buttons over a GPU surface, so the shell draws
//! them, in the system's look (46 by 32, its glyphs, a red close on hover), and
//! answers `HTMAXBUTTON` over the middle one, which is what makes Windows 11
//! offer its snap layouts there. Every press and release stays in this file;
//! the page never sees them.
//!
//! **macOS** keeps the real traffic lights (the window is made full-size under
//! a transparent title bar), so nothing is drawn: only the area is reported.
//!
//! The pure half (where the buttons are, which one a point is over, what they
//! paint) is plain code with tests. The window procedure is the other half.

use rux_layout::{
    Background, Corners, LineCap, LineJoin, Paint, PaintPath, PaintRect, PathContent, PathPaint, Rgba, Sides,
};

/// A button's width and the strip's height, in logical px: Windows 11's.
pub(crate) const BUTTON_WIDTH: f32 = 46.0;
pub(crate) const HEIGHT: f32 = 32.0;
/// The three buttons together.
pub(crate) const BUTTONS_WIDTH: f32 = BUTTON_WIDTH * 3.0;
/// What a macOS title bar leaves for the traffic lights, and its height.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) const MAC_LIGHTS_WIDTH: f32 = 78.0;
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) const MAC_HEIGHT: f32 = 28.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Button {
    Minimize,
    Maximize,
    Close,
}

/// What the buttons are showing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct State {
    pub hover: Option<Button>,
    pub pressed: Option<Button>,
    /// Whether the window is the active one: its glyphs dim when it is not.
    pub active: bool,
    pub maximized: bool,
}

impl Default for State {
    fn default() -> Self {
        Self { hover: None, pressed: None, active: true, maximized: false }
    }
}

/// A button's rectangle in logical px, `(x, y, width, height)`, in a window
/// `width` wide: Minimize, Maximize, Close from the left, the last at the edge.
pub(crate) fn rect(width: f32, button: Button) -> (f32, f32, f32, f32) {
    let slot = match button {
        Button::Minimize => 3.0,
        Button::Maximize => 2.0,
        Button::Close => 1.0,
    };
    (width - BUTTON_WIDTH * slot, 0.0, BUTTON_WIDTH, HEIGHT)
}

/// The button a point is over, in logical px.
pub(crate) fn hit(width: f32, x: f32, y: f32) -> Option<Button> {
    if !(0.0..HEIGHT).contains(&y) {
        return None;
    }
    [Button::Minimize, Button::Maximize, Button::Close].into_iter().find(|b| {
        let (left, _, w, _) = rect(width, *b);
        x >= left && x < left + w
    })
}

/// The row the app may use, for `env(titlebar-area-*)`: everything left of the
/// buttons.
pub(crate) fn area(width: f32) -> rux_runtime::Area {
    rux_runtime::Area { x: 0.0, y: 0.0, width: (width - BUTTONS_WIDTH).max(0.0), height: HEIGHT }
}

/// Whether the page is dark behind the buttons, so they are drawn light: the
/// colour of the topmost opaque box under the middle button, and the page's own
/// colour scheme (`fallback`) where no box says. The app has no way to say what
/// its title row looks like, and a dark bar on a light page is the usual case.
pub(crate) fn behind_is_dark(paints: &[Paint], width: f32, fallback: bool) -> bool {
    let (x, y) = (width - BUTTON_WIDTH * 1.5, HEIGHT / 2.0);
    let under = paints.iter().rev().find_map(|p| match p {
        Paint::Rect(r) if x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height => match r.background {
            Some(Background::Color(c)) if c.a >= 0.5 => Some(c),
            _ => None,
        },
        _ => None,
    });
    match under {
        Some(c) => 0.299 * c.r + 0.587 * c.g + 0.114 * c.b < 0.5,
        None => fallback,
    }
}

/// The buttons as paints, over the page.
pub(crate) fn paints(width: f32, state: State, dark: bool) -> Vec<Paint> {
    let mut out = Vec::new();
    for button in [Button::Minimize, Button::Maximize, Button::Close] {
        let (x, y, w, h) = rect(width, button);
        // Lit only while the pointer is over it, and while one is held no
        // other one lights.
        let lit = state.hover == Some(button) && (state.pressed.is_none() || state.pressed == Some(button));
        let held = lit && state.pressed == Some(button);
        let close = button == Button::Close;
        let wash = match (lit, close, held, dark) {
            (false, ..) => None,
            (true, true, false, _) => Some(Rgba::new(0.769, 0.169, 0.110, 1.0)),
            (true, true, true, _) => Some(Rgba::new(0.769, 0.169, 0.110, 0.9)),
            (true, false, false, false) => Some(Rgba::new(0.0, 0.0, 0.0, 0.0627)),
            (true, false, true, false) => Some(Rgba::new(0.0, 0.0, 0.0, 0.0373)),
            (true, false, false, true) => Some(Rgba::new(1.0, 1.0, 1.0, 0.0605)),
            (true, false, true, true) => Some(Rgba::new(1.0, 1.0, 1.0, 0.0419)),
        };
        if let Some(color) = wash {
            out.push(Paint::Rect(PaintRect {
                x,
                y,
                width: w,
                height: h,
                background: Some(Background::Color(color)),
                radius: Corners::default(),
                radius_y: None,
                border: Sides::ZERO,
                border_color: None,
            }));
        }
        let ink = if close && lit {
            Rgba::new(1.0, 1.0, 1.0, 1.0)
        } else {
            let base = if dark { 1.0 } else { 0.0 };
            Rgba::new(base, base, base, if state.active { 1.0 } else { 0.4 })
        };
        // A 10 by 10 glyph, centred, on whole pixels so a one pixel line is
        // crisp at 100%.
        let gx = x + ((w - 10.0) / 2.0).floor();
        let gy = y + ((h - 10.0) / 2.0).floor();
        for d in glyph(button, state.maximized) {
            out.push(Paint::Path(PaintPath {
                x: gx,
                y: gy,
                content: PathContent::parse(d),
                paint: PathPaint {
                    fill: None,
                    fill_rule: Default::default(),
                    stroke: Some(ink),
                    stroke_width: 1.0,
                    cap: LineCap::Butt,
                    join: LineJoin::Miter,
                },
            }));
        }
    }
    out
}

/// A glyph's strokes, in a 10 by 10 box.
fn glyph(button: Button, maximized: bool) -> &'static [&'static str] {
    match (button, maximized) {
        (Button::Minimize, _) => &["M0 5.5H10"],
        (Button::Maximize, false) => &["M0.5 0.5H9.5V9.5H0.5Z"],
        // The front square, and the back one showing as an L behind it.
        (Button::Maximize, true) => &["M0.5 2.5H7.5V9.5H0.5Z", "M2.5 2.5V0.5H9.5V7.5H7.5"],
        (Button::Close, _) => &["M0 0L10 10", "M10 0L0 10"],
    }
}

#[cfg(windows)]
pub(crate) use win::{attach, set_active, set_metrics, state};

/// Nothing to attach elsewhere: only Windows draws its own buttons.
#[cfg(not(windows))]
pub(crate) fn set_metrics(_window: &winit::window::Window, _scale: f64) {}
#[cfg(not(windows))]
pub(crate) fn set_active(_window: &winit::window::Window, _active: bool) {}

#[cfg(windows)]
mod win {
    use super::{hit, Button, State};
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::sync::Arc;
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use winit::window::Window;

    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
    use windows_sys::Win32::Graphics::Gdi::ScreenToClient;
    use windows_sys::Win32::UI::HiDpi::{GetDpiForWindow, GetSystemMetricsForDpi};
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        ReleaseCapture, SetCapture, TrackMouseEvent, TME_LEAVE, TME_NONCLIENT, TRACKMOUSEEVENT,
    };
    use windows_sys::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetClientRect, GetWindowRect, IsZoomed, PostMessageW, SetWindowPos, HTCLIENT, HTCLOSE, HTMAXBUTTON,
        HTMINBUTTON, HTTOP, HTTOPLEFT, HTTOPRIGHT, NCCALCSIZE_PARAMS, SC_MAXIMIZE, SC_MINIMIZE, SC_RESTORE,
        SM_CXPADDEDBORDER, SM_CYSIZEFRAME, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
        SWP_NOZORDER, WM_CAPTURECHANGED, WM_CLOSE, WM_LBUTTONUP, WM_MOUSEMOVE, WM_NCCALCSIZE, WM_NCDESTROY,
        WM_NCHITTEST, WM_NCLBUTTONDBLCLK, WM_NCLBUTTONDOWN, WM_NCMOUSELEAVE, WM_NCMOUSEMOVE, WM_SYSCOMMAND,
    };

    /// What the window procedure needs from the shell, and what it tells back.
    /// Kept per window (by its `HWND`) since an app may have several: the
    /// procedure runs on the thread that made the window.
    #[derive(Clone, Copy)]
    struct Shared {
        /// Physical px per logical px.
        scale: f64,
        state: State,
    }

    thread_local! {
        static SHARED: RefCell<HashMap<isize, Shared>> = RefCell::new(HashMap::new());
        static WINDOWS: RefCell<HashMap<isize, Arc<Window>>> = RefCell::new(HashMap::new());
    }

    const FRESH: Shared = Shared { scale: 1.0, state: State { hover: None, pressed: None, active: true, maximized: false } };

    fn shared(hwnd: isize) -> Shared {
        SHARED.with(|s| s.borrow().get(&hwnd).copied().unwrap_or(FRESH))
    }

    /// The window's handle, which is what its state is kept under.
    fn key(window: &Window) -> Option<isize> {
        let RawWindowHandle::Win32(h) = window.window_handle().ok()?.as_raw() else { return None };
        Some(h.hwnd.get())
    }

    const ID: usize = 0x7275_7863; // "ruxc"

    /// Tell the procedure the window's scale (physical px per logical px).
    pub(crate) fn set_metrics(window: &Window, scale: f64) {
        if let Some(hwnd) = key(window) {
            SHARED.with(|s| s.borrow_mut().entry(hwnd).or_insert(FRESH).scale = scale);
        }
    }

    pub(crate) fn set_active(window: &Window, active: bool) {
        if let Some(hwnd) = key(window) {
            SHARED.with(|s| s.borrow_mut().entry(hwnd).or_insert(FRESH).state.active = active);
            redraw(hwnd);
        }
    }

    /// What the buttons are showing now (`maximized` is the caller's to fill).
    pub(crate) fn state(window: &Window) -> State {
        key(window).map_or(FRESH.state, |hwnd| shared(hwnd).state)
    }

    fn update(hwnd: isize, change: impl FnOnce(&mut State)) {
        let mut v = shared(hwnd);
        let before = v.state;
        change(&mut v.state);
        SHARED.with(|s| s.borrow_mut().insert(hwnd, v));
        if v.state != before {
            redraw(hwnd);
        }
    }

    fn redraw(hwnd: isize) {
        WINDOWS.with(|w| {
            if let Some(w) = w.borrow().get(&hwnd) {
                w.request_redraw();
            }
        });
    }

    /// Give the window's client area its caption back, and take over the
    /// buttons. Before the window is shown, so it never opens with a bar.
    /// The client area keeps the size it was opened with.
    pub(crate) fn attach(window: &Arc<Window>) {
        let Ok(handle) = window.window_handle() else { return };
        let RawWindowHandle::Win32(h) = handle.as_raw() else { return };
        let hwnd = h.hwnd.get() as HWND;
        let kept = window.inner_size();
        WINDOWS.with(|w| w.borrow_mut().insert(h.hwnd.get(), window.clone()));
        SHARED.with(|s| s.borrow_mut().entry(h.hwnd.get()).or_insert(FRESH).scale = window.scale_factor());
        // SAFETY: `hwnd` is this window's, on its own thread; the procedure
        // reads only thread-locals.
        unsafe {
            if SetWindowSubclass(hwnd, Some(proc_), ID, 0) == 0 {
                return;
            }
            SetWindowPos(
                hwnd,
                std::ptr::null_mut(),
                0,
                0,
                0,
                0,
                SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
            );
            // The frame change gave the client area the caption's height, and
            // the window kept its outer size. Grow the window by the
            // difference so the app has the size it asked for, below the row.
            if IsZoomed(hwnd) == 0 {
                let mut client: RECT = std::mem::zeroed();
                let mut outer: RECT = std::mem::zeroed();
                GetClientRect(hwnd, &mut client);
                GetWindowRect(hwnd, &mut outer);
                let grew = (client.bottom - client.top) - kept.height as i32;
                if grew != 0 {
                    SetWindowPos(
                        hwnd,
                        std::ptr::null_mut(),
                        0,
                        0,
                        outer.right - outer.left,
                        outer.bottom - outer.top - grew,
                        SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
                    );
                }
            }
        }
    }

    fn lo(l: LPARAM) -> i32 {
        (l & 0xffff) as i16 as i32
    }
    fn hi(l: LPARAM) -> i32 {
        ((l >> 16) & 0xffff) as i16 as i32
    }

    /// A client-area point in logical px, and the client's logical width.
    unsafe fn logical(hwnd: HWND, px: i32, py: i32) -> (f32, f32, f32) {
        let scale = shared(hwnd as isize).scale as f32;
        let mut rc: RECT = std::mem::zeroed();
        GetClientRect(hwnd, &mut rc);
        (px as f32 / scale, py as f32 / scale, rc.right as f32 / scale)
    }

    /// The button a screen point (a non-client message's `lparam`) is over.
    unsafe fn button_at_screen(hwnd: HWND, l: LPARAM) -> Option<Button> {
        let mut pt = POINT { x: lo(l), y: hi(l) };
        ScreenToClient(hwnd, &mut pt);
        let (x, y, w) = logical(hwnd, pt.x, pt.y);
        hit(w, x, y)
    }

    unsafe fn button_at_client(hwnd: HWND, l: LPARAM) -> Option<Button> {
        let (x, y, w) = logical(hwnd, lo(l), hi(l));
        hit(w, x, y)
    }

    fn state_of(hwnd: HWND) -> State {
        shared(hwnd as isize).state
    }

    unsafe fn frame_y(hwnd: HWND) -> i32 {
        let dpi = GetDpiForWindow(hwnd);
        GetSystemMetricsForDpi(SM_CYSIZEFRAME, dpi) + GetSystemMetricsForDpi(SM_CXPADDEDBORDER, dpi)
    }

    unsafe extern "system" fn proc_(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        _id: usize,
        _data: usize,
    ) -> LRESULT {
        match msg {
            WM_NCCALCSIZE if wparam != 0 => {
                let params = &mut *(lparam as *mut NCCALCSIZE_PARAMS);
                let top = params.rgrc[0].top;
                let result = DefSubclassProc(hwnd, msg, wparam, lparam);
                if result != 0 {
                    return result;
                }
                // Everything the system worked out stays but the caption: the
                // top goes back to where the window starts. A maximized
                // window overhangs the screen by its frame, so it starts
                // below that overhang instead.
                params.rgrc[0].top = if IsZoomed(hwnd) != 0 { top + frame_y(hwnd) } else { top };
                0
            }
            WM_NCHITTEST => {
                let result = DefSubclassProc(hwnd, msg, wparam, lparam);
                if result != HTCLIENT as LRESULT {
                    return result;
                }
                let mut pt = POINT { x: lo(lparam), y: hi(lparam) };
                ScreenToClient(hwnd, &mut pt);
                let (x, y, w) = logical(hwnd, pt.x, pt.y);
                match hit(w, x, y) {
                    Some(Button::Minimize) => return HTMINBUTTON as LRESULT,
                    Some(Button::Maximize) => return HTMAXBUTTON as LRESULT,
                    Some(Button::Close) => return HTCLOSE as LRESULT,
                    None => {}
                }
                // The top edge: the frame's resize border lost its place when
                // the caption went, so it is answered here.
                if IsZoomed(hwnd) == 0 && pt.y < frame_y(hwnd) / 2 {
                    // Half the frame: the other half is the row's own.
                    let edge = frame_y(hwnd);
                    let mut rc: RECT = std::mem::zeroed();
                    GetClientRect(hwnd, &mut rc);
                    return if pt.x < edge {
                        HTTOPLEFT
                    } else if pt.x >= rc.right - edge {
                        HTTOPRIGHT
                    } else {
                        HTTOP
                    } as LRESULT;
                }
                HTCLIENT as LRESULT
            }
            WM_NCMOUSEMOVE => {
                let over = button_at_screen(hwnd, lparam);
                update(hwnd as isize, |s| s.hover = over);
                if over.is_some() {
                    let mut track = TRACKMOUSEEVENT {
                        cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                        dwFlags: TME_LEAVE | TME_NONCLIENT,
                        hwndTrack: hwnd,
                        dwHoverTime: 0,
                    };
                    TrackMouseEvent(&mut track);
                }
                DefSubclassProc(hwnd, msg, wparam, lparam)
            }
            WM_NCMOUSELEAVE => {
                update(hwnd as isize, |s| s.hover = None);
                DefSubclassProc(hwnd, msg, wparam, lparam)
            }
            WM_NCLBUTTONDOWN | WM_NCLBUTTONDBLCLK
                if matches!(wparam as u32, HTMINBUTTON | HTMAXBUTTON | HTCLOSE) =>
            {
                let button = button_at_screen(hwnd, lparam);
                update(hwnd as isize, |s| {
                    s.pressed = button;
                    s.hover = button;
                });
                SetCapture(hwnd);
                0
            }
            // While a button is held the mouse is captured, so its moves and
            // its release arrive as client messages, and are the buttons'.
            WM_MOUSEMOVE if state_of(hwnd).pressed.is_some() => {
                let over = button_at_client(hwnd, lparam);
                update(hwnd as isize, |s| s.hover = over);
                0
            }
            WM_MOUSEMOVE => {
                if state_of(hwnd).hover.is_some() {
                    update(hwnd as isize, |s| s.hover = None);
                }
                DefSubclassProc(hwnd, msg, wparam, lparam)
            }
            WM_LBUTTONUP if state_of(hwnd).pressed.is_some() => {
                let held = state_of(hwnd).pressed;
                let over = button_at_client(hwnd, lparam);
                update(hwnd as isize, |s| {
                    s.pressed = None;
                    s.hover = over;
                });
                ReleaseCapture();
                if held.is_some() && held == over {
                    let action = match held {
                        Some(Button::Minimize) => Some((WM_SYSCOMMAND, SC_MINIMIZE as WPARAM)),
                        Some(Button::Maximize) => {
                            let command = if IsZoomed(hwnd) != 0 { SC_RESTORE } else { SC_MAXIMIZE };
                            Some((WM_SYSCOMMAND, command as WPARAM))
                        }
                        Some(Button::Close) => Some((WM_CLOSE, 0)),
                        None => None,
                    };
                    if let Some((message, w)) = action {
                        PostMessageW(hwnd, message, w, 0);
                    }
                }
                0
            }
            WM_CAPTURECHANGED => {
                if state_of(hwnd).pressed.is_some() {
                    update(hwnd as isize, |s| s.pressed = None);
                }
                DefSubclassProc(hwnd, msg, wparam, lparam)
            }
            WM_NCDESTROY => {
                WINDOWS.with(|w| w.borrow_mut().remove(&(hwnd as isize)));
                SHARED.with(|s| s.borrow_mut().remove(&(hwnd as isize)));
                RemoveWindowSubclass(hwnd, Some(proc_), ID);
                DefSubclassProc(hwnd, msg, wparam, lparam)
            }
            _ => DefSubclassProc(hwnd, msg, wparam, lparam),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_buttons_sit_at_the_right_edge_in_order() {
        assert_eq!(rect(800.0, Button::Close), (754.0, 0.0, 46.0, 32.0));
        assert_eq!(rect(800.0, Button::Maximize).0, 708.0);
        assert_eq!(rect(800.0, Button::Minimize).0, 662.0);
    }

    #[test]
    fn a_point_is_over_the_button_under_it_or_none() {
        assert_eq!(hit(800.0, 780.0, 10.0), Some(Button::Close));
        assert_eq!(hit(800.0, 710.0, 0.0), Some(Button::Maximize));
        assert_eq!(hit(800.0, 662.0, 31.9), Some(Button::Minimize));
        assert_eq!(hit(800.0, 661.9, 10.0), None, "left of the strip");
        assert_eq!(hit(800.0, 780.0, 32.0), None, "below the strip");
        assert_eq!(hit(800.0, 780.0, -1.0), None, "above the window");
    }

    #[test]
    fn the_title_area_is_everything_left_of_the_buttons() {
        let a = area(800.0);
        assert_eq!((a.x, a.y, a.width, a.height), (0.0, 0.0, 662.0, 32.0));
        assert_eq!(area(100.0).width, 0.0, "a window narrower than the buttons has no area, not a negative one");
    }

    fn colors(paints: &[Paint]) -> Vec<Rgba> {
        paints
            .iter()
            .filter_map(|p| match p {
                Paint::Rect(r) => match r.background {
                    Some(Background::Color(c)) => Some(c),
                    _ => None,
                },
                _ => None,
            })
            .collect()
    }

    #[test]
    fn at_rest_there_is_a_glyph_and_no_wash() {
        let p = paints(800.0, State::default(), false);
        assert!(colors(&p).is_empty());
        // minimize 1, maximize 1, close 2
        assert_eq!(p.iter().filter(|p| matches!(p, Paint::Path(_))).count(), 4);
    }

    #[test]
    fn a_maximized_window_shows_the_restore_glyph() {
        let p = paints(800.0, State { maximized: true, ..State::default() }, false);
        assert_eq!(p.iter().filter(|p| matches!(p, Paint::Path(_))).count(), 5);
    }

    #[test]
    fn hover_washes_one_button_and_close_goes_red() {
        let over = |b| State { hover: Some(b), ..State::default() };
        let light = colors(&paints(800.0, over(Button::Maximize), false));
        assert_eq!(light, vec![Rgba::new(0.0, 0.0, 0.0, 0.0627)]);
        let dark = colors(&paints(800.0, over(Button::Maximize), true));
        assert_eq!(dark, vec![Rgba::new(1.0, 1.0, 1.0, 0.0605)]);
        let close = colors(&paints(800.0, over(Button::Close), false));
        assert_eq!(close, vec![Rgba::new(0.769, 0.169, 0.110, 1.0)]);
    }

    #[test]
    fn a_held_button_stays_lit_only_while_the_pointer_is_over_it() {
        let held = State { pressed: Some(Button::Minimize), hover: Some(Button::Minimize), ..State::default() };
        assert_eq!(colors(&paints(800.0, held, false)).len(), 1);
        let strayed = State { pressed: Some(Button::Minimize), hover: Some(Button::Close), ..State::default() };
        assert!(colors(&paints(800.0, strayed, false)).is_empty(), "no other button lights while one is held");
        let off = State { pressed: Some(Button::Minimize), hover: None, ..State::default() };
        assert!(colors(&paints(800.0, off, false)).is_empty());
    }

    #[test]
    fn the_glyphs_follow_the_colour_behind_them() {
        let bar = |c: Rgba| {
            vec![Paint::Rect(PaintRect {
                x: 0.0,
                y: 0.0,
                width: 800.0,
                height: 40.0,
                background: Some(Background::Color(c)),
                radius: Corners::default(),
                radius_y: None,
                border: Sides::ZERO,
                border_color: None,
            })]
        };
        assert!(behind_is_dark(&bar(Rgba::new(0.17, 0.18, 0.23, 1.0)), 800.0, false), "a dark bar on a light page");
        assert!(!behind_is_dark(&bar(Rgba::new(0.95, 0.95, 0.95, 1.0)), 800.0, true), "a light bar on a dark page");
        assert!(behind_is_dark(&[], 800.0, true), "nothing painted there: the page's scheme");
        assert!(!behind_is_dark(&bar(Rgba::new(0.0, 0.0, 0.0, 0.2)), 800.0, false), "a see-through box says nothing");
    }

    #[test]
    fn an_inactive_window_dims_its_glyphs() {
        let alpha = |active| {
            paints(800.0, State { active, ..State::default() }, false)
                .into_iter()
                .find_map(|p| match p {
                    Paint::Path(p) => p.paint.stroke.map(|c| c.a),
                    _ => None,
                })
                .unwrap()
        };
        assert_eq!(alpha(true), 1.0);
        assert!(alpha(false) < 0.5);
    }
}
