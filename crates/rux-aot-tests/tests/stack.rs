//! Recursion to the depth limit on each engine, on a test thread's own
//! (2 MB) stack: the limit has to be reached before Rust's stack is. The
//! script is the corpus's, so the compiled engine really runs compiled.

use rux_aot_tests::corpus::CORPUS;
use rux_aot_tests::generated::install_all;
use rux_script::Builder;

/// The corpus script that recurses and records its depth in `d`.
const DEEP: usize = 11;

/// How deep a run gets before it is stopped: every call records its depth,
/// and the last one recorded is read back.
fn reached(interpreted: bool) -> i64 {
    install_all();
    let mut b = Builder::new();
    if interpreted {
        b.interpreted();
    }
    let mut e = b.build(CORPUS[DEEP].0).expect("builds");
    assert_eq!(e.compiled_functions() > 0, !interpreted);
    assert_eq!(e.eval_display("deep(1)", &[]), "", "it is stopped, not answered");
    e.eval_display("d", &[]).parse().expect("a number")
}

/// Watchlist 35: a debug build's interpreter frames were so large that the
/// stack budget stopped script calls about 20 deep. Both engines now nest
/// at least 100 calls in a debug build, and a release build meets the depth
/// limit (128) first.
#[test]
fn both_engines_nest_a_hundred_calls() {
    for (name, interpreted) in [("interpreted", true), ("compiled", false)] {
        let d = reached(interpreted);
        println!("{name}: {d} calls deep, about {} bytes of stack per call", 768 * 1024 / d);
        assert!(d >= 100, "{name}: stopped {d} calls deep");
    }
}
