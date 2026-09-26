//! Debug builds run Rux's interpreter beside the fork, and panic where they
//! disagree.
//!
//! Step 5.3 of `docs/11-next.md`, the same proof steps 2 to 4 used: every
//! binding, handler, effect and component script the test suite runs, which
//! is every example, recipe and `/learn` chapter, is run twice, once by each
//! engine, and the two must give the same value, fail together, leave the
//! same state, read the same signals and ask the runtime for the same things
//! (emissions, navigations, timers, element actions, printed lines). The
//! fork's answer is the one used; the interpreter's is only compared.
//!
//! `RUX_SHADOW=0` turns it off, for timing a debug build.

use std::cell::Cell;
use std::collections::HashSet;

use rux_reactive::Value;

use crate::interp::{Interp, V};
use crate::{ElementAction, Nav, TimerRequest};

thread_local! {
    /// While the interpreter runs as the shadow: `print` does not echo.
    pub(crate) static SHADOWING: Cell<bool> = const { Cell::new(false) };
}

/// Whether debug builds shadow the fork.
pub(crate) fn on() -> bool {
    cfg!(debug_assertions) && std::env::var("RUX_SHADOW").map_or(true, |v| v != "0")
}

/// What one run asked the runtime for.
#[derive(Debug, Default, PartialEq)]
pub(crate) struct Effects {
    logs: Vec<String>,
    emissions: Vec<(String, Option<Value>)>,
    navigations: Vec<Nav>,
    timers: Vec<TimerRequest>,
    actions: Vec<ElementAction>,
}

/// How long each sink was, to find what a run added.
#[derive(Clone, Copy)]
pub(crate) struct Marks {
    logs: usize,
    emissions: usize,
    navigations: usize,
    timers: usize,
    actions: usize,
    warnings: usize,
    next_timer: f64,
}

pub(crate) fn marks() -> Marks {
    Marks {
        logs: crate::LOGS.with(|l| l.borrow().len()),
        emissions: crate::EMISSIONS.with(|e| e.borrow().len()),
        navigations: crate::NAVIGATIONS.with(|n| n.borrow().len()),
        timers: crate::TIMER_REQUESTS.with(|t| t.borrow().len()),
        actions: crate::ELEMENT_ACTIONS.with(|a| a.borrow().len()),
        warnings: crate::WARNINGS.with(|w| w.borrow().len()),
        next_timer: crate::NEXT_TIMER_ID.with(Cell::get),
    }
}

/// What was added since `m`, left in place.
pub(crate) fn since(m: Marks) -> Effects {
    Effects {
        logs: crate::LOGS.with(|l| l.borrow()[m.logs..].to_vec()),
        emissions: crate::EMISSIONS.with(|e| e.borrow()[m.emissions..].to_vec()),
        navigations: crate::NAVIGATIONS.with(|n| n.borrow()[m.navigations..].to_vec()),
        timers: crate::TIMER_REQUESTS.with(|t| t.borrow()[m.timers..].to_vec()),
        actions: crate::ELEMENT_ACTIONS.with(|a| a.borrow()[m.actions..].to_vec()),
    }
}

/// What was added since `m`, taken back out, with the timer ids rewound, so
/// the fork's run that follows starts where this one did.
fn undo(m: Marks) -> Effects {
    let out = since(m);
    crate::LOGS.with(|l| l.borrow_mut().truncate(m.logs));
    crate::EMISSIONS.with(|e| e.borrow_mut().truncate(m.emissions));
    crate::NAVIGATIONS.with(|n| n.borrow_mut().truncate(m.navigations));
    crate::TIMER_REQUESTS.with(|t| t.borrow_mut().truncate(m.timers));
    crate::ELEMENT_ACTIONS.with(|a| a.borrow_mut().truncate(m.actions));
    crate::WARNINGS.with(|w| w.borrow_mut().truncate(m.warnings));
    crate::NEXT_TIMER_ID.with(|n| n.set(m.next_timer));
    out
}

/// Run `f` as the shadow: nothing it does reaches the runtime's sinks or the
/// read sets the fork is filling. Hands back what it asked for and what it
/// read.
pub(crate) fn isolated<T>(f: impl FnOnce() -> T) -> (T, Effects, HashSet<String>) {
    let m = marks();
    let reads = crate::READS.with(|r| r.borrow_mut().replace(HashSet::new()));
    let spans = crate::SPANS.with(|s| std::mem::take(&mut *s.borrow_mut()));
    SHADOWING.with(|s| s.set(true));
    let out = f();
    SHADOWING.with(|s| s.set(false));
    crate::SPANS.with(|s| *s.borrow_mut() = spans);
    let read = crate::READS.with(|r| std::mem::replace(&mut *r.borrow_mut(), reads)).unwrap_or_default();
    (out, undo(m), read)
}

/// The same value, as the runtime sees one. `NaN` is itself here.
pub(crate) fn same(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x == y || (x.is_nan() && y.is_nan()),
        (Value::List(x), Value::List(y)) => x.len() == y.len() && x.iter().zip(y).all(|(a, b)| same(a, b)),
        (Value::Map(x), Value::Map(y)) => {
            let mut x: Vec<&(String, Value)> = x.iter().collect();
            let mut y: Vec<&(String, Value)> = y.iter().collect();
            x.sort_by(|a, b| a.0.cmp(&b.0));
            y.sort_by(|a, b| a.0.cmp(&b.0));
            x.len() == y.len() && x.iter().zip(&y).all(|(a, b)| a.0 == b.0 && same(&a.1, &b.1))
        }
        // The fork shows a function value as its own text.
        (Value::Text(x), Value::Text(y)) if x.starts_with("Fn(") && y.starts_with("Fn(") => true,
        _ => a == b,
    }
}

/// Where the two may give different values on purpose: each is a decision
/// of `docs/11-next.md` the interpreter keeps and the fork cannot.
pub(crate) fn allowed(fork: &Value, ours: &Value) -> bool {
    match (fork, ours) {
        // An `int` is an `int`: the fork made every signal a float, so
        // `type_of` of a whole-number signal said "f64" (Numbers, Changed).
        (Value::Text(a), Value::Text(b)) => a == "f64" && b == "i64",
        _ => false,
    }
}

/// Panic, naming what disagreed, in a debug build.
#[track_caller]
pub(crate) fn differ(what: &str, src: &str, fork: impl std::fmt::Debug, ours: impl std::fmt::Debug) -> ! {
    panic!(
        "the interpreter disagrees with the fork on {what}\n  source: {src}\n  fork:   {fork:?}\n  interp: {ours:?}\n\
         (RUX_SHADOW=0 turns the comparison off)"
    );
}

/// Every signal the fork holds, against the interpreter's state.
pub(crate) fn same_state(src: &str, fork: &[(String, Value)], ir: &Interp) {
    for (name, v) in fork {
        match ir.global(name) {
            Some(ours) if same(v, &ours.to_value()) => {}
            ours => differ(&format!("the value of `{name}` afterwards"), src, v, ours.map(V::to_value)),
        }
    }
}
