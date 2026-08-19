# 05. As Built (current state of the runtime)

**This is the authoritative description of what Rux actually does today.**

Docs [01–04](./README.md) describe the *design intent* and are still worth reading
for the *why*, but the implementation has diverged from them in places. Where
they disagree, **this document wins**. Divergences are called out below. For what
is *not* built yet and in what order, see [Roadmap](./06-roadmap.md).

Last updated: 2026-08-08, for **v0.5.0**. The original M0–M9 milestones that
built the runtime are all complete; everything since has shipped in the v0.2
through v0.5 releases, which the [Roadmap](./06-roadmap.md) lists.

---

## Starting a project

```bash
cargo install ruxlang
rux new my-app
cd my-app
rux run
```

`rux new` writes a project that runs as it stands, and it is the answer to
"where do things go", which nothing else documented:

```
my-app/
  app.rux            the entry point: <template>, <style>, <script>
  components/
    task.rux         imported by `use components::task;`
  assets/            images; `src` resolves relative to the .rux file
  README.md
  .gitignore
```

Both conventions are ones the runtime already followed: `use components::task;`
names `components/task.rux`, and an `<image src="assets/logo.png">` resolves
from the document's own directory rather than from wherever `rux` was run. The
scaffold checks clean and is already formatted the way `rux fmt` writes, so the
first `rux fmt` in a new project changes nothing.

**A workspace is a directory containing `app.rux` or `index.rux`.** That is the
whole definition. There is no manifest file: a `rux.toml` would have to carry a
window title, an icon and a target, and each of those is a decision `rux build`
owns and has not made yet. If one arrives, `rux new` is where it gets written.

## Running it

Rux is on crates.io, so the shortest path needs no clone at all:

```bash
rux run                         # the workspace's app.rux or index.rux
rux run app.rux                 # or a named file
rux app.rux                     # the same, said shorter
```

`rux run` with no file looks for `app.rux`, then `index.rux`, in the current
directory and then in every parent, so it works from `components/` the way
`git` does. Only `app.rux` is generated; `index.rux` is accepted because the
web habit is strong and someone will reach for it.

**Bare `rux` prints the usage**, the way `cargo` and `git` do. A tool that
launches a GUI when invoked with no arguments is a surprise, and this one used
to do something worse: it defaulted to `examples/battery.rux`, a path that
exists only in a checkout of this repo, so the first thing typed after
`cargo install ruxlang` was a panic out of the file watcher. `rux run` with
nothing to run says what it looked for and how to make one.

From a checkout of this repo, with the examples to hand:

```bash
cargo run -- examples/battery.rux  # a bare `cargo run` now prints the usage
cargo run -- examples/form.rux     # inputs + two-way binding + overflow-wrap
cargo run -- examples/list.rux     # a scrolling list (wheel, drag the bar, Tab)
cargo run -- examples/scroll.rux   # horizontal + both-axes scrolling, scrollbars
cargo run -- examples/selection.rux # drag-select, double-click, Ctrl+A/C/X/V
cargo run -- examples/gallery.rux  # images, opacity, flex-shrink, clipping
cargo run -- examples/dashboard.rux
```

Edit any `.rux` file (including imported components) and it **hot-reloads**: no
rebuild. Only changing the compiled Rust host requires `cargo run` again.

## Formatting

```bash
rux fmt                         # every .rux under the current directory, in place
rux fmt app.rux                 # or a named file or directory
rux fmt --check .               # change nothing; exit non-zero if a file would
rux fmt --indent 4 app.rux      # one indent level: spaces, or `tab` (default 2)
rux fmt -                       # read stdin, write stdout (what an editor uses)
```

`<template>` and `<script>` are only **re-indented**: nothing on a line is
rewritten, wrapped or reordered, because a `@tap` handler is rhai and
rearranging someone's expressions is not a formatter's business. `<style>` *is*
formatted, one space before `{`, long rules broken one declaration per line,
short ones (up to three) kept inline. Line endings are preserved.

