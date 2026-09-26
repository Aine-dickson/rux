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
