# 05 — As Built (current state of the runtime)

**This is the authoritative description of what Rux actually does today.**

Docs [01–04](./README.md) describe the *design intent* and are still worth reading
for the *why* — but the implementation has diverged from them in places. Where
they disagree, **this document wins**. Divergences are called out below. For what
is *not* built yet and in what order, see [06 — Roadmap](./06-roadmap.md).

Last updated: 2026-07-15. All milestones **M0–M9 are complete**, plus several
follow-up passes. Branch: `build/m0-window`.

---

## Running it

```bash
cargo run                          # examples/battery.rux (default)
cargo run -- examples/form.rux     # inputs + two-way binding + overflow-wrap
cargo run -- examples/list.rux     # scrolling (wheel over the list)
cargo run -- examples/gallery.rux  # images, opacity, flex-shrink, clipping
cargo run -- examples/dashboard.rux
```

Edit any `.rux` file (including imported components) and it **hot-reloads** — no
rebuild. Only changing the compiled Rust host requires `cargo run` again.

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
| `rux-shell` | winit window, wgpu/vello, input, focus, file watcher |
| `rux-cli` | `rux [file.rux]` |
| `rux-reactive` | just `Value`, the untyped value `rux-script` and `rux-style` pass around |

---

## What works

### Elements
`<screen>` `<view>` `<text>` `<image>` `<button>` `<input>` + imported
components as custom tags. `role=` is honored for **selectors and semantics**
(and matches **case-insensitively**: `role="Heading"` matches `[role="heading"]`).

`<image src="assets/logo.png">` — `src` resolves **relative to the .rux file**
(not the working directory), and `:src` binds an expression. With no CSS size it
lays out at the file's intrinsic pixel size; a `width`/`height` scales it to fit.
Formats: PNG, JPEG, GIF, WebP. A missing file logs to stderr and paints nothing.

### Layout — **use `display: flex`**
> **DIVERGENCE from docs 01–04.** The inline/block-by-role model was **built and
> then deliberately removed**. taffy has no inline text-flow, so inline elements
> hugged inside flex parents but filled inside block ones (full-width buttons) —
> confusing. It's gone.

- **Everything defaults to `display: block`.** Block containers make children fill.
- **Use `display: flex` for layout.** Flex cross-axis defaults to **flex-start**
  (children hug), not CSS's `stretch` — a deliberate divergence for ergonomics.
