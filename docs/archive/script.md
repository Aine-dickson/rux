# Script reference: removed text

## If you know rhai

removed 2026-11-06 from 07-script.md

Rux ran on a fork of rhai until 2026-09-26, and its script still reads like
rhai in most places. Nine things the fork changed carry over as the language's
own rules (the fork's record of them, `DIVERGENCE.md`, went with it and is in
the repository's history):

1. `?.` guards a missing **property**, not only an absent base.
2. `x++` and `x--` exist, in statement position.
3. Arrow functions.
4. **A component's function reads its instance's state where it runs**: the
   instance hands its state in with every call. This is what is left of the
   fork's rule that every plain call captured its caller's scope; `f!(…)` is
   gone, and a caller's other names are not reachable (see
   [Modules](#modules)).
5. **JavaScript truthiness**, including empty array and empty map being truthy.
6. A whole `f64` can index an array.
7. `{ a: 1 }` is a map, as `#{ a: 1 }` is.
8. **Type annotations**: `let n: int`, `fn f(x: T): R`, `(a: T) => …` and
   `type Name = …;` parse, and are checked.
9. `?[` guards a missing key and an index past the end, not only an absent
   base.

Outside the engine, and so not divergences: strict map properties, JS method
names, `.length`, `none`, `print`/`debug`, `===`/`!==`, and float division for
two integers.

## Built-in methods: rhai's string and array methods

removed 2026-11-06 from 07-script.md

A string's methods **return** a new string and leave their receiver alone, as
JavaScript's do: `name.replace("-", " ")` replaces the first `-` and gives the
result back. That is worth saying because rhai's own string methods were
in-place mutators: its `trim` emptied the string it was given and returned
nothing, so `{{ name.trim() }}` rendered blank, and its `replace` changed the
receiver, every match, and gave `none` back. Rux's shadow both.

`forEach` is called with `(item, index)` and falls back to `(item)`. `every`
and `findIndex` take a callback as JavaScript's do (`some` and `find` are
rhai's own and already match), and `sort((a, b) => a - b)` sorts by the
comparison and **returns** the sorted array, which rhai's in-place `sort` did
not. rhai's own
`for_each` is still there and is **not** the same thing: it hands the item over
as `this` and the *index* as the argument, so `items.for_each(|x| total += x)`
adds up the indexes. Write `forEach`.

## Functions: a method call and the surrounding scope

removed 2026-11-06 from 07-script.md

**A method call does not capture the surrounding scope.** `thing.helper()`
cannot reach `level`; `helper(thing)` can. Method dispatch passes its receiver by
reference, and the scope cannot be borrowed at the same time. This is upstream's
limitation and it stands.

(No longer true on Rux's interpreter: `thing.helper(a)` is `helper(thing, a)`,
and a function sees its own file's names.)

## Lifecycle and truthiness: dated notes

removed 2026-11-06 from 07-script.md

Until 2026-10-13 neither
ran on a close or a hot reload, whatever this page said.

This is a behaviour change from v0.6, and the only one v0.7 made to existing
documents.

`@tap="drain()"` is a handler like any other and the write is tracked. **This
changed in v0.7**: before it, a `fn` could not touch state at all and every
handler had to be written inline, which is why older examples and `/learn` still
show them that way.

## Native code: debug build measurement and retired host::name()

removed 2026-11-06 from 07-script.md

A debug window scrolls as fast as a release one (a list of 1,000
rows: 40 wheel notches took 2.5 s of CPU before, 0.33 s now)

`host::name()`, which reached a function a Rust program registered by hand, is
retired and is an error that points here.
