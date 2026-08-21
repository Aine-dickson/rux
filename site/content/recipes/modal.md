+++
title = "A modal"
description = "A dialog over a scrim, both animated: how a cover is positioned, why a tap does not bubble, and what a swallow costs."
weight = 3
+++

A dialog centred over a scrim, both animated, dismissed by the scrim or by
Cancel, with the page behind it exactly where it was.

Run it:

```
rux examples/recipes/modal.rux
```

## The scrim is the modal

```rux
<view class="scrim" r-if="open" r-transition @tap="dismiss()">
  <view class="dialog" @tap="0">
    …
  </view>
</view>
```

One `r-if`, one swap. The scrim is what covers the page, what takes the tap that
dismisses, and what the `r-transition` is written on, so the scrim and
everything inside it arrive and leave as a single animation.

## A cover is `fixed`

```css
.scrim {
  position: fixed;
  top: 0; left: 0; right: 0; bottom: 0;
  display: flex;
  align-items: center;
  justify-content: center;
}
```

`fixed` measures against the window rather than against a box, so where the
modal is written stops mattering: it can sit at the end of the template, where
it reads as an afterthought, because it is one. A fixed box is also outside
every scroller, so a modal opened over a scrolled list does not drift when the
list moves underneath it.

`absolute` would also work here, and would be the answer if you wanted the cover
to stop at some box rather than at the window. It measures against the nearest
ancestor that is **not** `position: static`, which is CSS's rule: `static` is the
default and is the one value that is not a containing block. So a cover that
should fill a panel wants `position: relative` on that panel, and nothing in
between needs to say anything.

The flex centring is what puts the dialog in the middle. A scrim is one of the
few places `align-items: center` earns its keep.

## A tap does not bubble

**This is the line the recipe exists for:**

```rux
<view class="dialog" @tap="0">
```

A tap goes to the topmost element that *has a handler*, and it stops there.
There is no propagation, which means there is nothing to stop, and it also means
a box with no handler is not a tap target at all. So without that `@tap`, a tap
anywhere on the dialog falls straight through to the scrim behind it and closes
the thing you were trying to use.

The handler does nothing on purpose. `0` is an expression that evaluates and
changes no state; its whole job is to make the dialog a tap target so the scrim
never sees the tap.

### What the swallow costs

It is not free. A `@tap` box is an interactive element by definition, so the
dialog takes the focus ring when it is tapped and becomes a Tab stop that does
nothing. There is currently no way to say "tappable but not focusable".

Worth knowing before deciding that a scrim should dismiss at all. A Cancel
button costs nothing, needs no swallow, and is the only dismissal a keyboard
user can reach anyway.

## Two elements, two rules, one swap

```css
.scrim { transition: opacity 180ms ease-out; }
.scrim:enter-from { opacity: 0; }
.scrim:leave-to   { opacity: 0; }

.dialog { transition: transform 180ms ease-out; }
.scrim:enter-from .dialog { transform: translateY(12px) scale(0.96); }
.scrim:leave-to   .dialog { transform: translateY(12px) scale(0.96); }
```

The swap is opened by the `r-if` on the scrim, and `:enter-from` / `:leave-to`
apply to the whole subtree under it. So the scrim fades while the dialog moves,
from one swap, with no second `r-transition` and nothing to keep in step.

Note what is deliberately *absent*: no `position: absolute` on `:leave-to`. That
rule is for a departing box that was in the flow and is holding a space it
should give up. The scrim is already out of the flow, so it has no place to give
up and nothing below it to disturb. Adding the rule here would be cargo from
[the tab bar](@/recipes/tab-bar.md), where it is essential.

## The page behind does not move

Nothing in the modal touches the layout of the page under it: the scrim is out
of the flow, so the rows behind keep their positions and their scroll. That is
worth checking by eye whenever you build one, because the usual way to get it
wrong is to make the modal a sibling that is still in the flow, and the symptom
is the page jumping as the dialog opens.
