# rux-bindgen

Reads a [Rux](https://ruxlang.dev) app's `native/` Rust crate for what it
exports with `#[rux::export]`, and says what each Rust type is in Rux. Used
by the `#[rux::export]` macro and by the `rux` CLI, so what the checker is
told and what the compiled code does come from one mapping. Step 8 of
`docs/11-next.md`.

## Internal to Rux

This is one crate of the [Rux](https://ruxlang.dev) workspace. It is published
so the toolchain builds from crates.io, not as a library to depend on: it makes
no stability promise of its own. An app depends on `rux-native`.
