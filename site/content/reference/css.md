+++
title = "Honored CSS"
description = "The authoritative list of properties the runtime interprets, plus selectors, pseudo-classes and transitions."
weight = 4
+++

<!-- GENERATED FROM docs/05-as-built.md BY site/sync-docs.sh. DO NOT EDIT HERE. -->

```
display (block|flex|grid|inline|none)
flex-direction, justify-content, align-items, gap, row-gap, column-gap
align-self, justify-self, justify-items, align-content
flex-grow, flex-shrink, flex-basis, flex-wrap, flex (shorthand)
grid-template-columns, grid-template-rows
grid-column, grid-row (+ -start/-end)   (1 / 3, span 2, -1; no named lines)
grid-auto-flow, grid-auto-rows, grid-auto-columns
transform (translate/scale/rotate; visual only; hit regions aren't transformed)
position (relative|absolute) + top/right/bottom/left, aspect-ratio
width, height, min/max-width, min/max-height
padding, margin        (shorthand 1–4 values + -top/-right/-bottom/-left)
border, border-width, border-color, border-<side>, border-<side>-width
background / background-color / background-image, opacity
  (colour, linear-/radial-gradient, or url(…) image, cover-sized, clipped to corners)
box-shadow (single, outer; inset parsed but not drawn)
transition (property duration easing delay, comma-separated; see below)
border-radius (1–4 diagonal shorthand + per-corner -top-left/-top-right/…)
color, font-size, font-weight, font-family, font-style (italic), text-align
letter-spacing, word-spacing, line-height, white-space (nowrap|pre)
text-decoration (underline / line-through)                (color: hex, rgb()/rgba(), CSS names)
overflow / overflow-x / overflow-y   (hidden|clip = clip; auto|scroll = scroll;
                                      both axes together; x and y can't differ)
overflow-wrap (break-word), word-break (break-all)
cursor (pointer, on @tap boxes only)
```
**Selectors:** tag, `.class`, `#id`, `[role="…"]`, compounds, and all four
combinators: descendant (`.a .b`), child (`.a > .b`), next-sibling (`.a + .b`),
subsequent-sibling (`.a ~ .b`).

**Pseudo-classes:** `:hover`, `:focus`, `:active`, `:checked`, `:current` (a
link whose `to` names the path you are on), and `:enter-from` / `:leave-to`
(the two sides of an enter/leave swap, below). They stack
(`.btn:hover:active`), count as class-level specificity, and work anywhere in a
chain, `.card:hover .title` recolours the title while the pointer is over the
card. `:hover`/`:active` hold for the whole chain under the pointer, as in CSS;
`:active` is press-to-release and drops if you drag off the element; `:focus`
matches the input holding the caret. Driven in `examples/pseudo.rux`.

Any *other* pseudo-class (`:disabled`, `:nth-child(…)`, `::selection`) **never
matches**, and says so once on stderr. Before this existed the `:` was silently
dropped, so `.box:hover` parsed as `.box` and applied *unconditionally*, failing
closed is the safer half of that trade.

**Transitions:** `transition` walks a property to its new value instead of
jumping to it, whatever moved it: a signal, a `:class`, or a pseudo-class.
```css
.card { transition: background-color 200ms ease-out, transform 200ms ease-out; }
.card:hover { background: #45475a; transform: translateY(-4px); }

.panel { height: 0; opacity: 0; transition: all 250ms ease-in-out 50ms; }
.panel.open { height: 60px; opacity: 1; }
```
Each entry is a property, a duration, an easing and a delay, in any order after
the property. As in CSS the **first** time is the duration and the second is the
delay, `all` stands for every animatable property, and a bare `transition: 200ms`
means `all 200ms`. Easings: `linear`, `ease` (the default), `ease-in`,
`ease-out`, `ease-in-out`, `cubic-bezier(x1, y1, x2, y2)`. `steps()` is not
supported.

