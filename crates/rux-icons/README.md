# rux-icons

Tabler's icon set, as path data [Rux](https://ruxlang.dev) can draw.

**Internal to Rux.** The supported entry point is the
[`ruxlang`](https://crates.io/crates/ruxlang) crate, which installs the `rux`
command. Nothing here is a stable API for anyone else.

An icon is a list of paths on a 24 by 24 grid, in two variants. Outline is
stroked; filled is solid and exists for roughly one icon in five, so the
default variant is outline and asking for a filled icon that has none is an
error rather than a silent fallback.

Paint is **per path**, not only per variant. Around ninety outline icons carry
a path that is solid, or filled with no stroke: the dot on the head in
`accessible`, the slice in `percentage-25`. Anything drawing an icon has to
honour those overrides or those icons come out visibly wrong.

Everything else about paint belongs to the author. `fill`, `stroke` and
`stroke-width` are CSS in Rux and cascade like any other property, so an icon
takes its colour from the text around it with no plumbing here.

## The data is generated, and committed

```bash
git clone --depth 1 --branch v3.47.0 https://github.com/tabler/tabler-icons
cargo run -p rux-icons --bin generate -- tabler-icons
```

That rewrites `src/generated.rs`, which is committed. **Nobody using Rux runs
this**: an author never downloads six thousand SVG files and never runs a build
step to get an icon. It is run when Tabler is bumped, by whoever bumps it, and
the version it read is recorded in the output.

The generator refuses rather than guesses. A filled icon with no outline
counterpart, an SVG containing anything but `<path>`, or a paint attribute it
has not seen before all stop it, because each of those means an assumption the
design rests on has changed.

## Licence

Rux is MIT or Apache-2.0, at your option.

**The icon data is Tabler's**, MIT licensed, Copyright (c) 2020-2026 Paweł Kuna.
The notice travels with the data in `LICENSE-TABLER`, and that file is part of
this crate rather than a reference to somewhere else.
