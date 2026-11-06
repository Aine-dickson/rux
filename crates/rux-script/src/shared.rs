//! Stores shared by every document in the process.
//!
//! A store is a `.rux` module with signals in it. One document loads it once,
//! and every importer in that document sees the same state. With child
//! windows an app has several documents, and a store has to be the same state
//! in all of them, so the values live here, on the UI thread, and each
//! document's engine writes the ones it changes and reads the ones another
//! changed.
//!
//! The values are the interpreter's own (`V`), not the runtime's `Value`: an
//! `int` stays an `int`, and a record keeps its shape. A function value is not
//! shared (it points into the engine that made it), and neither is an element
//! handle; a signal holding one stays per document.
//!
//! A store starts once per app. A second document finds the values already
//! here and adopts them; the module's top level still runs in each document
//! (its functions and computeds need it), so a side effect in it runs again.

use std::cell::RefCell;
use std::collections::HashMap;

use crate::interp::V;

#[derive(Default)]
struct Hub {
    values: HashMap<String, V>,
    /// Moves on every change, so a document can tell with one compare that
    /// there is nothing to read.
    epoch: u64,
}

thread_local! {
    static HUB: RefCell<Hub> = RefCell::new(Hub::default());
}

/// Where the shared stores are now.
pub fn epoch() -> u64 {
    HUB.with(|h| h.borrow().epoch)
}

/// Forget every shared value: a hot reload starts the stores again.
pub fn reset() {
    HUB.with(|h| {
        let mut h = h.borrow_mut();
        h.values.clear();
        h.epoch += 1;
    });
}

/// Whether `v` can be handed to another engine.
pub(crate) fn shareable(v: &V) -> bool {
    match v {
        V::Fn(_) | V::Element(_) => false,
        V::Array(items) => items.iter().all(shareable),
        V::Map(m) => m.values().all(shareable),
        V::Rec(r) => r.vals.iter().all(shareable),
        _ => true,
    }
}

/// Put `v` under `name`. Says `(was, now)`, the epoch before and after, when
/// that changed anything.
pub(crate) fn put(name: &str, v: V) -> Option<(u64, u64)> {
    HUB.with(|h| {
        let mut h = h.borrow_mut();
        if h.values.get(name).is_some_and(|old| *old == v) {
            return None;
        }
        let was = h.epoch;
        h.values.insert(name.to_string(), v);
        h.epoch += 1;
        Some((was, h.epoch))
    })
}

pub(crate) fn get(name: &str) -> Option<V> {
    HUB.with(|h| h.borrow().values.get(name).cloned())
}

/// Every shared value, for a document to compare with its own.
pub(crate) fn all() -> Vec<(String, V)> {
    HUB.with(|h| h.borrow().values.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
}
