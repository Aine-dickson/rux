# rux-syntax

The parser for Rux's script language: what goes inside `<script>`, a `{{ }}`
binding and an `@tap` handler. It turns source text into a Rux AST, with a
span on every node, and says what is wrong with text that is not Rux, in Rux's
words.

This is step 2 of the move off rhai described in `docs/11-next.md`. Until
Rux's own interpreter replaces rhai, the script tier still runs through the
fork in `rux-rhai`; this parser is the front door that decides what is valid,
and its AST is what the type checker moves onto next.

## Internal to Rux

This is one crate of the [Rux](https://ruxlang.dev) workspace. It is published
so the toolchain builds from crates.io, not as a library to depend on: it makes
no stability promise of its own and its API moves whenever the runtime needs it
to. The supported entry point is
[`ruxlang`](https://crates.io/crates/ruxlang).

```bash
cargo install ruxlang     # installs a `rux` command
rux run app.rux
```

[ruxlang.dev](https://ruxlang.dev) · [learn](https://ruxlang.dev/learn/) ·
[reference](https://ruxlang.dev/reference/) ·
[try it in a browser](https://ruxlang.dev/playground/)

## Licence

Dual licensed under
[Apache-2.0](https://github.com/Aine-dickson/rux/blob/main/LICENSE-APACHE) or
[MIT](https://github.com/Aine-dickson/rux/blob/main/LICENSE-MIT), at your
option.
