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

**Long lists: `content-visibility`** (v0.9). A list of two thousand rows is
built whole, and was laid out and drawn whole every frame. CSS's answer is
to let a row that is far from the screen skip its contents:

```css
.row { content-visibility: auto; contain-intrinsic-size: auto 40px; }
```

- **`content-visibility: auto`** lays out and draws a box's contents only
  while the box is near the screen: on screen, inside every clip it sits in,
  or within half the window's size of it. When a box first comes that near,
  every box within a window and a half is shown along with it, and they stay
  shown until the next one does, so what is shown changes about once a window
  of scrolling rather than on every frame. Further away, the box is laid out
  with no children, its own background and border drawn, its texts never
  measured. `hidden` skips them always; `visible` is the default.
- **`contain-intrinsic-size`** is the size a skipped box takes in place of its
  contents, plus its own padding and border, so the list keeps its length and
  its scrollbar its size. One value is both axes, two are width then height;
  `none` is zero. **`auto 40px`** is 40px until the box has been drawn, and the
  size it was last drawn at after that, so a row that scrolls away keeps its
  place exactly. `contain-intrinsic-width`, `contain-intrinsic-height`,
  `contain-intrinsic-inline-size` and `contain-intrinsic-block-size` set one axis. Without it a skipped box collapses to its
  padding, as in CSS.
- **Within the frame.** Which boxes are near is decided against where the
  last frame drew them; a box found skipped and on screen (a fast fling, the
  first frame) has its frame laid out again before it is drawn, so a row never
  shows up empty. The first frame lays every row out as its stand-in, finds
  the ones on screen, and lays those out properly.
- **Containment comes with it**, as in CSS: a box with `content-visibility`
  other than `visible` clips what is inside it, is a stacking context, and is
  the containing block for its `absolute` and `fixed` descendants.

Measured on the long-list benchmark (2,000 rows of three texts in a scroller,
scrolled a little each frame, release, desktop):

| | every row laid out | `content-visibility: auto` |
|---|---|---|
| layout, a scrolled frame | 23.8 ms | 7.7 ms, 3.1x faster |
| scene, a scrolled frame | 0.75 ms | 0.33 ms, 2.3x faster |
| the first frame's layout | 127 ms | 18.8 ms, 6.7x faster |
| building the document | 104 ms | 109 ms, the same |

Those were measured when it landed. Since then a frame that only scrolls
lays nothing out again, and rows scrolled out of view are passed over (see
"A frame that only scrolls"): a scrolled frame is now 0.9 ms without
`content-visibility` and 1.0 ms with it, so on a desktop it no longer makes
scrolling faster. What it still saves is the first frame, and the work of
laying out rows whose contents change while they are far away.

Building the rows is the same either way: every row is still styled and
stays in the document, so `query()`, `r-model` and a handler reach it.

**`scrollIntoView()` on an element inside a skipped box** scrolls to the box
standing in for it; its contents are there the next frame. Not yet, against
CSS: **Tab and assistive technology do not reach inside a skipped box**, which
in a browser they do, and a handler reading the box of an element inside one
gets none, as for an element not on screen.
