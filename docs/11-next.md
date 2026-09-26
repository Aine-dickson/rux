# The Rux language

The reference for the script language Rux owns: what goes inside `<script>`,
in a script-only module, and in a `{{ }}` binding or an `@tap` attribute.

**Status: the reference for the language being built. None of the changes
below run yet.** [Script](./07-script.md) and [Types](./10-types.md) describe
what runs today, on the rhai fork. This document describes the language that
replaces it: it keeps what those two built where it still holds, and marks
every place that moves. When the build finishes, this becomes the only script
reference and those two retire into history.

The direction behind it (Rux's own parser, checker, typed IR and interpreter,
Rust code generation for release, no rhai) is recorded in
[Architecture](#architecture) at the end, with the build order.

## How to read the marks

Every section and most rules carry one of three marks:

| Mark | Means |
|---|---|
| **Kept** | Runs today on the fork and stays as it is. The section in [Script](./07-script.md) or [Types](./10-types.md) it comes from still describes it |
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
  side they are a `float`. `**` is power, and is a `float` unless both sides are
  `int` and the right is not negative (proposed).
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
cannot be shadowed, and nothing can subscribe to it (Kept). `rux fmt` rewrites
`null` to `none`.

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

**One quirk is marked for step 3** (Changed, proposed). In the fork, `is`
binds as `<` does on its left but has no precedence on its right, so
`a == b is int` is `(a == b) is int`, where the table says
`a == (b is int)`. Step 3 gives `is` its level on both sides.

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

`if … { } else if … { } else { }`, `while cond { }`, `for x in c { }`,
`for i in 0..n { }`, `break`, `continue`, `return`, `return value`.

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
proposed). Today `signal()` is identity and a plain top-level `let` is state
anyway, so the marker marks nothing. Making the unmarked form read-only gives
`signal` a meaning without adding `const`: writing one is an error that
suggests `signal(…)`. Before this is switched on, the repository's apps and
examples are surveyed for a plain top-level `let` that is written, as the
caller-scope rule was.

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
use types::Task;
import type { Task } from "./types";

use components::card;
import { card } from "./components/card";

use stores::cart;
import { cart } from "./stores/cart";
```

`use` and `import` are both accepted and mean the same. `import type` is the
form for types; `use` tells a type by its capital letter (Kept). `as` renames
in both: `use stores::cart as basket`, `import { cart as basket } from
"./stores/cart"` (proposed).

Resolution is Kept: beside the importing file first, then the project root. No
`super::` and no `..`. `rux fmt` can rewrite every import to one form, chosen
by `imports = "use"` or `imports = "import"` in `rux.toml`; without the setting
it leaves both alone (decided).

### Script-only modules (decided)

A file with a `<script>` and no `<template>` is a module: functions, types and
state, no view. It may be written as a bare `.rux` file with no `<script>` tag
at all (proposed), since there is nothing else it could hold.

- **`export` makes a declaration visible to importers** (proposed):
  `export fn`, `export async fn`, `export type`, `export let`. Anything not
  exported is private to the module.
- A types file is a module that exports only types (Kept, as the `types.rux`
  files that exist today, which now need `export` on each type; `rux fmt`
  adds it).
- A module is loaded once, the first time anything imports it, and its
  top-level statements run then.

### Stores (proposed reading of the owner's "Pinia-like" intent)

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
  module** (proposed). A binding that reads `cart.items` subscribes to it like
  any signal. Changing it from outside goes through the module's exported
  functions, which is what makes a store's rules live in one place.
- A component's own state is still changed only inside the component (Kept).
- A factory that gives each caller its own copy can come later.

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

### Reaching it (decided shape, proposed keyword)

The keyword is **`native`**, the same in both module forms:

```rux
use native::database;
import { database } from "native/database";
```

`host::` is retired. "Native" says what the thing is: implemented in Rust, not
in Rux.

### Exporting from Rust (decided in principle)

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
- `#[rux::hide(…)]` and `#[rux::only(…)]` trim what Rux sees.
- A type Rux cannot represent in an exported signature is a **compile error at
  the export** saying why, never dropped silently.
- There is **no escape hatch for calling unexported Rust on a Rux value**. A
  missing capability is written as a small exported Rust function.

### The interface file (proposed)

Exports are read from the Rust **source** (with `syn`), not from a build, into
a generated interface file per crate. The checker, `rux check` and the editor
read that file, so completion, hover and errors on native calls work within
seconds of saving the Rust, without waiting for `cargo`.

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
  value and is allowed (proposed).
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
4. **The typed IR.**
5. **Rux's interpreter on the IR**, replacing rhai for everything, with the
   test suite as the proof that behaviour did not move. Rhai is deleted here.
6. **`async fn` / `await`** in the interpreter.
7. **Script modules and stores.**
8. **`#[rux::export]`, the interface file, `native` modules.**
9. **Rust code generation** for release builds, native then wasm.

Steps 2 to 5 change nothing an author sees except the decided syntax, which is
what makes them safe to take in order.

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
