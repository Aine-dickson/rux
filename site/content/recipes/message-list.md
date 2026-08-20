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

<!-- FROM: examples/recipes/message-list.rux -->
```rux
<template>
  <screen class="app">
    <text class="title">a message list</text>
    <text class="lead">A list that grows at the bottom and stays there. Send a few, then scroll up and send another: the view follows the newest message without taking the scroll away from you mid-read.</text>

    <!-- The scroller is this box, not the screen. Nothing in Rux scrolls until
         something is told to, and the thing told is the box with the overflow,
         so `.thread` is what gets the height and the bar. Give the height to
         the screen instead and the list grows past the window and clips. -->
    <view class="thread">
      <!-- The rows live in a box of their own, and that box is what makes the
           anchor below work. See the comment on the anchor. -->
      <view class="rows">
        <!-- `r-key` is doing two jobs. It is what lets a row animate in,
             because without an identity an insert and a reorder are the same
             picture; and it is what keeps the rows above a new one from being
             rebuilt, so a message you are reading does not flicker when one
             arrives. -->
        <view class="msg" :class="#{ mine: m.mine }" r-for="m in messages" r-key="m.id" r-transition>
          <text class="who">{{ m.who }}</text>
          <text class="body">{{ m.text }}</text>
        </view>
      </view>

      <!-- The anchor: no content, and there only to be scrolled to.

           Two things about it are load-bearing. **It has to already exist**,
           because `query` reads the tree that is on screen, which is the one
           from before the handler ran, so a message just pushed onto the list
           has no element yet and cannot be asked for.

           And **it has to sit at a position that does not move**, which is why
           the rows are wrapped above. `query` hands back a *path*, a position
           among siblings, not an identity. Put the anchor directly after the
           `r-for` and every new message pushes it along by one, so the path
           captured before the rebuild names whatever slid into its place, and
           the list scrolls to a message somewhere in the middle instead of to
           the end. Nothing reports this: the reveal lands on a real element,
           just not the one that was asked for. -->
      <view class="anchor"></view>
    </view>

    <view class="composer">
      <input class="field" r-model="draft" placeholder="say something…" @submit="send()" />
      <view class="send" :class="#{ dead: draft.trim() == &quot;&quot; }" @tap="send()">
        <text class="send-label">send</text>
      </view>
    </view>

    <view class="composer">
      <view class="send" @tap="reply()">
        <text class="send-label">a message arrives</text>
      </view>
    </view>
  </screen>
</template>

<style>
  .app {
    display: flex;
    flex-direction: column;
    gap: 12px;
    padding: 20px;
    background: #1e1e2e;
    height: 100%;
  }

  .title { color: #cdd6f4; font-size: 22px; font-weight: 700; }
  .lead { color: #a6adc8; font-size: 13px; line-height: 20px; }

  /* The two lines that make a scroller: a height it cannot exceed, and
     permission to overflow. Without the height there is nothing to overflow,
     and the box simply grows. */
  .thread {
    display: flex;
    flex-direction: column;
    /* `align-items` on a flex parent defaults to `flex-start` here, not CSS's
       `stretch`, so a child hugs its content unless it is told to fill. Leave
       this out and the thread is only as wide as its longest message, which
       looks like a bug in the scroller and is not one. */
    width: 100%;
    height: 300px;
    overflow-y: auto;
    padding: 12px;
    background: #181825;
    border-radius: 12px;
  }

  /* The gap lives here now rather than on `.thread`, so the anchor is not
     pushed a gap's width away from the last message. */
  .rows { display: flex; flex-direction: column; gap: 8px; width: 100%; }

  .msg {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 8px 12px;
    background: #313244;
    border-radius: 10px;
    max-width: 320px;
    opacity: 1;
    transition: opacity 180ms ease-out, transform 180ms ease-out;
  }

  /* A row arrives from just below where it lands, which reads as coming up out
     of the composer. There is no `:leave-to` here on purpose: nothing removes a
     message, so a leaving rule would be a rule nobody can reach. */
  .msg:enter-from { opacity: 0; transform: translateY(10px); }

  .mine { background: #45475a; align-self: flex-end; }

  .who { color: #89b4fa; font-size: 11px; font-weight: 700; }
  .body { color: #cdd6f4; font-size: 14px; line-height: 20px; }

  /* Height rather than nothing: a zero-height box is still laid out, but giving
     it the gap's worth of room means the newest message clears the bottom edge
     instead of sitting flush against it. */
  .anchor { height: 4px; }

  .composer { display: flex; flex-direction: row; gap: 8px; align-items: center; width: 100%; }

  .field {
    flex: 1;
    padding: 10px 12px;
    background: #313244;
    border: 2px solid #45475a;
    border-radius: 10px;
    color: #cdd6f4;
    font-size: 14px;
  }

  .field:focus { border-color: #89b4fa; }

  .send {
    padding: 10px 16px;
    background: #89b4fa;
    border-radius: 10px;
    cursor: pointer;
  }

  .send:hover { background: #b4befe; }
  .send-label { color: #1e1e2e; font-size: 13px; font-weight: 700; }
  .dead { background: #45475a; }
  .dead .send-label { color: #6c7086; }
</style>

<script>
  let draft = signal("");
  let nextId = signal(3);
  let messages = signal([
    #{ id: 1, who: "ada", text: "The scroller is the box with the overflow, not the screen.", mine: false },
    #{ id: 2, who: "you", text: "And the list keeps its own height.", mine: true },
  ]);

  // Push, then scroll. The order matters less than it looks: `scrollIntoView`
  // does not move anything itself, it asks the host to, and the host acts after
  // the tree has been rebuilt with the new row in it.
  fn send() {
    let text = draft.trim();
    if text == "" { return; }
    messages.push(#{ id: nextId, who: "you", text: text, mine: true });
    nextId++;
    draft = "";
    toBottom();
  }

  fn reply() {
    messages.push(#{ id: nextId, who: "ada", text: "Message number " + nextId + ", arriving while you read.", mine: false });
    nextId++;
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

That is the file in `examples/recipes/`, verbatim: the one the example tests
walk and the one that was driven in a window before this page was written. The
second button, which makes a message arrive without you sending it, is the case
worth driving: it is how you find out whether the list follows the thread while
you are reading it.

## What this recipe found

`scrollIntoView` did nothing at all for an element below the fold. The shell
picked which scroller to move by asking whose rectangle contained the element,
and that rectangle is the part of the scroller *on screen*, so a row scrolled
past the bottom belonged to no scroller and the request was dropped in silence.
Only reveals already a nudge away from being visible worked, which is the case
nobody asks for. It matches against the scroller's content now.

That is the argument for writing recipes at all. The pattern is ordinary, the
suite was green, and driving it once found a bug in the reveal that had shipped.
