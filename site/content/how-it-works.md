+++
title = "How it works"
description = "From a .rux file to pixels on the GPU: the pipeline, the crates it leans on, and the parts that are honestly still crude."
template = "page.html"
weight = 2
+++

Rux is a pipeline, not a framework. A `.rux` file goes in one end, a native window
comes out the other, and every stage is a crate you can read in an afternoon. The
whole runtime is about 7,000 lines.

## The pipeline

```
  app.rux
     │
     ▼
┌──────────────┐   <template> / <style> / <script>
│  rux-parser  │   hand-rolled SFC split + XML-ish template parser
└──────┬───────┘
       ▼
┌──────────────┐   lightningcss parses the CSS; our cascade resolves it onto
│  rux-style   │   nodes. Directives (r-for/r-if/r-model), {{ bindings }},
└──────┬───────┘   and component expansion happen here → a styled node tree
       ▼
┌──────────────┐   Style → taffy (flex / grid / block) → absolute paint items,
│  rux-layout  │   plus hit, focus and scroll regions
└──────┬───────┘
       ▼
┌──────────────┐   paint items → a vello Scene: fills, gradients, shadows,
│  rux-paint   │   borders, clips, transforms, images, glyph runs
└──────┬───────┘
       ▼
┌──────────────┐   winit window + wgpu surface; vello renders; input, focus,
│  rux-shell   │   selection, scrolling, clipboard, and the file watcher
└──────────────┘
```

Two crates sit alongside rather than in the flow: **rux-script** (the [rhai]
engine holding state and running `@tap` handlers) and **rux-text** ([parley]
shaping, measuring and drawing text). **rux-runtime** owns the `Document` that
ties it together — load, resolve imports, build the engine, rebuild the tree.

| Crate | Job |
|---|---|
| `rux-parser` | SFC split + template parser (ours) |
| `rux-style` | lightningcss → our cascade → `Style`; directives; components |
| `rux-script` | rhai engine (state + handlers) + `host::` registry |
| `rux-layout` | `Style` → taffy → paint items, hit / focus / scroll regions |
| `rux-text` | parley shaping, measure, wrapping; vello glyph drawing |
| `rux-paint` | paint items → vello scene |
| `rux-runtime` | `Document`: load, imports, engine, rebuild |
| `rux-shell` | window, GPU, input, focus, clipboard, watcher |
| `rux-cli` | `rux [file.rux]` |

## Layout is CSS, not a lookalike

`rux-style` runs real [lightningcss] over your `<style>` block, then applies its
own cascade: specificity, source order, and inheritance for the properties that
inherit (`color`, `font-size`, `font-family`). The result is a `Style` struct per
node, which `rux-layout` maps onto [taffy] — the same layout engine Dioxus and
Bevy use.

That mapping is the honest boundary of "it's just CSS." Taffy implements flexbox,
CSS grid and block layout properly, so those behave the way you expect, traps
included:

- A scroll container needs a **bounded height**, or it just grows.
- Rows in a scrolling flex column need **`flex-shrink: 0`**, or the column
  squeezes them all in and there's nothing to scroll.
- A word longer than its box **overflows** unless you set `overflow-wrap:
  break-word`.

Those aren't Rux quirks — that's CSS. What taffy has no notion of is **inline text
flow**: there's no inline formatting context, so text can't wrap around a floating
image and `display: inline` isn't real. Everything defaults to `display: block`,
and you reach for `display: flex` to lay things out.

## Text hugs its box

Text is [parley] end to end: shaping, line breaking, cursor geometry, font
fallback. One deliberate deviation is worth knowing, because it shows up
everywhere:

**Rux trims leading.** A line box is `ascent + descent`, not the font's full line
height, and the baseline sits at `top + ascent`. Text therefore hugs its box, and
`padding` reads equally on all four sides — which is what you want when you're
styling with CSS. Set `line-height` and you get the normal behaviour back.

The cost is that anything positioning itself against drawn glyphs must use *our*
line stepping, not parley's. That's why `selection_rects` takes only the
horizontal extent from parley's selection geometry and recomputes `y` itself: take
parley's rects wholesale and the highlight drifts further off the glyphs with
every wrapped line.

## Painting

`rux-layout` doesn't produce a widget tree — it produces a flat, ordered list of
paint items: `Rect`, `Text`, `Image`, `Shadow`, `Tick`, and the bracket pairs
`PushClip`/`PopClip`, `PushTransform`/`PopTransform`, `PushOpacity`/`PopOpacity`.
`rux-paint` walks that list into a [vello] `Scene`, keeping a transform stack so a
transformed element carries its subtree with it.

Vello renders the scene with compute shaders on the GPU via [wgpu]. Rux is
**event-driven**: it paints in response to a redraw request — resize, tap,
keystroke, reload — not on a frame clock. An idle Rux window uses no CPU. The one
exception is the caret, which blinks on a 530 ms timer while an input is focused,
and stops the timer the moment it isn't.

## State, and the rebuild

State lives in [rhai] signals:

```rux
<script>
  let n = signal(0);
</script>
```

`{{ n }}` reads it. `@tap="n = n + 1"` writes it. `r-model="name"` binds an input
two ways. Handlers are interpreted, so **they hot-reload**; heavy work goes to
compiled Rust through a `host::` registry, which needs a rebuild. That split is
deliberate: script is glue, Rust is the engine.

Here is the crude part, stated plainly: **any signal change rebuilds the entire
tree.** Not a diff, not a subscription — the whole thing, then a fresh layout.

At these sizes it's imperceptible, and it's simple to reason about. But it means
every piece of *ephemeral* UI state — the caret, the selection, scroll offsets,
which dropdown is open, where keyboard focus is — is thrown away on every
keystroke and has to be **put back by hand** afterwards. There's a named restore
pass for each one. This has already produced one real bug (the caret stayed
visible in the input you'd just left, because the restore pass only ever *set* a
caret and never cleared one), which is why the roadmap keeps a running list of
every restore pass that exists.

Fine-grained, per-binding reactivity deletes that whole category. It's the next
significant piece of work, and the last real divergence between the architecture
doc and the code.

## Hot reload

A [notify] watcher watches the file's directory recursively — so imported
components reload too — and wakes the event loop through an `EventLoopProxy` on
any `.rux` change. The `Document` reloads, rebuilds, repaints. If the new file
fails to parse, the last good document stays on screen and the error goes to
stderr rather than blanking the window.

That's a stand-in for a proper dev overlay, and it points at a real ceiling:
**Rux's error surfacing is poor.** Unknown CSS is ignored (it now warns once per
property), a bad file gives you stderr, and there's no in-window diagnostic. It's
on the list.

## Why compose instead of build

Every heavy part of Rux is someone else's crate — taffy, parley, vello, wgpu,
winit, lightningcss, rhai. That's Law 4, and it's why a UI language with a GPU
renderer fits in ~7,000 lines. It also means Rux inherits their bugs and their
release cadences, which is a trade worth making: those crates are maintained by
people who care about layout, text and rasterisation more than this project ever
could.

[taffy]: https://github.com/DioxusLabs/taffy
[parley]: https://github.com/linebender/parley
[vello]: https://github.com/linebender/vello
[wgpu]: https://wgpu.rs/
[lightningcss]: https://lightningcss.dev/
[rhai]: https://rhai.rs/
[notify]: https://github.com/notify-rs/notify
