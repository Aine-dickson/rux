//! Where the device is, from the app's own Rust: the same calls script's
//! `location` has.
//!
//! **One fix, or a watch.** [`current`] answers with one [`Fix`]. [`watch`]
//! starts continuous updates: each fix goes to every [`on_fix`] listener (the
//! app's Rust, with no page on screen) and becomes [`position`], which script
//! reads as `location.position`. [`stop`] ends it.
//!
//! It needs `[permissions.location]` in `rux.toml` and the person's consent
//! (`rux::permissions::request("location")`, from a tap). A watch ends with the
//! app unless a `background` service of type `location` keeps the process:
//!
//! ```ignore
//! use rux::location::{self, Options};
//!
//! let fix = location::current(Options::new()).await?;
//! location::on_fix(|fix| { /* post fix.lat, fix.lng to the API */ });
//! location::watch(Options::new().interval_ms(15_000))?;
//! ```
//!
//! The shell installs the backend at start-up with [`set_backend`], so these
//! calls fail with `unavailable` in a program that never started one.

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

use crate::{Any, Error, IntoRux, Signal};

/// How exact a fix should be.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Accuracy {
    /// GPS where there is one. The default.
    High,
    /// Cell and Wi-Fi: coarser, cheaper.
    Balanced,
}

impl Accuracy {
    /// The word script writes.
    pub fn word(self) -> &'static str {
        match self {
            Accuracy::High => "high",
            Accuracy::Balanced => "balanced",
        }
    }
}

/// What a call asks for. Every part is optional.
#[derive(Clone, Debug, PartialEq)]
pub struct Options {
    pub accuracy: Accuracy,
    /// How long [`current`] waits for a fix before it gives up with `timeout`.
    pub timeout_ms: u64,
    /// The shortest wait between a watch's fixes.
    pub interval_ms: u64,
    /// The shortest distance, in metres, between a watch's fixes. 0 for none.
    pub distance_m: f64,
}

impl Options {
    pub fn new() -> Options {
        Options { accuracy: Accuracy::High, timeout_ms: 30_000, interval_ms: 5_000, distance_m: 0.0 }
    }

    pub fn accuracy(mut self, accuracy: Accuracy) -> Options {
        self.accuracy = accuracy;
        self
    }

    pub fn timeout_ms(mut self, ms: u64) -> Options {
        self.timeout_ms = ms;
        self
    }

    pub fn interval_ms(mut self, ms: u64) -> Options {
        self.interval_ms = ms;
        self
    }

    pub fn distance_m(mut self, metres: f64) -> Options {
        self.distance_m = metres;
        self
    }
}

impl Default for Options {
    fn default() -> Options {
        Options::new()
    }
}

/// One position.
#[derive(Clone, Debug, PartialEq)]
pub struct Fix {
    pub lat: f64,
    pub lng: f64,
    /// The radius, in metres, the device is within.
    pub accuracy_m: f64,
    /// When it was taken: milliseconds since the Unix epoch.
    pub at: i64,
    /// Android's mock-location flag. Always `false` where the platform cannot say.
    pub is_mock: bool,
}

impl IntoRux for Fix {
    fn into_rux(self) -> Result<Any, Error> {
        let mut map = std::collections::BTreeMap::new();
        map.insert("lat".to_string(), Any::Float(self.lat));
        map.insert("lng".to_string(), Any::Float(self.lng));
        map.insert("accuracyM".to_string(), Any::Float(self.accuracy_m));
        map.insert("at".to_string(), Any::Int(self.at));
        map.insert("isMock".to_string(), Any::Bool(self.is_mock));
        Ok(Any::Map(map))
    }
}

/// A fix arriving later.
pub type Answer = Pin<Box<dyn Future<Output = Result<Fix, Error>> + Send + 'static>>;

/// What the shell provides.
pub struct Backend {
    pub current: fn(Options) -> Answer,
    pub watch: fn(Options) -> Result<(), Error>,
    pub stop: fn() -> Result<(), Error>,
}

static BACKEND: OnceLock<Backend> = OnceLock::new();

/// Install the backend. The first one wins, as the shell may start twice.
pub fn set_backend(backend: Backend) {
    let _ = BACKEND.set(backend);
}

