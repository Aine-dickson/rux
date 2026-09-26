//! How the answer to an awaited native call comes back.
//!
//! Step 6 of `docs/11-next.md` built this for `host::` functions; step 8
//! made those native modules (`crate::native`). An `await` of an `async`
//! native export starts its future where `rux_native::spawn` runs futures,
//! with a [`Completer`] for its answer. The completer can be used from any
//! thread; the answer is queued, the waker set with [`set_waker`] is called,
//! and the runtime goes on with the function on the script's thread when it
//! next settles its tasks.

use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use rux_native::{Any, Error};

type Waker = Arc<dyn Fn() + Send + Sync>;
type Answer = Result<Any, Error>;

static ANSWERS: Mutex<Vec<(u64, Answer)>> = Mutex::new(Vec::new());
static WAKER: Mutex<Option<Waker>> = Mutex::new(None);
/// Calls whose task is gone: an answer for one is dropped when it comes.
static ABANDONED: Mutex<Option<HashSet<u64>>> = Mutex::new(None);
static NEXT: AtomicU64 = AtomicU64::new(1);

/// Call `f` whenever an answer arrives, from whichever thread gave it: how a
/// window learns it has work to do while it sleeps. The shell sends itself an
/// event from here.
pub fn set_waker(f: impl Fn() + Send + Sync + 'static) {
    *WAKER.lock().unwrap_or_else(|e| e.into_inner()) = Some(Arc::new(f));
}

/// A fresh number for a waiting call or a task, never given out twice in the
/// process, so two documents cannot mistake each other's answers.
pub(crate) fn next_id() -> u64 {
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// Take the answers whose number `wanted` accepts, leaving the rest queued
/// for whoever is waiting on them.
pub(crate) fn take_answers(wanted: impl Fn(u64) -> bool) -> Vec<(u64, Answer)> {
    let mut q = ANSWERS.lock().unwrap_or_else(|e| e.into_inner());
    let (mine, rest): (Vec<_>, Vec<_>) = q.drain(..).partition(|(t, _)| wanted(*t));
    *q = rest;
    mine
}

/// Nobody waits for `tickets` any more: drop an answer already queued for
/// one, and any that comes later.
pub(crate) fn abandon(tickets: impl IntoIterator<Item = u64>) {
    let tickets: HashSet<u64> = tickets.into_iter().collect();
    if tickets.is_empty() {
        return;
    }
    let mut q = ANSWERS.lock().unwrap_or_else(|e| e.into_inner());
    let answered: HashSet<u64> = q.iter().map(|(t, _)| *t).filter(|t| tickets.contains(t)).collect();
    q.retain(|(t, _)| !tickets.contains(t));
    drop(q);
    let mut a = ABANDONED.lock().unwrap_or_else(|e| e.into_inner());
    let set = a.get_or_insert_with(HashSet::new);
    // One answered already will not be answered again; the rest may be.
    set.extend(tickets.into_iter().filter(|t| !answered.contains(t)));
}

/// Whether any answer is queued, for anyone.
pub fn has_answers() -> bool {
    !ANSWERS.lock().unwrap_or_else(|e| e.into_inner()).is_empty()
}

/// The one way an awaited call answers: [`Completer::complete`] with what
/// the future gave. One dropped without answering answers with an error
/// saying so, so a script never waits forever on a future that was lost.
pub struct Completer {
    ticket: u64,
    name: String,
    done: bool,
}

impl Completer {
    pub(crate) fn new(ticket: u64, name: &str) -> Self {
        Completer { ticket, name: name.to_string(), done: false }
    }

    pub fn complete(mut self, answer: Answer) {
        self.send(answer);
    }

    fn send(&mut self, answer: Answer) {
        if self.done {
            return;
        }
        self.done = true;
        if let Some(set) = ABANDONED.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
            if set.remove(&self.ticket) {
                return;
            }
        }
        ANSWERS.lock().unwrap_or_else(|e| e.into_inner()).push((self.ticket, answer));
        let waker = WAKER.lock().unwrap_or_else(|e| e.into_inner()).clone();
        if let Some(wake) = waker {
            wake();
        }
    }
}

impl Drop for Completer {
    fn drop(&mut self) {
        let name = self.name.replace('/', "::");
        self.send(Err(Error::new("error", format!("{name} finished without answering"))));
    }
}

/// Start `future`, the call waited on under `ticket`, where futures run, its
/// answer (or its panic) going to the queue.
pub(crate) fn start(ticket: u64, name: &str, future: rux_native::BoxFuture) {
    let done = Completer::new(ticket, name);
    rux_native::spawn(Box::pin(async move {
        let answer = match rux_native::CatchUnwind::new(future).await {
            Ok(a) => a,
            Err(p) => Err(Error::new("panic", p)),
        };
        done.complete(answer);
    }));
}
