# rux-runtime

The Rux document model: it turns a `.rux` file on disk into a renderable tree,
and keeps that tree in step with the app's state.

Loading a document means resolving its `use` component imports and loading each
imported `.rux`, merging the main and component scripts into one engine,
registering the host functions, and then building the tree with bindings,
directives and component expansions all resolved. Running a `@tap` handler
mutates engine state; the runtime then patches the nodes that state actually
reaches rather than rebuilding everything, using the read and write sets
`rux-script` records.

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
