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
//! `window.open(path, options)` opens another `.rux` file as its own document
//! in its own window and gives back a [`WindowHandle`]: `w.close()`,
//! `w.focus()` and `w.isOpen`. The call only **asks**: it queues an
//! [`OpenRequest`], and the shell, which owns the windows, drains the queue
//! after the event it came from ([`take_requests`]).
//!
//! Android and the web have no such window: the calls throw `unsupported`,
//! `maximized` reads `false` and `canOpen` is `false`.

use rux_runtime::native::{install, Any, Call, Error, Module, Signal};
use winit::window::WindowId;

/// Whether this build can open windows: desktop yes, Android and the web no.
static CAN_OPEN: Signal<bool> = Signal::new();

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
    calls.insert("open".to_string(), Call::sync(open));
    calls.insert("Window.close".to_string(), Call::sync(|args| ask(args, true)));
    calls.insert("Window.focus".to_string(), Call::sync(|args| ask(args, false)));
    calls.insert("Window.isOpen".to_string(), Call::sync(is_open));
    calls.insert("Window.error".to_string(), Call::sync(error_of));
    calls.insert("openRemote".to_string(), Call::sync(open_remote));
    calls.insert("canOpen".to_string(), Call::sync(|_args: Vec<Any>| CAN_OPEN.__read()));
    let _ = install(Module { interface, calls });
    CAN_OPEN.set(cfg!(not(any(target_arch = "wasm32", target_os = "android"))));
    #[cfg(any(target_arch = "wasm32", target_os = "android"))]
    MAXIMIZED.set(false);
}

/// The window the running code belongs to, for the shell to note as the
/// opener of a window it opens.
#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
fn current() -> Option<WindowId> {
    CURRENT.with(|c| c.get())
}

/// A window an app opened, as script holds it: `isOpen` follows it, `close()`
/// and `focus()` ask the shell. A resource, so script can keep it in a signal.
pub(crate) struct WindowHandle {
    #[cfg_attr(any(target_arch = "wasm32", target_os = "android"), allow(dead_code, reason = "read by `close()` and `focus()`, desktop only"))]
    pub(crate) id: u64,
    open: std::sync::atomic::AtomicBool,
    /// Why a window from `openRemote` did not open, for `Window.error`.
    error: std::sync::Mutex<Option<String>>,
}

impl rux_runtime::native::Resource for WindowHandle {
    const NAME: &'static str = "Window";
}

impl WindowHandle {
    /// The window is gone: `isOpen` is `false` from now, wherever it was read.
    #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
    pub(crate) fn mark_closed(&self) {
        if self.open.swap(false, std::sync::atomic::Ordering::AcqRel) {
            rux_runtime::native::notify();
        }
    }

    /// The window never opened, and why: `error` says it, `isOpen` is `false`.
    #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
    pub(crate) fn fail(&self, message: String) {
        *self.error.lock().unwrap_or_else(|e| e.into_inner()) = Some(message);
        self.mark_closed();
        rux_runtime::native::notify();
    }
}

/// The window a `Window` method or property was called on.
fn handle_of(args: &[Any]) -> Result<std::sync::Arc<WindowHandle>, Error> {
    match args.first() {
        Some(Any::Resource(h)) => h.get::<WindowHandle>().ok_or_else(|| Error::new("type", "that is not a Window")),
        _ => Err(Error::new("type", "that is not a Window")),
    }
}

fn error_of(args: Vec<Any>) -> Result<Any, Error> {
    Ok(handle_of(&args)?.error.lock().unwrap_or_else(|e| e.into_inner()).clone().map_or(Any::None, Any::Str))
}

fn is_open(args: Vec<Any>) -> Result<Any, Error> {
    Ok(Any::Bool(handle_of(&args)?.open.load(std::sync::atomic::Ordering::Acquire)))
}

#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
fn ask(args: Vec<Any>, close: bool) -> Result<Any, Error> {
    let id = handle_of(&args)?.id;
    REQUESTS.with(|r| r.borrow_mut().push(if close { Request::Close(id) } else { Request::Focus(id) }));
    Ok(Any::None)
}

#[cfg(any(target_arch = "wasm32", target_os = "android"))]
fn ask(_args: Vec<Any>, _close: bool) -> Result<Any, Error> {
    Err(unsupported())
}

#[cfg(any(target_arch = "wasm32", target_os = "android"))]
fn open(_args: Vec<Any>) -> Result<Any, Error> {
    Err(unsupported())
}

#[cfg(any(target_arch = "wasm32", target_os = "android"))]
fn open_remote(_args: Vec<Any>) -> Result<Any, Error> {
    Err(unsupported())
}

#[cfg(any(target_arch = "wasm32", target_os = "android"))]
fn unsupported() -> Error {
    Error::new("unsupported", "this build has no desktop windows to open: check `window.canOpen` first")
}

