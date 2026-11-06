//! `window.minimize()`, `window.maximize()`, `window.restore()`,
//! `window.toggleMaximize()` and `window.maximized`: the desktop window, for
//! an app that draws its own title bar (Mava Studio's MS-012).
//!
//! The calls act at once, on **the window the code is running in**: the shell
//! names that window around every entry into a document ([`as_window`]), so in
//! a window an app opened, `window.minimize()` is that window's and
//! `window.maximized` is that window's value. `maximized` is kept current by
//! the shell: it is set when a window opens and again on every resize, which is
//! when a maximize, a restore or a snap lands, so a button can show the right
//! glyph.
//!
//! Android and the web have no such window: the calls throw `unsupported`,
//! and `maximized` reads `false`.

use rux_runtime::native::{install, Any, Call, Error, Module, Signal};
use winit::window::WindowId;

/// Moves whenever any window's `maximized` does. It is the change trigger and
/// not the value: every document reads its own window's value again when it
/// moves (see [`as_window`]).
static MAXIMIZED: Signal<bool> = Signal::new();

#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
static WINDOWS: std::sync::Mutex<Option<std::collections::HashMap<WindowId, Open>>> = std::sync::Mutex::new(None);

/// A window the calls can act on, and what `maximized` says of it.
#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
struct Open {
    window: std::sync::Arc<winit::window::Window>,
    maximized: bool,
}

thread_local! {
    /// The window whose document is running, if any. Scripts run on the one
    /// UI thread, so a thread-local is the whole of the plumbing.
    static CURRENT: std::cell::Cell<Option<WindowId>> = const { std::cell::Cell::new(None) };
}

/// Run `f` as the document of window `id`: what `window.*` means inside it.
/// `None` is a document with no window (before the first one opens).
pub(crate) fn as_window<R>(id: Option<WindowId>, f: impl FnOnce() -> R) -> R {
    let before = CURRENT.with(|c| c.replace(id));
    let out = f();
    CURRENT.with(|c| c.set(before));
    out
}

/// Install the calls and the signal for every document this process loads.
pub(crate) fn install_window() {
    let interface = rux_runtime::builtins::window_interface();
    let mut calls = std::collections::HashMap::new();
    for (name, act) in [
        ("minimize", Act::Minimize),
        ("maximize", Act::Maximize),
        ("restore", Act::Restore),
        ("toggleMaximize", Act::Toggle),
    ] {
        calls.insert(
            name.to_string(),
            Call::sync(move |_args: Vec<Any>| {
                perform(act)?;
                Ok(Any::None)
            }),
        );
    }
    calls.insert("maximized".to_string(), Call::sync(|_args: Vec<Any>| read_maximized()));
    let _ = install(Module { interface, calls });
    #[cfg(any(target_arch = "wasm32", target_os = "android"))]
    MAXIMIZED.set(false);
}

#[derive(Clone, Copy)]
enum Act {
    Minimize,
    Maximize,
    Restore,
    Toggle,
}

#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
fn perform(act: Act) -> Result<(), Error> {
    let window = CURRENT
        .with(|c| c.get())
        .and_then(|id| WINDOWS.lock().unwrap_or_else(|e| e.into_inner()).as_ref()?.get(&id).map(|o| o.window.clone()))
        .ok_or_else(|| Error::new("window", "the window is not open yet"))?;
    match act {
        Act::Minimize => window.set_minimized(true),
        Act::Maximize => window.set_maximized(true),
        Act::Restore => {
            // A minimized window comes back as it was; a maximized one steps
            // down to its restored size.
            window.set_minimized(false);
            window.set_maximized(false);
        }
        Act::Toggle => window.set_maximized(!window.is_maximized()),
    }
    Ok(())
}

#[cfg(any(target_arch = "wasm32", target_os = "android"))]
fn perform(_act: Act) -> Result<(), Error> {
    Err(Error::new("unsupported", "this build has no desktop window to minimize, maximize or restore"))
}

/// `maximized` of the window the code runs in; `none` until it has one.
#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
fn read_maximized() -> Result<Any, Error> {
    let now = CURRENT
        .with(|c| c.get())
        .and_then(|id| WINDOWS.lock().unwrap_or_else(|e| e.into_inner()).as_ref()?.get(&id).map(|o| o.maximized));
    Ok(now.map_or(Any::None, Any::Bool))
}

#[cfg(any(target_arch = "wasm32", target_os = "android"))]
fn read_maximized() -> Result<Any, Error> {
    MAXIMIZED.__read()
}

/// A window the calls can act on, and the first value of its `maximized`.
#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
pub(crate) fn attach(window: &std::sync::Arc<winit::window::Window>) {
    WINDOWS.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(Default::default).insert(
        window.id(),
        Open { window: window.clone(), maximized: window.is_maximized() },
    );
    // Every document looks again: this window's value is new.
    MAXIMIZED.set(window.is_maximized());
}

#[cfg(any(target_arch = "wasm32", target_os = "android"))]
pub(crate) fn attach(_window: &std::sync::Arc<winit::window::Window>) {}

/// Set window `id`'s `maximized` from the window, when it has changed.
#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
pub(crate) fn follow(id: WindowId) {
    let moved = {
        let mut guard = WINDOWS.lock().unwrap_or_else(|e| e.into_inner());
        match guard.as_mut().and_then(|m| m.get_mut(&id)) {
            Some(open) => {
                let now = open.window.is_maximized();
                let moved = open.maximized != now;
                open.maximized = now;
                moved.then_some(now)
            }
            None => None,
        }
    };
    if let Some(now) = moved {
        // Every document reads its own window's value again.
        MAXIMIZED.set(now);
    }
}

#[cfg(any(target_arch = "wasm32", target_os = "android"))]
pub(crate) fn follow(_id: WindowId) {}
