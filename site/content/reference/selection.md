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

The highlight is painted behind the glyphs. Its colour is the author's
`::selection` background when one applies, the platform's own highlight on
Android (the theme's `textColorHighlight`), and the focus-ring blue elsewhere.
Its rectangles come from
parley, but only their *horizontal* extent: the vertical position is recomputed
from our own leading-trimmed line stepping, since parley's line pitch isn't ours
(see `rux-text::selection_rects`).

**`::selection`** honours `background-color` (or the colour in `background`)
and `color`, the two properties a browser applies to a highlight. Anything else
written there warns. The rule's selector picks the element, and its colours
reach every field below it, so `::selection { background: gold }` on its own
styles every field in the app, and a nearer rule overrides one property at a
time. `color` is drawn by painting the glyphs a second time, clipped to the
highlight, so a glyph cut by the selection's edge is two colours as it is in a
browser. `::selection` anywhere but the end of a selector matches nothing.
Under test in `crates/rux-runtime/tests/selection_style.rs`.

**Selection handles.** A finger that selects gets Android's teardrops: one
hanging left of the selection's start, one right of its end, drawn by Rux on
every platform, because the platform's belong to `TextView` and are offered to
no other view. Dragging one moves that end and keeps the other; the ends may
cross and may not meet. A mouse press puts them away. Their colour is the
field's own `accent-color` (CSS defines it as the accent of the controls an
element generates), else the platform accent (`colorControlActivated`), else
the focus-ring blue.

**The caret handle comes only from a long press on empty space**: past the end
of a line, after the last word, or on the spaces between words. That press puts
the caret where the finger is, with its handle and the menu of what a caret can
do: Paste (only when the clipboard holds something, as in any Android field),
Select text (the word at the caret, or the one before it when the caret is
after it; the label is the phone's own) and Select all. Autofill, which
WhatsApp's fields also offer there, waits for phase 6: it needs the input view
to describe its fields to Android's autofill service, and an item that does
nothing is worse than none. Both go after five seconds with nothing touching them;
dragging the handle or tapping it resets the clock, and a tap on it toggles the
menu. A plain tap places a caret and shows nothing, which is the user's call
from the phone session. A selection has no clock. A long press *on* a word takes
the word, and a finger that then moves keeps the whole word and extends from its
far end; it used to cut the word back to wherever the finger sat inside it.

**Android uses the platform's text menu, not the drawn toolbar.** A floating
`ActionMode` started on the input view: the same menu `TextView` shows, in its
order (Cut, Copy, Paste, Share, Select all), with its ids and strings, its
overflow, and after them every app registered for `PROCESS_TEXT` (Translate,
Gemini, a dictionary). A chosen app gets the selected text and, unless the field
is read-only, its answer replaces the selection. Seeing those apps at all needs
the manifest's `<queries>` entry for `PROCESS_TEXT`: since Android 11 another
app is invisible without it, and the list is simply empty. The menu is held back
while a finger is on the glass and returns on lift. Copy lets go of the selection
as Android's fields do; Share closes the menu; Back with a selection lets go of
it rather than closing the app. A password offers no Copy, Cut, Share or apps,
and Select all is left out when everything is already selected. **A long press
on a password takes all of it**, as Android's own password fields do: split at
its spaces, the selection showed where the spaces were. A long press past the
bullets is empty space, as in any other field. The shell
computes the whole menu state each frame and calls Java only when it changed
(`App::sync_text_menu`).

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
