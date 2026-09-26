//! What Rux's interpreter costs on the work a document's script does: a
//! binding over a long list, a handler, a function that calls itself, a
//! loop. Step 5 of `docs/11-next.md` benchmarks the interpreter from its
//! first commit; the runtime-level numbers are the `RUX_PROFILE` runs in
//! `rux-harness/script-cost/`.
//!
//! Two columns: through [`Engine`], which is how the runtime runs script
//! (text and locals handed in, values converted at the seam), and through
//! [`Interp`] directly. The difference is what the seam costs.
//!
//! The fork's numbers, measured the same way when the interpreter first ran
//! beside it (9a461b7, one release run, µs per pass): sum 37 209, filter
//! 21 830, row binding 3 432, handler 1 523, fib(18) 10 462, loop 3 062. The
//! interpreter took 0.22x to 0.69x of those.
//!
//! Ignored by default, since a timing is not a pass or a fail:
//!
//! ```text
//! cargo test --release -p rux-script --test interp_cost -- --ignored --nocapture
//! ```

use std::time::Instant;

use rux_reactive::Value;
use rux_script::interp::Interp;
use rux_script::{Builder, Engine};

const SCRIPT: &str = r#"
let items = signal([]);
let count = signal(0);
let total = signal(0.0);
fn fill(n) { let i = 0; while i < n { items.push({ id: i, title: "item " + i, price: i * 0.5, done: i % 3 == 0 }); i++; } }
fn fib(n) { if n < 2 { n } else { fib(n - 1) + fib(n - 2) } }
fill(1000);
"#;

/// Each case: what it is, its text, the locals handed in, and how many
/// times one pass runs it.
fn cases() -> Vec<(&'static str, &'static str, Vec<(String, Value)>, u32)> {
    let row = vec![(
        "item".to_string(),
        Value::Map(vec![
            ("title".into(), Value::Text("a row".into())),
            ("price".into(), Value::Number(2.5)),
            ("done".into(), Value::Bool(true)),
        ]),
    )];
    vec![
        ("sum binding", "items.reduce((s, i) => s + i.price, 0.0)", Vec::new(), 20),
        ("filter binding", "items.filter(i => i.done).length", Vec::new(), 20),
        ("row binding, x1000", "item.title + \" costs \" + item.price", row, 1000),
        ("handler, x1000", "count += 1; total = total + 2.5;", Vec::new(), 1000),
        ("fib(18)", "fib(18)", Vec::new(), 1),
        ("loop to 10000", "let s = 0; for i in 0..10000 { s += i; } s", Vec::new(), 1),
    ]
}

#[test]
#[ignore]
fn the_interpreter_through_the_engine_and_directly() {
    const ROUNDS: u32 = 5;
    let mut engine: Engine = Builder::new().build(SCRIPT).expect("the engine builds");
    let mut ours = Interp::from_script(SCRIPT).expect("the interpreter builds");
    println!("{:<20} {:>12} {:>12} {:>8}", "case", "engine µs", "interp µs", "ratio");
    for (what, src, locals, times) in cases() {
        // Once each first, so both have compiled and cached the text.
        engine.eval_value(src, &locals);
        ours.run(src, &locals, true).unwrap().0.unwrap();
        let t = Instant::now();
        for _ in 0..ROUNDS * times {
            std::hint::black_box(engine.eval_value(src, &locals));
        }
        let through = t.elapsed().as_nanos() as f64 / ROUNDS as f64 / 1000.0;
        let t = Instant::now();
        for _ in 0..ROUNDS * times {
            let _ = std::hint::black_box(ours.run(src, &locals, true).unwrap());
        }
        let mine = t.elapsed().as_nanos() as f64 / ROUNDS as f64 / 1000.0;
        println!("{what:<20} {through:>12.1} {mine:>12.1} {:>7.2}x", mine / through);
    }
}

/// What an `async fn` costs to start, stop at an `await` and go on (step 6
/// of `docs/11-next.md`): a thousand tasks, each answered at once by the
/// host, then each resumed. Compared with the same body as an ordinary
/// function, which is what the stopping and going on add.
#[test]
#[ignore]
fn an_async_fn_started_stopped_and_resumed() {
    const N: u32 = 1000;
    // Answered at once, on this thread: what is timed is the interpreter's
    // stopping and going on, not a thread starting.
    rux_native::set_spawner(|task| rux_native::block_on(task));
    rux_native::Module::new("cost_now")
        .export(rux_native::Export::function(
            "now",
            &[],
            "float",
            rux_native::Call::future(|_| async { Ok(rux_native::Any::Float(1.0)) }),
        ))
        .install();
    let script = "let n = signal(0);\n\
                  async fn step() { n += 1; let v = await cost.now(); n += v; }\n\
                  fn plain() { n += 1; n += 1; }\n";
    let alias = rux_script::link::Alias {
        local: "cost".into(),
        target: rux_script::link::Target::Module("native/cost_now".into()),
        line: 1,
    };
    let mut b = Builder::new();
    b.aliases(vec![alias]);
    let mut engine: Engine = b.build(script).expect("the engine builds");
    engine.run_handler("step()");
    engine.run_handler("plain()");
    let t = Instant::now();
    for _ in 0..N {
        engine.run_handler("plain()");
    }
    let plain = t.elapsed().as_nanos() as f64 / N as f64 / 1000.0;
    let t = Instant::now();
    for _ in 0..N {
        engine.run_handler("step()");
    }
    let started = t.elapsed().as_nanos() as f64 / N as f64 / 1000.0;
    let t = Instant::now();
    let mut resumed = 0;
    while resumed < N + 1 {
        for id in engine.collect_answers() {
            engine.resume_task(id);
            resumed += 1;
        }
    }
    let going_on = t.elapsed().as_nanos() as f64 / N as f64 / 1000.0;
    println!("plain fn call     {plain:>8.2} µs");
    println!("async fn start    {started:>8.2} µs (to its await)");
    println!("async fn resume   {going_on:>8.2} µs (answer taken, rest run)");
}

