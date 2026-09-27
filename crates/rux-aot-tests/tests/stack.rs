//! Recursion to the depth limit on each engine, on a test thread's own
//! (2 MB) stack: the limit has to be reached before Rust's stack is.

use rux_aot_tests::generated::install_all;
use rux_script::Builder;

const SCRIPT: &str = "fn deep(n: int): int { deep(n + 1) }\n";

#[test]
fn interpreted_recursion_stops_at_the_limit() {
    let mut b = Builder::new();
    b.interpreted();
    let mut e = b.build(SCRIPT).expect("builds");
    assert_eq!(e.eval_display("deep(0)", &[]), "");
}

#[test]
fn compiled_recursion_stops_at_the_limit() {
    install_all();
    let mut e = Builder::new().build(SCRIPT).expect("builds");
    assert_eq!(e.eval_display("deep(0)", &[]), "");
}
