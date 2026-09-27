//! Every corpus script on both engines: interpreted, and with its functions
//! compiled. They must agree on every value, every change to state, what
//! each run read, and every warning (a failure is one, with its words).

use rux_aot_tests::corpus::CORPUS;
use rux_aot_tests::generated::{install_all, COVERAGE};
use rux_script::{take_timer_requests, take_warnings, Builder, Engine, TimerRequest};

/// What running `what` against `e` produced, as text to compare.
fn run(e: &mut Engine, what: &str) -> String {
    let _ = take_warnings();
    let _ = take_timer_requests();
    let (shown, touched) = match what.strip_prefix('!') {
        Some(handler) => ("(handler)".to_string(), e.run_handler_tracked(handler)),
        None => e.eval_display_tracked(what, &[]),
    };
    let mut touched: Vec<String> = touched.into_iter().collect();
    touched.sort();
    let warned: Vec<String> = take_warnings().into_iter().map(|w| format!("{w:?}")).collect();
    // Timers asked for, without their handles: both engines run on one
    // thread and share its handle count, so the numbers always differ.
    let timers: Vec<String> = take_timer_requests()
        .into_iter()
        .map(|t| match t {
            TimerRequest::Start { ms, body, .. } => format!("start {ms} {body:?}"),
            TimerRequest::Cancel(_) => "cancel".to_string(),
        })
        .collect();
    format!("{shown} | {touched:?} | {warned:?} | {timers:?}")
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
fn the_generator_knows_every_method_that_changes_its_receiver() {
    let mut ours: Vec<&str> = rux_codegen_list().to_vec();
    let mut theirs: Vec<&str> = rux_script::aot::MUTATING.to_vec();
    ours.sort_unstable();
    theirs.sort_unstable();
    assert_eq!(ours, theirs, "rux-codegen's MUTATING has drifted from the interpreter's");
}

fn rux_codegen_list() -> &'static [&'static str] {
    rux_aot_tests::MUTATING_IN_CODEGEN
}

#[test]
fn closure_bodies_run_compiled() {
    install_all();
    for i in [8, 9] {
        let (script, _) = CORPUS[i];
        let e = Builder::new().build(script).expect("builds");
        assert!(e.compiled_closures() > 0, "script {i}: no closure body compiled");
    }
    let mut b = Builder::new();
    b.interpreted();
    assert_eq!(b.build(CORPUS[9].0).expect("builds").compiled_closures(), 0);
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
    for (i, functions, skipped, closures, typed, compiled, handed_back) in COVERAGE {
        println!(
            "script {i}: {functions} functions compiled, {skipped} skipped, {closures} closure bodies, \
             {typed} typed; {compiled} statements compiled, {handed_back} handed back"
        );
    }
}

/// Step 10: every function of the typed script has a typed body except
/// `show`, which makes a string, and `burn` and `burn_deep`, which return
/// nothing; and the untyped scripts none that could
/// not be one.
#[test]
fn typed_bodies_are_written() {
    let typed = |i: usize| COVERAGE.iter().find(|c| c.0 == i).map(|c| (c.1, c.4)).expect("script");
    let (functions, n) = typed(12);
    assert_eq!(n, functions - 3, "the typed script: {n} of {functions} typed");
    // `fib`, `big`, `forever`, `half`, `deep`: all numbers.
    assert_eq!(typed(0).1, 5);
    // State, records, strings: none.
    assert_eq!(typed(1).1, 0);
    assert_eq!(typed(4).1, 0);
}

/// The corpus script of records, whose `fill()` makes the lists it walks.
const RECORDS: usize = 13;

