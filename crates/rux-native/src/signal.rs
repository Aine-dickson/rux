//! A value Rust owns and a document follows (Mava Studio's MS-003).
//!
//! `#[rux::export] pub static FRAME: rux::Signal<Frame> = rux::Signal::new();`
//! puts `frame` in its module, read in script like any other module's signal:
//! `{{ sim.frame?.tick }}` re-renders when it moves, and an `effect` reading it
//! runs again. Only Rust writes it, with [`Signal::set`], from any thread.
//!
//! **Latest value wins.** `set` stores the value and wakes the window; the
//! document reads what is there when it next settles, so a simulation setting
//! a frame five hundred times between two paints is drawn once, with the
//! newest. That is what a stream of frames wants, and a status the same.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use crate::{Any, Error, IntoRux};

type Waker = Arc<dyn Fn() + Send + Sync>;

/// Counted up by every [`Signal::set`] anywhere, so the runtime can tell
/// whether any signal moved without asking each one.
static EPOCH: AtomicU64 = AtomicU64::new(0);
static WAKER: Mutex<Option<Waker>> = Mutex::new(None);

/// Call `f` whenever a signal is set, from whichever thread set it: how a
/// sleeping window learns it has something to draw. The runtime installs the
/// same waker an `await`'s answer uses.
pub fn set_signal_waker(f: impl Fn() + Send + Sync + 'static) {
    *WAKER.lock().unwrap_or_else(|e| e.into_inner()) = Some(Arc::new(f));
}

/// Which change of the signals this is: see [`EPOCH`].
pub fn signal_epoch() -> u64 {
    EPOCH.load(Ordering::Acquire)
}

/// Something Rust owns moved on its own: wake the window, and let whatever
/// reads a native value read it again. What [`Signal::set`] does, for state
/// that is not a signal, such as a resource's properties (a playing sound's
/// `currentTime`). Any thread.
pub fn notify() {
    moved();
}

fn moved() {
    EPOCH.fetch_add(1, Ordering::AcqRel);
    let waker = WAKER.lock().unwrap_or_else(|e| e.into_inner()).clone();
    if let Some(wake) = waker {
        wake();
    }
}

/// A value Rust sets and Rux reads. Holds nothing (`none` in Rux) until the
/// first [`Signal::set`].
pub struct Signal<T> {
    value: Mutex<Option<T>>,
}

impl<T> Signal<T> {
    /// An empty signal, for a `static`.
    pub const fn new() -> Self {
        Signal { value: Mutex::new(None) }
    }

    /// Give it `value`, from any thread, and wake the window.
    pub fn set(&self, value: T) {
        *self.value.lock().unwrap_or_else(|e| e.into_inner()) = Some(value);
        moved();
    }

    /// Back to holding nothing: `none` in Rux.
    pub fn clear(&self) {
        *self.value.lock().unwrap_or_else(|e| e.into_inner()) = None;
        moved();
    }

    /// Change the value in place, and wake the window.
    pub fn update(&self, f: impl FnOnce(&mut Option<T>)) {
        f(&mut self.value.lock().unwrap_or_else(|e| e.into_inner()));
        moved();
    }
}

impl<T: Clone> Signal<T> {
    /// What it holds now.
    pub fn get(&self) -> Option<T> {
        self.value.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

impl<T: Clone + IntoRux> Signal<T> {
    /// What it holds, as Rux reads it. What the export's getter calls.
    #[doc(hidden)]
    pub fn __read(&self) -> Result<Any, Error> {
        match self.get() {
            Some(v) => v.into_rux(),
            None => Ok(Any::None),
        }
    }
}

/// What an exported signal may hold. It is `Clone` and nothing more; the
/// trait is here to say so in words when it is not, where the bound on
/// [`Signal::__read`] named a generated method (Tracer's AW-001).
#[diagnostic::on_unimplemented(
    message = "`{Self}` is held by an exported Rux signal, so it needs `#[derive(Clone)]`",
    label = "a signal's value is copied to Rux on every read",
    note = "add `#[derive(Clone)]` to `{Self}`"
)]
pub trait SignalValue: Clone {}

impl<T: Clone> SignalValue for T {}

/// [`Signal::__read`], as the export's getter calls it: a function, so a
/// value that is not `Clone` is reported by [`SignalValue`].
#[doc(hidden)]
pub fn read_exported<T: SignalValue + IntoRux>(signal: &Signal<T>) -> Result<Any, Error> {
    signal.__read()
}

impl<T> Default for Signal<T> {
    fn default() -> Self {
        Signal::new()
    }
}

/// Values Rust sends and Rux receives every one of, in order: a stream of
/// events, a socket's messages, a benchmark's samples (Tracer's AW-011). A
/// [`Signal`] keeps only the newest, which is right for a status and wrong
/// for these.
///
/// Rux reads it as a list, `T[]`, of what was pushed since the document last
/// looked, which is once a frame: an `effect` that reads it runs once for
/// each list that has anything in it, so every value reaches it, and a burst
/// of a thousand costs one update.
pub struct Channel<T> {
    queue: Mutex<Vec<T>>,
}

impl<T> Channel<T> {
    /// An empty channel, for a `static`.
    pub const fn new() -> Self {
        Channel { queue: Mutex::new(Vec::new()) }
    }

    /// Send `value`, from any thread, and wake the window.
    pub fn push(&self, value: T) {
        self.queue.lock().unwrap_or_else(|e| e.into_inner()).push(value);
        moved();
    }

    /// Send every value of `values`, in order, with one wake.
    pub fn extend(&self, values: impl IntoIterator<Item = T>) {
        self.queue.lock().unwrap_or_else(|e| e.into_inner()).extend(values);
        moved();
    }
}

impl<T: IntoRux> Channel<T> {
    /// What was pushed since the last take, taken, as Rux reads it.
    #[doc(hidden)]
    pub fn __take(&self) -> Result<Any, Error> {
        let taken = std::mem::take(&mut *self.queue.lock().unwrap_or_else(|e| e.into_inner()));
        Ok(Any::Array(taken.into_iter().map(IntoRux::into_rux).collect::<Result<_, _>>()?))
    }
}

impl<T> Default for Channel<T> {
    fn default() -> Self {
        Channel::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    static NUMBER: Signal<i64> = Signal::new();

    #[test]
    fn a_signal_holds_nothing_until_set_and_counts_each_set() {
        let before = signal_epoch();
        assert!(matches!(NUMBER.__read(), Ok(Any::None)));
        NUMBER.set(7);
        assert!(matches!(NUMBER.__read(), Ok(Any::Int(7))));
        assert!(signal_epoch() > before);
        NUMBER.clear();
        assert!(matches!(NUMBER.__read(), Ok(Any::None)));
    }
}
