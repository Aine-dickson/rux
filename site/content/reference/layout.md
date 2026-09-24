+++
title = "Layout"
description = "Everything defaults to block; use display: flex. Hug, fill, and why inline flow is gone."
weight = 3
+++

<!-- GENERATED FROM docs/05-as-built.md BY site/sync-docs.sh. DO NOT EDIT HERE. -->

> **DIVERGENCE from docs 01–04.** The inline/block-by-role model was **built and
> then deliberately removed**. taffy has no inline text-flow, so inline elements
> hugged inside flex parents but filled inside block ones (full-width buttons),
> confusing. It's gone.

- **Everything defaults to `display: block`.** Block containers make children fill.
- **Use `display: flex` for layout.** Flex cross-axis defaults to **`stretch`**,
  as in CSS: a child of a flex column fills its width, and a child of a row its
  height. A button or a badge that should hug says `align-self: flex-start`, or
  its parent says `align-items`. This was `flex-start` until v0.8, a preference
  taken for ergonomics and reverted because Rux follows CSS's defaults.
- **Hug means `fit-content`**: a box with no `width` is clamped to its parent's
  inner width, so it can't burst out of a narrower parent. An explicit `width` (or
  `flex-shrink: 0`) is your call and *will* overflow, so clip it with `overflow: hidden`.
- `display: grid` works (`grid-template-columns` / `-rows`: `1fr`, `px`, `auto`).
- No inline text flow: two `<text>` siblings **stack**, they don't share a line.
- **Lengths are logical pixels.** Layout and taps run in logical space and the
  scene is scaled to the display's DPI, so `16px` is the same physical size on a
  1x and a 2x screen.
