# 06 — Roadmap

Where Rux goes next. Written 2026-07-15, on branch `build/m0-window` (7 commits
ahead of the last docs commit, **not pushed**).

For *what works today*, read [05 — As Built](./05-as-built.md). This document is
only about what is **not done yet**, and in what order.

## Release cadence (set 2026-07-18)

**v0.2.0 ships Monday 2026-07-20** (off-cycle, frozen at tag `v0.2.0-rc1`).
**After that, one release every Friday.** Every release is a git tag *and* a blog
post — no post, no tag.

A milestone (v0.3, v0.4, …) can span several Friday **point-releases**; each Friday
must still ship something coherent and demoable on its own. A new *minor* (v0.4.0)
opens only once the previous milestone's whole scope is done.

- **v0.3** — two tracks, both under this banner. Ships across Fridays as v0.3.x:
  - `v0.3.0` — `.rux` syntax coloring (self-contained; ships first).
  - `v0.3.1` — reactivity groundwork (subscriptions + delete the `apply_focus`
    restore pass).
  - `v0.3.2` — reactivity complete (remaining restore passes deleted).
- **v0.4.0** — opens once v0.3 is done; first item is pseudo-classes, unblocked by
  reactivity. See the [v0.4 section](#v04--drawn-from-the-known-ceilings) below.

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
- ~~**Pseudo-classes** (`:hover`, `:active`, `:focus`, `:checked`)~~ — ✅ done
  2026-07-25; see the v0.4 section.
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

### Approach + progress (2026-07-18)

**Signals stay transparent** (`n`, `n = n + 1`, `{{ n }}` — no `.get()/.set()`),
so no example changes. The dependency graph comes from **instrumentation**, not an
authoring-model change:
- **Reads** — rhai's `on_var` callback records which signal names a binding touches
  as it evaluates. That is the binding's subscription set.
- **Writes** — a handler's changed signals are found by diffing signal values
  across the run (`run_handler_tracked`); input edits already name their signal.

Phasing: **v0.3.1** = this tracking + a binding registry so `{{ }}`/attribute
updates *patch in place* (structural directives still fall back to full rebuild);
**v0.3.2** = structural (`r-if`/`r-for`) in place, then delete the restore passes
one at a time, each with its negative-case test. **Then (v0.3.2+ or a follow-on):
the controlled-state opt-in** — a `value`/`on-*` binding on scrollers and stateful
controls (the `r-model` idea, extended), added where a real example wants to own
the value. Decided 2026-07-18 as the middle path; it does *not* reorder the
reactivity core, only layers on top. See [Ephemeral UI state in the
rationale](./01-rationale.md#ephemeral-ui-state-automatic-by-default-controllable-by-opt-in).

- ✅ **Tracking primitives (`rux-script`)** — `eval_value_tracked` returns a
  binding's value *and* its signal deps (locals/params filtered out);
  `run_handler_tracked` returns the signals a handler changed. Headless-tested
  (`tracks_binding_dependencies`, `tracks_handler_writes`). Purely additive — the
  rebuild path is untouched, so nothing regresses. No UI surface yet; that arrives
  when it drives in-place patching (next step).
- ✅ **Binding registry + in-place patch (mechanism)** — `build_styled_tree_tracked`
  threads a child-index *path* through the builder and returns a `BindingRegistry`:
  the patchable `{{ }}` text bindings (path + raw template + `r-for` locals + signal
  deps) and the `structural` set (signals read by any non-text site — directives,
  attributes, input values, component props). `Document::patch(changed)` re-evaluates
  only the text bindings whose deps changed and writes their nodes in place; it
  declines (returns `false`, mutating nothing) when a changed signal is in
  `structural`, leaving the caller to `rebuild()`. Kept `LayoutNode` reactivity-free
  — the registry lives in `rux-style`/`rux-runtime`. Headless-tested
  (`patch_updates_text_and_preserves_caret`, `patch_declines_on_structural_change`).
- ✅ **Shell wired + input values patch in place** — `@tap` handlers
  (`apply_handler`) and input edits (`apply_edit`) run tracked and patch in place,
  rebuilding only on a structural change. Input *values* are now a patchable
  `ValueBinding` (path + model + placeholder/colors), so **keystrokes no longer
  rebuild** unless the signal is also read structurally (e.g. by an `r-if`).
  `RUX_TRACE=1` prints `patched` vs `rebuilt` per change. Headless tests cover
  value-in-place, placeholder fallback, and the structural decline.
- ✅ **`r-show` in place (structural slice 1)** — flips `hidden` only, no shape
  change, no path invalidation: a `ShowBinding` re-evaluates the condition and
  rewrites the bool. Headless-tested both ways.
- **`r-if` / `r-for` in place — the reconciliation engine (slice 2, the big one).**
  These change tree *shape*, which the positional-path registry can't survive
  (an insert/remove shifts every later node's path).
  1. ✅ **Slice 2a — capture (done, additive).** A template-path (AST element-child
     indices) is threaded alongside the tree-path through
     `build_node`/`build_children`/`expand_component`; `build_children` returns the
     structural deps it saw, and `build_node` records a `StructuralParent`
     (tree-path + template-path + deps) for any parent holding structural children.
     Structural changes still full-rebuild — no behavior change yet. Tested.
  2. ✅ **Slice 2b — reconcile-and-splice (done, headless-tested; still to be driven).**
     Took the lower-risk route: `Document::reconcile` builds a fresh tree (reusing
     `build_styled_tree_tracked`), splices only the affected structural subtrees into
     the live one, and re-applies focus *scoped* to those subtrees; `resolve_images`
     runs on the fresh tree. Unaffected subtrees keep their live node identity, so a
     caret elsewhere survives with no whole-tree restore. The registry refreshes to
     the fresh build (live and fresh shapes match, so paths stay valid). `patch()`
     declines only on the non-reconcilable `structural` cases (component props,
     `:src`/`:options`, toggle `checked` class). Tests: r-if reveal/hide preserves an
     outside caret; r-for grows its rows; a toggle still declines.
     *Trade-off vs. the surgical design:* a reconcile still builds a full fresh tree
     (cheap at these sizes) rather than rebuilding just the slot — but it preserves
     ephemeral state, which is the point. Inputs that are *siblings* of an r-if (same
     parent) are still restored by the scoped `apply_focus`, not persistence.
  3. ✅ **Toggles reconcile (checkbox/radio).** A toggle changes only its own node
     (the synthetic `checked` class → style + mark), no shape change: `Toggle::of`
     carries the checked state's deps, the toggle branch records a `ToggleBinding`
     (node path + deps), and `reconcile` splices just that node from the fresh
     build. Siblings are untouched, so a neighbouring caret persists by identity.
     Tested. (Radios group by their shared `r-model` signal — no `name` attribute;
     a radio is checked when `signal == value`, and selecting one un-checks the
     rest.)
  4. ✅ **`:src` / `:options` patch in place.** Value-like: an `AttrBinding` (path +
     expr + deps) rewrites `node.image.src` (then `resolve_images`) or
     `node.options` — no shape change, out of `reg.structural`. Tested.
  5. ✅ **Component props reconcile — the restore-pass category is closed.** A prop
     change re-expands the instance subtree: `expand_component` records a
     `ComponentBinding` (path + prop deps), and `reconcile` splices the whole subtree
     + scoped focus. `note_structural` is removed — **`reg.structural` is now always
     empty, so no interaction wholesale-rebuilds.** A fresh tree is only ever made on
     hot-reload (`reload` → `Document::load`, focus legitimately reset), never on an
     interaction.
     - **`apply_focus` is not deleted, and shouldn't be** — the honest resolution.
       It is (a) the *set-caret* mechanism `set_focus` calls, and (b) the *scoped*
       restore `reconcile` runs after splicing a subtree that may hold the focused
       input. What is gone is its role as a **per-interaction whole-tree restore
       pass** — the stale-caret *category*. The whole-tree `apply_focus` in
       `rebuild()` is now unreachable from interactions (kept as a safety fallback,
       `patch` never returns false).
  6. ✅ **Driven & verified (2026-07-19).** Reactivity driven in the window and
     confirmed working — caret/selection/scroll undisturbed when something elsewhere
     changes. The "not done until driven" gate for v0.3.2 is cleared, so the
     reactivity work is genuinely done, not just headless-green.

### Labels — `for=` (2026-07-19)

Closed the "label without `for`" gap for **toggle targets**: `id`/`label_for` on the
layout `Node`, and a build-time `link_labels` pass gives a `for=` label (with no
`@tap` of its own) the `@tap` of the input whose `id` it targets — so tapping the
label toggles the checkbox/radio. Runs inside the build, so it survives a reconcile.
**Not covered:** focusing a *text* input from its label (a text input has no `@tap`;
needs a shell focus-by-id path) — and `role="label"` semantics for accessibility.
Both are follow-ups. (Radios still group by shared `r-model`, not `name`.)
- ⏳ **Then delete `apply_focus`** — once no structural change rebuilds, a fresh
  tree is never made mid-session, so caret/selection live on the persistent tree
  and the restore pass drops. (`apply_focus` stays only as the *set-caret*
  mechanism `set_focus` calls on focus moves.) Toggle `checked`-class in place is a
  separate small slice (no shape change) that can land alongside.

### Vue-style `:` attribute bindings — `:class`, `:style`, `:attr`

**✅ Done — full Vue parity (2026-07-19):**
- **`:class`** — string, array, and **object/conditional** (`#{ active: cond }` →
  keys whose value is truthy), fed into the cascade.
- **`:style`** — string and **object** (`#{ background: c }`) inline CSS at highest
  priority; **static `style=`** honored too.
- **Interpolation** — rhai backtick template literals (`:style="\`background:
  ${c}\`"`), confirmed working — no template-layer code.
- **`Value::Map`** added (rux-reactive) with rhai `#{…}` ↔ `Value` conversion, the
  foundation for the object forms.
- **Reactivity** reuses the node-splice reconcile (a `StyledBinding` reconciles the
  node when a signal it reads changes; an `r-for`-local-only `:style` rides the
  loop's reconcile). The chip demo is `examples/css-showcase.rux`.
- Tested: `dynamic_class_reconciles`, `dynamic_inline_style_interpolates_and_reconciles`,
  `r_for_chip_styles`, `conditional_class_object_form`, `style_object_form`,
  `css_showcase_example_builds`.

**Still open:** general `:attr` for *arbitrary* attributes (only the styling/data
ones are dynamic today); rhai's object syntax is `#{…}`, not Vue's `{…}`.

**Goal: inherit Vue's `:whatever` model.** `:` uniformly means "bind this attribute
to a script expression." Already partly true (`:src`, `:options`, component `:props`);
this makes it general and adds the two the examples keep reaching for. **String
interpolation is already free** — `:` values evaluate as rhai, and rhai has backtick
template literals (`:style="\`background: ${c}\`"`), so no template-layer work is
needed.

- **`:class`** — dynamic classes fed into the cascade. This is the synthetic
  `checked`-class pattern generalized: evaluate and push into `desc.classes` right
  beside the `checked` push in `build_node` (`rux-style`), then the cascade matches
  them. Forms to support (Vue parity):
  - string — `:class="c"` → those classes;
  - array — `:class="[a, b]"` → each item a class;
  - object (conditional) — `:class="#{ active: is_active }"` → keys whose value is
    truthy. **Note:** rhai object maps are `#{ … }`, not Vue's `{ … }`, and `Value`
    has no `Map` variant today — the object form needs a `Value::Map` (or
    Dynamic-level handling). String/array forms need neither.
- **`:style`** — dynamic inline CSS, applied at **highest priority** (inline wins
  over the cascade). Forms: string (`:style="\`background: ${c}\`"`) and object
  (`#{ background: c }`). **Also honor static `style="…"`** while here (not honored
  today). Needs an **inline-declaration parser** (`"a: b; c: d"` → pairs; reuse
  `lightningcss`) merged into `props` before `interpret`.
- **`:attr` (general)** — bind any attribute to an expression, uniformly (today only
  a hand-picked set is dynamic).
- **Reactivity — already built.** A `:class`/`:style` change re-cascades/re-interprets
  *one node* — exactly the **node-splice reconcile** toggles and components already
  use. Record a `{path, deps}` binding and add it to `reconcile`'s node-splice list;
  no new reconcile logic. Inside an `r-for`, a change to the *collection* signal
  reconciles the parent (re-evaluating `:style` per item), which is why the chip
  example "just works" once the binding is wired.

**Note on the example:** for *data-driven* colours, `:style` is the workhorse;
`:class="c"` only does something if a matching CSS rule exists (`.red { … }`) — a raw
hex like `#a1b2c3` isn't a usable class. Use `:class` for values that map to
predefined classes (themes/states), `:style` for computed values.

**Effort:** `:class` string/array small; `:style` string medium (inline parser); the
object forms gate on the `Value::Map` decision. Slots with the "real work" CSS bucket
(`var()`, `@media`) below.

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

**A second answer, now decided: controlled state — opt-in.** Controlled state makes
ephemeral state *model-owned* (`<scroll value="{off}" on-scroll="…">`), so it
survives a rebuild because it lives in the model, and author logic can *drive* it
(scroll-to-top on submit, persist a position). The **decision (2026-07-18)** is the
middle path: fine-grained reactivity keeps the uncontrolled defaults automatic and
surviving, and controlled state is an **opt-in** for values the author wants to own.
**This does not reorder the reactivity work** — build the reactivity core first
(below); add the controlled-state opt-in as an additive binding afterward, where a
real example needs it. Full argument:
[01 — Rationale → Ephemeral UI state](./01-rationale.md#ephemeral-ui-state-automatic-by-default-controllable-by-opt-in).

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

## v0.3 (also) — `.rux` syntax coloring

Scheduled alongside reactivity; independent of it. Today a `.rux` code block on
the [site](../site/) renders as flat text, and editing a `.rux` file in an editor
gives no coloring at all. Both are fixable with **one artifact**.

**The key finding (verified 2026-07-18):** Zola 0.22's highlighter,
[Giallo][giallo], consumes **TextMate JSON grammars** — the *same format VS Code
uses*. So a single `rux.tmLanguage.json` serves both consumers. No fork, no second
grammar in a second format.

### Progress (2026-07-18) — grammar + wiring done, `zola build` verify pending

- ✅ **Grammar written**, self-contained (design decision below resolved that way):
  `editors/vscode/syntaxes/rux.tmLanguage.json`, copied to `site/syntaxes/`. Scopes
  all three sections and the Rux tokens (`{{ }}`, `r-for`/`r-if`/`r-model`, `@tap`,
  `:prop`, `signal`).
- ✅ **Wired**: `extra_grammars` added to `site/config.toml`; VS Code extension
  scaffolded (`package.json`, `language-configuration.json`, `README.md`).
- ✅ **Verified via the real tokenizer** — ran all 18 examples through
  `vscode-textmate` + `vscode-oniguruma` (the engine VS Code uses) and inspected
  scopes: every section and Rux token colors correctly; combinators / `@media` /
  `:hover` / `var()` confirmed on a synthetic case. No exceptions across the sweep.
- ⏳ **Still to do before tagging v0.3.0:** `cd site && zola build` (no `zola`
  binary in the dev env used — must run where Zola 0.22.1 is installed) and **look**
  at a rendered `.rux` block; then install the `.vsix` and open a `.rux` file. The
  standing rule: not done until driven.

**Design decision (resolved): self-contained**, no `source.css`/`source.rust`/
`text.html.basic` includes — chosen for identical rendering in both VS Code and
Giallo regardless of what either host bundles (Giallo's bundled set was the
unknown). The `@media (...)` condition interior is left uncolored; documented in
`editors/vscode/README.md`.

**Not to be confused** with the *language* feature of authors including an
external stylesheet (see Further out). That is a runtime/parser feature; the only
grammar impact when it lands is coloring the include statement itself
(`src=` / `@import "…"`), a few lines. It does **not** push us toward the TextMate
`source.css` include — an external file is colored by the editor's own CSS grammar,
and our self-contained patterns keep handling inline `<style>`.

### The work

1. **Write `rux.tmLanguage.json`** — this is the real task; the wiring is trivial.
   A `.rux` file is a multi-language SFC (like Vue), so the grammar scopes the
   three sections and colors what's inside each:
   - `<template>` — HTML-like tags, plus the Rux-specific bits: `{{ }}`
     interpolation, `r-for` / `r-if` / `r-model` directives, `@tap` handlers,
     `:prop` bindings.
   - `<style>` — CSS.
   - `<script>` — rhai (Rust-like: `let`, `fn`, `//`, strings, `signal(...)`).

   **Design decision to make first:** embed sub-grammars via `include`
   (`source.css`, `source.rust`, `text.html.basic`) or write **self-contained**
   inline patterns. Include is less code but depends on each host bundling those
   sub-grammars (VS Code does; Giallo's bundled set needs checking). Self-contained
   is more work but fully portable and predictable. Lean self-contained for the
   Rux-specific tokens (`{{ }}`, `r-`, `@`, `signal`) regardless, since no stock
   grammar knows them.

   **Known imprecision:** rhai has no standard TextMate grammar. `<script>` will
   lean on Rust's, which is close (keywords, comments, strings line up) but not
   exact — rhai-only constructs won't be perfect. Acceptable; document it.

2. **Wire into Zola** — drop the grammar in `site/syntaxes/`, add to
   `site/config.toml`:
   ```toml
   [markdown.highlighting]
   extra_grammars = ["syntaxes/rux.tmLanguage.json"]
   ```
   The site's fences are already ```` ```rux ````, so they light up once the
   grammar registers `rux` as a language. Pin the Giallo/Zola version note in the
   config comment.

3. **VS Code extension** — scaffold `editors/vscode/`: `package.json`
   (`contributes.languages` + `contributes.grammars`), `language-configuration.json`
   (comments, brackets, auto-close), and the **same** `rux.tmLanguage.json` (copy
   or symlink — one source of truth). Ship a `.vsix` in the repo for local install;
   publishing to the Marketplace is optional and needs a publisher account.

### Verification (the standing rule applies to tooling too)

- `cd site && zola build` warning-clean, then **open the site and look** at a
  `.rux` block — real colors, not flat text.
- Install the extension locally and **open a `.rux` file** — template, style and
  script sections all colored, `{{ }}` / `r-` / `@tap` picked out.

[giallo]: https://github.com/getzola/giallo

---

## Dev tooling — the editor extension beyond coloring

Syntax coloring is the first slice of a real editor experience, not the whole of
it. The rest is sequenced below. **The governing principle:** anything that needs
to *understand* a `.rux` file (format it, find its errors, complete a name) must
reuse the **real parser** in `rux-parser` / `rux-style` / `rux-script` — never a
second, drifting reimplementation in TypeScript. The pattern mirrors the grammar's
"one source of truth, two consumers": build the capability as a **`rux` CLI
subcommand** first (serving CLI users), then have a thin VS Code extension shell out
to it. Today `rux` only runs an app (`rux [path]`, `crates/rux-cli`); these add
subcommands.

### Tier 0 — declarative + a stopgap formatter (✅ done 2026-07-18)

Ships with coloring.
- ✅ **Snippets** (`snippets/rux.json`) — component scaffold, section blocks,
  `r-for`/`r-if`/`r-model`, `signal`, `@tap`, interpolation, common elements.
- ✅ **Folding** of the three sections + **HTML-style tag indentation**
  (`indentationRules`, `onEnterRules`) and bracket/quote auto-close, in
  `language-configuration.json`.
- ✅ **File icon** — `.rux` files show the Rux mark via `contributes.languages.icon`
  (shown when the active file-icon theme falls back to language icons; Seti does).
- ✅ **Basic Format Document** (`Shift+Alt+F`) — a bracket/tag-aware **re-indenter**
  in a plain-JS `extension.js` (no build toolchain). Tracks nesting across tags,
  braces, brackets, parens; preserves multi-line comment prose; idempotent; matches
  14/18 hand-written examples byte-for-byte, normalizes the rest. **This is a
  stopgap, not `rux fmt`** — it only fixes indentation, never intra-line spacing,
  wrap, or alignment of multi-line continuations. Registering it made the extension
  a *runtime* extension (a `main`), but still no compiler/bundler — the parser-backed
  formatter below is the real one.

### Tier 1 — Rust CLI subcommand + thin extension glue

**This is the threshold** where the extension stops being static config and becomes
a **compiled TypeScript extension** (an activation entry, a node build). Worth it,
but it's a real step up — plan it deliberately.

- **`rux fmt`** — parse with the real parser, pretty-print, reuse in a VS Code
  `DocumentFormattingEditProvider`. Also a standalone CLI formatter (`rux fmt
  app.rux`) and a pre-commit hook. **Supersedes the Tier-0 stopgap re-indenter** —
  do it via the parser, not a naive indenter, since intra-line spacing, wrap, and
  multi-line-continuation alignment (exactly what the stopgap punts on) need the
  real tree.
- **`rux check`** — parse + report parse errors *and* the unhonored-CSS warnings the
  runtime already computes (`warn_if_unhonored`), emitted in a machine-readable form
  the extension turns into inline **diagnostics** (squiggles). **This retires the
  "Error surfacing" known ceiling** — arguably the highest-value tool here, since
  today a bad `.rux` file just falls back to an empty screen with nothing said.

### Tier 2 — a language server (`rux-lsp`)

A real project (weeks), once Tier 1 proves the parser-reuse path. A `tower-lsp`
server over the same crates, giving live diagnostics, **hover** (a class → its
`.style` rule; a signal → its declaration), **go-to-definition**, **completion**
(CSS property names, declared class names, in-scope signals), and **rename**. The
LSP subsumes `rux check`'s diagnostics and is editor-agnostic (VS Code, Neovim,
Zed) — the same portability win the self-contained grammar bought.

**Sequencing note:** none of Tier 1–2 blocks the v0.3 reactivity work; it's a
parallel track. But `rux check` / the LSP pair naturally with the pseudo-class and
`var()` CSS work in v0.4, since both add things the checker should know about.

---

## v0.4 — drawn from the Known ceilings

**Opens once the whole v0.3 milestone (syntax coloring + reactivity) has shipped.**
Nothing here is committed to a specific Friday yet; this is the ordered pool the
v0.4.x point-releases draw from. Reactivity landing in v0.3 is what unblocks the
first item.

1. **Pseudo-classes** (`:hover`, `:focus`, `:active`, `:checked`) — ✅ **done
   2026-07-25, driven & verified in the window.** All four match; they stack, carry
   class-level specificity, and work anywhere in a chain (`.card:hover .title`).

   - **How the state gets in.** `rux_style::InteractionState` (hovered path, active
     path, focused `r-model`) is threaded through the build; `ElemDesc` carries an
     `ElemStates` the pseudo-classes test. `:checked` resolves from the toggle's
     `r-model` at build time; the other three come from the shell.
   - **How the shell knows what to report.** A node any `:hover`/`:active` rule
     could match is marked with its tree path (`Node.state_path`), and the layout
     emits a `StateRegion` for it — so a document with no pointer-state rules emits
     none and pays nothing. Deliberately an over-approximation (only the
     pseudo-carrying compound is tested): over-flagging costs a region,
     under-flagging would be a rule that silently never fires.
   - **How it stays cheap.** `Document::set_interaction` re-cascades only the
     subtree where the old and new pointer chains **diverge**, reusing the v0.3
     node-splice reconcile — so a caret or selection elsewhere survives by node
     identity. Moving within one element is not a state change and does no work.
   - **Two bugs this surfaced.** (a) `parse_compound` used to stop at the `:` and
     drop it, so `.box:hover` parsed as plain `.box` and applied *unconditionally*;
     an unknown pseudo-class now fails closed and warns once. (b) Found only by
     driving it: the pointer leaving the window fires `CursorLeft`, not a
     `CursorMoved`, so a hovered element stayed lit after the pointer was gone —
     the "set but never cleared" category again, now covered by a test that asserts
     the *clearing*.
   - **Still open:** the synthetic `checked` class is kept one release for
     compatibility (examples and docs are migrated to `:checked`); entering or
     leaving *all* interactive boxes re-cascades from the root rather than a
     sub-path, which is correct but coarser than the sibling-to-sibling case.
2. **CSS custom properties + `var()`** — ✅ **done 2026-07-25, driven & verified.**
   `--name` declarations inherit like `color`; `var()` is substituted into every
   declaration *after* the cascade and inline styles merge, so every property gets
   variables without its parser knowing they exist. Fallbacks, variables defined in
   terms of variables, per-subtree overrides, and a depth cap for cycles. Undefined
   + no fallback = the declaration is dropped (CSS's rule) and warned once. The
   var map is shared by `Rc`, so a subtree declaring none copies nothing.
   `examples/theme.rux` swaps a whole palette with one `:class`.

   **Gap this surfaced:** attribute values were never entity-decoded, so there was
   no way to write a script string literal inside a `:` binding or `@tap` — the
   attribute and the expression share the `"` delimiter. `decode_entities` moved to
   `rux-parser` and now runs as attributes are read (`&quot;` works), with
   `rux-style` reusing it rather than keeping a second table.
3. **`@media` queries** — ✅ **done 2026-07-26, driven & verified.** Rules inside a
   non-matching block are simply never emitted, so matching, cascade and
   specificity are untouched: a block adds no specificity, and the order counter
   runs across blocks so a later `@media` rule beats an earlier plain one.
   `min-`/`max-width`/`-height`, `orientation`, `screen`/`all`, `and` chains and
   comma alternatives; unsupported conditions warn once and never apply.

   **Landmine:** lightningcss *normalizes* `(min-width: 600px)` into the Media
   Queries Level 4 range form `(width >= 600px)` before we see it, so the range
   spelling is the one that actually arrives — the `min-`/`max-` arm is the
   compatibility path, not the main one. Both are parsed, including double-ended
   `(400px <= width <= 600px)`.

   **Cost control:** `Document::set_viewport` evaluates every media condition at
   the old and new size and re-cascades only if that vector differs — so a resize
   crossing no breakpoint does nothing, and a document without `@media` never
   re-cascades. `examples/responsive.rux` drives it.
4. **Error surfacing / dev overlay** — today unknown CSS is silently ignored and a
   bad `.rux` file falls back to an empty screen. The biggest gap before this is in
   anyone else's hands. (Also listed under Known ceilings.)
5. **Accessibility** — parley 0.11 already pulls in `accesskit`; `role=` is honored
   for selectors but wired to nothing. That door is open.

Deliberately **not** in the v0.4 pool: true inline text flow — flagged below as
"a real project, not a patch," too big for a weekly slot.

**Also expected (not yet scheduled):** **external CSS include** — let authors pull
in a stylesheet from a separate file (e.g. `<style src="…">` or an `@import`)
instead of only inline `<style>`. A runtime/parser feature; the syntax-coloring
grammar needs only a small addition to color the include statement when this lands
(see the syntax-coloring design note — this is unrelated to the grammar's
self-contained-vs-`source.css` decision).

---

## Known ceilings (not scheduled — they need a decision first)

- **True inline text flow.** Two `<text>` siblings stack; they cannot share a
  line. taffy has no inline formatting context, so this needs our own line-breaker
  over parley — a real project, not a patch.
- **Error surfacing.** Unknown CSS is silently ignored; a bad `.rux` file falls
  back to an empty screen. There is no dev overlay. Fine for a prototype, not for
  anyone else's hands.
- ~~**`:checked` and other pseudo-classes.**~~ ✅ Resolved 2026-07-25: `:hover`,
  `:focus`, `:active` and `:checked` all match. The synthetic `checked` class
  survives one more release for compatibility, then goes.
- **Accessibility.** `role=` is honored for selectors and semantics but is wired
  to nothing. parley 0.11 pulls in `accesskit`; that door is open.

---

## Further out (post-v0.4, sequenced but not versioned yet)

Bigger themes queued after the v0.4 Known-ceilings pool drains. Ordered; each is a
milestone in its own right, not a Friday slice. Recorded 2026-07-18.

1. **TailwindCSS integration.** A utility-class layer over the CSS engine. Only
   sane *after* the "real work" CSS lands in v0.4 — Tailwind leans hard on custom
   properties (`var()`), `@media`, and pseudo-classes (`:hover`/`:focus`), so it
   depends on items 1–3 of the v0.4 pool existing first. Open question to settle up
   front: ship a curated utility set generated into Rux CSS, or run an actual
   Tailwind pass over `.rux` files — the former is self-contained, the latter pulls
   in the Node/Tailwind toolchain we've so far avoided.

2. **Element access + manipulation from script.** A DOM-like handle so `<script>`
   can read and mutate the tree directly (query a node, set a property, add/remove
   children) instead of only driving it through signals. This is a large surface
   and interacts with fine-grained reactivity (v0.3): script mutations must feed the
   same subscription graph, or they desync from the declarative tree. Sequence it
   *after* reactivity is solid for exactly that reason.

3. **Script documentation.** Rux's script layer needs its own docs, *beyond*
   upstream rhai's — because the intent is to **modify the rhai version Rux adopts**
   (Rux-specific builtins like `signal(...)`, the element-access API from item 2,
   and whatever language changes the fork carries). Once we diverge from stock rhai,
   pointing users at rhai's docs is no longer correct. This depends on item 2's API
   being settled, so it comes last of the three.

   A concrete fork motivator, established by probing the engine directly: **no
   stock-rhai callable can mutate a top-level signal.** A named `fn` errors (signals
   aren't in a function's scope — rhai functions are pure/scopeless); a closure's
   `taps = taps + 1` rebinds the captured name rather than writing back; and even a
   pre-shared `Dynamic` cell with `+=` fails to propagate. Only a statement run
   *directly in the top scope* mutates a signal, which is why inline
   `@tap="taps = taps + 1"` works but a reusable handler function does not. So
   JS-style arrow functions that mutate signals — `const inc = () => taps++` — cannot
   be real functions over stock rhai. Two paths: inline them as named statement
   templates expanded at the handler call site (no engine change, but they stay
   compile-time macros, not first-class values), or make signals shared cells the
   fork can write through (true first-class functions, but a reactivity-core change).
   The second is the language change this fork would carry.