/// What `window.open` asked for, once its options are read.
#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
pub(crate) struct OpenRequest {
    pub(crate) handle: std::sync::Arc<WindowHandle>,
    /// The window the code that opened it was running in.
    pub(crate) opener: Option<WindowId>,
    pub(crate) path: String,
    pub(crate) settings: crate::WindowSettings,
    pub(crate) resizable: Option<bool>,
    /// Above the opener, minimizing with it and with no taskbar button.
    pub(crate) owned: bool,
    pub(crate) props: Vec<(String, rux_reactive::Value)>,
    /// What to run in the opener when the window closes: a script `fn`, as a
    /// callback handle (`rux_script::native::Callback`).
    pub(crate) on_close: Option<rux_runtime::native::Handle>,
}

/// What `window.openRemote` asked for, once its options are read and the
/// grants are held to what the opener may hand out.
#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
pub(crate) struct RemoteRequest {
    pub(crate) handle: std::sync::Arc<WindowHandle>,
    pub(crate) opener: Option<WindowId>,
    pub(crate) url: String,
    /// What the document runs on: the grants, and the opener's limits.
    pub(crate) terms: rux_runtime::Sandbox,
    pub(crate) settings: crate::WindowSettings,
    pub(crate) resizable: Option<bool>,
    pub(crate) owned: bool,
    pub(crate) on_close: Option<rux_runtime::native::Handle>,
    /// What to run in the opener when the document could not be fetched or
    /// loaded. The reason is the handle's `error`.
    pub(crate) on_error: Option<rux_runtime::native::Handle>,
}

/// What the shell is asked to do, in the order it was asked.
#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
pub(crate) enum Request {
    Open(OpenRequest),
    OpenRemote(RemoteRequest),
    Close(u64),
    Focus(u64),
}