This is the one implementation. The VS Code extension used to carry its own copy
in JavaScript and the two drifted: the JS list of non-nesting tags came from
HTML, which has `img` but not Rux's `<image>`, so everything after an `<image
src="…">` without a self-closing slash was indented one level too deep. The
extension now shells out to `rux fmt`.

> **The shipped `examples/` are not formatted to this.** All 28 differ, about
> half on indent width and half on CSS, which inlines rules of up to three
> declarations that the examples write expanded. Running the formatter over them
> is a real decision (the expanded form may read better when teaching) and has
> not been made.

## Checking a file without opening a window

```bash
rux check                       # every .rux under the current directory
rux check examples              # or a named file or directory
rux check --deny-warnings .     # warnings fail too, which is what CI wants
rux check --format json .       # for an editor to turn into squiggles
```

Output is `path:line:col: severity: message`, the shape every compiler emits and
every editor and CI log already parses. Exit codes: **0** clean, **1** problems
found, **2** the request itself was wrong (no such path, unknown flag).

It loads through the same code the window does, so it cannot disagree with the
runtime about what a valid file is. **Errors** are failures to load and carry a
line and column. **Warnings** are the things the dev overlay lists.

**Every event handler is compiled when the document loads**, and one that cannot
compile is a warning. Nothing compiled a handler until it was tapped, so a
syntax error used to reach the window as a button that looked right and did
nothing at all. Handlers in branches that are not currently rendered are checked
too, since a false `r-if` is where a broken handler hides longest. It is syntax
only: a handler naming an `r-for` local or a component's own state is fine,
because those are runtime lookups rather than compile errors.

**CSS warnings carry a line**, in the file's own numbering rather than the
`<style>` block's. An unhonored property reports the line of the *declaration*,
so an expanded rule sends you to the property and not to its selector.
Selector-level warnings (an unknown pseudo-class) and unsupported `@media`
conditions report the line of the rule, which is where they are written.

There is no column. lightningcss locates a rule and gives the declarations
inside it no position of their own, so the declaration's line is recovered by
scanning the source forward from the rule, stopping at the closing brace. That
finds the line but not the offset within it.

Two kinds are still unplaced, deliberately rather than by omission:

- **Expression failures** (`{{ user.nmae }}`, a broken `@tap`). An expression
  comes from a template attribute or a `{{ }}` span and the template parser does
  not record where each one started.
- **Anything from a component's CSS.** A component's rules live in a *different*
  file, and a warning carries a line but not a file, so a line from the
  component's numbering would point confidently at the wrong part of whichever
  document imported it. A wrong line is worse than none.

Walking a directory **skips components**, the files whose template root is not
`<screen>`. A component's `{{ prop }}` values come from whoever uses it, so
loading one on its own reports every prop as undefined. Naming a component
explicitly checks it anyway, since that was asked for on purpose.

## Telling an editor what the runtime understands

```bash
rux vocab                       # elements, attributes, directives, honored CSS
```

JSON on stdout, for editors. The VS Code extension offers completions from it,
and the two lists that a crate already owns are read from that crate rather than
copied: the CSS properties are the same slice the unhonored-property warning
consults, and the void tags are the same one the formatter indents by. So the
guarantee is that **if the editor offers it, it works**, and a property honored
in a later release reaches completions without anyone remembering to update a
second list.

The extension also ships a generated copy, so completions work before
`cargo install ruxlang` has finished; `scripts/sync-vocabulary.sh --check`
regenerates it and is a release gate. That gate exists because this exact drift
already shipped: the extension's own void-tag list was inherited from HTML,
which has `img` and not Rux's `<image>`, and over-indented everything after an
`<image src="…">` for two releases.

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

### Elements
`<screen>` `<view>` `<text>` `<image>` `<path>` `<button>` `<input>` + imported
components as custom tags, plus two that render no box of their own: `<slot>`
(a component's hole for the caller's children) and `<router>`/`<route>` (see
[Routing](#routing)). `role=` is honored for **selectors and semantics**
(and matches **case-insensitively**: `role="Heading"` matches `[role="heading"]`).

`<image src="assets/logo.png">`: `src` resolves **relative to the .rux file**
(not the working directory), and `:src` binds an expression. With no CSS size it
lays out at the file's intrinsic pixel size; a `width`/`height` scales it to fit.
Formats: PNG, JPEG, GIF, WebP. A missing file logs to stderr and paints nothing.

### `<path>`: vector geometry
A leaf that draws Bézier geometry, so a Rux app is not limited to boxes and
images. The renderer was always past that ceiling; until v0.7 the language was
not.
```rux
<path class="wave" d="M 0 60 C 40 0, 80 120, 120 60" />
```
```css
.wave { stroke: #89b4fa; stroke-width: 4px; stroke-linecap: round; fill: none; }
```
**`d` is the full SVG path grammar**: `M L H V C S Q T A Z`, absolute or
relative, in the element's own coordinates. Path data pasted from a design tool
works, arcs included. `:d` binds an expression, which is what a chart drawn from
a signal uses.

**Geometry is an attribute and paint is CSS**, and the split is the design
rather than an accident. Paint belongs in the cascade because that is where
every other appearance in Rux is written, so `fill` gets `:hover`, `:class`,
`:enter-from` and `transition` for nothing. Geometry belongs in an attribute
because a data-driven path is computed per row, and the cascade is the wrong
place for a value that changes with the data.

| Property | |
|---|---|
| `fill` | the colour inside. Defaults to **opaque black**, as SVG does, so a path with geometry and no paint still draws. `fill: none` turns it off |
| `fill-rule` | `nonzero` (default) or `evenodd`, for a path that overlaps itself |
| `stroke` | the outline colour. Defaults to **none** |
| `stroke-width` | px. A width of `0` draws no outline, the way a zero-width border is no border |
| `stroke-linecap` | `butt` (default), `round`, `square` |
| `stroke-linejoin` | `miter` (default), `round`, `bevel` |

The fill is painted first and the stroke over it, which is SVG's order and the
one that looks right: a stroke is a border on the shape and belongs over its own
fill.

**With no CSS size a path lays out at the size of its own geometry**, so pasting
path data and seeing it needs no box to be written. Naming a `width` does not
rescale the drawing, it changes the box the drawing sits in; scaling is
`transform`, which every other element already uses for the same thing. There is
no `viewBox`: it is a second coordinate system to learn, and it can be added
later without breaking anything, which is not true in the other direction.

**Shapes morph.** `transition: d` animates the geometry itself, and the rule is
the one every other animatable value follows: **two paths with the same sequence
of commands interpolate, and anything else jumps**. That is why the parser
normalises everything to moves, cubics and closes, straight lines included: a
square written with four sides and a circle written with four arcs have the same
sequence, so one becomes the other. Nothing is resampled and no correspondence
is guessed, because guessing produces a fold as often as a morph, and writing
two shapes with matching commands is the discipline every morphing tool already
imposes. `fill`, `stroke` and `stroke-width` animate as the ordinary colours and
lengths they are.

`alt=` describes the drawing to the accessibility tree. **Without it a path is
treated as decoration** and left out, which is right far more often than
announcing an unnamed graphic.

Malformed path data keeps whatever parsed before the problem, which is what SVG
itself specifies and what shows an author where the typo is. An empty box shows
nothing at all.

Driven in `examples/chart.rux` and `examples/morph.rux`.

### Layout: **use `display: flex`**
> **DIVERGENCE from docs 01–04.** The inline/block-by-role model was **built and
> then deliberately removed**. taffy has no inline text-flow, so inline elements
> hugged inside flex parents but filled inside block ones (full-width buttons),
> confusing. It's gone.

- **Everything defaults to `display: block`.** Block containers make children fill.
- **Use `display: flex` for layout.** Flex cross-axis defaults to **flex-start**
  (children hug), not CSS's `stretch`, which is a deliberate divergence for ergonomics.
- **Hug means `fit-content`**: a box with no `width` is clamped to its parent's
  inner width, so it can't burst out of a narrower parent. An explicit `width` (or
  `flex-shrink: 0`) is your call and *will* overflow, so clip it with `overflow: hidden`.
- `display: grid` works (`grid-template-columns` / `-rows`: `1fr`, `px`, `auto`).
- No inline text flow: two `<text>` siblings **stack**, they don't share a line.
- **Lengths are logical pixels.** Layout and taps run in logical space and the
  scene is scaled to the display's DPI, so `16px` is the same physical size on a
  1x and a 2x screen.

### Honored CSS
```
display (block|flex|grid|inline|none)
flex-direction, justify-content, align-items, gap, row-gap, column-gap
align-self, justify-self, justify-items, align-content
flex-grow, flex-shrink, flex-basis, flex-wrap, flex (shorthand)
grid-template-columns, grid-template-rows
grid-column, grid-row (+ -start/-end)   (1 / 3, span 2, -1; no named lines)
grid-auto-flow, grid-auto-rows, grid-auto-columns
transform (translate/scale/rotate; visual only; hit regions aren't transformed)
position (static|relative|sticky|absolute|fixed) + top/right/bottom/left, aspect-ratio
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
fill, fill-rule, stroke, stroke-width, stroke-linecap, stroke-linejoin
  (<path> only; see above)
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
`font-size`, `transform`, the insets (`top`/`right`/`bottom`/`left`, which
animate together), and on a `<path>`: `fill`, `stroke`, `stroke-width` and `d`,
the geometry itself. Naming anything else warns and lists what is animatable,
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

### Reactivity & script

> **[Script](./07-script.md) is the reference for this section.** What follows is
> the tour; that is the whole surface in depth, including every way Rux's script
> differs from stock rhai and from JavaScript.

- `<script>` is **rhai**, forked as `rux-rhai`. `let x = signal(v)` declares state (numbers coerce to float).
- `{{ expr }}` interpolation; `r-if` / `r-elif` / `r-else`, `r-for="x in list"`, `r-show`.
- `@tap="…"` handlers.
- `host::fn()` calls into compiled Rust (registered in `rux-runtime::build_engine`).

> **CHANGED in v0.7: a function can now read and write the state around it.**
> This used to be the single biggest trap in the language, and it is gone.
>
> ```
> let level = signal(82);
> fn drain() { level-- }        // works
> fn is_low() { level < 20 }    // works
> ```
>
> `@tap="drain()"` is a handler like any other, and the write is tracked, so the
> screen updates. Handlers no longer have to be written inline, which is why so
> much of this document and of `/learn` still shows them that way.
>
> Two things to know:
> - **A method call does not see the surrounding scope.** `thing.helper()`
>   cannot reach `level`; `helper(thing)` can. Method dispatch passes its
>   receiver by reference and the scope cannot be borrowed at the same time.
> - **Anything heavy still belongs in a `host::` function.** Script is for
>   describing what the UI does, not for doing work.

**`query(selector)` reads the tree from a handler.** It takes a CSS selector,
the same one the stylesheet takes (tags, ids, classes, `role`, and the `>`, `+`
and `~` combinators), and returns the matching elements in document order. It is
the stylesheet's own matcher, so the two always agree about the same document.

```rux
@tap="count = query('.card').length"
```

Each result carries `tag`, `id` and `classes`. `id` is absent rather than empty
when the element has none, so `el.id ?? "none"` reads the way it does anywhere
else.

Three rules, all of which are the design rather than temporary limits:

- **It matches the tree, not the template.** A `<view r-if="open">` that is
  closed is not there to be found, because it is not on screen.
- **It works in a handler and nowhere else.** In a `{{ }}` binding, a `:style`
  or an `r-if` it raises and the overlay says why. A binding that read the tree
  would have to invalidate whenever layout changed, and invalidating it rebuilds
  and relayouts, which never settles.
- **A selector that does not parse is an error**, not an empty result, so a typo
  cannot look like "nothing matched".

**Geometry reads back as `x`, `y`, `width` and `height`**, in absolute window
pixels, from the frame currently on screen. That makes it **one frame stale**,
exactly as `getBoundingClientRect` is in a browser: a handler runs before the
next layout, so it reads what the last one produced.

```rux
fn measure() {
  let card = query(".card")[0];
  report = card.width + " x " + card.height;
}
```

Geometry is **absent rather than zero** when there is no frame to read: nothing
has been laid out yet, or the node is hidden by `r-show="false"` and so has no
box. `card.width ?? "unknown"` is how to handle it. Under `rux check` it is
always absent, since checking runs with no window and no GPU on purpose.

**Three actions, which are not tree edits:** `el.focus()`, `blur()` and
`el.scrollIntoView()`. They change host state (what holds the caret, where a
scroller sits), so the next build produces the same tree it would have anyway.
Each records an intent that is applied once the handler has finished, so a
handler that focuses something and then writes a signal does not race the
rebuild its own write causes. `blur()` is free-standing rather than a method,
because there is only one focused element. Only a text input can take focus,
since focus is keyed by `r-model`; asking anything else says so.

**`focus()` is not a tap.** It puts the caret in an input and nothing more: it
runs no `@tap` handler, follows no `to=` link and toggles no checkbox.

**`el.tap()` is the whole gesture.** It presses the element as a finger would,
at the centre of its box, through the same dispatch a real pointer goes through.
So it runs the `@tap` handler, follows a `to=` link, toggles a checkbox or
radio, opens a `type="select"` dropdown, moves keyboard focus and puts the caret
in a text input, in each case because that is what a press already does. It is
not a wrapper that calls the handler.

Two consequences of it being a real press rather than an imitation:

- **It hit-tests.** The topmost element at that point wins, so tapping something
  covered by an open dropdown taps the dropdown, exactly as a finger would.
- **An element with no box cannot be tapped.** Hidden by `r-show="false"`, or
  never laid out, means there is nothing to press, and it says so.

A tap runs a handler, and that handler can tap something else. The chain is cut
off after eight rounds, the same bound and the same reasoning as an `emit`
chain, so a button that taps itself stops rather than hanging the window.

Watch the quoting: `'x'` is a single **character** in a script, not a string, so
a selector needs `"…"`. Inside a `@tap="…"` attribute there is no room for
those, which is the practical reason to name the handler and call it.

**There is no way to mutate the tree from script, deliberately.** No setting a
property, no adding or removing children. A state change regenerates the
affected tree from the template, so any such edit would be overwritten by the
next patch, reconcile or rebuild, silently, at a moment decided by an unrelated
handler. The tree is a function of state, and state is how it changes.

`examples/element-query.rux` demonstrates all of it.

### Inputs
`<input r-model="sig" placeholder="…">`: tap to focus, type to edit. There is a
real **caret**: tapping puts it where you tapped, ←/→ move it, Home/End jump,
Backspace/Delete cut either side of it, and typing inserts at it. Esc unfocuses.
Every edit writes the signal, so `{{ }}` updates live. Placeholder shows when
empty. The caret survives the rebuild that follows each keystroke.

Inputs **fill their slot** (default `width: 100%`) rather than hug their text, so
a field doesn't shrink as you type, and single-line inputs **never wrap** and
**clip** overflow (no horizontal scroll yet).

`<input type="textarea" r-model="sig">` is the same, but **Enter inserts a
newline** (single-line inputs ignore it), the value wraps across lines,
**Up/Down move the caret between lines**, and it **scrolls vertically**: the
wheel scrolls it and typing keeps the caret in view.

`<input type="select" r-model="sig" :options="list">` shows the bound value and,
on tap, opens a **dropdown** of the `:options` (evaluated to strings), a floating
panel with a shadow, the current value picked out as a pill, and separators.
Tapping a row writes it back to the signal; any other tap closes it. The open
state lives in the shell and survives rebuilds (like scroll offsets).
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

### Text input and composition
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

### Touch
A finger takes the same path as the mouse: it taps buttons and toggles, focuses
inputs, drags a scrollbar thumb, drags out a text selection, and scrolls content
directly when it grabs something that is none of those. A drag that stays inside
the tap slop is still a tap.

Touch went a long time doing only the scrolling half, because there was no touch
hardware here to try it on. It was found within a minute of the playground being
opened on a phone, so treat "no hardware" as a reason to be suspicious of a path
rather than a reason to call it done. In a browser the canvas also needs
`touch-action: none`, or the page claims the gesture and the runtime never sees
a drag.

**Not done:** no kinetic or inertial fling after the finger lifts, and no pinch
zoom. Multi-touch is *reported* (see the pointer vocabulary below) but nothing
in the runtime interprets a second finger yet.

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

### Selection & clipboard
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

### Scrolling
`overflow: auto | scroll` makes a box scroll **on whichever axis its content
overflows**: vertical, horizontal, or both. It scrolls by:

- **wheel** (Shift+wheel, or a horizontal wheel, scrolls sideways),
- **dragging a scrollbar thumb**,
- **touch**: a finger drags the content itself,
- **keyboard**: arrows, PageUp/PageDown, Home/End scroll the box **under the
  pointer**, when no input has focus.

**Scrollbars** are an overlay on the box's trailing edge: they appear only on an
axis that actually has travel, the thumb is the box's fraction of the content (to
a grabbable floor), and when both axes scroll the tracks stop short of the corner.
They are drawn *over* the content, because a scroller clips its children so they can't
be part of the subtree, and drawn from the same geometry the drag hit-tests, so
they can't disagree.

**Scroll-into-view** runs on Tab: focusing something below the fold scrolls its
box far enough to show it (typing in a textarea does the same for the caret).

Offsets live in the shell keyed by the scroller's index in tree order, so they
survive the whole-tree rebuild, so tapping a row doesn't scroll the list to the top.
A press on a thumb never becomes a tap on the content beneath it.

**Not done:** no click-on-track paging, no kinetic/inertial touch fling, no
scrollbar hover/fade states, no `scrollbar-width`/`scrollbar-color`, no
`overscroll-behavior`, and `overflow-x`/`overflow-y` can't yet differ (one
`overflow` governs both axes).

