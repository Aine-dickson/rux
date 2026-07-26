+++
title = "Laying things out"
description = "Flexbox, the one divergence from CSS you need to know, and why two <text> elements never share a line."
weight = 2
+++

Start the app's header. Replace the body of `hello.rux`:

```rux
<template>
  <screen class="app">
    <view class="head">
      <text class="title">Tasks</text>
      <text class="tally">0 / 0</text>
    </view>
  </screen>
</template>

<style>
  .app {
    display: flex;
    flex-direction: column;
    gap: 16px;
    padding: 24px;
    background: #1e1e2e;
  }

  .head {
    display: flex;
    flex-direction: row;
    justify-content: space-between;
    align-items: center;
  }
  .title { color: #cdd6f4; font-size: 24px; font-weight: 700; }
  .tally { color: #9399b2; font-size: 15px; }
</style>
```

The title sits left, the tally right. That is ordinary flexbox and it is the
real thing — taffy implements the same spec a browser does, so `flex-grow`,
`flex-basis`, `gap`, `align-self` and the rest behave as you already know.

## Use `display: flex`

Here is the rule that will save you the most time:

> **Everything defaults to `display: block`, and block containers make their
> children fill. If you want a layout, say `display: flex`.**

Delete `display: flex` from `.head` and save. The title and tally stack
vertically and each spans the full width. That is `block` doing exactly what
block does — it is just rarely what you want.

## Two `<text>` elements never share a line

There is **no inline text flow**. Two `<text>` siblings stack; they do not run
together into a paragraph the way two `<span>`s would.

This is a real limitation, not a preference — taffy has no inline formatting
context, and building one means writing a line-breaker. So if you want words on
one line, they go in **one** `<text>`, and if you want them laid out beside each
other, that is a flex row.

Text *within* a single `<text>` wraps normally, honors `text-align`, and shapes
properly through parley — including scripts that need it.

## The one divergence: children hug

In CSS, a flex container's cross axis defaults to `stretch`. In Rux it defaults
to **`flex-start`** — children hug their content instead of filling.

This is deliberate. Stretch-by-default is the source of a lot of "why is my
button full width" confusion, and hugging is what you want most of the time.
When you *do* want children to fill, say so:

```css
.list { display: flex; flex-direction: column; align-items: stretch; }
```

You will need exactly that in chapter 4, so it is worth remembering where it is.

Hugging means `fit-content`, and it is clamped to the parent's inner width — a
box with no `width` cannot burst out of a narrower parent. An explicit `width`
is your call and *will* overflow; clip it with `overflow: hidden` if you meant
it.

## Sizes are logical pixels

`16px` is the same physical size on a 1× and a 2× display. Layout and taps run
in logical space and the scene is scaled to the display's DPI at paint time, so
you never write DPI-handling code. `%`, `rem` (16px), `vw`, `vh` and `dvh` all
work.

`display: grid` works too, with `grid-template-columns` / `-rows`, spans and
auto-flow — useful once you have more than a list to arrange.

## Checkpoint

`examples/learn/02-layout.rux`.

The header says `0 / 0` because it is hard-coded. Next, make it mean something.
