# rux-paint

The Rux painter: it turns the paint items `rux-layout` produces into a `vello`
scene.

Boxes become filled rounded rectangles with their borders and backgrounds; text
leaves become glyph runs, drawn through `rux-text`. This is the last stage
before the GPU, and it is deliberately the dumbest one: everything is already
resolved to absolute rects and concrete colours by the time it arrives here, so
this crate makes no decisions about what anything should look like.

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
