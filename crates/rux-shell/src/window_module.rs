//! `window.minimize()`, `window.maximize()`, `window.restore()`,
//! `window.toggleMaximize()` and `window.maximized`: the desktop window, for
//! an app that draws its own title bar (Mava Studio's MS-012).
//!
//! The calls act at once, on the window the shell opened. `maximized` is a
//! signal the shell keeps current: it is set when the window opens and again
//! on every resize, which is when a maximize, a restore or a snap lands, so a
//! button can show the right glyph.
//!
//! Android and the web have no such window: the calls throw `unsupported`,
//! and `maximized` reads `false`.

use rux_runtime::native::{install, Any, Call, Error, Module, Signal};

static MAXIMIZED: Signal<bool> = Signal::new();

#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
static WINDOW: std::sync::Mutex<Option<std::sync::Arc<winit::window::Window>>> = std::sync::Mutex::new(None);

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
    calls.insert("maximized".to_string(), Call::sync(|_args: Vec<Any>| MAXIMIZED.__read()));
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
    let guard = WINDOW.lock().unwrap_or_else(|e| e.into_inner());
    let Some(window) = guard.as_ref() else {
        return Err(Error::new("window", "the window is not open yet"));
    };
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

/// The window the calls act on, and the first value of `maximized`.
#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
pub(crate) fn attach(window: &std::sync::Arc<winit::window::Window>) {
    *WINDOW.lock().unwrap_or_else(|e| e.into_inner()) = Some(window.clone());
    follow();
}

#[cfg(any(target_arch = "wasm32", target_os = "android"))]
pub(crate) fn attach(_window: &std::sync::Arc<winit::window::Window>) {}

/// Set `maximized` from the window, when it has changed.
#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
pub(crate) fn follow() {
    let now = WINDOW
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .map(|w| w.is_maximized());
    if let Some(now) = now {
        if MAXIMIZED.get() != Some(now) {
            MAXIMIZED.set(now);
        }
    }
}

#[cfg(any(target_arch = "wasm32", target_os = "android"))]
pub(crate) fn follow() {}