### Components
```rust
<script> use components::stat; </script>       // → components/stat.rux
```
```xml
<stat :label="title" :value="level" />         // props evaluated in caller scope
```
Component instances are isolated (only props are visible inside). Their CSS styles
their own subtree. Editing a component hot-reloads.

**Components are a desktop feature today.** `use components::stat;` names a
*file*, and the web build has no filesystem to read it from: a document run in
a browser is handed no components, so every component tag renders nothing and
every `<route>` warns that its view is not imported. Bundling components into a
web build is `rux build`'s job. Nothing about the component model itself is
web-specific, so this is a packaging gap rather than a design one.

**Slots.** A `<slot />` in a component's template renders whatever the caller
wrote between the tags, so a component can wrap markup it has never seen:
```xml
<!-- components/panel.rux -->
<view class="panel">
  <text>{{ title }}</text>
  <slot><text>nothing here yet</text></slot>   <!-- children = the default -->
</view>
```
```xml
<panel :title="&quot;stats&quot;">
  <text class="stat">{{ count }}</text>        <!-- this file's signal, this file's CSS -->
</panel>
```
Slot content belongs to the **caller**: it reads the caller's signals (the
component cannot see the caller's own instance state), is styled by the
caller's stylesheet, and its
handlers run in the caller's scope. Only its position comes from the component.
An unfilled slot falls back to its own children, as in HTML. A `<slot>` emits no
box of its own, so a component adds no wrapper nobody wrote.

