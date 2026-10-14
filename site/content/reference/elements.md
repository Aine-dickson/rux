+++
title = "Elements"
description = "The six elements the runtime renders, plus slot, router and route."
weight = 2
+++

<!-- GENERATED FROM docs/05-as-built.md BY site/sync-docs.sh. DO NOT EDIT HERE. -->

`<screen>` `<view>` `<text>` `<image>` `<path>` `<button>` `<input>` + imported
components as custom tags, plus two that render no box of their own: `<slot>`
(a component's hole for the caller's children) and `<router>`/`<route>` (see
[Routing](/reference/routing/)). `role=` is honored for **selectors and semantics**
(and matches **case-insensitively**: `role="Heading"` matches `[role="heading"]`).

**Words go in a `<text>`, except in a `<button>`.** `<button>Join</button>` is
read as `<button><text>Join</text></button>`, `{{ }}` included, so a button's
label can be written the way it would be in HTML. Anywhere else, bare text
draws nothing.

**Spaces in a `<text>` follow CSS's `white-space`.** Under the default,
`normal`, every run of spaces, tabs and newlines is one space, and spaces at
the start and end are gone, so a sentence can be written over several indented
lines and reads as one sentence. That applies to `{{ }}` values as well: a
value with line breaks in it is drawn on one line unless the text says
`white-space: pre-line` (keep the newlines, collapse the rest) or `pre-wrap`
(keep everything). `&nbsp;` is never collapsed, which is how to ask for two
spaces in a row. Until v0.9 a newline inside a `<text>` was a line break and
the indentation after it was drawn; text from data that relied on that needs
`white-space: pre-line` now. Text fields are not affected: what is typed is
kept as typed.

**A `<text>` inside a `<text>` is part of the same sentence.** This is how a
word is made bold, a link is put in a line, or a value is coloured on its own:

```html
<text>Read the <text class="b">terms</text> before you
  <text class="link" @tap="open()">sign</text>.</text>
```

The whole sentence wraps as one paragraph, its runs moving with it, and spaces
collapse across them: the space before `<text class="b">` is the space between
two words. A run inherits the sentence's text style, as every element does
(see the CSS list), and the sentence's underline or strike-through is drawn
under its words too, as CSS draws a decoration through nested boxes. It has its own
`color`, `font-*`, `text-decoration` and spacing, and a solid `background` on
a run highlights its words, line by line. Each run is still an element: its own
`{{ }}`, `r-if`, `r-for`, `@tap` (found by a tap on its words, and a Tab stop),
`:class` and `query()`. `:hover` and `:active` match a run while the pointer
is over its words, so a link can change colour under it. Padding, borders and margins on a
run are not drawn. The words between tags are the sentence's
own and match no rule, not even `*`, as in CSS.

**`<image>` and `<path>` inside a `<text>` sit in the line** as a box the size
of their `width` and `height` (or their own size), wrapping with the words
around them. `vertical-align` places one: `baseline` (the default, CSS's)
stands it on the baseline, which leaves an icon as tall as the text sitting
high; `middle` centres it on the text, which is what an icon usually wants;
`text-top` and `text-bottom` line it up with the top or bottom of the text. A
taller box makes its line taller. Anything else inside a `<text>` (a `<view>`,
a `<button>`, a component) is an error: a box with its own layout in a line is
CSS's `inline-block`, which Rux does not have, so put the two side by side in a
`<view>` instead. Two `<text>` *siblings* still stack: they are two paragraphs.

**`<screen>` is the display, wherever it is written.** At the document root
that is nothing new: the root has always been forced to the viewport, whatever
its tag. Below the root it now means what the word says. A `<screen>` inside a
240x120 box with `overflow: hidden` still covers the window, because it
defaults to `position: fixed` with all four insets at zero, and the four edges
of the initial containing block are the display. Both are defaults: name a
`position` or an inset of your own and it is yours.

It is not a divergence under the rule below, because `<screen>` has no CSS
counterpart whose defaults it could follow. `<body>` is the
nearest analogue and it is an ordinary box in a scrolling document, which is
the one thing a screen on a phone is not.

**An element that holds nothing closes itself.** `<image>`, `<input>`,
`<path>` and `<router-view>` take no children, so `<input type="text">` is
complete as written and there is no `</input>` for it to be missing. The slash
is still legal and is what `examples/` uses. Writing a closing tag for one is an
error that says so: *"`<input>` holds nothing, so it has no closing tag; delete
`</input>`"*. Until v0.7.1 the parser demanded the closing tag and, not finding
it, named whichever closing tag it found next — so `<view><input></view>` was
reported as "expected `</input>`, found `</view>`", against a file whose only
mistake was being written the way HTML is written.

`<image src="assets/logo.png">`: `src` resolves **relative to the .rux file**
(not the working directory), and `:src` binds an expression. With no CSS size it
lays out at the file's intrinsic pixel size; a `width`/`height` scales it to fit.
Formats: PNG, JPEG, GIF, WebP. A missing file logs to stderr and paints nothing.