/// What compiling buys, release only (`--ignored`): the same calls on both
/// engines, microseconds per call.
#[test]
#[ignore]
fn cost() {
    install_all();
    let cases: &[(usize, &str, u32)] = &[
        (0, "fib(18)", 5),
        (2, "sum(10000)", 5),
        (2, "odd(20)", 20000),
        (1, "!bump(1)", 20000),
        (4, "open()", 20000),
        // Closures created, `switch`, `try`, `?.` chains, writes through places.
        (4, "titles()", 20000),
        (8, "offset([1, 5, 9], 2)", 20000),
        (8, "adders()", 20000),
        (5, "band(42)", 20000),
        (5, "pet(1)", 20000),
        (6, "first_bad([1, 0, 2, -1, 5])", 20000),
        (6, "kinds()", 20000),
        (7, "local()", 20000),
        (7, "grid()", 20000),
        // Closure bodies compiled (9.6).
        (9, "total()", 20000),
        (9, "names()", 20000),
        (9, "by_n()", 20000),
        (9, "counter()", 20000),
        (9, "deep()", 20000),
        (9, "looped()", 20000),
        // `setInterval` started by compiled code (9.7).
        (10, "twice()", 20000),
        // `int ** int` with a name on the right: a float power before the
        // owner's decision, an `int` and a typed body after.
        (12, "powi(3, 34)", 200000),
        (12, "powi(2, 10)", 200000),
        (12, "powers(10000)", 20),
        // Records (track (c)): the script-cost apps' work, after `fill()`.
        (RECORDS, "sum(items)", 20),
        (RECORDS, "over(few, 40)", 200),
        (RECORDS, "one().name", 20000),
        (RECORDS, "bumped()", 20000),
        (RECORDS, "make(1000).length", 200),
    ];
    // Times per call, and how many times faster compiled is: 3.6x faster
    // means compiled takes a 3.6th of the interpreter's time.
    println!("{:<30} {:>14} {:>14} {:>14}", "call", "interpreted µs", "compiled µs", "speedup");
    for (i, what, times) in cases {
        let (script, _) = CORPUS[*i];
        let mut b = Builder::new();
        b.interpreted();
        let mut engines = [b.build(script).expect("builds"), Builder::new().build(script).expect("builds")];
        // The engines take turns, and each keeps its best round: a machine
        // busy for a moment then slows one round, not one engine.
        let mut took = [f64::MAX; 2];
        for e in engines.iter_mut() {
            if *i == RECORDS {
                run(e, "!fill()");
            }
            run(e, what);
        }
        for _round in 0..7 {
            for (k, e) in engines.iter_mut().enumerate() {
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
                took[k] = took[k].min(t.elapsed().as_nanos() as f64 / *times as f64 / 1000.0);
            }
        }
        println!("{what:<30} {:>14.2} {:>14.2} {:>7.1}x faster", took[0], took[1], took[0] / took[1]);
    }
}

/// Track (c.4): records back from the runtime hold every number as a
/// `float` (the runtime has one number), and may hold their fields in
/// another order. A typed body meeting one gives up and the untyped body
/// runs, so both engines agree, and the answer is the one records made in
/// script give.
#[test]
fn records_from_the_runtime_run_the_untyped_body() {
    use rux_reactive::Value;
    install_all();
    let (script, _) = CORPUS[RECORDS];
    let row = |id: f64, price: f64, qty: f64, swapped: bool| {
        let mut fields = vec![
            ("id".to_string(), Value::Number(id)),
            ("name".to_string(), Value::Text(format!("item {id}"))),
            ("price".to_string(), Value::Number(price)),
            ("qty".to_string(), Value::Number(qty)),
        ];
        if swapped {
            fields.reverse();
        }
        Value::Map(fields)
    };
    for swapped in [false, true] {
        let list = Value::List((0..50).map(|i| row(i as f64, (i % 97) as f64 + 0.5, (i % 7 + 1) as f64, swapped)).collect());
        let locals = [("list".to_string(), list)];
        let mut b = Builder::new();
        b.interpreted();
        let mut interpreted = b.build(script).expect("builds");
        let mut compiled = Builder::new().build(script).expect("builds");
        let a = interpreted.eval_display("sum(list)", &locals);
        let c = compiled.eval_display("sum(list)", &locals);
        assert_eq!(a, c, "swapped: {swapped}");
        // The same fifty made in script.
        compiled.run_handler("fill()");
        assert_eq!(c, compiled.eval_display("sum(items.slice(0, 50))", &[]), "swapped: {swapped}");
    }
}
