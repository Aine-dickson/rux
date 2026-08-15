+++
title = "Reactivity"
description = "Signals, computed values, effects, and what re-runs when one changes."
weight = 5
+++

<!-- GENERATED FROM docs/05-as-built.md BY site/sync-docs.sh. DO NOT EDIT HERE. -->


> **[Script](/reference/script/) is the reference for this section.** What follows is
> the tour; that is the whole surface in depth, including every way Rux's script
> differs from stock rhai and from JavaScript.

- `<script>` is **rhai**, forked as `rux-rhai`. `let x = signal(v)` declares state (numbers coerce to float).
- `{{ expr }}` interpolation; `r-if` / `r-elif` / `r-else`, `r-for="x in list"`, `r-show`.
- `@tap="…"` handlers.
- `host::fn()` calls into compiled Rust (registered in `rux-runtime::build_engine`).

> **CHANGED in v0.7: a function can now read and write the state around it.**
> This used to be the single biggest trap in the language, and it is gone.
>
> ```
> let level = signal(82);
> fn drain() { level-- }        // works
> fn is_low() { level < 20 }    // works
> ```
>
> `@tap="drain()"` is a handler like any other, and the write is tracked, so the
> screen updates. Handlers no longer have to be written inline, which is why so
> much of this document and of `/learn` still shows them that way.
>
> Two things to know:
> - **A method call does not see the surrounding scope.** `thing.helper()`
>   cannot reach `level`; `helper(thing)` can. Method dispatch passes its
>   receiver by reference and the scope cannot be borrowed at the same time.
> - **Anything heavy still belongs in a `host::` function.** Script is for
>   describing what the UI does, not for doing work.

**`query(selector)` reads the tree from a handler.** It takes a CSS selector,
the same one the stylesheet takes (tags, ids, classes, `role`, and the `>`, `+`
and `~` combinators), and returns the matching elements in document order. It is
the stylesheet's own matcher, so the two always agree about the same document.

```rux
@tap="count = query('.card').length"
```

Each result carries `tag`, `id` and `classes`. `id` is absent rather than empty
when the element has none, so `el.id ?? "none"` reads the way it does anywhere
else.

Three rules, all of which are the design rather than temporary limits:

- **It matches the tree, not the template.** A `<view r-if="open">` that is
  closed is not there to be found, because it is not on screen.
- **It works in a handler and nowhere else.** In a `{{ }}` binding, a `:style`
  or an `r-if` it raises and the overlay says why. A binding that read the tree
  would have to invalidate whenever layout changed, and invalidating it rebuilds
  and relayouts, which never settles.
- **A selector that does not parse is an error**, not an empty result, so a typo
  cannot look like "nothing matched".

**Geometry reads back as `x`, `y`, `width` and `height`**, in absolute window
pixels, from the frame currently on screen. That makes it **one frame stale**,
exactly as `getBoundingClientRect` is in a browser: a handler runs before the
next layout, so it reads what the last one produced.

```rux
fn measure() {
  let card = query(".card")[0];
  report = card.width + " x " + card.height;
}
```

Geometry is **absent rather than zero** when there is no frame to read: nothing
has been laid out yet, or the node is hidden by `r-show="false"` and so has no
box. `card.width ?? "unknown"` is how to handle it. Under `rux check` it is
always absent, since checking runs with no window and no GPU on purpose.

**Three actions, which are not tree edits:** `el.focus()`, `blur()` and
`el.scrollIntoView()`. They change host state (what holds the caret, where a
scroller sits), so the next build produces the same tree it would have anyway.
Each records an intent that is applied once the handler has finished, so a
handler that focuses something and then writes a signal does not race the
rebuild its own write causes. `blur()` is free-standing rather than a method,
because there is only one focused element. Only a text input can take focus,
since focus is keyed by `r-model`; asking anything else says so.

**`focus()` is not a tap.** It puts the caret in an input and nothing more: it
runs no `@tap` handler, follows no `to=` link and toggles no checkbox.

**`el.tap()` is the whole gesture.** It presses the element as a finger would,
at the centre of its box, through the same dispatch a real pointer goes through.
So it runs the `@tap` handler, follows a `to=` link, toggles a checkbox or
radio, opens a `type="select"` dropdown, moves keyboard focus and puts the caret
in a text input, in each case because that is what a press already does. It is
not a wrapper that calls the handler.

Two consequences of it being a real press rather than an imitation:

- **It hit-tests.** The topmost element at that point wins, so tapping something
  covered by an open dropdown taps the dropdown, exactly as a finger would.
- **An element with no box cannot be tapped.** Hidden by `r-show="false"`, or
  never laid out, means there is nothing to press, and it says so.

A tap runs a handler, and that handler can tap something else. The chain is cut
off after eight rounds, the same bound and the same reasoning as an `emit`
chain, so a button that taps itself stops rather than hanging the window.

Watch the quoting: `'x'` is a single **character** in a script, not a string, so
a selector needs `"…"`. Inside a `@tap="…"` attribute there is no room for
those, which is the practical reason to name the handler and call it.

**There is no way to mutate the tree from script, deliberately.** No setting a
property, no adding or removing children. A state change regenerates the
affected tree from the template, so any such edit would be overwritten by the
next patch, reconcile or rebuild, silently, at a moment decided by an unrelated
handler. The tree is a function of state, and state is how it changes.

`examples/element-query.rux` demonstrates all of it.
