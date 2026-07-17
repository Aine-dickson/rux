# 06 — Roadmap

Where Rux goes next. Written 2026-07-15, on branch `build/m0-window` (7 commits
ahead of the last docs commit, **not pushed**).

For *what works today*, read [05 — As Built](./05-as-built.md). This document is
only about what is **not done yet**, and in what order.

---

## Where we are

The runtime works end to end and has now been **driven, not just tested**:
windows open, text lays out, inputs edit, lists scroll, images draw, files
hot-reload. 56 tests pass.

That last sentence matters more than the number. **Every real bug found in the
last few sessions was invisible to the test suite and obvious within seconds of
using the app:**

| Bug | Test suite said | The window said |
|---|---|---|
| Text re-wrapped and spilled over its siblings | green | last word of a line breaks and collides |
| A hugging box burst out through its parent | green | green boxes hanging out of the card |
| The caret stayed in the input you left | green | two carets on screen |
| The checkbox tick was a font glyph | green | reads as a letter, not a control mark |
| vello 0.9 renders Rgba8 into a Bgra8 surface | **compiled** | panic on launch |

**So the rule for v0.1 is: a feature is not done until it has been driven in the
window.** Tests protect against regression; they do not tell you the thing works.

---

## v0.1 — the shake-down (next up)

The goal is not new features. It is to make v0.1 mean something.

### 1. Make the examples worth testing
Nearly every example is fixed-width (`320px` cards, fields, lists), so **resizing
the window proves almost nothing**. Only `dashboard.rux` (a `1fr 1fr 1fr` grid)
actually re-flows.

- `list.rux` → responsive (`width: 100%; max-width: 520px`).
- `gallery.rux` → a `flex-wrap` grid of thumbnails, so content must re-flow.
- Keep one fixed-width example on purpose (`battery.rux`) as the control.

### 2. Drive every example against this checklist
Type and click around both fields · scroll the list, tap a row, scroll again ·
toggle the checkbox and both radios · resize wide · resize **very narrow** ·
minimize and restore · hot-reload each file with the window open · drag between
monitors of different DPI, if available.

### 3. Watch specifically for
- **Text escaping its box at narrow widths** — the wrap invariant (a text box is
  never narrower than the text measured) breaking under a new width.
- **Scroll offsets stranding content after a resize** — they are re-clamped per
  layout, but that path is untested against a *changing* viewport.
- **A panic on minimize** — the surface goes to zero; wgpu now reports occluded /
  timed-out frames as a status we skip, but that is unverified.
- **`ScaleFactorChanged`** — we handle `Resized` but not this. Layout reads the
  scale factor every frame so it *should* be fine. Unverified.
- **Any ephemeral UI state that does not survive a rebuild** (see below).

### Shake-down progress (2026-07-15)
- ✅ `list.rux`, `gallery.rux` made responsive; `gallery` is now a `flex-wrap`
  grid. `dashboard.rux` cleaned up into a dark-themed `1fr 1fr 1fr` grid demo.
- ✅ Driven on screen: `gallery`, `list`, `form`, `dashboard`. Three
  test-invisible bugs found and fixed (see the CSS section below for the two
  layout ones; the `r-for` `@tap` one is there too).
- ✅ **Minimize/restore: verified clean** — no panic when the surface goes to
  zero, editing/caret resume correctly on restore.
- ✅ Blinking caret added (user request): 530ms, solid while typing.
- ✅ `minmax(0, 1fr)` grid tracks added (user request, after the dashboard's
  `1fr` columns overflowed a narrow window — expected CSS, but ungraceful):
  `Track::MinMax` → taffy `minmax()`, a paren-aware `parse_tracks`. Lets tracks
  shrink below content instead of overflowing. `dashboard.rux` now uses it.
- ⏳ **`ScaleFactorChanged` / cross-DPI drag: still unverified** — needs a
  second monitor (deferred to the week of 2026-07-20).
- ⏳ `battery.rux` (the fixed-width control) not re-driven yet.

### 4. Then tag `v0.1`
Only once the above is clean — specifically, **do not tag until the cross-DPI
drag is verified**, since `ScaleFactorChanged` is the last untested surface path.

---

## v0.2 — inputs, polish, and CSS

