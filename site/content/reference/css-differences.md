+++
title = "Where Rux differs from CSS"
description = "The short list of places Rux does not answer the way CSS does, and which kind of difference each one is."
weight = 6
+++

<!-- GENERATED FROM docs/05-as-built.md BY site/sync-docs.sh. DO NOT EDIT HERE. -->


**The rule, set 2026-09-18: Rux's CSS behaves the way real CSS does, defaults
included, unless a divergence is genuinely necessary.** An author arrives
knowing CSS, and every place Rux answers differently is something they learn by
being surprised in the window. Documenting a divergence does not pay for it;
only necessity does, and "better ergonomics" is not necessity.

So this list is short on purpose, and each entry says which kind it is.

| Difference | Kind | Standing |
|---|---|---|
| ~~Flex cross-axis defaults to `flex-start`, not `stretch`~~ | a preference, taken for ergonomics | **Reverted in v0.8.** The default is `stretch`, as in CSS |
| `border-radius` in percent resolves against the **shorter side**, so `50%` on a 160x60 box is a pill and not CSS's ellipse | a capability: Rux draws one radius per corner and cannot draw an elliptical corner | Keep |
| ~~`box-sizing` defaults to `border-box`, not CSS's `content-box`~~ | found 2026-10-23, never chosen: Taffy's default, taken without a decision | **Reverted 2026-10-23, the owner's call.** The default is `content-box`, as in CSS: `width: 100px` with `padding: 20px` is 140px wide. `<input>` and `<button>` are `border-box` unless written otherwise, as browsers' own sheets make their controls, and the root is the window's size whatever its padding. The examples now open with `* { box-sizing: border-box; }`, the reset web authors write |
| No `display: inline` or `inline-block`: two `<text>` siblings stack, and a line holds only words, nested `<text>`, `<image>` and `<path>` | a capability: inline flow is built for text (a `<text>` inside a `<text>`), not for boxes with their own layout | Keep |

Two things that look like differences and are not: `display` defaults to
`block`, and `flex: 1` means `1 1 0%`. Both are CSS's own answers.
