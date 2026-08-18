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
