//! A Rust program giving a Rux page two asynchronous native functions, and
//! the page awaiting them from `async fn`s: steps 6 and 8 of
//! `docs/11-next.md`.
//!
//! ```text
//! cargo run -p rux-shell --example async_demo
//! ```
//!
//! The module is registered by hand, as a program that runs the shell itself
//! does; an app's `native/` crate gets the same from `#[rux::export]`.
//! `demo.lookup(id)` answers after a second, off the UI thread;
//! `demo.store(x)` fails after half a second, which the page catches.

use std::time::Duration;

use rux_runtime::native::{Any, Call, Error, Export, Module};

fn main() {
    Module::new("demo")
        .export(Export::record("User", &[("name", "string"), ("age", "int")]))
        .export(Export::function(
            "lookup",
            &[("id", "int")],
            "User",
            Call::future(|args| async move {
                let id = match args.first() {
                    Some(Any::Int(n)) => *n,
                    _ => 0,
                };
                std::thread::sleep(Duration::from_secs(1));
                let names = ["Ada", "Grace", "Edsger", "Barbara"];
                let mut user = std::collections::BTreeMap::new();
                user.insert("name".to_string(), Any::Str(names[id.rem_euclid(4) as usize].into()));
                user.insert("age".to_string(), Any::Int(30 + id));
                Ok(Any::Map(user))
            }),
        ))
        .export(Export::function(
            "store",
            &[("n", "int")],
            "void",
            Call::future(|_| async {
                std::thread::sleep(Duration::from_millis(500));
                Err(Error::new("ReadOnly", "the disk is read-only"))
            }),
        ))
        .install();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/async_demo.rux");
    rux_shell::run(path);
}
