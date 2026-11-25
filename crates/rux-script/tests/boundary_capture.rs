//! What an error boundary needs from the engine: the failure itself, as the
//! `Error` a `catch e` would bind, kept when a run fails; and `retry()`.
//!
//! A failure is still reported as before (the overlay and `rux check` hear of
//! it); this only adds that a boundary can ask what failed.

use rux_reactive::Value;
use rux_script::{faults, last_fault, pop_boundary, push_boundary, take_retries, take_warnings, Builder, Engine};

const SCRIPT: &str = r#"
let count = signal(0);
fn boom(): string { throw { message: "no orders", kind: "network" }; }
fn missing(): int { let m: any = { a: 1 }; return m.b.c; }
"#;

fn engine() -> Engine {
    let mut b = Builder::new();
    b.interpreted();
    b.build(SCRIPT).expect("builds")
}

fn field(v: &Value, name: &str) -> Value {
    match v {
        Value::Map(entries) => entries.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone()).expect("field"),
        other => panic!("not a record: {other:?}"),
    }
}

#[test]
fn a_failing_expression_hands_its_error_to_the_boundary_around_it() {
    let mut e = engine();
    push_boundary();
    let shown = e.eval_display("boom()", &[]);
    let caught = pop_boundary().expect("the failure was kept");
    assert_eq!(shown, "");
    assert_eq!(field(&caught, "message"), Value::Text("no orders".into()));
    assert_eq!(field(&caught, "kind"), Value::Text("network".into()));
    // Still reported, as it always was.
    assert!(!take_warnings().is_empty());
}

#[test]
fn a_language_failure_arrives_as_an_error_with_a_kind() {
    let mut e = engine();
    push_boundary();
    e.eval_display("missing()", &[]);
    let caught = pop_boundary().expect("kept");
    assert_eq!(field(&caught, "kind"), Value::Text("error".into()));
    assert!(matches!(field(&caught, "message"), Value::Text(m) if !m.is_empty()));
    let _ = take_warnings();
}

#[test]
fn a_clean_run_leaves_the_boundary_empty() {
    let mut e = engine();
    push_boundary();
    assert_eq!(e.eval_display("count + 1", &[]), "1");
    assert!(pop_boundary().is_none());
}

#[test]
fn only_the_first_failure_is_kept() {
    let mut e = engine();
    push_boundary();
    e.eval_display("boom()", &[]);
    e.eval_display("missing()", &[]);
    let caught = pop_boundary().unwrap();
    assert_eq!(field(&caught, "message"), Value::Text("no orders".into()));
    let _ = take_warnings();
}

#[test]
fn faults_move_with_a_failure_and_last_fault_reads_it_without_a_frame() {
    let mut e = engine();
    let before = faults();
    e.eval_display("count + 1", &[]);
    assert_eq!(faults(), before);
    e.eval_display("boom()", &[]);
    assert!(faults() > before);
    let last = last_fault().expect("last failure");
    assert_eq!(field(&last, "message"), Value::Text("no orders".into()));
    let _ = take_warnings();
}

#[test]
fn a_handler_that_fails_is_a_fault_too() {
    let mut e = engine();
    let before = faults();
    e.run_handler("boom()");
    assert!(faults() > before);
    let _ = take_warnings();
}

#[test]
fn retry_is_counted_for_the_runtime_to_take() {
    let mut e = engine();
    let _ = take_retries();
    assert!(e.run_handler("retry()"));
    assert_eq!(take_retries(), 1);
    assert_eq!(take_retries(), 0);
}

#[test]
fn an_apps_own_retry_function_is_not_the_builtin() {
    let mut b = Builder::new();
    b.interpreted();
    let mut e = b
        .build("let tries = signal(0);\nfn retry(): string { tries = tries + 1; return \"mine\"; }")
        .expect("builds");
    let _ = take_retries();
    assert_eq!(e.eval_display("retry()", &[]), "mine");
    assert_eq!(take_retries(), 0);
}
