//! Records as slots (step 10, track (c) of `docs/11-next.md`): what an author
//! sees of a record, held to the owner's three decisions. A record shows and
//! walks its fields in the order its type declares them; an optional field
//! never given reads as `none` and is left out while it holds `none`; a write
//! of a field a declared type does not have fails with `kind` `"type"`. The
//! compiled engine is held to the same answers by `rux-aot-tests`, whose
//! corpus script 13 runs these on both engines.

use rux_reactive::Value;
use rux_script::{Builder, Engine};

const SCRIPT: &str = r#"
type Pt = { y: int, x: int, label?: string };
type User = { name: string, pet?: string };
fn pt(): Pt { { x: 1, y: 2 } }
fn bob(): User { { name: "bob" } }
fn labelled(): Pt { let p = pt(); p.label = "a"; p }
fn undeclared(): string { let p: any = pt(); let r = "wrote"; try { p.z = 1; } catch e { r = e.kind; } r }
fn grown(): string { let m: any = { b: 1 }; m.set("a", 2); `${m}` }
fn caught(): string { let r = ""; try { let n = [1][5]; } catch e { r = `${keys(e)}`; } r }
"#;

fn engine() -> Engine {
    let mut b = Builder::new();
    b.interpreted();
    b.build(SCRIPT).expect("builds")
}

fn shown(e: &mut Engine, what: &str) -> String {
    e.eval_display(what, &[])
}

#[test]
fn fields_show_in_the_order_the_type_declares() {
    let mut e = engine();
    assert_eq!(shown(&mut e, "`${pt()}`"), "y: 2, x: 1");
    assert_eq!(shown(&mut e, "keys(pt())"), "y, x");
    assert_eq!(shown(&mut e, "values(labelled())"), "2, 1, a");
    // Leaving script for the runtime keeps the order, and so does coming back.
    let v = e.eval_value("pt()", &[]).expect("a value");
    assert_eq!(v, Value::Map(vec![("y".into(), Value::Number(2.0)), ("x".into(), Value::Number(1.0))]));
    let locals = [("p".to_string(), v)];
    assert_eq!(e.eval_display("`${p}`", &locals), "y: 2, x: 1");
}

#[test]
fn a_record_nothing_declares_keeps_the_order_written_and_a_map_stays_sorted() {
    let mut e = engine();
    assert_eq!(shown(&mut e, "`${ { b: 1, a: 2 } }`"), "b: 1, a: 2");
    // A new key makes it the map it always was.
    assert_eq!(shown(&mut e, "grown()"), "a: 2, b: 1");
}

#[test]
fn an_optional_field_never_given_is_none_and_not_shown() {
    let mut e = engine();
    assert_eq!(shown(&mut e, "bob().pet ?? \"no pet\""), "no pet");
    assert_eq!(shown(&mut e, "`${bob()}`"), "name: bob");
    assert_eq!(shown(&mut e, "keys(bob())"), "name");
    assert_eq!(shown(&mut e, "\"pet\" in bob()"), "false");
    // Given one, it is shown, in its place.
    assert_eq!(shown(&mut e, "`${labelled()}`"), "y: 2, x: 1, label: a");
}

#[test]
fn a_field_the_type_does_not_declare_cannot_be_written() {
    let mut e = engine();
    assert_eq!(shown(&mut e, "undeclared()"), "type");
}

#[test]
fn records_are_equal_whatever_order_they_were_written_in() {
    let mut e = engine();
    assert_eq!(shown(&mut e, "pt() == { x: 1, y: 2 }"), "true");
    assert_eq!(shown(&mut e, "pt() == { y: 2, x: 1 }"), "true");
    assert_eq!(shown(&mut e, "pt() == { y: 2, x: 5 }"), "false");
    assert_eq!(shown(&mut e, "bob() == { name: \"bob\" }"), "true");
}

#[test]
fn an_error_is_a_record_of_message_then_kind() {
    let mut e = engine();
    assert_eq!(shown(&mut e, "caught()"), "message, kind");
    assert_eq!(shown(&mut e, "`${Err(\"no\")}`"), "ok: false, error: no");
}

/// A map gains a key with `set`; `=` writes a key that is there, and for a
/// missing one says to use `set` (owner, 2026-09-28; watchlist 40).
#[test]
fn a_map_gains_a_key_with_set() {
    let mut b = Builder::new();
    b.interpreted();
    let mut e = b
        .build(
            r#"
fn added(): string { let d: { [string]: int } = { b: 1 }; d.set("a", 2); d["b"] = 5; `${d}` }
fn assigned(): string { let d: { [string]: int } = { b: 1 }; let r = ""; try { d["c"] = 3; } catch e { r = e.message; } r }
"#,
        )
        .expect("builds");
    assert_eq!(e.eval_display("added()", &[]), "a: 2, b: 5");
    assert!(e.eval_display("assigned()", &[]).contains(".set(\"c\", value)"), "{}", e.eval_display("assigned()", &[]));
}