#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
thread_local! {
    static REQUESTS: std::cell::RefCell<Vec<Request>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// The requests made since the last call.
#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
pub(crate) fn take_requests() -> Vec<Request> {
    REQUESTS.with(|r| std::mem::take(&mut *r.borrow_mut()))
}

#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
static NEXT_WINDOW: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
fn option_error(message: String) -> Error {
    Error::new("option", message)
}

#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
fn open(args: Vec<Any>) -> Result<Any, Error> {
    let Some(Any::Str(path)) = args.first() else {
        return Err(option_error(
            "window.open takes the path of a .rux file first: window.open(\"windows/inspector.rux\", { ... })".into(),
        ));
    };
    let options = match args.get(1) {
        Some(Any::Map(m)) => m.clone(),
        None | Some(Any::None) => Default::default(),
        Some(other) => {
            return Err(option_error(format!("window.open takes its options as a map, not a {}", other.kind())));
        }
    };
    let number = |key: &str, v: &Any| match v {
        Any::Int(i) => Ok(*i as f64),
        Any::Float(f) => Ok(*f),
        other => Err(option_error(format!("`{key}` takes a number, not a {}", other.kind()))),
    };
    let flag = |key: &str, v: &Any| match v {
        Any::Bool(b) => Ok(*b),
        other => Err(option_error(format!("`{key}` takes true or false, not a {}", other.kind()))),
    };
    let mut settings = crate::WindowSettings::default();
    let mut resizable = None;
    let mut owned = true;
    let mut props = Vec::new();
    let mut on_close = None;
    for (key, v) in &options {
        match (key.as_str(), v) {
            (_, Any::None) => {}
            ("title", Any::Str(t)) => settings.title = Some(t.clone()),
            ("title", other) => {
                return Err(option_error(format!("`title` takes text, not a {}", other.kind())));
            }
            ("width", v) => settings.width = Some(number(key, v)?),
            ("height", v) => settings.height = Some(number(key, v)?),
            ("minWidth", v) => settings.min_width = Some(number(key, v)?),
            ("minHeight", v) => settings.min_height = Some(number(key, v)?),
            ("maximized", v) => settings.maximized = flag(key, v)?,
            ("decorations", v) => settings.frameless = !flag(key, v)?,
            ("titleBar", Any::Str(t)) if t == "overlay" => settings.title_overlay = true,
            ("titleBar", _) => return Err(option_error("`titleBar` takes \"overlay\"".into())),
            ("resizable", v) => resizable = Some(flag(key, v)?),
            ("owned", v) => owned = flag(key, v)?,
            ("props", Any::Map(m)) => {
                for (name, value) in m {
                    props.push((name.clone(), prop_value(name, value)?));
                }
            }
            ("props", other) => {
                return Err(option_error(format!("`props` takes a map of the window's props, not a {}", other.kind())));
            }
            ("onClose", Any::Resource(h)) if h.type_name() == "Callback" => on_close = Some(h.clone()),
            ("onClose", _) => return Err(option_error("`onClose` takes a function".into())),
            (other, _) => {
                return Err(option_error(format!("window.open has no option `{other}`: it takes {}", rux_runtime::window_open::OPTIONS.join(", "))));
            }
        }
    }
    let handle = std::sync::Arc::new(WindowHandle {
        id: NEXT_WINDOW.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        open: std::sync::atomic::AtomicBool::new(true),
        error: std::sync::Mutex::new(None),
    });
    REQUESTS.with(|r| {
        r.borrow_mut().push(Request::Open(OpenRequest {
            handle: handle.clone(),
            opener: current(),
            path: path.clone(),
            settings,
            resizable,
            owned,
            props,
            on_close,
        }))
    });
    Ok(Any::Resource(rux_runtime::native::Handle::from_arc(handle)))
}

/// `window.openRemote(url, options)`: fetch a document from an address and
/// open it in a window of its own, sandboxed. The call checks the address and
/// the options and answers at once; the shell fetches on another thread and
/// opens the window when the document has come.
#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
fn open_remote(args: Vec<Any>) -> Result<Any, Error> {
    let Some(Any::Str(url)) = args.first() else {
        return Err(option_error(
            "window.openRemote takes the address of a .rux file first: window.openRemote(\"https://example.com/app.rux\", { ... })"
                .into(),
        ));
    };
    rux_runtime::fetch::check_url(url).map_err(|why| Error::new("address", why))?;
    let options = match args.get(1) {
        Some(Any::Map(m)) => m.clone(),
        None | Some(Any::None) => Default::default(),
        Some(other) => {
            return Err(option_error(format!("window.openRemote takes its options as a map, not a {}", other.kind())));
        }
    };
    let number = |key: &str, v: &Any| match v {
        Any::Int(i) => Ok(*i as f64),
        Any::Float(f) => Ok(*f),
        other => Err(option_error(format!("`{key}` takes a number, not a {}", other.kind()))),
    };
    let flag = |key: &str, v: &Any| match v {
        Any::Bool(b) => Ok(*b),
        other => Err(option_error(format!("`{key}` takes true or false, not a {}", other.kind()))),
    };
    let mut settings = crate::WindowSettings::default();
    let mut resizable = None;
    let mut owned = true;
    let mut grants: Vec<String> = Vec::new();
    let mut on_close = None;
    let mut on_error = None;
    for (key, v) in &options {
        match (key.as_str(), v) {
            (_, Any::None) => {}
            ("title", Any::Str(t)) => settings.title = Some(t.clone()),
            ("title", other) => {
                return Err(option_error(format!("`title` takes text, not a {}", other.kind())));
            }
            ("width", v) => settings.width = Some(number(key, v)?),
            ("height", v) => settings.height = Some(number(key, v)?),
            ("minWidth", v) => settings.min_width = Some(number(key, v)?),
            ("minHeight", v) => settings.min_height = Some(number(key, v)?),
            ("maximized", v) => settings.maximized = flag(key, v)?,
            ("decorations", v) => settings.frameless = !flag(key, v)?,
            ("titleBar", Any::Str(t)) if t == "overlay" => settings.title_overlay = true,
            ("titleBar", _) => return Err(option_error("`titleBar` takes \"overlay\"".into())),
            ("resizable", v) => resizable = Some(flag(key, v)?),
            ("owned", v) => owned = flag(key, v)?,
            ("sandbox", Any::Array(items)) => {
                for item in items {
                    match item {
                        Any::Str(g) => grants.push(g.clone()),
                        other => {
                            return Err(option_error(format!(
                                "`sandbox` takes the names to grant as text, such as [\"clipboard\"], not a {}",
                                other.kind()
                            )));
                        }
                    }
                }
            }
            ("sandbox", other) => {
                return Err(option_error(format!(
                    "`sandbox` takes a list of the names to grant, such as [\"clipboard\"], not a {}",
                    other.kind()
                )));
            }
            ("onClose", Any::Resource(h)) if h.type_name() == "Callback" => on_close = Some(h.clone()),
            ("onClose", _) => return Err(option_error("`onClose` takes a function".into())),
            ("onError", Any::Resource(h)) if h.type_name() == "Callback" => on_error = Some(h.clone()),
            ("onError", _) => return Err(option_error("`onError` takes a function".into())),
            (other, _) => {
                return Err(option_error(format!(
                    "window.openRemote has no option `{other}`: it takes {}",
                    rux_runtime::window_open::REMOTE_OPTIONS.join(", ")
                )));
            }
        }
    }
    // What this document runs on is what it may hand out: a sandboxed opener
    // gives no more than it holds.
    let terms = rux_runtime::window_open::remote_terms(rux_runtime::current_sandbox().as_ref(), &grants)
        .map_err(|why| Error::new("grant", why))?;
    let handle = std::sync::Arc::new(WindowHandle {
        id: NEXT_WINDOW.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        open: std::sync::atomic::AtomicBool::new(true),
        error: std::sync::Mutex::new(None),
    });
    REQUESTS.with(|r| {
        r.borrow_mut().push(Request::OpenRemote(RemoteRequest {
            handle: handle.clone(),
            opener: current(),
            url: url.clone(),
            terms,
            settings,
            resizable,
            owned,
            on_close,
            on_error,
        }))
    });
    Ok(Any::Resource(rux_runtime::native::Handle::from_arc(handle)))
}

/// A prop the opener passed, as a document takes one. A resource cannot be
/// one: it is Rust's, and the window is a document of its own.
#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
fn prop_value(name: &str, v: &Any) -> Result<rux_reactive::Value, Error> {
    use rux_reactive::Value;
    Ok(match v {
        Any::None => Value::Null,
        Any::Bool(b) => Value::Bool(*b),
        Any::Int(i) => Value::Number(*i as f64),
        Any::Float(f) => Value::Number(*f),
        Any::Str(s) => Value::Text(s.clone()),
        Any::Array(items) => Value::list(items.iter().map(|i| prop_value(name, i)).collect::<Result<_, _>>()?),
        Any::Map(m) => {
            Value::Map(m.iter().map(|(k, v)| Ok((k.clone(), prop_value(name, v)?))).collect::<Result<_, Error>>()?)
        }
        Any::Resource(_) => {
            return Err(option_error(format!("the prop `{name}` holds a resource, and a window's props are plain values")));
        }
    })
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
#[cfg_attr(target_arch = "wasm32", allow(dead_code, reason = "the web opens its one window elsewhere"))]
pub(crate) fn attach(_window: &std::sync::Arc<winit::window::Window>) {}

/// A window is gone: its calls have nothing to act on.
#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
pub(crate) fn detach(id: WindowId) {
    if let Some(map) = WINDOWS.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
        map.remove(&id);
    }
}

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

#[cfg(all(test, not(any(target_arch = "wasm32", target_os = "android"))))]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn call(url: &str, options: &[(&str, Any)]) -> Result<Any, Error> {
        let map: BTreeMap<String, Any> = options.iter().map(|(k, v)| (k.to_string(), v.clone())).collect();
        open_remote(vec![Any::Str(url.into()), Any::Map(map)])
    }

    fn grants(names: &[&str]) -> Any {
        Any::Array(names.iter().map(|n| Any::Str(n.to_string())).collect())
    }

    #[test]
    fn a_bad_address_throws_at_the_call_and_asks_for_nothing() {
        let _ = take_requests();
        let err = call("http://example.com/a.rux", &[]).err().expect("refused");
        assert_eq!(err.kind, "address");
        assert!(err.message.contains("https"), "{}", err.message);
        assert!(take_requests().is_empty());
    }

    #[test]
    fn an_option_it_does_not_take_throws_and_props_is_one() {
        let err = call("https://example.com/a.rux", &[("props", Any::Map(BTreeMap::new()))]).err().expect("refused");
        assert_eq!(err.kind, "option");
        assert!(err.message.contains("no option `props`"), "{}", err.message);
    }

    #[test]
    fn the_apps_own_stores_and_unknown_names_cannot_be_granted() {
        for bad in ["settings", "secrets.get", "nonsense"] {
            let err = call("https://example.com/a.rux", &[("sandbox", grants(&[bad]))]).err().expect("refused");
            assert_eq!(err.kind, "grant", "{bad}");
        }
        let err = call("https://example.com/a.rux", &[("sandbox", Any::Str("clipboard".into()))]).err().expect("refused");
        assert_eq!(err.kind, "option");
    }

    #[test]
    fn a_good_call_answers_with_a_window_and_queues_the_fetch() {
        let _ = take_requests();
        let handle = call("https://example.com/a.rux", &[("sandbox", grants(&["clipboard"])), ("title", Any::Str("P".into()))])
            .expect("accepted");
        assert!(matches!(handle, Any::Resource(_)));
        let mut requests = take_requests();
        assert_eq!(requests.len(), 1);
        let Some(Request::OpenRemote(remote)) = requests.pop() else { panic!("an OpenRemote") };
        assert_eq!(remote.url, "https://example.com/a.rux");
        assert!(remote.terms.grants("clipboard"));
        assert_eq!(remote.settings.title.as_deref(), Some("P"));
        // A handle that failed reads closed, with the reason.
        remote.handle.fail("no".into());
        assert!(!remote.handle.open.load(std::sync::atomic::Ordering::Acquire));
        assert_eq!(error_of(vec![Any::Resource(rux_runtime::native::Handle::from_arc(remote.handle.clone()))]).unwrap(), Any::Str("no".into()));
    }
}
