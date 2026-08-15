+++
title = "Script"
description = "The script language: state, functions, values, the element API, and every way it differs from rhai and from JavaScript."
weight = 6
+++

<!-- GENERATED FROM docs/07-script.md BY site/sync-docs.sh. DO NOT EDIT HERE. -->


What goes inside `<script>`, and what a `{{ }}` binding or an `@tap` attribute
may contain.

Rux's script tier is **`rux-rhai`**, a fork of [rhai](https://rhai.rs) 1.25.1.
This document is the reference for it. Pointing at rhai's own documentation is no
longer correct: the fork changes what several things *mean*, not only what is
available, and every difference is listed here.

Where this document and [As Built](/reference/) overlap, they agree; As
Built is the wider tour of the whole language, this is the script half in depth.
`crates/rux-rhai/DIVERGENCE.md` is the engine-level record of what the fork
changes and why, for anyone rebasing it.

## The three places script runs

Almost every rule below follows from which of these you are in.

| Where | Example | Runs |
|---|---|---|
| The top-level script | `let n = signal(0);` | Once, when the document loads |
| A binding | `{{ n * 2 }}`, `:class`, `r-if`, `r-for` | On every build, possibly many times a second |
| A handler | `@tap="bump()"` | When something happens |

A **binding is an expression that must not do anything**. It is re-evaluated on
every build and once per row of an `r-for`, so a side effect in one happens an
unpredictable number of times. Nothing enforces this for arbitrary code, but it
is why `query()` is rejected in a binding and why `navigate()` in one is a
mistake rather than a feature.

A **handler is a statement**, and may be several. It can write state, call
functions, navigate, and read the tree.

## State

`signal(v)` declares a reactive value. A binding that reads one *is* a
subscription to it.

```rux
let level = signal(82);
let items = signal(["a", "b"]);
```

`signal()` is identity: it returns what it was given, and its job is to mark the
declaration. Numbers are coerced to float on the way through, so arithmetic is
consistent.

**`computed name = expr;`** declares derived state, readable anywhere a signal
is. Refreshing is one pass in declaration order, so a computed may read one
declared above it and not below.

**`effect { … }`** runs when what it read changes, and once on load. It
subscribes to what it actually read on its last run, and is never woken by its
own writes.

Both are document-level: a component's own `computed` and `effect` lines are
stripped rather than run. See [As Built](/reference/) for the detail on
both.

## Lifecycle

**`mounted { … }`** runs once, after the first tree exists. **`unmounted { … }`**
runs when the document stops being the one on screen, which is the window
closing or a hot reload replacing it.

```rux
<script>
  let level = signal(0);
  mounted   { level = host::last_saved(); }
  unmounted { host::save(level); }
</script>
```

Blocks, not functions, matching `effect { }`. A hook is not something the author
calls, and giving it a callable name would invite exactly that. Several blocks
of the same kind run in the order written.

`mounted` runs **after** the effects, so a hook reading a signal sees what the
effects decided, and it runs **exactly once**, which is the whole difference
between it and an `effect` that happens to fire on load. Whatever it writes
reaches the first frame: it deliberately runs after the tree exists rather than
during construction, or the screen would show the value the hook was written to
replace.

`unmounted` runs for its side effect. Nothing is done with what it writes,
because by then there is no tree for a write to reach. It runs at most once even
if teardown is reached twice, so a document closed by a reload and then by the
window does not save twice.

> **Not supported inside a component yet.** A component that declares either is
> warned rather than ignored, since a hook that silently never runs is the exact
> failure the dev overlay exists to catch.

## Functions

A function sees the scope it was written in, and can read and write it.

```rux
let level = signal(82);

fn drain() { level-- }          // writes the signal, the screen follows
fn is_low() { level < 20 }      // reads it
```

`@tap="drain()"` is a handler like any other and the write is tracked. **This
changed in v0.7**: before it, a `fn` could not touch state at all and every
handler had to be written inline, which is why older examples and `/learn` still
show them that way.

**A method call does not capture the surrounding scope.** `thing.helper()`
cannot reach `level`; `helper(thing)` can. Method dispatch passes its receiver by
reference, and the scope cannot be borrowed at the same time. This is upstream's
limitation and it stands.

**Anything heavy belongs behind `host::`.** Script describes what the UI does; it
is not where work gets done.

## Values

Numbers are `f64` in every practical case: `signal()` coerces, and a value on its
way into a binding is normalised. Two integer literals divide as floats, so
`10 / 3` is `3.333…` and not `3`. Division by zero gives `Infinity` or `NaN` as
in JavaScript, and those display under those names.

