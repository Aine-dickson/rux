# rux-rhai

**Rux's fork of [rhai](https://github.com/rhaiscript/rhai) 1.25.1.** Almost all
of the code here is rhai's, by rhai's authors, under rhai's MIT-or-Apache
licence.

**If you are looking for an embedded scripting language for Rust, you want
[`rhai`](https://crates.io/crates/rhai), not this.** This crate exists because
every Rux crate has to be published for `cargo publish` to resolve the workspace,
and it carries a small set of changes that only make sense inside Rux. It has no
stability guarantees of its own and will track whatever Rux needs.

The library target is still named `rhai`, so `use rhai::…` works unchanged.

## What is different

[`DIVERGENCE.md`](./DIVERGENCE.md) is the complete list, with the reasoning and
the upstream files touched. In short: `?.` guards a missing property and not only
an absent base, because Rux runs with `fail_on_invalid_map_property` on and
upstream's strictness has no opt-out.

That file also records the changes Rux considered and **did not** make, because
the discipline this fork is kept under is that nothing is changed here which can
be changed from outside the engine.

## Upstream

Rux's own reasoning for forking at all is in `docs/06-roadmap.md`, under v0.7.
Bugs in the language itself belong upstream, at
<https://github.com/rhaiscript/rhai>; please do not report them to Rux unless
they are caused by something in `DIVERGENCE.md`.
