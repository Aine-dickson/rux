//! Whether the app is on screen, for the app's own Rust and for script's
//! `app.state`.
//!
//! The shell calls [`set_visible`] when the platform says the app came to the
//! front or went to the back (Android's `onStart` and `onStop`, a browser tab
//! becoming hidden, every desktop window minimized). The same value is
//! `app.state` in script, `"foreground"` or `"background"`, so a poll in a
//! service can slow down when nobody is looking and a page can say so.
//!
//! ```ignore
//! use rux::app;
//!
//! if app::state() == "background" {
//!     // poll less often
//! }
//! app::on_change(|state| println!("the app is now {state}"));
//! ```
//!
//! Before the shell has said anything the app counts as in the front: it
//! cannot have started without a screen.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use crate::Signal;

static VISIBLE: AtomicBool = AtomicBool::new(true);

type Listener = Box<dyn Fn(&'static str) + Send + Sync>;
static LISTENERS: Mutex<Vec<Listener>> = Mutex::new(Vec::new());

/// What script's `app.state` reads.
#[doc(hidden)]
pub static STATE: Signal<String> = Signal::new();

fn name(visible: bool) -> &'static str {
    if visible {
        "foreground"
    } else {
        "background"
    }
}

/// `"foreground"` while the app is on screen, `"background"` while it is not.
pub fn state() -> &'static str {
    name(VISIBLE.load(Ordering::Acquire))
}

/// Call `f` each time the state changes, from whichever thread the shell
/// reports it on (the UI thread): keep it short and hand the work on.
pub fn on_change(f: impl Fn(&'static str) + Send + Sync + 'static) {
    LISTENERS.lock().unwrap_or_else(|e| e.into_inner()).push(Box::new(f));
}

/// The shell's report. Saying the same thing twice changes nothing, so a
/// platform that sends it on every resume does not wake the listeners.
#[doc(hidden)]
pub fn set_visible(visible: bool) {
    let before = VISIBLE.swap(visible, Ordering::AcqRel);
    // The first report always sets the signal: it holds nothing until then.
    if before == visible && STATE.get().is_some() {
        return;
    }
    STATE.set(name(visible).to_string());
    if before != visible {
        for listener in LISTENERS.lock().unwrap_or_else(|e| e.into_inner()).iter() {
            listener(name(visible));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::sync::Arc;

    /// One test, because the state is the process's: parallel tests would
    /// see each other's reports.
    #[test]
    fn the_state_follows_what_the_shell_reports_and_listeners_hear_only_changes() {
        assert_eq!(state(), "foreground");
        let heard = Arc::new(AtomicUsize::new(0));
        let h = heard.clone();
        on_change(move |_| {
            h.fetch_add(1, Ordering::SeqCst);
        });

        set_visible(true);
        assert_eq!(STATE.get().as_deref(), Some("foreground"), "the first report fills the signal");
        assert_eq!(heard.load(Ordering::SeqCst), 0, "nothing changed");

        set_visible(false);
        assert_eq!(state(), "background");
        assert_eq!(STATE.get().as_deref(), Some("background"));
        assert_eq!(heard.load(Ordering::SeqCst), 1);

        set_visible(false);
        assert_eq!(heard.load(Ordering::SeqCst), 1, "the same report twice is one change");

        set_visible(true);
        assert_eq!(state(), "foreground");
        assert_eq!(heard.load(Ordering::SeqCst), 2);
    }
}
