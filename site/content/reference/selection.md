+++
title = "Selection"
description = "Drag-select, double-click, and the clipboard keys."
weight = 10
+++

<!-- GENERATED FROM docs/05-as-built.md BY site/sync-docs.sh. DO NOT EDIT HERE. -->

A focused input has a **selection**, not just a caret: `Focus` carries a `caret`
and an `anchor`, and the range between them is selected (`anchor == caret` means
nothing is). Both are re-applied after every rebuild, like the caret.

- **Drag** across text to select; **double-click** selects a word.
- **Shift** + a movement (arrows, Home/End, Up/Down in a textarea) extends from
  the anchor; the same movement without Shift collapses the selection.
- Typing, pasting, Backspace and Delete **replace** the selection.
- **Ctrl+A** select all · **Ctrl+C** copy · **Ctrl+X** cut · **Ctrl+V** paste
  (via `arboard`, the real system clipboard). Pasting several lines into a
  single-line input keeps only the first.

**A single-line input scrolls horizontally to keep its caret in view**, added in
v0.5.1. Before that the caret walked out of the box and was clipped away, so a
field could not be used at all once its value outgrew its width: not by typing,
arrows, End, a tap or a drag, on any platform. The cause was that an input is
given `overflow: clip` while a textarea is given `overflow: scroll`, and only
the latter produces a scroll region for `scroll_caret_into_view` to move.

The offset moves only when the caret would otherwise fall outside, so the text
does not slide under a caret that is already visible, and it is clamped so the
field never scrolls past the start nor leaves a gap after the end. Hit testing
applies the same offset, or a tap in a scrolled field would land a character out
by exactly the scroll distance.

The highlight is painted behind the glyphs in the focus-ring blue: **not
author-controlled**: there is no `::selection` yet. Its rectangles come from
parley, but only their *horizontal* extent: the vertical position is recomputed
from our own leading-trimmed line stepping, since parley's line pitch isn't ours
(see `rux-text::selection_rects`).

**A selection toolbar** appears above the focused field whenever something is
selected (below it when there is no room above), offering **Copy**, **Cut**,
**Paste** and **Select all**. It runs the same four actions the Ctrl shortcuts
do, not a second copy of them.

It exists because on a phone there is no Ctrl+C, and in a browser there was no
clipboard at all: `arboard` is a desktop-only dependency. The browser's *own*
copy bubble cannot be used either, whatever the selection says. The hidden
`<input>` is `pointer-events: none`, `opacity: 0` and one pixel square, so the
browser never sees a selection gesture on it, and setting the range from code
does not raise native selection UI. That was verified on a phone rather than
assumed. Before v0.5.1 the only thing that worked was paste, and only because
the keyboard writes into the hidden input directly, arriving as an ordinary
`input` event that never touched clipboard code.

On the web the toolbar goes through `navigator.clipboard`. Writing is fired and
forgotten. Reading cannot be: the API is a promise and may prompt for
permission, so a paste is *started* by the tap and applied later, when the read
resolves. A refused prompt is silent, since declining is a decision rather than
a fault. A press on the toolbar is refused by the text press handler, or moving
the caret would collapse the selection the button is about to act on.

The selection is also kept in step with the hidden input in both directions as
of v0.5.1: a drag on the canvas is written out, and a range set in the input is
read back, including which end the caret is at (`selectionStart`/`End` are
ordered, so `selectionDirection` carries it).

**Limits:** no word-wise movement (Ctrl+arrows moves by character), no
triple-click line-select, no drag-and-drop of selected text, no middle-click
paste on X11, and a `select` has no arrow-key list navigation or native mobile
picker.