Before this, children written between the tags were **silently dropped**, which
made every component a fixed shape: no cards, panels, modals or layout wrappers.
Driven in `examples/slots.rux`.

**A component has its own state.** Its `<script>`'s top-level `let`s run **once
per instance**, so three `<counter>` elements are three counts:
```rux
<!-- components/counter.rux -->
<view @tap="count = count + step"><text>{{ count }}</text></view>
<script> let count = signal(0); </script>   <!-- private to each instance -->
```
The isolation is about **declarations, and it runs one way**. A component's
`<script>` executes in a scope of its own, so its `let`s are private: the
document cannot read them, and the same name declared on both sides is two
different variables, the component's winning inside it.

What the component's **template and handlers** see is wider. They are evaluated
against the document's scope with the instance's own names pushed on top, so a
document signal the component does not shadow is visible to `{{ }}` and can be
assigned in a `@tap`:
```rux
<!-- components/card.rux: `theme` is the document's, not this file's -->
<view @tap="theme = &quot;dark&quot;"><text>theme is {{ theme }}</text></view>
```
This is deliberate and the router depends on it: `{{ route }}` works inside a
route view, which is a component. It is also the coupling a component author
should be aware of, since a component reading a name it never declared will only
work in an app that happens to declare it. Anything a component means to be told
should come in as a **prop**, and anything it means to report should go out as an
**event**. Reaching for a document signal by name is available, not recommended.

