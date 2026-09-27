//! Every corpus script on both engines: interpreted, and with its functions
//! compiled. They must agree on every value, every change to state, what
//! each run read, and every warning (a failure is one, with its words).

use rux_aot_tests::corpus::CORPUS;
use rux_aot_tests::generated::{install_all, COVERAGE};
use rux_script::{take_warnings, Builder, Engine};

/// What running `what` against `e` produced, as text to compare.
fn run(e: &mut Engine, what: &str) -> String {
    let _ = take_warnings();
    let (shown, touched) = match what.strip_prefix('!') {
        Some(handler) => ("(handler)".to_string(), e.run_handler_tracked(handler)),
        None => e.eval_display_tracked(what, &[]),
    };
    let mut touched: Vec<String> = touched.into_iter().collect();
    touched.sort();
    let warned: Vec<String> = take_warnings().into_iter().map(|w| format!("{w:?}")).collect();
    format!("{shown} | {touched:?} | {warned:?}")
}

#[test]
fn both_engines_agree_on_the_whole_corpus() {
    install_all();
    for (i, (script, cases)) in CORPUS.iter().enumerate() {
        let mut b = Builder::new();
        b.interpreted();
        let mut interpreted = b.build(script).expect("builds");
        let mut compiled = Builder::new().build(script).expect("builds");
        assert_eq!(interpreted.compiled_functions(), 0);
        assert!(compiled.compiled_functions() > 0, "script {i}: nothing ran compiled");
        for case in *cases {
            let a = run(&mut interpreted, case);
            let b = run(&mut compiled, case);
            assert_eq!(a, b, "script {i}, `{case}`:\ninterpreted: {a}\ncompiled:    {b}");
        }
    }
}

#[test]
fn a_changed_text_runs_interpreted() {
    install_all();
    let (script, _) = CORPUS[0];
    let changed = format!("{script}\n// one more line");
    let e = Builder::new().build(&changed).expect("builds");
    assert_eq!(e.compiled_functions(), 0, "compiled code never runs for text it was not made from");
}

#[test]
fn coverage() {
    for (i, functions, skipped, compiled, handed_back) in COVERAGE {
        println!("script {i}: {functions} functions compiled, {skipped} skipped; {compiled} statements compiled, {handed_back} handed back");
    }
}

/// What compiling buys, release only (`--ignored`): the same calls on both
/// engines, microseconds per call.
#[test]
#[ignore]
fn cost() {
    install_all();
    let cases: &[(usize, &str, u32)] =
        &[(0, "fib(18)", 20), (2, "sum(10000)", 20), (2, "odd(20)", 20000), (1, "!bump(1)", 20000), (4, "open()", 20000)];
    println!("{:<14} {:>14} {:>14} {:>7}", "call", "interpreted µs", "compiled µs", "ratio");
    for (i, what, times) in cases {
        let (script, _) = CORPUS[*i];
        let mut b = Builder::new();
        b.interpreted();
        let mut engines = [b.build(script).expect("builds"), Builder::new().build(script).expect("builds")];
        let mut took = [0.0f64; 2];
        for (k, e) in engines.iter_mut().enumerate() {
            run(e, what);
            let t = std::time::Instant::now();
            for _ in 0..*times {
                match what.strip_prefix('!') {
                    Some(h) => {
                        e.run_handler(h);
                    }
                    None => {
                        std::hint::black_box(e.eval_value(what, &[]));
                    }
                }
            }
            took[k] = t.elapsed().as_nanos() as f64 / *times as f64 / 1000.0;
        }
        println!("{what:<14} {:>14.2} {:>14.2} {:>6.2}x", took[0], took[1], took[1] / took[0]);
    }
}
