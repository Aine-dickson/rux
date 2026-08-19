+++
title = "A message list"
description = "A scroller that follows its newest row: what makes a scroller, why query hands back a position rather than a thing, and where a list's width goes."
weight = 1
+++

A thread that grows at the bottom and stays there. Rows arrive animated, the
list keeps its own height, and sending scrolls to the newest message without
taking the scroll away from you mid-read.

Run it:

```
rux examples/recipes/message-list.rux
```

## The scroller is a box, not the screen

Nothing in Rux scrolls until something is told to, and the thing told is the box
with the overflow. That takes two declarations and both are load-bearing:

```css
.thread {
  width: 100%;
  height: 300px;
  overflow-y: auto;
}
```

Without the `height` there is nothing to overflow, so the box simply grows and
the page runs off the window. Without `overflow-y` there is no bar and no
wheel. Put the height on the screen instead and the whole document scrolls,
which is a different app.

### The width is not a detail

`align-items` on a flex parent defaults to `flex-start` here, not CSS's
`stretch`. That is a deliberate divergence, and children hug their content
because of it. Leave `width: 100%` off the thread and it is only as wide as its
longest message: the scroller works perfectly and looks broken.

Every box in this recipe that should span its parent says so.

## Rows that arrive

```rux
<view class="msg" r-for="m in messages" r-key="m.id" r-transition>
```

```css
.msg { transition: opacity 180ms ease-out, transform 180ms ease-out; }
.msg:enter-from { opacity: 0; transform: translateY(10px); }
```

`r-key` is doing two jobs at once. It is what lets a row animate at all, because
without an identity an insert and a reorder are the same picture and there is
nothing to hold a row by. And it is what keeps the rows above a new one from
being rebuilt, so a message you are part-way through reading does not flicker
when another arrives.

There is no `:leave-to` rule here on purpose. Nothing in this recipe removes a
message, so a leaving rule would be one nobody can reach.

## Following the newest row

This is the part worth reading twice.

```rux
<view class="thread">
  <view class="rows">
    <view class="msg" r-for="m in messages" r-key="m.id" r-transition>…</view>
  </view>
  <view class="anchor"></view>
</view>
```

```rux
fn toBottom() {
  let anchor = query(".anchor");
  if anchor.length > 0 {
    anchor[0].scrollIntoView();
  }
}
```

The anchor has no content and exists only to be scrolled to. Two things about
it are load-bearing, and both come from the same rule.

**It has to already exist.** `query` reads the tree that is *on screen*, which
is the one from before the handler ran. A message pushed onto the list a line
earlier has no element yet, so there is nothing to ask for and nothing to
reveal. Something that was already there can be revealed, and after the rebuild
it sits below the new row.

**It has to sit at a position that does not move.** `query` hands back a
*path*, which is a position among siblings, not an identity. Write the anchor
directly after the `r-for` and every new message pushes it along by one: the
path captured before the rebuild then names whatever slid into its place, and
the thread scrolls to a message somewhere in the middle. Nothing reports this,
because the reveal lands on a real element, just not the one that was asked
for.

Wrapping the rows in a box of their own fixes it. The anchor is then always the
second child of the thread, whatever the list does.

That is also why the `gap` moved from `.thread` onto `.rows`: with the anchor as
a sibling of the row box, a gap on the thread would push it a gap's width past
the last message.

## The whole file

```rux
<template>
  <screen class="app">
    <view class="thread">
      <view class="rows">
        <view class="msg" :class="#{ mine: m.mine }" r-for="m in messages" r-key="m.id" r-transition>
          <text class="who">{{ m.who }}</text>
          <text class="body">{{ m.text }}</text>
        </view>
      </view>
      <view class="anchor"></view>
    </view>

    <view class="composer">
      <input class="field" r-model="draft" placeholder="say something…" @submit="send()" />
      <view class="send" @tap="send()"><text class="send-label">send</text></view>
    </view>
  </screen>
</template>

<style>
  .thread { display: flex; flex-direction: column; width: 100%; height: 300px; overflow-y: auto; }
  .rows { display: flex; flex-direction: column; gap: 8px; width: 100%; }
  .msg { transition: opacity 180ms ease-out, transform 180ms ease-out; }
  .msg:enter-from { opacity: 0; transform: translateY(10px); }
  .anchor { height: 4px; }
  .composer { display: flex; flex-direction: row; gap: 8px; width: 100%; }
  .field { flex: 1; }
</style>

<script>
  let draft = signal("");
  let nextId = signal(3);
  let messages = signal([
    #{ id: 1, who: "ada", text: "The scroller is the box with the overflow, not the screen.", mine: false },
    #{ id: 2, who: "you", text: "And the list keeps its own height.", mine: true },
  ]);

  fn send() {
    let text = draft.trim();
    if text == "" { return; }
    messages.push(#{ id: nextId, who: "you", text: text, mine: true });
    nextId++;
    draft = "";
    toBottom();
  }

  fn toBottom() {
    let anchor = query(".anchor");
    if anchor.length > 0 {
      anchor[0].scrollIntoView();
    }
  }
</script>
```

The file in `examples/recipes/` carries the full styling and a second button
that makes a message arrive without you sending it, which is the case worth
driving: it is how you find out whether the list follows the thread while you
are reading it.

## What this recipe found

`scrollIntoView` did nothing at all for an element below the fold. The shell
picked which scroller to move by asking whose rectangle contained the element,
and that rectangle is the part of the scroller *on screen*, so a row scrolled
past the bottom belonged to no scroller and the request was dropped in silence.
Only reveals already a nudge away from being visible worked, which is the case
nobody asks for. It matches against the scroller's content now.

That is the argument for writing recipes at all. The pattern is ordinary, the
suite was green, and driving it once found a bug in the reveal that had shipped.
