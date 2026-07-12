//! `rux` — the Rux command-line entry point.
//!
//! Usage: `rux [path/to/app.rux]`. With no argument it loads the bundled
//! `examples/battery.rux`, so `cargo run` shows something immediately.

use std::path::PathBuf;

fn main() {
    let path = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("examples/battery.rux"));

    rux_shell::run(path);
}
