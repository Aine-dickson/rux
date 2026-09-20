+++
title = "Elements"
description = "The six elements the runtime renders, plus slot, router and route."
weight = 2
+++

<!-- GENERATED FROM docs/05-as-built.md BY site/sync-docs.sh. DO NOT EDIT HERE. -->

`<screen>` `<view>` `<text>` `<image>` `<icon>` `<path>` `<button>` `<input>` + imported
components as custom tags, plus two that render no box of their own: `<slot>`
(a component's hole for the caller's children) and `<router>`/`<route>` (see
[Routing](/reference/routing/)). `role=` is honored for **selectors and semantics**
(and matches **case-insensitively**: `role="Heading"` matches `[role="heading"]`).

**`<screen>` is the display, wherever it is written.** At the document root
that is nothing new: the root has always been forced to the viewport, whatever
its tag. Below the root it now means what the word says. A `<screen>` inside a
240x120 box with `overflow: hidden` still covers the window, because it
defaults to `position: fixed` with all four insets at zero, and the four edges
of the initial containing block are the display. Both are defaults: name a
`position` or an inset of your own and it is yours.

It is not a divergence under the rule below, because `<screen>` has no CSS
counterpart whose defaults it could follow. `<body>` is the
nearest analogue and it is an ordinary box in a scrolling document, which is
the one thing a screen on a phone is not.

**An element that holds nothing closes itself.** `<image>`, `<input>`,
`<path>` and `<router-view>` take no children, so `<input type="text">` is
complete as written and there is no `</input>` for it to be missing. The slash
is still legal and is what `examples/` uses. Writing a closing tag for one is an
error that says so: *"`<input>` holds nothing, so it has no closing tag; delete
`</input>`"*. Until v0.7.1 the parser demanded the closing tag and, not finding
it, named whichever closing tag it found next — so `<view><input></view>` was
reported as "expected `</input>`, found `</view>`", against a file whose only
mistake was being written the way HTML is written.

`<image src="assets/logo.png">`: `src` resolves **relative to the .rux file**
(not the working directory), and `:src` binds an expression. With no CSS size it
lays out at the file's intrinsic pixel size; a `width`/`height` scales it to fit.
Formats: PNG, JPEG, GIF, WebP. A missing file logs to stderr and paints nothing.
