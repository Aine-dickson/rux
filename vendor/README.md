# Vendored crates

## `taffy/`: Taffy 0.15.0, with a `calc()` resolver

Taffy, the layout engine under `rux-layout`, resolves CSS `calc()` itself, at
the moment a percentage's basis is known: a child's `calc(100% - 20px)` is
worked out against its parent's final width, cyclic percentages in a parent
sized by its content included. It asks the tree what each `calc()` comes to,
through `LayoutPartialTree::resolve_calc_value`. The stock `TaffyTree`
answers that question with 0, and has no way to answer it otherwise.

The patch is the published 0.15.0 source with one addition, in
`src/tree/taffy_tree.rs`: `TaffyTree::set_calc_resolver(fn(*const (), f32) -> f32)`,
which the tree's `resolve_calc_value` calls. Every change is marked with a
`Rux:` comment. `rux-layout` sets the resolver, and its `calc()` values are
pointers to interned expressions (see `crates/rux-layout/src/calc.rs`).

The workspace patches crates.io's `taffy` with this copy (`[patch.crates-io]`
in the root `Cargo.toml`). A patch reaches this workspace only: anything that
depends on the published `rux-layout` would get stock Taffy, and every
percentage `calc()` would be 0 there.

**Upgrading Taffy:** copy the new release's source over `taffy/`, apply the
`Rux:` changes again, and run the workspace tests. If a release lets a
`TaffyTree` resolve `calc()` on its own, drop this directory and the patch.
