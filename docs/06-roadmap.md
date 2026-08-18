# 06. Roadmap

Where Rux goes next. Last updated 2026-08-11, with the **`rux-rhai` fork design**
under v0.7.

For *what works today*, read [As Built](./05-as-built.md). This document is
only about what is **not done yet**, and in what order.

## Release cadence (set 2026-07-18)

**v0.2.0 ships Monday 2026-07-20** (off-cycle, frozen at tag `v0.2.0-rc1`).
**After that, one release every Friday.** Every release is a git tag *and* a blog
post. No post, no tag.

A milestone (v0.3, v0.4, …) can span several Friday **point-releases**; each Friday
must still ship something coherent and demoable on its own. A new *minor* (v0.4.0)
opens only once the previous milestone's whole scope is done.

- **v0.3**: two tracks, both under this banner. Ships across Fridays as v0.3.x:
  - `v0.3.0`: `.rux` syntax coloring (self-contained; ships first).
  - `v0.3.1`: reactivity groundwork (subscriptions + delete the `apply_focus`
    restore pass).
  - `v0.3.2`: reactivity complete (remaining restore passes deleted).
- **v0.4.0**: opens once v0.3 is done; first item is pseudo-classes, unblocked by
  reactivity. See the [v0.4 section](#v04-drawn-from-the-known-ceilings) below.

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

## v0.1: the shake-down (next up)

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
- **Text escaping its box at narrow widths**: the wrap invariant (a text box is
  never narrower than the text measured) breaking under a new width.
- **Scroll offsets stranding content after a resize**: they are re-clamped per
  layout, but that path is untested against a *changing* viewport.
- **A panic on minimize**: the surface goes to zero; wgpu now reports occluded /
  timed-out frames as a status we skip, but that is unverified.
- **`ScaleFactorChanged`**: we handle `Resized` but not this. Layout reads the
  scale factor every frame so it *should* be fine. Unverified.
- **Any ephemeral UI state that does not survive a rebuild** (see below).

### Shake-down progress (2026-07-15)
- **Done.** `list.rux`, `gallery.rux` made responsive; `gallery` is now a `flex-wrap`
  grid. `dashboard.rux` cleaned up into a dark-themed `1fr 1fr 1fr` grid demo.
- **Done.** Driven on screen: `gallery`, `list`, `form`, `dashboard`. Three
  test-invisible bugs found and fixed (see the CSS section below for the two
  layout ones; the `r-for` `@tap` one is there too).
- **Done.** **Minimize/restore: verified clean**: no panic when the surface goes to
  zero, editing/caret resume correctly on restore.
- **Done.** Blinking caret added (user request): 530ms, solid while typing.
- **Done.** `minmax(0, 1fr)` grid tracks added (user request, after the dashboard's
  `1fr` columns overflowed a narrow window, expected CSS but ungraceful):
  `Track::MinMax` → taffy `minmax()`, a paren-aware `parse_tracks`. Lets tracks
  shrink below content instead of overflowing. `dashboard.rux` now uses it.
- **Pending.** **`ScaleFactorChanged` / cross-DPI drag: still unverified**: needs a
  second monitor (deferred to the week of 2026-07-20).
- **Pending.** `battery.rux` (the fixed-width control) not re-driven yet.

### 4. Then tag `v0.1`
Only once the above is clean, specifically **do not tag until the cross-DPI
drag is verified**, since `ScaleFactorChanged` is the last untested surface path.

---

## v0.2: inputs, polish, and CSS

**All four items are done (2026-07-17).** What's left under each is listed there
as *Not done*: long-tail CSS (variables, `@media`, pseudo-classes) is the
biggest of it, and fine-grained reactivity (below) is still the largest gap
between this code and [Architecture](./04-architecture.md).

### 1. Text selection + clipboard: **done (2026-07-17)**

A focused input now has a **selection, not just a caret**. `rux_runtime::Focus`
carries `model` + `caret` + `anchor`; the range between them is the selection, and
`apply_focus` re-applies both after every rebuild: one restore pass, not two.

- **Done.** **Drag-select** (press anchors, drag extends), **double-click** selects a
  word (`DOUBLE_CLICK` window + `TAP_SLOP`).
- **Done.** **Shift+movement** extends from the anchor; a movement without Shift
  collapses. Typing/pasting/Backspace/Delete replace the selection.
- **Done.** **Ctrl+A / C / X / V** against the real system clipboard (`arboard`, with
  `image-data` off). A multi-line paste into a single-line input keeps the first
  line only. No clipboard → a warning at startup and copy/paste no-ops, rather
  than a crash.
- **Done.** **The highlight** is painted behind the glyphs in the focus-ring blue (no
  `::selection` yet, so it isn't author-controlled).

**The trap worth remembering:** parley's `Selection::geometry` returns rects laid
out on *parley's* line pitch, but we draw lines with the leading trimmed
(`ascent + descent`, or `line-height`). Taking its rects wholesale would drift the
highlight further off the glyphs with every wrapped line. `selection_rects` takes
only the **horizontal** extent from parley and recomputes `y` from our own
stepping, keyed by the line index parley hands back. Guarded by
`rects_line_up_with_our_own_line_stepping` and `rects_follow_line_height`.

Also: `press_text` runs before tap dispatch (a selection drag has to start on
press), but declines while a dropdown is open, since otherwise an option floating over
a textarea would focus the textarea instead of picking the option.

**Not done:** word-wise movement (Ctrl+arrows), triple-click line-select,
drag-and-drop of selected text, `::selection` styling, middle-click paste on X11.

### 2. The last two input types: mostly done (2026-07-16)
- **Done.** **`type="textarea"`**: a multi-line text input. It's the ordinary text
  input plus a `multiline` flag on the node → `FocusRegion`; the shell inserts a
  newline on Enter (single-line inputs still ignore it), and the value wraps.
- **Done.** **`type="select"`**: evaluates `:options` to strings at build time
  (`Node.options`), exposed as a `SelectRegion`. The shell owns the open state
  (`open_select`, survives rebuilds), draws the dropdown as an overlay appended
  on top of the scene, hit-tests the rows itself (`dropdown_row`), writes the
  chosen value back to the model, and closes on any other tap. Guarded by a
  `rux-style` test; driven in `examples/form-controls.rux`.
- **Done.** **checkbox/radio keyboard-reachability (2026-07-17)**: the layout now emits
  `Layout.focusables` (a document-ordered `FocusItem` list: text inputs, buttons,
  toggles, selects). The shell keeps a `focus_index`; **Tab**/**Shift+Tab** move a
  focus ring through them (tapping syncs it too), a focused text input edits, and
  a focused button/checkbox/radio activates on **Space/Enter** (select opens).
- **Done.** **Input polish (2026-07-17, from testing):** inputs default to `width:100%`
  (they were hugging their text and shrinking as you typed); single-line inputs
  are `nowrap` + clip; textarea gets Up/Down caret movement; the dropdown is
  restyled as one floating panel (shadow, selected pill, separators).
- **Still open:** select has no keyboard list navigation or native mobile
  picker; a select's `cursor: pointer` doesn't apply (selects aren't `@tap` hit
  regions); text inputs don't scroll horizontally (long single lines clip).

### 3. Scrolling polish: **done (2026-07-17)**

Scrolling was wheel-only and vertical-only. Now:

- **Done.** **Horizontal scrolling.** The offset is a two-axis `rux_layout::Offset`, and a
  scroller reports `content_width`/`content_height` and a `max` on each axis, so a
  box scrolls whichever way its content actually overflows. Shift+wheel (and a
  horizontal wheel) scroll sideways.
- **Done.** **Scrollbars.** An overlay on the box's trailing edge, drawn *over* the
  content (a scroller clips its children, so a bar inside the subtree would be
  clipped away). A bar only exists on an axis with travel; the thumb is the box's
  fraction of the content, floored so it stays grabbable; with both axes live the
  tracks stop short of the corner. Paint and hit-testing share `bar_track` /
  `bar_thumb`, so what you see is what you can grab.
- **Done.** **Drag.** A press on a thumb starts a drag and never becomes a tap on the
  content beneath it; pointer travel down the *track* maps to the content's travel
  through its full range.
- **Done.** **Touch.** A finger drags the content itself. **Unverified: no touch
  hardware here**; it is the one part of this item nobody has driven.
- **Done.** **Keyboard.** Arrows, PageUp/PageDown, Home/End scroll the box under the
  pointer, reached only after a focused input has declined the key, so it can't
  steal a caret key.
- **Done.** **Scroll-into-view.** Tab to something below the fold and its box scrolls to
  show it (`scroll_focus_into_view`, beside the other restore passes).

**Found by driving it, invisible to the tests:** the horizontal thumb was painted
with the *track's length* as its thickness, a pale slab across the whole box,
because `bar_thumb`'s X arm took the wrong component out of the track tuple. Every
test only looked at the vertical bar. The lesson is the standing one: the axis you
didn't test is the axis that's broken.

**Not done:** track-click paging, kinetic touch fling, scrollbar hover/fade,
`scrollbar-width`/`-color`, `overscroll-behavior`, and `overflow-x`/`overflow-y`
differing from each other (one `overflow` still governs both axes). Single-line
text inputs still clip rather than scroll horizontally.

### 4. CSS: close the gap

The honored set is listed in [As Built](./05-as-built.md). Everything else is
**parsed and silently ignored**: which is the worst failure mode we have: you
write valid CSS, nothing happens, and nothing tells you why. This is the item most
likely to make Rux feel like a toy, so it gets real scope.

**Already fixed during the v0.1 shake-down** (kept here as a landmine map):

- **`@tap` inside `r-for` couldn't see the loop variable.** `@tap="picked = item"`
  silently did nothing: handlers run later in *global* scope (`run_handler` →
  `eval(src, &[])`), where the `r-for` `item` no longer exists, so the assignment
  failed and the bound `r-if` never fired. `form.rux` worked only because its
  handlers reference no loop var, so nothing tested this path. Fixed by baking the
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
  so it fills up to the cap for any overflowing content, i.e. the wrap case).
  Guarded by `crates/rux-layout/tests/wrap.rs`. A version bump does **not** fix
  it, so don't reach for one.

**First, two things that are bugs, not gaps**: both now **fixed** (2026-07-15):

- **Done.** **`>`, `+`, `~` were treated as descendant combinators.** `parse_selector`
  skipped the token, so `.card > text` matched *any* descendant `text`, and the
  wrong elements, silently. Fixed: `parse_selector` now records a `Combinator`
  between each pair of compounds (a bare space is descendant), and a recursive
  `matches_chain` honors all four: descendant, child (`>`), next-sibling (`+`)
  and subsequent-sibling (`~`). Sibling combinators needed preceding-sibling
  context, so the ancestor chain is now `AncNode { desc, prev }` (each ancestor
  carries its own preceding siblings), which resolves even `.a ~ .b .c`. Guarded
  by unit tests that assert the *negative* case (the combinator must NOT match
  where descendant would) plus an end-to-end test and a lightningcss
  serialization round-trip. Known limitation: sibling combinators don't see the
  synthetic `checked` class on a preceding sibling.
- **Done.** **`cursor` was ignored.** Now honored: `Style.cursor` (`rux-layout`), mapped
  from `cursor: pointer` in `interpret`, carried on `HitRegion`, and applied by
  the shell's `update_cursor` on `CursorMoved` (topmost region under the pointer
  wins; the window is only touched when the shape changes). Because it rides on
  the hit regions we already compute, `cursor` is honored **only on tappable
  (`@tap`) boxes**: a `cursor` on a plain box still does nothing. Widen to a
  dedicated cursor-region pass if that bites.

**Cheap, the engine already supports it, we just don't map it:**

| Property | Backed by | Status |
|---|---|---|
| `align-self`, `justify-self`, `align-content`, `justify-items` | taffy | done 2026-07-16 |
| `row-gap` / `column-gap` | taffy | done 2026-07-16 |
| `aspect-ratio` | taffy | done 2026-07-16 |
| `position: relative\|absolute` + `top`/`right`/`bottom`/`left` | taffy (`Position`, `inset`) | done 2026-07-16 |
| CSS named colours (`red`, `teal`, …) | our `parse_color` | done 2026-07-16 |
| `flex-flow` | taffy | n/a |
| `grid-column` / `grid-row` (+ `-start`/`-end`) | taffy (`GridPlacement`) | done 2026-07-16 (`1 / 3`, `span n`, `-1`; no named lines) |
| `grid-auto-flow`, `grid-auto-rows/columns` | taffy | done 2026-07-16 |
| per-corner `border-radius` | kurbo (`RoundedRectRadii`) | done 2026-07-16 |
| `letter-spacing`, `word-spacing` | parley | done 2026-07-16 |
| `font-style: italic` | parley | done 2026-07-16 |
| `white-space: nowrap\|pre` | parley (`TextWrapMode`) | done 2026-07-16 |
| `line-height` | parley (`LineHeight`) | done 2026-07-16 |
| `text-decoration` (underline/strikethrough) | our own line-drawing off `RunMetrics` | done 2026-07-16 |
| `box-shadow` | vello (`draw_blurred_rounded_rect`) | done 2026-07-16 (single outer; inset parsed, not drawn) |
| linear/radial `gradient` backgrounds | peniko `Gradient` | done 2026-07-16 |
| `transform` (translate/scale/rotate) | kurbo `Affine` | done 2026-07-16 (visual only; hit regions not transformed) |
| `background-image: url(…)` | our `ImageCache` | done 2026-07-16 (cover-sized, clipped; no repeat/size/position) |

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
`Vec<Affine>` so every draw (fills, strokes, shadows, images, clips, and
`TextEngine::draw`, which gained a transform arg) uses the accumulated matrix.
**Caveat, by design:** hit/focus/scroll regions are computed untransformed, so a
transformed element still responds to taps at its *original* position. Driven in
`examples/transform.rux`.

That closes the paint-heavy set. `background-image: url(…)` reuses the
`Background` enum (`Image(src)`); the runtime resolves the path against the .rux
file in `resolve_images` (beside `<image>`), and the painter decodes via
`ImageCache` and draws it `cover`-sized, clipped to the box's rounded corners.
`background-size`/`-position`/`-repeat` are not honored (so they still warn).
Finally the two text props that had been deferred: **`line-height`** (unitless ×
font-size, or a length) now sets the line box in both `measure` and `draw`, and when
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
so the *next* text property is a one-line struct field, not another signature
change everywhere. `rux_paint::text_style(&TextContent)` builds the struct and is
shared by the painter and the shell. Proven headless (letter-spacing widens a
run; nowrap keeps one line) and driven in `examples/fonts.rux`.

**`font-family`: done 2026-07-16.** Was the single most visible gap (you
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

- **CSS custom properties + `var()`**: would let the checked-state palette (and
  any theme) live in one place instead of being hard-coded per class. Wants a
  resolution pass in the cascade.
- **`@media` queries**: the honest way to make examples responsive, rather than
  hand-tuning `max-width` per screen.
- ~~**Pseudo-classes** (`:hover`, `:active`, `:focus`, `:checked`)~~: done
  2026-07-25; see the v0.4 section.
- **`!important`, `inherit`/`initial`**: cascade completeness.
- **`text-overflow: ellipsis`**: needs measure-and-truncate; parley won't do it
  for us.
- **Per-side border *colours***: we store per-side widths but stroke one uniform
  rounded rect, so four different colours means four paths.
- **`box-sizing`**: taffy sizes border-box; `content-box` needs real work.

**Also worth doing while in here:** *say something* when a declaration is ignored.
**Done (2026-07-15):** `warn_if_unhonored` prints one line per unhonored
property (`rux: CSS property \`box-shadow\` is parsed but not yet honored, so it
will have no effect`), deduped for the life of the process via a `static` set so
the whole-tree rebuild doesn't repeat it every keystroke. The honored set is the
`HONORED_PROPERTIES` list in `rux-style`: **when you honor a new property below,
add it there too**, or authors get told a working property does nothing.

**Landmine found doing this (2026-07-15):** named colors beyond
`black`/`white`/`transparent` are not resolved, and lightningcss *minifies* hex
to keywords (`#ff0000` → `red`), so a plain `color: #ff0000` silently falls back
to the default. Add a named-color table (it's cheap) as part of the color work.

---

## v0.3: fine-grained reactivity

A signal write **rebuilds the whole tree**. It is correct, and at these screen
sizes the cost is imperceptible, so this is deliberately *not* v0.2.

### Approach + progress (2026-07-18)

**Signals stay transparent** (`n`, `n = n + 1`, `{{ n }}`, with no `.get()/.set()`),
so no example changes. The dependency graph comes from **instrumentation**, not an
authoring-model change:
- **Reads**: rhai's `on_var` callback records which signal names a binding touches
  as it evaluates. That is the binding's subscription set.
- **Writes**: a handler's changed signals are found by diffing signal values
  across the run (`run_handler_tracked`); input edits already name their signal.

Phasing: **v0.3.1** = this tracking + a binding registry so `{{ }}`/attribute
updates *patch in place* (structural directives still fall back to full rebuild);
**v0.3.2** = structural (`r-if`/`r-for`) in place, then delete the restore passes
one at a time, each with its negative-case test. **Then (v0.3.2+ or a follow-on):
the controlled-state opt-in**: a `value`/`on-*` binding on scrollers and stateful
controls (the `r-model` idea, extended), added where a real example wants to own
the value. Decided 2026-07-18 as the middle path; it does *not* reorder the
reactivity core, only layers on top. See [Ephemeral UI state in the
rationale](./01-rationale.md#ephemeral-ui-state-automatic-by-default-controllable-by-opt-in).

- **Done.** **Tracking primitives (`rux-script`)**: `eval_value_tracked` returns a
  binding's value *and* its signal deps (locals/params filtered out);
  `run_handler_tracked` returns the signals a handler changed. Headless-tested
  (`tracks_binding_dependencies`, `tracks_handler_writes`). Purely additive: the
  rebuild path is untouched, so nothing regresses. No UI surface yet; that arrives
  when it drives in-place patching (next step).
- **Done.** **Binding registry + in-place patch (mechanism)**: `build_styled_tree_tracked`
  threads a child-index *path* through the builder and returns a `BindingRegistry`:
  the patchable `{{ }}` text bindings (path + raw template + `r-for` locals + signal
  deps) and the `structural` set (signals read by any non-text site: directives,
  attributes, input values, component props). `Document::patch(changed)` re-evaluates
  only the text bindings whose deps changed and writes their nodes in place; it
  declines (returns `false`, mutating nothing) when a changed signal is in
  `structural`, leaving the caller to `rebuild()`. Kept `LayoutNode` reactivity-free
  The registry lives in `rux-style`/`rux-runtime`. Headless-tested
  (`patch_updates_text_and_preserves_caret`, `patch_declines_on_structural_change`).
- **Done.** **Shell wired + input values patch in place**: `@tap` handlers
  (`apply_handler`) and input edits (`apply_edit`) run tracked and patch in place,
  rebuilding only on a structural change. Input *values* are now a patchable
  `ValueBinding` (path + model + placeholder/colors), so **keystrokes no longer
  rebuild** unless the signal is also read structurally (e.g. by an `r-if`).
  `RUX_TRACE=1` prints `patched` vs `rebuilt` per change. Headless tests cover
  value-in-place, placeholder fallback, and the structural decline.
- **Done.** **`r-show` in place (structural slice 1)**: flips `hidden` only, no shape
  change, no path invalidation: a `ShowBinding` re-evaluates the condition and
  rewrites the bool. Headless-tested both ways.
- **`r-if` / `r-for` in place: the reconciliation engine (slice 2, the big one).**
  These change tree *shape*, which the positional-path registry can't survive
  (an insert/remove shifts every later node's path).
  1. **Done.** **Slice 2a, capture (done, additive).** A template-path (AST element-child
     indices) is threaded alongside the tree-path through
     `build_node`/`build_children`/`expand_component`; `build_children` returns the
     structural deps it saw, and `build_node` records a `StructuralParent`
     (tree-path + template-path + deps) for any parent holding structural children.
     Structural changes still full-rebuild, so no behavior change yet. Tested.
  2. **Done.** **Slice 2b, reconcile-and-splice (done, headless-tested; still to be driven).**
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
     (cheap at these sizes) rather than rebuilding just the slot, but it preserves
     ephemeral state, which is the point. Inputs that are *siblings* of an r-if (same
     parent) are still restored by the scoped `apply_focus`, not persistence.
  3. **Done.** **Toggles reconcile (checkbox/radio).** A toggle changes only its own node
     (the synthetic `checked` class → style + mark), no shape change: `Toggle::of`
     carries the checked state's deps, the toggle branch records a `ToggleBinding`
     (node path + deps), and `reconcile` splices just that node from the fresh
     build. Siblings are untouched, so a neighbouring caret persists by identity.
     Tested. (Radios group by their shared `r-model` signal, with no `name` attribute;
     a radio is checked when `signal == value`, and selecting one un-checks the
     rest.)
  4. **Done.** **`:src` / `:options` patch in place.** Value-like: an `AttrBinding` (path +
     expr + deps) rewrites `node.image.src` (then `resolve_images`) or
     `node.options`, no shape change, out of `reg.structural`. Tested.
  5. **Done.** **Component props reconcile: the restore-pass category is closed.** A prop
     change re-expands the instance subtree: `expand_component` records a
     `ComponentBinding` (path + prop deps), and `reconcile` splices the whole subtree
     + scoped focus. `note_structural` is removed: **`reg.structural` is now always
     empty, so no interaction wholesale-rebuilds.** A fresh tree is only ever made on
     hot-reload (`reload` → `Document::load`, focus legitimately reset), never on an
     interaction.
     - **`apply_focus` is not deleted, and shouldn't be**: the honest resolution.
       It is (a) the *set-caret* mechanism `set_focus` calls, and (b) the *scoped*
       restore `reconcile` runs after splicing a subtree that may hold the focused
       input. What is gone is its role as a **per-interaction whole-tree restore
       pass**: the stale-caret *category*. The whole-tree `apply_focus` in
       `rebuild()` is now unreachable from interactions (kept as a safety fallback,
       `patch` never returns false).
  6. **Done.** **Driven & verified (2026-07-19).** Reactivity driven in the window and
     confirmed working, with caret/selection/scroll undisturbed when something elsewhere
     changes. The "not done until driven" gate for v0.3.2 is cleared, so the
     reactivity work is genuinely done, not just headless-green.

### Labels: `for=` (2026-07-19)

Closed the "label without `for`" gap for **toggle targets**: `id`/`label_for` on the
layout `Node`, and a build-time `link_labels` pass gives a `for=` label (with no
`@tap` of its own) the `@tap` of the input whose `id` it targets, so tapping the
label toggles the checkbox/radio. Runs inside the build, so it survives a reconcile.
**Not covered:** focusing a *text* input from its label (a text input has no `@tap`;
needs a shell focus-by-id path), and `role="label"` semantics for accessibility.
Both are follow-ups. (Radios still group by shared `r-model`, not `name`.)
- **Pending.** **Then delete `apply_focus`**: once no structural change rebuilds, a fresh
  tree is never made mid-session, so caret/selection live on the persistent tree
  and the restore pass drops. (`apply_focus` stays only as the *set-caret*
  mechanism `set_focus` calls on focus moves.) Toggle `checked`-class in place is a
  separate small slice (no shape change) that can land alongside.

### Vue-style `:` attribute bindings: `:class`, `:style`, `:attr`

**Done, full Vue parity (2026-07-19):**
- **`:class`**: string, array, and **object/conditional** (`#{ active: cond }` →
  keys whose value is truthy), fed into the cascade.
- **`:style`**: string and **object** (`#{ background: c }`) inline CSS at highest
  priority; **static `style=`** honored too.
- **Interpolation**: rhai backtick template literals (`:style="\`background:
  ${c}\`"`), confirmed working, with no template-layer code.
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
interpolation is already free**: `:` values evaluate as rhai, and rhai has backtick
template literals (`:style="\`background: ${c}\`"`), so no template-layer work is
needed.

- **`:class`**: dynamic classes fed into the cascade. This is the synthetic
  `checked`-class pattern generalized: evaluate and push into `desc.classes` right
  beside the `checked` push in `build_node` (`rux-style`), then the cascade matches
  them. Forms to support (Vue parity):
  - string: `:class="c"` → those classes;
  - array: `:class="[a, b]"` → each item a class;
  - object (conditional): `:class="#{ active: is_active }"` → keys whose value is
    truthy. **Note:** rhai object maps are `#{ … }`, not Vue's `{ … }`, and `Value`
    has no `Map` variant today, so the object form needs a `Value::Map` (or
    Dynamic-level handling). String/array forms need neither.
- **`:style`**: dynamic inline CSS, applied at **highest priority** (inline wins
  over the cascade). Forms: string (`:style="\`background: ${c}\`"`) and object
  (`#{ background: c }`). **Also honor static `style="…"`** while here (not honored
  today). Needs an **inline-declaration parser** (`"a: b; c: d"` → pairs; reuse
  `lightningcss`) merged into `props` before `interpret`.
- **`:attr` (general)**: bind any attribute to an expression, uniformly (today only
  a hand-picked set is dynamic).
- **Reactivity, already built.** A `:class`/`:style` change re-cascades/re-interprets
  *one node*, exactly the **node-splice reconcile** toggles and components already
  use. Record a `{path, deps}` binding and add it to `reconcile`'s node-splice list;
  no new reconcile logic. Inside an `r-for`, a change to the *collection* signal
  reconciles the parent (re-evaluating `:style` per item), which is why the chip
  example "just works" once the binding is wired.

**Note on the example:** for *data-driven* colours, `:style` is the workhorse;
`:class="c"` only does something if a matching CSS rule exists (`.red { … }`); a raw
hex like `#a1b2c3` isn't a usable class. Use `:class` for values that map to
predefined classes (themes/states), `:style` for computed values.

**Effort:** `:class` string/array small; `:style` string medium (inline parser); the
object forms gate on the `Value::Map` decision. Slots with the "real work" CSS bucket
(`var()`, `@media`) below.

**But the cost is not really performance. It is structural, and it compounds
through v0.2.** Because the tree is thrown away on every change, every piece of
*ephemeral UI state* must be restored by hand afterwards. Two such passes exist
already:

- `apply_focus` in `rux-runtime`: puts the caret back.
- scroll offsets in `rux-shell`: keyed by the scroller's index in tree order.

The stale-caret bug (the caret stayed in the input you had just left) was **one
instance of that category, not a one-off**: `apply_focus` set a caret but never
cleared one. Per-binding subscriptions, what
[Architecture](./04-architecture.md) always described, delete the category.
This is the last real divergence between the architecture doc and the code.

**A second answer, now decided: controlled state, opt-in.** Controlled state makes
ephemeral state *model-owned* (`<scroll value="{off}" on-scroll="…">`), so it
survives a rebuild because it lives in the model, and author logic can *drive* it
(scroll-to-top on submit, persist a position). The **decision (2026-07-18)** is the
middle path: fine-grained reactivity keeps the uncontrolled defaults automatic and
surviving, and controlled state is an **opt-in** for values the author wants to own.
**This does not reorder the reactivity work**: build the reactivity core first
(below); add the controlled-state opt-in as an additive binding afterward, where a
real example needs it. Full argument:
[Rationale → Ephemeral UI state](./01-rationale.md#ephemeral-ui-state-automatic-by-default-controllable-by-opt-in).

### What that means for v0.2: the standing debt
Selection, hover, drag and scroll-into-view are all ephemeral UI state. **Each one
shipped before v0.3 must add its own restore-after-rebuild pass, and each is a
chance to reproduce the caret bug.** So, while building v0.2:

1. **Keep the restore passes together and named.** When you add one, add it beside
   `apply_focus`. Do not scatter them through the shell.
2. **Test the negative case.** The caret bug slipped through because the test
   asserted the caret *appeared* in the focused input and never that it
   *disappeared* from the other. For every piece of ephemeral state, assert it is
   cleared where it should be, not just set where it should be.
3. **Keep a list here** of every restore pass added, so v0.3 knows exactly what it
   is deleting:
   - `apply_focus` (caret **and selection**): `rux-runtime`
   - scroll offsets: `rux-shell` (now two-axis; re-clamped per layout, since the
     content can shrink under them)
   - `scroll_caret_into_view` (textarea caret): `rux-shell`
   - `scroll_focus_into_view` (Tab target): `rux-shell`
   - `open_select` (open dropdown): `rux-shell`
   - `focus_index` (keyboard focus ring; re-ranged after each rebuild): `rux-shell`
   - *(add new ones as they land)*

---

## v0.3 (also): `.rux` syntax coloring

Scheduled alongside reactivity; independent of it. Today a `.rux` code block on
the [site](../site/) renders as flat text, and editing a `.rux` file in an editor
gives no coloring at all. Both are fixable with **one artifact**.

**The key finding (verified 2026-07-18):** Zola 0.22's highlighter,
[Giallo][giallo], consumes **TextMate JSON grammars**: the *same format VS Code
uses*. So a single `rux.tmLanguage.json` serves both consumers. No fork, no second
grammar in a second format.

### Progress (2026-07-18): grammar + wiring done, `zola build` verify pending

- **Done.** **Grammar written**, self-contained (design decision below resolved that way):
  `editors/vscode/syntaxes/rux.tmLanguage.json`, copied to `site/syntaxes/`. Scopes
  all three sections and the Rux tokens (`{{ }}`, `r-for`/`r-if`/`r-model`, `@tap`,
  `:prop`, `signal`).
- **Done.** **Wired**: `extra_grammars` added to `site/config.toml`; VS Code extension
  scaffolded (`package.json`, `language-configuration.json`, `README.md`).
- **Done.** **Verified via the real tokenizer**: ran all 18 examples through
  `vscode-textmate` + `vscode-oniguruma` (the engine VS Code uses) and inspected
  scopes: every section and Rux token colors correctly; combinators / `@media` /
  `:hover` / `var()` confirmed on a synthetic case. No exceptions across the sweep.
- **Pending.** **Still to do before tagging v0.3.0:** `cd site && zola build` (no `zola`
  binary in the dev env used, so it must run where Zola 0.22.1 is installed) and **look**
  at a rendered `.rux` block; then install the `.vsix` and open a `.rux` file. The
  standing rule: not done until driven.

**Design decision (resolved): self-contained**, no `source.css`/`source.rust`/
`text.html.basic` includes, chosen for identical rendering in both VS Code and
Giallo regardless of what either host bundles (Giallo's bundled set was the
unknown). The `@media (...)` condition interior is left uncolored; documented in
`editors/vscode/README.md`.

**Not to be confused** with the *language* feature of authors including an
external stylesheet (see Further out). That is a runtime/parser feature; the only
grammar impact when it lands is coloring the include statement itself
(`src=` / `@import "…"`), a few lines. It does **not** push us toward the TextMate
`source.css` include, since an external file is colored by the editor's own CSS grammar,
and our self-contained patterns keep handling inline `<style>`.

### The work

1. **Write `rux.tmLanguage.json`**: this is the real task; the wiring is trivial.
   A `.rux` file is a multi-language SFC (like Vue), so the grammar scopes the
   three sections and colors what's inside each:
   - `<template>`: HTML-like tags, plus the Rux-specific bits: `{{ }}`
     interpolation, `r-for` / `r-if` / `r-model` directives, `@tap` handlers,
     `:prop` bindings.
   - `<style>`: CSS.
   - `<script>`: rhai (Rust-like: `let`, `fn`, `//`, strings, `signal(...)`).

   **Design decision to make first:** embed sub-grammars via `include`
   (`source.css`, `source.rust`, `text.html.basic`) or write **self-contained**
   inline patterns. Include is less code but depends on each host bundling those
   sub-grammars (VS Code does; Giallo's bundled set needs checking). Self-contained
   is more work but fully portable and predictable. Lean self-contained for the
   Rux-specific tokens (`{{ }}`, `r-`, `@`, `signal`) regardless, since no stock
   grammar knows them.

   **Known imprecision:** rhai has no standard TextMate grammar. `<script>` will
   lean on Rust's, which is close (keywords, comments, strings line up) but not
   exact: rhai-only constructs won't be perfect. Acceptable; document it.

2. **Wire into Zola**: drop the grammar in `site/syntaxes/`, add to
   `site/config.toml`:
   ```toml
   [markdown.highlighting]
   extra_grammars = ["syntaxes/rux.tmLanguage.json"]
   ```
   The site's fences are already ```` ```rux ````, so they light up once the
   grammar registers `rux` as a language. Pin the Giallo/Zola version note in the
   config comment.

3. **VS Code extension**: scaffold `editors/vscode/`: `package.json`
   (`contributes.languages` + `contributes.grammars`), `language-configuration.json`
   (comments, brackets, auto-close), and the **same** `rux.tmLanguage.json` (copy
   or symlink, one source of truth). Ship a `.vsix` in the repo for local install;
   publishing to the Marketplace is optional and needs a publisher account.

### Verification (the standing rule applies to tooling too)

- `cd site && zola build` warning-clean, then **open the site and look** at a
  `.rux` block, with real colors rather than flat text.
- Install the extension locally and **open a `.rux` file**: template, style and
  script sections all colored, `{{ }}` / `r-` / `@tap` picked out.

[giallo]: https://github.com/getzola/giallo

---

## Dev tooling: the editor extension beyond coloring

Syntax coloring is the first slice of a real editor experience, not the whole of
it. The rest is sequenced below. **The governing principle:** anything that needs
to *understand* a `.rux` file (format it, find its errors, complete a name) must
reuse the **real parser** in `rux-parser` / `rux-style` / `rux-script`, never a
second, drifting reimplementation in TypeScript. The pattern mirrors the grammar's
"one source of truth, two consumers": build the capability as a **`rux` CLI
subcommand** first (serving CLI users), then have a thin VS Code extension shell out
to it. Today `rux` only runs an app (`rux [path]`, `crates/rux-cli`); these add
subcommands.

### Tier 0: declarative + a stopgap formatter (done 2026-07-18)

Ships with coloring.
- **Done.** **Snippets** (`snippets/rux.json`): component scaffold, section blocks,
  `r-for`/`r-if`/`r-model`, `signal`, `@tap`, interpolation, common elements.
- **Done.** **Folding** of the three sections + **HTML-style tag indentation**
  (`indentationRules`, `onEnterRules`) and bracket/quote auto-close, in
  `language-configuration.json`.
- **Done.** **File icon**: `.rux` files show the Rux mark via `contributes.languages.icon`
  (shown when the active file-icon theme falls back to language icons; Seti does).
- **Done.** **Basic Format Document** (`Shift+Alt+F`): a bracket/tag-aware **re-indenter**
  in a plain-JS `extension.js` (no build toolchain). **Superseded in v0.5**: the
  JS copy is gone and the extension shells out to `rux fmt`, which is one
  implementation instead of two that had already drifted. What is left of the
  history is worth keeping: a second copy of a rule set is a bug waiting for a
  week to pass.

### Tier 1: Rust CLI subcommand + thin extension glue

**Reached in v0.5.** This was expected to be the threshold where the extension
became a **compiled TypeScript extension** (an activation entry, a node build).
It was not: shelling out to `rux fmt` and `rux check --format json` from plain
JavaScript needs no compiler and no bundler, only `child_process`. The step up
that was budgeted for did not have to be taken, which is worth remembering the
next time a toolchain looks unavoidable.

- **`rux fmt`**: parse with the real parser, pretty-print, reuse in a VS Code
  `DocumentFormattingEditProvider`. Also a standalone CLI formatter (`rux fmt
  app.rux`) and a pre-commit hook. **Supersedes the Tier-0 stopgap re-indenter**,
  do it via the parser, not a naive indenter, since intra-line spacing, wrap, and
  multi-line-continuation alignment (exactly what the stopgap punts on) need the
  real tree.
- **`rux check`**: parse + report parse errors *and* the unhonored-CSS warnings the
  runtime already computes (`warn_if_unhonored`), emitted in a machine-readable form
  the extension turns into inline **diagnostics** (squiggles). **This retires the
  "Error surfacing" known ceiling**: arguably the highest-value tool here, since
  today a bad `.rux` file just falls back to an empty screen with nothing said.

### Tier 2: a language server (`rux-lsp`)

A real project (weeks), once Tier 1 proves the parser-reuse path. A `tower-lsp`
server over the same crates, giving live diagnostics, **hover** (a class → its
`.style` rule; a signal → its declaration), **go-to-definition**, **completion**
(CSS property names, declared class names, in-scope signals), and **rename**. The
LSP subsumes `rux check`'s diagnostics and is editor-agnostic (VS Code, Neovim,
Zed), the same portability win the self-contained grammar bought.

**Sequencing note:** none of Tier 1–2 blocks the v0.3 reactivity work; it's a
parallel track. But `rux check` / the LSP pair naturally with the pseudo-class and
`var()` CSS work in v0.4, since both add things the checker should know about.

---

## v0.4: drawn from the Known ceilings

**Opens once the whole v0.3 milestone (syntax coloring + reactivity) has shipped.**
Nothing here is committed to a specific Friday yet; this is the ordered pool the
v0.4.x point-releases draw from. Reactivity landing in v0.3 is what unblocks the
first item.

1. **Pseudo-classes** (`:hover`, `:focus`, `:active`, `:checked`): **done
   2026-07-25, driven & verified in the window.** All four match; they stack, carry
   class-level specificity, and work anywhere in a chain (`.card:hover .title`).

   - **How the state gets in.** `rux_style::InteractionState` (hovered path, active
     path, focused `r-model`) is threaded through the build; `ElemDesc` carries an
     `ElemStates` the pseudo-classes test. `:checked` resolves from the toggle's
     `r-model` at build time; the other three come from the shell.
   - **How the shell knows what to report.** A node any `:hover`/`:active` rule
     could match is marked with its tree path (`Node.state_path`), and the layout
     emits a `StateRegion` for it, so a document with no pointer-state rules emits
     none and pays nothing. Deliberately an over-approximation (only the
     pseudo-carrying compound is tested): over-flagging costs a region,
     under-flagging would be a rule that silently never fires.
   - **How it stays cheap.** `Document::set_interaction` re-cascades only the
     subtree where the old and new pointer chains **diverge**, reusing the v0.3
     node-splice reconcile, so a caret or selection elsewhere survives by node
     identity. Moving within one element is not a state change and does no work.
   - **Two bugs this surfaced.** (a) `parse_compound` used to stop at the `:` and
     drop it, so `.box:hover` parsed as plain `.box` and applied *unconditionally*;
     an unknown pseudo-class now fails closed and warns once. (b) Found only by
     driving it: the pointer leaving the window fires `CursorLeft`, not a
     `CursorMoved`, so a hovered element stayed lit after the pointer was gone, the "set but never cleared" category again, now covered by a test that asserts
     the *clearing*.
   - **Still open:** the synthetic `checked` class is kept one release for
     compatibility (examples and docs are migrated to `:checked`); entering or
     leaving *all* interactive boxes re-cascades from the root rather than a
     sub-path, which is correct but coarser than the sibling-to-sibling case.
2. **CSS custom properties + `var()`**: **done 2026-07-25, driven & verified.**
   `--name` declarations inherit like `color`; `var()` is substituted into every
   declaration *after* the cascade and inline styles merge, so every property gets
   variables without its parser knowing they exist. Fallbacks, variables defined in
   terms of variables, per-subtree overrides, and a depth cap for cycles. Undefined
   + no fallback = the declaration is dropped (CSS's rule) and warned once. The
   var map is shared by `Rc`, so a subtree declaring none copies nothing.
   `examples/theme.rux` swaps a whole palette with one `:class`.

   **Gap this surfaced:** attribute values were never entity-decoded, so there was
   no way to write a script string literal inside a `:` binding or `@tap`, the
   attribute and the expression share the `"` delimiter. `decode_entities` moved to
   `rux-parser` and now runs as attributes are read (`&quot;` works), with
   `rux-style` reusing it rather than keeping a second table.
3. **`@media` queries**: **done 2026-07-26, driven & verified.** Rules inside a
   non-matching block are simply never emitted, so matching, cascade and
   specificity are untouched: a block adds no specificity, and the order counter
   runs across blocks so a later `@media` rule beats an earlier plain one.
   `min-`/`max-width`/`-height`, `orientation`, `screen`/`all`, `and` chains and
   comma alternatives; unsupported conditions warn once and never apply.

   **Landmine:** lightningcss *normalizes* `(min-width: 600px)` into the Media
   Queries Level 4 range form `(width >= 600px)` before we see it, so the range
   spelling is the one that actually arrives, the `min-`/`max-` arm is the
   compatibility path, not the main one. Both are parsed, including double-ended
   `(400px <= width <= 600px)`.

   **Cost control:** `Document::set_viewport` evaluates every media condition at
   the old and new size and re-cascades only if that vector differs, so a resize
   crossing no breakpoint does nothing, and a document without `@media` never
   re-cascades. `examples/responsive.rux` drives it.
4. **Error surfacing / dev overlay**: **done 2026-07-26, driven & verified.**
   A broken file no longer opens an empty window: the shell paints a dev overlay
   above everything (including a dropdown) with the failure, and a **failed
   hot-reload keeps the last good tree on screen** and marks it stale, so a typo
   mid-edit neither blanks the window nor passes unnoticed. Fixing the file clears
   it; `Document::replace_with` carries the window's own state (viewport, pointer)
   across the reload so a narrow window doesn't come back with desktop styling.

   - **Parse errors carry line and column**, offset onto the *file's* numbering
     rather than the `<template>` section's, so they match an editor's gutter.
   - **Warnings are collected, not just printed.** Thread-local sinks in
     `rux-style` (unhonored property, unknown pseudo-class, undefined `var()`,
     unsupported `@media`) and `rux-script` (expressions that failed to compile or
     evaluate), drained per build by the runtime. stderr keeps its process-wide
     dedupe so a rebuild doesn't spam the terminal; the sinks dedupe only within a
     build so the overlay always lists everything currently wrong.
   - **The silent-expression hole is closed**: `eval` used to swallow every error
     with `.ok()?`, so a typo in `{{ }}` or `@tap` rendered empty and said nothing.
   - `crates/rux-runtime/tests/examples.rs` asserts every shipped example loads
     **warning-free**: a noisy overlay in `examples/` is now a test failure.

   **Still open:** the overlay is not dismissable, and warnings are not yet
   located (no line numbers for CSS). The machine-readable `rux check` subcommand
   in the Dev-tooling section below is the natural next step, it can reuse these
   same sinks.

   **Not fixable from outside the engine:** rhai returns `()` for a missing map
   property instead of erroring, so `{{ user.nmae }}` is still silently empty.
   That is now recorded as the **second motivator for the rhai fork** (Further out
   → item 3), beside the signal-mutation constraint.
5. **Accessibility**: **done 2026-07-26, driven & verified.** `role=` now means
   something to assistive technology, not just to selectors: Rux publishes a real
   accessibility tree via `accesskit` + `accesskit_winit` (0.24 / 0.33, which share
   our exact winit 0.30, no duplicate winit).

   - **Roles resolved at build time** in `rux-style` into a small `AccessRole` enum
     owned by `rux-layout`, so neither crate depends on `accesskit`, only the
     shell translates. Text→Label, `@tap` box→Button, inputs→TextInput/
     MultilineTextInput/ComboBox, toggles→CheckBox/RadioButton with live checked
     state, images→Image, scrollers→ScrollView; `role=` overrides.
   - **Names** come from authored `label=`/`alt=`, else a `<text for="…">` linked
     by the existing `link_labels` pass, else (inputs only) the placeholder. A
     button is named by the text inside it.
   - **Geometry** rides the existing `collect` walk, so every exposed node carries
     real on-screen bounds; `r-show="false"` nodes are absent, not just invisible.
   - Republished per frame, but only while an AT is attached (`update_if_active`).

   **Two things only driving it could have caught:** the adapter must be created
   *before* the window is first shown (it panics otherwise, the window is now
   created hidden and revealed after), and accesskit takes a `Role::Label`'s name
   from its **value**, not its label, so every static text was nameless until that
   was special-cased. Verified end-to-end by querying the live UI Automation tree
   from PowerShell: correct control types, names, values, toggle state and bounds,
   updating live when a checkbox is ticked.

   **Not done:** action requests (an AT asking to click/focus) are received but not
   dispatched; the tree is flat under the window (no landmarks/nesting); no live
   regions. `examples/form-controls.rux` gained `id`/`for=` pairs so its controls
   are actually named, the old adjacent-label pattern left them anonymous.

Deliberately **not** in the v0.4 pool: true inline text flow, flagged below as
"a real project, not a patch," too big for a weekly slot.

**Also expected (not yet scheduled):** **external CSS include**: let authors pull
in a stylesheet from a separate file (e.g. `<style src="…">` or an `@import`)
instead of only inline `<style>`. A runtime/parser feature; the syntax-coloring
grammar needs only a small addition to color the include statement when this lands
(see the syntax-coloring design note); this is unrelated to the grammar's
self-contained-vs-`source.css` decision).

---

## Known ceilings (not scheduled: they need a decision first)

- **True inline text flow.** Two `<text>` siblings stack; they cannot share a
  line. taffy has no inline formatting context, so this needs our own line-breaker
  over parley, a real project, not a patch.
- ~~**Error surfacing.**~~ Resolved 2026-07-26: there is a dev overlay. A bad
  `.rux` file shows the error (with line/column) instead of an empty screen, a
  failed hot-reload keeps the last good UI, and dead CSS / failed expressions are
  listed in-window. Remaining: no line numbers on CSS warnings, no `rux check`.
- ~~**`:checked` and other pseudo-classes.**~~ Resolved 2026-07-25: `:hover`,
  `:focus`, `:active` and `:checked` all match. The synthetic `checked` class
  survives one more release for compatibility, then goes.
- ~~**Accessibility.**~~ Resolved 2026-07-26: a real `accesskit` tree is
  published (roles, names, values, checked state, bounds), verified through UI
  Automation. Remaining: action requests aren't dispatched, the tree is flat, and
  there are no live regions.

---

## Further out: the original three (recorded 2026-07-18)

> Superseded as a *sequence* by the version plan below, which folds these
> three in and says where each landed. Kept because the engine notes under
> item 3 are the most detailed record of why the rhai fork exists.

Bigger themes queued after the v0.4 Known-ceilings pool drains. Ordered; each is a
milestone in its own right, not a Friday slice. Recorded 2026-07-18.

1. **TailwindCSS integration.** A utility-class layer over the CSS engine. Only
   sane *after* the "real work" CSS lands in v0.4: Tailwind leans hard on custom
   properties (`var()`), `@media`, and pseudo-classes (`:hover`/`:focus`), so it
   depends on items 1–3 of the v0.4 pool existing first. Open question to settle up
   front: ship a curated utility set generated into Rux CSS, or run an actual
   Tailwind pass over `.rux` files; the former is self-contained, the latter pulls
   in the Node/Tailwind toolchain we've so far avoided.

2. **Element access + manipulation from script.** A DOM-like handle so `<script>`
   can read and mutate the tree directly (query a node, set a property, add/remove
   children) instead of only driving it through signals. This is a large surface
   and interacts with fine-grained reactivity (v0.3): script mutations must feed the
   same subscription graph, or they desync from the declarative tree. Sequence it
   *after* reactivity is solid for exactly that reason.

3. **Script documentation.** Rux's script layer needs its own docs, *beyond*
   upstream rhai's, because the intent is to **modify the rhai version Rux adopts**
   (Rux-specific builtins like `signal(...)`, the element-access API from item 2,
   and whatever language changes the fork carries). Once we diverge from stock rhai,
   pointing users at rhai's docs is no longer correct. This depends on item 2's API
   being settled, so it comes last of the three.

   A concrete fork motivator, established by probing the engine directly: **no
   stock-rhai callable can mutate a top-level signal.** A named `fn` errors (signals
   aren't in a function's scope, since rhai functions are pure/scopeless); a closure's
   `taps = taps + 1` rebinds the captured name rather than writing back; and even a
   pre-shared `Dynamic` cell with `+=` fails to propagate. Only a statement run
   *directly in the top scope* mutates a signal, which is why inline
   `@tap="taps = taps + 1"` works but a reusable handler function does not. So
   JS-style arrow functions that mutate signals, such as `const inc = () => taps++`, cannot
   be real functions over stock rhai. Two paths: inline them as named statement
   templates expanded at the handler call site (no engine change, but they stay
   compile-time macros, not first-class values), or make signals shared cells the
   fork can write through (true first-class functions, but a reactivity-core change).
   The second is the language change this fork would carry.

   **A second fork motivator, found building the dev overlay (2026-07-26):
   stock rhai fails *silently* on a missing map property.** `user.nmae` on an
   object map evaluates to `()` rather than raising an error, so a typo in a
   `{{ }}` binding renders empty and the overlay has nothing to report, the exact
   failure mode the error-surfacing work exists to kill. A missing *function* or
   *variable* does error, and those are reported; the map hole is rhai's semantics,
   not ours, so it cannot be closed from outside the engine.

   Options, in the order they'd be tried: (a) evaluate bindings under a stricter
   engine option if one lands upstream; (b) have the fork raise on unknown-property
   access, at least in a "strict bindings" mode Rux turns on for `{{ }}` and `:`
   expressions; (c) leave it and document it. (c) is what ships today. This is
   *cheaper* than the signal-mutation change, it's a lookup-failure path, not a
   reactivity-core change, so it's a good first divergence to attempt, and a good
   test of how painful carrying a fork actually is. Both motivators point the same
   way: the strict-bindings mode and shared-cell signals are the two changes the
   fork exists to carry.

   > **Superseded in part, 2026-08-11.** The two motivators above still stand and
   > are still why the fork exists, but the second one is now answered by **full
   > lexical scoping** rather than by shared-cell signals, which were the narrower
   > version of the same fix. The settled design, including the crate name, the
   > three buckets of work and the decisions on truthiness, numbers and object
   > literals, is under [v0.7](#v07-the-script-tier-and-the-fork-then-animation-then-packaging).
   > Read this item for *why* the fork exists and that one for *what it does*.

---

## After v0.4: the version plan

Written 2026-07-27, once the v0.4 pool had drained. Each heading below is a
milestone with a theme, drawn from several Friday point-releases, in the order
they unblock each other. The three "Further out" items recorded on 2026-07-18
are folded in, re-sequenced where the reason for moving them is given.

A version ships when its theme is true, not when every bullet is ticked.
Anything that slips moves to the next one rather than holding the release.

---

### v0.5: usable by someone who is not you

Everything so far has been built and judged by the person who wrote it. This
milestone is about a stranger being able to install Rux, get told what is wrong
with their file, and format it the same way we do.

1. **Publish to crates.io** as `ruxlang`, with the `rux-*` crates beneath it.
   The rename already landed on this branch. What is left is per-crate metadata
   (description, keywords, categories, README), checking each crate builds on
   docs.rs, and deciding which of the thirteen are public API and which are
   implementation detail nobody should depend on.

   **Slipped v0.5.0, prepared in v0.5.1.** The 24 bare intra-workspace path
   deps now carry versions, held in one `[workspace.dependencies]` block so a
   release bumps them once. Eleven crates publish; `rux-web` does not, a wasm
   cdylib being nothing anything can depend on, and neither does
   `rux-highlight`, whose grammar handling is not ready to be a dependency.
   Every library's description names `ruxlang` as the supported entry point,
   and `rux-shell` asks docs.rs for the wasm target, since four of its public
   functions live behind `cfg(target_arch = "wasm32")` and a host-only build
   would not mention the web shell at all.

   The first published version is 0.5.1 rather than 0.1.0: five releases are
   already public, and starting the crate line below them would contradict
   them.

   Publishing itself is a staircase, not a batch. `cargo publish --dry-run`
   resolves against the real index, so only the five leaf crates can be
   verified before anything goes up; each layer above is unverifiable until the
   layer beneath it is on the index. Expect the rate limit on new names to stop
   it partway, and resume where it stopped. See `RELEASING.md` section 6.
2. **`rux fmt` as a CLI subcommand** (Tier 1 in the dev-tooling section). This
   also closes a duplication that has already cost us: the re-indenter exists
   twice, in `editors/vscode/extension.js` and in `crates/rux-fmt`, and the two
   drifted within a week. The JS copy missed `<image>` in its void-tag list
   (inherited from HTML, which has `img`), so an `<image src="...">` written
   without a self-closing slash over-indented everything after it. The
   extension should shell out to `rux fmt` and the JS copy should go.

   *Done.* `rux fmt [path...]` formats in place, `--check` changes nothing and
   exits non-zero if anything would, `--indent <n|tab>` sets one level, and `-`
   reads stdin and writes stdout, which is what an editor wants because the
   buffer it needs formatted is usually unsaved. The extension shells out to it
   and the JavaScript copy is gone, along with the `<image>` bug, which is now a
   test.

   **Settled in v0.5.0:** the shipped `examples/` did not match the formatter,
   all 28 of them, roughly half on indent width and half because the CSS
   formatter inlines rules of up to three declarations that the examples wrote
   expanded. The examples were reformatted rather than `INLINE_MAX` relaxed: a
   formatter whose own repository fails its `--check` is not one to hand anyone
   else. `rux fmt --check examples` is now clean and can go into CI.
3. **`rux check`**: *done.* The machine-readable half of the dev overlay, so CI
   and an editor can act on the errors the overlay shows.

   `rux check [path...]` loads through the same path the window does, because a
   checker that disagrees with the runtime is worse than none. It reports
   `path:line:col: severity: message`, or JSON with `--format json`, and exits 0
   clean, 1 on problems, 2 if the request itself was wrong. Warnings do not fail
   a build unless `--deny-warnings` is passed: a document that renders should
   not break someone's build over a property Rux has not got to yet.

   Two things fell out of building it. The warning sinks were mirrored to stderr
   as they were raised, which printed every finding twice, so both sinks now
   have an off switch that any tool formatting them itself can use. And walking
   a directory has to skip *components*: their props come from the parent, so
   checking one standalone reports every prop as undefined. The test is the one
   the spec already sets, a document roots at `<screen>`, which also catches a
   component nothing currently imports. Naming one explicitly still checks it.

   Still missing a position: CSS and expression warnings report a file but no
   line, which is item 5.
4. **The playground catches up.** *Done.* It could report an error message and
   nothing else: no line to jump to, and no warnings at all, while the desktop
   window had both.

   `rux-web` gains `diagnose(source)`, which sets the document and returns
   everything wrong with it as JSON: the error with its line and column, and
   every warning with its line. The page lists them under the editor, and each
   one that knows its line is a button that selects that line. It runs on load
   as well as on Run, because a shared link carries its source in the URL hash,
   so the first thing on screen can already be someone else's broken document.

   `setSource` stays exactly as it was. The deployed page is built from `main`
   while the runtime it loads is pinned to the latest **tag**, so the page has to
   work against a build that predates its own features. It feature-detects
   `diagnose` and falls back; that fallback was tested against a module with the
   export removed, and degrades to the old behaviour with no errors. It can go
   once a deployed build actually carries `diagnose`, which needs the tag *and*
   a site rebuild against it: a tag push does not trigger the deploy by itself.
5. **Locate the warnings.** *Done.*

   A warning is now a `Warning { message, line }` rather than a string, and
   every CSS warning carries the line of the rule it came from, counted in the
   file rather than in the `<style>` block. That needed the parser to record
   which file line each section starts on, since the sections are trimmed before
   any later stage sees them. There is no column.

   It reported the *rule's* line at first, which was wrong the moment a rule was
   written across more than one line, and every real stylesheet is. lightningcss
   records a location for a rule and none for the declarations inside it, so the
   line is now recovered by scanning the source forward from the rule for the
   declaration, stopping at the closing brace so a property that is absent
   borrows no line from the next rule. Selector-level warnings (an unknown
   pseudo-class) still report the rule's line, which is where they belong.

   Two kinds stay unplaced on purpose. **Expression failures**, because the
   template parser does not record where each binding started. And **anything
   from a component's CSS**, because those rules are in another file and a
   warning carries no file: a line from the component's numbering would point
   confidently at the wrong part of whichever document imported it. Giving
   `Warning` a file as well as a line would fix both, and is the natural next
   step if this starts to bite.

   The overlay is dismissed by tapping it, and says so. Dismissal is remembered
   against the diagnostics it was for, so the panel returns as soon as what is
   wrong changes rather than staying hidden until restart.

   **Verified in the window** on 2026-08-07, having been unverifiable when it
   was built (that session had no reachable desktop). The panel paints, a tap
   dismisses it and reveals the document underneath, and introducing a parse
   error into an already-dismissed document brings the panel straight back, red,
   with the error above the warnings. Looking at it is also what caught the
   wrong-line bug above.
6. **A keyboard on a phone.** *Done.* Reported 2026-08-02: tapping a text input
   in the playground focused it inside the runtime, but the browser saw only a
   `<canvas>` with nothing focusable, so the on-screen keyboard never opened.

   Both halves landed. The shell has an IME: it asks for composition with
   `set_ime_allowed`, handles `WindowEvent::Ime`, carries the provisional range
   on `Focus` and underlines it, and restores the field when a composition is
   abandoned. That was the wider fix worth taking, because there had been no
   composition anywhere, so dead keys, accents and CJK had never worked on the
   desktop either. Verified in the window: with the US-International layout, `'`
   then `e` produces `é`.

   On the web the same pathway is fed by a hidden `<input>` laid over the
   focused field, since a browser raises a keyboard for a focused DOM element
   and not for a canvas. Verified under browser touch emulation, driven from the
   tap through typing, backspace, composition and a committed CJK character.
   The one thing emulation cannot demonstrate is a keyboard physically rising,
   so it is still worth half a minute on a real phone, and it only reaches
   ruxlang.dev once a tag carrying it has been deployed.

   **Confirmed on hardware 2026-08-08**, along with everything in v0.5.1 below.

#### v0.5.1: text input that works on a phone

Not planned. It came out of putting v0.5.0 in front of a real phone, which
found four things in a row, each one uncovered by fixing the one before it.

1. **A diagnostic pointed at a line that does not exist.** rhai appends its own
   `(line 1, position N)` to every error, measured inside the expression rather
   than the file, so it was always line 1. Stripped.
2. **Touch used the mouse's gestures.** A finger drag selected. A phone expects
   drag to move the caret, long press to take the word, and long press then drag
   to extend. Touch now has its own state machine.
3. **A single-line input never scrolled to its caret**, so the caret left the
   box and was clipped. Every platform, every means of moving it. This one had
   nothing to do with touch; the caret drag just made it visible.
4. **A long press picked the word one scroll-distance behind the finger**,
   because the caret path had been given the new offset and the word path had
   not. Both now go through one conversion.

Then the piece all of that was for: **a selection toolbar** (Copy, Cut, Paste,
Select all) over `navigator.clipboard`, because a browser gave Rux no clipboard
at all and a phone has no Ctrl+C. It serves desktop browsers too.

The thread running through the four is the same one this project keeps pulling
on: each was invisible to the suite, and three of them were invisible to a
desktop as well. The last is the more specific lesson, and the reason two of
these commits end with a refactor rather than a fix: **two places doing the same
coordinate arithmetic will eventually disagree**, so the fix is one conversion,
not two correct ones.

### v0.6: apps bigger than one screen

Everything Rux can express today fits in one file and one screen. This is the
set of things you hit the moment that stops being true.

1. **External CSS include** (`<style src="...">` or an `@import`), already
   listed as expected but never scheduled. A shared palette currently has to be
   pasted into every file.
2. **Component slots and events.** Props go in; nothing comes out. A component
   cannot render caller-supplied children, and cannot tell its caller that
   something happened, so every piece of state gets hoisted to the root. This
   is the single biggest ceiling on component reuse and it is not on any list
   today.
3. **A router.** `docs/02-spec.md` already promises `role="link"` with
   `to="/path"` "handled by the router". There is no router. Either build one or
   strike the promise.
4. **Keyed `r-for`.** Reconciliation is by count, so reordering a list rebuilds
   more rows than it needs to.
5. **Computed values and effects.** A `{{ }}` expression is the only "computed"
   there is, and there is no way to run a side effect when a signal changes.
**`rux build` was scheduled here on 2026-08-09 and moved out again on
2026-08-11**, to sit after the script tier rather than before it. See v0.7 below.
The reason is the one this document already recorded when packaging sat at v1.0:
the language should stop moving before its output format is committed to. What
changed is the judgement of how long "stop moving" takes, not the argument.

### v0.7: the script tier and the fork, then animation, then packaging

The two fork motivators recorded above are both real and both point the same
way. This milestone is where Rux stops being stock rhai.

1. **Strict bindings** (motivator b), shipped together with `?.` and `??`.
   **Not a fork change at all**, established 2026-08-11 by reading rhai 1.25.1's
   source; see "What upstream already has" below. A missing map property
   evaluates to `()` in silence, which is exactly the failure the dev-overlay
   work exists to kill. Still first, because it is now nearly free rather than
   because it is a cheap divergence. The optional-chaining operators are not a
   separate nicety here,
   they are the escape hatch strict bindings creates the need for: without them,
   every legitimately absent property, from data still loading to an optional
   field, turns a silent-wrong result into a noisy-wrong one with no way to say
   "absent is fine". They ship in the same release as the strictness that
   requires them.
2. **Full lexical scoping: done.** A function sees the scope it was written in
   and can read and write it, so `fn bump() { n++ }` is a working handler and
   `@tap="bump()"` moves the screen. This retires the single biggest trap in the
   language: `/learn` spends a callout on it, `docs/05-as-built.md` calls it
   "the single biggest trap", and every handler in every example is inline
   because of it. Settled 2026-08-11 in favour of real scoping over the narrower
   "signals are shared cells the fork writes through".

   rhai already had the mechanism, as an opt-in `f!(…)` call form that runs in
   the caller's scope; the fork makes that what a call *means*. **Two lines**,
   which is worth saying plainly because it is wildly out of proportion to the
   size of the language change, and is the strongest argument that vendoring the
   whole engine was the right call: the change was cheap precisely because it
   could be made in the one place that already knew about scopes.

   One consequence: **a method call does not capture the caller's scope**, and
   the flag is cleared for those rather than raising, or `colors.len()` would
   become illegal. That limitation is upstream's and stands, since method
   dispatch passes its receiver by reference and the scope cannot also be
   borrowed. A function that needs the surrounding state is written as a plain
   call, which does capture.

   `05-as-built.md` is updated, since it describes the tree as built. **`/learn`
   is deliberately not**, and must not be until v0.7 ships: the standing rule is
   that it documents the latest *release*, never the tip, and it currently
   carries a whole section called "The rule that catches everyone" explaining
   this exact limitation to people running v0.6, for whom it is still true.
   Rewriting that section is **part of the v0.7 release work**, along with
   `examples/learn/03-state.rux` and the assertions in
   `crates/rux-runtime/tests/learn.rs` that hold it honest.

   Worth noting while it is fresh: this also removes the reason `docs/03-guide.md`
   was declared unpublishable. Its central example was a `fn` mutating state,
   which is why the guide was abandoned and `/learn` hand-written against the
   real runtime instead. That is not an argument for reviving it, but the
   blocker recorded against it is no longer the blocker.
3. **Element access from script.** Designed 2026-08-11; the section "Element
   access: `query` and the handle" below is canonical. Sequenced after
   reactivity for the reason already recorded, and that sequencing settled the
   design rather than merely ordering it: **tree mutation is cut**, and what
   ships is querying, reading, and a small set of actions that are not tree
   edits.
4. **Script documentation.** Done 2026-08-12: `docs/07-script.md`, published at
   `/reference/script/`. Pointing at rhai's own docs stopped being correct the
   moment the fork changed what things mean, so this covers the whole surface
   and closes with the two lists that matter, what differs from rhai and what
   differs from JavaScript.

   Writing it found a bug, which is the argument for writing this kind of
   document by checking rather than by recall: rhai's string methods mutate in
   place, so `trim` **emptied** its receiver and returned nothing, and
   `{{ name.trim() }}` rendered blank. Every other string method in Rux returns
   a value, as JavaScript's do. Now shadowed by one that does too.
5. **Animation**, added 2026-08-11. Rux has no transition of any kind, which is
   the most visible thing missing from a UI toolkit that is otherwise usable.
   Sequenced *after* the fork rather than mixed into it, and in tiers, because
   they differ enormously in cost:
   - **`transition` on style changes** first and alone. **Built.** It fires
     when a node's computed style changes between builds, from a signal or from
     a pseudo-class flipping, so the tree shape never changes and enter/leave
     never arises. This is the case almost every app wants. The clock it needed
     mostly existed: the shell already scheduled the caret blink and the
     long-press timer on `ControlFlow::WaitUntil`, and the animator became a
     third deadline on the same scheduler, which keeps the property that matters
     on a phone: an idle app sleeps instead of burning frames, and frames stop
     the moment a transition lands. See
     [Transitions](./05-as-built.md#honored-css) for the surface, and
     `examples/transition.rux`.
   - **Enter and leave** second, and it shares a foundation with lifecycle
     hooks: both need a subtree to outlive its removal from the tree. **Built**
     2026-08-18, see item 6.
   - **Route transitions last**, never first. They *are* the enter/leave
     problem, so building them for the router alone means writing a bespoke
     animator and throwing it away. **Built** 2026-08-18, last as planned, and
     they cost one identity decision rather than an animator.
6. **Lifecycle hooks**, `mounted` and `unmounted`. **Document and component
   level are both done** (2026-08-17), along with `setInterval` and with
   `computed` / `effect` inside a component. **Enter/leave landed 2026-08-18**;
   route transitions are what is left.

   The premise this was planned under has expired and the decision was re-made
   rather than inherited. Hooks were to be inline blocks *because* a named `fn`
   could not mutate a signal; full lexical scoping removed that constraint, so
   blocks were re-chosen on their merits: a hook is not something the author
   calls, and a callable name would invite exactly that.

   They run where the effects run, after the build, so their writes feed back
   through `apply_change_depth` and inherit its loop guard rather than needing
   one of their own. `mounted` runs after the effects and exactly once, which is
   the only thing separating it from an `effect` that fires on load.

   **What was built for the component level**, since the shape is worth
   recording: the build reports each instance that appeared or was pruned
   through a lifecycle sink, and the runtime runs the bodies afterwards, never
   during a build. `unmounted` never fires unless `mounted` fired, so an
   instance created by one build and dropped by the next runs neither; and when
   one build swaps two components, the leaver's `unmounted` runs before the
   arriver's `mounted`.

   **Enter/leave: BUILT 2026-08-18**, as the **builder-owned live pair**
   decided the same day. While a swap is pending the build emits *both*
   branches: two real, laid-out, still-updating trees rather than a snapshot
   held by the animator, because a swipe is reversible and a corpse cannot be
   resurrected. Under builder ownership both instances stay reached by the
   build, so pruning kept working untouched and `unmounted` fires at commit
   rather than at removal, which is the behaviour a cancelled swipe needs.

   **The authoring surface, decided 2026-08-18 by the user** after weighing a
   Vue-style `<transition>` wrapper and a script-driven pair against it:
   `r-transition` marks the element, and the two sides are **CSS**, on the new
   `:enter-from` and `:leave-to` pseudo-classes. Nothing new to learn if you
   know how the rest of an element's appearance is written, no non-rendering
   wrapper element, and no convention-based class names. It fits the grain:
   `:current` was already a build-time pseudo-class, so there was a precedent
   for resolving one from state rather than from the shell.

   **The payoff of that choice, which was not the reason for it:** tier 1
   needed almost no change. `:enter-from` is worn for one build and dropped by
   the next, and that drop is an ordinary style change, which is exactly what
   `transition` already animates. Enter/leave turned out to be *build* work
   rather than animator work.

   **What it cost, as predicted:** the build path and the identity bookkeeping.
   `Swaps` has to remember what the **last** build showed, since a swap opens on
   the difference between that and what this build asks for and the tree cannot
   report the first of those. Keyed `r-for` rows needed more: a departing row's
   item is already gone from the collection, so the row's locals and its
   position are remembered too, and a row leaving from the middle of a list is
   spliced back in where it sat rather than appended.

   **Progress can be driven, not just timed**, which is the half the live pair
   was really chosen for: `:r-transition="expr"` hands progress to the author,
   0 to 1, so a `@drag` can drive a swap and change its mind. Yielding `null`
   hands it back to the clock, which is how a released finger settles rather
   than snapping. Driven in `examples/enter-leave.rux`.

   **Route transitions: BUILT 2026-08-18**, on top of it and not beside it,
   which is the sequencing rule this milestone kept insisting on. The router
   contributes the identity (**which route matched**, not which path, so a
   param-only navigation updates in place) and nothing else; the tests passed
   first time. That is the return on not having built them for the router
   alone.

   **It found a bug with nothing to do with animation.** A route view ran no
   lifecycle at all: no `mounted`, no `unmounted`, no `computed`, no `effect`.
   The `<route>` element stands in for the component tag when a view is
   expanded, so every route view was filed under the tag `route`, and
   everything an instance looks up afterwards is keyed by that tag. Nothing had
   noticed because nothing had asked. Leaving a route also dropped its
   instances without queueing their `unmounted`, bypassing the lifecycle sink.
   Both fixed.
7. **`rux build`, for web and Windows only.** Moved out of v0.6 on 2026-08-11
   to sit here, at the end, after the language has stopped moving. Two targets
   and no more: a static web bundle, which is close to what the playground
   already produces, and a Windows executable, which is the platform Rux is
   actually developed and tested on. **No `.msi`, no `.app`, no `.apk`.**
   Mobile packaging waits for v0.8, where mobile itself lives.

The ordering within this milestone is the point: the fork changes what a script
can do, animation and hooks both build on that, and packaging commits to an
output format only once nothing above it is still moving.

#### The fork: `rux-rhai`

Designed 2026-08-11. Everything in this section is decided, not proposed.

The fork is published to crates.io as **`rux-rhai`**, under rhai's own
MIT-or-Apache terms with attribution. It cannot be a vendored directory: every
Rux crate publishes, and `cargo publish` rejects a path dependency without a
version, so the fork has to be a real crate with a real version like the other
eleven. The name is the one that communicates what it is at a glance in a
dependency list.

The organising question is not "what should the language have", it is **what the
fork must carry**. Anything achievable from outside the engine is free forever;
anything inside it is a rebase cost at every upstream rhai release, for as long
as Rux exists. So the work splits three ways, and the split governs the order.

**Bucket 1, no fork needed.** Registration and configuration in `rux-script`.
All of it is a pass, and it should land first, because it moves the
JS-familiarity needle further than anything below it and costs no divergence.

- JS method names on arrays and strings: `forEach`, `find`, `includes`,
  `indexOf`, `slice`, `join`, `startsWith`, `endsWith`, `trim`, and the case
  conversions. `map`, `filter` and `reduce` already match.
- `length` as a property getter, rather than `len()`. Arrays and strings only:
  JS has no `length` on a plain object, and adding one to maps would be inventing
  a rule rather than matching a known one, which is the thing this whole exercise
  is trying not to do.
- `join` and `forEach`, which rhai does not have under any name. `map` and
  `filter` build a new array and a loop is a statement, so there was no way to
  run a side effect per item as an expression. `forEach` is called with
  `(item, index)` like JS and falls back to `(item)`, because rhai rejects a
  closure handed more arguments than it declares and `|x| …` is the form nearly
  everyone writes.
- **`.length` returns an integer**, which is the opposite of the rule everywhere
  else and is deliberate until numbers are unified. `items[items.length - 1]`
  and `for i in 0..items.length` are the two commonest uses of a length and both
  need an integer, since rhai indexes and builds ranges with `INT`. A `length`
  that reads correctly in a binding and fails the moment it is used to index
  would be worse than not having it. **The all-f64 change below has to teach
  indexing and ranges to coerce at the same time**, or it will break this.
  Found by an example rather than by a test: `keyed-list.rux` was rewritten to
  use `.length` and stopped rotating.
- **Numeric arguments taken as `Dynamic` and coerced at every boundary.** A
  literal `1` is still an rhai integer while anything through `signal()` is a
  float, so `items.slice(1)` and `items.slice(start)` would otherwise resolve to
  different overloads and one of them would not exist. This is the
  numbers-are-two-types problem biting in the first five minutes of use, and it
  is the strongest practical argument for the all-f64 change below: coercing at
  each boundary is the version of that fix available without a fork, and it has
  to be remembered at every single registration.
- `null` as an alias for `()`. Not a scope binding: `null` is a *reserved
  keyword* in rhai, so it never reaches variable resolution. Registered as custom
  syntax, which is the one hook that sees it, and which has the side benefit that
  `null` cannot be shadowed by a `let` and never enters the signal set.
- **Printf-debugging**, which the script tier had no way to do at all. For a JS
  developer this is the most reflexive tool they own, and its absence is a larger
  practical problem than any syntax difference on this page.

  Spelled **`print(…)` and `debug(…)`**, rhai's own names, wired to a Rux sink.
  Deliberately **not `log(…)`**, which is what a JS developer would reach for:
  rhai's arithmetic package already defines `log` as the logarithm, and its more
  specific `f64` overload beats a registered `Dynamic` one, so `log(2)` quietly
  computes `0.301` and reports nothing. That is the worst available outcome and
  precisely the class of silent failure the rest of this milestone exists to
  remove. `console.log` is not offered either, since there is no `console` object
  and inventing one to hold a single function would misrepresent what else is
  there.

  The sink is separate from the warning sink. A warning is something wrong with
  the document and a print is the author talking to themselves; merging them
  would fill the overlay's problem list with output that is working as intended,
  and `rux check` would start failing on it. Prints are not deduplicated, unlike
  warnings: the same line ten times is the information.

  It reaches **the dev overlay**, on `Diagnostics::prints`, for the same reason
  the warnings do: nobody running a GUI app is watching stderr, so debugging
  output that only went there would not be debugging. The panel takes a slate
  blue rather than the warnings' amber when nothing is actually wrong, since
  amber for a healthy document teaches the eye to ignore amber. `Diagnostics`
  grew a `has_problems()` alongside `is_empty()` to keep the two questions apart:
  the overlay asks "is there anything to show", `rux check` and CI ask "is
  anything wrong", and a leftover `print` must answer yes to the first and no to
  the second. Collection is deliberately order-independent, because a tap reaches
  the runtime through either a patch or a rebuild depending on whether the tree's
  shape changed, and printf-debugging that only worked on one of those paths
  would be worse than none.
- **Diagnostics reskinned into Rux's vocabulary.** The script tier is an
  implementation detail; someone writing a `.rux` file has been told they are
  writing Rux, and "Property not found: nmae" is rhai talking to a rhai user
  about a rhai object map. Each rewrite also says what to do next, since these
  are failures with exactly one sensible fix, and the escape-hatch advice on a
  missing property is not guessable given `?.` does not provide one. The four
  translated are missing property, undefined variable, missing function and
  reserved word; anything unrecognised passes through untouched, because a
  slightly foreign message beats a confidently wrong one and the list will never
  be complete.

  Spans into the `.rux` source are **not** part of this and remain unbuilt. Every
  binding is compiled as its own small script, so rhai's line is always 1 and its
  position counts characters inside the expression; the existing code already
  strips that suffix rather than printing a location that is not one. Giving a
  real file position means the template parser recording where each `{{ }}` and
  attribute began, which is parser work, not script work.

**What upstream already has.** Checked against rhai 1.25.1's source on
2026-08-11, before writing any of it, because the whole point of sequencing the
cheap divergence first was to measure fork pain and it would be absurd to pay for
a divergence upstream already supports. Three items came back free:

- **Strict map properties already exist as an engine option.**
  `Engine::set_fail_on_invalid_map_property(true)` (`src/api/options.rs`) is
  enforced in `src/eval/chaining.rs` and raises `ErrorPropertyNotFound`, which is
  exactly the semantics wanted. Option (a) of the three recorded under Further
  out item 3, "evaluate bindings under a stricter engine option if one lands
  upstream", is the one that applies. It had landed and nobody had looked.
- **`??` works.** `?.` exists as a token but **did not do what item 1 needs**,
  which only came out when it was run rather than read: rhai's `?.` guards a
  *base that is absent*, so `missing?.anything` is fine, but on a map that does
  exist it raised for a missing property exactly as `.` does. So strict bindings
  had no JS-shaped escape hatch upstream, and a property-guarding `?.` was fork
  work after all. **This became the fork's first change**; see below.
- **`===` and `!==` are reserved tokens**, and `Engine::register_custom_operator`
  explicitly accepts `Token::Reserved`, so both are a registration rather than a
  parser change.

So **item 1's strictness is bucket 1**, and it shipped that way. The consequence
is that the milestone lost its warm-up: item 1 was chosen to go first partly as a
cheap gauge of how painful carrying a fork would be, and the strictness half is
not a divergence at all. The gauge moves to the sugar below, which is mechanical
parser work and a better measurement anyway, and which should therefore be the
fork's first commit rather than an afterthought bundled behind scoping.

**The fork now exists**, as `crates/rux-rhai`, and this was its first change.
`?.` guards a missing property, not only an absent base, so `params?.id` is how
a document says that absent is a legitimate answer; `examples/router.rux` uses
it. Without it, strictness had no opt-out at all, since the only alternative was
`"id" in params`, which works but is not what a JS developer reaches for and
cannot be written inline in the middle of a chain.

Three things about how the fork is kept, all of them deliberate:

- **Vendored, not patched.** The whole of rhai 1.25.1 is in the tree.
  `crates/rux-rhai/DIVERGENCE.md` is the complete list of what was changed, with
  the upstream files, so a rebase is working through one list rather than reading
  a diff of forty thousand lines. It also records what was considered and *not*
  changed, so nobody pays twice to rediscover that something needed no fork.
- **The library target is still called `rhai`** even though the package is
  `rux-rhai`. Upstream's several hundred doc-tests are written `use rhai::…` and
  compile as external crates against the *target* name, so renaming it would have
  silently dropped the test coverage a fork most needs to keep. Every `use
  rhai::…` in `rux-script` is untouched too, which is what made the swap a
  one-line change to the workspace manifest.
- **The fork was proved inert before it diverged.** Vendored, wired in, full
  suite green including rhai's own tests, and only then changed. A fork that is
  built and modified in one step cannot tell a porting mistake from a deliberate
  change.

This also turned up a hole in `scripts/release.sh`, now fixed: it found
intra-workspace dependencies by *name* (`^rux-[a-z]+`), so the aliased
`rhai = { package = "rux-rhai", … }` line was invisible to both the version gate
and the release bump, and would have shipped still saying `-dev`. Both now match
on `path = "crates/`, which is the honest invariant.

**A caution this turned up, unrelated to any of the above.** rhai's default
optimizer removes a call whose result is unused when it believes the call is
pure, and it cannot know that a host-registered function is not. Nearly every Rux
builtin is called for effect and discarded: `print(x)`, `emit("change")`,
`navigate("/")`. It was found by `print(1); print(1)` producing one line instead
of two, which is a harmless symptom of a rule that could as easily have eaten a
navigation. The optimizer is now off. There is nothing for it to win on
expressions the size of a single binding.

**Bucket 2, parser sugar, and now the fork's first actual divergence.** New
tokens desugaring to existing AST, no semantic change, low rebase risk:

- **`++` and `--`: done.** Reserved upstream but not implemented, and rhai's
  custom operators are binary only, so these could not be registered from
  outside. Desugared to `+=` and `-=` at parse time, so they inherit lvalue
  checking, operator overloads and write tracking rather than introducing
  anything new at evaluation. Statement position only, on purpose: JS's `x++`
  *expression* evaluates to the value before the increment, which is the rule
  behind the `i = i++` puzzle, and rhai's assignment is a statement anyway, so
  supporting it would mean inventing JS's confusing half rather than matching
  something that exists.
- **`=>` for closures: done, and it was not sugar.** All four shapes work:
  `x => …`, `() => …`, `(x) => …` and `(a, b) => …`. Each is exactly the
  matching `|…|` form, so nothing new reaches evaluation.

  It needed a change to the parser's input path, which is why it was worth
  more than the label "sugar" suggested. rhai's `TokenStream` was a `Peekable`,
  giving one token of lookahead, and one is not enough to tell `(a, b) => …`
  from `(a + b)`: they are identical until several tokens in, and a `Peekable`
  cannot put back what it read. `TokenStream` is now a small buffered struct
  whose `next` and `peek` keep their exact signatures, so the ninety-odd call
  sites in the parser did not change; `peek_nth` is the only addition. Tokens
  are pulled on demand and never eagerly, because the tokenizer is stateful and
  reading ahead of what was asked for could tokenize under settings the parser
  has not applied yet.

  The lookahead consumes nothing, so an ordinary parenthesised expression falls
  through untouched, which is the property that made the change safe to make at
  all. There is a test asserting exactly that, because a lookahead that guessed
  wrong would quietly break arithmetic rather than failing loudly.
- `===` and `!==` are **not** in this bucket after all; see above. Loose equality
  with coercion is still not adopted, both spellings mean the strict comparison.

**Object literals stay `#{ }`.** Decided against accepting bare `{ }` in
expression position, even though it is what a JS developer types. One spelling
learned once and for all beats two spellings and a rule about which contexts
accept which, and it is the highest-ambiguity change on the list for a purely
cosmetic win. The consequence is that the docs teach `#{ }` early, prominently,
and on its own, rather than letting it first appear in passing.

**Bucket 3, semantic divergence.** The permanent cost, and the reason the fork
exists.

- **Strict property lookup**, item 1 above.
- **Full lexical scoping**, item 2 above. Chosen over the narrower shared-cell
  approach, which would have punched a signal-shaped hole through rhai's
  scopeless-function wall rather than taking the wall down. The hole is cheaper
  and was the wrong trade: it solves exactly one motion, a handler bumping one
  signal, and leaves a helper that reads a top-level `let` or calls another
  top-level `fn` still broken, which is most of what "reusable handler" means in
  practice. It also leaves the language with an exception that gets *harder* to
  state as it gets smaller, where real scoping is one sentence a JS developer
  already knows. It keeps `=>` closures and named `fn`s as one construct with
  one capture rule instead of two. And it is load-bearing for three things
  already scheduled: navigation guards, lifecycle hooks, and element access,
  each of which would otherwise need its own carve-out. Shared cells do not
  disappear entirely, but their job narrows: scoping delivers the *write*, and
  the cell remains the mechanism by which the reactivity graph *notices* it.
- **JS truthiness, matched exactly: done.** `false`, `0`, `NaN`, `""`, `null`
  and `()` are falsy and everything else is truthy, including empty arrays and
  empty maps. Stock rhai requires a real `bool` in a condition, which breaks
  `r-if="user"` and `r-if="items.length"`, the two most natural conditionals
  someone will write, in the position where they are least likely to be thinking
  about a scripting language's type rules. JS's rules are adopted wholesale
  rather than a Rux-specific set, because a half-familiar rule is worse than
  either alternative.

  **This is a behaviour change in existing documents, and the only one in v0.7.**
  `r-if="items"` used to mean "there are items" and now means "items exists".
  `r-if="items.length"` is how to ask the first question. Nothing in the repo
  relied on it, and it is pinned by a test whose comment says why, but it belongs
  in the release notes rather than in a changelog line.
- **All numbers are f64: not done, and on the evidence it should not be.** It
  was agreed on the premise "if matching JS truthiness exactly requires it".
  Building truthiness showed the premise is false: `Dynamic::is_truthy` treats
  `Union::Int(0)` and `Union::Float(0.0)` by the same rule, and `from_dynamic`
  normalises both to `f64` before a binding ever sees them, so **truthiness
  already matches JS exactly with two numeric types in the engine**.

  What remained was integer division, and the seam it appears at is narrower
  than it looked: `signal()` coerces to `f64`, so every number a document holds
  is already a float and already divides like one. Only two bare literals
  divided by each other could give `3` for `10 / 3`. That is fixed by
  **registering `/` for two integers**, which is not a fork change at all, and
  costs one line instead of diverging from every upstream doc-test that asserts
  an integer result (16 of them use `eval::<i64>` directly, and each would have
  to be rewritten, which is upstream churn carrying no divergence).

  The two consequences that *did* need doing were done: **indexing coerces a
  whole `f64`** (this was a live bug, see below), and division by zero now yields
  `Infinity` / `NaN` as in JS, spelled the way JS spells them rather than Rust's
  `inf`. **Bitwise operators are left alone**, since nothing in a UI has wanted
  one; if that changes, JS's coerce-to-int32 rule is the shape to follow.

  Recorded as a decision rather than a deferral: there is no known behaviour a
  Rux document can observe that all-f64 would change, and the cost is a permanent
  divergence across upstream's test suite, which is the coverage a fork most
  needs to keep.

**Sequence.** Bucket 1 entirely, including diagnostics, before any divergence.
Then item 1 with `?.` and `??`. Then item 2 with `++`, `--` and `=>`, so the
syntax that makes the new capability feel familiar arrives with the capability.
Then truthiness and all-f64 batched into one release, so there is a single
"semantics changed" note rather than two. Then items 3 through 7 as listed.

**Explicitly not in the fork: `setTimeout` and `setInterval`.** They are one
family and raise identical questions about what scope a deferred callback runs
in and what happens when its component is gone, but they are declined for
different reasons and with different futures.

`setTimeout` is **declined outright**. Nearly every real use of it in UI code is
"after the transition", debouncing, or auto-dismissal, and the animation tier
above is the honest answer to all three. The docs should say so rather than
leaving it as an unexplained absence.

`setInterval` is **deferred, not declined**, because the use case behind it,
clocks, countdowns, polling and progress ticks, has no declarative substitute
anywhere on this page. Two things have to be true before it can land. It has to
come after `unmounted` exists, because there is nowhere to write a cancellation
today and a leaked repeating timer stacks another copy on every hot reload,
where a leaked one-shot fires once and is done. And it must not be spelled
`setInterval` or hand back a handle somebody has to remember to clear. A
one-shot composes with the shell's `ControlFlow::WaitUntil` scheduling perfectly,
since it books exactly one wake and then the app sleeps again; a free-floating
repeating timer is a one-line declaration that this app is never idle again,
which nobody notices on desktop and which is the entire battery story on the
phones v0.8 targets. The shape that fits is a repeating timer the runtime owns
and ties to the component instance, cancelled automatically when that instance
is pruned. v0.6's instance pruning is already that foundation, the same one
`unmounted` and enter/leave need, which puts this alongside item 6 rather than
before it.

#### Element access: `query` and the handle

Designed 2026-08-11, against the code rather than against the older sketch.
Everything in this section is decided.

**The feature this item promised cannot be built, and that is the finding, not a
setback.** The 2026 sketch above asks for "a DOM-like handle so `<script>` can
read and mutate the tree directly (query a node, set a property, add/remove
children)". That was written before fine-grained reactivity existed. Today a
state change regenerates the affected tree from the template: a patch rewrites
bound nodes in place, a reconcile splices lists, and a rebuild throws the tree
away entirely. A node a script had mutated is overwritten by whichever of those
runs next, with no error and no warning, and *when* it is overwritten depends on
which signal some unrelated handler happened to write. That is not a hard
problem to solve carefully. It is a feature that would appear to work in a demo
and fail in a real document, which is the failure mode strict bindings and the
dev overlay exist to eliminate.

So mutation is cut, and the rule that replaces it is one the framework already
teaches everywhere else: **the tree is a function of state, and state is how you
change it.** Nothing is lost that signals cannot express. What signals genuinely
cannot express is the other half, and that is what ships.

**What ships is two things that are not tree edits.**

*Reads*, of facts the tree knows and a template cannot state: measured geometry
(position, size), scroll offset, and whether a node currently holds focus. These
are properties of a laid-out frame, so they are **one frame stale**, exactly as
`getBoundingClientRect` is in a browser. A handler runs before the next layout,
and it reads the numbers from the last one. That is the honest guarantee and it
should be documented as such rather than papered over.

*Actions*, which change host state rather than the tree: `focus()`, `blur()`,
`scrollIntoView()`. These are the fourth instance of an idiom the script tier
already uses three times over. `emit`, `navigate` and `back`/`forward` all record
an intent onto a queue the runtime drains, precisely because a script function
cannot reach runtime state and should not pretend to. Element actions are queued
the same way, and are applied at the same point in the frame as a navigation.

**Reads are legal in handlers only**, not in a `{{ }}` binding, a `:style` or an
`r-if`. This is a hard restriction and it exists because the alternative has no
fixed point: a binding that reads geometry has to invalidate when layout changes,
invalidating it triggers a rebuild, and the rebuild relayouts. The loop is not
hypothetical, it is what a subscription graph does when a node's output is also
its input. Forbidding it by construction costs almost nothing, since the uses
people actually have (measure on tap, scroll a thing into view, position a
popover after opening it) are all handler-shaped. Using a read outside a handler
is a diagnostic, not a silent absence.

**Elements are named with CSS selectors**, `query(".card")` returning a list and
matching on tag, id, class, role and combinators. Chosen 2026-08-11 over a
narrower `el("id")` form. The concern about a selector engine being a large
surface to build and keep stable turned out not to apply here: **Rux already has
one**, and it is the same one the stylesheet uses. `parse_selector` handles
compounds, specificity, and the `>`, `+` and `~` combinators; `matches_chain`
matches right-to-left with backtracking over an ancestor chain and preceding
siblings. A second, weaker way to name an element would have been the thing that
needed justifying, because a document would then have two spellings for one idea
and a rule about which contexts take which.

**Built 2026-08-11 and 2026-08-12: all of it.** `query(selector)` works from a
handler and returns handles carrying `tag`, `id`, `classes` and the geometry of
the frame on screen; `focus()`, `blur()` and `scrollIntoView()` are queued as
intents. The element index is retained from the build and the stylesheet's
matcher runs over it unchanged. The handler-only rule is enforced by the
resolver simply not being installed outside a handler, so it needs no separate
check to remember. `examples/element-query.rux` demonstrates it and is driven by
a test rather than only loaded by one.

Two things worth keeping, both found by building it:

- **Paths reach the layout through the taffy-tree walker, not through
  `collect`.** `collect` walks taffy ids, so using a child's index there assumes
  taffy children line up with `LayoutNode` children, and a mismatch would hand
  back confident, wrong geometry. The walker that builds the tree recurses over
  `node.children` directly, so its indices are the real ones. Setting
  `state_path` on every node was also considered and rejected: it would change
  hover hit-testing as a side effect of an unrelated feature.
- **Nothing compiled an event handler until it was tapped.** A syntax error in a
  `@tap` reached the window as a button that looked right and did nothing, and
  `rux check` missed it too, because checking reuses the loader and the loader
  never looked. Handlers are now compiled at load. Found by shipping the bug:
  `query('#note')` sat in an example and checked clean, because `'x'` is a
  character in a script and not a string.

**The one real cost is that the built tree has forgotten what it is.**
`ElemDesc`, the `{ tag, id, classes, role, states }` a selector matches against,
is computed during the style pass and discarded once the cascade has run.
`LayoutNode` keeps `id` and `key`, because `for=` and reconciliation need them,
and keeps neither `tag` nor `classes` nor `role`. So the work is not the matcher
and not the parser, it is **retaining an element index from the build**: per
built node, its descriptor and its tree path, enough to run the existing matcher
without rebuilding the ancestor context by hand. Built once per build, thrown
away with the tree, and paid for only by documents that call `query`.

Two consequences of that follow and are worth stating before anyone rediscovers
them. Geometry lives in the shell, not the document: `layout_scrolled` is called
by `rux-shell` and the resulting `Layout` never comes back, so reads need last
frame's metrics handed in each frame the way `InteractionState` and `Viewport`
already are. And **`rux check` has no layout at all**, which is the point of it
running in CI without a window or a GPU. A geometry read there has no answer, so
checking a document must not require one: `query` resolves, the handle exists,
and the metrics are absent.

### v0.8: mobile

The README has said "desktop first, then mobile and embedded" since the
beginning, and there is not one line of mobile code in the repo. Four releases
have all been desktop. This milestone either makes the claim true or it comes
out of the pitch.

**The point of it is the developer experience, not only the target** (decided
2026-08-18). Building for a phone must not drag a Rux user through Android
Studio and Gradle; if a toolchain is needed, that is Rux's problem to hide
rather than theirs to learn. Compile time and machine load count as part of
whether this milestone worked, at development time and at build time alike. How
is deliberately left open until the milestone is reached rather than guessed
here.

1. **Android and iOS**, through winit and wgpu, which both support them. The
   wasm work in v0.5 is a useful rehearsal: it already forced the shell to stop
   assuming a filesystem, a blocking main thread and a system clipboard.
2. **Touch for real.** Kinetic scrolling is unimplemented and touch drag is
   marked untested because there is no touch hardware here.
3. **Native pickers** for `<input type="select">`, promised in the spec and
   currently a drawn dropdown.
4. **Safe areas, orientation, density.** `@media (orientation)` already exists
   from v0.4, which helps.
5. **Splash screens.** Raised 2026-08-18 and undesigned; a phone app that shows
   nothing while it starts looks broken, and this is the milestone that finds
   out what Rux should do about it.
6. **`rux.toml`**, with `rux build`. A manifest is genuinely needed rather than
   optional, and it lands when packaging does: a window title, an icon and a
   target have nowhere to live until there is an artifact to put them in, and
   inventing the file first would commit the output format by accident.
7. **Multi-touch, on hardware.** The gesture vocabulary itself landed in v0.7
   (`@press`, `@release`, `@longpress`, `@swipe`, `@drag`), and every event
   already reports a *list* of touch points rather than one synthesised pointer.
   What v0.8 owes it is a second finger: the shell tracks each id winit gives
   it, but no desktop here can produce two, and nothing yet interprets a pinch
   or a rotate. The axis-claim rule is half-settled for the same reason: a
   `@drag` claims the finger, and whether a scroll can take it back mid-gesture
   waits for a real screen.

### v0.9: text, and the long tail

1. **True inline text flow**, the standing known ceiling. Two `<text>` siblings
   cannot share a line, so bold inside a sentence is impossible. taffy has no
   inline formatting context, so this is our own line-breaker over parley: a
   real project, and the reason it has never fitted in a weekly slot.
2. **Text editing gaps**: word-wise movement, triple-click line select,
   drag-and-drop of a selection, `::selection` styling.
3. **Scrolling gaps**: click-on-track paging, scrollbar hover and fade,
   independent `overflow-x` / `overflow-y`, `overscroll-behavior`.
4. **CSS long tail**: per-side border colours, `position: sticky` and `fixed`.

### v1.0: freeze

1. **`rux build`, the rest of it.** The web bundle and the Windows executable
   moved to v0.7; what stays here is what genuinely needs a language that has
   stopped moving: installer formats, an app bundle, and a platform matrix
   wider than the one machine Rux is developed on.
2. **Re-derive the spec.** `docs/02-spec.md` describes itself as the v0.1
   design surface, not the built surface, and is published only as history.
   1.0 means the spec and the runtime agree again.
3. **`rux-lsp`** (Tier 2): go-to-definition, hover, completion, diagnostics
   from `rux check`.
4. **TailwindCSS**, if it still looks worth it. See below.

---

### Worth deciding early

Things that are not scheduled above but that change the plan if the answer is
not what we assume.

- **Packaging: v1.0 → v0.6 (2026-08-09) → v0.7, after the script tier
  (2026-08-11).** Settled. The argument for moving it out of v1.0 stands and has
  not been revisited: nobody can build something real with Rux until they can
  hand the result to someone else, and narrowing to web and Windows with no
  installer formats means nothing durable gets frozen.

  What changed on the second move is where "the language has stopped moving"
  falls. v0.7 changes what a script can do, and animation adds a whole property
  family on top of that; shipping a build format immediately before both would
  have meant committing to an output surface and then watching it move
  underneath. Packaging now sits at the *end* of v0.7 rather than the start of
  v0.6, which is the same argument this list already made, applied with a better
  estimate of the timing. `rux build` stays listed under v1.0 as well, because
  what is left there (installers, the platform matrix) really does want a
  language that has stopped moving.

- **Animation had no milestone at all until 2026-08-11.** It is now v0.7, after
  the fork. The gap was found by accident while answering a question about route
  transitions: the word "animation" appeared exactly once in the entire doc set,
  in `02-spec.md`, saying `r-key` exists for "reordering, animation". A UI
  toolkit with no transitions reads as unfinished in a way a missing Tailwind
  layer does not, which is why it goes ahead of Tailwind rather than beside it.

- **Tailwind was first on the old list; here it is last.** It is a layer over a
  CSS engine that already works, and it mostly pays off for people already
  fluent in it. Components, a router, mutable-from-function state and mobile
  all unblock things that are impossible today rather than making possible
  things terser. The unresolved question underneath it has not changed: a
  curated utility set generated into Rux CSS is self-contained, while a real
  Tailwind pass pulls in the Node toolchain the project has avoided everywhere
  else, including in the web playground.

- **`docs/05-as-built.md` is maintained by hand and has been wrong.** It
  claimed the whole-tree rebuild was the largest remaining gap after v0.3 had
  already deleted it, and that was found by accident. The honored-CSS set is
  derivable from `rux-style`; generating the matrix would make the reference
  true by construction. This is a small job that removes a whole class of
  quiet error.

- **`docs/03-guide.md` should be deleted.** It opens by saying it does not work
  as written, it is excluded from the site build, and `/learn` now covers the
  same ground against the runtime that exists. Keeping a known-wrong tutorial
  in the repo costs more than the history is worth.

- **There is no test story for app authors.** The runtime has thirty-six test
  binaries; somebody writing a `.rux` file has nothing. If Rux wants apps, at
  some point "how do I test this screen" needs an answer better than "open it
  and look", which is the standing rule for *our* work precisely because it is
  hard to automate.

- **The host still needs a rebuild.** Template, style and script hot-reload;
  the compiled host does not. Dynamic-library reload would take the last
  rebuild out of the loop, and it is the kind of thing that is much easier to
  design in early than to retrofit.

- **`host::` is unrestricted.** That is fine while every `.rux` file is one you
  wrote. It stops being fine the moment files are shared, fetched, or pasted
  from a playground link. The browser build is sandboxed by wasm and safe by
  accident rather than by design; native is not sandboxed at all.