- **Hug means `fit-content`**: a box with no `width` is clamped to its parent's
  inner width, so it can't burst out of a narrower parent. An explicit `width` (or
  `flex-shrink: 0`) is your call and *will* overflow — clip it with `overflow: hidden`.
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
transform (translate/scale/rotate; visual only — hit regions aren't transformed)
position (relative|absolute) + top/right/bottom/left, aspect-ratio
width, height, min/max-width, min/max-height
padding, margin        (shorthand 1–4 values + -top/-right/-bottom/-left)
border, border-width, border-color, border-<side>, border-<side>-width
background / background-color / background-image, opacity
  (colour, linear-/radial-gradient, or url(…) image — cover-sized, clipped to corners)
box-shadow (single, outer; inset parsed but not drawn)
border-radius (1–4 diagonal shorthand + per-corner -top-left/-top-right/…)
color, font-size, font-weight, font-family, font-style (italic), text-align
letter-spacing, word-spacing, line-height, white-space (nowrap|pre)
text-decoration (underline / line-through)                (color: hex, rgb()/rgba(), CSS names)
overflow / overflow-x / overflow-y   (hidden|clip = clip; auto|scroll = scroll)
overflow-wrap (break-word), word-break (break-all)
cursor (pointer, on @tap boxes only)
```
**Selectors:** tag, `.class`, `#id`, `[role="…"]`, compounds, and all four
combinators — descendant (`.a .b`), child (`.a > .b`), next-sibling (`.a + .b`),
subsequent-sibling (`.a ~ .b`).
`flex: 1` means `1 1 0%` (CSS's shorthand defaults), not `1 1 auto`.
`opacity` fades the node **and its subtree** as one layer.
`background`/`border` work on `<text>` nodes, not just containers.
**Units:** `px`, `%`, `rem` (=16px), `vw`, `vh`/`dvh`.

`font-family` takes a CSS list (`font-family: "Inter", sans-serif`) — parley
parses it and does name-matching + fallback; the generic families (`serif`,
`sans-serif`, `monospace`, …) always resolve. It **inherits**, like `color` and
`font-size`. `color`/`font-size`/`font-family` are the three inheriting text
properties.

Anything else is **parsed but not honored** — but no longer *silently*: the
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
`<input r-model="sig" placeholder="…">` — tap to focus, type to edit. There is a
real **caret**: tapping puts it where you tapped, ←/→ move it, Home/End jump,
Backspace/Delete cut either side of it, and typing inserts at it. Esc unfocuses.
Every edit writes the signal, so `{{ }}` updates live. Placeholder shows when
empty. The caret survives the rebuild that follows each keystroke.

Inputs **fill their slot** (default `width: 100%`) rather than hug their text, so
a field doesn't shrink as you type, and single-line inputs **never wrap** and
**clip** overflow (no horizontal scroll yet).

`<input type="textarea" r-model="sig">` is the same, but **Enter inserts a
newline** (single-line inputs ignore it), the value wraps across lines,
**Up/Down move the caret between lines**, and it **scrolls vertically** — the
wheel scrolls it and typing keeps the caret in view.

`<input type="select" r-model="sig" :options="list">` shows the bound value and,
on tap, opens a **dropdown** of the `:options` (evaluated to strings) — a floating
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

A checked box carries a synthetic **`checked` class**, so its checked look is
ordinary CSS — there is no `:checked` pseudo-class:
```css
.box         { background: #313244; border: 2px #45475a solid; color: #cdd6f4; }
.box.checked { background: #a6e3a1; color: #ffffff; }   /* white tick on green */
```
The mark is drawn in the box's own `color`: a **stroked checkmark** for a checkbox
(a path, not a ✓ glyph — a glyph is whatever the system font ships and reads as a
letter), a dot for a radio. Keep the checked `border` a shade apart from the
checked `background`, or the ring dissolves into the fill. A radio is **round** unless you give it a `border-radius` (and a
huge radius like `9999px` is clamped to a circle, so that's how you re-round one
that inherited a radius from another class).

**Limits:** no selection (no shift-arrow, no drag-select), no clipboard, no
`type="select|textarea"`, and checkboxes/radios can't be reached by keyboard.

### Components
```rust
<script> use components::stat; </script>       // → components/stat.rux
```
```xml
<stat :label="title" :value="level" />         // props evaluated in caller scope
```
Component instances are isolated (only props are visible inside). Their CSS styles
their own subtree. Editing a component hot-reloads.

---

## Gotchas (these will bite)

1. **String literals in attributes need single-quoted attrs:**
   `@tap='name = ""'`, `r-if='city != ""'`. We do **not** decode HTML entities,
   and rhai treats `'x'` as a *char*, not a string.
2. **`use` must be alone on its own line** in `<script>`.
3. **rhai `fn`s can't touch globals** (see above) — the single biggest trap.
4. **`text-align` needs a box wider than the text** (set a width, or the element
   must fill) — otherwise there's nothing to align within.
5. **A scroll container needs a bounded height** (`height`, `max-height`, or a
   `flex-grow` slot). Without one it just grows and there is nothing to scroll.
6. **Rows inside a scrolling flex column need `flex-shrink: 0`.** Otherwise the
   column squeezes them all in to fit and, again, nothing scrolls. CSS does this
   too — it is the single most common "why won't it scroll" trap.
7. **A word longer than its box overflows** unless you set `overflow-wrap:
   break-word` — nothing can shrink below min-content. The browser does this too.

---

## Known gaps / backlog

- Input **selection** (shift-arrow, drag-select) and clipboard; `type=select`
  and `type=textarea`; keyboard reachability for checkbox/radio.
- Scrolling is **wheel-only**: no scrollbars, no drag/touch, no keyboard, no
  horizontal scrolling (the offset is vertical), no scroll-into-view.
- CSS: `box-shadow`, `position`/`top`/`left`, per-corner radius, per-side border
  *colors*.
- True inline text-flow (taffy can't; would need our own line-breaker).
- **Fine-grained reactivity** — a signal change currently rebuilds the *whole
  tree* (architecture doc's per-binding subscription model is not implemented).
- Fine-grained reactivity is the largest remaining gap between the architecture
  doc and the code.

---

## Where the design docs are still right

The [rationale](./01-rationale.md)'s core laws still hold and still guide changes:
**layout lives in CSS, not markup** (no `<Padding>`/`<Center>` widgets); **reuse
mature crates**; **keep the element set tiny**. The [architecture](./04-architecture.md)
pipeline (parse → cascade → layout → paint → present, with a file watcher) is
exactly what got built — only the *reactive graph* stage is simpler than described.
