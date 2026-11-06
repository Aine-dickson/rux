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

---

## The `rux` command

Creating a project, running it, checking it and formatting it each have a page
under [Tooling](/tooling/), along with how to set up the VS Code extension.

## Crates

| Crate | Job |
|---|---|
| `rux-parser` | SFC split + XML-ish template parser (ours) |
| `rux-style` | lightningcss → our cascade → `Style`; directives; component expansion |
| `rux-syntax` | Rux's own parser for the script language: tokens, AST |
| `rux-script` | Rux's type checker and interpreter (state + handlers), and native modules in the language |
| `rux-ir` | The typed IR between the checker and every backend: its verifier and printer |
| `rux-codegen` | Rust generated from the typed IR, for release builds |
| `rux-aot-tests` | Runs the corpus's scripts compiled by `rux-codegen` beside the interpreter, and compares |
| `rux-native` | What an app's Rust depends on (as `rux`): `#[rux::export]`, the boundary value, the registry |
| `rux-native-macros` | The attributes behind it: `#[rux::export]`, `#[rux::resource]`, `#[rux::init]` |
| `rux-bindgen` | Reads an app's `native/` crate for its exports, for the macros and the CLI |
| `rux-layout` | `Style` → taffy (flex/grid/block) → paint items, hit + focus regions |
| `rux-text` | parley 0.11 shaping/measure/wrapping + vello 0.9 glyph drawing |
| `rux-paint` | paint items → vello scene (fills, borders, clips, text) |
| `rux-icons` | Tabler's icons as path data, for `<icon>` |
| `rux-audio` | Decoding and a mixer for `<audio>` and `loadSound` (desktop and Android) |
| `rux-video` | The platform's own video decoder, read into RGBA frames, for `<video>` |
| `rux-runtime` | `Document`: load, resolve imports, build engine, rebuild tree |
| `rux-shell` | winit window, wgpu/vello, input, focus, clipboard, file watcher |
| `rux-web` | The runtime and shell compiled to WebAssembly, rendering to a canvas |
| `rux-fmt` | The formatter behind `rux fmt` |
| `rux-highlight` | A TextMate grammar interpreter, so `.rux` highlights from the editor's own grammar |
| `rux-cli` | `rux`: new, run, check, fmt, build, vocab, doctor |
| `rux-reactive` | just `Value`, the untyped value `rux-script` and `rux-style` pass around |

---

## What works

- [Elements](/reference/elements/): The ten elements the runtime renders, plus slot, router, route and router-view.
- [Layout](/reference/layout/): Everything defaults to block; use display: flex. Hug and fill, and how boxes size.
- [Paths](/reference/paths/): SVG path data as an element: the d attribute, paint as CSS, and shapes that morph.
- [Audio](/reference/audio/): A sound as an element: drawn controls, autoplay as a browser allows it, and its events.
- [Honored CSS](/reference/css/): The authoritative list of properties the runtime interprets, plus selectors, pseudo-classes and transitions.
- [Where Rux differs from CSS](/reference/css-differences/): The short list of places Rux does not answer the way CSS does, and which kind of difference each one is.
- [Reactivity](/reference/reactivity/): Signals, computed values, effects, and what re-runs when one changes.
- [Inputs](/reference/inputs/): Text fields, textarea, select, checkbox and radio, and two-way binding with r-model.
- [Text input](/reference/text-input/): The caret, the soft keyboard, and IME composition for text that is not typed one key at a time.
- [Touch](/reference/touch/): What a finger does: taps, presses, drags and swipes, and what is not built.
- [Selection](/reference/selection/): Drag-select, double-click, and the clipboard keys.
- [Scrolling](/reference/scrolling/): Scrollers, scrollbars, and the ways a scroll can be driven.
- [Components](/reference/components/): Importing a file as a tag, props, slots, events, and what a component cannot see.
- [Routing](/reference/routing/): Routes, parameters, named routes, links, and the fact that the path is an ordinary signal.
- [Accessibility](/reference/accessibility/): The real accessibility tree, the roles elements map to, and what a screen reader is told.
- [Errors](/reference/errors/): What happens when a document will not load, and what the overlay shows.

