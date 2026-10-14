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

**Each axis on its own** (v0.9). `overflow-x` and `overflow-y` may differ,
and `overflow` takes one value for both or two, `x y`. `overflow-x: hidden;
overflow-y: auto` scrolls up and down and clips what is wider. As CSS computes
it, `visible` beside an axis that is `hidden`, `auto` or `scroll` acts as
`auto`, so `overflow-y: hidden` alone can scroll sideways when the content is
wider. A longhand beside the shorthand wins, whichever was written first (the
cascade has lost the order by then; the longhand is the one written to narrow
the other). **One divergence, written down:** `clip` on one axis beside
`visible` on the other clips both in Rux, since a clip here is the whole box.

**Scroll chaining** (v0.9). What a scroller cannot take passes to the scroller
it sits in, as in a browser: a list at its end lets the page go on scrolling,
by the wheel or a finger. `overscroll-behavior: contain` (or `none`, which is
the same here: Rux draws no end-of-scroll effect for them to differ on) keeps
it, per axis with `overscroll-behavior-x` / `overscroll-behavior-y` or
`overscroll-behavior: x y`. A fling keeps to the scroller it started in, and
so does a run of wheel notches: until the wheel rests for 300ms or the pointer
moves, the notches go to the scroller the run began on, so a list that slides
under the pointer as the page scrolls does not take over half way.

**Scrollbars come and go** (v0.9), as Windows 11, macOS and Android draw them.
A scroller's bars show while it scrolls or the pointer moves over it, stay for
a second after, and fade. A bar is thin at rest and wide, with its track
showing, under the pointer, in a 10px lane along the box's edge. **A press on
the track** pages toward it (seven eighths of the box), and held, pages on
until the thumb reaches the pointer. `scrollbar-width: thin` draws it thinner,
`none` draws none (the box still scrolls, and its edge is no longer a lane to
press). `scrollbar-color: <thumb> <track>` colours it; the default is a grey
that shows on a light page and a dark one.

**Kinetic fling after a touch drag landed 2026-09-21**, described under touch
above. No bounce at the ends yet: that is the physics item's.
