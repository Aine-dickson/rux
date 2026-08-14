# rux-reactive

`Value`, the untyped representation that the rest of Rux passes around for
bindings, `r-for` locals and component props.

The crate began as Rux's reactivity core, a flat signal table plus a small
expression evaluator. Both were replaced by the `rhai` engine in `rux-script`,
which owns state and evaluation now, and by the per-binding subscription model
in `rux-runtime`, which patches only the nodes a changed signal actually
reaches. What survives here is the shared value type those layers hand to each
other, which is why the crate is much smaller than its name suggests.

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
