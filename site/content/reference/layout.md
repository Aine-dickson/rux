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
- Two `<text>` siblings **stack**, as two paragraphs do. A `<text>` *inside* a
  `<text>` shares its line: see [Elements](#elements).
- **Lengths are logical pixels.** Layout and taps run in logical space and the
  scene is scaled to the display's DPI, so `16px` is the same physical size on a
  1x and a 2x screen.
- **`direction` runs a layout right to left.** `dir="rtl"` on an element (or
  `direction: rtl` in a rule, which wins over the attribute) is inherited by
  everything inside it. A flex row then starts from the right, grid columns
  count from the right, a block's children start on the right, and a text's
  lines start at its right edge, which is where `text-align: start` puts them;
  `text-align: left` and `right` stay the sides they name. Text of both
  directions in one paragraph is ordered by the Unicode bidirectional
  algorithm, against the paragraph's direction. `dir="auto"` leaves a text to
  its first letter with a direction, which is also what a text field does. The
  default is `ltr`, as in CSS: a text that starts with an Arabic or Hebrew
  letter in a left-to-right page is a left-to-right paragraph with right-to-left
  words in it, and needs `dir="rtl"` or `dir="auto"` to be a right-to-left one.
  The logical properties, `margin-inline-start` and `-end`, `margin-inline`,
  the same for `padding` and `inset`, and their `block` counterparts (always
  top and bottom), name a side by the direction, so one stylesheet serves both.
  A right-to-left scroller opens at its right end, its scrollbar thumb starts at
  the right, and the wheel, a finger, the arrow keys and the thumb all move it
  the way they move the content on screen. A text field follows what is typed
  into it (`dir="auto"`) unless `dir` or `direction` reaches it, in which case
  an empty one keeps its caret on that side; Left and Right step through
  right-to-left words the way they are drawn, as a browser's field does. Not
  yet: a run of a different direction inside a sentence (`dir` on a nested
  `<text>`).
