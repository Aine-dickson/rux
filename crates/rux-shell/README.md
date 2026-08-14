# rux-shell

The Rux runtime shell: window, GPU surface, input and frame loop.

It opens a native window with `winit`, manages the GPU through vello's render
context, asks `rux-runtime` for the current tree and paints it through
`rux-paint`. Input lives here too: pointer and touch, keyboard, text editing
and selection, scrolling. A file watcher wakes the event loop on every save, so
editing a `.rux` file repaints it live.

The same crate drives Rux in a browser, where there is no filesystem, no
blocking main thread and no system clipboard. Those four public functions sit
behind `cfg(target_arch = "wasm32")`, and the crate carries a docs.rs target
list so they appear in the rendered documentation even though docs.rs builds
the host.

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
