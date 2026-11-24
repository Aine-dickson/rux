//! Whether the device has a network connection, for the app's own Rust and for
//! script's `network.online`.
//!
//! The shell calls [`set_online`] when the platform says the default network
//! came or went (Android's `ConnectivityManager`, a browser's `online` and
//! `offline` events). The same value is `network.online` in script, a `bool`
//! signal.
//!
//! **It says a link exists, not that a server answers**: a captive portal or a
//! dead upstream is "online" here. Use it to show "offline" and to pause calls
//! that would only fail, and still handle a failed request.
//!
//! ```ignore
//! use rux::network;
//!
//! if !network::online() {
//!     // wait for a connection
//! }
//! network::on_change(|online| println!("online: {online}"));
//! ```
//!
//! A platform that cannot say (the desktop) reports online. Before the shell
//! has said anything the device counts as online.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use crate::Signal;

static ONLINE: AtomicBool = AtomicBool::new(true);

type Listener = Box<dyn Fn(bool) + Send + Sync>;
static LISTENERS: Mutex<Vec<Listener>> = Mutex::new(Vec::new());

/// What script's `network.online` reads.
#[doc(hidden)]
pub static ONLINE_SIGNAL: Signal<bool> = Signal::new();

/// Whether there is a network connection.
pub fn online() -> bool {
    ONLINE.load(Ordering::Acquire)
}

/// Call `f` each time the connection comes or goes, from whichever thread the
/// shell reports it on: keep it short and hand the work on.
pub fn on_change(f: impl Fn(bool) + Send + Sync + 'static) {
    LISTENERS.lock().unwrap_or_else(|e| e.into_inner()).push(Box::new(f));
}

/// The shell's report. Saying the same thing twice changes nothing.
#[doc(hidden)]
pub fn set_online(online: bool) {
    let before = ONLINE.swap(online, Ordering::AcqRel);
    // The first report always sets the signal: it holds nothing until then.
    if before == online && ONLINE_SIGNAL.get().is_some() {
        return;
    }
    ONLINE_SIGNAL.set(online);
    if before != online {
        for listener in LISTENERS.lock().unwrap_or_else(|e| e.into_inner()).iter() {
            listener(online);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::sync::Arc;

    /// One test, because the state is the process's.
    #[test]
    fn a_change_reaches_the_signal_and_every_listener_once() {
        assert!(online(), "online until told otherwise");
        let heard = Arc::new(AtomicUsize::new(0));
        let h = heard.clone();
        on_change(move |_| {
            h.fetch_add(1, Ordering::SeqCst);
        });
        set_online(true);
        assert_eq!(ONLINE_SIGNAL.get(), Some(true), "the first report fills the signal");
        assert_eq!(heard.load(Ordering::SeqCst), 0, "and is no change");
        set_online(false);
        set_online(false);
        assert!(!online());
        assert_eq!(ONLINE_SIGNAL.get(), Some(false));
        assert_eq!(heard.load(Ordering::SeqCst), 1, "the same word twice is one change");
        set_online(true);
        assert_eq!(heard.load(Ordering::SeqCst), 2);
    }
}
