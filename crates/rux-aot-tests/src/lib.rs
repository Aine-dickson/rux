//! The step 9 proof of `docs/11-next.md`: the corpus's scripts, compiled to
//! Rust by `rux-codegen` in `build.rs`, and the tests in `tests/` running
//! each on both engines.

pub mod corpus;

/// `rux-codegen`'s own list, reached through the build: the tests compare
/// it with the interpreter's.
pub const MUTATING_IN_CODEGEN: &[&str] = &generated::MUTATING_IN_CODEGEN;

/// The generated modules, `install_all()` and `COVERAGE`.
pub mod generated {
    include!(concat!(env!("OUT_DIR"), "/aot.rs"));
}
