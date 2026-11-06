# Archive: docs/05-as-built.md, Honored CSS through Anchor positioning

Text removed 2026-11-06 from docs/05-as-built.md. Kept for the reasons it records.

## `position`, and which box an out-of-flow one is measured against (removed 2026-11-06 from 05-as-built.md)

**This used to be wrong, and silently.** The default was `relative`, which made
*every* box a containing block, which made "against the nearest positioned
ancestor" and "against the parent" the same sentence. They are not, and an
author writing `position: relative` on the right box as CSS requires was being
ignored and getting the right answer anyway. Only an unpositioned wrapper in
between told them apart. `fixed` was silently treated as `absolute`, so it
scrolled away with its ancestor; `sticky` and `static` were silently treated as
`relative`, so `static` even honored insets; and a misspelled value was
`relative` too, so a typo and a rule that does nothing looked identical.

## Honored CSS: percentages (removed 2026-11-06 from 05-as-built.md)

Until v0.7.1 a percentage on `font-size`, `letter-spacing`, `word-spacing` or a
border width was ignored in silence, because the interpreter read them with a
px-only parser while the length check validated with a percentage-capable one:
the value was dropped and then pronounced fine.

## Honored CSS: a border may differ per side (removed 2026-11-06 from 05-as-built.md)

Until v0.7.1 `border-bottom` drew *nothing*: the cascade computed all four
sides, and paint carried one width taken from the top, so `border-bottom` was
dropped and `border-top` was drawn on all four sides. Both silent, and
`border-bottom` is offered by the editor's completion list, which is supposed to
mean it works.

## Honored CSS: attribute selectors (removed 2026-11-06 from 05-as-built.md)

Until 2026-10-23 only `[role=…]` was read, and any other attribute test was
dropped from its compound without a word, so `input[type=checkbox]` styled every
input. `data-*` and `aria-*` were refused as unknown attributes.

## Honored CSS: pseudo-classes (removed 2026-11-06 from 05-as-built.md)

Before an unsupported pseudo-class never matched, the `:` was silently dropped,
so `.box:hover` parsed as `.box` and applied *unconditionally*. Failing closed is
the safer half of that trade.

## Honored CSS: units (removed 2026-11-06 from 05-as-built.md)

`rem` and `em` did not reach the box model before v0.7. `width` and `height`
understood `rem` from the start, while `padding`, `margin`, `gap`, border widths,
corner radii, `letter-spacing`, `box-shadow` and `translate()` went through a
px-only parser and dropped the declaration silently.

## Honored CSS: invalid values (removed 2026-11-06 from 05-as-built.md)

Until 2026-10-29 each property's reader guessed: `grid-template-rows: minmax 1fr`
(the function without its brackets) became the tracks `auto 1fr`, and since it
was the later rule it replaced Tailwind's `grid-rows-2`, where a browser ignores
it and keeps the two equal rows. Until v0.7.1 a typo got the same "not yet
honored" line a real unbuilt property got, so it read as a feature on its way and
an author could wait for a release that was never going to fix it.

## Honored CSS: font bitmaps (removed 2026-11-06 from 05-as-built.md)

vello takes any font with a bitmap strike (`EBLC`/`EBDT`, in Windows' Calibri and
Cambria) down its emoji path and dropped every glyph whose bitmap it could not
read, so until 2026-10-22 Calibri drew three glyphs out of a sentence (watchlist
#45). Such a font is handed to vello with those two tables hidden.

## Honored CSS: default colours (removed 2026-11-06 from 05-as-built.md)

Until 2026-10-23 Rux's window was a fixed `#11111b` with `#cdd6f4` text, which
nobody chose against CSS's, and text Tailwind leaves uncoloured came out faint.

## Keyed lists (removed 2026-11-06 from 05-as-built.md)

Before an `r-model` was addressed by `(model, row key)`, focusing one row put a
caret in all of them and lit every row's `:focus` rule at once. Before the loop
variable wrote through, every edit in a list row, typed, pasted or cut, was
thrown away on the next build without a word (found on the phone in inputs phase
5). An `r-model` that is a path (`user.name`) used to create a variable *named*
`user.name` and leave `user` untouched.

## Effects (removed 2026-11-06 from 05-as-built.md)

Until Tracer's AW-027 an effect stopped listening to everything it wrote, and
heard only the first push of a list it drained.

## Quiet CSS (removed 2026-11-06 from 05-as-built.md)

The table, list and `::backdrop` properties pass quietly because a warning each
opened every Tailwind app under a panel of things its author never wrote.
