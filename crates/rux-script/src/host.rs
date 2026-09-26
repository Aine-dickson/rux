//! Host functions a script awaits, and how their answers come back.
//!
//! Step 6 of `docs/11-next.md`. `await host::name(args)` inside an `async fn`
//! calls a function registered here with the arguments and a [`Completer`],
//! and the function waits there until the completer is used. The Rust side
//! decides where the work runs: on a thread of its own, on an executor it
//! already has, or at once. The completer can be used from any thread; the
//! answer is queued, the waker set with [`set_waker`] is called, and the
//! runtime goes on with the function on the script's thread when it next
//! settles its tasks.
//!
//! Registered process-wide, since an app registers its host functions once and
//! every document it loads may await them. Step 8 turns these into `native`
//! modules; the mechanism stays.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use rux_reactive::Value;

/// A host function a script can await: called with the arguments and a
/// [`Completer`] for the answer.
pub type AsyncFn = Arc<dyn Fn(Vec<Value>, Completer) + Send + Sync>;

type Waker = Arc<dyn Fn() + Send + Sync>;

static REGISTRY: Mutex<Vec<(String, AsyncFn)>> = Mutex::new(Vec::new());
static ANSWERS: Mutex<Vec<(u64, Result<Value, String>)>> = Mutex::new(Vec::new());
static WAKER: Mutex<Option<Waker>> = Mutex::new(None);
/// Calls whose task is gone: an answer for one is dropped when it comes.
static ABANDONED: Mutex<Option<HashSet<u64>>> = Mutex::new(None);
static NEXT: AtomicU64 = AtomicU64::new(1);

/// Register `host::<name>(…)` for every script loaded after this, to be
/// awaited from an `async fn`. A second registration of a name replaces the
/// first.
pub fn register_async(name: &str, f: impl Fn(Vec<Value>, Completer) + Send + Sync + 'static) {
    let mut r = REGISTRY.lock().unwrap_or_else(|e| e.into_inner());
    r.retain(|(n, _)| n != name);
    r.push((name.to_string(), Arc::new(f)));
}

/// What is registered, for a new interpreter.
pub(crate) fn registered() -> HashMap<String, AsyncFn> {
    REGISTRY.lock().unwrap_or_else(|e| e.into_inner()).iter().cloned().collect()
}

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
pub(crate) fn take_answers(wanted: impl Fn(u64) -> bool) -> Vec<(u64, Result<Value, String>)> {
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

/// The one way a host function answers: [`Completer::ok`] with the value, or
/// [`Completer::fail`] with what went wrong, which the `await` throws. One
/// dropped without either answers with an error saying so, so a script never
/// waits forever on a function that forgot.
pub struct Completer {
    ticket: u64,
    name: String,
    done: bool,
}

impl Completer {
    pub(crate) fn new(ticket: u64, name: &str) -> Self {
        Completer { ticket, name: name.to_string(), done: false }
    }

    /// Answer with `value`.
    pub fn ok(self, value: Value) {
        self.complete(Ok(value))
    }

    /// Answer with an error: the `await` throws `message`.
    pub fn fail(self, message: impl Into<String>) {
        self.complete(Err(message.into()))
    }

    pub fn complete(mut self, answer: Result<Value, String>) {
        self.send(answer);
    }

    fn send(&mut self, answer: Result<Value, String>) {
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
        let name = self.name.clone();
        self.send(Err(format!("host::{name} finished without answering")));
    }
}