fn backend() -> Result<&'static Backend, Error> {
    BACKEND.get().ok_or_else(|| Error::new("unavailable", "no location backend is installed: the shell has not started"))
}

static WATCHING: AtomicBool = AtomicBool::new(false);

type Listener = Box<dyn Fn(&Fix) + Send + Sync>;
static LISTENERS: Mutex<Vec<Listener>> = Mutex::new(Vec::new());

/// What script's `location.position` reads: the newest fix, `none` before one.
#[doc(hidden)]
pub static POSITION_SIGNAL: Signal<Fix> = Signal::new();

/// What script's `location.watching` reads.
#[doc(hidden)]
pub static WATCHING_SIGNAL: Signal<bool> = Signal::new();

/// The newest fix a watch (or [`report`]) gave, if any.
pub fn position() -> Option<Fix> {
    POSITION_SIGNAL.get()
}

/// Whether a watch is running now.
pub fn watching() -> bool {
    WATCHING.load(Ordering::Acquire)
}

/// Call `f` with each fix a watch delivers, from whichever thread delivers
/// it: keep it short, and hand real work (a request) to your own thread.
pub fn on_fix(f: impl Fn(&Fix) + Send + Sync + 'static) {
    LISTENERS.lock().unwrap_or_else(|e| e.into_inner()).push(Box::new(f));
}

/// The shell's report of a fix a watch delivered.
#[doc(hidden)]
pub fn report(fix: Fix) {
    POSITION_SIGNAL.set(fix.clone());
    for listener in LISTENERS.lock().unwrap_or_else(|e| e.into_inner()).iter() {
        listener(&fix);
    }
}

/// The shell's report that a watch started or ended.
#[doc(hidden)]
pub fn set_watching(on: bool) {
    let before = WATCHING.swap(on, Ordering::AcqRel);
    // The first report always sets the signal: it holds nothing until then.
    if before != on || WATCHING_SIGNAL.get().is_none() {
        WATCHING_SIGNAL.set(on);
    }
}

/// One fix. Fails with `permission` (not allowed), `disabled` (location
/// services are off), `timeout` or `unsupported`.
pub async fn current(options: Options) -> Result<Fix, Error> {
    (backend()?.current)(options).await
}

/// Start a watch, or change the one running. Fixes go to [`on_fix`] listeners
/// and [`position`].
pub fn watch(options: Options) -> Result<(), Error> {
    (backend()?.watch)(options)
}

/// End the watch. Stopping what is not running is not an error.
pub fn stop() -> Result<(), Error> {
    (backend()?.stop)()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::sync::Arc;

    fn fix(lat: f64) -> Fix {
        Fix { lat, lng: 32.58, accuracy_m: 12.0, at: 1_700_000_000_000, is_mock: false }
    }

    /// One test, because the state is the process's.
    #[test]
    fn a_fix_reaches_the_signal_and_every_listener_and_a_call_without_a_shell_says_so() {
        assert_eq!(watch(Options::new()).unwrap_err().kind, "unavailable");
        assert_eq!(stop().unwrap_err().kind, "unavailable");
        assert!(position().is_none() && !watching());

        let heard = Arc::new(AtomicUsize::new(0));
        let h = heard.clone();
        on_fix(move |_| {
            h.fetch_add(1, Ordering::SeqCst);
        });
        report(fix(0.31));
        report(fix(0.32));
        assert_eq!(heard.load(Ordering::SeqCst), 2);
        assert_eq!(position().unwrap().lat, 0.32, "the newest wins");

        set_watching(true);
        assert!(watching());
        assert_eq!(WATCHING_SIGNAL.get(), Some(true));
        set_watching(false);
        assert_eq!(WATCHING_SIGNAL.get(), Some(false));
    }

    #[test]
    fn a_fix_is_read_in_rux_as_a_record() {
        let Any::Map(m) = fix(0.31).into_rux().unwrap() else { panic!("not a map") };
        assert!(matches!(m.get("lat"), Some(Any::Float(v)) if *v == 0.31));
        assert!(matches!(m.get("accuracyM"), Some(Any::Float(v)) if *v == 12.0));
        assert!(matches!(m.get("at"), Some(Any::Int(1_700_000_000_000))));
        assert!(matches!(m.get("isMock"), Some(Any::Bool(false))));
    }
}