**All four items are done (2026-07-17).** What's left under each is listed there
as *Not done* — long-tail CSS (variables, `@media`, pseudo-classes) is the
biggest of it, and fine-grained reactivity (below) is still the largest gap
between this code and [04 — Architecture](./04-architecture.md).

### 1. Text selection + clipboard — **done (2026-07-17)**

A focused input now has a **selection, not just a caret**. `rux_runtime::Focus`
carries `model` + `caret` + `anchor`; the range between them is the selection, and
`apply_focus` re-applies both after every rebuild — one restore pass, not two.

- ✅ **Drag-select** (press anchors, drag extends), **double-click** selects a
  word (`DOUBLE_CLICK` window + `TAP_SLOP`).
- ✅ **Shift+movement** extends from the anchor; a movement without Shift
  collapses. Typing/pasting/Backspace/Delete replace the selection.
- ✅ **Ctrl+A / C / X / V** against the real system clipboard (`arboard`, with
  `image-data` off). A multi-line paste into a single-line input keeps the first
  line only. No clipboard → a warning at startup and copy/paste no-ops, rather
  than a crash.
- ✅ **The highlight** is painted behind the glyphs in the focus-ring blue (no
  `::selection` yet, so it isn't author-controlled).

**The trap worth remembering:** parley's `Selection::geometry` returns rects laid
out on *parley's* line pitch, but we draw lines with the leading trimmed
(`ascent + descent`, or `line-height`). Taking its rects wholesale would drift the
highlight further off the glyphs with every wrapped line. `selection_rects` takes
only the **horizontal** extent from parley and recomputes `y` from our own
stepping, keyed by the line index parley hands back. Guarded by
`rects_line_up_with_our_own_line_stepping` and `rects_follow_line_height`.

Also: `press_text` runs before tap dispatch (a selection drag has to start on
press), but declines while a dropdown is open — otherwise an option floating over
a textarea would focus the textarea instead of picking the option.

**Not done:** word-wise movement (Ctrl+arrows), triple-click line-select,
drag-and-drop of selected text, `::selection` styling, middle-click paste on X11.

### 2. The last two input types — ✅ mostly done (2026-07-16)
- ✅ **`type="textarea"`** — a multi-line text input. It's the ordinary text
  input plus a `multiline` flag on the node → `FocusRegion`; the shell inserts a
  newline on Enter (single-line inputs still ignore it), and the value wraps.
- ✅ **`type="select"`** — evaluates `:options` to strings at build time
  (`Node.options`), exposed as a `SelectRegion`. The shell owns the open state
  (`open_select`, survives rebuilds), draws the dropdown as an overlay appended
  on top of the scene, hit-tests the rows itself (`dropdown_row`), writes the
  chosen value back to the model, and closes on any other tap. Guarded by a
  `rux-style` test; driven in `examples/form-controls.rux`.
- ✅ **checkbox/radio keyboard-reachability (2026-07-17)** — the layout now emits
  `Layout.focusables` (a document-ordered `FocusItem` list: text inputs, buttons,
  toggles, selects). The shell keeps a `focus_index`; **Tab**/**Shift+Tab** move a
  focus ring through them (tapping syncs it too), a focused text input edits, and
  a focused button/checkbox/radio activates on **Space/Enter** (select opens).
- ✅ **Input polish (2026-07-17, from testing):** inputs default to `width:100%`
  (they were hugging their text and shrinking as you typed); single-line inputs
  are `nowrap` + clip; textarea gets Up/Down caret movement; the dropdown is
  restyled as one floating panel (shadow, selected pill, separators).
- ⏳ **Still open:** select has no keyboard list navigation or native mobile
  picker; a select's `cursor: pointer` doesn't apply (selects aren't `@tap` hit
  regions); text inputs don't scroll horizontally (long single lines clip).

### 3. Scrolling polish — **done (2026-07-17)**

Scrolling was wheel-only and vertical-only. Now:

- ✅ **Horizontal scrolling.** The offset is a two-axis `rux_layout::Offset`, and a
  scroller reports `content_width`/`content_height` and a `max` on each axis, so a
  box scrolls whichever way its content actually overflows. Shift+wheel (and a
  horizontal wheel) scroll sideways.
- ✅ **Scrollbars.** An overlay on the box's trailing edge, drawn *over* the
  content (a scroller clips its children, so a bar inside the subtree would be
  clipped away). A bar only exists on an axis with travel; the thumb is the box's
  fraction of the content, floored so it stays grabbable; with both axes live the
  tracks stop short of the corner. Paint and hit-testing share `bar_track` /
  `bar_thumb`, so what you see is what you can grab.
- ✅ **Drag.** A press on a thumb starts a drag and never becomes a tap on the
  content beneath it; pointer travel down the *track* maps to the content's travel
  through its full range.
- ✅ **Touch.** A finger drags the content itself. **Unverified — no touch
  hardware here**; it is the one part of this item nobody has driven.
- ✅ **Keyboard.** Arrows, PageUp/PageDown, Home/End scroll the box under the
  pointer, reached only after a focused input has declined the key, so it can't
  steal a caret key.
- ✅ **Scroll-into-view.** Tab to something below the fold and its box scrolls to
  show it (`scroll_focus_into_view`, beside the other restore passes).

**Found by driving it, invisible to the tests:** the horizontal thumb was painted
with the *track's length* as its thickness — a pale slab across the whole box —
because `bar_thumb`'s X arm took the wrong component out of the track tuple. Every
test only looked at the vertical bar. The lesson is the standing one: the axis you
didn't test is the axis that's broken.

**Not done:** track-click paging, kinetic touch fling, scrollbar hover/fade,
`scrollbar-width`/`-color`, `overscroll-behavior`, and `overflow-x`/`overflow-y`
differing from each other (one `overflow` still governs both axes). Single-line
text inputs still clip rather than scroll horizontally.

### 4. CSS: close the gap

The honored set is listed in [05 — As Built](./05-as-built.md). Everything else is
**parsed and silently ignored** — which is the worst failure mode we have: you
write valid CSS, nothing happens, and nothing tells you why. This is the item most
likely to make Rux feel like a toy, so it gets real scope.

**Already fixed during the v0.1 shake-down** (kept here as a landmine map):

- **`@tap` inside `r-for` couldn't see the loop variable.** `@tap="picked = item"`
  silently did nothing: handlers run later in *global* scope (`run_handler` →
  `eval(src, &[])`), where the `r-for` `item` no longer exists, so the assignment
  failed and the bound `r-if` never fired. `form.rux` worked only because its
  handlers reference no loop var — nothing tested this path. Fixed by baking the
  active loop bindings into the handler as a `let` prelude at build time
  (`bind_locals` in `rux-style`, `Value::to_rhai_literal` in `rux-reactive`), so
  the handler is self-contained; the `let`s are dropped by `eval`'s existing
  `rewind`, so nothing leaks. Guarded by an end-to-end test in `rux-style`.
- **`flex-wrap` + percentage width + `max-width` mis-measured its own height.**
  A wrapping container written `width: 100%; max-width: 520px` reserved height
  for *one* row while painting *two*, so the following sibling rode up over the
  wrapped last item. Root cause is a **taffy bug still present in 0.12**: it
  measures wrap content at the full percentage width (ignoring the cap), sizes
  the cross-axis for one row, then clamps the width and wraps without revisiting
  the height. Fixed in `rux-layout` `to_taffy`: for that exact combination the
  width maps to taffy `auto` (fit-content, still capped by the same `max-width`,
  so it fills up to the cap for any overflowing content — i.e. the wrap case).
  Guarded by `crates/rux-layout/tests/wrap.rs`. A version bump does **not** fix
  it, so don't reach for one.

**First, two things that are bugs, not gaps** — both now **fixed** (2026-07-15):

- ✅ **`>`, `+`, `~` were treated as descendant combinators.** `parse_selector`
  skipped the token, so `.card > text` matched *any* descendant `text` — the
  wrong elements, silently. Fixed: `parse_selector` now records a `Combinator`
  between each pair of compounds (a bare space is descendant), and a recursive
  `matches_chain` honors all four — descendant, child (`>`), next-sibling (`+`)
  and subsequent-sibling (`~`). Sibling combinators needed preceding-sibling
  context, so the ancestor chain is now `AncNode { desc, prev }` (each ancestor
  carries its own preceding siblings), which resolves even `.a ~ .b .c`. Guarded
  by unit tests that assert the *negative* case (the combinator must NOT match
  where descendant would) plus an end-to-end test and a lightningcss
  serialization round-trip. Known limitation: sibling combinators don't see the
  synthetic `checked` class on a preceding sibling.
- ✅ **`cursor` was ignored.** Now honored: `Style.cursor` (`rux-layout`), mapped
  from `cursor: pointer` in `interpret`, carried on `HitRegion`, and applied by
  the shell's `update_cursor` on `CursorMoved` (topmost region under the pointer
  wins; the window is only touched when the shape changes). Because it rides on
  the hit regions we already compute, `cursor` is honored **only on tappable
  (`@tap`) boxes** — a `cursor` on a plain box still does nothing. Widen to a
  dedicated cursor-region pass if that bites.

**Cheap — the engine already supports it, we just don't map it:**

| Property | Backed by | Status |
|---|---|---|
| `align-self`, `justify-self`, `align-content`, `justify-items` | taffy | ✅ done 2026-07-16 |
| `row-gap` / `column-gap` | taffy | ✅ done 2026-07-16 |
| `aspect-ratio` | taffy | ✅ done 2026-07-16 |
| `position: relative\|absolute` + `top`/`right`/`bottom`/`left` | taffy (`Position`, `inset`) | ✅ done 2026-07-16 |
| CSS named colours (`red`, `teal`, …) | our `parse_color` | ✅ done 2026-07-16 |
| `flex-flow` | taffy | — |
| `grid-column` / `grid-row` (+ `-start`/`-end`) | taffy (`GridPlacement`) | ✅ done 2026-07-16 (`1 / 3`, `span n`, `-1`; no named lines) |
| `grid-auto-flow`, `grid-auto-rows/columns` | taffy | ✅ done 2026-07-16 |
| per-corner `border-radius` | kurbo (`RoundedRectRadii`) | ✅ done 2026-07-16 |
| `letter-spacing`, `word-spacing` | parley | ✅ done 2026-07-16 |
| `font-style: italic` | parley | ✅ done 2026-07-16 |
| `white-space: nowrap\|pre` | parley (`TextWrapMode`) | ✅ done 2026-07-16 |
| `line-height` | parley (`LineHeight`) | ✅ done 2026-07-16 |
| `text-decoration` (underline/strikethrough) | our own line-drawing off `RunMetrics` | ✅ done 2026-07-16 |
| `box-shadow` | vello (`draw_blurred_rounded_rect`) | ✅ done 2026-07-16 (single outer; inset parsed, not drawn) |
| linear/radial `gradient` backgrounds | peniko `Gradient` | ✅ done 2026-07-16 |
| `transform` (translate/scale/rotate) | kurbo `Affine` | ✅ done 2026-07-16 (visual only — hit regions not transformed) |
| `background-image: url(…)` | our `ImageCache` | ✅ done 2026-07-16 (cover-sized, clipped; no repeat/size/position) |

Mapped this round: the four alignment self/content properties, per-axis gaps,
`aspect-ratio`, `position`+inset (absolute rides taffy's resolved location, so
the painter needed no change), and the full CSS named-colour table (killing the
`#ff0000`→`red` landmine). Driven clean in `examples/css-showcase.rux`.

Then per-corner `border-radius` and grid placement. `Style.radius` became a
`Corners` (`[f32; 4]`, CSS order TL/TR/BR/BL) threaded to `PaintRect`/`PushClip`;
the painter builds a `RoundedRectRadii` (`rux_paint::rounded_rect`, which also
insets for the border stroke), and the radio-circle `radius == 0` shortcut became
`== [0.0; 4]`. `border-radius` parses the diagonal 1–4 shorthand plus the four
`-corner` longhands. Grid items get `grid-column`/`grid-row` (`GridPlace` →
taffy `line()`/`span()`), including the `-start`/`-end` longhands; named lines
are not supported. Driven in `examples/grid.rux`.

Then the first paint-heavy pair. **`box-shadow`** (single outer shadow) parses
`<dx> <dy> <blur>? <spread>? <color>?` into `Style.box_shadow`, and `collect`
emits a `Paint::Shadow` behind the box that the painter draws with vello's
`draw_blurred_rounded_rect` (blur ≈ 2σ). **Gradients**: `Style.background` grew
from `Option<Rgba>` to `Option<Background>` (`Color` | `Gradient`), so the fill
site now brushes with a peniko `Gradient` for linear/radial. Linear endpoints use
the CSS gradient-line formula for the angle (`<n>deg`/`turn` or `to <side>`);
stops without a position spread evenly. Driven in `examples/shadows.rux` and
`examples/gradients.rux`.

Then `transform` and `grid-auto-*`. `grid-auto-flow`/`-rows`/`-columns` are a
plain taffy mapping (auto tracks use the non-repeated track type, so a second
`to_auto_track` sits beside `to_track`). **`transform`** threads a transform
*stack* through the painter: `Style.transform` is the six affine coefficients,
`collect` bakes the transform-origin (box centre) into the matrix and brackets
the subtree with `Paint::PushTransform`/`PopTransform`, and `build_scene` keeps a
`Vec<Affine>` so every draw — fills, strokes, shadows, images, clips, and
`TextEngine::draw` (which gained a transform arg) — uses the accumulated matrix.
**Caveat, by design:** hit/focus/scroll regions are computed untransformed, so a
transformed element still responds to taps at its *original* position. Driven in
`examples/transform.rux`.

That closes the paint-heavy set. `background-image: url(…)` reuses the
`Background` enum (`Image(src)`); the runtime resolves the path against the .rux
file in `resolve_images` (beside `<image>`), and the painter decodes via
`ImageCache` and draws it `cover`-sized, clipped to the box's rounded corners.
`background-size`/`-position`/`-repeat` are not honored (so they still warn).
Finally the two text props that had been deferred: **`line-height`** (unitless ×
font-size, or a length) now sets the line box in both `measure` and `draw` — when
unset, the leading-trim hug is unchanged, so the old text-hug guard still holds;
and **`text-decoration`** (`underline` / `line-through`) is drawn as filled rects
across each glyph run, placed from parley `RunMetrics`. Driven in
`examples/background-image.rux` and `examples/text-detail.rux`.

**The v0.2 CSS list is now complete.** Remaining CSS work is genuinely long-tail
(CSS variables, `@media`, pseudo-classes, `!important`/`inherit`, per-side border
*colours*, `box-sizing`, `text-overflow: ellipsis`), tracked with the ceilings
below and in the "real work" section above.

Text-shaping props came before those (`font-style: italic`, `letter-spacing`,
`word-spacing`, `white-space: nowrap`). While wiring them, the text engine's
methods (which had grown to 6+ positional args after `font-family`) were
refactored to take a single `rux_text::TextStyle` struct, and `rux-layout`'s
`Measure` closure now takes the whole `&TextContent` instead of unpacking fields
— so the *next* text property is a one-line struct field, not another signature
change everywhere. `rux_paint::text_style(&TextContent)` builds the struct and is
shared by the painter and the shell. Proven headless (letter-spacing widens a
run; nowrap keeps one line) and driven in `examples/fonts.rux`.

**`font-family` — ✅ done 2026-07-16.** Was the single most visible gap (you
could not choose a font at all). Now a raw CSS list flows as
`TextContent.font_family` (a new inheriting text property, alongside
`color`/`font-size`) and reaches parley as `FontFamily::Source`, which parses the
list and does name-matching + fallback. Threaded through every text path:
`rux-text`'s `build`/`measure`/`draw`/`caret_geometry`/`index_at_point` gained a
`family: Option<&str>` arg; the `Measure` closure type gained the same. Inherits
down the tree via a new `Inherited { color, font_size, font_family }` struct
(replacing the old `(color, font_size)` tuple). Verified headless by a shaping
test (`monospace` vs default gives different measured widths, blank falls back)
and driven in `examples/fonts.rux`.

**Real work (new machinery, not just mapping):**

- **CSS custom properties + `var()`** — would let the checked-state palette (and
  any theme) live in one place instead of being hard-coded per class. Wants a
  resolution pass in the cascade.
- **`@media` queries** — the honest way to make examples responsive, rather than
  hand-tuning `max-width` per screen.
- **Pseudo-classes** (`:hover`, `:active`, `:focus`, `:checked`) — needs
  interaction state threaded into matching. `:hover`/`:active` also need pointer
  tracking to invalidate. This retires the synthetic `checked` class hack.
- **`!important`, `inherit`/`initial`** — cascade completeness.
- **`text-overflow: ellipsis`** — needs measure-and-truncate; parley won't do it
  for us.
- **Per-side border *colours*** — we store per-side widths but stroke one uniform
  rounded rect, so four different colours means four paths.
- **`box-sizing`** — taffy sizes border-box; `content-box` needs real work.

**Also worth doing while in here:** *say something* when a declaration is ignored.
✅ **Done (2026-07-15):** `warn_if_unhonored` prints one line per unhonored
property (`rux: CSS property \`box-shadow\` is parsed but not yet honored — it
will have no effect`), deduped for the life of the process via a `static` set so
the whole-tree rebuild doesn't repeat it every keystroke. The honored set is the
`HONORED_PROPERTIES` list in `rux-style` — **when you honor a new property below,
add it there too**, or authors get told a working property does nothing.

**Landmine found doing this (2026-07-15):** named colors beyond
`black`/`white`/`transparent` are not resolved, and lightningcss *minifies* hex
to keywords (`#ff0000` → `red`), so a plain `color: #ff0000` silently falls back
to the default. Add a named-color table (it's cheap) as part of the color work.

---

## v0.3 — fine-grained reactivity

A signal write **rebuilds the whole tree**. It is correct, and at these screen
sizes the cost is imperceptible, so this is deliberately *not* v0.2.

**But the cost is not really performance — it is structural, and it compounds
through v0.2.** Because the tree is thrown away on every change, every piece of
*ephemeral UI state* must be restored by hand afterwards. Two such passes exist
already:

- `apply_focus` in `rux-runtime` — puts the caret back.
- scroll offsets in `rux-shell` — keyed by the scroller's index in tree order.

The stale-caret bug (the caret stayed in the input you had just left) was **one
instance of that category, not a one-off**: `apply_focus` set a caret but never
cleared one. Per-binding subscriptions — what
[04 — Architecture](./04-architecture.md) always described — delete the category.
This is the last real divergence between the architecture doc and the code.

### What that means for v0.2 — the standing debt
Selection, hover, drag and scroll-into-view are all ephemeral UI state. **Each one
shipped before v0.3 must add its own restore-after-rebuild pass, and each is a
chance to reproduce the caret bug.** So, while building v0.2:

1. **Keep the restore passes together and named.** When you add one, add it beside
   `apply_focus` — do not scatter them through the shell.
2. **Test the negative case.** The caret bug slipped through because the test
   asserted the caret *appeared* in the focused input and never that it
   *disappeared* from the other. For every piece of ephemeral state, assert it is
   cleared where it should be, not just set where it should be.
3. **Keep a list here** of every restore pass added, so v0.3 knows exactly what it
   is deleting:
   - `apply_focus` (caret **and selection**) — `rux-runtime`
   - scroll offsets — `rux-shell` (now two-axis; re-clamped per layout, since the
     content can shrink under them)
   - `scroll_caret_into_view` (textarea caret) — `rux-shell`
   - `scroll_focus_into_view` (Tab target) — `rux-shell`
   - `open_select` (open dropdown) — `rux-shell`
   - `focus_index` (keyboard focus ring; re-ranged after each rebuild) — `rux-shell`
   - *(add new ones as they land)*

---

## Known ceilings (not scheduled — they need a decision first)

- **True inline text flow.** Two `<text>` siblings stack; they cannot share a
  line. taffy has no inline formatting context, so this needs our own line-breaker
  over parley — a real project, not a patch.
- **Error surfacing.** Unknown CSS is silently ignored; a bad `.rux` file falls
  back to an empty screen. There is no dev overlay. Fine for a prototype, not for
  anyone else's hands.
- **`:checked` and other pseudo-classes.** Today a checked box gets a synthetic
  `checked` *class* — deliberately, to avoid new selector machinery. If pseudo-
  classes arrive (`:hover`, `:focus`, `:disabled`), that hack should be retired
  with them.
- **Accessibility.** `role=` is honored for selectors and semantics but is wired
  to nothing. parley 0.11 pulls in `accesskit`; that door is open.
