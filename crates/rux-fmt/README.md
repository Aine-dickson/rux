# rux-fmt

The formatter behind `rux fmt`, the playground's Format button and the VS Code
extension's auto-indent.

It does two deliberately different jobs. `<template>` and `<script>` are only
re-indented: leading whitespace is corrected and nothing on a line is
rewritten, wrapped or reordered, because a `@tap` handler is rhai and
rearranging someone's expressions is not this tool's business. `<style>` is
genuinely formatted, one space before the brace, long rules broken one
declaration per line and short ones kept inline, because CSS has a conventional
shape worth enforcing.

The indenter walks each line once as a small state machine, which is what gets
multi-line comments right without a separate pass.

## Internal to Rux

This is one crate of the [Rux](https://ruxlang.dev) workspace. It is published
so the toolchain builds from crates.io, not as a library to depend on: it makes
no stability promise of its own and its API moves whenever the runtime needs it
to. The supported entry point is
[`ruxlang`](https://crates.io/crates/ruxlang).

```bash
cargo install ruxlang     # installs a `rux` command
rux fmt .                 # format in place
rux fmt --check .         # verify only, exits non-zero
```

[ruxlang.dev](https://ruxlang.dev) · [learn](https://ruxlang.dev/learn/) ·
[reference](https://ruxlang.dev/reference/) ·
[try it in a browser](https://ruxlang.dev/playground/)

## Licence

Dual licensed under
[Apache-2.0](https://github.com/Aine-dickson/rux/blob/main/LICENSE-APACHE) or
[MIT](https://github.com/Aine-dickson/rux/blob/main/LICENSE-MIT), at your
option.
