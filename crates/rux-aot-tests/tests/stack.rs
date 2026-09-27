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

/// How deep typed calls nest (the typed script's `chain`, typed calling
/// typed): the largest `n` for which `chain(n)` answers, found by halving.
fn reached_typed(interpreted: bool) -> i64 {
    install_all();
    let mut b = Builder::new();
    if interpreted {
        b.interpreted();
    }
    let mut e = b.build(CORPUS[CORPUS.len() - 1].0).expect("builds");
    let (mut lo, mut hi) = (1i64, 1 << 20);
    while lo + 1 < hi {
        let mid = (lo + hi) / 2;
        if e.eval_display(&format!("chain({mid})"), &[]).is_empty() {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    lo
}

/// Watchlist 35: a debug build's interpreter frames were so large that the
/// stack budget stopped script calls about 20 deep. Both engines now nest
/// at least 100 calls in a debug build, and a release build meets the depth
/// limit (256) first.
#[test]
fn both_engines_nest_a_hundred_calls() {
    for (name, interpreted) in [("interpreted", true), ("compiled", false)] {
        let d = reached(interpreted);
        println!("{name}: {d} calls deep, about {} bytes of stack per call", 768 * 1024 / d);
        assert!(d >= 100, "{name}: stopped {d} calls deep");
    }
}

/// Typed calls count their depth themselves and look at Rust's stack only
/// every so many levels (10.3): they still stop where the interpreter does
/// in a release build, at the depth limit, and nest far in a debug build.
/// `chain(n - 1) + twice(1)` is a call from inside an expression, which an
/// interpreted debug call takes more of the stack for (96 deep, 2026-09-28).
#[test]
fn typed_calls_stop_where_the_interpreter_does() {
    let ds: Vec<i64> = [("interpreted", true), ("compiled, typed", false)]
        .into_iter()
        .map(|(name, interpreted)| {
            let d = reached_typed(interpreted);
            println!("{name}: chain nests {d} calls deep, about {} bytes of stack per call", 768 * 1024 / d);
            assert!(d >= 64, "{name}: stopped {d} calls deep");
            d
        })
        .collect();
    if !cfg!(debug_assertions) {
        assert_eq!(ds[0], ds[1], "both engines stop at the depth limit");
    }
}
