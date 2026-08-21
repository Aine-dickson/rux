+++
title = "State that changes"
description = "Signals, interpolation, tap handlers, and functions that can change state."
weight = 3
+++

Add a `<script>` section. It is [rhai], a small embedded scripting language, and
`signal()` is the only new thing in it:

```rux
<script>
  let count = signal(0);
</script>
```

Now read it in the template with `{{ }}`, and change it with `@tap`:

```rux
<view class="head">
  <text class="title">Tasks</text>
  <text class="tally">{{ count }} added</text>
</view>

<button class="add" @tap="count = count + 1">
  <text class="addlabel">Add one</text>
</button>
```

Tap the button and the tally moves.

Anything between `{{ }}` is a rhai *expression*, not just a name. `{{ count * 2 }}`
and `{{ items.len() }}` are both fine. You will use that in the next chapter to
count finished tasks without keeping a second signal in sync.

## Only what reads it updates

When `count` changes, Rux updates the bindings that actually read `count`, not
the tree. Nothing is rebuilt, so a caret you had placed stays where it was, a
scrolled list stays scrolled, and an open dropdown stays open.

You can watch this happen:

```bash
RUX_TRACE=1 rux run 03-state.rux
```

Every interaction prints the path it took: `patched in place (no rebuild)` or
`rebuilt (structural)`. Display changes patch; changes that add or remove nodes
(`r-if` flipping, a list growing) reconcile just that subtree.

## Functions

A `fn` sees the scope it was written in, and can read and write it. So the
obvious thing works:

```rux
<script>
  let count = signal(0);

  fn add() {
    count = count + 1;
  }
</script>
```
```rux
<button @tap="add()">
```

That is worth a sentence of history, because it is new. Until v0.7 a `fn` got
its own scope with none of your signals in it, so `add()` ran, changed nothing,
and reported no error, and every handler in every example had to be written
inline. If you have read older Rux material telling you to keep `fn`s pure, that
is what it was about, and it no longer applies.

Inline still works and is still right for one-liners: `@tap="count = count + 1"`
needs no function. Handlers take several statements too, and an `if`. Reach for
a `fn` when the handler stops fitting on a line, or when two handlers want the
same thing.

### The one that still catches people

A closure passed to a **method** cannot see the surrounding scope:

```rux
fn tally() {
  // WRONG: `done` is not visible inside the closure
  items.filter(|t| t.done == done).len()
}
```

A method call passes its receiver by reference, and the scope cannot be borrowed
at the same time, so method dispatch does not capture. A plain call does. Lift
what the closure needs into a parameter, or use a loop.

## Quoting

Handlers are attributes, and the script wants real string literals, so an
expression containing a string needs the quotes kept apart. Two ways, both fine:

```rux
<button @tap='name = ""'>                <!-- single-quoted attribute -->
<button @tap="name = &quot;&quot;">      <!-- entity inside a double-quoted one -->
```

HTML entities are decoded, so `&quot;` is a real double quote by the time the
script sees it. The examples in this repository lean on it, because an attribute
that already contains single quotes has no other way out.

What does **not** work is `'x'`: the script reads single quotes as a **character**,
not a string, which is rhai's rule and not something Rux changes. So
`@tap="name = 'ada'"` is an error rather than an assignment.

## `r-if`

While `count` is 0, show something else:

```rux
<text class="empty" r-if="count == 0">Nothing yet.</text>
```

`r-if` takes a condition and removes the element when it is false. There is
`r-elif` and `r-else` for chains, and `r-show`, which keeps the element's space
but doesn't paint it.

## Checkpoint

`examples/learn/03-state.rux`.

Next: a real list, and a real text input.

[rhai]: https://rhai.rs/
