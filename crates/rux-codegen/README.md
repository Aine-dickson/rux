# rux-codegen

Rust generated from a [Rux](https://ruxlang.dev) file's typed IR, for release
builds: step 9 of `docs/11-next.md`. Each function becomes a Rust function
that runs inside Rux's interpreter through `rux_script::aot`, and any
statement it cannot compile yet is handed back to the interpreter.

## Internal to Rux

This is one crate of the [Rux](https://ruxlang.dev) workspace. It is published
so the toolchain builds from crates.io, not as a library to depend on: it makes
no stability promise of its own.
