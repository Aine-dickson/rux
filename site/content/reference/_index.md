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

Last updated: 2026-08-08, for **v0.5.0**. The original M0–M9 milestones that
built the runtime are all complete; everything since has shipped in the v0.2
through v0.5 releases, which the [Roadmap](/roadmap/) lists.

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
`line-height`, `box-shadow`, gradients, `transform`, and CSS variables.
`position` is no longer among them: all five values are honored.

Colours accept `#hex` (3/6/8-digit), `rgb()`/`rgba()`, and the full CSS named-
colour list (`red`, `rebeccapurple`, …). The named list matters because
lightningcss *minifies* hex to keywords (`#ff0000` → `red`), so without it a
plain `color: #ff0000` would fall back to the default.

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

**A `@drag` claims the finger**: the page under it does not scroll while the
drag runs. That is the settled half of the axis-claim rule. Whether a scroll can
take the finger back mid-gesture is deliberately undecided until there is touch
hardware to argue with.

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
