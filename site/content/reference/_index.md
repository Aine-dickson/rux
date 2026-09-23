+++
title = "Reference"
description = "What Rux actually does today: the authoritative honored-CSS set, elements, and directives."
weight = 1
sort_by = "weight"
template = "docs-section.html"
page_template = "docs.html"
+++

<!-- GENERATED FROM docs/05-as-built.md BY site/sync-docs.sh. DO NOT EDIT HERE. -->


**This is the authoritative description of what Rux actually does today.**

Docs [01–04](https://github.com/Aine-dickson/rux/tree/main/docs) describe the *design intent* and are still worth reading
for the *why*, but the implementation has diverged from them in places. Where
they disagree, **this document wins**. Divergences are called out below. For what
is *not* built yet and in what order, see [Roadmap](/roadmap/).

This document says what each rule *does*. For how to build a particular piece of
an app, with the consequences of those rules worked through on a real file, see
the recipes at `/recipes/`: a message list, a tab bar and a modal.

Last updated: 2026-08-19, for **v0.7**. The original M0–M9 milestones that built
the runtime are all complete; everything since has shipped in the v0.2 through
v0.6 releases, which the [Roadmap](/roadmap/) lists.

---

## The `rux` command

Creating a project, running it, checking it and formatting it each have a page
under [Tooling](/tooling/), along with how to set up the VS Code extension.

## Crates

| Crate | Job |
|---|---|
| `rux-parser` | SFC split + XML-ish template parser (ours) |
| `rux-style` | lightningcss → our cascade → `Style`; directives; component expansion |
| `rux-script` | rhai engine (state + handlers) + `host::` registry |
| `rux-layout` | `Style` → taffy (flex/grid/block) → paint items, hit + focus regions |
| `rux-text` | parley 0.11 shaping/measure/wrapping + vello 0.9 glyph drawing |
| `rux-paint` | paint items → vello scene (fills, borders, clips, text) |
| `rux-runtime` | `Document`: load, resolve imports, build engine, rebuild tree |
| `rux-shell` | winit window, wgpu/vello, input, focus, clipboard, file watcher |
| `rux-cli` | `rux [file.rux]` |
| `rux-reactive` | just `Value`, the untyped value `rux-script` and `rux-style` pass around |

---

## What works

- [Elements](/reference/elements/): The six elements the runtime renders, plus slot, router and route.
- [Layout](/reference/layout/): Everything defaults to block; use display: flex. Hug, fill, and why inline flow is gone.
- [Paths](/reference/paths/): SVG path data as an element: the d attribute, paint as CSS, and shapes that morph.
- [Honored CSS](/reference/css/): The authoritative list of properties the runtime interprets, plus selectors, pseudo-classes and transitions.
- [Where Rux differs from CSS](/reference/css-differences/): The short list of places Rux does not answer the way CSS does, and which kind of difference each one is.
- [Reactivity](/reference/reactivity/): Signals, computed values, effects, and what re-runs when one changes.
- [Inputs](/reference/inputs/): Text fields, textarea, select, checkbox and radio, and two-way binding with r-model.
- [Text input](/reference/text-input/): The caret, the soft keyboard, and IME composition for text that is not typed one key at a time.
- [Touch](/reference/touch/): What a finger does today, and why @tap is the whole vocabulary.
- [Selection](/reference/selection/): Drag-select, double-click, and the clipboard keys.
- [Scrolling](/reference/scrolling/): Scrollers, scrollbars, and the ways a scroll can be driven.
- [Components](/reference/components/): Importing a file as a tag, props, slots, events, and what a component cannot see.
- [Routing](/reference/routing/): Routes, parameters, named routes, links, and the fact that the path is an ordinary signal.
- [Accessibility](/reference/accessibility/): The real accessibility tree, the roles elements map to, and what a screen reader is told.
- [Errors](/reference/errors/): What happens when a document will not load, and what the overlay shows.

### `position`, and which box an out-of-flow one is measured against

All five of `static`, `relative`, `sticky`, `absolute` and `fixed` mean what CSS
says.

**`static` is the default and is the only value that is not a containing block.**
That is the rule that gives the other three their meaning: an `absolute` box is
measured against its nearest ancestor that is *not* static, so a wrapper with no
`position` of its own is passed straight over, and `position: relative` on the
box you actually mean is what claims it. Its `inset` is ignored, which is the
whole difference between `static` and `relative`.

**This used to be wrong, and silently.** The default was `relative`, which made
*every* box a containing block, which made "against the nearest positioned
ancestor" and "against the parent" the same sentence. They are not, and an
author writing `position: relative` on the right box as CSS requires was being
ignored and getting the right answer anyway. Only an unpositioned wrapper in
between told them apart. `fixed` was silently treated as `absolute`, so it
scrolled away with its ancestor; `sticky` and `static` were silently treated as
`relative`, so `static` even honored insets; and a misspelled value was
`relative` too, so a typo and a rule that does nothing looked identical.

**`fixed` is against the window**, whatever it is written inside, and it is
outside every scroller, so it does not move when one scrolls. A fixed box with
no inset named lands in the window's top-left corner and warns, since that is a
legal answer to what was written and never what was meant. A sticky box with no
inset warns for the same reason: it has no edge to stick to and will never
move.

**A `transform` makes a containing block**, whatever the box's own `position`
says, and for `fixed` descendants as well as absolute ones. This is CSS's rule
and the reason `position: fixed` stops being fixed inside a transformed parent.
It is not an oddity to work around: a transform moves the whole subtree, so
there is no way to hold a descendant still against the window while its ancestor
slides. (CSS gives `filter`, `perspective`, `will-change` and `contain` the same
power; none of those is honored here, so `transform` is the only one that can.)

**`sticky` is in flow, and its insets are thresholds rather than offsets.** The
box sits where it was laid out until its scroller's edge reaches the threshold,
then rides that edge, and stops again when its own parent runs out from under
it. Inside a scroller the parent to stop at is the scroller's **content** box,
not the part of it on screen, which is itself sliding.

**Two sticky boxes never interact.** A list of sections looks as though an
arriving heading shoves the one at the top out of the way; neither can see the
other. Each is clamped to its own section, and one section's bottom edge is
exactly where the next section's heading begins, so "clamped to the end of my
section" and "pushed by the next heading" describe the same pixel. The
consequence is worth knowing before writing one: headings that are flat siblings
of the rows, with no box around each group, are all clamped to the scroller
instead, so they pin at the same edge and pile up on each other. The wrapper per
section is not tidiness, it is what makes the hand-over happen. That last clamp is what makes a list of sections work: a heading rides the
top until the next section arrives and pushes it off, rather than sitting over
the wrong rows. With no scroller above it, the window is what it sticks to.

Nothing else moves while it travels: a sticky box keeps its original space the
whole time, so its siblings do not reflow. It is resolved at paint time, because
it is a question about the scroll offset and the layout does not know one, and
its hit region and metrics move with it. A sticky box paints **over** its
in-flow siblings, as a positioned box does; `relative` boxes are not reordered,
which is a divergence.

**An out-of-flow box that names no inset keeps its static position**, which is
where it would have sat in its parent's flow, so it stays with its parent rather
than travelling to a containing block. That is what `:leave-to { position:
absolute }` relies on, and it is why a departing element needs no coordinates.

The containing block is the ancestor's **padding box**, so padding on it does not
push an out-of-flow child inwards.

**Whether a departing element keeps its place is yours to say.** Left alone it
stays in the flow until the swap commits, so nothing below it moves while it is
still on screen. Giving `:leave-to` a `position: absolute` hands its space over
at the *start* of the swap instead, which is what a page swap wants, since the
arriving page should take that space rather than queue below it. A box taken out
of the flow and naming no inset keeps the place it would have had, so it needs
no coordinates and no wrapper to be measured against.

On a list, `r-transition` needs `r-key` on the same element and says so if it
is missing: without a key there is nothing to hold a departing row by, and a
removal and a reorder are the same picture. A row that leaves from the middle
of a list animates out **where it was**, not at the end.

**Driving a swap yourself:** `:r-transition="expr"` hands progress to the
author instead of the clock. The expression is re-read every build and yields
0 to 1: reaching 1 commits the swap and returning to 0 abandons it. That is
what binds a swap to a finger, and it is why both branches have to be live: a
swap that can change its mind cannot be a snapshot of a departed tree.
```rux
<view class="card" r-if="card" :r-transition="dismiss" @drag="onDrag(event)">…</view>
```
```rux
if event.phase == "start" { card = false; dismiss = 0; }
else if event.phase == "move" { dismiss = event.totalX / 240; }
else if dismiss > 0.45 { dismiss = 1; }        // commit
else { card = true; dismiss = null; }          // and settle back
```
Yielding **`null` hands the swap back to the clock**, which runs the rest of
the declared duration from wherever the drag let go. That is how a released
finger settles instead of snapping. Under a bound driver the declared duration
does not set the pace; it still says which properties take part, and it takes
over again on the handover.

The condition is yours throughout. Abandoning a swap does not put it back:
the release handler that decides to abandon is the same one that restores the
condition, so what is on screen and what the signal says never disagree.

**Route transitions** are the same feature again: `r-transition` on the
`<router>` animates a navigation, holding the page being left on screen beside
the page being entered.
```rux
<router r-transition>
  <route path="/" view="home-page" />
  <route path="/crew/:id" view="crew-detail" />
</router>
```
```css
.page:enter-from { opacity: 0; transform: translateX(28px); }
.page:leave-to   { opacity: 0; transform: translateX(-28px); }
```
The identity is **which route matched, not which path**. Two paths matching the
same route (`/crew/grace` and `/crew/kim`) are one page showing different data,
so they update in place rather than crossing over, the same way a router reuses
a component. The outgoing page's `unmounted` runs when the transition
**commits**, so a navigation that reverses mid-swap never fires one.

A third tier, keyframes, is not built. Driven in `examples/enter-leave.rux` and
`examples/router.rux`.

**Computed values:** `computed name = expr;` in `<script>` declares derived
state, written once and readable anywhere a signal is:
```rux
let qty = signal(2);
let price = signal(12);
computed subtotal = qty * price;
computed total = subtotal + subtotal / 10;   // may read the one above it
```
A computed *is* a signal: the line is rewritten to a plain `let`, so `{{ total }}`
tracks it like any other, and it re-evaluates when what it reads changes. Only a
real change propagates, so a computed landing on the same answer patches nothing.

Refreshing is **one pass in declaration order**, so a computed may read
computeds declared above it and not below. That is a deliberate limit rather
than a fixpoint loop, which would turn a circular typo into a hang.

**Effects:** `effect { … }` runs statements when what they read changes, **and
once on load**, so an effect can establish something rather than only react to a
later edit:
```rux
effect {
  status = if total > 100 { "over budget" } else { "ok" };
}
```
An effect subscribes to what it actually read on its last run, so a signal it
never touched does not wake it, and a conditional branch changes what it
watches.

**An effect is never woken by its own writes.** Assigning to a signal also
resolves its name, so the tracker cannot tell the write from a read; without
this rule every effect that wrote anything would re-trigger itself. The cost is
that an effect writing `x` will not re-run when something *else* changes `x`,
which is the right way round: that effect is the one deciding what `x` is.
Effects that feed *each other* still cycle; that is stopped after 8 rounds and
reported in the overlay rather than hung on.

Both are document-level today: a component's own `computed`/`effect` lines are
stripped, not run. Driven in `examples/computed.rux`.

**Keyed lists:** `r-key` on the same element as `r-for` says what a row *is*,
rather than where it sits:
```html
<view r-for="item in items" r-key="item.id"> … </view>
```
The key is evaluated once per row with that row's loop variable in scope.
Duplicate keys warn (two rows claiming one identity is worse than none), and so
does an `r-key` on an element with no `r-for`.

**A key is what makes an input inside a list work at all.** An `r-model` is
stored **as written**, so every row of a list carries the same one, and an
identity taken from it alone cannot tell two rows apart. Everything that
addresses an input is now `(model, row key)`: the caret and selection, `:focus`
matching, and the value the shell reads and writes. Before this, focusing one
row put a caret in **all** of them and lit every row's `:focus` rule at once.

The value is read and written **in the row's own scope**, using the loop
variables captured where the input was built, so a model may mention the loop
variable:
```html
<input r-for="item in items" r-key="item.id"
       r-model="items[item.at.to_int()].note" />
```
Writing goes through an assignment rather than setting a scope variable, so an
`r-model` that is a path (`user.name`, `items[0].note`) now writes through to
the real target. It previously created a variable *named* `user.name` and left
`user` untouched, in or out of a list.

Two consequences worth knowing. Numbers are f64, so an index needs `to_int()`.
And the caret follows its row across a reorder with nothing to remap, because
the identity *is* the row; tapping a button to reorder still moves keyboard
focus to that button, as any tap on a button does.

**Still keyed by model alone:** `type="select"`. A `<select>` inside an `r-for`
has the same ambiguity inputs had. Driven in `examples/keyed-list.rux`.

**A document's rules reach its components.** A `<style>` block styles its own
markup *and* the components the document uses, so a look is written once at the
top instead of imported into every component file:

```xml
<!-- app.rux -->
<style>
  .chip { padding: 0.75rem; border-radius: 8px; background: #a6e3a1; }
</style>
```
```xml
<!-- components/chip.rux: no <style> at all, and still green -->
<template><view class="chip"><text>{{ label }}</text></view></template>
```

A component's own rules are applied **after** the ones it inherits, so it wins a
tie without needing a more specific selector, which is CSS's own order.

`<style scoped>` opts out, and means the same thing from either side:

- On a **component**: "I own my appearance." Nothing from outside styles it.
- On a **document**: "my rules stay in my markup." They reach no component.

> **This changed in v0.7.** A component used to see only its own `<style>`, so
> sharing a palette meant repeating `<style src="theme.css">` in every single
> component. If a component and its caller happen to use the same class name and
> you want the old isolation, that is what `scoped` is for.

**Custom properties already cascaded**, before and after this change: a
`--brand` defined on the document has always been readable as `var(--brand)`
inside a component, because variables inherit down the tree rather than being
matched by a selector.

**External stylesheets:** `<style src="…">` pulls in one or more `.css` files,
so a palette can be shared instead of pasted into every document:
```html
<style src="palette.css, cards.css">
  .app { background: var(--bg); }   /* the document's own rules, as before */
</style>
```
Paths are relative to the **file that names them**, the same rule as `use`
imports and `<image src>`, so a component's include is relative to the
component. Comma-separated, in the order written.

Included sheets cascade **before** the `<style>` body, so a rule in the
document beats a rule of the same specificity in the include. That is what
makes including a palette useful: you pull one in to override part of it, and
needing `!important` to do so would mean the include had been layered on top
instead of underneath.

A stylesheet that is not there **fails the load**, like a missing component,
and the overlay names the path. A document that quietly renders unstyled reads
as a layout bug, which is a much longer walk back to a typo. Editing an
included `.css` hot-reloads the window, same as editing the `.rux`.

The playground is the exception: it has source text and no file, so there is
nothing for a path to be relative to and nothing to read. An include there is
ignored, with a warning saying exactly that. Driven in
`examples/shared-style.rux`, which shares `examples/palette.css`.

**Custom properties + `var()`:** `--name: value` declarations **inherit** down the
tree (like `color`), so a palette declared once is readable anywhere below:
```css
.app        { --brand: #89b4fa; --radius: 10px; }
.btn        { background: var(--brand); border-radius: var(--radius); }
.app.light  { --brand: #1e66f5; }   /* same sheet, different values */
```
Substitution happens after the cascade *and* inline styles merge, so `var()` works
in every property, in `style=` and in `:style`. Supported: fallbacks
(`var(--x, 12px)`, including fallbacks with their own parens), variables defined
in terms of other variables, and overriding a variable on any element to retheme
its subtree. A cycle terminates rather than hanging.

An **undefined** variable with no fallback makes the declaration invalid, so it is
dropped (as in CSS) and warned about once. Driven in `examples/theme.rux`, which
swaps a whole palette with one `:class`.

**`env()`: the strip of the display the device has already taken.**
```css
.bar  { padding-top: env(safe-area-inset-top); }      /* under a notch */
.dock { padding-bottom: env(safe-area-inset-bottom); } /* above a home indicator */
.app  { --gutter: env(safe-area-inset-left, 0px); }    /* a fallback, as in CSS */
```
Four names, `safe-area-inset-top`, `-right`, `-bottom` and `-left`, resolving to
a length. **On a desktop window every one of them is zero**, and zero is an
answer rather than a placeholder: a window that owns its whole surface has no
unsafe edges, so the same stylesheet is correct in both places and the phone
simply has more to avoid.

It resolves wherever `var()` does, a custom property's own value included, so
`--gutter: env(safe-area-inset-bottom)` is the way to write it once. A fallback
covers a name Rux cannot answer, **not** a known name answering zero:
`env(safe-area-inset-top, 20px)` on a desktop is 0, not 20, which is what CSS
does and is the only reading that lets a fallback mean "you are somewhere that
has no such thing". An unknown name with no fallback makes the declaration
invalid and is warned about once, the same as an undefined variable.

The other `env()` names in CSS (`titlebar-area-*`, `viewport-segment-*`) name
surfaces Rux does not have, so they are unknown here; answering for one would be
inventing a number rather than reporting one.

To see a non-zero inset on a desktop, see `rux run --preview` below.

**`@media` queries:** evaluated against the window's **logical** size.
```css
@media (max-width: 600px) { .row { flex-direction: column; } }
@media screen and (min-width: 400px) and (max-width: 600px) { … }
@media (max-width: 400px), (min-width: 1000px) { … }   /* alternatives */
@media (orientation: portrait) { … }
@media (prefers-color-scheme: dark) { .app { background: #11111b; } }
@media (prefers-reduced-motion: reduce) { … }
@media (min-resolution: 2dppx) { .logo { background-image: url(logo@2x.png); } }
```
Supported features: `min-`/`max-width`, `min-`/`max-height`, `orientation`,
`resolution` (`dppx`, `x`, `dpi`, `dpcm`), `prefers-color-scheme`,
`prefers-reduced-motion`, the
`screen`/`all` types, `and` chains and comma alternatives. A block adds **no
specificity**: rules inside it cascade by ordinary source order, so a later
`@media` rule beats an earlier plain one, and `#id` still beats a `.class` in a
media block. Anything else (`not …`, `(hover)`) warns once and never applies.

**`orientation` and `resolution` are answered by the device**, and both were
driven on one: rotating a phone re-cascades live, and a Pixel 6 reports 2.625,
so `min-resolution: 2dppx` matches there and `3dppx` does not.

Resizing re-cascades **only when a query changes answer**: dragging a window
edge within a breakpoint costs nothing, and a document with no `@media` never
re-cascades at all. Driven in `examples/responsive.rux`.

**The two preference queries are asked of the operating system, not guessed.**
`prefers-color-scheme` comes from the window's own theme, which every platform
that has a notion of one answers, and a change made while the app is running
arrives as an event, so flipping the system between light and dark repaints
within a frame. `prefers-reduced-motion` is asked of Windows directly, since
winit exposes nothing for it, and it is re-read when the environment is next
rebuilt, which is at startup and on a resize: turning that setting off
mid-session is not noticed until then. A stylesheet that never mentions reduced
motion is stilled anyway, because honoring only the written query would leave
almost every app animating at someone who asked it not to.

`flex: 1` means `1 1 0%` (CSS's shorthand defaults), not `1 1 auto`.
`opacity` fades the node **and its subtree** as one layer.
`background`/`border` work on `<text>` nodes, not just containers.
**Units:** `px`, `%`, `rem` (=16px), `em`, `vw`, `vh`/`dvh`.

`em` is relative to the element's own resolved `font-size`, and on `font-size`
itself it is relative to the inherited one, which is what it means in CSS. It is
resolved in a pass before the properties are interpreted, the same way `var()`
is, so it works anywhere a length does.

> **`rem` and `em` did not reach the box model before v0.7.** `width` and
> `height` understood `rem` from the start, while `padding`, `margin`, `gap`,
> border widths, corner radii, `letter-spacing`, `box-shadow` and `translate()`
> went through a px-only parser and **dropped the declaration silently**. So
> `width: 2rem` worked, `padding: 2rem` did nothing, and nothing said why. `%`
> is still only honored where the list below says so; it is not a box-model
> unit.

`font-family` takes a CSS list (`font-family: "Inter", sans-serif`), and parley
parses it and does name-matching + fallback; the generic families (`serif`,
`sans-serif`, `monospace`, …) always resolve. It **inherits**, like `color` and
`font-size`. `color`/`font-size`/`font-family` are the three inheriting text
properties.

Anything else is **parsed but not honored**, and never *silently*: the runtime
prints one line per unhonored property, once each. It says which of three things
happened, because they are not the same problem:

| You wrote | It says |
|---|---|
| `outline: 1px solid red` | ``CSS property `outline` is real CSS that Rux does not honor yet, so it will have no effect`` |
| `paddding: 8px` | ``` `paddding` is not a CSS property Rux knows, so it will have no effect. Did you mean `padding`? ``` |
| `florble: 3` | ``` `florble` is not a CSS property Rux knows, so it will have no effect ``` |

The middle case is the one worth having. Until v0.7.1 a typo got the same "not
yet honored" line a real unbuilt property got, so it read as a feature on its
way and an author could wait for a release that was never going to fix it.

Real CSS Rux has not built includes `outline`, `z-index`, `box-sizing`,
`transform-origin`, `visibility`, `filter`, `text-transform`, `background-size`,
`list-style` and the `animation` family. `line-height`, `box-shadow`,
gradients, `transform`, CSS variables and all five `position` values used to be
on that list and are honored now.

Colours accept `#hex` (3/6/8-digit), `rgb()`/`rgba()`, and the full CSS named-
colour list (`red`, `rebeccapurple`, …). The named list matters because
lightningcss *minifies* hex to keywords (`#ff0000` → `red`), so without it a
plain `color: #ff0000` would fall back to the default.

### Numbers, switches and sliders

`<input type="number" r-model="qty">` is a one-line field whose signal **always
holds a number**, never the text of one, so `qty + 1` is arithmetic. While what
is typed is not a number yet (`-`, `1.`, an emptied field) the signal keeps the
last number it had and the field shows the typing, the same way a composition
lives in the field before it is committed. Nothing typed is refused: a letter
stays in the field and changes nothing. Leaving the field shows the number
again.

**The decimal point is the person's own**, read from the device's language
(Android's locale, Windows' regional setting, the browser's language; a dot
elsewhere). The other of comma and dot is a thousands separator and is
dropped, as are spaces, but only before the point:

| Typed | English (`.`) | German (`,`) |
|---|---|---|
| `24,000` | 24000 | 24 |
| `2,5` | 25 | 2.5 |
| `1.234,5` | not a number yet | 1234.5 |

That is more forgiving than HTML and Android, which both refuse `24,000` in a
number field, and it keeps the rule that nothing typed is refused. `@input` fires when the *number*
changes, and every event hands over the number as `event.value`. A phone raises
a number keyboard with a minus sign and a decimal point; an `inputmode` still
wins, so `inputmode="numeric"` gives a digits-only pad.

`<input type="switch" r-model="wifi">` is a checkbox in all but looks: a pill
track with a round thumb at the end the value points to, announced to a screen
reader as a switch. `:checked` matches it on, and `@change` fires on each flip.
Unstyled it is 44 by 24 with a grey track that turns blue when on; `width`,
`height`, `padding`, `background` and `color` (the thumb) override each part,
and `accent-color` sets the "on" track alone:

```css
.wifi         { accent-color: #a6e3a1; }
.wifi:checked { background: #40a02b; }   /* or style the on track directly */
```

`<input type="slider" r-model="volume" min="0" max="11" step="1">` holds a
number between `min` and `max` (0 and 100 when left off), moving in steps of
`step` (1; `step="any"` for none). The value is rounded to as many decimal
places as the step is written with, so ten steps of `0.1` read `1`. A **tap**
puts the thumb where it lands and a **sideways drag** moves it. The drag is an
ordinary `@drag` on the element, so the axis claim applies: a vertical swipe
that starts on a slider inside a scrolling page scrolls the page. `@input`
fires as the value moves and `@change` when the finger lifts, as HTML has them
for a range. `accent-color` colours the fill and the thumb; the element's own
`height` (32 unstyled) is the touch target, and the bar is drawn centred in it.
A `min`, `max` or `step` that is not a number, or a `max` not above `min`, is an
error naming the line.

`<input type="date" r-model="due" min="2026-01-01" max="2026-12-31">` holds a
day as `YYYY-MM-DD`, the format HTML's date input holds, or an empty string.
**On Android a tap opens the platform's date picker**, starting on the day the
field holds (today when empty) and offering only the days between `min` and
`max`. Choosing commits at once and fires `@change`, as a select does;
dismissing leaves the day alone. A read-only date offers no picker. Elsewhere,
until a drawn picker exists, the field is typed into with the rule a number
follows: only a real day inside the range reaches the signal (`2026-9-3` is
written back as `2026-09-03`), and anything short of one stays in the field. A
`min` or `max` that is not a date is an error.

Not yet: arrow keys on a focused slider, a thumb that slides rather than jumps
between the switch's two ends, a drawn date picker on desktop, and the
browser's own date picker on the web.

### Field attributes and events

The attributes are HTML's, named and valued as HTML names them, and they mean
what HTML means by them.

| Attribute | On | What it does |
|---|---|---|
| `disabled` | `<input>`, `<button>` | No tap, no link, no gesture, no focus, and nothing for Tab or a label's `for=` to reach. The value still shows. Matched by `:disabled` |
| `readonly` | text inputs | Focusable, selectable and copyable. Every edit is refused, the toolbar offers only Copy and Select all, and no keyboard opens |
| `maxlength` | text inputs | The most text the field takes, in UTF-16 units as HTML counts. Typing past it is refused; a paste is cut short. The *inserted* text is what gets cut, so typing into the middle of a full field does not eat its end. Not applied mid-composition, where the commit is cut instead |
| `inputmode` | text inputs | Which keyboard a phone raises: `text`, `numeric`, `decimal`, `tel`, `email`, `url`, `search`, or `none` for no on-screen keyboard at all |
| `enterkeyhint` | text inputs | What the action key says: `enter`, `done`, `go`, `next`, `previous`, `search`, `send` |
| `autofocus` | text inputs | Takes focus when it first appears, if nothing has focus |

`disabled` and `readonly` are **boolean attributes**: written means on, whatever
they say, so `disabled="false"` is disabled, exactly as in HTML, and warns.
`:disabled="expr"` and `:readonly="expr"` are the bound forms, and a change to
what they read restyles the element in place. An unknown `inputmode` or
`enterkeyhint`, or a `maxlength` that is not a whole number, is an error naming
the line, because each would otherwise give an ordinary field in silence.

**`inputmode` is a keyboard, not a type.** `email`, `tel` and `url` are text
fields that want different keys, so they are hints and not `type=` values. A PIN,
a card number or a phone number is a digit *string*: `inputmode="numeric"`,
never a number. On a password field only `numeric` changes the keyboard (to a
PIN pad that still learns nothing), because trading the password variation for
an email layout would give up the no-learning guarantee.

**Enter in a one-line field commits it.** It fires `@change` if the value
changed, then does what `enterkeyhint` says the field can do by itself: `next`
and `previous` move focus as Tab and Shift+Tab do, `done` drops focus and the
keyboard. `go`, `search` and `send` name what the *app* will do, which is its
`@change`. On Android the action key is a real Enter, so all of this is one path.

Rux draws no default look for a disabled control, the same way it draws no
default look for a button. Style `:disabled`:
```css
.field:disabled { color: #6c7086; border-color: #313244; }
button:disabled { opacity: 0.5; }
```

**Events.** Four, on an input, each handed `event.value`:

| Event | When |
|---|---|
| `@input` | After every change to the text, however it was made: a key, a paste, a cut, a phone's keyboard |
| `@change` | A changed value is committed: the field is left, or Enter is pressed in a one-line field. A select fires it on choosing a different option, a checkbox or radio on every toggle |
| `@focus` | The field gained focus |
| `@blur` | The field lost focus, after its `@change` when both fire |

```xml
<input r-model="query" enterkeyhint="search" @change="run_search(event.value)" />
<input r-model="code" inputmode="numeric" maxlength="6" autofocus />
<button :disabled="code.len() < 6" @tap="verify()">Verify</button>
```

Handlers run **after** the edit or the focus change that caused them is
complete, never from inside it, so a handler that writes the field's own
signal, focuses another field or blurs this one sees the field as the person
does. A `@blur` that focuses the field it left, answered by a `@focus` that
blurs it, is stopped after 64 handlers with a warning. A handler written in a
component runs in that instance, and one in an `r-for` row sees the row.

Not yet: `@submit` and `role="form"` (Enter in a field does not submit
anything), validation (`required`, `pattern`), and `autocomplete`.

### The pointer vocabulary

Beyond `@tap`, five attributes report what a finger or button is doing:
`@press`, `@release`, `@longpress`, `@swipe` and `@drag`. `@tap` is deliberately
not one of them: it is the finished gesture, it is what a keyboard activation
produces and what `tap()` from script synthesises, and none of those has a
pointer at all. So an element with only `@drag` is hit-tested but is not a tap
target and is not keyboard-activatable, and it does not swallow a tap meant for
what is under it.

Every handler, `@tap` included, is handed an `event`:

| Field | What it is |
|---|---|
| `x`, `y` | the pointer, relative to the element the handler is on |
| `pageX`, `pageY` | the same point, relative to the window |
| `width`, `height` | the element's own size, so `event.x / event.width` is how far across it the pointer is |
| `touches` | every finger down, each with `id`, `x`, `y` |

`touches` is **a list even when there is one finger**, and a mouse counts as one
finger with `id` 0. That shape is the point: a two-finger gesture can arrive
later without changing what any handler already written reads.

`@swipe` adds `direction`, chosen by the dominant axis. `@drag` adds `phase`
(`start`, `move`, `end`). Both add two distances, named so neither can be read
as the other: `totalX` / `totalY` from where the press landed, and `moveX` /
`moveY` from the previous event. Following a finger wants the first; velocity
and flick detection want the second.

**A drag that ends as a flick also fires `@swipe`.** They are not rivals: a page
that follows the finger still has to be told, at the end, whether the hand meant
to throw it, which is how a reversible transition decides whether to commit.
They were exclusive at first, and that made `@swipe` unreachable on any element
that also declared `@drag`, since movement starts the drag first. A long press
stays exclusive, because a press that moved is not resting.

**The axis claim, settled on hardware 2026-09-21.** A `@drag` used to take every
finger that landed on it, in any direction, and the page under it never scrolled.
That is fine on a desktop and wrong on a phone: it made a draggable row inside a
scrolling list a **dead zone**, where a thumb that happened to start on the wrong
element could not scroll the page at all. Driven on a device before the change: a
purely vertical drag beginning on a `@drag` box fired six drag events and moved
the list by nothing.

**The winner is decided by direction, once.** At the moment the finger passes
`TAP_SLOP`, the dominant axis is compared against what a scroller under the press
can actually travel on. If some scroller can move on that axis, it takes the
gesture and `@drag` never starts for that press. Otherwise the element keeps it.
The answer is never revisited: a gesture that changes owner under the hand is
visibly wrong, and no platform does it. iOS arbitrates its pan recognizers on the
initial translation, Android intercepts at the slop crossing, and the web decides
ahead of time from `touch-action`.

So the question this was filed under, "can a scroll take the finger back
mid-gesture", turned out to be the wrong question. Nothing is taken back. The
decision simply happens earlier, and on direction rather than on time.

**"Can travel" is stricter than "is a scroller".** A box whose content fits has
no room on that axis, so it does not take the gesture: winning a finger and then
doing nothing with it is the worst of both answers. This is also what makes a
horizontal carousel inside a vertical page work with **no CSS at all**, since
nothing can scroll sideways there and the drag keeps the axis it needs.

**`touch-action` overrides it**, with CSS's own values: `none` gives the element
the finger outright, `pan-x` and `pan-y` reserve one axis for the scroller, and
the default `auto` is the rule above. The default is deliberately not "the
element wins", because that was the behaviour that produced the dead zone.

**It is touch only.** A mouse scrolls by wheel and never by dragging, so the
mouse path has no drag-scroll to hand a gesture to; letting a scroller win there
would stop drags working on the desktop for nothing gained.

**A laptop touchpad reaches the app as a mouse**, so it reports one finger
however many are on the pad. Only a touchscreen, or a browser's touch emulation
against the wasm build, exercises the list.

**Touch text has its own gestures**, added in v0.5.1 and confirmed on hardware.
Until then a finger dragged across text *selected*, because touch was routed
down the same press/drag/release path a mouse takes, which is the desktop model
and not what a phone does:

- **drag** on text moves the caret along the path;
- **long press** (500 ms) selects the word under the finger;
- **long press then drag** extends the selection from that word.

The long press is therefore the only gesture that selects, which is what frees a
drag to mean something else. The decision is one-way: a press that moves before
the timer is a caret drag and cannot become a selection however long the finger
then rests, so a drag never turns into a selection halfway through. A resting
finger raises no events, so the press deadline is a second clock in
`about_to_wait` beside the caret blink. The mouse is unchanged and still
drag-selects.

## Gotchas (these will bite)

1. **String literals in attributes need single-quoted attrs:**
   `@tap='name = ""'`, `r-if='city != ""'`. We do **not** decode HTML entities,
   and rhai treats `'x'` as a *char*, not a string.
2. **`use` must be alone on its own line** in `<script>`.
3. **A `fn` called in method style cannot see the surrounding scope.**
   `helper(thing)` reaches the state around it; `thing.helper()` does not. This
   is all that is left of what used to be the single biggest trap here, "rhai
   `fn`s can't touch globals", which v0.7 removed (see above).
4. **`text-align` needs a box wider than the text** (set a width, or the element
   must fill), or there's nothing to align within.
5. **A scroll container needs a bounded height** (`height`, `max-height`, or a
   `flex-grow` slot). Without one it just grows and there is nothing to scroll.
6. **Rows inside a scrolling flex column need `flex-shrink: 0`.** Otherwise the
   column squeezes them all in to fit and, again, nothing scrolls. CSS does this
   too. It is the single most common "why won't it scroll" trap.
7. **A word longer than its box overflows** unless you set `overflow-wrap:
   break-word`, since nothing can shrink below min-content. The browser does this too.

---

## Known gaps / backlog

- **The soft keyboard has not been tried on real phone hardware.** It is driven
  and passing under browser touch emulation, tap through to committed CJK (see
  "Text input and composition"), so the mechanism works: the hidden `<input>` is
  created on demand, focused by the tap, and laid over the field. What emulation
  cannot show is a keyboard physically rising, since that is the OS's decision.
  Still worth thirty seconds on a real phone. Note it only reaches ruxlang.dev
  once a tag carrying it has been deployed, because the playground is built from
  the latest release rather than from `main`, and a tag push does not itself
  trigger a deploy.
- Text editing: no word-wise movement (Ctrl+arrows), no triple-click line-select,
  no drag-and-drop of selected text, no `::selection` styling.
- Scrolling: no track-click paging, no kinetic touch fling, no scrollbar
  hover/fade, and `overflow-x` / `overflow-y` can't differ from each other.
- CSS: `box-shadow`, `position`/`top`/`left`, per-corner radius, per-side border
  *colors*.
- True inline text-flow (taffy can't; would need our own line-breaker).
- `r-for` rebuilds more rows than a keyed diff would; effects and computed
  values are still absent from the reactive tier.

> Fine-grained reactivity **shipped in v0.3**: a signal change now patches only
> the bindings that read it, and the wholesale rebuild no longer fires. This list
> claimed otherwise until 2026-07-26. If a gap here reads as more pessimistic
> than the [release blog](/blog/), trust the blog and fix this
> file.

---

## Where the design docs are still right

The [rationale](/reference/rationale/)'s core laws still hold and still guide changes:
**layout lives in CSS, not markup** (no `<Padding>`/`<Center>` widgets); **reuse
mature crates**; **keep the element set tiny**. The [architecture](/contribute/)
pipeline (parse → cascade → layout → paint → present, with a file watcher) is
exactly what got built. Only the *reactive graph* stage is simpler than described.