Strings are double-quoted. **`'x'` is a single character, not a string**, which
is inherited from rhai and is the single most common thing to trip over. A
selector or any other text needs `"…"`. Inside a `@tap="…"` attribute there is no
room for double quotes, which is the practical reason to name a handler and call
it:

```rux
<!-- wrong: '#note' is a character literal -->
<button @tap="query('#note')[0].focus()">

<!-- right -->
<button @tap="focus_note()">
```

Object literals are **`#{ key: value }`**, not `{ … }`. Bare braces are a block
everywhere in this language, and keeping one rule is worth the unfamiliar sigil.

`null` exists and is the empty value. It is a literal rather than a variable, so
it cannot be shadowed and nothing can subscribe to it. `()` is the same value
under rhai's own name.

### Truthiness

**JavaScript's rules exactly.** Falsy: `0`, `NaN`, `""`, `null`, `()`. Everything
else is truthy, **including an empty array and an empty map**.

```rux
r-if="items"           <!-- "items exists", NOT "there are items" -->
r-if="items.length"    <!-- what you meant -->
```

This is a behaviour change from v0.6, and the only one v0.7 made to existing
documents.

## Operators

| Operator | Notes |
|---|---|
| `===` / `!==` | Mean what `==` / `!=` mean. Both spellings are the strict comparison; there is no loose `==` to choose between |
| `x++` / `x--` | Statement position only, desugared to `+= 1` / `-= 1` |
| `?.` | Guards an absent base **and a missing property** |
| `??` | The value on the right when the left is absent |
| `=>` | Arrow functions, in all four shapes: `x =>`, `() =>`, `(x) =>`, `(a, b) =>` |

`?.` and `??` are not conveniences. Rux turns on strict property access for every
document, so a typo like `user.nmae` raises instead of quietly evaluating to
nothing, and these two are the way to say "absent is a legitimate answer here":

```rux
{{ user?.nickname ?? "none" }}
```

## Built-in methods

JavaScript's names, on arrays and strings: `map`, `filter`, `reduce`, `forEach`,
`find`, `includes`, `indexOf`, `slice`, `split`, `join`, `charAt`, `repeat`,
`startsWith`, `endsWith`, `trim`, `toLowerCase`, `toUpperCase`.

All of them **return** a value and leave their receiver alone, as JavaScript's
do. That is worth saying because rhai's own string methods are in-place
mutators: its `trim` empties the string it was given and returns nothing, so
`{{ name.trim() }}` rendered blank. Rux's `trim` shadows it.

`forEach` is called with `(item, index)` and falls back to `(item)`.

**`.length` is a property, not `len()`.** Arrays and strings only: JavaScript has
no `length` on a plain object, and inventing one for maps would be making up a
rule rather than matching a known one. Use `keys(m)` and `values(m)` for maps.

## Talking to the outside

| Call | Does |
|---|---|
| `print(x)`, `debug(x)` | Printf-debugging. Reaches the **dev overlay**, not just stderr, because nobody running a GUI is watching stderr |
| `host::name(…)` | Calls into compiled Rust |
| `emit("name")`, `emit("name", payload)` | A component telling its caller something happened |
| `navigate(path)`, `replace(path)` | Router. `replace` is the only way to redirect: `navigate` leaves the redirecting page in the history, so Back returns to it and redirects again |
| `back()`, `forward()` | Walk the history |
| `path_for("route")`, `path_for("route", #{ id: x })` | Build a path from a named route |

**`log` is rhai's logarithm**, not a logging function. `log(2)` returns `0.301`.
Use `print`.

`emit`, `navigate` and the history verbs record an intent rather than acting
immediately. What they mean belongs to the runtime, and they are applied once the
handler and everything it set off have finished, so a handler that navigates and
then writes a signal does not render the old route with the new state in it.

## Reading the tree

**`query(selector)`** returns the elements matching a CSS selector, in document
order. It takes the same selectors the stylesheet takes, because it is the
stylesheet's own matcher: tags, ids, classes, `role`, and the `>`, `+` and `~`
combinators.

```rux
fn count() {
  report = query(".card").length + " cards";
}
```

**It works in a handler and nowhere else.** In a `{{ }}` binding, a `:style` or
an `r-if` it raises and the overlay says why. A binding that read the tree would
have to invalidate whenever layout changed, and invalidating it rebuilds and
relayouts, which never settles. Every real use of it is handler-shaped.

**It matches the tree, not the template.** A `<view r-if="open">` that is closed
is not there to be found. **A selector that does not parse is an error**, not an
empty list, so a typo cannot look like "nothing matched".

Each result carries:

| Property | Notes |
|---|---|
| `tag`, `id`, `classes` | `id` is absent rather than empty when there is none |
| `x`, `y`, `width`, `height` | The laid-out box, in absolute window pixels |