**Animatable:** `opacity`, `background-color`, `color`, `border-color`,
`border-width`, `border-radius`, `width`, `height`, `padding`, `margin`, `gap`,
`font-size`, `transform`, and the insets (`top`/`right`/`bottom`/`left`, which
animate together). Naming anything else warns and lists what is animatable,
rather than leaving you with an element that silently never moves. A longhand of
an animatable shorthand (`padding-left`) is pointed at the shorthand: the four
sides animate as a unit.

Three limits, all of them deliberate:
- **A value that has no midpoint jumps.** `10px` → `50%` needs a layout to
  resolve, and a colour becoming a gradient has no halfway. Same-unit lengths
  interpolate; anything else lands at once.
- **Enter and leave are opt-in.** Unmarked, a node still arrives at its
  authored style and a node the build stops reaching is simply gone. Add
  `r-transition` to animate the way in and out; see below.
- **`transition` does not inherit**, exactly as in CSS. A parent's `color`
  change does not animate a child's text; put the transition on the element
  whose style is moving.

An app with no transitions running is still fully event-driven: it sleeps
waiting for real events and renders nothing. Frames are scheduled only while
something is actually in flight, and stop the frame it lands. Driven in
`examples/transition.rux`.

**Enter and leave:** `r-transition` on an element with `r-if` (or on a keyed
`r-for` row) says that its arrival and departure are animated. What the two
sides look like is CSS, on `:enter-from` and `:leave-to`; how long the swap
lasts is the element's own `transition`.
```rux
<view class="panel" r-if="open" r-transition>…</view>
```
```css
.panel { opacity: 1; transform: translateY(0px);
         transition: opacity 300ms ease-out, transform 300ms ease-out; }
.panel:enter-from { opacity: 0; transform: translateY(-16px); }
.panel:leave-to   { opacity: 0; transform: translateY(-16px); }
```
The two sides are ordinary rules and the walk between them is the same
machinery a `:hover` uses. `:enter-from` is worn for one frame and dropped by
the next, which is what turns an arrival into an ordinary style change;
`:leave-to` is held from the moment the swap opens until it commits.

While a swap is pending **both branches are really there**: laid out, styled,
and still updating. That is why the condition changing back mid-swap reverses
the swap instead of stacking a second one, and why a departing component's
`unmounted` fires when the swap **commits** rather than when it starts. A
cancelled swap never fired one.

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

**`@media` queries:** evaluated against the window's **logical** size.
```css
@media (max-width: 600px) { .row { flex-direction: column; } }
@media screen and (min-width: 400px) and (max-width: 600px) { … }
@media (max-width: 400px), (min-width: 1000px) { … }   /* alternatives */
@media (orientation: portrait) { … }
```
Supported features: `min-`/`max-width`, `min-`/`max-height`, `orientation`, the
`screen`/`all` types, `and` chains and comma alternatives. A block adds **no
specificity**: rules inside it cascade by ordinary source order, so a later
`@media` rule beats an earlier plain one, and `#id` still beats a `.class` in a
media block. Anything else (`min-resolution`, `not …`, `(hover)`) warns once and
never applies.

Resizing re-cascades **only when a query changes answer**: dragging a window
edge within a breakpoint costs nothing, and a document with no `@media` never
re-cascades at all. Driven in `examples/responsive.rux`.
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

Anything else is **parsed but not honored**: but no longer *silently*: the
runtime now prints one line per unhonored property (`rux: CSS property
\`box-shadow\` is parsed but not yet honored …`), once each. Notably absent:
`line-height`, `position` (relative/absolute *is* honored; `sticky`/`fixed` are
not), `box-shadow`, gradients, `transform`, and CSS variables.

Colours accept `#hex` (3/6/8-digit), `rgb()`/`rgba()`, and the full CSS named-
colour list (`red`, `rebeccapurple`, …). The named list matters because
lightningcss *minifies* hex to keywords (`#ff0000` → `red`), so without it a
plain `color: #ff0000` would fall back to the default.
