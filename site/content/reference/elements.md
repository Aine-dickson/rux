+++
title = "Elements"
description = "The six elements the runtime renders, plus slot, router and route."
weight = 2
+++

<!-- GENERATED FROM docs/05-as-built.md BY site/sync-docs.sh. DO NOT EDIT HERE. -->

`<screen>` `<view>` `<text>` `<image>` `<path>` `<button>` `<input>` + imported
components as custom tags, plus two that render no box of their own: `<slot>`
(a component's hole for the caller's children) and `<router>`/`<route>` (see
[Routing](/reference/routing/)). `role=` is honored for **selectors and semantics**
(and matches **case-insensitively**: `role="Heading"` matches `[role="heading"]`).

`<image src="assets/logo.png">`: `src` resolves **relative to the .rux file**
(not the working directory), and `:src` binds an expression. With no CSS size it
lays out at the file's intrinsic pixel size; a `width`/`height` scales it to fit.
Formats: PNG, JPEG, GIF, WebP. A missing file logs to stderr and paints nothing.