**Geometry is one frame stale**, exactly as `getBoundingClientRect` is in a
browser: a handler runs before the next layout, so it reads what the last one
produced. It is **absent rather than zero** when there is no frame to read, which
covers both a node hidden by `r-show="false"` and a document that has not been
laid out at all. Under `rux check` it is always absent, since checking runs with
no window and no GPU on purpose.

```rux
let card = query(".card")[0];
report = card.width ?? "not laid out yet";
```

## Acting on an element

None of these edits the tree, which is why they can exist at all. They move host
state, and the next build produces the tree it would have produced anyway.

| Call | Does |
|---|---|
| `el.tap()` | Presses the element as a finger would |
| `el.focus()` | Puts the caret in a text input |
| `blur()` | Drops focus entirely |
| `el.scrollIntoView()` | Scrolls the containing scroller until it is visible |

**`el.tap()` is the whole gesture, not a way to call the handler.** It presses the
centre of the element through the same dispatch a real pointer goes through, so
it runs the `@tap` handler, follows a `to=` link, toggles a checkbox or radio,
opens a `type="select"` dropdown, moves keyboard focus and puts the caret in a
text input. It cannot drift from a real press because it is one.

Two consequences follow from that and are worth stating:

- **It hit-tests.** The topmost element at that point wins, so tapping something
  covered by an open dropdown taps the dropdown, as a finger would.
- **An element with no box cannot be tapped**, and says so.

A tap runs a handler that can tap again; the chain is cut off after eight rounds,
the same bound as an `emit` chain, so a button that taps itself stops rather than
hanging the window.

**`focus()` is not a tap.** It only puts the caret in a text input: no handler,
no link, no toggle. Only a text input can take focus, because focus is keyed by
`r-model`, and asking anything else says so rather than half-focusing.

`blur()` is free-standing rather than a method, because there is only one focused
element; asking a particular one to blur would either do nothing or take focus
from something else.

## Errors

Every event handler is **compiled when the document loads**, so a syntax error in
a `@tap` is reported rather than waiting to be discovered by whoever taps it.
Handlers in branches that are not currently rendered are checked too, since a
false `r-if` is where a broken handler hides longest. It is syntax only: naming
an `r-for` local or a component's own state is a runtime lookup, not an error.

A failing expression is **reported, not swallowed**. It reaches the dev overlay
and `rux check`, and rhai's wording is translated into Rux's: a missing property,
an undefined variable, a missing function and a reserved word all say what they
mean in this language's vocabulary.

`print` output is kept apart from warnings. A leftover `print` is not something
wrong with the document and must not fail a build.

## Deliberately absent

- **Tree mutation.** No setting a property, no adding or removing children. A
  state change regenerates the affected tree from the template, so any such edit
  would be overwritten by the next patch, reconcile or rebuild, silently, at a
  moment decided by an unrelated handler. The tree is a function of state, and
  state is how it changes.
- **`setTimeout`.** Animation absorbs the cases behind it.
- **`setInterval`.** Deferred rather than declined, and it will not be spelled
  this way: a free-floating repeating timer is a one-line declaration that the
  app never idles again, which nobody notices on a desktop and which is the whole
  battery story on a phone. What fits is a repeating timer the runtime owns and
  ties to a component instance, cancelled when that instance goes away.
- **Bare `{ }` object literals.** See above.

## If you know rhai

Six things behave differently under the fork. All six are in
`crates/rux-rhai/DIVERGENCE.md` with the files they touch.

1. `?.` guards a missing **property**, not only an absent base.
2. `x++` and `x--` exist, in statement position.
3. Arrow functions.
4. **Every plain call captures the caller's scope**, which is upstream's opt-in
   `f!(…)` form made the default. Method calls do not, and clear the flag rather
   than raising.
5. **JavaScript truthiness**, including empty array and empty map being truthy.
6. A whole `f64` can index an array.

Outside the engine, and so not divergences: strict map properties, JS method
names, `.length`, `null`, `print`/`debug`, `===`/`!==`, and float division for
two integers.

## If you know JavaScript

The things most likely to surprise, in the order they usually do:

1. **`'x'` is a character**, not a string.
2. **Object literals are `#{ }`**.
3. **`.length` on arrays and strings only**, not on objects.
4. `let` declares state only at the top level of `<script>`; `signal()` marks it.
5. **A method call cannot see the surrounding scope.** Write `helper(thing)`
   rather than `thing.helper()` when it needs to.
6. `x++` is a statement, so `let a = x++` is not a thing.
7. There is no `undefined` distinct from `null`.