Only `fn` definitions are shared with the document's engine, because a function
is code and state is not. A handler carries its instance from the cascade to the
shell, so the identical handler text in two instances still writes to the right
one. Props are re-derived from the caller on every build and are **not**
writable from inside: assigning to one would look like it worked and be
forgotten on the next build.

A change to instance state **rebuilds** rather than patches, since the state is
not a signal and the binding registry has nothing to look it up by. A component
is a subtree, so it is bounded, but it is coarser than a signal change. Driven
in `examples/component-state.rux`.

**An instance lives as long as it is on screen.** A component closed over by an
`r-if`, or a row that leaves an `r-for`, loses its state, and shows up new if it
comes back. Every build walks the whole template, so what a build does not reach
is what has gone. This is the same rule a route view already followed, and until
now it was the *only* place that followed it: a hidden component used to keep
its state for the life of the process and hand it back on the way in, and the
instance map only ever grew. Anything meant to outlive being hidden belongs in a
document signal.

**Events.** A component tells its caller that something happened with `emit`,
and the caller listens with `@event` on the tag:
```rux
<!-- components/stepper.rux -->
<view @tap="count = count + 1; emit(&quot;change&quot;, 1)"><text>{{ count }}</text></view>
<script> let count = signal(0); </script>
```
```xml
<stepper @change="total = total + event" />    <!-- payload arrives as `event` -->
```
The body of a listener is the **caller's** code and runs in the caller's scope,
the same rule slot content follows: a component with its own `total` cannot be
written to by mistake. `emit` with no payload leaves `event` undeclared rather
than defining it empty. An event nobody listens to is ignored, so a component
can offer more events than any one caller wants. An `emit` outside a component
has no caller and warns.

A listener is carried as text and never evaluated at build time, which is why it
is `@event` and not a prop: a prop is evaluated on every build, and a statement
that ran once per build would be the opposite of an event. A payload is read
where `emit` is written, so `emit("change", 0 - count); count = 0` reports the
count it had. A chain of components emitting at each other is stopped after 8
rounds with a warning.

Together with props this closes the loop: state can stay in the component that
owns it instead of being hoisted into the document so the document can see it
change. Driven in `examples/events.rux`.

