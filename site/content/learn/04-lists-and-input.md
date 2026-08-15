+++
title = "Lists and input"
description = "r-model, r-for, and the snapshot rule, and why writing to the loop variable silently does nothing."
weight = 4
+++

Time to build the actual app. Three things go in: a text field, a list, and a
way to tick a row off.

## The field

```rux
<input class="field" r-model="draft" placeholder="add a task…" />
```

```rux
<script>
  let draft = signal("");
</script>
```

`r-model` binds the input to a signal in both directions. Type, and `draft`
changes on every keystroke; write to `draft`, and the field updates.

It is a real text input, not a rectangle that collects characters: there is a
caret you can place by tapping, arrow keys, Home/End, Backspace and Delete
around it, drag-select, double-click to select a word, Shift+arrows to extend,
and Ctrl+A/C/X/V against the actual system clipboard. Tab moves focus through
every interactive element with a focus ring. None of that needs anything from
you.

## Adding an item

Make `items` a list of maps, using rhai's map literal `#{ … }`:

```rux
<script>
  let draft = signal("");
  let items = signal([
    #{ label: "read the reference", done: true },
    #{ label: "build a task list", done: false },
  ]);
</script>
```

The Add button pushes onto it and clears the field:

```rux
<button class="add" @tap='if draft != "" { items.push(#{ label: draft, done: false }); draft = ""; }'>
  <text class="addlabel">Add</text>
</button>
```

Single-quoted attribute, because of the `""`. The guard means an empty field
adds nothing.

Rux notices `items` changed by comparing its value before and after the handler,
so **mutating methods like `push` count as a change**: you do not have to
reassign the whole list.

## Repeating a row

```rux
<view class="row" r-for="t in items">
  <view class="box" />
  <text class="label">{{ t.label }}</text>
</view>
```

`r-for="t in items"` renders its element once per item, with `t` bound inside.

The tally from the last chapter can now count for real, with a closure and no
second signal to keep in sync:

```rux
<text class="tally">{{ items.filter(|t| t.done).len() }} / {{ items.len() }}</text>
```

And the empty state gets a real condition:

```rux
<text class="empty" r-if="items.len() == 0">Nothing yet. Add your first task.</text>
```

## The snapshot rule

Now tick a row off. The obvious handler is:

```rux
<view class="row" r-for="t in items" @tap="t.done = !t.done">   <!-- WRONG: does nothing -->
```

It does nothing at all. No error, no change.

`t` is a **snapshot**. When Rux compiles the handler it bakes the loop variable
in as a literal value, because by the time you tap, the loop is long over. So
`t.done = !t.done` flips a copy that is discarded a moment later, and since
`items` never changed, nothing re-renders.

The same is true of rhai's own `for` loop, for the same reason:

```rux
@tap="for t in items { t.done = true; }"    <!-- WRONG: also a copy -->
```

**Writes have to go through the signal, by index:**

```rux
<view class="row" r-for="t in items"
      @tap='for i in 0..items.len() { if items[i].label == t.label { items[i].done = !items[i].done; } }'>
```

`items[i]` reaches into the signal itself, so the change is real and detected.
Reading `t` is still fine, and that is what `t.label` is doing, matching the
snapshot against the live list to find the right row.

It is more verbose than it should be. `r-for` has no index variable yet, which
is why the match is on `label`, and the underlying reason a reusable
`toggle(i)` function can't help here is the `fn` rule from
[the last chapter](@/learn/03-state.md). Both are known gaps rather than
design.

## Styling by state

`:class` binds a class from an expression, and it merges into the classes the
cascade matches, so a bound class styles exactly like a written one:

```rux
<view class="row" r-for="t in items" :class='#{ done: t.done }' @tap='…'>
```

```css
.row.done .box   { background: #a6e3a1; border: 2px #a6e3a1 solid; }
.row.done .label { color: #6c7086; text-decoration: line-through; }
```

`#{ done: t.done }` is rhai's object form: apply `done` when the value is true.
A plain string or an array works too.

> **Watch out:** rhai has **no ternary operator**. `:class='t.done ? "done" : ""'`
> does not parse, and the class silently never appears. Use an `if` expression
> instead, either `:class='if t.done { "done" } else { "" }'` or the object
> form above.

There is also `:style`, which takes a declaration string and overlays it like an
inline style, and `:src` / `:options` for images and selects. Any attribute
prefixed with `:` is a rhai expression, and rhai has backtick templates, so
`` :style="`background: ${c}`" `` works with no extra syntax.

## Making it scroll

```css
.list {
  display: flex;
  flex-direction: column;
  align-items: stretch;
  gap: 8px;
  height: 320px;
  overflow: auto;
}
.row { flex-shrink: 0; /* … */ }
```

Two lines there are load-bearing, and leaving either out is the most common
"why won't it scroll":

- **`height`**: a scroll container needs a bounded height (or `max-height`, or
  a `flex-grow` slot). Without one it just grows, and there is nothing to
  scroll.
- **`flex-shrink: 0` on the rows**: otherwise the column squeezes every row
  in to fit, and again nothing overflows. A browser does this too.

`align-items: stretch` is the divergence from [chapter 2](@/learn/02-layout.md):
without it the rows hug their labels instead of filling the list.

Scrolling then works by wheel, by dragging the scrollbar, by touch, and by
keyboard. The offset survives re-renders, so ticking a row off doesn't
jump you back to the top.

## Checkpoint

`examples/learn/04-tasks.rux`, a complete working app.
