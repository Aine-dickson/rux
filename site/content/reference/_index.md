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

## Running it

```bash
cargo run                          # examples/battery.rux (default)
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
`<screen>` `<view>` `<text>` `<image>` `<button>` `<input>` + imported
components as custom tags. `role=` is honored for **selectors and semantics**
(and matches **case-insensitively**: `role="Heading"` matches `[role="heading"]`).

`<image src="assets/logo.png">`: `src` resolves **relative to the .rux file**
(not the working directory), and `:src` binds an expression. With no CSS size it
lays out at the file's intrinsic pixel size; a `width`/`height` scales it to fit.
Formats: PNG, JPEG, GIF, WebP. A missing file logs to stderr and paints nothing.

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
position (relative|absolute) + top/right/bottom/left, aspect-ratio
width, height, min/max-width, min/max-height
padding, margin        (shorthand 1–4 values + -top/-right/-bottom/-left)
border, border-width, border-color, border-<side>, border-<side>-width
background / background-color / background-image, opacity
  (colour, linear-/radial-gradient, or url(…) image, cover-sized, clipped to corners)
box-shadow (single, outer; inset parsed but not drawn)
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

**Pseudo-classes:** `:hover`, `:focus`, `:active`, `:checked`. They stack
(`.btn:hover:active`), count as class-level specificity, and work anywhere in a
chain, `.card:hover .title` recolours the title while the pointer is over the
card. `:hover`/`:active` hold for the whole chain under the pointer, as in CSS;
`:active` is press-to-release and drops if you drag off the element; `:focus`
matches the input holding the caret. Driven in `examples/pseudo.rux`.

Any *other* pseudo-class (`:disabled`, `:nth-child(…)`, `::selection`) **never
matches**, and says so once on stderr. Before this existed the `:` was silently
dropped, so `.box:hover` parsed as `.box` and applied *unconditionally*, failing
closed is the safer half of that trade.

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
**Units:** `px`, `%`, `rem` (=16px), `vw`, `vh`/`dvh`.

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

### Reactivity & script
- `<script>` is **rhai**. `let x = signal(v)` declares state (numbers coerce to float).
- `{{ expr }}` interpolation; `r-if` / `r-elif` / `r-else`, `r-for="x in list"`, `r-show`.
- `@tap="…"` handlers.
- `host::fn()` calls into compiled Rust (registered in `rux-runtime::build_engine`).

> **DIVERGENCE / IMPORTANT:** **rhai functions cannot read or mutate global
> state.** The guide's `fn drain() { level.update(...) }` **does not work**.
> - State changes go **inline** in handlers: `@tap="level = level - 1"`.
> - Script `fn`s must be **pure** (take args, return values): `{{ hours(level) }}`.
> - Anything heavier belongs in a **`host::`** function.

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

**Not done:** no kinetic or inertial fling after the finger lifts, no
multi-touch, and no pinch zoom.

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

### Accessibility

Rux publishes a real accessibility tree through **accesskit**, so a screen reader
(Narrator/UI Automation on Windows, AT-SPI on Linux, NSAccessibility on macOS)
can enumerate and describe the UI. Roles are resolved during the build, where the
tag and `type=` are still known:

| Markup | Role |
|---|---|
| `<text>` | Label (`role="heading"` → Heading) |
| `<view @tap>` / `<button>` | Button, **named by the text inside it** |
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
[Roadmap](/roadmap/) (Further out → *Script documentation*). Expression
failures and anything from a component's CSS are still reported without a line,
for the reasons under "Checking a file without opening a window".

---

## Gotchas (these will bite)

1. **String literals in attributes need single-quoted attrs:**
   `@tap='name = ""'`, `r-if='city != ""'`. We do **not** decode HTML entities,
   and rhai treats `'x'` as a *char*, not a string.
2. **`use` must be alone on its own line** in `<script>`.
3. **rhai `fn`s can't touch globals** (see above). The single biggest trap.
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
> than the [release blog](https://ruxlang.dev/blog/), trust the blog and fix this
> file.

---

## Where the design docs are still right

The [rationale](/why/)'s core laws still hold and still guide changes:
**layout lives in CSS, not markup** (no `<Padding>`/`<Center>` widgets); **reuse
mature crates**; **keep the element set tiny**. The [architecture](/contribute/)
pipeline (parse → cascade → layout → paint → present, with a file watcher) is
exactly what got built. Only the *reactive graph* stage is simpler than described.