`computed` and `effect` work inside a component, per instance and in that
instance's own scope. A computed is declared in the instance's script as a
placeholder rather than as its own expression, because creating an instance runs
that script in a scope without the document's signals: a computed reading one
would fail there, and a failed script takes the instance's whole state with it.
The real value is computed at mount, before the tree that shows it is built.

The rest is dependency bookkeeping the document already does, kept per instance:
a computed re-reads when what it read moves, an effect re-runs on the same terms
and is never woken by its own writes, and both are dropped when the instance is.
One thing is specific to instances: a computed or effect that writes only
instance state has moved nothing the change pipeline reasons about, so it forces
the rebuild itself rather than leaving the old value on screen.

`mounted` and `unmounted` are supported, and run per instance in that instance's
own scope. The build is the only place that knows an instance has appeared or
gone, and the wrong place to act on it, so it reports both and the runtime runs
the bodies after the tree is in place. An instance dropped before either hook was
reached runs neither, and when one build swaps two components the leaver's
`unmounted` runs before the arriver's `mounted`. Driven in
`examples/lifecycle.rux`.

### Routing

A `<router>` renders the one `<route>` whose path matches, and a route maps a
path to a component, so a page is a component like any other:
```xml
<router>
  <route path="/"          view="home-page" />
  <route path="/crew"      view="crew-list" :crew="crew" />
  <route path="/crew/:id"  view="crew-detail" :crew="crew" />
  <route fallback          view="lost-page" />
</router>
```
Like `<slot>`, a router leaves **no box of its own** behind: the matched view
expands in its place. Routes are tried in the order written and the first match
wins, so a `fallback` can sit anywhere among them. A path nothing matches and no
fallback catches renders nothing, and warns.

**The path is an ordinary signal called `route`.** That is the whole design:
`{{ route }}`, `r-if="route == \"/about\""` and `:class` already understand
navigation, and a route change reconciles the router's subtree rather than
rebuilding the document.

**Parameters.** A `:name` segment matches anything and is handed to the view as
a prop, so `/crew/grace` reaches `crew-detail` with `id` set to `"grace"`. A
match must account for the whole path, not just its front, or `/` would match
everything. A trailing slash is not a difference.

**Nested routes.** A `<route>` may contain `<route>` children, and the parent's
view places a `<router-view />` where they render:
```xml
<router>
  <route path="/" view="home-page" />
  <route path="/crew" view="crew-list">
    <route path=""            view="crew-empty" />
    <route name="crew-detail" path=":id" view="crew-detail" />
  </route>
  <route fallback view="lost-page" />
</router>
```
A child path is **relative** unless it begins with `/`, so a section can be moved
by editing one line. `path=""` is the index route: it fills the outlet at the
parent's own path, and without one `/crew` renders the list with an empty outlet
rather than an error. `<router-view />` leaves no box of its own, like `<slot>`.

The parent **stays mounted** while the child changes under it, so a list keeps
its state and its scroll position as you move between the things it lists.

Parameters are **merged down the chain**: a child view sees what its parent
captured, and the `params` signal outside the router sees what a child captured.
A name resolves to its **full** path, built from its ancestors, so
`path_for("crew-detail", #{ id: "grace" })` returns `/crew/grace` from a name
written on the child.

A path that matches a parent but nothing under it is not a half match: the whole
branch fails and the next sibling is tried, ending at the fallback. That is why
`/crew/grace/extra` lands on `lost-page` rather than on the crew list.

Two mistakes are reported rather than rendered as silence: a route with children
whose view never places a `<router-view />`, and a `<router-view />` in something
that is not a route's view.

**Links.** `to="/path"` makes an element tap to that path, announce as a link
rather than a button, and match `:current` when it names the path you are on,
which is how a nav bar shows where you are:
```css
.tab:current { background: #89b4fa; color: #11111b; }
```
`:to="…"` is the computed form, for a list whose every row links somewhere
different (`:to="&quot;/crew/&quot; + member.id"`). An explicit `@tap` wins over
both, so a link can still do something else on the way.

**Parameters are also readable from outside the matched view**, as `params`:
```xml
<text r-if="params.id != ()">viewing: {{ params.id }}</text>
```
The view gets them as props, which is enough for the view. It is not enough for
a title bar or a breadcrumb, which sit in the document's own layout and are not
the matched view. `params` empties when a route captures nothing, rather than
keeping the last page's answer.

**History.** `navigate("/path")`, `replace("/path")`, `back()` and `forward()`
are callable from any handler. History is one list with a cursor, so going back
and then somewhere new drops what was ahead. Navigating to where you already are
is not a visit, or tapping the current tab would fill the history with repeats.
On the desktop, **Alt+Left / Alt+Right** and the mouse's side buttons walk it.

`replace` goes somewhere *instead of* where you are, overwriting the current
entry, and it is what a redirect needs rather than a nicety. Redirect with
`navigate` and the redirecting page stays in the history, so Back lands on it
and is redirected forward again: the Back button appears broken and nothing in
userland can fix it.

**`can_go_back` and `can_go_forward`** are signals, so a history button can grey
itself out:
```xml
<view class="step" :class="#{ dead: !can_go_back }" @tap="back()">
```
Signals rather than functions because what they are for is disabling a control,
and disabling a control is a class, and a class reads signals.

