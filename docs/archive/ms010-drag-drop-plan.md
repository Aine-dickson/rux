# MS-010: drag and drop primitives

## Context

Mava Studio (field report `docs/field-reports/mava-studio-2.md`, MS-010) has to
reorder and re-parent rows of an outline tree by dragging, as SortableJS does
in ilovestem. Rux has `@drag` (a gesture) and layout rects, but no drop
targets, no dragging between containers, no drag copy and no animated reflow.
The owner chose **primitives** (2026-11-06): a drag source, drop targets that
are told the slot under the pointer, a drag copy drawn by Rux, and a general
move animation for keyed rows. The author moves the data. A sortable list is
then about 15 lines of script, shipped as an example. This also settles the
roadmap's "Drag and drop" item under "Worth deciding early".

## The syntax (the owner approves it with this plan)

```rux
<view class="lessons" drop-group="outline"
      @dragover="moveTo(event.data, section.id, event.index)"
      @drop="save()">
  <view r-for="l in section.lessons" r-key="l.id" r-transition class="row"
        :drag-data="l.id" drag-group="outline"
        @dragend="if !event.dropped { undo() }">
    <view drag-handle>≡</view> {{ l.title }}
  </view>
</view>
```
```css
.row { transition: transform 150ms ease; }   /* siblings slide */
.row:dragging { opacity: 0.4; }               /* the item in its slot */
.lessons:drag-over { background: #eef; }      /* the target under the pointer */
```

- **Source:** any element with `:drag-data="expr"` (or a static `drag-data`).
  The value is evaluated at build time and handed to targets as `event.data`.
  `drag-group="name"` is optional. `@dragstart` and `@dragend` fire on it.
- **Handle:** `drag-handle` on a descendant. If a source has a handle, only a
  press on the handle picks the source up.
- **Target:** any element with `@drop`. `drop-group="name"` says which group it
  accepts; with no `drop-group` it accepts sources with no `drag-group`.
  `@dragenter`, `@dragover` and `@dragleave` are optional. The source can
  also write `@dragmove` (every pointer move; see "Scope rule" below).
