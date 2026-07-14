# 05 — As Built (current state of the runtime)

**This is the authoritative description of what Rux actually does today.**

Docs [01–04](./README.md) describe the *design intent* and are still worth reading
for the *why* — but the implementation has diverged from them in places. Where
they disagree, **this document wins**. Divergences are called out below.

Last updated: 2026-07-14. All milestones **M0–M9 are complete**, plus several
follow-up passes. Branch: `build/m0-window`.

---

## Running it

```bash
cargo run                          # examples/battery.rux (default)
cargo run -- examples/form.rux     # inputs + two-way binding
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
| `rux-text` | parley shaping/measure + vello glyph drawing |
| `rux-paint` | paint items → vello scene (fills, borders, clips, text) |
| `rux-runtime` | `Document`: load, resolve imports, build engine, rebuild tree |
| `rux-shell` | winit window, wgpu/vello, input, focus, file watcher |
| `rux-cli` | `rux [file.rux]` |
| `rux-reactive` | **mostly dead** — only `Value` is used; its `Signals`/evaluator were superseded by `rux-script`. Worth trimming. |

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
flex-direction, justify-content, align-items, gap
flex-grow, flex-shrink, flex-basis, flex-wrap, flex (shorthand)
grid-template-columns, grid-template-rows
width, height, min/max-width, min/max-height
padding, margin        (shorthand 1–4 values + -top/-right/-bottom/-left)
border, border-width, border-color, border-<side>, border-<side>-width
background / background-color, border-radius, opacity
color, font-size, font-weight, text-align
overflow / overflow-x / overflow-y   (hidden|clip = clip; auto|scroll = scroll)
overflow-wrap (break-word), word-break (break-all)
```
`flex: 1` means `1 1 0%` (CSS's shorthand defaults), not `1 1 auto`.
`opacity` fades the node **and its subtree** as one layer.
`background`/`border` work on `<text>` nodes, not just containers.
**Units:** `px`, `%`, `rem` (=16px), `vw`, `vh`/`dvh`.

Anything else is **parsed but silently ignored** (no error).

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
`<input r-model="sig" placeholder="…">` — tap to focus, type to edit. Characters
and space append, Backspace deletes. Each keystroke writes the signal, so `{{ }}`
updates live. Placeholder shows when empty.

**Limits:** no caret, arrow keys, selection, or click-to-position (append/backspace
at the end only). **Only text inputs** — `type="checkbox|select|radio|textarea"`
from the spec is **not built**.

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

- Input caret, selection, cursor positioning; non-text input types
  (checkbox/select/radio/textarea — only `type=text` exists).
- Scrolling is **wheel-only**: no scrollbars, no drag/touch, no keyboard, no
  horizontal scrolling (the offset is vertical), no scroll-into-view.
- CSS: `box-shadow`, `position`/`top`/`left`, per-corner radius, per-side border
  *colors*.
- True inline text-flow (taffy can't; would need our own line-breaker).
- **Fine-grained reactivity** — a signal change currently rebuilds the *whole
  tree* (architecture doc's per-binding subscription model is not implemented).
- Trim the dead `rux-reactive` code (its `Value` is still used; the Signals and
  evaluator are superseded by `rux-script`).

---

## Where the design docs are still right

The [rationale](./01-rationale.md)'s core laws still hold and still guide changes:
**layout lives in CSS, not markup** (no `<Padding>`/`<Center>` widgets); **reuse
mature crates**; **keep the element set tiny**. The [architecture](./04-architecture.md)
pipeline (parse → cascade → layout → paint → present, with a file watcher) is
exactly what got built — only the *reactive graph* stage is simpler than described.
