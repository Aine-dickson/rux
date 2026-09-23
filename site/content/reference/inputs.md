+++
title = "Inputs"
description = "Text fields, textarea, select, checkbox and radio, and two-way binding with r-model."
weight = 7
+++

<!-- GENERATED FROM docs/05-as-built.md BY site/sync-docs.sh. DO NOT EDIT HERE. -->


**`r-model` is not optional.** It is the whole of an input's identity: the
layout gives a focus region only to an input that carries one, the shell tracks
the caret by the model text, and the value the field shows is read back out of
that signal. An `<input placeholder="…" />` with nothing bound paints its box,
renders its placeholder, and takes no tap, no caret and no keystroke. That was
silent until v0.7.1 and is now an error naming the line:

```
this `<input>` has no `r-model`, so nothing can be typed into it: the caret,
the keystrokes and the value it shows are all addressed by the signal it binds
```

`<input r-model="sig" placeholder="…">`: tap to focus, type to edit. There is a
real **caret**: tapping puts it where you tapped, ←/→ move it, Home/End jump,
Backspace/Delete cut either side of it, and typing inserts at it. Esc unfocuses.
Every edit writes the signal, so `{{ }}` updates live. Placeholder shows when
empty. The caret survives the rebuild that follows each keystroke.

Inputs **fill their slot** (default `width: 100%`) rather than hug their text, so
a field doesn't shrink as you type, and single-line inputs **never wrap** and
**clip** overflow (no horizontal scroll yet).

`<input type="textarea" r-model="sig">` is the same, but **Enter inserts a
newline** (in a single-line input it commits the field, below), the value wraps across lines,
**Up/Down move the caret between lines**, and it **scrolls vertically**: the
wheel scrolls it and typing keeps the caret in view.

`<input type="select" r-model="sig" :options="list">` shows the bound value and,
on tap, opens a **dropdown** of the `:options` (evaluated to strings), a floating
panel with a shadow, the current value picked out as a pill, and separators.
Tapping a row writes it back to the signal; any other tap closes it. The open
state lives in the shell and survives rebuilds (like scroll offsets).

**On Android the platform's own picker opens instead**, a Material dialog with
the current value already selected. A drawn dropdown is the right answer on a
desktop and a browser emulation on a phone: it does not fling, does not dismiss
on Back, and is not announced as a picker by a screen reader. Dismissing the
dialog, by Back or by tapping outside, leaves the value alone. `:options` and
`r-model` are the same either way, so a document never knows which it got.
`background-size` and native mobile pickers are not done.

**Keyboard focus:** **Tab** / **Shift+Tab** move a focus ring through every
interactive element (text/textarea/select inputs, buttons, checkboxes, radios) in
document order; tapping one also moves the ring there. A focused text input edits;
a focused **button/checkbox/radio** activates on **Space/Enter** (running the same
handler as a tap); a focused **select** opens on Space/Enter. So checkboxes and
radios are now keyboard-reachable, not tap-only.

`<input type="checkbox" r-model="flag">` and
`<input type="radio" r-model="choice" value="pro">` are **tap-toggles**: no focus,
no keyboard. They write the bound signal through the ordinary handler path
(`flag = !flag`, `choice = "pro"`), so an authored `@tap` overrides them.

A ticked box matches **`:checked`**:
```css
.box          { background: #313244; border: 2px #45475a solid; color: #cdd6f4; }
.box:checked  { background: #a6e3a1; color: #ffffff; }   /* white tick on green */
```
It *also* still carries the synthetic **`checked` class** that predated the
pseudo-class, so stylesheets written against `.box.checked` keep working. That is
deprecated and goes away in a later release, write `:checked`.
The mark is drawn in the box's own `color`: a **stroked checkmark** for a checkbox
(a path, not a ✓ glyph, since a glyph is whatever the system font ships and reads as a
letter), a dot for a radio. Keep the checked `border` a shade apart from the
checked `background`, or the ring dissolves into the fill. A radio is **round** unless you give it a `border-radius` (and a
huge radius like `9999px` is clamped to a circle, so that's how you re-round one
that inherited a radius from another class).
