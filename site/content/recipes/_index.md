+++
title = "Recipes"
description = "How to build the things apps are actually made of: a message list, a tab bar, a modal. Each one is a working file, and each one is written around the part that surprises people."
weight = 3
sort_by = "weight"
template = "docs-section.html"
page_template = "docs.html"
+++

The [reference](@/reference/_index.md) says what each rule does. [Learn](@/learn/_index.md)
walks through the language once, from an empty file. This is the third thing:
how to build a particular piece of an app, start to finish.

Every recipe here is a real file under `examples/recipes/`, runs with
`rux examples/recipes/<name>.rux`, and is checked by the test suite on every
commit. Copy one and change it.

## What a recipe is for

A rule can be right and still surprise you, because its consequence turns up
somewhere you were not looking. A departing element keeps its place, which is
correct, and the consequence lands on the element *below* it. Paint is CSS,
which is correct, and the consequence is that a more specific selector changes
how long an animation lasts.

Those consequences are what these pages are made of. Each recipe is built around
the one or two places its pattern goes wrong, with the reason it is not a bug,
because a consequence is much easier to learn from a worked example than from a
list of rules. The register of every such trap, and where each is explained,
lives in [author notes](@/contribute/author-notes.md).

## The recipes

- **[A message list](@/recipes/message-list.md)**: a scroller that follows its
  newest row. Teaches what makes a scroller, why `query` hands you a position
  rather than a thing, and where a list's width goes.
- **[A tab bar](@/recipes/tab-bar.md)**: three tabs over one router, with pages
  that cross rather than queue. Teaches `to=` and `:current`, and the one CSS
  line that stops a route transition stacking.
- **[A modal](@/recipes/modal.md)**: a dialog over a scrim, both animated.
  Teaches how a cover is positioned, why a tap does not bubble, and what a
  swallow costs.

These track the **tip**, not the latest release, so a recipe may use something
that is not in a tagged version yet. `/learn` is the one that tracks releases.
