+++
title = "Text input"
description = "The caret, the soft keyboard, and IME composition for text that is not typed one key at a time."
weight = 8
+++

<!-- GENERATED FROM docs/05-as-built.md BY site/sync-docs.sh. DO NOT EDIT HERE. -->

Typing is not only key presses. Anything past unaccented Latin is *composed*:
a dead key and a vowel make one accented character, and a CJK keyboard spells a
character out of several keystrokes, showing the half-finished result as it goes.
The shell asks the platform for those events with `set_ime_allowed` whenever a
text field is focused, and parks the candidate window under the caret with
`set_ime_cursor_area` so the list of characters to choose from does not cover the
text it is being chosen for.

Composed text is written straight into the bound signal as it is typed, which is
what a browser does to an `<input>`'s value mid-composition, so it renders
through the ordinary text path. `Focus` carries the byte range that is still
provisional and the painter underlines it. A composition can be abandoned as
well as committed: clicking away, tabbing off, or the input method detaching all
put the field back exactly as it was before composing started. While one is
running the input method owns the keyboard, and raw key presses are ignored, or
every letter would be typed twice.

On the web none of that applies, because a browser will not raise a phone's
on-screen keyboard for a `<canvas>`. There the shell keeps a real `<input>` laid
over the field it is editing, invisible and `pointer-events: none` so taps still
reach the canvas and still move the caret, focused only in response to the tap
that focused the field. It holds the real text rather than acting as an event
sink, which hands composition, autocorrect, dictation and the keyboard's own
backspace to the browser; the shell copies the value back into the signal. This
happens only on touch devices, because focusing it takes DOM focus off the canvas
where winit listens for keys.

The sharp edge there is that a browser counts a caret in UTF-16 code units and
Rux indexes strings by bytes. They agree only on ASCII, an emoji being 4 bytes
and 2 code units, so the conversion is done explicitly and tested rather than
assumed.
