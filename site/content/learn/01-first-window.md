+++
title = "Your first window"
description = "Open a native window from a file with no build step, and watch it reload as you type."
weight = 1
+++

Create a file called `hello.rux`:

```rux
<template>
  <screen class="app">
    <text class="title">Hello, Rux</text>
    <text class="sub">Edit this file and save. The window reloads itself.</text>
  </screen>
</template>

<style>
  .app {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 32px;
    background: #1e1e2e;
  }
  .title { color: #cdd6f4; font-size: 28px; font-weight: 700; }
  .sub   { color: #9399b2; font-size: 15px; }
</style>
```

Run it:

```bash
cargo run -p ruxlang -- hello.rux
```

A native window opens. Not a webview and not a browser tab. [taffy] laid that
out, [parley] shaped the text, and [vello] painted it on the GPU.

## Change something

Leave the window open. Change `font-size: 28px` to `48px` and save.

The window updates. There is no build step and no restart: Rux watches the file
and reloads it. This is the loop you will work in for the rest of the guide, so
it is worth getting a feel for now: put the editor and the window side by side
and change a few colours.

Only the compiled Rust host needs `cargo run` again. Everything in a `.rux` file
is read at runtime.

## What the pieces are

`<screen>` is the root element. Inside it, `<text>` draws text and `<view>` is a
plain box, the equivalent of a `<div>`. There are six elements in total and you
have already met three of them; the rest are `<image>`, `<button>` and `<input>`.

The `class` attribute works exactly like the web's, and `<style>` is literal
CSS, not a lookalike DSL. `.title { font-size: 28px }` is parsed by
[lightningcss], the same engine that minifies CSS for the web, and matched with a
real cascade. Selectors, specificity and shorthands behave the way you expect.

Rux does not honor *all* of CSS, and it tells you when you use something it
doesn't. Try adding `line-height: 2` to `.title` and watch the terminal:

```
rux: CSS property `line-height` is parsed but not yet honored
```

One line per unhonored property, once each. Nothing fails silently, which
matters a lot when the thing you are styling is a window rather than a page you
can inspect. The full honored set is in [the reference](@/reference/_index.md).

## Checkpoint

`examples/learn/01-hello.rux`.

Next: what those three sections are actually for.

[taffy]: https://github.com/DioxusLabs/taffy
[parley]: https://github.com/linebender/parley
[vello]: https://github.com/linebender/vello
[lightningcss]: https://lightningcss.dev/