/// A store against the document's own state (step 7 of `docs/11-next.md`):
/// the same handler and binding, once on the document's signal and function,
/// once through a module linked in. Linking happens when text is first
/// compiled, so a run of either should cost the same.
#[test]
#[ignore]
fn a_store_against_the_documents_own_state() {
    use rux_script::link::{Alias, Target};
    use rux_script::ModuleSource;
    const ROUNDS: u32 = 5;
    const TIMES: u32 = 1000;
    let own = "let n = signal(0);\nfn bump() { n += 1; }";
    let mut doc: Engine = Builder::new().build(own).expect("builds");
    let mut b = Builder::new();
    b.module(ModuleSource {
        name: "stores/count".into(),
        script: "export let n = signal(0);\nexport fn bump() { n += 1; }".into(),
        aliases: Vec::new(),
        types: Vec::new(),
    });
    b.aliases(vec![Alias { local: "count".into(), target: Target::Module("stores/count".into()), line: 1 }]);
    let mut store: Engine = b.build("").expect("builds");
    println!("{:<24} {:>12} {:>12} {:>8}", "case, x1000", "document µs", "store µs", "ratio");
    for (what, mine, theirs) in [("call that writes", "bump()", "count.bump()"), ("read in a binding", "n + 1", "count.n + 1")] {
        doc.eval_value(mine, &[]);
        store.eval_value(theirs, &[]);
        let t = Instant::now();
        for _ in 0..ROUNDS * TIMES {
            std::hint::black_box(doc.eval_value(mine, &[]));
        }
        let a = t.elapsed().as_nanos() as f64 / ROUNDS as f64 / 1000.0;
        let t = Instant::now();
        for _ in 0..ROUNDS * TIMES {
            std::hint::black_box(store.eval_value(theirs, &[]));
        }
        let b = t.elapsed().as_nanos() as f64 / ROUNDS as f64 / 1000.0;
        println!("{what:<24} {a:>12.1} {b:>12.1} {:>7.2}x", b / a);
    }
}

/// What calling Rust costs (step 8 of `docs/11-next.md`): a synchronous
/// native function, and a native record's method, against the same work
/// written as a script `fn`. The difference is the boundary: arguments into
/// `rux_native::Any`, the registry's code, and the answer back.
#[test]
#[ignore]
fn a_native_call_against_a_script_call() {
    const N: u32 = 10_000;
    let add = rux_native::Call::sync(|args| match (&args[0], &args[1]) {
        (rux_native::Any::Int(a), rux_native::Any::Int(b)) => Ok(rux_native::Any::Int(a + b)),
        _ => Err(rux_native::Error::new("type", "ints")),
    });
    rux_native::Module::new("cost_add")
        .export(rux_native::Export::function("add", &[("a", "int"), ("b", "int")], "int", add))
        .install();
    let script = "let n = signal(0);\nfn plain(a: int, b: int): int { a + b }\n\
                  fn native_loop() { let i = 0; while i < 1000 { n = cost.add(n, 1); i += 1; } }\n\
                  fn script_loop() { let i = 0; while i < 1000 { n = plain(n, 1); i += 1; } }\n";
    let alias = rux_script::link::Alias {
        local: "cost".into(),
        target: rux_script::link::Target::Module("native/cost_add".into()),
        line: 1,
    };
    let mut b = Builder::new();
    b.aliases(vec![alias]);
    let mut engine: Engine = b.build(script).expect("the engine builds");
    for (what, handler) in [("script fn call", "script_loop()"), ("native fn call", "native_loop()")] {
        engine.run_handler(handler);
        let t = Instant::now();
        for _ in 0..N / 1000 {
            engine.run_handler(handler);
        }
        let per = t.elapsed().as_nanos() as f64 / N as f64 / 1000.0;
        println!("{what:<16} {per:>8.3} µs per call (with the loop around it)");
    }
}
