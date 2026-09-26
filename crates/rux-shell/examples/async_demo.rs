//! A Rust program giving a Rux page two asynchronous host functions, and the
//! page awaiting them from `async fn`s: step 6 of `docs/11-next.md`.
//!
//! ```text
//! cargo run -p rux-shell --example async_demo
//! ```
//!
//! `host::lookup(id)` answers from another thread after a second;
//! `host::store(x)` fails after half a second, which the page catches.

use std::time::Duration;

use rux_reactive::Value;

fn main() {
    rux_runtime::host::register_async("lookup", |args, done| {
        let id = match args.first() {
            Some(Value::Number(n)) => *n,
            _ => 0.0,
        };
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(1));
            let names = ["Ada", "Grace", "Edsger", "Barbara"];
            let name = names[(id as usize) % names.len()];
            done.ok(Value::Map(vec![
                ("name".into(), Value::Text(name.into())),
                ("age".into(), Value::Number(30.0 + id)),
            ]));
        });
    });
    rux_runtime::host::register_async("store", |_, done| {
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(500));
            done.fail("the disk is read-only");
        });
    });
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/async_demo.rux");
    rux_shell::run(path);
}
