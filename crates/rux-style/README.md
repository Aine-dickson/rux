# rux-style

Rux styling: literal CSS, parsed with `lightningcss`, cascaded onto the
template tree.

Rux's fourth law is that the CSS in a `.rux` file is real CSS rather than a
lookalike dialect, so this crate parses it with the same engine a browser
toolchain would, then matches the rules against the template with its own small
selector engine and applies the cascade. Specificity and source order resolve
conflicts exactly as they do in CSS. Selector support covers tag, `.class`,
`#id`, `[role="..."]`, compound selectors such as `view.card`, and all four
combinators. The output is a styled node tree for `rux-layout`.

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
