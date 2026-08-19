+++
title = "A tab bar"
description = "Three tabs over one router, with pages that cross rather than queue: to= and :current, the one CSS line that stops a route transition stacking, and a guard on a tab."
weight = 2
+++

Three tabs, one router, and a page that slides in while the last one slides out.
The bar never moves and never re-renders.

Run it:

```
rux examples/recipes/tab-bar.rux
```

## The bar goes outside the router

```rux
<view class="bar">
  <view class="tab" to="/"><text class="tab-label">inbox</text></view>
  <view class="tab" to="/drafts"><text class="tab-label">drafts</text></view>
  <view class="tab" to="/sent"><text class="tab-label">sent</text></view>
</view>

<view class="stage">
  <router r-transition>
    <route path="/" view="tab-inbox" />
    <route path="/drafts" view="tab-drafts" />
    <route path="/sent" view="tab-sent" />
  </router>
</view>
```

Only the matched view is swapped, so anything written outside the `<router>` is
built once and left alone by every navigation. That is the whole reason the bar
does not flicker, and it is worth knowing before reaching for anything cleverer.

`to=` is the entire link. It taps to that path, announces as a link rather than
a button, and matches `:current` when it names the path you are on:

```css
.tab:current { background: #89b4fa; }
```

So there is no `active` signal to keep in step, and nothing that can drift out
of step with where the router actually is. The highlight is not a copy of the
router's state, it *is* the router's state.

For a list whose every row links somewhere different, `:to="…"` is the computed
form.

## The one line that stops the pages stacking

`r-transition` on the `<router>` makes a navigation animated, and the way it
does that is by keeping **both pages really on screen** while it runs. That is
what makes a crossfade possible, and it is also the thing that surprises
everybody:

```css
.page:leave-to { position: absolute; top: 0; left: 0; right: 0; }
```

Without it, the two pages queue. A departing box with no inset keeps the place
it would have had, and with both pages live the place the arriving one has is
*below* the departing one. The stage jumps to twice its height for the length of
the transition and then snaps back, which is far more visible than the animation
you were trying to add.

Taking the leaver out of the flow hands its space over at the *start* of the
swap instead, so the pair overlap. Both behaviours are wanted, in different
situations: a page swap wants the space handed over, a row dropping out of a
list does not.

Two things have to be true of the box they sit in:

```css
.stage {
  position: relative;   /* what the departing page is pinned to */
  height: 180px;        /* so the box does not collapse while it is pinned */
}
```

The height can be anything that does not depend on the content, including a
`min-height` or a `flex-grow` slot. What it cannot be is nothing, or the stage
collapses at the moment the only page still in the flow is the one leaving.

## The transition, and where its duration comes from

```css
.page {
  transition: opacity 240ms ease-out, transform 240ms ease-out;
}
.page:leave-to  { opacity: 0; transform: translateX(-40px); }
.page:enter-from { opacity: 0; transform: translateX(40px); }
```

Every view's root carries `.page`, so one pair of rules covers all of them.

**A swap lasts as long as the element's own `transition` says**, read off the
computed style. Write `transition: opacity 240ms` and the page is held on screen
for 240ms without saying so twice, and the two can never disagree. The
consequence is that a more specific rule changes the length of the swap and not
only the speed of the walk:

```css
.stage.slow .page { transition: opacity 1400ms ease-out, transform 1400ms ease-out; }
```

That is intended, and it is still the sort of thing that is easier to meet here
than in your own app at midnight.

## Sliding the other way

Every navigation in this recipe slides the same direction. Making it slide
*back* when you move left along the bar needs the direction known before the
swap opens, and `to=` navigates without giving anything a chance to run first.
So it becomes an explicit handler:

```rux
<view class="tab" @tap="dir = -1; navigate(&quot;/drafts&quot;)">
```

```css
.back .page:leave-to  { transform: translateX(40px); }
.back .page:enter-from { transform: translateX(-40px); }
```

with `dir` driving a class on the stage. That is a real trade: you give up
`:current` and the link semantics and have to keep an index yourself. Worth it
for a phone-style tab bar, not worth it for three tabs on a desktop.

## Keeping someone out of a tab

A `guard` on a route decides whether a navigation to it may happen:

```rux
<route path="/sent" view="outbox" guard="gate()" />
<route path="/locked" view="sign-in" />
```

```rux
fn gate() {
  if !unlocked {
    return "/locked";
  }
}
```

`false` cancels, a string redirects to that path, and **anything else allows**,
including `()`. That last part is what makes the shape above work: a function
that falls off the end has said nothing, and saying nothing is consent, so the
only branch you have to write is the one that objects.

**The guard runs before the history moves.** That is the whole reason it lives on
the route rather than in the page: a refused navigation leaves no entry behind
and opens no transition, whereas a page that refuses to render itself has already
been navigated to. It follows that Back, Forward and a deep link go through
guards as well. A guard that only covered `navigate` would protect nothing, since
Back reaches the same page without passing it, and Back is how anyone leaves a
sign-in screen.

A guard on the `<router>` runs on every navigation, and a guard on a parent route
covers every page inside it, so a whole section can be closed off in one line.

Guards are synchronous: there are no promises in the script language, so a guard
decides from state that is already there. Fetch first, then navigate.

**A guard that fails refuses.** It is the one place in Rux where an expression
that blows up does not fall back to something harmless, and the reason is that a
guard has no harmless answer: `guard="user.is_admin"` while `user` is still
loading would otherwise admit everyone and look exactly like a working app.

## Identity is which route matched

Two paths that match the same route are one page showing different data, so
`/crew/7` to `/crew/12` updates in place rather than swapping. This matters the
moment a tab has detail pages under it: moving between two rows of a list is not
a navigation as far as the transition is concerned, and should not be.