**Query strings** are read through a `query` map, and are not part of the path:
```xml
<text>looking for {{ query.q }}</text>   <!-- /search?q=dark+mode -->
```
`route` stays `/search`, so every `route == "/search"` already written keeps
meaning what it says. A query is an argument to a page rather than a different
page, so it takes no part in matching either. The history stores the whole
address, so going back to a search restores what was being searched for. `+` is
a space and `%xx` is decoded; a key with no `=` is present and empty; a repeated
key keeps the first.

**Named routes.** A path is written into every link that leads to it, so a URL
scheme that can never be changed afterwards is not much of a scheme. Name a
route and build its path with `path_for`:
```xml
<route name="crew-detail" path="/crew/:id" view="crew-detail" />
...
<view :to="path_for(&quot;crew-detail&quot;, #{ id: member.id })">
```
It returns a **string**, so it composes with `to`, `:to`, `navigate` and
`replace` rather than needing a second form of each. Values matching a `:name`
segment fill it; whatever is left over becomes a query string, which is what
makes `path_for("search", #{ q: "rust" })` work for a route with no parameters
at all. Values are escaped on the way in and unescaped on the way out, so an id
containing a `/` survives the round trip. A missing parameter or an unknown name
warns, and produces a path that visibly does not work: landing on the fallback
page is a bug you can see, and landing on the wrong record is not.

`route`, `params`, `query`, `can_go_back` and `can_go_forward` are all provided,
and all reserved: a script declaring one is warned rather than quietly
overwritten.

**A route's view starts fresh when you return to it.** Instance state is keyed by
template position, so *keeping* it across a visit is what would happen by
accident; anything meant to outlive a visit belongs in a document signal. Driven
in `examples/router.rux`.

**An app can open on a page other than its first one**, which is what a link
someone shared arrives as. On the desktop that is a flag:
```text
rux run app.rux --route /crew/grace
```
The arrival page is the *first* page, not the second: there is no `/` behind it,
because no one visited one, so Back has nowhere to go. Saving the file while a
page other than `/` is showing now reloads onto that page instead of jumping
home, so an edit to a page three taps in can actually be seen.

**On the web the URL bar is the app's address bar**, if the page hands it over:
```js
start(canvas, source, "/");     // served at the root of a domain
start(canvas, source, "/app/"); // served from a subdirectory
start(canvas, source);          // leave the URL alone
```
The base is subtracted from the URL, so an app is written the same way wherever
it is deployed: the route is `/crew`, the URL is `/app/crew`. With a base given,
opening a URL opens that route, navigating adds a history entry, and the
browser's own Back and Forward walk the app, including a long-press that jumps
several entries at once. Each entry carries its position in the history, which
is what makes a multi-entry jump one move rather than a guess about direction.

Passing no base leaves the URL untouched, and that is the default on purpose:
the playground runs documents written by whoever is typing into them, and one of
them containing a `<router>` must not be able to rewrite the address of the page
hosting it.

> **A `<router>` cannot render a route view on the web yet.** A route's view is
> a component, a component is loaded from a file, and a browser has no
> filesystem: the web entry point is handed no components at all, so every
> `<route>` warns that its view is not imported and the router renders nothing.
> The URL half above is built and works, and `route` is an ordinary signal, so
> `r-if="route == &quot;/about&quot;"` does work on the web today. What is
> missing is the bundling of components into a web build, which is what
> `rux build` is for. Until then, treat the router as desktop-only.

**Scroll restoration** is on, and `<router restore-scroll="false">` turns it off.
The flag means **remember**, not *always restore*: a page you open starts at the
top, and a page you go **back** to comes back where you left it. Which of the
two you get is decided by how you arrived rather than by a preference, which is
what every platform does. A flag meaning "always restore" would drop you into
the middle of a page you had just opened for the first time, which reads as a
bug. Turned off, every arrival is the top. A redirect through `replace` is an
arrival, not a return, so it lands at the top too.

Offsets are stored on the **history entry**, not on the route. A scroll region
is identified by its position among the scrolling boxes in tree order, so those
ids only line up when the tree has the same shape, and an entry is always one
route: by the time the offsets are read back, the shape is the one they were
recorded against.

**Route guards.** `guard="expr"` on a `<router>` runs on every navigation; on a
`<route>` it runs whenever that route is part of what matched, so a guard on a
section covers every page inside it without being written on each one. Outermost
first: a section's guard is the coarser question, and answering it second would
mean running the finer one for a place you were never going to reach.

The answers are vue-router's: **`false` cancels**, **a string redirects to that
path**, and **anything else allows**. Anything else includes `()`, which is what
a guard body with no explicit answer evaluates to, so the usual shape is object
or say nothing:

```rux
<route path="/sent" view="outbox" guard="gate()" />
```
```rux
fn gate() {
  if !signed_in {
    return "/login";
  }
}
```

`to` and `from` are in scope, along with whatever parameters the guard's own
level captured, so a guard on `/crew/:id` can read `id` and decide about that
member rather than only about the section.

**A guard runs before the history moves**, which is the whole reason it is here
rather than in a page: a refused navigation leaves no entry behind and opens no
route transition, and by the time a page could refuse to render itself both have
already happened. It follows that **Back, Forward and a deep link go through
guards too**. A guard written on `navigate` alone would protect nothing, since
Back reaches the same page without passing it, and Back is how anyone leaves a
login screen.

A guard that redirects to the path it was asked about has allowed it. A circle of
redirects is cut off after eight and reported, the same bound and the same reason
as an `emit` chain.

