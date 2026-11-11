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
  <view class="task" :class="{ done: done }">
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

<script>
  prop label: string;
  prop done: bool;
</script>
```

The two `prop` lines are the component's inputs: what its caller has to hand
it. `prop done: bool = false;` would give one a default, so a caller could leave it
off.

Import it and use it as a tag. A prop is passed as a `:`-prefixed attribute,
evaluated in the caller's scope:

```rux
<script>
  use components::row;

  let draft = signal("");
  let items = signal([ /* … */ ]);
</script>
```

```rux
<view class="slot" r-for="(t, i) in items" @tap="toggle(i)">
  <row :label="t.label" :done="t.done" />
</view>
```

`use components::row;` resolves to `components/row.rux`, relative to the file
doing the importing.

The declarations are checked at the tag. Misspell one (`:lable`) or pass
something `row.rux` never declared, and `rux check` says so, naming the props
it does have. Leave off `:done` and it says that too.

## What isolation means

A component instance sees **only its props**. `items` and `draft` do not exist
inside `row.rux`, and its CSS styles its own subtree and nothing else.

That is why the tap handler stays in the parent, on a wrapper `<view>`: the
handler needs `items` (through `toggle`), which the component cannot reach. The component receives
two plain values and decides how they look. It is a slightly awkward split here
and a very useful one as an app grows: a component can never quietly reach into
state it doesn't own.

The other way round is `emit`, which is how a component tells its caller
something happened without being handed the state to change. A row could
`emit("toggle")` and let the parent decide what that means. This chapter keeps
the wrapper, because one idea at a time.

Editing `row.rux` hot-reloads the running window just like the main file.

## Checkpoint

`examples/learn/05-components.rux`, plus `examples/learn/components/row.rux`.

That is the whole language. Templates, CSS, signals, four directives, and
components. There is no fifth concept waiting for you.

## Try changing it

The most useful next step is to break it. A few that are worth the time:

- **Delete a row.** `items.remove(i)`, from a button inside the row. Watch out
  for the tap on the row firing at the same time.
- **Filter it.** Add a `filter` signal and an `r-if` on the row, or drive the
  list through `items.filter(…)` in the `r-for` expression itself.
- **Persist it.** `settings.set("tasks", items)` keeps a value between runs, and
  `settings.get("tasks")` reads it back at start (see the script reference).
  For anything the script cannot do, write it in Rust in the project's `native/`
  crate and call it as a module.
- **Use a real checkbox.** `<input type="checkbox" r-model="flag" />` is a
  tap-toggle that writes its signal directly, and `.box:checked { … }` styles
  it.

## Where the edges are

Rux is `0.x` and honest about it. The [reference](@/reference/_index.md) ends
with the list of what is not built yet, kept current with every change, and the
[roadmap](@/roadmap/_index.md) says in what order those gaps close.

## Where to go next

- The **[recipes](@/recipes/_index.md)** are the next thing to read if you want
  to build something specific: a message list, a tab bar, a modal, each a
  working file written around the part of the pattern that surprises people.
- The **[reference](@/reference/_index.md)** has the exact honored-CSS set and
  the full list of what is and is not built.
- The **[roadmap](@/roadmap/_index.md)** has the order the gaps get closed in,
  and the **[blog](@/blog/_index.md)** covers each release as it lands.
