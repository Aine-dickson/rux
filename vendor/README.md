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

## `vello/` and `vello_encoding/`: vello 0.9.0, its scratch buffers sized to the scene

vello sizes the GPU buffers its coarse stages allocate into (lines, tiles,
segments, the per-tile command lists, binning, blend spill) at fixed lengths,
about 150 MB whatever the scene. A Rux window of text and boxes held about
475 MB of GPU memory, which on an integrated GPU is system RAM (Tracer's
AW-022). The GPU already counts what a frame needed of each, in the
`BumpAllocators` it writes, and vello has the readback for it, unused outside
its debug layers.

The patch: `vello_encoding` takes the lengths as a `BumpSizes`
(`RenderConfig::new_sized`; `RenderConfig::new` keeps vello's), and vello's
`Renderer::render_to_texture` runs the coarse stages, reads the counts back,
and runs them again larger when a buffer ran out, before the fine stage, so a
frame is never drawn short. The lengths start small and come down again
after a long run of frames that needed far less (`src/bump.rs`, Rux's own,
with its tests); `WgpuEngine::trim_pool` lets go of buffers of a length no
longer asked for. The web keeps vello's path, since a frame there cannot wait
for the GPU. Every change is marked with a `Rux:` comment.

Measured on a window of 1,000 rows of text (release, AMD integrated GPU):
GPU memory 475 MB to 297 MB, private memory 586 MB to 417 MB, and the wait
for the counts costs about 1.6 ms a frame.

The rest was the allocator. `RUX_GPU_REPORT=1` showed 10 MB allocated inside
256 MB reserved: wgpu's default `MemoryHints::Performance` takes device
memory in large blocks. `util::RenderContext` now asks for
`MemoryHints::MemoryUsage`, and the window holds 61 MB of GPU memory and
193 MB private, at no measured cost per frame.

Both are patched in the root `Cargo.toml`, and into an app's generated
wrapper when Rux comes from a checkout (`crates/rux-cli/src/build.rs`).

**Upgrading vello:** copy the new release's `vello` and `vello_encoding` over
these, apply the `Rux:` changes again (and keep `src/bump.rs`), and run the
workspace tests and a window. If a release sizes its buffers itself, drop
both directories and the patches.
