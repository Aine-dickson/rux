# The Rux language

The reference for the script language Rux owns: what goes inside `<script>`,
in a script-only module, and in a `{{ }}` binding or an `@tap` attribute.

**Status: the reference for the language being built.** Steps 1 to 8 of the
[build order](#build-order) are done (2026-09-27): Rux's own parser, checker,
typed IR and interpreter run every script, rhai is gone, and `async fn`,
modules and stores, and native modules run. Rust generation (step 9) does not
run yet, and neither do some of the Changed rules below; the build order says
which. [Script](./07-script.md) and [Types](./10-types.md) describe what
runs today. This document describes the language that replaces them: it keeps
what those two built where it still holds, and marks every place that moves.
When the build finishes, this becomes the only script reference and those two
retire into history.

The direction behind it (Rux's own parser, checker, typed IR and interpreter,
Rust code generation for release, no rhai) is recorded in
[Architecture](#architecture) at the end, with the build order.

## How to read the marks

Every section and most rules carry one of three marks:

| Mark | Means |
|---|---|
| **Kept** | Ran on the fork, runs today, and stays as it is. The section in [Script](./07-script.md) or [Types](./10-types.md) it comes from still describes it |
| **Changed** | Runs today, differently. What it was is said beside what it becomes |
| **New** | Does not exist today |

And one of two for who took it:

| Mark | Means |
|---|---|
| **(decided)** | The project owner took it |
| **(proposed)** | Delegated, or filled in while writing this reference. Stands unless overruled |

A rule with no decision mark is Kept, and was decided when it was built.

## Principles (decided)

1. **Rux owns the language; Rust is the substrate.** Rux types are the
   contract a Rux author sees. Rust types are how the compiler represents them.
   A Rux author never needs ownership, lifetimes, traits or `Arc`.
2. **Borrow famous syntax, exactly, and only where it earns its place.**
   `async`/`await`, `try`/`catch`, `<T>`, `import`, `reduce(fn, init)` are
   taken as the world already knows them. Rux is not "JavaScript compiled to
   Rust": it keeps `signal`, `let`, `mounted { }` and its template language.
3. **One way to say a thing.** Where two spellings would mean the same, one is
   chosen. The named exceptions are `use`/`import`, `T[]`/`Array<T>` and
   `{ [string]: T }`/`Map<string, T>`.
4. **The boundary to Rust is typed and costs nothing at run time.**
5. **Types are checked, not erased.** Every expression has a type the compiler
   knows, and the same meaning in the interpreter and in a compiled build.

## The three places script runs (Kept)

| Where | Example | Runs |
|---|---|---|
| The top-level script | `let n = signal(0);` | Once, when the document or instance loads |
| A binding | `{{ n * 2 }}`, `:class`, `r-if`, `r-for` | On every build, possibly many times a second |
| A handler | `@tap="bump()"` | When something happens |

A **binding is an expression that must not do anything**: it may run any number
of times. A **handler is one or more statements**, and may write state, call
functions, navigate and read the tree. Both are parsed by the same grammar as
`<script>`, a binding as one expression and a handler as a statement list.

## Lexical structure

### Comments and whitespace (Kept)

`// to the end of the line` and `/* a block */`. Whitespace and newlines
separate tokens and mean nothing else. A statement ends with `;`, which may be
left off before a `}` and at the end of a handler or a binding.

### Names

A name is a letter or `_` followed by letters, digits and `_`. Names are case
sensitive.

- **A declared type's name starts with a capital letter** (Kept). The built-in
  types are lower case, so the two never collide.
- **Everything else is camelCase by convention**, and every built-in name is
  (Changed, proposed): `pathFor`, not `path_for`. See
  [Names that change](#names-that-change).

### Keywords (Changed)

Reserved everywhere:

```text
let fn async await return if else while for in break continue switch
try catch throw true false none type use import export from as is
```

Reserved only at the start of a top-level statement, so they stay usable as
ordinary names elsewhere (`let type = 1;` keeps working, as it does today):

```text
signal computed effect mounted unmounted prop native
```

`null` is no longer a keyword (Changed, decided): see [The empty value](#the-empty-value).
`const`, `var`, `class`, `interface`, `function`, `new` and `this` are reserved
and are errors that name the Rux spelling, so a habit from JavaScript is
answered rather than misread (New, proposed).

### Literals

| Literal | Type | Mark |
|---|---|---|
| `0`, `42`, `1_000` | `int` | **Changed**: was `number` |
| `0.5`, `1.0`, `2e3`, `1_000.5` | `float` | **Changed**: was `number` |
| `0x1F`, `0b101` | `int` | Kept |
| `"text"` | `string` | Kept |
| `'text'` | `string` | **Changed** (proposed): was a single character |
| `` `a ${x} b` `` | `string` | Kept |
| `true`, `false` | `bool` | Kept |
| `none` | `T?` for whatever `T` is expected | **Changed** (decided): was `null` and `()` |
| `[1, 2]` | `int[]` | Kept |
| `{ id: 1, name: "ada" }` | a record | Kept |

**`'x'` is a string.** Rhai's character literal was the single most common
thing authors tripped over, and the new parser has no reason to keep it. Both
quotes mean the same, which is what lets a handler inside a double-quoted
attribute hold text: `@tap="query('#note')[0].focus()"` now works. There is no
character type.

Escapes in all three string forms: `\n`, `\t`, `\\`, `\"`, `\'`, `` \` ``,
`\$`, `\u{…}`.

**A number literal has no suffix** and no leading `+`. `-1` is the unary minus
applied to `1`.

### Map and record literals (Kept, one change)

`{ key: value }`, with a name or a string as the key (`{ "x-y": 1 }`), and `{}`
for an empty map. A `{` whose first thing is not `key:` is a block, as after
`if`. No shorthand (`{ a }`) and no spread.

**`#{ … }`, rhai's spelling, is removed** (Changed, proposed). `rux fmt` has
rewritten it to `{` since 2026-09-24, and it runs once more as part of the
move.

## Types

### The shape of it (Kept)

```rux
<script>
  type Filter = "all" | "open" | "done";
  type Task = { id: int, title: string, done: bool, note?: string };

  let tasks: Task[] = signal([]);
  let filter: Filter = signal("all");

  fn visible(): Task[] {
    switch filter {
      "all"  => tasks,
      "open" => tasks.filter(t => !t.done),
      "done" => tasks.filter(t => t.done),
    }
  }
</script>
```

Annotations are TypeScript's: `name: T` after a name, `): T` after a parameter
list (decided, kept). On a signal the annotation describes the value, not a
wrapper: there is no `Signal<T>` to write.

### The types

| Type | Holds | Mark |
|---|---|---|
| `int` | A 64-bit signed integer | **Changed** (decided): was a subtype of `number` |
| `float` | A 64-bit float | **New** (decided) |
| `number` | Retired. An error naming `int` and `float` | **Changed** (decided) |
| `string` | Text | Kept |
| `bool` | `true` or `false` | Kept |
| `void` | The result of a function that returns nothing | **New** (decided) |
| `any` | Anything, checked when the program runs | **Changed** (decided): was "unchecked" |
| `"all"` | Exactly that string | Kept |
| `T[]`, `Array<T>` | An array of `T`; two spellings of one type | **Changed** (decided): `Array<T>` was refused |
| `{ id: int, title: string }` | A record | Kept |
| `{ note?: string }` | A record whose `note` may be absent | Kept |
| `{ [string]: T }`, `Map<string, T>` | A map from text to `T`; two spellings of one type | **Changed** (decided): `Map` is new |
| `Map<K, V>` | A map with keys of `K` (`int`, `string` or a string literal union) | **New** (decided) |
| `Set<T>` | Distinct values of `T` | **New** (decided) |
| `T?`, `Option<T>` | `T` or `none`; two spellings of one type | **Changed** (decided): was `T \| null` |
| `Result<T, E>` | A value or an error, kept on purpose | **New** (decided); see [Result](#result) |
| `A \| B` | Either | Kept |
| `(Task, int) => bool` | A function value | Kept |
| `Error` | What `catch` binds: `{ message: string, kind: string }` | **New** (decided) |
| `Task`, `Page<T>` | A declared or imported type | **Changed** (decided): may take parameters |

Not in the first version, and errors if written (Kept unless marked): number
and boolean literal types, intersections (`A & B`), tuples, anything
class-shaped, `interface` (decided), explicit type arguments at a call, `extends`
constraints, conditional and mapped types (decided).

### Declaring a type (Kept, one change)

`type Name = …;` at the top level of `<script>` or of a module, before or after
its first use. It declares nothing at run time.

**A type may take parameters** (New, decided):

```rux
type Page<T> = { items: T[], next: string? };
type Loaded<T> = { state: "loading" } | { state: "done", value: T };
```

A union may begin with `|` so a long one lines up (Kept).

### Numbers (Changed)

- **An integer literal is an `int`; a literal with a point or an exponent is a
  `float`** (proposed).
- **An `int` widens to a `float`** wherever a `float` is expected, and when the
  two meet in arithmetic: `1 + 0.5` is a `float` (proposed). A `float` never
  narrows to an `int` by itself; that takes a conversion.
- **`/` always gives a `float`**: `7 / 2` is `3.5` (proposed). This is today's
  behaviour, now said by the type. `intDiv(7, 2)` is `3`, truncating toward
  zero (proposed).
- `+`, `-`, `*` and `%` of two `int`s are an `int`; with a `float` on either
  side they are a `float`. `**` is power, and of two `int`s is an `int`
  (decided 2026-09-27): exact, or an `overflow` throw, never a rounded
  `float`. A negative power of an `int` throws when it runs; one written out
  (`2 ** -1`) is a checker error that says to make the base a `float`
  (`2.0 ** -1`). A `float` on either side makes a `float`.
- **`int` arithmetic that overflows throws** an `Error` with `kind`
  `"overflow"`, in the interpreter and in a compiled build alike (New,
  proposed). Rust's release default is to wrap silently, and a UI showing a
  wrapped negative total is worse than an error the overlay names.
  `intDiv(x, 0)` and `x % 0` on `int`s throw with `kind` `"division"`.
- **`float` keeps IEEE behaviour** (Kept): `1.0 / 0` is `Infinity`, `0.0 / 0`
  is `NaN`, and both display under those names.
- **Converting** (Changed, proposed): `x.toFloat()` on an `int`;
  `x.trunc()`, `x.round()`, `x.floor()`, `x.ceil()` on a `float` give an
  `int`, and throw with `kind` `"overflow"` for `NaN`, an infinity, or a value
  out of range. `to_int()` becomes `trunc()`.
- **Text to a number** (Changed, proposed): `parseInt(s): int?` and
  `parseFloat(s): float?` read a leading number as JavaScript's do and give
  `none` where JavaScript gives `NaN`. `Number(s)` is retired in favour of
  those two (one way to say it), and `isNaN(x)` stays for `float`s.
  `String(x)` goes the other way.

Why an `int` literal and not today's `number` literal: today an integer literal
is a `number` so that `let n = signal(0); n = n / 2;` is not an error. Under the
new rules it still is not, if `n` is declared as what it is:
`let n: float = signal(0);`. Without the annotation, `n` is an `int` and
`n = n / 2` is an error whose fix is that annotation.

### The empty value (Changed, decided)

**`none`**, not `null`. `T?` is the short spelling of `Option<T>`, as in Swift
and Kotlin, so everything built on `null` keeps working with the new word:

- `?.` and `?[` read through a value that may be `none` (Kept).
- `??` gives the right side when the left is `none` (Kept).
- Narrowing by `x != none`, `if x`, an early return, `r-if` (Kept).
- A prop declared `T?` that the tag passed nothing to is `none` (Kept).

There is no `undefined` and no `()`. `none` is a literal, not a variable: it
cannot be shadowed, and nothing can subscribe to it (Kept). `null` and `()`
are checker errors that name `none` (a warning from step 3 until step 5); they
still run as `none`, and `rux fmt` rewrites them.

### Checked, not erased (Changed, decided)

Today annotations are erased and only props and `is` are checked at run time.
In the new language every expression has a type the checker knows, and the
interpreter and the compiled build both rely on it.

- **A name whose type cannot be worked out is an error in a release build**
  and a warning in the interpreter (decided). The warning is what keeps hot
  reload working on a half-finished file. Writing `: any` opts out.
- **`any` is the one dynamic type.** A value of it is checked when it is used:
  reading a field that is not there, calling what is not a function, or
  handing an `any` to a place that wants an `int` and getting a string, throws
  an `Error` with `kind` `"type"` (Changed, decided).
- **Assigning an `any` to a typed name is checked at that point** (Changed,
  proposed). Today it is allowed unchecked because checking cost something on
  every assignment. Under checked types it is the one place an unknown value
  can enter typed code, so it is the place to check it; `is` remains the way to
  ask without throwing.

### Inference (Kept, with the new primitives)

| Written | Inferred |
|---|---|
| `signal(0)`, `42` | `int` |
| `signal(0.0)`, `1.5` | `float` |
| `signal("")`, `"all"` | `string`: a literal widens |
| `true` | `bool` |
| `[1, 2]` | `int[]` |
| `{ id: 1, name: "ada" }` | `{ id: int, name: string }` |
| `items.length` | `int` |
| `tasks.filter(t => t.done)` | `Task[]` when `tasks` is a `Task[]` |
| a call to a function | what that function returns |
| a call to a generic function | its result with the type parameters filled in from the arguments |

A string literal stays a literal type only where something asks for one
(Kept). `signal([])`, `signal(none)`, `signal({})` and an unannotated parameter
say nothing, so they are the places an annotation is needed (Kept, now an
error in release builds).

**A closure's parameters come from where it is used** (Kept): in
`tasks.filter(t => t.done)`, `t` is a `Task`. An arrow that is not an argument
needs its parameters annotated.

### Optional fields, `?.` and narrowing (Kept)

Everything in [Types, optional fields](./10-types.md#optional-fields-and-) and
[unions and narrowing](./10-types.md#unions-and-narrowing) holds, with `null`
read as `none`:

- An optional field, a value that may be `none`, and a dictionary key must be
  read with `?.` or `?[`, except inside a region where a check has ruled their
  absence out.
- These narrow: `x == "open"`, `x != none`, `if x`, `load.state == "done"`,
  `"note" in t`, `x is Task`, an early `return`, each arm of a `switch`, and
  `r-if`/`r-elif`/`r-else` in a template.
- A fact about a signal does not outlive a call that writes it, nor an `await`.
- Comparing against a literal a union does not contain, or between types with
  nothing in common, is an error.

**`type_of(x) == "string"` is retired** (Changed, proposed) in favour of
`x is string`, which says the same thing in the checker's own words and has no
tag strings to misspell (`"f64"`, `"()"`).

### `switch` (Kept)

`switch value { a => …, b => …, _ => … }` is an expression. On a union-typed
value with no `_` arm it must name every member, and an error lists the ones
left out. A `switch` on a discriminant narrows the value in each arm. An arm
may be a block: `"done" => { … }`.

### `is` (Kept)

`x is T` checks at run time and narrows. It walks the value against the type,
never throws, and binds as `<` does. A validator is generated only for the
types that appear on the right of an `is`, and for props.

### Result (New, proposed)

`Result<T, E>` is for a value an author keeps on purpose, where an error is an
answer rather than an exception. It is a union with a discriminant, so it is
read with the narrowing that already exists and needs no pattern syntax:

```rux
// Result<T, E> is { ok: true, value: T } | { ok: false, error: E }
fn parseAge(s: string): Result<int, string> {
  let n = parseInt(s);
  if n == none { return Err("not a number"); }
  if n < 0     { return Err("negative"); }
  Ok(n)
}

let r = parseAge(input);
if r.ok { age = r.value; } else { problem = r.error; }
```

`Ok(v)` and `Err(e)` build one. `r.unwrap()` gives the value or throws the
error as an `Error` (`kind` `"result"`, or the error itself when `E` is
`Error`).

## Statements and expressions

### Precedence (Kept, written down for the first time)

Tightest first. This is the fork's table, which was never documented; Rux's
parser (step 2) implements it exactly, and a debug-build check compares the
two on every script the test suite compiles.

| Level | Operators | Associates |
|---|---|---|
| 1 | calls `f()`, `.x`, `?.x`, `[i]`, `?[i]` | left |
| 2 | unary `!`, `-`, `+`, and `await` (New) | right |
| 3 | `**` | right |
| 4 | `*`, `/`, `%` | left |
| 5 | `+`, `-` | left |
| 6 | `..`, `..=` | left |
| 7 | `??` | left |
| 8 | `<`, `<=`, `>`, `>=`, `is` | left |
| 9 | `in` | left |
| 10 | `==`, `!=` | left |
| 11 | `&&` | left |
| 12 | `\|\|` | left |
| 13 | `=>` | right |

Two things differ from JavaScript and are kept: **`??` binds tighter than a
comparison**, so `a ?? 0 == 1` is `(a ?? 0) == 1`, and **`in` binds looser
than one**, so `a < b in c` is `(a < b) in c`.

**One quirk of the fork's is gone** (Changed, done in step 3.5). In the fork,
`is` bound as `<` does on its left but had no precedence on its right, so
`a == b is int` was `(a == b) is int`, where the table says
`a == (b is int)`. `is` now has its level on both sides; the fork is handed
`__is(x, "T")` and never parses it.

**Assignment is a statement**, not an operator (Kept): `a = b = c` is an error.
`=`, `+=`, `-=`, `*=`, `/=`, `%=`, and `x++`/`x--` in statement position only.
The target is a name, a field (`a.b`), or an index (`a[i]`), to any depth.

**Removed** (Changed, proposed):

- **`===` and `!==`**. Today they mean what `==` and `!=` mean; with one way to
  say a thing, `rux fmt` rewrites them. `==` compares values structurally:
  two records are equal when their fields are.
- **Bitwise operators** (`&`, `|`, `^`, `<<`, `>>`) and rhai's non-short-circuit
  `&`/`|` on booleans. Nothing in the repository uses them. `|` stays only in
  types, where it means a union.
- **Rhai's `|x| body` closures**, in favour of arrows. `rux fmt` rewrites them;
  `/learn` chapter 4 and 5 use them today.

There is **no `?:` ternary** (Kept): `if` is an expression,
`if a { b } else { c }`.

### Truthiness (Kept)

JavaScript's rules. Falsy: `false`, `0`, `0.0`, `NaN`, `""`, `none`. Everything
else is truthy, **including an empty array and an empty map**. The checker
warns on a condition whose type is an array or a map, since it is always true
(New, proposed): `r-if="items"` almost always meant `items.length`.

### Blocks have values (Kept)

A block's value is its last expression when that expression has no `;` after
it. That is how a function returns without `return`, and how a `switch` arm or
an `if` produces a value:

```rux
fn label(t: Task): string { t.title }
let size = if wide { 24 } else { 16 };
```

### Control flow (Kept, one addition)

`if … { } else if … { } else { }`, `while cond { }`, `loop { }`,
`for x in c { }`, `for (x, i) in c { }` (item and index), `for i in 0..n { }`
and `0..=n`, `break`, `continue`, `return`, `return value`.

**`loop { }` runs until a `break`** (decided 2026-09-27): clearer than
`while true`. `do { } while`, `do { } until` and `break value` stay out, and
there is no C-style `for` (decided).

`for x in c` iterates whatever the compiler knows how to iterate: an array, a
range, a string's characters (as one-character strings), a `Set`, and a map's
keys through `keys(m)`. No iterable protocol is visible to authors (decided).
`r-for` has no index form (Kept).

### Errors: `try`, `catch`, `throw` (New, decided)

```rux
fn save() {
  try {
    native::store::write(draft);
    status = "saved";
  } catch e {
    status = "could not save: " + e.message;
  }
}
```

- **`catch e` binds an `Error`** (decided): `type Error = { message: string,
  kind: string }`. `catch { }` without a name is allowed (proposed).
- **`throw` takes a `string` or an `Error`** (proposed): `throw "empty title"`
  throws `{ message: "empty title", kind: "error" }`; `throw e` rethrows.
- There is no `finally` in the first version (proposed). It can be added
  without changing anything written before it.
- **What throws:** `throw`, a native function whose Rust `Result` is `Err`
  (its `kind` names the Rust error type), a failed `await`, a checked `any`
  that does not fit, `int` overflow, `r.unwrap()` on an error, and the
  run-time failures that raise today (a missing field read without `?.`, an
  index past the end read without `?[`).
- **An error nothing catches** is reported to the dev overlay and `rux check`
  exactly as a failing handler is today (Kept), with the file and line. The
  handler stops there; its writes before the error stay.

## Functions

### Declaring (Kept, with generics)

```rux
fn label(t: Task): string { t.title }
fn first<T>(items: T[]): T? { items?[0] }     // New (decided)
fn refresh() { load = native::tasks::load(); }
```

- **The result type is written `: T`** (decided) and may be left off, in which
  case it is inferred from the body. It must be written on a function that
  calls itself, directly or through another (Kept).
- **Parameters are never inferred from calls** (Kept). An unannotated
  parameter is `any` with a warning in the interpreter, and an error in a
  release build (Changed, decided). The editor's quick-fix writes in the type
  the calls agree on.
- **Type parameters are always inferred** at the call, from the arguments
  (decided). `first<User>(x)` is not a call form.
- A generic function is compiled once per type it is used with (decided).
- Several functions may not share a name, and there is no overloading by
  arity (Changed, proposed). Rhai allowed `fn f(a)` and `fn f(a, b)` side by
  side; one name meaning two things is exactly what one-way-to-say-it rules
  out.

### What a function can see (Changed)

**A function sees its own parameters and locals, and the names of the file it
is written in**: the document's or component's signals, computeds, props and
functions, and what the file imports. Nothing else (proposed).

This is today's rule for **typed** functions made the only rule. Two things
from the fork go:

- **A call no longer runs in its caller's scope** (fork divergence 4). An
  untyped function can read a caller's `let` or `r-for` variable today; that
  cannot be typed, and in the new language every function is typed. When the
  typed rule was switched on (2026-09-24), a survey of every function in the
  repository's examples, recipes, `/learn` and the apps built with Rux found
  none that relied on it.
- **A method call can reach the file's names** (upstream limitation removed):
  `thing.helper()` and `helper(thing)` see the same scope.

**Writing a signal from a function is unchanged** (Kept): `fn drain() { level-- }`
writes the document's `level`, and the screen follows.

### Arrows and closures (Kept, one change)

`x => …`, `() => …`, `(x) => …`, `(a, b) => …`, and `(a: int, b: int): int => …`
with types. An arrow in a variable is called like a function. A `fn` of the
same name as a variable is an error (Changed, proposed; today the `fn` wins
silently).

**A closure captures the names around it by reference** (Changed, proposed).
Today a closure writes to its own captured copies and cannot move a signal,
which is why an interval's body is a block. In the new interpreter a signal is
the document's state, not a copy, so a closure that writes one writes the real
one, whenever it runs: `items.forEach(i => total += i.price)` adds to `total`.
A captured local that the closure outlives keeps its last value.

## State and the file's own declarations

### `signal`, `let` (Kept, one change)

```rux
let level = signal(82);
let items: Item[] = signal([]);
```

`signal(v)` marks a top-level `let` as reactive state. A binding that reads one
is a subscription to it (Kept). There is no `const` (decided).

**A top-level `let` without `signal` is a value that is never written** (Changed,
proposed; on since 2026-09-26). Until then `signal()` was identity and a plain
top-level `let` was state anyway, so the marker marked nothing. Making the
unmarked form read-only gives `signal` a meaning without adding `const`: writing
one, or a field or element of one, is a checker error that suggests
`signal(…)`. Before it was switched on, every `.rux` file on the development
machine (359) was checked for such a write, as the caller-scope rule was, and
none had one.

A `let` inside a function, handler or block is an ordinary local (Kept).

### `computed`, `effect` (Kept)

`computed total: float = items.reduce((s, i) => s + i.price, 0.0);` declares
derived state, refreshed in declaration order. `effect { … }` runs when what it
last read changes, and once on load. Both run per instance inside a component.

**They are part of the grammar now** (Changed, invisible): today a line
preprocessor pulls them out before rhai sees the script, so each had to sit on
the line it starts on. Parsed properly, a `computed` may span lines, and an
error in one points at its own column.

### `prop` (Kept)

```rux
prop label: string;
prop price: float = 1.0;
prop kind: "primary" | "quiet" = "quiet";
```

Declared in a component. Required without a default, optional with one. Not
writable from the component. Checked at the tag by the checker and when the
component is built, in release builds too; a route parameter becomes the type
its prop takes. See [Types, props](./10-types.md#props).

### Lifecycle: `mounted`, `unmounted` (Kept, decided)

Blocks, not functions. `mounted` runs once after the first tree exists and
after the effects; `unmounted` when the document or instance goes. Per
instance inside a component. `unmounted` never runs without `mounted` having
run, and unmounts run before mounts in one build. See
[Script, lifecycle](./07-script.md#lifecycle).

### Intervals (Kept)

`let t = setInterval(1000) { … }` and `clearInterval(t)`. The handle is an
`int`. An interval belongs to whoever started it and stops with that instance.
It stays a block rather than a callback, though a closure could now write a
signal: the block shape is what ties it to its instance, and it was decided on
those grounds.

## Async (New, decided)

```rux
async fn loadUser(id: int): User {
  let user = await native::api::getUser(id);
  return user;
}
```

- **`async fn` and `await`, and nothing else.** No `Future`, `Promise` or task
  type is ever visible to an author (decided).
- **The result type is the value awaited** (proposed): `: User`, not a wrapper.
  `async` already says the function waits.
- **`await` is allowed only inside an `async fn`** (decided). Handlers,
  bindings, `effect`, `computed` and the lifecycle blocks cannot await.
- **A handler only calls an async function** (decided): `@tap="loadUser(3)"`
  starts it and returns at once. Its writes render as they happen, before and
  after each `await`. `mounted { loadUser(3); }` starts one on load.
- **An async fn may call another with `await`**, and gets its result. Calling
  one without `await` from inside an async fn starts it alongside and does not
  wait (proposed).
- **Everything between two `await`s runs without interruption** (proposed).
  There is one thread of script, so no lock is ever needed; a signal can change
  under a function only at an `await`, which is why an `await` ends every
  narrowing of a signal (Kept, from [Types](./10-types.md#unions-and-narrowing)).
- **An async fn started by a component instance belongs to it** (proposed), as
  an interval does. When the instance unmounts, the function is stopped at its
  next `await` and never resumes, so a late answer cannot write into a
  component that is gone. One started at document level lives as long as the
  document.
- **A failed `await` throws** an `Error` where the `await` is (decided), which
  `try`/`catch` handles. One nothing catches is reported as a failing handler
  is.

## Modules (New, decided)

### Two spellings, one meaning (decided)

```rux
use components::card;                    // a whole file, under its name
import card from "./components/card";

use stores::cart as basket;              // a whole file, renamed
import basket from "./stores/cart";

use utils::money::{format, tax as t};    // names the file exports
import { format, tax as t } from "./utils/money";

use type types::Task;                    // a type
use type types::{Task, Page};
import type { Task, Page } from "./types";
```

`use` and `import` are both accepted and mean the same (decided). A name
alone imports the **whole file**: a component's tag, or a module's namespace
(`cart.add(p)`), as Vue imports a component. **Braces pick what the file
exports** (decided 2026-09-26, step 7), in both spellings. Without braces the
last segment of a `use` is always the file; `use utils::money::format` looks
for `utils/money/format.rux`.

**A type is imported with `type`** (decided by the project owner, 2026-09-26),
never told by its capital letter: `use type a::T`, `import type { T } from
"./a"`, and `import type T from "./a"`, which is read as the braced form
since a type import always picks. A type imported without `type`, or a
`use type` of something that is not a type, is an error. The earlier
spelling, `use types::Task;`, still loads with a warning until step 7 ends,
and `rux fmt` rewrites it.

The string after `from` is the `use` path with `/`: an optional `./`, names,
an optional `.rux`. Resolution is Kept: beside the importing file first, then
the project root. No `super::`, and no `..` or `.` in a path. `rux fmt` can
rewrite every import to one form, chosen in `rux.toml` (decided):

```toml
[fmt]
imports = "use"      # or "import"
```

Without the setting it leaves both alone. Imports and `export` belong at the
top level of a file's script; a handler or a binding cannot hold one.

### Script-only modules (decided)

A file with a `<script>` and no `<template>` is a module: functions, types and
state, no view. It may be written as a bare `.rux` file with no `<script>` tag
at all (decided 2026-09-26), since there is nothing else it could hold.

- **`export` makes a declaration visible to importers** (decided):
  `export fn`, `export async fn`, `export type`, `export let`. Anything not
  exported is private to the module.
- A types file is a module that exports only types (Kept, as the `types.rux`
  files that exist today, which now need `export` on each type; `rux fmt`
  adds it, and a type imported that a module does not export is a warning
  until step 7's survey is clean, then an error).
- A module is loaded once per document, however many files import it, and
  its top-level statements run then, before the document's, each module
  after the ones it imports. Modules importing each other in a circle are
  refused (decided).
- A module holds no `computed`, `effect`, lifecycle block or `prop`
  (decided): those belong to a component or a document.

### Stores (decided 2026-09-26, the shared half of the owner's "Pinia-like" intent)

**A module's top-level signals are shared state.** Every importer sees the
same instance. That is a store, with no new keyword:

```rux
// stores/cart.rux
import type { Product, CartItem } from "./types";

export let items: CartItem[] = signal([]);

export fn add(p: Product) { items.push({ product: p, qty: 1 }); }
export fn total(): float {
  items.reduce((s, i) => s + i.product.price * i.qty.toFloat(), 0.0)
}
```

- **An exported signal is readable by importers and writable only inside its
  module** (decided). A binding that reads `cart.items` subscribes to it like
  any signal. Changing it from outside goes through the module's exported
  functions, which is what makes a store's rules live in one place.
- A component's own state is still changed only inside the component (Kept).
- **A component's names are its own** (decided, step 7.4): what it shares
  with the document or another component is a store, never the document's
  signals or functions.
- **An `async fn` a module exports belongs to the document** when it runs
  (decided): it writes only the module's state, which outlives any caller.
- A factory that gives each caller its own copy (the "scoped" half) can come
  later; what "scoped" means is the owner's to say.

### Values move by copy (Kept, stated for the first time)

Arrays, maps and records are **values**, as they are today under rhai:
`let b = a; b.push(1);` leaves `a` as it was, and a record handed to a function
is that function's own copy. This is the rule JavaScript does not have, and it
is what makes a signal's writes visible: the only way to change state is to
write to its name, directly or through a field or an index, and every such
write is tracked. The compiled build keeps the same meaning with shared,
copy-on-write storage, so a copy that is never written costs nothing.

A **resource** from native code (see [Native code](#native-code)) is the one
thing passed by handle rather than by copy.

## Collections and the standard library

### Rux owns the API (decided)

The methods of Rux's built-in types are Rux's. Rust's are never inherited, so a
Rust release cannot change what `Array` offers. **One name per operation,
JavaScript's camelCase** (proposed), and methods that return a new value leave
their receiver alone (Kept).

### Arrays

| Method | Mark |
|---|---|
| `.length` (a property, `int`) | Kept |
| `map`, `filter`, `find`, `findIndex`, `some`, `every`, `forEach`, `includes`, `indexOf`, `slice`, `join` | Kept |
| `reduce(fn, init)`, JavaScript's order | Kept (decided) |
| `sort((a, b) => …)`, returns the sorted array | Kept |
| `push(x)`, `pop(): T?`, `splice(i, n)`, `insert(i, x)` | **Changed** (proposed): in place, and a write to the array's name. `pop` gives `none` on an empty array. `remove(i)` becomes `splice(i, 1)` |
| `concat(other)`, `reverse()`, `flat()` | **New** (proposed), each returns a new array |
| `a + b` | Kept: concatenation |

**`.len()` is an error with a quick fix to `.length`** (decided). The same for
every Rust or rhai name with a Rux equivalent. `forEach` is called with
`(item, index)` or `(item)` (Kept); rhai's `for_each`, which handed over the
index as the argument, is gone.

### Strings

`.length`, `charAt`, `includes`, `indexOf`, `slice`, `split`, `startsWith`,
`endsWith`, `trim`, `toLowerCase`, `toUpperCase`, `repeat`, `replace(a, b)`
(every occurrence) (Kept, `replace` proposed). Strings are immutable; every
method returns a new one (Kept: rhai's in-place `trim` is already shadowed).

### Maps and sets (New, decided in principle)

A `Map<string, T>`, which is also `{ [string]: T }`: `m[k]` (read with `?[`
unless `k in m` is known), `m[k] = v`, `k in m`, `keys(m)`, `values(m)`,
`m.size`, `m.delete(k)` (proposed).

A `Set<T>`: `Set([a, b])`, `s.has(x)`, `s.add(x)`, `s.delete(x)`, `s.size`,
iterated with `for x in s` (proposed).

**`.length` is on arrays and strings only** (Kept); a map has `size`, as a
JavaScript `Map` does.

### Everything else global (Kept unless marked)

| Call | Does |
|---|---|
| `print(x)`, `debug(x)` | Reaches the dev overlay. Not a warning; never fails a build |
| `emit("name")`, `emit("name", payload)` | A component telling its caller something happened |
| `navigate(path)`, `replace(path)`, `back()`, `forward()` | The router; recorded, applied after the handler |
| `pathFor("route", { id: x })` | **Changed** (proposed): was `path_for` |
| `query(selector)` | The tree, in a handler only; see [Script, reading the tree](./07-script.md#reading-the-tree) |
| `el.tap()`, `el.focus()`, `blur()`, `el.scrollIntoView()` | Act on an element |
| `setInterval(ms) { }`, `clearInterval(h)` | Intervals |
| `intDiv(a, b)`, `parseInt`, `parseFloat`, `isNaN`, `String` | Numbers and text; see [Numbers](#numbers) |
| `abs`, `min`, `max`, `sqrt`, `sin`, `cos`, `atan2`, `exp`, `ln`, `log10` | **Changed** (proposed): typed maths, `int` in and out where that makes sense (`abs`, `min`, `max`). `log` is removed: rhai's `log` was base 10 and read as logging |
| `host::name(…)` | **Changed** (decided): retired, see [Native code](#native-code) |

### Names that change

`rux fmt` rewrites every mechanical one, so an app moves with one command
(decided).

| Today | Next | How |
|---|---|---|
| `null`, `()` | `none` | fmt |
| `number` | `int` or `float` | by hand, the checker says which |
| `#{ … }` | `{ … }` | fmt |
| `===`, `!==` | `==`, `!=` | fmt |
| `\|x\| body` | `x => body` | fmt |
| `.len()` | `.length` | fmt, and a quick fix |
| `.to_int()` | `.trunc()` | fmt |
| `.to_string()`, `Number(s)` | `String(x)`, `parseFloat(s)` | fmt; `parseFloat` gives `float?`, which the checker then asks to handle |
| `type_of(x) == "string"` | `x is string` | fmt for the literal tags it knows |
| `path_for` | `pathFor` | fmt |
| `.remove(i)` | `.splice(i, 1)` | fmt |
| `host::f()` | a `native` module | by hand, when a module maps directly fmt |
| `'x'` meaning a character | a string | nothing to do |
| `type T = …` in a types file | `export type T = …` | fmt |

## Native code

### Reaching it (decided shape, keyword built 2026-09-27)

The keyword is **`native`**, the same in both module forms:

```rux
use native::database;
import { database } from "native/database";
```

`host::` is retired (an error that points here). "Native" says what the thing
is: implemented in Rust, not in Rux. A native module is a `pub mod` of the
app's `native/` crate: `pub mod shop` is `native::shop`, and the crate root is
`native` itself. The type import is `use type native::shop::Product;`, as for
a script module.

### Exporting from Rust (decided in principle, built 2026-09-27)

```rust
#[rux::export]
pub struct Product { pub id: u64, pub name: String, pub price: f64 }

#[rux::export]
impl Product {
    pub fn discounted_price(&self, off: f64) -> f64 { self.price * (1.0 - off) }
}

#[rux::resource]
pub struct Database { pool: Arc<sqlx::PgPool> }
```

Three levels of exposure (proposed):

1. **Values.** Plain data (numbers, strings, lists, records of those) becomes a
   Rux `type` automatically and crosses the boundary without conversion.
2. **Resources.** Anything holding pools, locks, handles or lifetimes is
   opaque. Rux sees its exported methods only, and passes it by handle.
3. **`any`.** The explicit escape into dynamic values.

Rules (proposed):

- Rust `snake_case` becomes Rux camelCase (`discountedPrice`);
  `#[rux(name = "…")]` overrides.
- Every Rust integer type is a Rux `int`, and a value that does not fit throws
  `kind` `"overflow"` at the boundary; `f32` and `f64` are `float`.
- A function returning Rust's `Result<T, E>` appears in Rux as returning `T`,
  and its `Err` is thrown as an `Error` whose `kind` names the Rust error type
  (decided). An `async` Rust function is awaited from an `async fn`.
- `pub` decides what an exported `impl` shows, and `#[rux(skip)]` keeps a
  `pub fn` out. (`#[rux::hide(…)]` and `#[rux::only(…)]`, proposed here, were
  not built: see decision 10 below.)
- A type Rux cannot represent in an exported signature is a **compile error at
  the export** saying why, never dropped silently.
- There is **no escape hatch for calling unexported Rust on a Rux value**. A
  missing capability is written as a small exported Rust function.

### The interface (built 2026-09-27, without a file)

Exports are read from the Rust **source** (with `syn`, in `rux-bindgen`), not
from a build. The checker, `rux check` and the editor (through `rux check`)
have them within a second of saving the Rust, without waiting for `cargo`.
The proposal wrote them to an interface file; reading the source each time
takes milliseconds and cannot go stale, so there is no file. `rux native`
prints the interface in Rux syntax:

```text
// native::shop
export type Product = { id: int, name: string, price: float, note?: string };
export fn catalogue(): Product[];
export type Till;  // a resource: opaque to Rux
export fn openTill(): Till;
//   Product.withTax(rate: float): float
//   Till.ring(p: Product): int
//   Till.settle(): string  (async)
```

## Templates (Kept)

The template language, `r-*` directives, events and CSS do not change. What
[Types, templates](./10-types.md#templates) checks is checked the same way,
with `number` read as `int` or `float`:

- `r-for` gives its variable the element type; `r-if` narrows.
- A bound attribute is checked against what it takes (`:disabled` a `bool`,
  `:class` a map of `bool`s, a string or a string array).
- `r-model` must name something of the input's value type: `string` for text
  fields, **`float` for number and slider** (Changed, proposed), `bool` for
  checkbox and switch. A number field bound to an `int` writes a truncated
  value and is allowed (proposed; runs since step 5).
- `event` has the type of its event. Gesture coordinates are `float`s.
- Props are checked at the tag.

A handler is a statement list in the same grammar as a function body, with
`event` bound. A binding is one expression.

## Security (proposed)

- **Compiled app code has full trust**, as any compiled application does.
- **The interpreter is the sandbox** for anything untrusted: the playground,
  fetched or over-the-air documents, plugins. It carries step and memory
  limits and has no `native` access unless granted.
- **A native module declares the platform permissions it needs**, and the
  Android manifest is generated from those declarations.
- The rest of the 2026-09-24 posture (supply chain, dev channel, saved state)
  stands.

## If you know JavaScript

1. **`int` and `float` are different types.** `/` always gives a `float`;
   `intDiv` truncates.
2. **Arrays and records are values.** Assigning one copies it.
3. **`none`**, not `null` or `undefined`.
4. **`let` at the top level without `signal` cannot be written.** There is no
   `const`.
5. **No object shorthand or spread**: `{ a: a }`, never `{ a }`.
6. **`.length` on arrays and strings only**; a map has `size`.
7. **`x++` is a statement.** `let a = x++` is not a thing.
8. **A handler cannot `await`.** It calls an `async fn`, which does.
9. **`if` is an expression**, and there is no `?:`.

## If you know the fork (what moves)

1. `number` becomes `int` or `float`, and an integer literal is an `int`.
2. `null` becomes `none`.
3. `'x'` is a string.
4. A call no longer runs in its caller's scope, and a method call can see the
   file's names.
5. A closure can write a signal.
6. Types are checked, and an uninferable name is an error in a release build.
7. `===`, `#{`, `|x|`, bitwise operators and rhai's own names are gone.
8. A plain top-level `let` is read-only.
9. `host::` becomes `native` modules.

Fork divergences 1, 2, 3, 5, 6, 7 and 9 (`?.` on a missing property, `++`,
arrows, truthiness, float indexing, bare-brace maps, `?[`) carry over as the
language's own rules. Divergence 4 (caller scope) is dropped. Divergence 8
(annotations kept beside the AST) is replaced by an AST that has them.
Float indexing narrows to what the types allow: an index is an `int`, and a
`float` index is an error with `.trunc()` as the fix.

## Architecture

### The pipeline (proposed)

```text
.rux ──▶ Rux parser ──▶ type checker ──▶ typed Rux IR ─┬─▶ Rux interpreter
                                                        │     dev, hot reload,
                                                        │     playground, sandbox
                                                        └─▶ Rust code ──▶ rustc
                                                              release: native + wasm
```

- **A typed IR** sits between the checker and every backend. The language's
  meaning is defined once, there.
- **Release lowers to Rust source.** Calling an exported Rust function is then
  an ordinary Rust call; generics, optimisation and every target come from
  `rustc` and LLVM. Build time is the cost, paid only for release builds.
- **Rhai is removed completely** (decided). Development still needs an
  interpreter, because hot reload cannot wait for `rustc` and a browser cannot
  run it. That interpreter is Rux's own and runs the same IR, so development
  and release share one set of semantics.
- **The web** already runs the runtime as wasm; with generated Rust, scripts
  compile into that wasm too. The playground keeps the interpreter.
- A separate machine-code backend (Cranelift) is not needed.
- **The interpreter is benchmarked from its first commit** (decided), beside
  the `RUX_PROFILE` measurements in `rux-harness/script-cost/`.

### Build order

1. **This reference.** Done 2026-09-26.
2. **Rux's own parser**, producing a Rux AST, replacing the fork's parser. The
   type annotations built on the fork move over with it.
3. **The checker on the new AST**, extended to the decided types: `int`/`float`,
   `none`, `Option`/`Result`, generics, modules.
4. **The typed IR.** Done 2026-09-26, in the `rux-ir` crate, which depends
   on nothing else in the workspace and on no rhai. The checker keeps the
   type of every expression it checks, and `rux-script`'s `lower` builds a
   unit per file from the file's whole script: its signals, props,
   computeds, effects, lifecycle blocks, functions and template pieces, every
   name resolved to a slot and every conversion written. A verifier holds
   each unit to the IR's rules; debug builds lower and verify every file
   they load, and panic on a broken rule. Over the 359 `.rux` files on the
   development machine, no expression is left untyped and no rule is broken.
   What the IR has no node for is listed per file rather than refused, since
   the fork still runs everything: the only such thing found is a component
   reading or writing its caller's names (`examples/components/cart_row.rux`
   reads `currency` and `sale`; `session.rux` writes `opens` and `saved`),
   which the rule in [What a function can see](#what-a-function-can-see)
   does not allow and step 5 has to settle. Two things moved on the way: the
   checker now reads a file's whole script, so an `effect`, `mounted` or
   `unmounted` body and a `setInterval` body are type-checked for the first
   time (no finding changed across those files); and lowering costs about
   3.6 µs per expression in a release build, more than parsing does, which
   step 5 should look at before lowering runs on every load.
5. **Rux's interpreter on the IR**, replacing rhai for everything. Done
   2026-09-26. `rux-script`'s `interp` runs the IR as a tree: values with
   shared, copied-on-write arrays and maps, `int` and `float` kept apart with
   `int` arithmetic that stops at overflow, and the standard library under
   both of the fork's spellings. The runtime still hands in text (a binding,
   a handler, a component's script), so each piece is lowered against its
   file's unit the first time it runs and kept; its own expressions are
   `any`, and the file's functions it calls are typed. A name nothing
   declares is looked up where it runs, in the handler or component that ran
   it, then in the document's state: that is how a component's functions
   reach their instance's state and how `cart_row.rux` and `session.rux`
   reach the document's, which step 7 settles with stores. The proof was the
   step 2 kind: for a while every evaluation ran on both engines and debug
   builds panicked where they disagreed (value, failure, state, a component
   instance's state, what a binding read, what it asked the runtime for),
   over the whole test suite and every `.rux` file on the development
   machine. The one difference kept on purpose is that a whole-number signal
   is an `int` (`type_of` said `"f64"`). Then rhai was deleted: `rux-rhai`,
   the text rewriting that fed it, and its advisory job. Script is faster in
   the window: on the `rux-harness/script-cost` runs, a loop over 10 000
   records went from 18.6 to 4.7 ms a frame, a filter from 17.0 to 1.6, and
   a list's bindings from 1.6 to 0.08 (`tests/interp_cost.rs` times the same
   work outside the window). Three decided changes were switched on with it:
   `null` and `()` are errors rather than warnings, a number field bound to
   an `int` writes a whole number, and a plain top-level `let` is read-only
   (surveyed first: no file wrote one). Not done yet: a closure captures a
   local by value, not by reference, and an `any` entering typed code is not
   checked when it runs.
6. **`async fn` / `await`** in the interpreter. Done 2026-09-26, as the
   [Async](#async-new-decided) section says, with every (proposed) there
   taken as written. The IR has `Await` and `Start` (a call that does not
   await) and `Func.is_async`; Rust generation reads them as Rust's own
   `async`. The interpreter runs an `async fn` from ops that can stop at an
   `await`: the body is rewritten so each `await` is a statement of its own
   (what an expression read before it is kept in a temporary first, and
   `&&`, `??`, `if` and `switch` around one become statements), then the
   control flow around an `await` becomes jumps. Statements with no `await`
   run on the tree walker as before. The proof was a debug switch,
   `RUX_FLAT=1`, that sends every ordinary function through the same ops
   with all of its control flow compiled: the whole suite passes that way,
   and doing so found one defect (a `return` inside an `if` that is a body's
   value). A task belongs to whoever started it and is claimed where an
   interval is; the answer comes through `rux_script::host` (a host function
   registered with `register_async` gets a `Completer` usable from any
   thread) and wakes the window, which goes on with the task in its owner's
   scope. Driven in a desktop window with
   `crates/rux-shell/examples/async_demo`. Costs, release: starting one to
   its `await` about 1.5 µs, going on about 1.1 µs, against 0.7 µs for an
   ordinary call; the ordinary cases of `tests/interp_cost.rs` did not move.
   `catch e { }` is read as this reference spells it. Left open then and
   closed on 2026-09-27 (after step 8, without the owner): an `await` in a
   `switch` guard runs, the `switch` compiled as `if`s tried in order, each
   guard worked out only when no arm before was taken and its case matched
   (a case is a literal, so a guard is the only place in a `switch` that can
   wait); an `await` at or after a `?.` or `?[` runs only when the chain
   gets that far (`m?[await k()]` with `m` `none` never calls `k`, which it
   silently did before, since only an `await` above the optional step was
   caught); a mutating method whose argument waits (`rows[i].push(await
   f())`) works out its receiver's indexes before the wait, left to right as
   JavaScript does, and still reads the name itself when it runs, since
   arrays are values and a push made meanwhile must not be lost; and a
   failure inside a component's or a module's `async fn` is reported in that
   file at the line of the statement that failed. What is awaited comes from
   Rust through `native` modules (step 8).
7. **Script modules and stores.** Done 2026-09-26. Both spellings of an
   import are one `Import` node in `rux-syntax`, and a type import is marked
   with `type` (the owner's rule: never told by a capital letter). A `.rux`
   file with no `<template>` is a module, loaded once per document, ordered
   so each runs after what it imports. `rux_script::link` rewrites an
   importing file before it is checked: `cart.items`, `cart.add(p)` and a
   picked `add(p)` become linked names (`stores/cart::items`), and a write
   from outside the module, a private name and a module used as a value are
   refused there. Each module is checked on its own, and `lower_linked` puts
   it into the document's one unit under its linked names, so the
   interpreter and the runtime's reactivity, which tracks names, needed
   nothing new: one store read by the document and every component instance
   re-renders them all. Components got their own scope the same way (7.4):
   a component's functions are linked under its name, an instance's state
   carries which component it is, and its code sees its own names, its
   imports and what the runtime provides, not the document's. That ended
   the `Outer` fallback to document state that step 5 kept for
   `cart_row.rux` and `session.rux`; both examples and the `rux new`
   scaffold's pages moved their shared state into stores
   (`examples/stores/`, the scaffold's `stores/todo.rux`). The findings
   survey over the 359 `.rux` files found those three and their copies in
   other clones, nothing else. `examples/store.rux` (a cart store, a bare
   money module, two components) was driven in the window, as were the two
   migrated examples. A store costs what the document's own state does: a
   call that writes 0.96x, a read 1.03x (`tests/interp_cost.rs`). Also
   fixed on the way: a comment that mentioned `<template>` opened a section.
   Not done yet: a factory store (the "scoped" half); a generic function
   exported from a module is checked no further than `any` at the call; the
   debug-build IR verification lowers a file without its modules, so a
   linked name is looked up where it runs there; `rux check` on a component
   in a folder with no project root resolves its imports from the
   component's own directory.
8. **`#[rux::export]`, the interface file, `native` modules.** Planned
   2026-09-27 with nobody to approve it; every choice the plan makes is
   listed, numbered, in
   [Decided without the owner (2026-09-27)](#decided-without-the-owner-2026-09-27).
   In order:
   - 8.0 **`rux-native`**, the crate an app's Rust depends on (as `rux`):
     the boundary value `rux::Any`, conversions both ways for the Rust types
     that cross, resource handles, `rux::Error`, the registry of native
     modules, and where an `async` export's future runs.
   - 8.1 **`rux-bindgen`**, reading Rust source with `syn`: what each Rust
     type is in Rux, and every export of a crate with its Rux signature.
     Shared by the macros and the CLI, so the two cannot disagree.
   - 8.2 **`rux-native-macros`**: `#[rux::export]` on a `fn`, a `struct`
     and an `impl`, `#[rux::resource]`, `#[rux::init]`. Each export gets a
     hidden descriptor function; nothing runs before `main`.
   - 8.3 **The language side**: `use native::x` and `import … from
     "native/x"` link like a script module; the checker reads each native
     module's interface; a native call, awaited or not, lowers to its own
     IR callee; the interpreter calls it, converts at the boundary, and
     throws its errors as Rux `Error`s. Methods of native types
     (`p.discountedPrice(0.1)`) are checked and called. `host::` is
     retired.
   - 8.4 **The tooling**: the loader and `rux check` read the app's
     `native/` crate for signatures (no build needed); `rux run` and
     `rux build` compile it into the app and register every export;
     `rux native` prints what Rux sees.
   - 8.5 **An example app with a `native/` crate**, driven in the window;
     docs; the cost of a native call measured.

   Done 2026-09-27, as planned, in b194af7 (8.0 to 8.2), bd81e63 (8.3) and
   the commit after it (8.4, 8.5). `examples/native-shop` (a catalogue of
   `#[rux::export]` records with a method, a `#[rux::resource]` till with a
   Rust `Result` and an `async` method) was driven in a desktop window: the
   Rust catalogue and its method in a binding render; an empty till's
   `settle` is refused with `TillError: nothing to settle` while the window
   stays live; three items ring up to 5.90 in whole cents and settle. `rux
   run` built the app with its crate in, and rebuilt and restarted it eight
   seconds after `shop.rs` changed. `rux check` reports a misspelled native
   function ("does not export `catalog`: it exports `catalogue`, `openTill`"),
   a misspelled method, a wrong type and a Rust export that cannot cross
   (at its `.rs` line), with nothing built. A synchronous native call costs
   what a script `fn` call does (0.46 against 0.55 µs, loop included,
   `tests/interp_cost.rs`). Not done yet: async exports on the web (no
   threads; `rux::set_spawner` is the way in); permissions declared by a
   native module; `--route` and `--preview` for `rux run` of an app with a
   `native/` crate; a resource kept in a component instance's state (it
   passes through the runtime's display value and comes back as text: keep
   resources in a store or the document); a record's method in a binding or
   a handler is found by the record's fields, so two native record types with
   the same fields and method name are ambiguous there (typed script code is
   exact).
9. **Rust code generation** for release builds, native then wasm. Planned
   2026-09-27 without the owner, after step 8; the choices are 23 to 30 of
   [Decided without the owner](#decided-without-the-owner-2026-09-27). The
   shape: a release build lowers each file as the interpreter does, and
   writes Rust for its functions into the wrapper crate; the interpreter
   still loads every file, and where a function has compiled code it calls
   that instead of walking the tree. Nothing else changes: the runtime's
   seam, reactivity (a compiled read or write is tracked by the same calls),
   tasks and hot reload in development. In order:
   - 9.0 **`rux-codegen`**: IR to Rust source, one `fn` per IR function,
     against a small public API in `rux-script` (`rux_script::aot`) that
     exposes what a body needs: reading and writing globals with tracking,
     calling functions, built-ins, methods and native exports, operators,
     fields and indexes, and the step budget. A function it cannot compile
     yet is left to the interpreter, and says so in a coverage count, as the
     step 4 survey did.
   - 9.1 **The table**: generated code is keyed by a hash of the exact text
     it was generated from (the file's script with its modules' and
     components'). The interpreter uses it only when the hash matches what it
     lowered, so a stale build falls back to interpreting instead of running
     code for other source.
   - 9.2 **Typed fast paths**: where the IR says a local is an `int` or a
     `float`, arithmetic and comparisons are Rust's on `i64`/`f64` (with the
     language's overflow check), not the dynamic operator.
   - 9.3 **The proof**: every example and test script, generated, compiled
     into a test crate, and every function run on both engines with the same
     arguments; they must agree on value, state and failure. The benchmark
     is `tests/interp_cost.rs`'s cases, compiled.
   - 9.4 **`rux build --release`** generates and compiles it in, desktop and
     Android; 9.5 **the web**, where the same Rust compiles to wasm.
   - Later, and not in this plan: template pieces (bindings and handlers)
     compiled too, and `async fn` bodies (their resumable form).

   Status 2026-09-27 (overnight, without the owner): 9.0, 9.1, 9.3 and 9.4
   done in cd60009 and the commit after it; 9.5 compiles; 9.2 done in the
   commit after that, as two things. Operators whose values turn out to be
   two `int`s or two `float`s are done in Rust inline (an `int` overflow
   falls back to the interpreter's operator, so its failure reads the
   same). And a function none of whose statements is handed back runs
   without an interpreter frame: its locals are Rust variables, and only
   state, calls and the step budget go through `aot`. Such a call still
   counts toward the depth limit. With both, release: `fib(18)` 0.28x of
   the interpreter's time, a `for` over 10 000 0.32x, a small loop with
   `continue` and `break` 0.32x, a handler writing three signals 0.58x,
   and a function of handed-back statements unchanged. Proving it found a
   defect older than step 9: in a debug build one script call costs about
   35 KB of Rust stack, so 128 nested calls crashed the process instead of
   failing (watchlist 35); the interpreter now also stops at 768 KB of
   stack used, with the same error. Then widened (4605b4f): field and
   index reads with no `?.`, methods that do not change their receiver,
   built-ins, native calls, closure values, names found where they run,
   array and map literals, `for x in` a collection and `throw` compile
   too, each through the interpreter's own helper. Still handed back: a
   method that changes its receiver (its receiver is a place), `?.` chains,
   `try`, `switch`, closures created, `is`, and writes through fields and
   indexes. `if` and blocks used as values compile (second session of the
   night) when everything inside them does; a statement inside one has no
   path to be handed back by, so one that does not compile leaves the
   whole enclosing statement to the interpreter.

   Widened again 2026-09-27 (day session): `?.` and `?[` chains (a Rust
   labelled block the chain breaks out of with `none`), `is` (the type
   written out as its own text and read back once per thread, only when
   that text reads back as the same type), `switch` with its ranges, `|`
   and guards, `try` and `catch` (the body runs in a Rust closure, so a
   failure anywhere in it is the closure's value; a `return`, `break` or
   `continue` inside comes out as the interpreter's own `Flow` and is done
   after it), writes through fields and indexes, with `op=`, and methods
   that change their receiver when the receiver is a place. Each goes
   through the interpreter's own helper, as before; a local the compiled
   code holds as a Rust variable is changed in place, with the same walk
   and the same failure (b2d5a19). Writing the corpus for it found three
   defects older than step 9, all in both engines (watchlist 36 to 38):
   the checker lets `.` read an optional field, `trunc` of an infinity
   gives `Infinity`, and a `switch` guard that is a bare name parses as an
   arrow function.

   Then creating a closure compiled too (04bacc4): its code stays the
   interpreter's, found by its number among the function's closures
   (`rux_ir::ir::closures` numbers them, and both the generator and the
   interpreter use that walk), and what it captures is read in compiled
   code, in the interpreter's order. With it, every statement of the
   corpus compiles (9 scripts). It also found a generator defect:
   `total += f(10)` on a local of a function with no frame wrote
   `BinOp::Dyn("+"` without its closing parenthesis, because the operator
   was cut out of its `Some(...)` text by trimming; it is now written from
   the operator itself. Still handed back: `setInterval`.

   Routed pages need nothing more, which corrects the list of what was left
   below: a route's view is a component, and a component's functions are
   linked into the entry document's unit and hashed with it, so they
   compile with it (`examples/router.rux`'s unit holds
   `components/crew_detail::member_of`). What is not compiled in any file
   is its template pieces (handlers and bindings), closure bodies and
   `async fn` bodies.

   Cost at 79590d1, release, per call, two runs agreeing within a few
   percent (`cargo test -p rux-aot-tests --release --test differential --
   --ignored cost --nocapture`, which now prints the times and how many
   times faster):

   | Call | Interpreted | Compiled | Faster |
   |---|---|---|---|
   | `fib(18)`, recursion | 3465 µs | 968 µs | 3.6x |
   | `sum(10000)`, a `for` over a range | 847 µs | 245 µs | 3.4x |
   | `odd(20)`, `break` and `continue` | 3.08 µs | 0.95 µs | 3.2x |
   | `!bump(1)`, a handler writing 3 signals | 10.76 µs | 4.47 µs | 2.4x |
   | `first_bad(…)`, `try` in a loop | 2.23 µs | 1.08 µs | 2.1x |
   | `open()`, fields, a `for` over state | 0.94 µs | 0.57 µs | 1.6x |
   | `grid()`, nested index writes | 2.98 µs | 1.99 µs | 1.5x |
   | `band(42)`, `switch` | 0.74 µs | 0.51 µs | 1.5x |
   | `pet(1)`, a `?.` chain | 0.77 µs | 0.53 µs | 1.5x |
   | `adders()`, a closure made in a loop | 2.22 µs | 1.73 µs | 1.3x |
   | `kinds()`, nested `try` | 1.96 µs | 1.51 µs | 1.3x |
   | `local()`, writes, `push`, `join` | 3.68 µs | 2.96 µs | 1.2x |
   | `offset(…)`, `map` and `filter` | 3.53 µs | 3.27 µs | 1.08x |
   | `titles()`, `map` with a closure | 1.80 µs | 1.71 µs | 1.05x |

   Loops and arithmetic are three to four times faster. What is mostly
   library work (`join`, copying maps) gains less, since the library was
   Rust already. `map` and `filter` barely gain: creating the closure is
   compiled, but its body is not, and they call the interpreted body once
   per item. That is 9.6 below.

   9.5 driven 2026-09-27 (c0448ea): a two-file project (a document with a
   `<router>` and a component page) built with `rux build --release
   --target web` compiled 7 functions, none handed back, and was run in
   Brave over the DevTools protocol. A built web app now says in the
   console, once, when its script runs compiled: `rux: script
   0x83142b371c27822c: 7 functions run compiled`, the hash the build
   registered. A tap ran a handler whose function navigates to a path made
   of compiled results (recursion, a `switch` with a range, a caught
   failure, closures, `?.` chains), and the address bar read
   `/r/610-small-caught-12-rex-none`, the expected answer. Two things cost
   time and are about the harness, not Rux: a server already on the chosen
   port answered instead of the test's (Windows lets a second server bind
   it silently), and a browser launcher's process is not the browser, so
   stopping it leaves the old page for the next run to attach to.

   9.6 done 2026-09-27: closure bodies are compiled. `rux_ir::ir::closures`
   now numbers every closure a function creates at any depth, pre-order (a
   closure before those its own body creates), one count per function;
   the generator writes closure `k` of function `f` as `c{f}_{k}` and
   registers it beside the functions (`aot::register(hash, FNS,
   CLOSURES)`). The interpreter maps each closure's code, by address, to
   its compiled body, and a closure value carries that body from the
   moment it is made, so a call looks nothing up; a closure made by the
   interpreter from the same code (in a handed-back statement) runs
   compiled too, and one made by a template piece, whose code is not the
   unit's, stays interpreted. A body with nothing handed back holds its
   captures as Rust variables, its own copies per call, as the
   interpreter's frame does; one with a statement handed back runs in a
   frame holding them (`aot::run_closure`, `Cx::cstmt`, `Cx::cvalue`).
   The proof gained script 9 (`reduce`, `filter`, `map`, a `sort`
   comparator, `find`, `forEach`, closures three deep reading captures of
   captures, a closure writing its own copy of a capture, `return` from a
   `try`, a loop with `continue`, callbacks kept in state and one made by a
   handler, a failure inside a body, and a body in a frame calling an
   `async fn`); all 18 of its closure bodies compile, 1 statement is handed
   back, and both engines agree on every case.

   The bench was noisy enough to swing a speedup from 1.5x to 1.1x between
   two runs, because it timed one engine and then the other; the engines
   now take turns over seven rounds and each keeps its best. Before is the
   same bench run on 1c9e083; the interpreted times of the two runs agree
   to within 5%, the column below is the later run's. Per call, release:

   | Call | Interpreted | Compiled before | Compiled after | Faster than interpreted |
   |---|---|---|---|---|
   | `looped()`, a loop inside a closure | 1.73 µs | 1.64 µs | 0.75 µs | 1.1x, now 2.3x |
   | `adders()`, a closure made in a loop | 2.15 µs | 1.63 µs | 1.33 µs | 1.3x, now 1.6x |
   | `total()`, `reduce` | 1.48 µs | 1.41 µs | 0.91 µs | 1.0x, now 1.6x |
   | `offset(…)`, `map` and `filter` | 3.44 µs | 3.20 µs | 2.25 µs | 1.1x, now 1.5x |
   | `counter()`, a closure writing its capture | 1.35 µs | 1.01 µs | 0.89 µs | 1.3x, now 1.5x |
   | `deep()`, closures three deep | 1.92 µs | 1.76 µs | 1.27 µs | 1.1x, now 1.5x |
   | `by_n()`, `map` and a `sort` comparator | 4.94 µs | 4.58 µs | 3.45 µs | 1.1x, now 1.4x |
   | `names()`, `filter` then `map` | 3.37 µs | 3.24 µs | 2.55 µs | 1.0x, now 1.3x |
   | `titles()`, `map` reading one field | 1.72 µs | 1.67 µs | 1.44 µs | 1.1x, now 1.2x |

   The 2x expected for `titles()` did not come: its closure body is one
   field read, so what is left is the fixed cost around it (running the
   binding, the call, reading `tasks`, `join`), which compiling a closure
   body does not touch. Every row gained.

   9.7 done 2026-09-27: starting a `setInterval` is compiled. The
   generated code works out the first argument and calls
   `aot::interval(ms, text)`, which starts the timer as the interpreter's
   arm does (`start_interval`), from the body's text, since the runtime
   keeps a timer as text; the timer's body still runs interpreted when it
   fires. As in the interpreter, only the first argument is worked out,
   and one that is not a number is a delay of 0. The proof now compares
   the timers each case asks for too (delay and body, without the handle,
   which both engines draw from one count), and script 10 starts them in a
   function, in a loop, inside a closure body, cleared at once, kept in
   state and clearing itself, and with a text delay; none of its 16
   statements is handed back. `twice()`, two timers started in a loop:
   interpreted 1.19 µs; compiled 1.00 µs at 4ae5a93 (1.2x faster, the
   statement handed back) and 0.61 µs now (1.9x faster).

   9.8 done 2026-09-27 (watchlist 35): a debug build nests script calls
   to the depth limit again. Measured first, with the stack address at the
   start of each interpreter function: one call cost about 22 KB in a
   debug build (35 KB was noted earlier; this is today's), stopping recursion 35
   deep. `Interp::expr` alone had a 17 KB frame and `stmt_here` 12 KB,
   because a debug build gives every temporary of every `match` arm (each
   `?` makes several) its own slot. `expr` now works out only the cheap
   kinds itself (literals, names, calls, blocks) and hands each other kind
   to a function of its own (`expr_binary`, `expr_if`, `expr_step`,
   `expr_match`, `expr_rest`); `stmt_here` hands loops and `throw`/`try`
   to `stmt_loop` and `stmt_rest`. Those are kept from being inlined back
   in a debug build only: marked `#[inline(never)]` in a release build too,
   `kinds()` ran 20% slower compiled, so a release build keeps its own
   inlining and its numbers (`fib(18)` interpreted went from 3581 µs to
   3197 µs, the rest within noise). Now, in a debug build: interpreted,
   126 calls deep (the depth limit, about 6.2 KB a call); compiled, 105
   deep (about 7.5 KB, most of it the generated function's own frame,
   which only a release build ships). `tests/stack.rs` measures both on a
   corpus script, and holds both to 100; it used to run a script outside
   the corpus, so its "compiled" case had been interpreted.

   9.9 measured 2026-09-27, and the answer is to leave template pieces
   interpreted. `examples/dashboard.rux` and `examples/list.rux` do
   nothing between taps, so they were measured with the script-cost
   harness instead (outside the repo, `rux-harness/script-cost`: the same
   app changing state every frame from a `setInterval`, in four modes),
   with `RUX_PROFILE=1`, a release `rux run` at e8dba1e, averaged over 60
   frames. A timer's body there is `step();`, so the time charged to
   handlers is the file's functions, which a release build compiles
   already; a piece's own cost is the script time outside handlers:

   | Mode | Frame | Script | In handlers (functions) | Pieces' own | Piece runs a frame |
   |---|---|---|---|---|---|
   | idle | 2.34 ms | 0.023 ms | 0.020 ms | 0.003 ms | 4 |
   | sum, 10 000 records | 6.11 ms | 4.913 ms | 4.911 ms | 0.002 ms | 4 |
   | filter, 2 000 records | 12.49 ms | 1.468 ms | 1.458 ms | 0.010 ms | 41.5 |
   | list, 300 rows | 24.93 ms | 0.076 ms | 0.035 ms | 0.041 ms | 308 |

   Pieces cost at most 0.2% of a frame, even at 308 bindings a frame;
   where script matters (sum, 80% of the frame) it is a function's loop,
   which is compiled. The list's frame goes to layout (10.6 ms) and
   patching (6.9 ms). So the design questions below are written down for
   the owner, not built, and nothing about pieces is recommended now:
   (a) finding every piece's text at build time (walking each template as
   the runtime does, or recording them the first time an app runs);
   (b) naming what is only numbered at run time (by name, resolved once
   when the piece is first lowered, then cached); (c) the key a piece is
   found by (the runtime's own cache key, hashed with the unit's hash);
   (d) `async fn` bodies, whose resumable form (`flat.rs`) is what would
   be compiled. (d) is a separate question: no app measured spends time in
   an `async fn`'s own code rather than in what it waits for, and it
   should be measured on one that does before anything is built.

   What follows describes the state before 9.2.
   `rux-codegen` compiles control flow (`if`, `while`, ranges, `break`,
   `continue`, `return`), literals, locals, state reads and writes (`=` and
   `op=` on a name), operators, logic, `??`, templates and calls to the
   file's own functions; the rest is handed back statement by statement.
   The proof, `crates/rux-aot-tests`, generates Rust for a corpus of five
   scripts in its `build.rs` (recursion, overflow, a type failure, state,
   every loop form with `break` and `continue`, nested loops, text,
   optional values, and a script of mostly handed-back statements: fields,
   methods, closures, `try`, `throw`, `switch`) and runs every case on both
   engines: value, what changed, what was read, and every warning agree. A
   text that differs by one comment runs interpreted. Cost, release, per
   call: `fib(18)` 0.63x of the interpreter's time, a `for` over 10 000
   0.64x, a small loop 0.51x, a handler writing three signals 0.36x, a
   function that is mostly handed back 0.95x. `rux build --release` loads
   the entry document as the app will, compiles its unit, and the wrapper
   installs it first; `examples/native-shop`'s release build reports (with
   `RUX_AOT_REPORT=1`) that its compiled table was found under the hash the
   build computed, which proves the text hashed on the build machine is
   the text the embedded app lowers. `RUX_AOT=0` builds without it. The
   generated code compiles for wasm32; no web build was driven. Not done
   yet: typed fast paths (9.2, where the numbers above should move most);
   fields, indexes and methods compiled; routed pages loaded as documents of
   their own (only the entry document is compiled); template pieces and
   `async fn` bodies.

   **Next, planned 2026-09-27 for the next session.** Step 9's plan
   (9.0 to 9.5) is done. What is left, in order; each item ends with the
   differential proof green, the gate green, and the cost bench run and
   reported as times and how many times faster (the owner asked for that
   form), then a local commit. Nothing is pushed without asking.

   - 9.6 **Closure bodies compiled** (DONE 2026-09-27, see "9.6 done"
     above; the numbering chosen is one pre-order count per function).
     The plan as written: (the biggest gain left: `map`,
     `filter`, `reduce`, `sort` with a comparator, `find`, `forEach` and
     every callback stored in state). A closure's code is an
     `ir::Closure` with its own `Body` and `captures`. The generator
     writes one Rust function per closure body, numbered as
     `rux_ir::ir::closures` numbers them, extended to walk into closure
     bodies so a closure inside a closure has a number too (a path of
     numbers, or one pre-order count per function; decide and write it
     down). The interpreter, when it finds its table, maps each
     `Rc<ir::Closure>` pointer to its compiled body, so a closure runs
     compiled however it was created, by compiled code or by the
     interpreter. `call_closure` checks that map first. A compiled body
     gets its captures by value (`ExprKind::Capture(i)` reads the `i`th;
     an assignment to a capture changes only the closure's own copy, as
     the interpreter's frame does) and its arguments as locals. Without a
     frame when nothing is handed back, as functions are; with one
     otherwise, which needs an `aot` call that pushes a frame holding
     captures. A closure created by a template piece is not in the table
     and stays interpreted. Proof: corpus cases for `map`, `filter`,
     `reduce`, a `sort` comparator, a closure in a closure, a closure
     writing its capture, and a callback kept in state and called from a
     handler. Expected: `titles()` and `offset(…)` from about 1.05x to
     2x or more faster.
   - 9.7 **`setInterval`** in compiled code (DONE 2026-09-27, see "9.7
     done" above). The plan as written: `ExprKind::Interval` becomes
     a call to an `aot` helper doing what the interpreter's arm does
     (`start_interval(ms, text)`); its body stays the runtime's, which
     keeps a timer as text. Small.
   - 9.8 **Watchlist 35, the real fix** (DONE 2026-09-27, see "9.8 done"
     above). The plan as written: a debug build still stops script
     calls about 20 deep, because one interpreted call costs about 35 KB
     of Rust stack. Measure the bytes per call first (the probe in
     `crates/rux-aot-tests/tests/stack.rs`), then shrink `Interp::expr`
     and `Interp::stmt` frames: move the large, rarely taken arms
     (`Match`, `Closure`, `Interval`, method calls, `Try`) into their own
     `#[inline(never)]` functions so the common path's frame is small.
     Target: 100 nested calls in a debug build within the 768 KB budget,
     and no change to the release numbers above.
   - 9.9 (MEASURED 2026-09-27, see "9.9 measured" above: pieces stay
     interpreted.) **Template pieces and `async fn` bodies: a design, not code,
     until the owner decides.** Pieces (handlers, bindings, effects, a
     component's script) are lowered from text when they first run, and
     lowering appends to `unit.outer` and to the provided globals in that
     order, so a build that lowered them itself would number those names
     differently from the running app. The design to write, with the
     choices for the owner: (a) how the build finds every piece's text
     and the names handed in with it (walking each template as the
     runtime does, or recording them the first time an app runs); (b)
     how compiled code names what is only numbered at run time (by name,
     resolved to numbers once when the piece is first lowered, then
     cached); (c) the key a piece is found by (the runtime's own cache
     key, hashed, together with the unit's hash); (d) `async fn` bodies,
     whose resumable form (`flat.rs`) is what would be compiled. Measure
     first: how much of a real app's frame time is pieces, with
     `RUX_PROFILE` on `examples/dashboard.rux` and `examples/list.rux`;
     if it is small, say so and leave pieces interpreted.
   - Not step 9, small, and found by it: watchlist 38 (a bare-name
     `switch` guard read as an arrow; parse the guard with arrows off, as
     `no_pipe` does for `|`), 37 (`trunc`, `round`, `floor`, `ceil` of
     `NaN` or an infinity throw `overflow`, as Numbers says), 36 (the
     checker refuses `.` on an optional field, and an array of records
     that differ infers the fields not all have as optional). Each
     changes what an author sees, so each is shown to the owner before it
     is committed.
10. **Closing the speed gap with Dart and the JVM.** Guidance, written
   2026-09-27 at the end of step 9, not a fixed plan: the next session
   reads it, checks it against the code, and proposes its own order where
   it sees better. What is measured is fact; what is suggested is a
   starting point.

   **Where things stand.** Script time per frame on the
   `rux-harness/script-cost` apps (release, desktop), from the rhai fork
   to today:

   | Stage | Sum, 10 000 records | Filter, 2 000 | List, 300 rows |
   |---|---|---|---|
   | rhai fork, after the 2026-09-25 fixes | 18.6 ms | 17.0 ms | 1.6 ms |
   | Rux's interpreter, rhai deleted (step 5) | 4.7 ms | 1.6 ms | 0.08 ms |
   | Today, interpreted (`rux run`) | 4.91 ms | 1.47 ms | 0.076 ms |
   | Today, compiled (`rux build --release`) | 3.64 ms | 1.19 ms | not measured |

   Before any of that work the whole frames were 88 ms (sum) and 78 ms
   (list). The list's frame is now about 25 ms, and its script is 0.08 ms
   of it: layout (10.6 ms, a fresh Taffy tree every frame) and patching
   (6.9 ms) are the rest. Compare script time, not whole frames: the `gpu`
   phase runs 1 to 9 ms depending on the window, whatever the app does.

   The same work in Dart compiled ahead of time (what a Flutter release
   runs) and Java 17 on the desktop JVM (standing in for Kotlin; ART on a
   phone is another VM), microseconds per call, best of seven rounds, the
   sources in `rux-harness/compare` outside the repo:

   | Work | Rux interpreted | Rux compiled | Dart | JVM |
   |---|---|---|---|---|
   | `fib(18)` | ~3200 | ~920 | 23.9 | 19.8 |
   | sum of ints to 10 000 | ~850 | ~245 | 7.4 | 3.9 |
   | 10 000 records, `price * qty` | 4910* | 3640* | 17.7 | 19.1 |
   | filter 2 000 records | 1470* | 1190* | 62 | 17 |
   | recursion before it is stopped | 128 | 128 | 57 090 | 21 712 |

   \* Per frame in the window, so a little work around the loop is in it.
   Compiled Rux is 30 to 40 times behind on calls and integer loops and
   about 200 times behind on records. Plain Rust does the record sum in
   about 8 µs, faster than both, so the gap is how the generated Rust
   holds values, not the language it is written in.

   **Why, as the code stands at e2b283a.** Every value is a dynamic `V`
   (a tagged enum, reference counted), even where the checker has proven
   an `int`. A record is a `BTreeMap<String, V>`, so `item.price` is a
   string search and a clone. A call between compiled functions goes
   `Cx::call`, `call_fn`, `aot::run`, with its arguments in a new `Vec`
   and a stack probe. `map` and `filter` call their closure through that
   machinery once per item. Every statement ticks the step budget through
   a call, and every state read is tracked by name.

   **What must not change.** The owner wants the syntax authors write
   today kept, and none of the tracks below needs a new spelling: the
   checker already puts a type on every expression of the IR. Behaviour
   must not change either, and the differential proof in
   `crates/rux-aot-tests` is what holds that; every track adds its cases
   there. Two behaviours are at risk and need a decision, not a default:
   a record shows and walks its fields sorted by name today (a layout in
   declaration order must keep showing them sorted), and a write to a
   field its type does not declare works today (a fixed layout needs a
   fallback, or the owner makes it an error). `any` keeps working and
   keeps its dynamic speed, as Dart's `dynamic` does.

   The second is decided (owner, 2026-09-27): **writing a field the type
   does not declare is an error, and the checker tells the author.** It
   already does for a typed record (`items[0].color = "red"` on an `Item`
   without `color`: "`Item` has no field `color`"); what works today is
   only what gets past the checker, in the interpreter, which warns and
   runs on. Under a fixed layout that write fails when it runs too, with
   `kind` `"type"`, as a wrong use of an `any` does. The first is decided
   too (owner, 2026-09-27): **fields are shown and walked in the order the
   type declares them**, not sorted by name.

   **Tracks, roughly by what each buys for what it costs.**
   - (a) Typed values in generated code: an expression whose type is
     `int`, `float` or `bool` becomes Rust's `i64`, `f64`, `bool`, and
     becomes a `V` only where it leaves (state, `any`, a call into the
     interpreter, a hand-back). Does not touch the value model. Should
     move `fib` and integer loops most. Watch: an `int` overflow must
     still fail with the interpreter's words, as `fast_path` does now.
   - (b) Direct calls: a compiled function calling another compiled one
     with typed arguments calls it as a Rust function, with the depth
     count kept as a plain counter. Pairs with (a).
   - (c) A record layout: fields at indexes the checker knows, the
     interpreter reading by name through the layout, compiled code by
     index. The largest lever (records, 200x) and the largest change: the
     interpreter, the standard library, bindings, equality and display all
     meet it. Needs the owner's two decisions above first.
   - (d) Callbacks written inline (`xs.filter(x => x.n > 1)`) compiled
     into a Rust loop, with no closure value and no call per item.
   - (e) Cheaper bookkeeping: the step tick as a counter decrement, a
     state read tracked once per function rather than per use.
   - (f) The recursion cap: measure a release call's stack (the probe in
     `tests/stack.rs` in a release build), then raise `MAX_DEPTH` for
     release builds to what the 1 MB main thread allows with room.
   - (g) Not script: incremental layout (keep the Taffy tree across
     frames) and a cheaper patch, which is where the list's frame goes.
     Matching Flutter on what a user sees needs this more than any of the
     above. Its own track, with its own measurements.

   **A suggested first move**, open to a better one: prototype (a) and (b)
   on `fib` and `sum`, and put the numbers beside Dart's and the JVM's
   before committing to anything larger. That shows whether generated
   typed Rust closes the gap as expected (the estimate: within a few
   times of Dart on those two), cheaply, and without touching the value
   model. Then bring (c) to the owner with its two decisions and the
   prototype's numbers.

   **Questions the next session should answer for itself** rather than
   take from here: whether (c) is better done as Rust structs per record
   type or as one shared layout-and-slots value; whether (d) is worth it
   once (a) to (c) exist, or falls out of them; whether any of this
   should wait behind (g), given that script is under 1% of the list's
   frame; and what a fair whole-frame comparison would be (the same app
   built in Flutter and Compose, on one device).

   **10.1 done 2026-09-27: tracks (a) and (b), for numbers only.** A
   function whose parameters, locals and result are all `int`, `float` or
   `bool`, and whose body does only arithmetic, comparison, logic, `if`,
   `while`, a `for` over a range, blocks and `if` as values, and calls to
   other such functions, gets a second body over Rust's `i64`, `f64` and
   `bool` (`rux-codegen/src/typed.rs`, found by a fixpoint, so a call to a
   function that does not qualify disqualifies the caller). Its `f{id}`
   calls it when the arguments it is handed are those kinds and runs as
   before when they are not, so nothing the checker did not vouch for
   meets typed code; the arguments are the only way in. A typed call to a
   typed function is a Rust call, counted toward the depth limit as any
   call is. Ticks and failure places are where the untyped body has them,
   and where Rust's operation stops (overflow, `% 0`, `**`) the
   interpreter's own operator runs on the same values, so its words are
   the failure's. Nothing an author writes or sees changes. Proof: corpus
   script 12, 32 cases (overflow at a statement and at a caller, `% 0`,
   the least `int` negated and divided by -1, NaN compared, `while` with
   `break` and `continue`, arguments of other kinds, untyped calling
   typed), both engines agree; the gate green (1115 tests). Release, µs
   per call, best of 7:

   | Call | Interpreted | Compiled before | Compiled, typed | Dart | JVM |
   |---|---|---|---|---|---|
   | `fib(18)` | 2975 | 922 | 40 (23x faster than before) | 23.9 | 19.8 |
   | `sum(10000)` | 826 | 234 | 42 (5.5x faster than before) | 7.4 | 3.9 |
   | `odd(20)` | 2.56 | 0.88 | 0.41 (2.1x faster) | | |

   `fib` is now within 2x of Dart and the JVM; `sum` 6x of Dart and 11x of
   the JVM. What is left in `sum` is not the arithmetic: the generated
   loop is plain Rust, and moving `tick`'s failure out of line changed
   nothing, so it inlines already. The likely cost is the step counter
   living behind `&mut Interp`, two dependent load-add-store rounds an
   iteration; keeping the budget in a local of the typed body and writing
   it back at calls, returns and failures (track (e)) is the next thing to
   try. Records (track (c)) are untouched: the script-cost sum over
   records does not change.

   `**` of two `int`s, looked at on the way (watchlist 39): it was an
   `int` only when its right side was a whole number written out and not
   below zero (`a ** 2`); with a name on the right (`a ** b`) it was a
   `float`, because the sign is not known before it runs. That was Rux's
   own rule, not Rust's, and it cost exactness: a `float` holds 53 bits, so
   `3 ** 34` came out as 16677181699666568, one less than the answer, with
   no error. The owner decided (2026-09-27) that `int ** int` is an `int`,
   as "Numbers" now says: exact, or an `overflow` throw, and a negative
   power throws (written out, the checker refuses it). Corpus script 12
   holds `3 ** 34`, `7 ** 20`, `2 ** 63` (overflow) and a negative power,
   and `powi` now has a typed body.

   **Field order decided too** (owner, 2026-09-27): a record shows and
   walks its fields **in the order its type declares them**, as
   JavaScript, Dart and Kotlin do, not sorted by name as today. Under the
   fixed layout of track (c) that is the slots' own order, so it costs
   nothing. With both of track (c)'s decisions taken, (c) is ready to plan.

   The power's speed, measured the same way before and after (the old
   commit in a worktree, the same bench case in both): `powers(10000)`, a
   compiled loop calling `powi(3, i % 30)`, went from 2534 µs to 387 µs
   (6.5x faster), because `powi` and the loop around it now have typed
   bodies. One call of `powi` on its own shows nothing (about 0.4 µs
   either way): that is the bench evaluating the call's text, not the
   power.

   **10.2 done 2026-09-28: track (e), the step count in a local.** An
   experiment first: with the typed bodies' ticks taken out, `sum(10000)`
   went from 42 µs to 3.5 µs, so the ticks were the whole of what was
   left. Taking them out is not allowed (the budget is what stops a loop
   that never ends from freezing the app), so a typed body now counts
   steps in a local, `__ops`, which Rust keeps in a register, against the
   budget read once when it starts, and hands the count back to the
   interpreter before anything else can read it: before calling another
   typed body (and reads it back after), before returning, and before any
   failure leaves (`u!`, the typed `t!`). Past the budget it fails with
   the interpreter's own words (`aot::too_many`, the same function the
   interpreter's `tick` uses), placed where the tick it replaces placed
   it. Proof: corpus script 12's `burn()` calls typed bodies and then
   writes state until the budget stops it, so the count it ends with
   shows any step counted differently; both engines agree on it. Release,
   µs per call, best of 7, two runs agreeing:

   | Call | Before (10.1) | After | Dart | JVM |
   |---|---|---|---|---|
   | `sum(10000)` | 42 | 5.7 (7.4x faster) | 7.4 | 3.9 |
   | `fib(18)` | 40 | 42 (the same, within noise) | 23.9 | 19.8 |
   | `powers(10000)` | 387 | 400 (the same) | | |

   Integer loops now beat Dart AOT and sit within 1.5x of the JVM. `fib`
   is calls: each is a depth check, a stack probe and a count handed back
   and forth, about 3 ns, where Dart's is under 1 ns. Cheaper calls
   between typed bodies (the depth counted in a local too, the stack
   probe only every so many levels) are what would move it, and are
   small; records are worth far more first.

   **10.3, next, before track (c): cheaper calls between typed bodies,
   and the recursion limit (track (f)).** Small, and first on purpose:
   left behind a bigger step, a small thing is the kind that is found
   again later as a surprise. `fib(18)` is 42 µs compiled against Dart's
   23.9 and the JVM's 19.8, and the difference is calls: each typed call
   today does `cx.enter()` (reads the frame stack's length and a
   counter, takes the stack pointer, compares it with the budget, adds
   one), hands the step count back to the interpreter and reads it again
   after, and `cx.leave()`. What to try, measuring each on its own:
   - pass the step count to a typed callee as `&mut u64` (or return it
     with the value) instead of storing it into the interpreter and
     reading it back;
   - keep the depth as a local passed down the typed calls, with the
     interpreter's counter written once when typed code is entered from
     outside, not per call;
   - take the stack probe only every 8 or 16 levels: a release typed
     frame is small, and the probe is there for debug frames;
   - then track (f): measure a release call's stack (`tests/stack.rs`
     run with `--release`) and raise `MAX_DEPTH` in release builds to
     what the 1 MB Windows main thread allows with room to spare. Today
     recursion stops at 128 deep in every build; Dart stops at about
     57 000 and the JVM at about 21 700.
   Proof: the corpus (`deep(0)`, `chain`, `burn()` hold depth, failure
   place and step count), the gate, and `fib(18)` benched before and
   after each change. The error words for too deep stay the same; the
   number in them changes with the limit, which the owner should see.

   **10.3 done 2026-09-28.** Each change measured on its own, release,
   `fib(18)` in µs per call, best of 7:

   | Change | `fib(18)` | Kept |
   |---|---|---|
   | Before (10.2) | 40.1 | |
   | Step count passed to the callee and returned with its value | 40.3 | yes, it is what the next row needs |
   | Depth as a local `__d` passed down; the full check (`cx.deeper_at`) only at the top of a run, at the limit and every 16 levels (`aot::PROBE_EVERY`) | 33.0 (1.2x faster) | yes |
   | The failure boxed, so the result fits two registers | 36.4 | no, slower |
   | The same, the count through a pointer | 39.4 | no, slower |

   So `fib(18)` is 33 µs, three runs agreeing (Dart 23.9, the JVM 19.8),
   and `sum(10000)` 5.5 µs, `powers(10000)` 380 µs, unchanged. Typed calls
   no longer touch the interpreter's depth counter at all; `cx.enter()` and
   `cx.leave()` remain for calls made from untyped code.

   Track (f), measured with the depth limit lifted (`tests/stack.rs`,
   release, the 768 KB stack budget): an interpreted call takes 1.7 KB of
   Rust's stack (451 deep), a compiled untyped one 2.1 KB (376 deep), a
   typed one 0.4 KB (1885 deep). So the stack, not `MAX_DEPTH`, is what
   stops a release build short of Dart's 57 000. `MAX_DEPTH` is now 256
   in a release build, 128 in a debug one: 256 leaves room under the 376
   of the heaviest call measured, for calls made from deep inside an
   expression, which take more. The words are the same, "Stack overflow:
   calls nested too deep (at most 256)", only the number changed. A new
   test, `typed_calls_stop_where_the_interpreter_does`, shows typed code
   counting depth as the interpreter does: both stop `chain` at 253 in a
   release build. The gate is green (1116 tests).

   **10.4, recursion as deep as Dart's (owner, 2026-09-28: smaller
   frames).** Not a
   larger number: the stack runs out first. Two ways, which can be
   combined, and the choice is the owner's because the first adds a
   dependency and behaves differently on the web:
   - a stack that grows: `stacker::maybe_grow` (what rustc uses) at each
     call, which moves onto a new heap segment when the stack is nearly
     used. Native only: on wasm it does nothing, so the web keeps today's
     limit. The check is the same compare the stack probe makes now.
   - smaller frames: an interpreted call is 1.7 KB and a compiled untyped
     one 2.1 KB of Rust's stack; `Interp::expr` split again, and the large
     `Flow` kept out of the frames that recurse, would buy maybe 2 to 4
     times, not 150.
   Proof either way: `tests/stack.rs` at the new depth, in both profiles,
   and the cost bench unchanged.

   **The plan for track (c), records as slots, written 2026-09-28 for the
   next session.** Guidance, as the rest of step 10 is: check it against
   the code and reorder where the code says otherwise. Both decisions it
   needed are taken (fields in declaration order; a write to an
   undeclared field is an error).

   - (c.0) **Measure first, and take the cheap win.** `for item in list`
     copies the whole array before walking it (`items_of` clones the
     `Vec`, and compiled code's `aot::items` calls it), so walking 10 000
     records is 10 000 reference counts taken and dropped before any
     field is read. Walk the shared array by index instead. Then profile
     the script-cost sum (3.64 ms a frame compiled) to see what is left:
     field lookup by string, the clone of each value read, the dynamic
     `V` arithmetic. That split says how much (c) can buy; without it the
     200x is a guess about where the time goes.
   - (c.1) **The value.** Recommended: one shared form for every record,
     `V::Rec(Rc<Shape>, Rc<[V]>)`, where a `Shape` holds the field names
     in declaration order and a name-to-slot index, made once per record
     type and shared. Rust structs per record type were considered and
     not recommended: the interpreter, the standard library, the native
     bridge and templates all need one form they can read without the
     generated code, so structs would be boxed and unboxed at every edge.
     Maps (`{ [string]: T }`, `Map<K, V>`) stay as they are; a literal
     becomes a record or a map by its type, as the IR's `ExprKind::Map`
     already says.
   - (c.2) **The interpreter by name, through the shape**, with nothing
     compiled changed yet. Every place that meets `V::Map` today (about 30:
     `interp/mod.rs`, `stdlib.rs` with `keys`/`values`, `value.rs` with
     display, equality and the template `Value` conversion, `native.rs`)
     learns `V::Rec`. Equality compares by field name, so two records
     with the same fields written in different orders stay equal.
     Display, `keys`, `values` and `for` go in slot order, which is the
     one change an author sees (decided). The differential proof and the
     gate hold everything else; expect snapshot and test text that shows
     a record to change order, and check each is only that.
   - (c.3) **Compiled reads by slot.** Where the checker knows the record
     type, `item.price` compiles to a check that the value's shape is the
     expected one (a pointer compare, as the typed bodies check their
     arguments' kinds) and a read of slot `k`; any other shape falls back
     to the lookup by name. Writes the same way; an undeclared field fails
     with `kind` `"type"`.
   - (c.4) **Typed record fields in typed bodies.** An `int` or `float`
     read from a slot joins the typed arithmetic of 10.1, so `t +=
     item.price * item.qty` becomes plain Rust inside the loop. That is
     where the record sum should come close to Dart.

   Decided for (c.2) (owner, 2026-09-28): **an optional field that was
   never given reads as `none`** (`bob.pet` on a `User` built without
   `pet` is `none`, where today it fails "there is no `pet` on that
   value"), as TypeScript, Kotlin and Swift read one; it is a slot
   holding `none`, with no hidden "never set" marker. **A field holding
   `none` that the type marks optional is left out when a record is
   shown, and by `keys`, `values` and `for`**, so what an author sees
   stays as today (`${bob}` is `{ name: "bob" }`, `keys(bob)` is
   `["name"]`), as JavaScript's `JSON.stringify` leaves out `undefined`.
   `bob.pet.name` still fails, at `.name` on `none`, and the checker
   should refuse it first once watchlist 36 is fixed. A union of records with different fields
   (a `Result`, a discriminated union) has a shape per member, so a read
   through the union's type goes by name, not by slot. Proof for every
   step: new corpus cases for records written and read in both engines,
   compared field order, equality across orders, undeclared writes, and
   the script-cost sum and filter re-measured after each step.

   **(c.0) to (c.2) done 2026-09-28.** The records are now measured in the
   cost bench itself: corpus script 13 holds the script-cost apps' sum over
   10 000 records and filter of 2 000 (`fill()` makes them), timed with
   everything else. Before anything changed, release, µs per call: `sum`
   4026 interpreted and 2609 compiled (Dart 17.7), the filter 835 and 463
   (Dart 62, the JVM 17).

   What (c.0) found, with probe functions timed one at a time on the
   compiled engine: walking 10 000 records with nothing read took 385 µs,
   a range loop of the same length 200, and reading one field added about
   1700 µs, 170 ns a read. The lookup is not slow as code: a record was an
   `Rc<BTreeMap<String, V>>`, so a read went from the value to the map, to
   a tree node, to each key's own text on the heap, a chain of dependent
   loads across several megabytes. Slots are the fix, because a record's
   values then sit together behind one pointer. (c.0) itself, `for` walking
   a shared array by index (`Items`) instead of copying it first: 385 to
   335 µs for the walk, nothing measurable on `sum`. A write to the list
   inside the loop copies it once, at the first write, as a copy-on-write
   value does, so the loop still walks what it started with.

   (c.1) and (c.2): `V::Rec(Rc<Record>)`, a `Record` being an
   `Rc<Shape>` and the values in a `Box<[V]>`, and `rux_ir::shape::Shape`
   the names in slot order, which of them are optional, and whether the
   shape is closed. A `{ }` literal is lowered by its type
   (`ExprKind::Record`, beside `ExprKind::Map`):
   - its type a map (`{ [string]: T }`, `Map<string, T>`): a map, sorted,
     as today;
   - its type a declared record type (`type Item = …`, or one member of a
     union such as a `Result` half): a record in the declared order, its
     shape closed and shared (interned per thread), so a write of a field
     the type does not declare fails with `kind` `"type"` even where
     `any` let it past the checker;
   - anything else (a record type written inline, a literal's own, `any`):
     a record in that type's order or the order written, open, so a new
     key makes it the map it always was.
   The runtime's values are the one part not planned for: a record leaves
   script as `rux_reactive::Value::Map` (ordered) for the template, and
   comes back through `V::from_value` for a binding, a handler baked with
   the row's literal, or a prop. It used to come back a sorted map, which
   would have shown `{{ item }}` sorted. Now a runtime map comes back an
   open record in the order it went out: a record keeps its type's order,
   and a map, which went out sorted, stays sorted. Errors (`catch e`) are
   records of `message` then `kind`, and `Ok`/`Err` of `ok` then `value`
   or `error`, the order their types declare.

   What an author sees change, checked by running corpus script 13 on the
   commit before and diffing: a declared record shows in its type's order
   (`type Pt = { y, x }` shows `y: 2, x: 1`, was `x: 1, y: 2`); an `Err`
   shows `ok: false, error: no` (was `error: no, ok: false`); an optional
   field never given can now be written (`p.label = "a"` failed with "no
   `label`"); and **one change the decisions did not name: a `{ }` with no
   declared type shows in the order written** (`{ b: 1, a: 2 }` shows `b:
   1, a: 2`, was sorted). It follows from decision 2 (JavaScript shows a
   literal as written) and is what keeps a record's order through a
   baked handler, whose literal no type describes, but it is the owner's
   to confirm. `crates/rux-script/tests/records.rs` holds each decided
   behaviour; corpus script 13 holds them on both engines.

   Release, µs per call, compiled code still reading fields by name:

   | Call | Interpreted before | after | Compiled before | after |
   |---|---|---|---|---|
   | `sum(items)`, 10 000 records | 4026 | 2916 (1.4x faster) | 2609 | 1293 (2.0x faster) |
   | `over(few, 40)`, filter of 2 000 | 835 | 817 | 463 | 398 (1.2x faster) |

   Not done by these steps, and why: a native record coming back from
   Rust arrives as a keyed map with no order (`rux_native::Any::Map`), so
   it stays a sorted map until the bridge carries its declared order;
   `dict()` in script 13 showed that `m["c"] = 3`, a new key written into
   a map, fails with "there is no `c`" (before this change too: watchlist
   40).

   **(c.3) and (c.4) done 2026-09-28.** (c.3): where the checker knows a
   value is a record, compiled `item.price` reads slot `k` when the
   record's slot `k` holds `price` (`aot::slot_is`), and by name when not;
   a record literal is built into its slots by compiled code rather than
   handed back. The guard asks the slot's name, not the shape's pointer,
   so a record back from the runtime with its fields in the same order
   takes the fast path too. (c.4): a typed body may now hold records and
   arrays (`K::V`, the interpreter's own value) and do three things with
   them: walk an array with `for` (the item borrowed where it is when the
   loop never writes its variable), read an `int`, `float` or `bool` field
   by its slot, and ask `.length`. There the guard is one pointer compare
   against the shape a record of that type has when made in script,
   fetched once as the body starts, with the slot's name as the fallback.
   Where a value is not what the checker said, the body bails
   (`aot::bail`): it has done nothing anything else can see, so `f{id}`
   puts the step count back and runs the untyped body from the start. A
   record from the runtime holds every number as a `float` (the runtime has
   one number), so `sum` over rows handed in from a template runs untyped;
   `records_from_the_runtime_run_the_untyped_body` holds both engines to
   the same answer, fields in order and reversed. That test found the one
   defect on the way: the bail was told apart by comparing the address of
   a `const` string, which Rust does not promise is one address, so a bail
   escaped as the failure "a typed body gave up". It compares the text now.

   Release, µs per call, each change measured on its own:

   | Change | `sum(items)`, 10 000 records | Filter of 2 000 |
   |---|---|---|
   | Before track (c) | 2609 | 463 |
   | (c.0) to (c.2), slots, compiled reads by name | 1293 | 398 |
   | (c.3) compiled reads by slot, the guard by name | 525 | 342 |
   | (c.4) a typed body, the guard by name | 100 | 350 |
   | the guard by the shape's pointer | 80 | |
   | the loop's record borrowed, not copied out | 45 (58x faster than before track (c)) | 347 (1.3x faster) |
   | Dart AOT / the JVM | 17.7 / 19.1 | 62 / 17 |

   The record sum is now 2.5 times Dart's and the JVM's, from 150 times.
   The filter barely moved: its time is a closure called per item, which
   is track (d). Everything else in the bench that touches records got
   faster with it (`looped()` 1.02 to 0.67 µs, `local()` 3.19 to 2.38),
   and nothing got slower (`!bump(1)`, 3.9 µs in the first run of the day,
   is 4.2 to 4.3 both on the commit before track (c) and after it). Driven
   in the window too: docs/08-user-tests.md,
   "Records in their declared order".

   **10.5, small follow-ups from track (c), each its own step, before
   track (d):**
   - (10.5.1) Compiled writes by slot. `item.qty += 1` in compiled code
     still goes through the interpreter's walk by name (correct, and
     through the shape, so no longer slow in the old way); (c.3) did
     reads only. Prove with a bench case of a write in a loop.
   - (10.5.2) `make(1000)` takes 750 µs, 750 ns a record, linear. Three
     costs in it, none of them the record: a backtick string displays each
     text part into a new `String` and then allocates the result again; `%`
     of two `int`s has no compiled fast path (it calls `cx.binary`); and
     a method call builds its argument vector. Each is small; measure
     each alone.
   - (10.5.3) A record in one allocation. Today it is an `Rc<Record>`
     holding the shape and a `Box<[V]>`, two allocations, so reading a
     field is two cache lines. One allocation (the shape and the slots
     behind one `Rc`) is what is left between 45 µs and Dart's 17.7 on the
     record sum. It needs either an unsized struct built by hand (unsafe) or
     a small inline array, so it is a choice to make, not a certainty.
   - (10.5.4) A native record keeps its declared order. `rux_native`
     hands a Rust struct back as `Any::Map`, keyed, so it arrives a sorted
     map. The bridge knows the struct's field order (`ItemKind::Record`);
     carrying it makes a native record a record like any other.

   **Decided by the owner, 2026-09-28:** a `{ }` with no declared type
   shows in the order written (confirmed); a map gains a key with
   `m.set(k, v)`, and `=` only writes a key that is there (watchlist 40
   closed as the rule, not a defect: the failure's words now say to use
   `set`); and 10.4 is done by smaller frames, not a stack that grows.
   Order from here: 10.5.1 to 10.5.4, then 10.4, then track (d).

Steps 2 to 5 change nothing an author sees except the decided syntax and
types, which is what made them safe to take in order.

## Settled 2026-09-26

1. **`catch e` binds a Rux `Error`** (decided): `{ message: string, kind:
   string }`. A native `Err` arrives as an `Error` whose `kind` names the Rust
   error; `throw` in Rux throws an `Error` too.
2. **`{ [string]: T }` stays** (decided) as a second spelling of
   `Map<string, T>`.
3. **`rux fmt` can normalise imports** (decided), per a `rux.toml` setting.
4. **The interpreter gets its own benchmarks from its first commit** (decided).

## Proposed while writing this reference

Filled in on 2026-09-26 where the decisions left a gap. Each is marked
(proposed) where it appears; this is the list to overrule from.

1. `'x'` is a string; there is no character type.
2. `===`, `!==`, bitwise operators, `|x|` closures and `#{` are removed.
3. `int` overflow throws, in every build.
4. `parseInt`/`parseFloat` give `int?`/`float?`; `Number()` is retired;
   `trunc`/`round`/`floor`/`ceil` convert a `float` to an `int`.
5. Assigning an `any` to a typed name checks it there.
6. `type_of` is retired in favour of `is`.
7. `Result<T, E>` is a discriminated union read with `r.ok`, built with
   `Ok`/`Err`.
8. `throw` takes a string or an `Error`; `catch` may omit its name; no
   `finally` yet.
9. No overloading by arity; a `fn` and a variable may not share a name.
10. A function sees its own names and its file's, never its caller's; a method
    call sees the same.
11. A closure writes the real signal.
12. A plain top-level `let` is read-only.
13. An async fn is stopped when the instance that started it unmounts.
14. `export` marks what a module shares; an exported signal is read-only
    outside its module.
15. A number field's `r-model` writes a `float`.
16. `is` binds at its level on both sides (the fork gives it none on its right).

## Settled 2026-09-27 (was: Open for the owner)

Raised after step 6. The owner took all three recommendations on
2026-09-27: `export let` keeps both meanings (built with step 7), `loop { }`
comes back with `do`/`until` and `break value` kept out and the `(x, i)` form
written into [Control flow](#control-flow-kept-one-addition), and `switch`
stays the one matching form. As they were put:

1. **`export let` keeps both meanings** (decided). `export let items =
   signal([])` shares state: importers read it and a binding that reads it
   updates, and only the module's exported functions change it. `export let
   RATE = 0.2` shares a constant, since a plain top-level `let` is already
   read-only. This is JavaScript's rule too: an importer cannot assign to
   what it imports. The weak spot is that `let` means state or constant by
   whether `signal(…)` follows it, which was already true inside a file.
   Rejected alternatives: `export const` (a second word for what `let`
   covers), a keyword of its own for shared state (new syntax), and
   exporting only functions (a binding would call `cart.items()` and lose
   its subscription).
2. **Loops were never decided one by one** (decided: `loop` back, the rest as proposed). What runs today: `for x in c`,
   `for (x, i) in c` (item and index, missing from
   [Control flow](#control-flow-kept-one-addition)), `for i in 0..n` and
   `0..=n`, `for c in "text"`, `while`, `break`, `continue`. Parsed and then
   refused as "not part of Rux", a decision taken in bulk at step 4 ("the
   fork's syntax gets no IR node"): `loop { }`, `do { } while c`,
   `do { } until c`, and `break value`. There is no C-style `for`.
   Proposed: bring `loop { }` back (clearer than `while true`), keep
   `do`/`until` and `break value` out, and write the `(x, i)` form into the
   reference.
3. **`switch` stays the one matching form** (decided). It is an
   expression, takes `a | b` alternatives, ranges and an `if` guard per arm,
   and must cover a union with no `_` arm. `match` is only a reserved word.

## Decided without the owner (2026-09-27)

Step 8 was planned and built overnight on 2026-09-27 while the owner was
away, on their instruction to go ahead. Each choice below was taken without
them and is listed to be overruled. Each says what it is, what it was chosen
over, and why.

1. **Three crates.** `rux-native` is what an app's Rust depends on, renamed
   `rux` in its `Cargo.toml` so the attribute reads `#[rux::export]`.
   `rux-native-macros` holds the attributes. `rux-bindgen` reads Rust source
   with `syn`, and both the macros and the CLI use it, so what the checker
   is told and what the compiled code does come from one mapping. Chosen
   over one crate, which would make every app compile `syn` for the
   checker's sake, and over two, which would copy the mapping.
2. **The Rust lives in `native/` beside `rux.toml`**, an ordinary library
   crate. No `rux.toml` key: the manifest adds a key when something needs
   another place, as it always has.
3. **A native module is a Rust module.** `pub mod database` in the crate is
   `native::database` in Rux (`native::db::users` for a nested one); items at
   the crate root are the module `native` itself (`use native::{greet};`).
   Every module on the way must be `pub`. Chosen over an attribute naming the
   module, which says the same thing twice.
4. **Registration is generated, not discovered.** The CLI reads the crate,
   so it knows every export, and the wrapper crate it generates registers
   each one by path before the first document loads. Chosen over
   `inventory`/`linkme`-style link-time collection: no code runs before
   `main`, nothing depends on the linker keeping a section, and it works the
   same on Android and the web.
5. **No interface file on disk.** The reference proposed one. Reading a
   crate with `syn` takes milliseconds, so `rux check`, the loader and the
   editor (through `rux check`) read the Rust source each time and can never
   see a stale copy. `rux native` prints the interface in Rux syntax for a
   person to read.
6. **The boundary value is `rux::Any`**: `None`, `Bool`, `Int(i64)`,
   `Float(f64)`, `Str`, `Array`, `Map`, `Resource`. It is also the Rux
   `any` escape (the reference's third level), so a Rust function taking or
   giving `rux::Any` is typed `any`. Integers keep their `int`-ness across
   the boundary, which `rux_reactive::Value` (one `f64`) could not.
7. **What crosses**: `bool`; every Rust integer type as `int`, a value that
   does not fit throwing `kind` `"overflow"`; `f32`/`f64` as `float`;
   `String`/`&str` as `string`; `Option<T>` as `T?`; `Vec<T>`/`&[T]` as
   `T[]`; `HashMap`/`BTreeMap<String, T>` as `Map<string, T>`; `()` as
   `void`; `Result<T, E>` as `T`, throwing; a `#[rux::export]` struct as a
   record type of the same name; a `#[rux::resource]` as an opaque type.
   Anything else is a compile error at the export saying why.
8. **A value struct crosses whole**, so every field must be `pub`. A struct
   with private fields is refused with a message pointing at
   `#[rux::resource]`. Chosen over copying only the `pub` fields, which
   would build a Rust value from a Rux record with fields missing.
9. **A resource** is `Send + Sync + 'static`, held in an `Arc`. Rux sees an
   opaque named type and its exported methods, may keep it in a signal and
   pass it around, and cannot look inside it or make one. Two are equal
   when they are the same resource. Its methods take `&self`; mutation is
   Rust's business (a `Mutex` inside), because Rux may hold the same handle
   in several places. It is dropped when the last Rux value holding it is.
10. **Methods.** `#[rux::export] impl T` exports its `pub fn`s; a method
    taking `&self` is a method in Rux (`p.discountedPrice(0.1)`). One
    without `self` (`Database::open`) is a function of the module, under its
    own name (`database.open(…)`), since Rux has no static methods; two
    exports of one module with the same Rux name are an error at the export.
    `#[rux(name = "…")]` overrides any name; `#[rux(skip)]` keeps a `pub fn`
    out. The reference's `#[rux::hide]`/`#[rux::only]` are not built: `pub`
    already decides.
11. **Errors.** A returned `Err(e)` throws an `Error` whose `message` is
    `e.to_string()` and whose `kind` is `E`'s type name. A panic inside a
    native call is caught and thrown with `kind` `"panic"` (not on the web,
    which aborts). A value of the wrong type handed to a native function from
    `any` throws `kind` `"type"`.
12. **`async` exports** are awaited from an `async fn`; calling one without
    `await` is an error. Each call's future runs off the UI thread: by
    default on a thread of its own, driven by a small `block_on` in
    `rux-native` (no executor dependency); an app with its own runtime
    (tokio) installs it once with `rux::set_spawner`. Futures must be
    `Send`. Not built: async exports on the web, which has no threads.
13. **`#[rux::init]`** marks one function run once before the first
    document loads, for setting a spawner or opening a pool.
14. **`host::` is retired.** `host::name()` is an error that says to import
    a native module. `rux_runtime::host::register_async` and
    `Builder::host_number` are replaced by registering a native module from
    Rust (`rux_native::Module`), which is also what the macros generate.
15. **Security.** Native code is reached only through an import; the
    playground and `rux check` have no native code (the checker has the
    signatures only). Handles are `Arc`s, never numbers, so Rux cannot
    forge one. Generated code has no `unsafe`. Every integer conversion is
    checked. Not built yet: a native module declaring the platform
    permissions it needs, which waits for the first mobile API that needs
    one.
16. **`rux run` with a `native/` crate** builds a dev host (the `rux build`
    dev wrapper, linked with the crate) and runs that; `.rux` files hot
    reload as before, and a change under `native/src` rebuilds and restarts
    the window.

Taken while building it:

17. **`rux check` never runs the app's Rust.** It still runs each document's
    top level, so a declared (read, not compiled) native function answers the
    empty value of its declared type: `0`, `""`, `none`, `[]`, a record of
    those, a placeholder resource. Chosen over building the crate to check a
    document (minutes, and running code a checker should not) and over
    skipping the top level (the rest of the document would go unchecked).
18. **The generated wrapper crate is named `rux-app-<name>`** and declares its
    own `[workspace]`. The first because an app's `native/` crate is likely
    to carry the app's name, and cargo cannot hold two packages of one name;
    the second so an app inside another cargo workspace builds.
19. **A native method whose receiver's type was not known where it was
    lowered** (a handler's or a binding's own expressions are `any`) is found
    at run time from the value: a resource by its type, a record by being the
    one native record type with such a method whose fields it has.
20. **`native/` is never embedded in a build.** A release carrying its own
    source is a leak, not an asset.
21. **A build honours `CARGO_TARGET_DIR`, and one against a checkout starts
    from the checkout's `Cargo.lock`**, so an app can share a target that is
    already compiled, and builds with the versions Rux was tested with.
22. **The linker's words win at the top level.** A top level that fails on a
    name the linker already refused reports the linker's message
    ("`native/shop` does not export `catalog`: it exports …") rather than the
    interpreter's ("`shop` is not defined").

Taken planning step 9 (2026-09-27):

23. **Compiled code runs inside the interpreter, function by function.** The
    alternative, generating a whole program with its own state and runtime
    seam, would duplicate reactivity, tasks, components and hot reload, and
    could not ship until it did everything. This way a file with one
    function the generator cannot compile still gets the rest compiled, and
    every step can be measured on real documents.
24. **A new crate, `rux-codegen`**, reading `rux-ir` and writing Rust text.
    It depends on nothing that runs; the generated code depends on
    `rux-script`'s `aot` module, the one surface it may call, so the
    interpreter's internals stay private.
25. **Keyed by a hash of the source text, never trusted without it.** A
    release build embeds its documents, so the text is the text the code was
    generated from; anything else (a hot-reloaded file, a document the build
    did not see) falls back to the interpreter. FNV-1a over the bytes, which
    is fast and has no dependency; this is a cache key, not a security
    boundary, since the compiled code is part of the same signed binary.
26. **The step budget stays.** Compiled loops and calls count steps as the
    interpreter's do, so a runaway loop in a release build is stopped with
    the same error instead of freezing the window.
27. **Values stay the interpreter's `V`** at every boundary: arguments,
    results, globals. Only a local the IR types as `int`, `float` or `bool`
    may be a plain Rust value inside one function (9.2).
28. **Functions first; template pieces and `async fn` bodies later.** A
    handler usually calls a function, where the work is; a piece's frame is
    decided by the runtime at run time, and an `async fn` runs from its
    resumable form. Both stay interpreted until the rest is proven.
29. **Proven differentially**, as steps 2 to 5 were: both engines, same
    inputs, same answers, over every script the project has.
30. **The generated code carries no `unsafe`**, like the macros' (decision 15).
