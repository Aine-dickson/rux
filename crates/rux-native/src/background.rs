//! Background work, from the app's own Rust: the same calls script's
//! `background` has.
//!
//! **What it is.** On Android a *foreground service* keeps the app's process,
//! and so the app's threads, running while the app is not on screen, and shows
//! a permanent notification the person can see and tap. That is how a rider
//! who is "on shift" keeps getting new jobs: the app's Rust polls on a thread,
//! and posts a [`notifications`](crate::notifications) notice when one comes.
//!
//! It needs `[background]` in `rux.toml` (what the service is for) and a
//! channel for its notification (`[channels.<id>]`), and Android 12 and later
//! start one only while the app is in front, so call [`start`] from a tap.
//!
//! ```ignore
//! use rux::background::{self, Options};
//!
//! background::start(Options::new("shift", "On shift").text("Looking for jobs").route("/board"))?;
//! while background::active() {
//!     // poll, and post a notice when there is something
//! }
//! background::stop()?;
//! ```
//!
//! **The service ends with the process.** Swiping the app away from Recents, or
//! Android taking the process, stops it, and the next start of the app finds
//! [`active`] `false`. Keep "on shift" in the app's own saved state and call
//! [`start`] again when the app opens.
//!
//! The shell installs the backend at start-up with [`set_backend`], so these
//! calls fail with `unavailable` in a program that never started one.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

use crate::{Error, Signal};

/// What the permanent notification says. `channel` is declared under
/// `[channels.<id>]`; prefer a quiet one, since it stays up.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Options {
    pub channel: String,
    pub title: String,
    /// The body. Empty for none.
    pub text: String,
    /// Where a tap opens the app: a route such as `/board`. `None` opens the
    /// app where it was.
    pub route: Option<String>,
}

impl Options {
    pub fn new(channel: impl Into<String>, title: impl Into<String>) -> Options {
        Options { channel: channel.into(), title: title.into(), text: String::new(), route: None }
    }

    /// The body text.
    pub fn text(mut self, text: impl Into<String>) -> Options {
        self.text = text.into();
        self
    }

    /// The route a tap opens.
    pub fn route(mut self, route: impl Into<String>) -> Options {
        self.route = Some(route.into());
        self
    }
}

/// What the shell provides.
pub struct Backend {
    pub start: fn(Options) -> Result<(), Error>,
    pub stop: fn() -> Result<(), Error>,
}

static BACKEND: OnceLock<Backend> = OnceLock::new();

/// Install the backend. The first one wins, as the shell may start twice.
pub fn set_backend(backend: Backend) {
    let _ = BACKEND.set(backend);
}

fn backend() -> Result<&'static Backend, Error> {
    BACKEND.get().ok_or_else(|| {
        Error::new("unavailable", "no background backend is installed: the shell has not started")
    })
}

static ACTIVE: AtomicBool = AtomicBool::new(false);

type Listener = Box<dyn Fn(bool) + Send + Sync>;
static LISTENERS: Mutex<Vec<Listener>> = Mutex::new(Vec::new());

/// What script's `background.active` reads.
#[doc(hidden)]
pub static ACTIVE_SIGNAL: Signal<bool> = Signal::new();

/// Whether the service is running now.
pub fn active() -> bool {
    ACTIVE.load(Ordering::Acquire)
}

/// Call `f` each time the service starts or stops, from whichever thread
/// reports it: keep it short.
pub fn on_change(f: impl Fn(bool) + Send + Sync + 'static) {
    LISTENERS.lock().unwrap_or_else(|e| e.into_inner()).push(Box::new(f));
}

/// The shell's report that the service started or stopped.
#[doc(hidden)]
pub fn set_active(active: bool) {
    let before = ACTIVE.swap(active, Ordering::AcqRel);
    // The first report always sets the signal: it holds nothing until then.
    if before == active && ACTIVE_SIGNAL.get().is_some() {
        return;
    }
    ACTIVE_SIGNAL.set(active);
    if before != active {
        for listener in LISTENERS.lock().unwrap_or_else(|e| e.into_inner()).iter() {
            listener(active);
        }
    }
}

/// Start the service, or update its notification when it is running. Any
/// thread, but Android 12 and later refuse it unless the app is in front.
pub fn start(options: Options) -> Result<(), Error> {
    (backend()?.start)(options)
}

/// Stop the service. Stopping what is not running is not an error.
pub fn stop() -> Result<(), Error> {
    (backend()?.stop)()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::sync::Arc;

    /// One test, because the state is the process's: parallel tests would see
    /// each other's reports.
    #[test]
    fn the_state_follows_what_the_shell_reports_and_a_call_without_a_shell_says_so() {
        assert!(!active());
        assert_eq!(start(Options::new("shift", "On shift").text("x").route("/board")).unwrap_err().kind, "unavailable");
        assert_eq!(stop().unwrap_err().kind, "unavailable");

        let heard = Arc::new(AtomicUsize::new(0));
        let h = heard.clone();
        on_change(move |_| {
            h.fetch_add(1, Ordering::SeqCst);
        });
        set_active(false);
        assert_eq!(ACTIVE_SIGNAL.get(), Some(false), "the first report fills the signal");
        assert_eq!(heard.load(Ordering::SeqCst), 0, "nothing changed");
        set_active(true);
        assert!(active());
        assert_eq!(ACTIVE_SIGNAL.get(), Some(true));
        set_active(true);
        assert_eq!(heard.load(Ordering::SeqCst), 1, "the same report twice is one change");
        set_active(false);
        assert!(!active());
        assert_eq!(heard.load(Ordering::SeqCst), 2);
    }
}
