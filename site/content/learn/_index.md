+++
title = "Learn"
description = "Build a real Rux app from an empty file: layout, state, lists, and components, in about half an hour."
weight = 0
sort_by = "weight"
template = "docs-section.html"
page_template = "docs.html"
+++

The [reference](@/reference/_index.md) tells you what Rux does. This tells you how
to use it. By the end you will have built a working task list. You will have typed
into a real text input, added rows, ticked them off and scrolled it, and met
every idea Rux has.

There are five of them, and that is the whole language:

- `<template>` says **what the content is**
- `<style>` is **literal CSS** that says where it goes and how it looks
- `<script>` holds **signals**, the state
- **directives** (`r-for`, `r-if`, `r-model`) connect the two
- **components** let you name a piece and reuse it

If you have written a web page, most of this is already familiar. The chapters
are short and each one ends with a file you can run.

## What you need

Rust, and about half an hour. No Node, no npm, no browser. Rux opens a native
window and paints it on the GPU.

```bash
git clone https://github.com/Aine-dickson/rux
cd rux
cargo run -p ruxlang -- examples/learn/01-hello.rux
```

The first build takes a few minutes. After that, edits to a `.rux` file reload
in the open window without rebuilding anything.

If you would rather not install anything yet, the
[playground](@/playground.md) runs the same runtime in your browser. Every
complete example in these chapters has a **Try it** button that opens it there,
so you can follow along and change things without cloning a thing. It needs a
browser with WebGPU, which means a recent Chrome, Edge, Firefox or Safari.

## The finished app

Every chapter's checkpoint lives in [`examples/learn/`](https://github.com/Aine-dickson/rux/tree/main/examples/learn),
so you can skip ahead, or check your file against a working one when something
looks wrong. The last chapter's version is `05-components.rux`.

```rux
<view class="row" r-for="t in items" :class='#{ done: t.done }'
      @tap='for i in 0..items.len() { if items[i].label == t.label { items[i].done = !items[i].done; } }'>
  <view class="box" />
  <text class="label">{{ t.label }}</text>
</view>
```

That is the heart of it: a repeated row, a class bound to state, and a handler
that writes back to the list. If that line looks slightly unusual, chapter 5
explains exactly why it is written that way, and what happens if you write the
obvious thing instead.

## A note on versions

This guide is written against **Rux v0.4.0**, the current release, and every
snippet in it is checked by the test suite on each commit. Rux is `0.x` and
moves weekly, so a feature you read about elsewhere may be newer than this
guide. The [reference](@/reference/_index.md) is always the authority on what
the current release honors, and the [blog](@/blog/_index.md) tracks what changed.
