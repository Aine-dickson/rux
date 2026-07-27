+++
title = "Components, and where next"
description = "Extract a row into its own file, understand what isolation buys you, and find the edges of the current release."
weight = 5
+++

The row has grown enough markup and CSS to deserve its own file. Put it in a
`components/` folder next to your app:

`components/row.rux`

```rux
<template>
  <view class="task" :class='#{ done: done }'>
    <view class="box" />
    <text class="label">{{ label }}</text>
  </view>
</template>

<style>
  .task {
    display: flex;
    flex-direction: row;
    flex-grow: 1;
    align-items: center;
    gap: 10px;
    padding: 12px;
    background: #313244;
    border-radius: 8px;
  }
  .box {
    width: 18px;
    height: 18px;
    flex-shrink: 0;
    background: #1e1e2e;
    border: 2px #585b70 solid;
    border-radius: 4px;
  }
  .label { color: #cdd6f4; font-size: 15px; }

  .task.done .box   { background: #a6e3a1; border: 2px #a6e3a1 solid; }
  .task.done .label { color: #6c7086; text-decoration: line-through; }
</style>
```

Import it and use it as a tag. Props are the `:`-prefixed attributes, evaluated
in the caller's scope:

```rux
<script>
  use components::row;

  let draft = signal("");
  let items = signal([ /* … */ ]);
</script>
```

```rux
<view class="slot" r-for="t in items"
      @tap='for i in 0..items.len() { if items[i].label == t.label { items[i].done = !items[i].done; } }'>
  <row :label="t.label" :done="t.done" />
</view>
```

`use components::row;` resolves to `components/row.rux`, relative to the file
doing the importing. It has to be **alone on its own line**: the import is
picked out of the script by a line scan, not parsed.

## What isolation means

A component instance sees **only its props**. `items` and `draft` do not exist
inside `row.rux`, and its CSS styles its own subtree and nothing else.

That is why the tap handler stays in the parent, on a wrapper `<view>`: the
handler needs `items`, which the component cannot reach. The component receives
two plain values and decides how they look. It is a slightly awkward split here
and a very useful one as an app grows: a component can never quietly reach
into state it doesn't own.

Editing `row.rux` hot-reloads the running window just like the main file.

## Checkpoint

`examples/learn/05-components.rux`, plus `examples/learn/components/row.rux`.

That is the whole language. Templates, CSS, signals, four directives, and
components. There is no fifth concept waiting for you.

## Try changing it

The most useful next step is to break it. A few that are worth the time:

- **Delete a row.** `items.remove(i)` inside the same indexed loop. Watch out
  for the tap on the row firing at the same time.
- **Filter it.** Add a `filter` signal and an `r-if` on the row, or drive the
  list through `items.filter(…)` in the `r-for` expression itself.
- **Persist it.** This one needs Rust: register a `host::` function and call it
  from a handler. That is the escape hatch for anything the script tier can't do.
- **Use a real checkbox.** `<input type="checkbox" r-model="flag" />` is a
  tap-toggle that writes its signal directly. A checked box carries a synthetic
  `checked` class, so `.box.checked { … }` styles it.

## Where the edges are

Rux is `0.x` and honest about it. Things you will run into if you keep going,
as of **v0.3.0**:

- **No CSS variables, `@media`, or pseudo-classes.** No `:hover`, `:focus` or
  `:checked`, which is why the `checked` class above exists as a stopgap.
  These are the headline items of the next release.
- **No `fn` that mutates state**, as [chapter 3](@/learn/03-state.md) covered.
- **No index in `r-for`**, and reordering a list re-renders more rows than a
  keyed diff would.
- **No effects or computed values.** A `{{ }}` expression is the only
  "computed" there is.
- **No true inline text flow**, so no mixing bold into a sentence yet.

The [reference](@/reference/_index.md) has the exact honored-CSS set and the
full gap list; the [roadmap](@/roadmap/_index.md) has the order they get fixed
in, and the [blog](@/blog/_index.md) covers each release as it lands.

If you build something with this, or hit an edge that isn't written down,
[the contribute page](@/contribute/_index.md) is the way in. It is a tour of
how a `.rux` file becomes pixels, and which crate owns which stage.
