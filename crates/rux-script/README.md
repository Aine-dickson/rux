# rux-script

The Rux script tier: a `rhai` engine that holds the app's live state and
evaluates everything a template asks of it.

The script section's top-level `let` variables persist in a scope, and this
crate evaluates `{{ }}` bindings, `r-if` and `r-for` expressions and `@tap`
handlers against them. It also tracks which signals each binding reads and
which ones a handler writes, which is what lets `rux-runtime` patch just the
affected nodes instead of rebuilding the tree. Native capabilities are exposed
to scripts under the `host::` namespace through the builder.

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
