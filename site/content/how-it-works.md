+++
title = "How it works"
description = "From a .rux file to pixels on the GPU: the pipeline, the crates it leans on, and the parts that are honestly still crude."
template = "page.html"
weight = 2
+++

Rux is a pipeline, not a framework. A `.rux` file goes in one end, a native window
comes out the other, and every stage is a crate of its own. The heavy parts
(layout, text, painting, the window) are mature crates Rux composes; what Rux
writes is the script language, the template parser, the reactive graph and the
document model. The [architecture tour](@/contribute/_index.md) has every stage in
detail.

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

Beside the flow sit **rux-script** (Rux's own script language: the checker, the
typed IR and the interpreter that holds state and runs handlers) and **rux-text**
([parley] shaping, measuring and drawing text). **rux-runtime** owns the
`Document` that ties it together: load, resolve imports, build the engine, patch
the tree.

| Crate | Job |
|---|---|
| `rux-parser` | SFC split + template parser (ours) |
| `rux-style` | lightningcss → our cascade → `Style`; directives; components |
| `rux-syntax`, `rux-ir`, `rux-script` | the script language: parser, typed IR, checker and interpreter |
| `rux-codegen` | Rust written from the IR, for release builds |
| `rux-reactive` | signal values |
| `rux-layout` | `Style` → taffy → paint items, hit / focus / scroll regions |
| `rux-text` | parley shaping, measure, wrapping; vello glyph drawing |
| `rux-paint` | paint items → vello scene |
| `rux-runtime` | `Document`: load, imports, engine, patching |
| `rux-shell` | window, GPU, input, focus, clipboard, accessibility, watcher |
| `rux-native`, `rux-bindgen` | an app's own Rust, `#[rux::export]`, and reading its signatures |
| `rux-audio`, `rux-video`, `rux-icons` | sound, video, and the Tabler icon set |
| `rux-web` | the same shell compiled to WebAssembly, on a canvas |
| `rux-cli` | the `rux` command |

## Layout is CSS, not a lookalike

`rux-style` runs real [lightningcss] over your `<style>` block, then applies its
own cascade: specificity, source order, and inheritance for the properties that
inherit (`color`, `font-size`, `font-family`). The result is a `Style` struct per
node, which `rux-layout` maps onto [taffy], the same layout engine Dioxus and
Bevy use.

That mapping is the honest boundary of "it's just CSS." Taffy implements flexbox,
CSS grid and block layout properly, so those behave the way you expect, traps
included:

- A scroll container needs a **bounded height**, or it just grows.
- Rows in a scrolling flex column need **`flex-shrink: 0`**, or the column
  squeezes them all in and there's nothing to scroll.
- A word longer than its box **overflows** unless you set `overflow-wrap:
  break-word`.

Those aren't Rux quirks. That's CSS. Inline flow is built for text: a `<text>`
inside a `<text>` is part of the same sentence, and an `<image>` or `<path>` in
one sits in the line. Boxes with their own layout do not flow inline (there is no
`display: inline-block`), so everything defaults to `display: block`, and you
reach for `display: flex` to lay things out.

## Text hugs its box

Text is [parley] end to end: shaping, line breaking, cursor geometry, font
fallback. One deliberate deviation is worth knowing, because it shows up
everywhere:

**Rux trims leading.** A line box is `ascent + descent`, not the font's full line
height, and the baseline sits at `top + ascent`. Text therefore hugs its box, and
`padding` reads equally on all four sides, which is what you want when you're
styling with CSS. Set `line-height` and you get the normal behaviour back.

The cost is that anything positioning itself against drawn glyphs must use *our*
line stepping, not parley's. That's why `selection_rects` takes only the
horizontal extent from parley's selection geometry and recomputes `y` itself: take
parley's rects wholesale and the highlight drifts further off the glyphs with
every wrapped line.

## Painting

`rux-layout` doesn't produce a widget tree. It produces a flat, ordered list of
paint items (rectangles, text, images, shadows, paths, and bracket pairs that push
and pop a clip, a transform or an opacity).
`rux-paint` walks that list into a [vello] `Scene`, keeping a transform stack so a
transformed element carries its subtree with it.

Vello renders the scene with compute shaders on the GPU via [wgpu]. Rux is
**event-driven**: it paints in response to a redraw request (resize, tap,
keystroke, reload) rather than on a frame clock. An idle Rux window paints
nothing. A running transition, playing media, an interval and the caret's blink
(a 530 ms timer while an input is focused) wake it while they last, and stop when
they end.

## State, and what a change touches

State lives in signals, declared in the script:

```rux
<script>
  let n = signal(0);
</script>
```

`{{ n }}` reads it. `@tap="n = n + 1"` writes it. `r-model="name"` binds an input
two ways. Script is interpreted, so **it hot-reloads**, and work that belongs in
Rust goes to the app's own `native/` crate, as functions marked `#[rux::export]`
that script calls like a module (that part needs a rebuild). A release build also
compiles the script's functions to Rust.

**A change touches only what reads it.** Each binding records the signals it
read, and a handler is known by the signals it wrote, so after a handler runs the
runtime patches the text and attributes those signals reach, mounts or unmounts
`r-if` subtrees, and reconciles `r-for` lists by `r-key`. Parse and cascade do
not run again. Because the tree is patched rather than rebuilt, the caret, the
selection, scroll offsets and an open dropdown stay where they were.
`RUX_TRACE=1 rux run app.rux` prints the path each interaction took.

## Hot reload

A [notify] watcher watches the file's directory recursively, so imported
components and stylesheets reload too. The document loads again and is swapped
in; the window keeps its size and the page it was on. If the new file fails to
load, the last good frame stays on screen under a **dev overlay** naming the
error, never a crash and never a blank window. `rux run --device` does the same
on an Android device or emulator.

## Why compose instead of build

Every heavy part of Rux is someone else's crate: taffy, parley, vello, wgpu,
winit, lightningcss, accesskit. That's Law 4, and it's why one project can carry
a UI language with a GPU renderer. It also means Rux inherits their bugs and their
release cadences, which is a trade worth making: those crates are maintained by
people who care about layout, text and rasterisation more than this project ever
could.

[taffy]: https://github.com/DioxusLabs/taffy
[parley]: https://github.com/linebender/parley
[vello]: https://github.com/linebender/vello
[wgpu]: https://wgpu.rs/
[lightningcss]: https://lightningcss.dev/
[notify]: https://github.com/notify-rs/notify
