+++
title = "Where Rux differs from CSS"
description = "The short list of places Rux does not answer the way CSS does, and which kind of difference each one is."
weight = 6
+++

<!-- GENERATED FROM docs/05-as-built.md BY site/sync-docs.sh. DO NOT EDIT HERE. -->


**Rux's CSS behaves the way real CSS does, defaults included, unless a
divergence is genuinely necessary.** An author arrives knowing CSS, and every
place Rux answers differently is something they learn by being surprised in the
window. So this list is short on purpose, and each entry says why it exists.

| Difference | Why |
|---|---|
| `border-radius` in percent resolves against the **shorter side**, so `50%` on a 160x60 box is a pill and not CSS's ellipse | Rux draws one radius per corner and cannot draw an elliptical corner |
| No `display: inline` or `inline-block`: two `<text>` siblings stack, and a line holds only words, nested `<text>`, `<image>` and `<path>` | inline flow is built for text (a `<text>` inside a `<text>`), not for boxes with their own layout |

Defaults are CSS's: `display` is `block`, `flex: 1` means `1 1 0%`, a flex
container's `align-items` is `stretch`, and `box-sizing` is `content-box`, so
`width: 100px` with `padding: 20px` is 140px wide. `<input>` and `<button>` are
`border-box` unless written otherwise, as browsers' own sheets make their
controls, and the root is the window's size whatever its padding. The examples
open with `* { box-sizing: border-box; }`, the reset web authors write.