- **`DropEvent`** (one type for all six events): `data`, `group`, `index`,
  `before`, `dropped`, plus the usual `x`, `y`, `pageX`, `pageY`, `width`,
  `height`, `button`, `touches`. `x` and `y` are relative to the element the
  handler is on.
  - `index` is the slot among the target's items, counted **without the
    dragged one** (SortableJS's `newIndex`), so a splice works.
  - `before` is the `drag-data` of the item it would land in front of, or
    `none` at the end. A filtered list can use it where an index cannot.
  - The target's **items** are the sources whose nearest target ancestor is
    that target.
- **When each event fires:**
  - `@dragover` fires on entering a target and again **each time the slot
    changes**, not on every pointer move, so a live move rebuilds only when it
    has to.
  - On a drop: `@drop` on the target, then `@dragend` with `dropped: true` on
    the source.
  - Escape, or a release outside every accepting target: `@dragleave`, then
    `@dragend` with `dropped: false`.
- **Pseudo-classes:**
  - `:dragging` matches every element whose `drag-data` equals the one being
    dragged. Matching by value rather than by path keeps it on the item as a
    live move carries it across lists.
  - `:drag-over` matches the target under the pointer.
- **Move animation (general, works without dragging too):** a keyed `r-for`
  row with `r-transition`, whose `transition` covers `transform` (or `all`),
  slides from its old place to its new one when the list changes, as Vue's
  `.v-move` does. A row is matched by its key across the whole frame, so a row
  moved to another list slides too. Mava's existing move-up and move-down
  commands get the animation for free.
- **Slot rule:** read from the target's direction.
  - Column: the pointer's y against each item's middle.
  - Row: the same on x.
  - Wrapping or grid: reading order (an item counts as before the pointer if
    the pointer is below it, or within its height and right of its middle).
- **Nested targets (the tree):** the deepest accepting target under the
  pointer wins, never one inside the dragged source, so a section cannot be
  dropped into itself. An empty list accepts a drop when it has size.
- **Touch:**
  - A press on a handle claims the finger once it passes the slop, as `@drag`
    does.
  - A source with no handle is picked up by a long press, so a scroll still
    scrolls.
  - On such a source, a `@contextmenu` on touch fires only if the finger lifts
    without moving after the long press. This is a change to MS-011's
    behaviour, for drag sources only.
- **An element with both `:drag-data` and `@drag`:** the source wins, and
  `rux check` warns.

## Implementation

Follow commit `66b4536` (MS-011) for where a new event touches the tree:
`rux-style` reads the attributes, `rux-layout` regions, `rux-shell` dispatch,
`rux-script/src/native.rs` and `check.rs` types, `rux-cli/src/vocab.rs`,
`editors/vscode/vocabulary.json`.

### Step 1: a general move transition for keyed rows (its own commit, before the rest)

Owner chose to include it, as a general feature (not a sortable one): any
keyed `r-for` list gets it, so move-up and move-down buttons animate too.

1. `rux-style` (`build_node_inner`, near the `gestures` block at
   `crates/rux-style/src/lib.rs:8693`): mark a keyed `r-transition` row whose
   transition list covers `transform` with `moves: Option<(duration, easing)>`
   on the layout `Node`. Reuse the easing types from `rux-style/src/anim.rs`.
   Keep `Node` small: put the new drag fields together behind
   `Option<Box<…>>`. `Style` must not grow (744-byte budget, watchlist #41).
2. `rux-layout`: report each `moves` row's key and rect in **unscrolled**
   coordinates (the containing scrollers' offsets added back), so an
   auto-scroll is not mistaken for a move. Take a per-frame `nudges` input
   (path → dx, dy) on `LayoutState` that `collect` composes before the node's
   own transform. It is an input outside the tree, so a frame that only nudges
   stays on the cheap `Prepared` path (as scroll offsets do), and the hit
   regions follow through `HitRegion.transform`.
3. `rux-shell`: keep the last frame's rects by key (a key seen once in both
   frames only). When a row moves, start an offset of old minus new, eased to
   zero over its duration, and request frames while any offset is running. A
   further move mid-slide starts from where the row is now drawn.

### Step 2: sources, targets and events

1. `rux-style`:
   - Read `:drag-data`/`drag-data` (stored as the value's literal,
     `Value::to_rhai_literal`, so `rux-layout` needs no new dependency),
     `drag-group`, `drag-handle`, `drop-group`.
   - Read the six handlers, with the row's locals baked in through
     `bind_locals` as the gestures are.
   - Add the `:dragging` and `:drag-over` cases to `Pseudo`/`ElemStates`. The
     shell hands the active data and target path in through
     `InteractionState`, the way `hovered` is handed in.
2. `rux-layout`:
   - `Owners` gains the nearest source and whether the box is inside a handle.
   - The `Layout` gains `drag_sources` (seq, rect, transform, data, group,
     has-handle, handlers, instance) and `drop_targets` (seq, path, rect,
     transform, accepted group, direction/wrap, handlers, instance, its items'
     rects and data).
   - Add `source_at`, and a `drop_at(targets, x, y, group, dragged)` that
     returns the target and the slot. Unit-test the slot rule and the nested
     rule here (pure functions).
3. `rux-shell`: a `DragDrop` state beside `GesturePress`.
   - Picked up in `move_gesture` past the slop (or on the long-press deadline
     for touch without a handle) in place of `@drag`.
   - Each move: hit-test with `drop_at`, then fire enter, over, leave and drop
     through the same `apply_handler_with_event` path as `fire_gesture`.
   - `end_gesture` drops; Escape in the key path cancels.
   - `@dragend` runs on the source found again by its data (fresh locals), and
     falls back to the one captured at pickup.
   - Restyle when `:dragging` or `:drag-over` changes.

### Scope rule (owner, 2026-11-06)

Rux ships primitives, not effects. If an author can build it from what Rux
exposes, Rux does not ship it. So there is **no shell-drawn drag copy** and
**no auto-scroll feature**:

- The author draws the copy as their own `position: fixed` box, from
  `event.data` and the pointer. For that the source gets `@dragmove`, which
  fires on **every** pointer move of the session with the `DropEvent` fields
  (`data`, `pageX`, `pageY`, and the grab offset `grabX`, `grabY` within the
  source). Shadow, rotation or a badge are the author's CSS.
- Auto-scroll: first check whether script can scroll a container by an amount
  (`scrollBy` or equivalent). If it cannot, add that one call; the author
  writes the edge test in `@dragmove`.

### Step 5: checks, docs, example

- `rux check`:
  - Warn on `:drag-data` together with `@drag`.
  - Warn on `drag-handle` outside a source.
  - Warn on `drop-group`, `@dragover`, `@dragenter` or `@dragleave` without a
    `@drop`.
  - Add `DropEvent` in `check.rs`/`native.rs`.
- Vocabulary: `vocab.rs` and `editors/vscode/vocabulary.json`.
- `examples/sortable.rux`: a two-level outline (sections holding lessons) with
  live moves, undo on cancel, reorder within a list, a move between lists, and
  an empty list.
- Docs:
  - A "Drag and drop" section in `docs/07-script.md`.
  - The move animation under keyed lists in `docs/05-as-built.md`.
  - `DropEvent` in `docs/10-types.md`.
  - Remove the roadmap item; set MS-010's status.
  - Rows in `docs/08-user-tests.md`.
  - Then `./site/sync-docs.sh` from Git Bash.
  - The handoff.
- Not in this work, and written down as such:
  - keyboard drag and drop (Mava has its commands);
  - dropping files from the OS onto a target;
  - a styleable drag copy;
  - drags between windows.

## Verification

- `cargo test -p rux-layout -p rux-style -p rux-shell -j 2`, then
  `cargo test --workspace -j 2` (full parallelism runs out of memory).
- New tests:
  - Slot rule (column, row, wrap, with the dragged item left out).
  - Nested and self-exclusion rules.
  - Shell-level tests on the existing gesture-test pattern
    (`crates/rux-shell/src/lib.rs` around 11463): pickup at the slop; the
    handle-only rule; the event order for a drop and for a cancel; the touch
    long-press pickup.
  - Move-animation offsets start and settle.
  - Debug self-checks stay on, so a nudged frame must equal a whole layout.
- `rux check` on the example (no warnings) and on probes that should warn.
- **Driven in the window** (owner rule): `examples/sortable.rux` and a probe in
  `C:\Users\USER\Mobile\rux-harness\ms010`, using `tools/drive.ps1 drag` and
  screenshots. Check reorder, a move between lists, a drop into the empty
  list, a section not droppable into itself, auto-scroll in a short window,
  Escape cancelling, and the move animation from a plain button reorder. Log
  each row in `docs/08-user-tests.md`. Touch, web, Android and macOS are listed
  there as not driven.
- Commit each step locally (no push). Never `git add -A`, since the tree is
  shared.