**Guards are synchronous.** There are no promises in the script language, so a
guard cannot await a network answer; it decides from state that is already there.
Fetch first, then navigate.

A guard is compiled at load, on the same terms as a `@tap` handler, so a syntax
error in one is reported without anyone having to navigate. It hides longer than
a handler otherwise would: nobody taps a guard, so a broken one is found by
whoever navigates, and what they see is a link that does nothing.

### Accessibility

Rux publishes a real accessibility tree through **accesskit**, so a screen reader
(Narrator/UI Automation on Windows, AT-SPI on Linux, NSAccessibility on macOS)
can enumerate and describe the UI. Roles are resolved during the build, where the
tag and `type=` are still known:

| Markup | Role |
|---|---|
| `<text>` | Label (`role="heading"` → Heading) |
| `<view @tap>` / `<button>` | Button, **named by the text inside it** |
| `to="/path"` on anything | Link, so navigating is announced as going somewhere |
| `<input>` | TextInput · `type="textarea"` → MultilineTextInput · `type="select"` → ComboBox |
| `<input type="checkbox">` / `="radio"` | CheckBox / RadioButton, with live **checked** state |
| `<image alt="…">` | Image |
| a scrolling box | ScrollView |
| `role="…"` on anything | that role, overriding the implicit one |

**The accessible name** comes from, in order: an authored `label="…"` (or `alt=`
on an image), then a `<text for="…">` pointing at the element's `id`, then, for
inputs only, the `placeholder` as a last resort. A hint never outranks a real
label. Controls also expose their **value**, and the platform's focus follows the
focused input.

```xml
<text for="email">Email address</text>
<input id="email" r-model="email" placeholder="you@example.com" />
<!-- announces as: "Email address, edit" -->
```

Plain layout boxes are **not** exposed, a tree full of anonymous groups is worse
than a short one. `r-show="false"` elements are absent from the tree, not merely
invisible. The whole tree is rebuilt per frame but only published while assistive
technology is actually attached, so it costs nothing otherwise.

**Not done:** accesskit *action requests* (a screen reader asking to click or
focus an element) are received but not yet dispatched into the app; there is no
nesting/landmark structure (the tree is flat under the window); and live-region
announcements are unimplemented.

### Errors & the dev overlay

Mistakes are shown **in the window**, not only on a stderr nobody running a GUI
app is watching.

- **A file that won't load** opens the window with a red panel naming the file and
  the failure. Parse errors carry a **line and column**, numbered against the whole
  `.rux` file (not the `<template>` section), so they line up with the editor
  gutter: `parse error at line 6, column 16: mismatched closing tag: expected
  </view>, found </vieww>`.
- **A hot-reload that fails keeps the last good UI on screen** and says so
  (*"showing the last version that loaded"*), so a typo mid-edit neither blanks
  the window nor passes unnoticed. Fixing the file clears the overlay; the window
  keeps its size and pointer state across the reload.
- **A document that builds but has dead CSS** gets a quieter amber panel listing
  what does nothing: unhonored properties, unknown pseudo-classes, undefined
  `var()`s, unsupported `@media` conditions, and **expressions that failed**, `expression \`dubble(n)\` failed: Function not found: dubble`. Long lists are
  capped at six with a count of the rest; everything still goes to stderr.
  CSS warnings are prefixed with the line they are on (`line 11: …`).
- **Tapping the panel dismisses it**, and it says so. The panel covers the app it
  is describing, which was a problem when the thing you needed to look at was
  underneath. The dismissal is remembered against *those* diagnostics, so it
  lasts exactly as long as the document's problems are the same ones: fix a
  warning, or introduce an error, and the panel comes straight back. A press
  landing on the panel does not reach the app under it either.

Every shipped example is checked to load **warning-free**, so a noisy overlay in
`examples/` is a test failure.

**The browser playground shows the same diagnostics**, in the page rather than
only on the canvas. `rux-web`'s `diagnose(source)` sets the document and returns
the error (with line and column) and every warning (with its line) as JSON; the
page lists them under the editor and each one that knows its line is a button
that selects that line. It runs on load as well as on Run, since a shared link
carries its source in the URL hash.

> The deployed page is built from `main` while the runtime it loads is pinned to
> the latest **tag**, so the page has to keep working against a build that
> predates its own features. It feature-detects `diagnose` and falls back to the
> older `setSource`, which reports an error and no warnings. The fallback is
> only removable once a deployed build actually carries `diagnose`, which means
> after the tag exists *and* the site has been rebuilt against it: pushing a tag
> deploys nothing on its own, since the workflow fires on pushes to `main`.

**Known limits:** rhai returns `()` for a missing *map property*, rather than
erroring, so `{{ user.nmae }}` still renders empty with nothing reported (a
missing *function* or variable does report). That one is rhai's semantics, not
ours, it is tracked as a motivator for the planned rhai fork in
[Roadmap](./06-roadmap.md) (Further out → *Script documentation*). Expression
failures and anything from a component's CSS are still reported without a line,
for the reasons under "Checking a file without opening a window".

---

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

The [rationale](./01-rationale.md)'s core laws still hold and still guide changes:
**layout lives in CSS, not markup** (no `<Padding>`/`<Center>` widgets); **reuse
mature crates**; **keep the element set tiny**. The [architecture](./04-architecture.md)
pipeline (parse → cascade → layout → paint → present, with a file watcher) is
exactly what got built. Only the *reactive graph* stage is simpler than described.