### `<icon>`: one of Tabler's icons
A name becomes a drawing, at the size you ask for.
```rux
<icon name="search" size="1em" />
<icon name="heart" variant="filled" size="32px" />
```
Rux bundles [Tabler](https://tabler.io/icons): 5,148 outline icons and 1,054
filled, MIT licensed. Nothing is downloaded and nothing is generated by the
author. The data ships with the tool, and a build embeds only the names a
project mentions.

**`size` is an attribute rather than CSS**, and that is forced rather than
preferred. The artwork is drawn on a 24-unit grid and has to be scaled to the
size asked for, which the element can only do if it knows that size while it is
building the node, before any stylesheet is resolved. It defaults to `16px`,
and `1em` is what puts an icon on the same footing as the text beside it.
`alt` describes an icon to the accessibility tree; without it the icon is
decoration and left out.

**Paint is CSS and inherits**, so an icon takes its colour from the text around
it with nothing written per icon. `fill`, `stroke` and `stroke-width` override
that, and the stroke scales with the drawing, so one weight stays right at
every size.

**Solid and outline are different artwork, not a render mode**, and cannot be
derived from one another. Roughly four icons in five are outline only, which is
why `outline` is the default and why `variant="filled"` on a name that has none
draws nothing rather than quietly falling back.

**Paint is per path as well as per variant.** Around ninety outline icons carry
a path that is solid, or filled with no stroke: the dot on the head in
`accessible`, the slice in `percentage-25`. Those overrides survive into the
drawing, because without them the dot renders as a ring.

**`rux check` tells the three mistakes apart**, because all three draw nothing
and a gap in a row does not say which one you made:

```text
app.rux:4: error: there is no icon called `hart`
app.rux:5: error: `abacus` has no filled artwork, and variant="filled" does not fall back
app.rux:6: error: <icon> needs a `name`
```

A bound `:name` is not checked, since it is not known until the app runs. That
is also the one case a build cannot shake down, so it embeds every icon and
says why.

**Some icons morph into one another**, because paint and geometry are both CSS
here and `transition: d` already interpolates two paths that share a command
sequence:

```css
icon { transition: d 200ms ease-in-out; }
```

A bound `:name` that swaps `chevron-down` for `chevron-up` then animates rather
than jumps. **It is not general**, and should not be planned around: two icons
morph only when their artwork happens to be built the same way. The pairs known
to work are `chevron-down`/`chevron-up`, `chevron-right`/`chevron-left`,
`arrow-right`/`arrow-left` and `maximize`/`minimize`. Everything else swaps.

### `<video>`: a video, and its controls
```rux
<video src="clips/intro.mp4" poster="clips/intro.png" controls />
```
`<audio>` with a picture: the same attributes (`src`, `controls`,
`autoplay`, `loop`, `muted`, `preload`), the same events, and the same reach
from script, plus `poster`, and `playsinline`, which is accepted and is what
Rux always does. `@loadedmetadata` gives `event.videoWidth` and
`event.videoHeight`.

**Sized as a browser sizes one**: by its picture when CSS gives no size, the
other side by the picture's shape when CSS gives one, 300 by 150 until the
picture's size is known. The picture is fitted by `object-fit` and
`object-position`, **`contain` unless set**, as a browser's own stylesheet
has it, so a video in a box of another shape is letterboxed on the box's
`background`. `poster` shows until the first play or seek; without one, the
first frame does. With `controls`, the `<audio>` controls lie in a dark band
along the bottom of the picture, in white, with a fullscreen button at the
end. **While it plays the band goes** three seconds after the pointer last
moved over the picture, pressed it, or a key reached one of its buttons, and
any of those bring it back; paused, it stays.

**Fullscreen** fills the screen with the video on black, letterboxed, over
everything on the page, and the window goes borderless across the monitor;
the button again, Escape, or Android's Back leave it, before any `@key` sees
the key. On Android the box covers the activity, which is the whole screen
already, and the system bars stay.

**Windows, Android and the web.** On Windows the picture is decoded by Media
Foundation, on the GPU where there is one: H.264 always, and VP9, HEVC and
AV1 when their store extensions are installed. On Android by the phone's
`MediaCodec`, in hardware where it has it, sound included. In a browser the browser
plays it, in whatever formats it plays, through a `<video>` of its own that
is never put in the page, and each frame its picture is copied onto the
canvas on the GPU. Anywhere else, or a format the machine has no decoder
for, `@error` fires with `kind` `"unsupported"` and the poster stays.

**Its sound plays, and the picture keeps to it.** The sound is decoded by
the same Media Foundation (AAC in an MP4, Opus or Vorbis in a WebM, whatever
the machine plays) and mixed with every other sound the app plays; `muted`,
the mute button and `volume` reach it. Each picture is shown when the sound
reaches it in what the speakers play, not in what has been mixed (on the dev
machine the output held about 40 ms), so lips and words stay together. A
sound the machine cannot decode leaves the video playing silent, with a
warning on the console; a video with no sound keeps time by the clock.
Taken away (the app put in the background, another app taking the sound,
the output unplugged), the sound pauses and the picture with it.

A picture the decoder delivers too late to be on time is passed over, as
every player does, so a slow machine shows fewer frames rather than falling
behind. A 1080p60 H.264 file with its sound passes over 0 to 3 frames in
300 on the dev machine. **Scrolled out of sight**, a video plays on, its
sound too, and its pictures stop being drawn until it is back.

### `position`, and which box an out-of-flow one is measured against

All five of `static`, `relative`, `sticky`, `absolute` and `fixed` mean what CSS
says.

**`static` is the default and is the only value that is not a containing block.**
That is the rule that gives the other three their meaning: an `absolute` box is
measured against its nearest ancestor that is *not* static, so a wrapper with no
`position` of its own is passed straight over, and `position: relative` on the
box you actually mean is what claims it. Its `inset` is ignored, which is the
whole difference between `static` and `relative`.

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
slides. A `filter`, a `backdrop-filter` and a `content-visibility` other than
`visible` do the same, as in CSS. `will-change` does not.

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
section is what makes the hand-over happen: a heading rides the top until the
next section arrives and pushes it off, rather than sitting over the wrong
rows. With no scroller above it, the window is what it sticks to.

Nothing else moves while it travels: a sticky box keeps its original space the
whole time, so its siblings do not reflow. It is resolved at paint time, because
it is a question about the scroll offset and the layout does not know one, and
its hit region and metrics move with it. A sticky box paints **over** the
in-flow content around it, as every positioned box does: see `z-index` below.

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
else { card = true; dismiss = none; }          // and settle back
```
Yielding **`none` hands the swap back to the clock**, which runs the rest of
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

Driven in `examples/enter-leave.rux` and `examples/router.rux`.

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

**An effect is never woken by its own writes.** A signal it only writes
(`pick = 0`) is not among what it listens to, so a handler changing `pick`
does not wake it. A signal it reads and also writes stays: the effect is not
woken by its own write, but is by anyone else's. One that drains a list it
reads (`let batch = inbox; inbox = [];`) hears every push. Effects that feed
*each other* still cycle; that is stopped after 8 rounds and reported in the
overlay rather than hung on.

A component declares its own `computed` and `effect` lines too, one set per
instance (see [Script](/reference/script/)). Driven in `examples/computed.rux`.

**The index, and filtering rows:** `r-for="(item, i) in items"`
binds the row's index too, an `int` from 0, as a script's `for (item, i) in items`
does. An `r-if` on the same element is read once a row, with both in scope,
and leaves out the rows it is false for; the index still counts every row of
the collection. This is Vue 2's order, not Vue 3's (which reads `v-if` before
the loop, where the item does not exist yet). `r-elif`, `r-else` and `r-show`
beside `r-for` still do nothing and warn.
```html
<view r-for="(item, i) in navItems" r-if="i != navItems.length - 1"> … </view>
```

**Keyed lists:** `r-key` on the same element as `r-for` says what a row *is*,
rather than where it sits:
```html
<view r-for="item in items" r-key="item.id"> … </view>
```
The key is evaluated once per row with that row's loop variable in scope.
Duplicate keys warn (two rows claiming one identity is worse than none), and so
does an `r-key` on an element with no `r-for`.

**Without `r-key`, a row is identified by its position**, as Vue patches an
unkeyed list in place. An input in row two is not the input in row three, so
focusing one puts a caret in that row alone. Position is not
identity, though: remove the first item and the second row's text field, with
its caret and anything half typed, now belongs to what was the third item. So
an unkeyed `r-for` whose rows hold state of their own warns, naming what it
found: an `<input>`, an `r-model`, a `<video>` or `<audio>`, or a component
holding any of those. A list of plain text does not warn, since position
identifies it well enough. The positional identity is not the row's key:
reconciling and row reuse still treat the list as unkeyed.

**A key is what makes an input inside a list work at all.** An `r-model` is
stored **as written**, so every row of a list carries the same one, and an
identity taken from it alone cannot tell two rows apart. Everything that
addresses an input is now `(model, row key)`: the caret and selection, `:focus`
matching, and the value the shell reads and writes.

The value is read and written **in the row's own scope**, using the loop
variables captured where the input was built, so a model may mention the loop
variable:
```html
<input r-for="item in items" r-key="item.id"
       r-model="items[item.at.to_int()].note" />
```
Writing goes through an assignment rather than setting a scope variable, so an
`r-model` that is a path (`user.name`, `items[0].note`) writes through to the
real target, in or out of a list.

**The loop variable writes through, too.** `r-model="item.name"` inside
`r-for="item in items"` writes `items[2].name`, the real row, and so does a
checkbox's `item.done` and a slider's value. `item` is a copy of its row, so
the write is routed to the row's place. Each row carries its place in its list as a hidden
local (`_rux_at_item = "items[2]"`, see `rux_style::model_target`), a list
nested in a row is placed through that row, and a computed collection
(`items.filter(…)`) has no place, so its rows still cannot be written to.
Under test in `crates/rux-runtime/tests/list_rows.rs`.

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

If a component and its caller happen to use the same class name and isolation
is wanted, that is what `scoped` is for.

**Custom properties** defined on the document are readable as `var(--brand)`
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

**Tailwind's directives in a `<style>` block**, as Vite does it. A file's `<style>` that uses `@apply`, `@reference`, `@utility`, `@variant`,
`@custom-variant`, `@theme`, `@source`, `@plugin`, `@config` or `@tailwind` is
handed to the `tailwindcss` CLI on PATH, run in the file's folder, and the CSS
it writes back is used in the block's place, after the includes. A block with
none never reaches the CLI.
```html
<style scoped>
  @reference "../assets/app.css";
  .bar { @apply px-4 bg-green-500 hover:bg-green-700; }
</style>
```
- **Cached** in `.rux-build/tailwind/<hash>.css` under the project root,
  hashed over the block's text and the contents of the files it names by a
  relative path (`@reference`, `@import`, `@plugin`, `@config`). A file those
  import in turn, and the CLI's version, are not in the hash: editing one keeps
  the old result until the block or the named file changes.
- **Carried by `rux build`** and by `rux run --device`'s hot reload, which
  compile every block on the machine and send the cache files with the
  documents. The app finds them under the same path through its embedded
  source, so a phone or a browser never runs Tailwind.
- **A failure fails the load**, as a missing stylesheet does: Tailwind's
  message (`Cannot apply unknown utility class `bg-gren-500``) on the line
  of the class it names, and a missing CLI on the line of the first
  directive.
- **Lines inside a compiled block are lost.** A warning from its CSS names the
  file and no line, since the rules Rux reads are the CLI's layout of them.
- **The playground and a document loaded from source** compile nothing: the
  directive warns, and says to move the rule into Tailwind's input file.

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
simply has more to avoid. **In a browser they are the browser's own**, which
are zero unless the page draws under the notch with `viewport-fit=cover`; the
page `rux build --target web` writes does.

It resolves wherever `var()` does, a custom property's own value included, so
`--gutter: env(safe-area-inset-bottom)` is the way to write it once. A fallback
covers a name Rux cannot answer, **not** a known name answering zero:
`env(safe-area-inset-top, 20px)` on a desktop is 0, not 20, which is what CSS
does and is the only reading that lets a fallback mean "you are somewhere that
has no such thing". An unknown name with no fallback makes the declaration
invalid and is warned about once, the same as an undefined variable.

`titlebar-area-x`, `-y`, `-width` and `-height` are the part of the title row
the window buttons leave free, and exist only in a window with
`[window] title-bar = "overlay"` (Windows and macOS). Anywhere else they are
undefined: a fallback is used, and with none the declaration is dropped with the
usual "not a length" warning, so write one. The other `env()` name in CSS
(`viewport-segment-*`) names a surface Rux does not have, so it is unknown here;
answering for it would be inventing a number rather than reporting one.

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
Supported features: `min-`/`max-width`, `min-`/`max-height` (and the range
spelling, `(width >= 600px)`, `(400px <= width < 600px)`), `orientation`,
`resolution` (`dppx`, `x`, `dpi`, `dpcm`), `prefers-color-scheme`,
`prefers-reduced-motion`, `hover`, `any-hover`, `pointer` and `any-pointer`
(fine with hover on a desktop; coarse with none on Android, in a phone preview
and in a touch browser, so Tailwind's `hover:` utilities stay off a phone), the
`screen`/`all` types, `and` chains and comma alternatives. A block adds **no
specificity**: rules inside it cascade by ordinary source order, so a later
`@media` rule beats an earlier plain one, and `#id` still beats a `.class` in a
media block. Anything else (`not …`, `aspect-ratio`, `prefers-contrast`)
warns once and never applies.

**Cascade layers, `!important`, `@supports` and `@property`**, as in CSS. `@layer` blocks and `@layer a, b;` statements order the layers; a later
layer beats an earlier one before specificity is asked, and a rule in no layer
beats every layered one. `!important` beats every normal declaration and turns
the layer order round, so an important rule in the first layer wins. `@supports`
asks whether Rux honors the property, or with `selector()` the pseudo-class, and
takes `not`, `and` and `or`. `@property --x { initial-value: 0; inherits: false; }`
gives a custom property its starting value, and with `inherits: false` a child
starts again from it instead of taking its parent's. `@keyframes` and
`@position-try` are read (see Keyframe animation, above, and Anchor
positioning, below). Any other at-rule (`@container`, `@font-face`, `@import`,
`@page`, `@scope`, `@starting-style`) warns and does nothing. A rule nested
inside a rule is read: see Nesting, under the selectors.

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
winit exposes nothing for it, and of the browser's own media query on the web
(macOS, Linux and Android do not ask, and read "no preference"). It is re-read when the environment is next
rebuilt, which is at startup and on a resize: turning that setting off
mid-session is not noticed until then. A stylesheet that never mentions reduced
motion is stilled anyway, because honoring only the written query would leave
almost every app animating at someone who asked it not to.

`flex: 1` means `1 1 0%` (CSS's shorthand defaults), not `1 1 auto`.
`opacity` fades the node **and its subtree** as one layer.
`background`/`border` work on `<text>` nodes, not just containers.
**Units:** `px`, `%`, `rem` (=16px), `em`, the viewport units `vw`, `vh`,
`vmin`, `vmax`, `vi` and `vb` (with their `sv*`, `lv*` and `dv*` forms, which
are all one here: no browser chrome comes and goes over a Rux window), the
absolute `pt`, `pc`, `in`, `cm`, `mm` and `Q`, and the font units `ch`, `ex`,
`cap`, `ic`, `lh` and `rlh`. Rux cannot ask a font for the width of its `0` or
the height of its `x` at cascade time, so `ch` and `ex` are half an em, `cap`
0.7 em and `lh` 1.2 em, which is CSS's own answer when the font cannot be
asked.

A viewport unit in a size or an inset (`width`, `height`, their `min`/`max`,
`top`, `right`, `bottom`, `left`, `flex-basis`) is resolved by the layout
against the window every frame. Anywhere else (`padding: 2vw`,
`font-size: 4vmin`, `gap`, a `translate()`) it is put in pixels during the
cascade, and a document that does so is cascaded again when the window is
resized, as it is when a breakpoint is crossed.

**Maths:** `calc()`, `min()`, `max()`, `clamp()`, `round()` (with `nearest`,
`up`, `down` or `to-zero`), `mod()`, `rem()`, `abs()` and `sign()`, nested in
one another as deep as needed, in any property that takes a length or a
number, written out or arriving through `var()`. They are worked out in the
cascade: `padding: calc(1rem + 2px)`, `gap: calc(4px * 2)`,
`opacity: calc(var(--o) / 2)`, `font-size: clamp(1rem, 2.5vw, 2rem)` (a
percentage in `font-size` is of the inherited size, as in CSS), and
`width: clamp(100px, 20vw, 300px)`, whose viewport units are put in pixels and
cascaded again on a resize.

Maths with a **percentage** of the box it sits in, `width: calc(100% - 20px)`
or `top: calc(50% - 10px)`, is worked out by the layout once it knows that
box's size, in `width`, `height`, their `min-` and `max-`, the insets and
`flex-basis`. A parent sized by its content takes the size it comes to
without the percentage, and the child's percentage is of that, as in CSS, so
nothing shrinks round and round. Elsewhere (`padding: calc(10% + 2px)`) a
percentage in maths is not worked out yet: the declaration is ignored and
the warning says so. The layout engine, Taffy, resolves these itself through
a hook its stock tree answers with 0; Rux builds against a copy with that
hook opened up (`vendor/README.md`).

`em` is relative to the element's own resolved `font-size`, and on `font-size`
itself it is relative to the inherited one, which is what it means in CSS. It is
resolved in a pass before the properties are interpreted, the same way `var()`
is, so it works anywhere a length does.

`%` is honored only where the list above says so; it is not a unit every
box-model property takes.

`font-family` takes a CSS list (`font-family: "Inter", sans-serif`), and parley
parses it and does name-matching + fallback; the generic families (`serif`,
`sans-serif`, `monospace`, …) always resolve. Every list ends with the
system's symbol and emoji fonts (Segoe UI Emoji, Segoe UI Symbol and Cambria
Math on Windows; Apple Color Emoji and Apple Symbols, Noto Color Emoji, Noto
Sans Symbols 2 and DejaVu Sans elsewhere, whichever are installed), as a
browser falls back to them, so ✕ ▾ ▸ ⌫ draw where the chosen font has no such
glyph instead of a box. It **inherits**, like `color` and
`font-size`. **The text properties inherit, as in CSS**: `color`, `font-size`,
`font-family`, `font-weight`, `font-style`, `letter-spacing`, `word-spacing`,
`line-height`, `text-align`, `white-space` and `direction`, so `font-weight: 700`
on a card makes every text in it bold. A bare `line-height` (`1.5`) is
inherited as the number and multiplied by each text's own size; a length is
inherited as its pixels. A text field takes the weight, size and family around
it and keeps its own alignment.

A font is drawn from its **outlines**, as a browser draws it, even when it also
carries black-and-white bitmaps for small sizes (`EBLC`/`EBDT`, in Windows' Calibri
and Cambria). vello takes any font with a
bitmap strike down its emoji path and drops every glyph whose bitmap it cannot
read. Such a font is handed to vello with those two tables hidden.
Colour bitmaps (emoji) are drawn as they were.

Anything else is **parsed but not honored**, and never *silently*: the runtime
prints one line per unhonored property, once each. It says which of three things
happened, because they are not the same problem:

| You wrote | It says |
|---|---|
| `float: left` | ``CSS property `float` is real CSS that Rux does not honor yet, so it will have no effect`` |
| `paddding: 8px` | ``` `paddding` is not a CSS property Rux knows, so it will have no effect. Did you mean `padding`? ``` |
| `florble: 3` | ``` `florble` is not a CSS property Rux knows, so it will have no effect ``` |

The middle case is the one worth having: a typo is told apart from a feature
that is not built.

**An invalid value is thrown away before the cascade, as a browser throws it
away.** The declaration is dropped from its rule, or from `style` / `:style`,
so whatever an earlier rule said still applies (`grid-template-rows: minmax 1fr`
does not replace an earlier `grid-template-rows: 1fr 1fr`), and it warns:

| You wrote | It says |
|---|---|
| `grid-template-rows: minmax 1fr` | ``` `grid-template-rows: minmax 1fr` is not a value `grid-template-rows` takes, so this declaration is skipped and any earlier rule for `grid-template-rows` still applies. `minmax` is a function and takes its arguments in brackets, like `minmax(…)` ``` |
| `style="width: wide"` | ``` `width: wide` is not a value `width` takes, so this declaration is skipped … ``` |

lightningcss gives the verdict for the properties it has a parser for; the
rest (`touch-action`, `scrollbar-width`, `scroll-snap-type`, `column-count`,
`anchor-name` and the others in `crates/rux-style/src/validity.rs`) are checked
against their CSS grammar there. The grammar is CSS's, not Rux's: a valid
keyword Rux does nothing with is kept. A few valid values lightningcss cannot
parse (`inherit`, `text-shadow: none`, `anchor()`, `color-mix()`, a 3-value
`transform-origin`) are listed as valid there; they were found by running
every sheet in the test suites through it. Open-ended properties (`content`,
`font-feature-settings`, `will-change`) are not judged.

A value with `var()` or `env()` cannot be judged until it is substituted. If
it is invalid then, the property is left at its initial value, which is what
CSS does with a value invalid at computed-value time. The grid readers answer
that way too: a track list or placement is taken whole or not at all
(`minmax()` needs two arguments and a minimum that is not an `fr`), and one
Rux cannot use says so and leaves the initial value instead of an `auto`
standing in for the word it did not know.

Real CSS Rux has not built: `float`, `clear`, `order`, `writing-mode`,
`hyphens`, `hyphenate-character`, `perspective`, `perspective-origin` and
`column-span` (each warns as pending), `:has()`, `:focus-within`,
`::first-letter` and `::first-line`, `display: contents` and `table`, `contain`,
container queries (`@container`, `container-type`), `@font-face`, `@import`,
`position-area` and `anchor-center`, and the `aspect-ratio` and `prefers-contrast`
media features.

**CSS for things Rux has none of passes quietly:** the table
properties (`border-collapse`, `border-spacing`, `table-layout`,
`caption-side`, `empty-cells`), the list properties (`list-style` and its
longhands) and `::backdrop` (a modal dialog's or a fullscreen box's). They are
real, and there is nothing in Rux for them to act on, as there is nothing on a
page without a table for `border-collapse` to act on. Every reset sheet sets
them, Tailwind's among them. If Rux grows one of these things, its CSS is built
and is no longer quiet.

Colours accept `#hex` (3/6/8-digit), the full CSS named-colour list (`red`,
`rebeccapurple`, …), `transparent`, `currentColor`, and every CSS colour
function: `rgb()`, `hsl()`, `hwb()`, `lab()`, `lch()`, `oklab()`, `oklch()`,
`color()` (`srgb`, `srgb-linear`, `display-p3`, `a98-rgb`, `prophoto-rgb`,
`rec2020`, `xyz`, `xyz-d50`, `xyz-d65`), `color-mix()` in any of those spaces
with any hue interpolation, and `light-dark()`. Both the comma form
(`hsl(120, 50%, 50%)`) and the space form (`hsl(120 50% 50% / .5)`) parse, and
so does `none` for a component. The named list matters because lightningcss
*minifies* hex to keywords (`#ff0000` → `red`), so without it a plain
`color: #ff0000` would fall back to the default.

The window is sRGB, so a colour outside it (a vivid `oklch()`, a `display-p3`
red) is brought inside the way CSS Color 4 tells a browser to: its chroma is
lowered at the same lightness and hue until what is left fits, so it stays
the colour it was asked to be, only less vivid. `currentColor` is the
element's own `color` (on `color` itself, the inherited one), and a border
with no colour of its own is drawn in it, as in CSS.

`light-dark(<light>, <dark>)` takes a side by the element's `color-scheme`,
which inherits: `light dark` follows the system's setting and is resolved
again when it changes, `dark` is always dark, and with no `color-scheme` at
all it is **light**, whatever the system says. That is CSS's rule: write
`:root`'s equivalent, `screen { color-scheme: light dark; }`, to opt in.

**The default colours are CSS's.** The window
is white and text is black, as a browser's page is. The root's `color-scheme`
picks both, as CSS's `Canvas` and `CanvasText`: `:root { color-scheme: dark; }`
is white text on `#121212`, Chrome's dark canvas, and `light dark` follows the
system. A root `background` colour is what the window shows behind and around
everything, as CSS propagates it to the canvas, and Rux's own dropdown and
selection toolbar are drawn in the root's scheme. The examples are designed
dark, so they say `:root { color-scheme: dark; }`.

### `z-index`, and what is drawn over what

Paint order is CSS's (CSS 2, appendix E), and so is what a tap finds, since the
topmost box is the one drawn last. Inside a **stacking context** the order is:
the context's own box, then boxes with a **negative** `z-index`, then the
**in-flow** content, then **positioned** boxes with `z-index: auto` or `0` in
document order, then the **positive** ones from lowest up. Equal values keep
document order.

A stacking context is drawn whole: nothing inside it can be sorted above or
below something outside it, however large its `z-index`. These make one:

- the root (the `<screen>`)
- a positioned box (anything but `static`) with a `z-index` other than `auto`
- a flex or grid item with a `z-index`, whatever its `position`
- `position: fixed` and `position: sticky`
- `opacity` below 1
- `transform`
- `filter`, `backdrop-filter`, `mix-blend-mode`, `clip-path` and `mask`
- `content-visibility` other than `visible`
- `isolation: isolate`, which does nothing else
- `will-change` naming a property that makes one of the above

`z-index` on a `static` box that is not a flex or grid item does nothing, as in
a browser.

Three consequences that surprise people, all of them CSS:

- **A positioned box is drawn over all the in-flow content of its context**, not
  only over its siblings. A `position: relative` card inside an ordinary wrapper
  is drawn over in-flow content that comes after the wrapper.
- **`position: fixed` is not automatically on top.** A fixed header with no
  `z-index` is covered by a positioned box later in the document. Give it
  `z-index: 1` (or more).
- **`z-index: -1` inside a positioned box with no `z-index` goes under that box's
  own background**, because the box is not a stacking context and the layer
  belongs to the one outside it. Give the box `z-index: 0` or `isolation:
  isolate` to keep the layer inside it.

A box drawn out of tree order is still clipped by every `overflow` box it sits
in, so a positioned row inside a scroller stays inside the scroller. Escaping an
ancestor's `overflow: hidden`, the dropdown inside a scrolling panel, is what
`position: fixed` is for, placed with anchor positioning (below) so it does not
have to be measured by hand.

**Tab order and the accessibility tree do not follow paint order.** They stay in
document order, whatever is drawn over what.

`transition: z-index` steps through the whole numbers between the two values, as
CSS does. A change to or from `auto` lands at once.

### Anchor positioning: a box placed against another

CSS anchor positioning, the way a dropdown, a tooltip or a popover is placed
against what opened it. One box is named, and another is placed against it:

```css
.more { anchor-name: --row-menu; }
.menu {
  position: fixed;
  position-anchor: --row-menu;
  top: anchor(bottom);
  left: anchor(left);
  width: anchor-size(width);
}
```

- **`anchor-name: --x`** names a box. Names are **document-wide**, so a menu
  component can anchor to a button anywhere on the screen. Rules still do not
  cascade into components; only the name is shared.
- **`position-anchor: --x`** is the anchor an `anchor()` naming none measures
  to, on an `absolute` or `fixed` box. On any other box `anchor()` means
  nothing, as in CSS.
- **`anchor(side)`** in `top`, `right`, `bottom` or `left` is a position on the
  anchor: `top`, `bottom`, `left`, `right`, `center` or a percentage along it.
  `top: anchor(bottom)` puts this box's top edge on the anchor's bottom edge;
  `bottom: anchor(top)` puts this box's bottom edge on the anchor's top edge.
  It can name its own anchor, `anchor(--other bottom)`, and carry a fallback,
  `anchor(bottom, 8px)`, used when there is no such anchor on screen. `top`
  and `bottom` take the vertical sides and `left` and `right` the horizontal
  ones; the other way round is said, and ignored.
- **`anchor-size(width)`** or `anchor-size(height)` in `width` or `height`
  makes the box as wide or as tall as its anchor. Both insets on one axis
  against one anchor (`left: anchor(left); right: anchor(right)`) size the
  box too.

**It is placed at paint time, against where the anchor is drawn**, the way
`sticky` is: scrolled, stuck and all. So a menu follows its button when the
panel under it scrolls, and a tap finds the menu where it is drawn. Its size is
the layout's, found in a second layout pass once the anchor has one, so text
inside it wraps at the right width. A `transform` on the anchor is ignored, as
in a browser: what counts is where its box was laid out.

**`fixed` is what escapes the clip.** An `absolute` anchored box is still cut by
the `overflow` boxes it sits in, as CSS says; a `fixed` one is outside all of
them, which is how a dropdown gets out of a scrolling panel.

**When there is no anchor**, because nothing has the name or the box that has
it is hidden (`r-show="false"`, `display: none`, an `r-if` that is false),
each `anchor()` uses its fallback, or without one that inset is left where the
layout put it. The runtime says so once per name: ``` `--x` names nothing on
screen ```.

**When the anchor is scrolled out of view**, entirely clipped away, the
anchored box is not drawn and cannot be tapped. That is CSS's default
`position-visibility: anchors-visible`: a menu does not float on alone after
its button has left the panel.

**Two boxes with one name**: the later one in the document is the anchor, as in
CSS. A list whose every row has a menu button gives each row its own name,
and the menu the open row's:

```rux
<view class="more" r-for="item in items" r-key="item.id" :style="`anchor-name: --row-${item.id}`" @tap="open = item.id">…</view>
<view class="menu" r-if="open != none" :style="`position-anchor: --row-${open}`">…</view>
```

**When there is no room, `position-try` finds another place.** A menu under a
button near the foot of the window goes above it instead:

```css
.menu {
  position: fixed;
  position-anchor: --row-menu;
  top: anchor(bottom);
  right: anchor(right);
  margin-top: 4px;
  position-try-fallbacks: flip-block;   /* or a rule of your own: */
  /* position-try-fallbacks: --above; */
}
@position-try --above { top: auto; bottom: anchor(top); margin-top: 0; margin-bottom: 4px; }
```

- **`position-try-fallbacks`** is a comma-separated list of places to try, in
  order, when the box's own does not fit. Each is a `@position-try` name, any of
  the flips, or a name followed by flips (`--above flip-inline`).
- **`flip-block`** trades top for bottom: the insets, the margins and the
  anchor edges they measure to, so `top: anchor(bottom); margin-top: 4px`
  becomes `bottom: anchor(top); margin-bottom: 4px`. **`flip-inline`** trades
  left for right, and **`flip-start`** top for left and bottom for right.
- **`@position-try --name { … }`** replaces only the sides it names, and takes
  the insets (`top`, `right`, `bottom`, `left`), the margins and
  `position-anchor`. It may sit anywhere in the sheet, before or after the
  rule that uses it, and in an included sheet.
- **`position-try-order`** (`normal`, `most-width`, `most-height`, and their
  `most-inline-size` and `most-block-size` spellings) tries the places with
  the most room on that axis first. **`position-try`** is the shorthand: an
  order, then the list.

**Fitting** means the box's margin box stays inside the room its insets leave
it: from the anchor edge it is placed against to the far side of its
containing block (the window, for `fixed`). The box's own place wins when it
fits; otherwise the first fallback that does; with none, its own place. As in
CSS, **a fallback that fitted is tried first next frame**, so a menu that
flipped above its button stays there while the panel scrolls and it still
fits, rather than jumping back the moment there is room below again. Anchored
to another box, it starts again from its own place.

**A margin on an anchored side counts.** `top: anchor(bottom); margin-top: 4px`
leaves 4px between the button and the menu.

A fallback moves the box and does not resize it: `width`, `height` and the
other sizes in a `@position-try` rule warn and are not honored yet, and neither
are `position-area` values in the list (`position-try-fallbacks: top`), which
warn and are skipped. A name with no `@position-try` rule is skipped, and
said. Not built: `position-area`, `anchor-center`, `anchor-scope`, and
animating an `anchor()`.

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
**On Android, and in a phone's browser, a tap opens the platform's date
picker**, starting on the day the
field holds (today when empty) and offering only the days between `min` and
`max`. Choosing commits at once and fires `@change`, as a select does;
dismissing leaves the day alone. A read-only date offers no picker. Elsewhere
the field is typed into with the rule a number
follows: only a real day inside the range reaches the signal (`2026-9-3` is
written back as `2026-09-03`), and anything short of one stays in the field. A
`min` or `max` that is not a date is an error.

`<input type="file" r-model="photo" accept="image/*">` holds a **`File`**, or
`none` before anything is chosen; with `multiple` it holds a `File[]`, empty
until then. It draws as a browser draws one: a "Choose file" button, then the
chosen file's name ("No file chosen" before, "3 files" for several). **A tap,
or Enter or Space when it is focused, opens the platform's own picker**; the
window keeps painting while it is open. Choosing writes the value and fires
`@change` with it as `event.value`; closing the picker with nothing chosen
fires `@cancel` and leaves the value as it was, as a browser keeps the file it
had. `required` is not met by `none` or an empty list.

- `accept` is HTML's: extensions (`.png`), media types (`application/pdf`) and
  families (`image/*`, `video/*`, `audio/*`), comma-separated. A desktop picker
  filters by extension, so media types reach it through a table of the common
  ones. The dialog opens on that filter, named as a browser names it
  ("Images" for `image/*`), with "All files" beside it: a hint, as in HTML,
  and `File.type` says what was really picked.
- `capture` (`environment` or `user`) asks a phone for its camera or recorder
  instead of the picker. Read and kept; the desktop ignores it, as a desktop
  browser does.
- **The button is `::file-selector-button`.** Unstyled it is a bordered grey
  button in the input's colour scheme (`light-dark()` colours); a
  `::file-selector-button` rule on the input restyles it, so Tailwind's `file:`
  utilities do what they do in a browser.
- **`File`** is a built-in record every script can name without a `use`:
  `name`, `size` (bytes, an `int`), `type` (the media type, `""` when unknown),
  `url` and `path`, plus `await file.text()`, the whole file as text (UTF-8,
  with what is not UTF-8 replaced, as a browser's `File.text()` does). `url` is
  what `<image :src>` shows: a `file:` URL on the desktop. `path` is the
  desktop's alone. A script's own `type File` replaces the built-in one.

**On the web** the tap opens the browser's own picker, through a hidden
`<input type="file">` clicked inside the tap itself, since a browser opens a
picker only during a person's gesture. `accept`, `multiple` and `capture` are
handed to it as written. The `File` has no `path` (a browser never tells a
page one); its `url` is an object URL (`blob:`), drawn by `<image :src>` from
the bytes read when an image is picked, and `text()` is the browser's own.
Object URLs are kept for the page's life: a `File` can be copied anywhere,
so nothing says when the last one goes.

**On Android** the tap opens the system's document picker
(`ACTION_OPEN_DOCUMENT`), which needs no storage permission for a file the
person chose; `accept` becomes its media types, and an extension the table
does not know asks for any file rather than hiding the one wanted. The `File`
has no `path`; its `url` is the picker's `content://` URI, good for this run
of the app, as a browser's `blob:` is for the page (no lasting permission is
taken). `text()` and `<image :src>` read it through the `ContentResolver`.
With `capture`, `image/*` opens the camera and `video/*` the video camera,
writing into the app's cache through a provider of Rux's own, and `audio/*` a
sound recorder, or the picker on a phone without one, as Chrome does. A camera
is what Android makes room for by killing the app, so which input asked is
kept with the rest of the saved state, and the photo is delivered to the app
as it comes back. `capture="user"` asks for the front camera through the
extras most camera apps read; the rest open on their own choice.

Dropping a file onto the input is not built. `pickFile` and `pickFiles` give the
same `File` on every platform, through the same picker on the web and
Android (see `docs/07-script.md`).

Not built: arrow keys on a focused slider, a thumb that slides rather than jumps
between the switch's two ends, and a drawn date picker on desktop or in a
browser with a mouse.

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
| `accept` | `type="file"` | Which files the picker offers, as HTML's: extensions, media types, `image/*`-style families |
| `multiple` | `type="file"` | More than one file; the model holds a `File[]` |
| `capture` | `type="file"` | `environment` or `user`: a phone's camera or recorder instead of the picker. Ignored on the desktop |

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
keyboard. `go`, `search` and `send` name what the *app* will do: in a
[form](#forms) they submit it, and outside one the app answers with `@change`.
Left off, the key is worked out: `next` on a form's typing fields and `go` on
its last, and outside a form `done` on a phone. That is the phone's action
key. A keyboard's Enter in a form's field submits the form from any field, as
HTML's does, and Tab is what moves between fields.

Rux draws no default look for a disabled control, the same way it draws no
default look for a button. Style `:disabled`:
```css
.field:disabled { color: #6c7086; border-color: #313244; }
button:disabled { opacity: 0.5; }
```

**Events.** Four on every input, each handed `event.value`, one more on a text
field and one on a file input:

| Event | When |
|---|---|
| `@input` | After every change to the text, however it was made: a key, a paste, a cut, a phone's keyboard |
| `@change` | A changed value is committed: the field is left, or Enter is pressed in a one-line field. A select fires it on choosing a different option, a checkbox or radio on every toggle |
| `@focus` | The field gained focus |
| `@blur` | The field lost focus, after its `@change` when both fire |
| `@select` | A text field's caret or selection came to a new place, the field being entered included. `event.start`, `event.end` and `event.caret`, in characters |
| `@cancel` | `type="file"` only: the picker closed with nothing chosen. The value is kept |

```xml
<input r-model="query" enterkeyhint="search" @change="run_search(event.value)" />
<input r-model="code" inputmode="numeric" maxlength="6" autofocus />
<button :disabled="code.len() < 6" @tap="verify()">Verify</button>
```

**Marks.** `:marks` on a text field draws runs of its text styled by a class,
for a code editor's keywords or a problem underlined in place:

```xml
<input type="textarea" r-model="query" :marks="marks(query)" />
```

```css
.kw { color: #0055ff; }
.bad { text-decoration: underline wavy #e00000; }
```

It binds an array of `{ start, end, class }`, the range in characters of the
value. Each range is drawn as a `<text>` with that class inside the field would
be, so a rule like `.editor .kw` reaches it. A mark changes how the text is
drawn and never what is typed, and the caret stays where the text is: `color`,
`background` and `text-decoration` count, and anything else (a size, a weight)
is said once and left out. Where two marks overlap, the later one in the array
wins. A password and a placeholder are drawn without marks. When the marks are
worked out from the field's own value, as above, they follow every keystroke;
marks that read something else and no longer fit the text are dropped until
they are worked out again. A change to what `:marks` reads restyles the field
as a `:class` does.

**`@input` says what kind of edit it was.** Its event is an `InputEvent`:
`event.value`, and `event.kind`, one of `"type"`, `"undo"`, `"redo"`, `"paste"`,
`"cut"`, `"insert"`, `"drop"` or `"autofill"`. A text field has its own undo
and redo (Ctrl+Z, and Ctrl+Y or Ctrl+Shift+Z), and `@key` on the field or
around it sees a key first; both are described in [Script](/reference/script/#keys).

Handlers run **after** the edit or the focus change that caused them is
complete, never from inside it, so a handler that writes the field's own
signal, focuses another field or blurs this one sees the field as the person
does. A `@blur` that focuses the field it left, answered by a `@focus` that
blurs it, is stopped after 64 handlers with a warning. A handler written in a
component runs in that instance, and one in an `r-for` row sees the row.

**Autofill.** `autocomplete` says what a field holds, in HTML's field names
(`username`, `current-password`, `new-password`, `email`, `tel`, `name`,
`given-name`, `family-name`, `street-address`, `postal-code`, `country-name`,
`cc-number`, `cc-exp`, `cc-csc`, `bday`, `one-time-code`, and a few more).
Section and address-kind tokens (`section-a`, `shipping`, `billing`, `home`)
are accepted and change nothing; an unknown token warns. `autocomplete="off"`
keeps the field away from autofill. With no `autocomplete`, a password, an
`inputmode="email"` and an `inputmode="tel"` field are recognised anyway.

```xml
<view role="form" @submit="sign_in(event.values)">
  <input r-model="user" autocomplete="username" inputmode="email" />
  <input r-model="pass" type="password" autocomplete="current-password" />
  <button type="submit">Sign in</button>
</view>
```

On Android the phone's autofill service (Google Password Manager, or whatever
the person chose) sees the fields of the **focused field's form** only, so a
sign-in address is never poured into a sign-up form further down the page. It
offers its suggestions under the field or in the keyboard's strip, a choice
fills every field it has a value for, and the caret menu offers Autofill. A
fill is `@input` then `@change`, as a browser fires them. When a form's
`@submit` runs, the service may ask to save what was typed, which is where
"Save password?" comes from. In a browser, the hidden input the keyboard types
into carries the focused field's `autocomplete`, and is a `type="password"`
input while a password field has focus, so the keyboard neither suggests nor
learns it. A browser's password manager is not offered a form to save, because
there is no `<form>` for it to see submitted.

### Forms

A form is a `<view role="form">`. The role groups the fields inside it; it
draws nothing and lays nothing out. A field belongs to the nearest form around
it, so a form inside another sends only its own fields.

```xml
<view role="form" class="signup" @submit="join(event.values)" @invalid="errors = event.errors">
  <input r-model="email" name="email" inputmode="email" required />
  <input r-model="code" name="code" inputmode="numeric" minlength="4" pattern="\d+" />
  <input type="checkbox" r-model="terms" name="terms" required />
  <button type="submit">Join</button>
</view>
```

**Submitting.** Three things submit a form: a `<button type="submit">` inside
it, after the button's own `@tap` has run, a keyboard's Enter in any of its
one-line fields, and a phone's action key in its last typing field, in an app or in a
phone's browser alike. Every field is checked first, and the outcome is one of two events on the
form:

| Event | When | `event` carries |
|---|---|---|
| `@submit` | Every field passed | `values`: each field's value under its `name`, or under its `r-model` when it has no name |
| `@invalid` | A field failed | `errors`: each failing field's name mapped to a sentence saying why. `values` as above |

A failed submission is **refused**, as HTML refuses one: `@submit` does not run,
the first field that failed takes the caret (a checkbox cannot, so the keyboard
goes down instead), and every field of the form matches `:user-invalid` from
then on. A disabled field is neither checked nor sent. A readonly one is sent
and not checked, since nobody could fix it, and so is a field hidden by
`r-show`. A name that appears in every row of a list sends every row's value,
as an array under that name. A radio group sends its one value.

**The checks.** HTML's, with HTML's rule that an empty field fails only
`required`:

| Attribute | Fails when |
|---|---|
| `required` | Empty, or a checkbox or switch that is off. `:required="expr"` binds it |
| `minlength` | Shorter than this, counted like `maxlength` |
| `pattern` | The *whole* value does not match this regular expression: `\d{4}` takes `1906` and refuses `19064` |
| `min`, `max` on `type="number"` | The number is outside them. A slider's range is its travel and a date's is its picker, so neither is a check |
| `inputmode="email"` | Not an address, by HTML's own definition of one |
| `inputmode="url"` | Not an absolute URL, `scheme:` and something after it |

A `pattern` that is not a regular expression, a `minlength` that is not a whole
number, a `<button type="submit">` with no form around it and `@submit` on
anything but a form are errors, because each would otherwise do nothing in
silence.

**The pseudo-classes.** `:invalid` and `:valid` match from the start, as in CSS,
so an empty required field is `:invalid` before anyone has touched it.
`:user-invalid` and `:user-valid` wait for the person: they match once the field
was changed and left, or once its form's submission was tried. Error styling
belongs on `:user-invalid`. A disabled or readonly field matches neither pair.
`:required` and `:optional` match any input.

```css
.field:user-invalid { border-color: #f38ba8; }
.field:focus:user-invalid { outline: 2px solid #f38ba8; }
```

**The action key.** On a phone, a form's typing fields say Next and its last
says Go, unless `enterkeyhint` says otherwise. Next moves among the form's own typing fields,
passing over buttons, toggles, selects and dates, as a phone's Next does. Go
(or any key but Next, Previous and Done) submits. A textarea keeps its Enter.

**A field the keyboard covers.** When the window shrinks under a focused field,
which is what a phone's keyboard opening does to it, the scroller holding the
field is moved to bring it back into view.

### The pointer vocabulary

Beyond `@tap`, five attributes report what a finger or button is doing:
`@press`, `@release`, `@longpress`, `@swipe` and `@drag`. `@tap` is deliberately
not one of them: it is the finished gesture, it is what a keyboard activation
produces and what `tap()` from script synthesises, and none of those has a
pointer at all. So an element with only `@drag` is not keyboard-activatable,
and a tap on it goes on to its nearest ancestor with a `@tap`.

**Every box takes the pointer, handler or not**, as in CSS: a tap goes to the
topmost box under it, then runs that box's `@tap` or its nearest ancestor's,
as a click bubbles. So a tap on the text inside a button is the button's, and
a backdrop with no handler drawn over a button keeps the tap from it, as does
a box drawn over a text field. `pointer-events: none` is how a box lets taps
through.

Every handler, `@tap` included, is handed an `event`:

| Field | What it is |
|---|---|
| `x`, `y` | the pointer, relative to the element the handler is on |
| `pageX`, `pageY` | the same point, relative to the window |
| `width`, `height` | the element's own size, so `event.x / event.width` is how far across it the pointer is |
| `button` | which button: 0 primary, 1 middle, 2 secondary; a finger is 0 |
| `touches` | every finger down, each with `id`, `x`, `y` |

`@contextmenu` (a secondary click, a held touch, the Menu key or Shift+F10)
and `@doubletap` are finished events like `@tap`: each goes to the nearest
element that wrote it and takes nothing from a `@press` or `@drag` around it.
The middle and secondary buttons fire `@press` and `@release` only.

`touches` is **a list even when there is one finger**, and a mouse counts as one
finger with `id` 0.

`@swipe` adds `direction`, chosen by the dominant axis. `@drag` adds `phase`
(`start`, `move`, `end`). Both add two distances, named so neither can be read
as the other: `totalX` / `totalY` from where the press landed, and `moveX` /
`moveY` from the previous event. Following a finger wants the first; velocity
and flick detection want the second.

**A drag that ends as a flick also fires `@swipe`.** They are not rivals: a page
that follows the finger still has to be told, at the end, whether the hand meant
to throw it, which is how a reversible transition decides whether to commit.
A long press stays exclusive, because a press that moved is not resting.

**The axis claim.** A `@drag` does not take every finger that lands on it, in any
direction: that would make a draggable row inside a scrolling list a **dead
zone**, where a thumb that happened to start on the wrong element could not
scroll the page at all.

**The winner is decided by direction, once.** At the moment the finger passes
`TAP_SLOP`, the dominant axis is compared against what a scroller under the press
can actually travel on. If some scroller can move on that axis, it takes the
gesture and `@drag` never starts for that press. Otherwise the element keeps it.
The answer is never revisited: a gesture that changes owner under the hand is
visibly wrong, and no platform does it. iOS arbitrates its pan recognizers on the
initial translation, Android intercepts at the slop crossing, and the web decides
ahead of time from `touch-action`.

Nothing is taken back mid-gesture: the decision happens at the slop crossing, on
direction rather than on time.

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

**Touch text has its own gestures.** A finger dragged across text does not
select, as it would under a mouse, which is the desktop model and not what a
phone does:

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

1. **A string literal inside a double-quoted attribute** is written with
   `&quot;`, as in HTML: `r-if="city != &quot;&quot;"`, or the attribute is
   single-quoted, `@tap='name = ""'`. Entities are decoded in attributes and
   in text alike, `{{ }}` included, so `Tom &amp; Jerry` shows `Tom & Jerry`.
   `'x'` in a script is a string of one character, equal to `"x"`.
2. **A component cannot see the document's signals.** It sees its props, its
   own state, its imports and `route`, `params` and `query`. Pass a value in as
   a prop, report out with `emit`, and share state through a store (see
   Components).
3. **A component tag takes only its declared props**, listeners and `r-`
   directives. `class`, `style`, `id` and `to` on one are errors: put them on an
   element inside the component, or wrap the tag in a `<view>`.
4. **`text-align` needs a box wider than the text** (set a width, or the element
   must fill), or there's nothing to align within.
5. **A scroll container needs a bounded height** (`height`, `max-height`, or a
   `flex-grow` slot). Without one it just grows and there is nothing to scroll.
6. **Rows inside a scrolling flex column need `flex-shrink: 0`.** Otherwise the
   column squeezes them all in to fit and, again, nothing scrolls. CSS does this
   too. It is the single most common "why won't it scroll" trap.
7. **A word longer than its box overflows** unless you set `overflow-wrap:
   break-word`, since nothing can shrink below min-content. CSS does this too.

---

## Known gaps / backlog

What is not built, or not yet verified. The roadmap ([06](/roadmap/)) has
the order and the reasons.

- **Physics.** Kinetic scrolling is built; momentum beyond it, rubber-banding and
  spring easing are not.
- **Live regions.** Nothing announces a change to a screen reader. The same
  section has two related gaps: accesskit action requests are dropped, and the
  tree is flat with no landmarks.
- **Accessibility off the desktop.** Android and the web publish no accessibility
  tree.
- **Pinch and rotate.** Touch reporting carries every finger, but nothing turns
  two fingers into a pinch or a rotate.
- **`hyphens: auto`.** It needs a hyphenation dictionary per language and a
  `lang` attribute, and a soft hyphen draws no hyphen where a line breaks at it.
- **Video on macOS and Linux.** `<video>` plays on Windows, Android and the web;
  elsewhere `@error` fires with `kind` `"unsupported"`.
- **iOS.** `rux build --target ios` is named and refused.
- **Real-phone checks.** Cases in [User tests](./08-user-tests.md) marked
  "unverified ... no phone" (selection handles, action keys, autofill, the soft
  keyboard physically rising) were driven on the Android emulator or under
  browser touch emulation, not on hardware. `rux doctor` reports the toolchain,
  not the attached device.
- **Error boundaries.** A failing expression is reported and the document
  carries on; a subtree cannot catch one and show a fallback.
- **A capability model for native code.** A native module has full trust.
- **Drag and drop** of elements. `@tap` and the gesture events are the
  vocabulary; dragging selected text inside a field is built.
- **Multiple windows.**
- **CSS left out:** table properties, `writing-mode`, `column-span`, and
  `user-select` on static text outside a field.

---

## Where the design docs are still right

The [rationale](/reference/rationale/)'s laws still guide changes and the code
follows them: **layout lives in CSS, not markup** (no `<Padding>` or `<Center>`
widgets), **an element has a fixed set of events** the author binds to, **meaning
beyond the element set is a `role`**, and **reuse mature crates** (lightningcss,
taffy, parley, vello, winit, accesskit). Two details in it are out of date: the
element count (there are fourteen, not six: `<screen>`, `<view>`, `<text>`,
`<image>`, `<icon>`, `<path>`, `<button>`, `<input>`, `<audio>`, `<video>`,
`<slot>`, `<router>`, `<route>` and `<router-view>`), and the script, which is
Rux's own language and not rhai.

The [architecture](/contribute/) is current: the pipeline (parse,
cascade, reactive graph, layout, paint and present, with a file watcher), the
crate layout and the reused-crates table describe what is built.

The [spec](/reference/spec/) is the v0.1 design surface and the [guide](https://github.com/Aine-dickson/rux/blob/main/docs/03-guide.md)
a tutorial written before the runtime existed. Both are history, and parts of
each are false. The script references are [Script](/reference/script/) (the guide to
using it), [The Rux language](./11-next.md) (the language itself) and
[Types](https://github.com/Aine-dickson/rux/blob/main/docs/10-types.md) (the type checker).
