# rux-parser

The Rux template parser: it splits a `.rux` single-file component into its
`<template>`, `<style>` and `<script>` sections, then parses the template
itself.

The template grammar is XML-shaped but is not XML, because it has to accept
Rux's own attribute spellings (`@tap`, `:device`, `r-for`, `r-if`) and `{{ }}`
interpolations. That is the one stage of the pipeline with no off-the-shelf
answer, so it is hand-rolled. Interpolations and directives come out as raw
strings; compiling them into bindings is `rux-script`'s job, not this crate's.

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
