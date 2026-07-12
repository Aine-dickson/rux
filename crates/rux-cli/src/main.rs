//! `rux` — the Rux command-line entry point.
//!
//! Milestone M0: no arguments yet; just opens the shell window. The
//! `rux run <app.rux>` interface arrives once there's a document to load.

fn main() {
    rux_shell::run();
}
