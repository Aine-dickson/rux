# Types: removed text

Text removed from `docs/10-types.md` on 2026-11-06, kept for the reasons and the history behind its rules.

## Header and status table (removed 2026-11-06 from 10-types.md)

**Superseded as a plan by [The Rux language](./11-next.md)** (2026-09-26), which keeps most of this and marks what changes (`int`/`float` for `number`, `none` for `null`, generics, checked types). This document still describes what runs today.

**Status: designed, being built.** Every rule below was decided on 2026-09-24
and none of it is open. The table says which parts run today; a part that does
not run yet is a design, and [Script](./07-script.md) stays the reference for
what the language does now. As each part lands, its rules move into Script and
the row below says so.

Every row of the table was Built on 2026-09-24: annotations parsed and erased; the checker (primitives, arrays, records, dictionaries, `T?`, inference); unions, narrowing, `switch` exhaustiveness and forced `?.`; functions with the caller-local rule (the sweep before it found no existing function it breaks); templates (`r-for`, `r-if`, bindings, `r-model`, events, props at the tag); boundaries (prop checks at run time, `x is T`, a route parameter becoming the type its prop takes); and the editor (hover, completion, the parameter quick-fix, from `rux check --format json --types`).

## A typed function cannot read its caller's locals (removed 2026-11-06 from 10-types.md)

Under the fork, a plain call ran in its **caller's** scope (its divergence 4),
so a function body could read a `let` of the function that called it. That cannot be typed: the same name would have a
different type, or no type, depending on the caller, and nothing at the
definition says which.

Since Rux's interpreter replaced the fork (step 5 of [The Rux language](./11-next.md#build-order)), a
`let` of a *function* in between is not reached; the fork reached that too, and
the test suite and the repository's `.rux` files, run on both, showed nothing
relying on it. Because a function with no
parameters is typed, this rule reaches functions that were never touched by an
annotation. Before it was switched on, every function in the repository's
examples, recipes and `/learn` chapters, and in the apps built with Rux so far,
was surveyed for a read of a caller's local. None did.

## Sharing types between files (removed 2026-11-06 from 10-types.md)

The word `type` is what makes it a type import (the project owner's
rule, 2026-09-26): `use components::task` imports the component in
`components/task.rux`, and `use type components::task::Row` imports the
type `Row` declared in that file. Before `type` marked one, a type import was
told by its capital letter (`use types::Task;`); that spelling still loads,
with a warning, and `rux fmt` rewrites it.

A file used only for its types holds a `<script>` and nothing else: no
`<template>` and no `<style>`. Its script may declare types and nothing
besides, since nothing would ever run a function or a statement in it, and
`rux` refuses to run the file, since it has nothing to show.

## Numbers (removed 2026-11-06 from 10-types.md)

The whole-number literal rule changed in step 3 of [The Rux language](./11-next.md#numbers); until then there was one `number` with `int` a kind of it.

## How it is built (removed 2026-11-06 from 10-types.md)

Until 2026-09-26 the script ran on a fork of rhai, which was handed the same
text with every annotation blanked, and before that the fork parsed
annotations itself and kept them in a side table the checker read. Both went
with the fork in step 5 of [The Rux language](./11-next.md#build-order).
