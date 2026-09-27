//! The step 9 proof of `docs/11-next.md`: the corpus's scripts, compiled to
//! Rust by `rux-codegen` in `build.rs`, and the tests in `tests/` running
//! each on both engines.

pub mod corpus;

/// The generated modules, `install_all()` and `COVERAGE`.
pub mod generated {
    include!(concat!(env!("OUT_DIR"), "/aot.rs"));
}
