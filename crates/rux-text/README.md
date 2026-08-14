# rux-text

The Rux text engine: `parley` for shaping and line layout over the system
fonts, `vello` for drawing the resulting glyph runs.

It owns the font and layout contexts so they are built once and reused. Layout
calls `measure` to size text leaves; painting calls `draw` to put the glyphs on
the scene. Text is sized and drawn with leading trimmed, so a line's box is
ascent plus descent rather than the full line height, and the baseline sits at
the top plus the ascent. That is what makes `padding` read equally on all four
sides of a text node instead of looking heavier above and below.

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
