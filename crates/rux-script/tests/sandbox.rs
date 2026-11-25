//! The sandbox half of the capability model: what a document opened from
//! somewhere that is not the app's own gets (roadmap v0.10 item 2). A file
//! never marks itself trusted; the opener hands the engine a [`Sandbox`], and
//! with none the document is trusted, as it has always been.

use rux_script::{take_warnings, Builder, Engine, Limits, Sandbox};

const SCRIPT: &str = "let level = signal(0);\n";
const SPIN: &str = "let n = 0; while n < 10000 { n += 1; }";

fn engine_with(sandbox: Option<Sandbox>) -> Result<Engine, rux_script::ScriptError> {
    let mut b = Builder::new();
    b.interpreted();
    if let Some(s) = sandbox {
        b.sandbox(s);
    }
    b.build(SCRIPT)
}

fn limits(f: impl FnOnce(&mut Limits)) -> Sandbox {
    let mut s = Sandbox::new();
    f(&mut s.limits);
    s
}

#[test]
fn a_sandbox_stops_a_run_at_its_own_step_budget() {
    let mut e = engine_with(Some(limits(|l| l.steps_per_run = 1000))).expect("builds");
    let _ = take_warnings();
    assert!(!e.run_handler(SPIN), "ten thousand steps do not fit in a thousand");
    let said: Vec<String> = take_warnings().into_iter().map(|w| w.message).collect();
    assert!(said.iter().any(|m| m.contains("without finishing")), "{said:?}");
    // The message names the sandbox's budget, not the default one.
    assert!(said.iter().any(|m| m.contains("1000 steps")), "{said:?}");
    // And the engine is still usable afterwards.
    e.run_handler("level = 5");
    assert_eq!(e.eval_display("level", &[]), "5");
}

#[test]
fn the_same_run_finishes_with_no_sandbox() {
    let mut e = engine_with(None).expect("builds");
    let _ = take_warnings();
    assert!(e.run_handler(SPIN));
    assert!(take_warnings().is_empty());
}

#[test]
fn a_default_sandbox_keeps_the_usual_budget() {
    let mut e = engine_with(Some(Sandbox::new())).expect("builds");
    let _ = take_warnings();
    assert!(e.run_handler(SPIN));
}

#[test]
fn a_document_past_the_size_limit_is_refused_at_load() {
    let big = format!("{SCRIPT}let t = \"{}\";\n", "x".repeat(4096));
    let mut b = Builder::new();
    b.interpreted();
    b.sandbox(limits(|l| l.max_source_bytes = 1024));
    let err = b.build(&big).err().expect("refused");
    assert!(err.message.contains("1024"), "names the limit: {}", err.message);
    // The same text loads with no sandbox.
    let mut b = Builder::new();
    b.interpreted();
    assert!(b.build(&big).is_ok());
}

#[test]
fn a_literal_nested_past_the_limit_is_refused_without_overflowing_the_stack() {
    let deep = format!("{SCRIPT}let a = {}1{};\n", "[".repeat(10_000), "]".repeat(10_000));
    let mut b = Builder::new();
    b.interpreted();
    b.sandbox(limits(|l| l.max_nesting = 64));
    let err = b.build(&deep).err().expect("refused");
    assert!(err.message.contains("64"), "names the limit: {}", err.message);
}

#[test]
fn nesting_within_the_limit_loads() {
    let ok = format!("{SCRIPT}let a = {}1{};\n", "[".repeat(10), "]".repeat(10));
    let mut b = Builder::new();
    b.interpreted();
    b.sandbox(limits(|l| l.max_nesting = 64));
    assert!(b.build(&ok).is_ok());
}
