+++
title = "Scrolling"
description = "Scrollers, scrollbars, and the ways a scroll can be driven."
weight = 11
+++

<!-- GENERATED FROM docs/05-as-built.md BY site/sync-docs.sh. DO NOT EDIT HERE. -->

`overflow: auto | scroll` makes a box scroll **on whichever axis its content
overflows**: vertical, horizontal, or both. It scrolls by:

- **wheel** (Shift+wheel, or a horizontal wheel, scrolls sideways),
- **dragging a scrollbar thumb**,
- **touch**: a finger drags the content itself,
- **keyboard**: arrows, PageUp/PageDown, Home/End scroll the box **under the
  pointer**, when no input has focus.

**Scrollbars** are an overlay on the box's trailing edge: they appear only on an
axis that actually has travel, the thumb is the box's fraction of the content (to
a grabbable floor), and when both axes scroll the tracks stop short of the corner.
They are drawn *over* the content, because a scroller clips its children so they can't
be part of the subtree, and drawn from the same geometry the drag hit-tests, so
they can't disagree.

**Scroll-into-view** runs on Tab: focusing something below the fold scrolls its
box far enough to show it (typing in a textarea does the same for the caret).

Offsets live in the shell keyed by the scroller's index in tree order, so they
survive the whole-tree rebuild, so tapping a row doesn't scroll the list to the top.
A press on a thumb never becomes a tap on the content beneath it.

**Not done:** no click-on-track paging, no kinetic/inertial touch fling, no
scrollbar hover/fade states, no `scrollbar-width`/`scrollbar-color`, no
`overscroll-behavior`, and `overflow-x`/`overflow-y` can't yet differ (one
`overflow` governs both axes).
