+++
title = "State that changes"
description = "Signals, interpolation, tap handlers, and the one rule about functions that trips up everybody."
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

## The rule that catches everyone

This is the single biggest trap in Rux, so it gets its own heading:

> **A rhai `fn` cannot read or write a signal.**

This looks completely reasonable and does not work:

```rux
<script>
  let count = signal(0);

  fn add() {           // WRONG: `count` is not visible in here
    count = count + 1;
  }
</script>

<button @tap="add()">  <!-- runs, changes nothing, reports no error -->
```

It is a constraint of stock rhai: a function body gets its own scope and the
globals are not in it. So:

- **State changes go inline in the handler.** `@tap="count = count + 1"`.
  Handlers can be several statements: `@tap='a = 1; b = 2'` is fine, and so is
  an `if`.
- **Script `fn`s must be pure**: take arguments, return a value, touch no
  state. `fn label(n) { if n == 1 { "task" } else { "tasks" } }`, called as
  `{{ label(count) }}`. That works, and is the right home for display logic.
- **Anything heavier belongs in Rust**, as a `host::` function.

If a tap seems to do nothing, this is the first thing to check. Lifting the
restriction needs a rhai fork, and it is on the roadmap.

## Quoting

Handlers are attributes, and rhai wants real string literals, so:

```rux
<button @tap='name = ""'>      <!-- single-quoted attribute, "" inside -->
```

Use **single quotes on the attribute** whenever the expression contains a
string. Rux does not decode HTML entities, and rhai reads `'x'` as a character
rather than a string, so the alternatives don't work. You will see this
throughout the next chapter.

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
