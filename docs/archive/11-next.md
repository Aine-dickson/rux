# The Rux language

The reference for the script language Rux owns: what goes inside `<script>`,
in a script-only module, and in a `{{ }}` binding or an `@tap` attribute.

Rux parses, checks and runs its own script: a parser (`rux-syntax`), a checker
and interpreter (`rux-script`) and a typed IR (`rux-ir`), with Rust code
generation for release builds (`rux-codegen`). [Script](./07-script.md) is the
guide to using it, with the runtime calls (files, sounds, settings, the
clipboard); [Types](./10-types.md) is the design of the type checker. This
document is the language itself: its grammar, types, modules and native
boundary, and how it is built.

## Not built

A rule of this reference that the parser, checker or interpreter does not
enforce carries the mark **Not built**, and says what happens instead. Every
rule without the mark runs.

## Principles

1. **Rux owns the language; Rust is the substrate.** Rux types are the
   contract a Rux author sees. Rust types are how the compiler represents them.
   A Rux author never needs ownership, lifetimes, traits or `Arc`.
2. **Borrow famous syntax, exactly, and only where it earns its place.**
   `async`/`await`, `try`/`catch`, `<T>`, `import`, `reduce(fn, init)` are
   taken as the world already knows them. Rux is not "JavaScript compiled to
   Rust": it keeps `signal`, `let`, `mounted { }` and its template language.
3. **One way to say a thing.** Where two spellings would mean the same, one is
   chosen. The named exceptions are `use`/`import`, `T[]`/`Array<T>`,
   `T?`/`Option<T>` and `{ [string]: T }`/`Map<string, T>`. Some spellings of
   the old engine still run beside the chosen one; each is marked **Not built**
   where it appears.
4. **The boundary to Rust is typed and costs nothing at run time.**
5. **Types are checked, not erased.** Every expression has a type the compiler
   knows, and the same meaning in the interpreter and in a compiled build.

## The three places script runs

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

### Comments and whitespace

`// to the end of the line` and `/* a block */`. Whitespace and newlines
separate tokens and mean nothing else. A statement ends with `;`, which may be
left off before a `}` and at the end of a handler or a binding.

### Names

A name is a letter or `_` followed by letters, digits and `_`. Names are case
sensitive.

- **A declared type's name starts with a capital letter.** The built-in types
  are lower case, so the two never collide.
- **Everything else is camelCase by convention**: `pathFor`, `parseInt`,
  `toFloat`. The snake_case names inherited from the old engine (`path_for`,
  `parse_int`, `to_int`, `len()`) are retired: see [Retired names](./07-script.md#retired-names).

### Keywords

Reserved everywhere:

```text
as async await break catch const continue do else export false fn for if
import in let loop none private return switch throw true try until while _
```

Reserved only at the start of a top-level statement, so they stay usable as
ordinary names elsewhere (`let type = 1;` works):

```text
type signal computed effect mounted unmounted prop
```

Words held back or answered with the Rux spelling cannot be names either:
`use is match yield var new this null nil void`, and the built-in functions
`print debug eval call curry Fn`. `null`, `()`, `var`,
`new`, `nil` and `this` are errors that say what to write instead (`none`,
`let`, a literal or a function that returns one). `class`, `interface` and
`function` are plain syntax errors, not answered by name. **Not built:** a
message for those three.

A keyword is an ordinary field name after a dot (`m.for`). As a map key it is
written as a string, `{ "for": 1 }`. Words other languages reserve (`default`,
`case`, `static`, `spawn`, `module`, `with`) are free.

### Literals

| Literal | Type |
|---|---|
| `0`, `42`, `1_000` | `int` |
| `0.5`, `1.0`, `2e3`, `1_000.5` | `float` |
| `0x1F`, `0b101` | `int` |
| `"text"` | `string` |
| `'x'` | `string` of one character |
| `` `a ${x} b` `` | `string` |
| `true`, `false` | `bool` |
| `none` | `T?` for whatever `T` is expected |
| `[1, 2]` | `int[]` |
| `[...a, x]` | a new array: `a`'s items, then `x` |
| `{ id: 1, name: "ada" }` | a record |

**Single quotes hold exactly one character**, and `'ab'` is an error that says
to use double quotes. A handler inside a double-quoted attribute therefore
cannot hold text: name a function and call it from the attribute.
**Not built:** `'text'` as a string of any length. There is no character type.

Escapes in strings: `\n`, `\r`, `\t`, `\\`, `\"`, `\'`, `` \` ``, `\$`, `\xHH`,
`\uHHHH` and `\UHHHHHHHH` (fixed width, no braces). A backtick string decodes
the same escapes. Spreading anything but an array or a string in an array
literal is an error.

**A number literal has no suffix** and no leading `+`. `-1` is the unary minus
applied to `1`.

### Map and record literals

`{ key: value }`, with a name or a string as the key (`{ "x-y": 1 }`), and `{}`
for an empty map. A `{` whose first thing is not `key:` is a block, as after
`if`. No shorthand (`{ a }`) and no spread in a map or record.

`#{ … }`, the old engine's spelling, still parses and `rux fmt` rewrites it to
`{`. **Not built:** refusing it.

## Types

### The shape of it

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
list. On a signal the annotation describes the value, not a wrapper: there is
no `Signal<T>` to write.

### The types

| Type | Holds |
|---|---|
| `int` | A 64-bit signed integer |
| `float` | A 64-bit float |
| `string` | Text |
| `bool` | `true` or `false` |
| `void` | The result of a function that returns nothing |
| `any` | Anything. A union with `any` in it is `any` |
| `"all"` | Exactly that string |
| `T[]`, `Array<T>` | An array of `T`; two spellings of one type |
| `{ id: int, title: string }` | A record |
| `{ note?: string }` | A record whose `note` may be absent |
| `{ [string]: T }`, `Map<string, T>` | A map from text to `T`; two spellings of one type |
| `T?`, `Option<T>` | `T` or `none`; two spellings of one type |
| `Result<T, E>` | A value or an error, kept on purpose; see [Result](#result) |
| `A \| B` | Either |
| `(Task, int) => bool` | A function value |
| `Error` | What `catch` binds: `{ message: string, kind: string }` |
| `Task`, `Page<T>` | A declared or imported type, which may take parameters |

`number` is not a type: it is an error naming `int` and `float`, and `rux fmt`
rewrites it to `float`.

**Not built:** `Set<T>` (there is no `Set` function either), and `Map<K, V>`
with keys other than `string`; each is an error that says so.

Not in the language, and errors if written: number and boolean literal types,
intersections (`A & B`), tuples, anything class-shaped, `interface`, explicit
type arguments at a call, `extends` constraints, conditional and mapped types.

### Declaring a type

`type Name = …;` at the top level of `<script>` or of a module, before or after
its first use. It declares nothing at run time. A type may take parameters:

```rux
type Page<T> = { items: T[], next: string? };
type Loaded<T> = { state: "loading" } | { state: "done", value: T };
```

A union may begin with `|` so a long one lines up.

### Numbers

- **An integer literal is an `int`; a literal with a point or an exponent is a
  `float`.**
- **An `int` widens to a `float`** wherever a `float` is expected, and when the
  two meet in arithmetic: `1 + 0.5` is a `float`. A `float` never narrows to an
  `int` by itself; that takes a conversion.
- **`/` always gives a `float`**: `7 / 2` is `3.5`. `intDiv(7, 2)` is `3`,
  truncating toward zero.
- `+`, `-`, `*` and `%` of two `int`s are an `int`; with a `float` on either
  side they are a `float`. `**` is power, and of two `int`s is an `int`: exact,
  or an `overflow` failure, never a rounded `float`. A negative power of an
  `int` fails when it runs; one written out (`2 ** -1`) is a checker error that
  says to make the base a `float` (`2.0 ** -1`).
- **`int` arithmetic that overflows throws** an `Error` with `kind`
  `"overflow"`, in the interpreter and in a compiled build alike. `intDiv(x, 0)`
  and `x % 0` on `int`s throw an `Error` with `kind` `"error"`.
- **`float` keeps IEEE behaviour**: `1.0 / 0` is `Infinity`, `0.0 / 0` is `NaN`,
  and both display under those names.
- **Converting:** `x.toFloat()` on an `int`; `x.trunc()`, `x.round()`,
  `x.floor()`, `x.ceil()` on a `float` give an `int`.
  **Not built:** failing for `NaN`, an infinity or a value out of range; they do
  not throw.
- **Text to a number:** `parseInt(s): int?` and `parseFloat(s): float?` read a
  leading number as JavaScript's do and give `none` where JavaScript gives
  `NaN`. `Number(s)` still exists and follows JavaScript (`NaN` for text it
  cannot read); `isNaN(x)` asks of a `float`; `String(x)` goes the other way.
- **An index is an `int`.** A `float` index is an error with `.trunc()` as the
  fix.

An unannotated `let n = signal(0);` holds an `int`, so `n = n / 2` is an error.
The fix is to say what it is: `let n: float = signal(0);`.

### The empty value

**`none`**, not `null`. `T?` is the short spelling of `Option<T>`.

- `?.` and `?[` read through a value that may be `none`.
- `??` gives the right side when the left is `none`.
- Narrowing by `x != none`, `if x`, an early return, `r-if`.
- A prop declared `T?` that the tag passed nothing to is `none`.

There is no `undefined`. `none` is a literal, not a variable: it cannot be
shadowed, and nothing can subscribe to it. `null` and `()` are checker errors
that name `none`; they still run as `none`, and `rux fmt` rewrites them.

### Checked, not erased

Every expression has a type the checker knows, and the interpreter and the
compiled build both rely on it. Annotations are erased when the program runs;
the type checking is done by `rux check`, the dev overlay and the editor.

- **A name whose type cannot be worked out is `any`, with a warning.** Writing
  `: any` opts out. **Not built:** making it an error in a release build.
- **`any` is the one dynamic type.** Reading a field that is not there, or
  calling what is not a function, fails when it runs. **Not built:** checking
  an `any` where it is assigned to a typed name or passed to a typed
  parameter; it is accepted unchecked. `x is T` is the way to ask without
  failing.

### Inference

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

A string literal stays a literal type only where something asks for one.
`signal([])`, `signal(none)`, `signal({})` and an unannotated parameter say
nothing, so they are the places an annotation is needed.

**A closure's parameters come from where it is used**: in
`tasks.filter(t => t.done)`, `t` is a `Task`. An arrow that is not an argument
needs its parameters annotated.

### Optional fields, `?.` and narrowing

Everything in [Types, optional fields](./10-types.md#optional-fields-and-) and
[unions and narrowing](./10-types.md#unions-and-narrowing) holds:

- An optional field, a value that may be `none`, and a dictionary key must be
  read with `?.` or `?[`, except inside a region where a check has ruled their
  absence out.
- These narrow: `x == "open"`, `x != none`, `if x`, `load.state == "done"`,
  `"note" in t`, `x is Task`, an early `return`, each arm of a `switch`, and
  `r-if`/`r-elif`/`r-else` in a template.
- A fact about a signal does not outlive a call that writes it, nor an `await`.
- Comparing against a literal a union does not contain, or between types with
  nothing in common, is an error.

`x is string` asks a type at run time. `type_of(x)` is retired.

### `switch`

`switch value { a => …, b => …, _ => … }` is an expression. On a union-typed
value with no `_` arm it must name every member, and an error lists the ones
left out. A `switch` on a discriminant narrows the value in each arm. An arm
may be a block: `"done" => { … }`. Arms take `a | b` alternatives, ranges and
an `if` guard. `match` is only a reserved word.

### `is`

`x is T` checks at run time and narrows. It walks the value against the type,
never throws, and binds as `<` does, on both sides, so `a == b is int` is
`a == (b is int)`. A validator is generated only for the types that appear on
the right of an `is`, and for props.

### Result

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

`Ok(v)` and `Err(e)` build one. `r.unwrap()` gives the value, or throws an
`Error` with `kind` `"unwrap"` whose message carries the error.

## Statements and expressions

### Precedence

Tightest first.

| Level | Operators | Associates |
|---|---|---|
| 1 | calls `f()`, `.x`, `?.x`, `[i]`, `?[i]` | left |
| 2 | unary `!`, `-`, `+`, and `await` | right |
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

Two things differ from JavaScript: **`??` binds tighter than a comparison**, so
`a ?? 0 == 1` is `(a ?? 0) == 1`, and **`in` binds looser than one**, so
`a < b in c` is `(a < b) in c`.

**Assignment is a statement**, not an operator: `a = b = c` is an error. `=`,
`+=`, `-=`, `*=`, `/=`, `%=`, and `x++`/`x--` in statement position only. The
target is a name, a field (`a.b`), or an index (`a[i]`), to any depth.

`==` compares values structurally: two records are equal when their fields are,
whatever order they were written in. `===` and `!==` are accepted and mean what
`==` and `!=` mean. **Not built:** removing them.

The bitwise operators `&`, `|`, `^`, `<<`, `>>` parse and are not part of Rux:
a handler or binding using one is refused at load, and elsewhere the result is
not meaningful. `|x| body` closures are the old engine's spelling and still
work; arrows are the form to write. **Not built:** refusing either `|x|` or
`===`.

There is **no `?:` ternary**: `if` is an expression, `if a { b } else { c }`.

### Truthiness

JavaScript's rules. Falsy: `false`, `0`, `0.0`, `NaN`, `""`, `none`. Everything
else is truthy, **including an empty array and an empty map**, so
`r-if="items"` means "items exists", and `r-if="items.length"` is what was
meant. **Not built:** a checker warning on a condition whose type is an array
or a map.

### Blocks have values

A block's value is its last expression when that expression has no `;` after
it. That is how a function returns without `return`, and how a `switch` arm or
an `if` produces a value:

```rux
fn label(t: Task): string { t.title }
let size = if wide { 24 } else { 16 };
```

### Control flow

`if … { } else if … { } else { }`, `while cond { }`, `loop { }` (until a
`break`), `for x in c { }`, `for (x, i) in c { }` (item and index),
`for i in 0..n { }` and `0..=n`, `break`, `continue`, `return`, `return value`.
There is no C-style `for`. `do { } while`, `do { } until` and `break value`
parse and are not part of Rux: a handler or binding holding one is refused at
load with "is not part of Rux", and a `<script>` body or function holding one
is not refused and does nothing useful.

`for x in c` iterates an array, a range, a string's characters (as
one-character strings), and a map's keys through `keys(m)`. No iterable
protocol is visible to authors. `r-for="(item, i) in list"` binds the index in
a template.

### Errors: `try`, `catch`, `throw`

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

- **`catch e` binds the error.** An error raised by the language, a native
  `Err`, a failed `await`, an `int` overflow or `r.unwrap()` arrives as an
  `Error`, `{ message: string, kind: string }`. `catch { }` without a name is
  allowed.
- **`throw` takes a value.** `throw` of a record with a `message` field
  (`throw { message: "empty title", kind: "input" }`, or `throw e` to rethrow)
  arrives as that record. **Not built:** `throw "empty title"` arriving as an
  `Error` with `kind` `"error"`; a thrown string is caught as that string, so
  `e.message` on it fails although the checker types `e` as an `Error`.
- **`kind`** is `"error"` for failures the language raises, `"overflow"` for
  `int` overflow, `"unwrap"` for `r.unwrap()` on an error, `"type"` for a
  write to a field a declared record does not have, `"panic"` for a panic in
  native code, and the Rust error type's name for a native `Err`.
- There is no `finally`.
- **An error nothing catches** is reported to the dev overlay and `rux check`
  with the file and line. The handler stops there; its writes before the error
  stay.

## Functions

<a id="declaring-kept-with-generics"></a>

### Declaring

```rux
fn label(t: Task): string { t.title }
fn first<T>(items: T[]): T? { items?[0] }
fn refresh() { load = native::tasks::load(); }
```

- **The result type is written `: T`** and may be left off, in which case it
  is inferred from the body. It must be written on a function that calls
  itself, directly or through another.
- **Parameters are never inferred from calls.** An unannotated parameter is
  `any` with a warning. The editor's quick-fix writes in the type the calls
  agree on.
- **Type parameters are always inferred** at the call, from the arguments.
  `first<User>(x)` is not a call form.
- A generic function is compiled once per type it is used with.
- Two functions of one name are not checked for: the later wins. **Not
  built:** refusing a repeated name, and refusing a `fn` that shares a name
  with a variable (the `fn` wins).

### What a function can see

**A function sees its own parameters and locals, and the names of the file it
is written in**: the document's or component's signals, computeds, props and
functions, and what the file imports. Nothing else. A caller's `let` or
`r-for` variable is not reachable, and `thing.helper()` and `helper(thing)`
see the same scope. A function that writes a signal writes the document's
(`fn drain() { level-- }`), and the screen follows.

### Arrows and closures

`x => …`, `() => …`, `(x) => …`, `(a, b) => …`, and `(a: int, b: int): int => …`
with types. An arrow in a variable is called like a function.

**A closure copies the local variables it uses.** It reads the value a local
had when the closure was made, and a write to it changes only the closure's own
copy. A closure does write a signal or the document's state:
`items.forEach(i => total += i.price)` adds to `total` when `total` is a
signal. **Not built:** sharing a local variable between a closure and the
function around it.

## State and the file's own declarations

### `signal`, `let`

```rux
let level = signal(82);
let items: Item[] = signal([]);
```

`signal(v)` marks a top-level `let` as reactive state. A binding that reads one
is a subscription to it.

**A top-level `let` without `signal` is a value that is never written.**
Writing one, or a field or element of one, is a checker error that suggests
`signal(…)`. `const x = 1;` declares a name that cannot be assigned, at the top
level or in a function. A `let` inside a function, handler or block is an
ordinary local.

### `computed`, `effect`

`computed total: float = items.reduce((s, i) => s + i.price, 0.0);` declares
derived state, refreshed in declaration order. `effect { … }` runs when what it
last read changes, and once on load; it is never woken by its own writes. Both
run per instance inside a component. Both are part of the grammar, so a
`computed` may span lines, and an error in one points at its own column.

### `prop`

```rux
prop label: string;
prop price: float = 1.0;
prop kind: "primary" | "quiet" = "quiet";
```

Declared in a component. Required without a default, optional with one. Not
writable from the component. Checked at the tag by the checker and when the
component is built, in release builds too; a route parameter becomes the type
its prop takes. See [Types, props](./10-types.md#props).

### Lifecycle: `mounted`, `unmounted`

Blocks, not functions. `mounted` runs once after the first tree exists and
after the effects; `unmounted` when the document or instance goes. Per
instance inside a component. `unmounted` never runs without `mounted` having
run, and unmounts run before mounts in one build. See
[Script, lifecycle](./07-script.md#lifecycle).

### Intervals

`let t = setInterval(1000) { … }` and `clearInterval(t)`. The handle is an
`int`. An interval belongs to whoever started it and stops with that instance.
It is a block rather than a callback because the block shape is what ties it
to its instance.

## Async

```rux
async fn loadUser(id: int): User {
  let user = await native::api::getUser(id);
  return user;
}
```

- **`async fn` and `await`, and nothing else.** No `Future`, `Promise` or task
  type is ever visible to an author. A call without `await` gives a handle (an
  `int`), and `cancel(handle)` stops the run at its next `await`.
- **The result type is the value awaited**: `: User`, not a wrapper.
- **`await` is allowed only inside an `async fn`.** Handlers, bindings,
  `effect`, `computed` and the lifecycle blocks cannot await.
- **A handler only calls an async function**: `@tap="loadUser(3)"` starts it
  and returns at once. Its writes render as they happen, before and after each
  `await`. `mounted { loadUser(3); }` starts one on load.
- **An async fn may call another with `await`**, and gets its result. Calling
  one without `await` from inside an async fn starts it alongside and does not
  wait.
- **Everything between two `await`s runs without interruption.** There is one
  thread of script, so no lock is ever needed; a signal can change under a
  function only at an `await`, which is why an `await` ends every narrowing of
  a signal.
- **An async fn started by a component instance belongs to it**, as an
  interval does. When the instance unmounts, the function is stopped at its
  next `await` and never resumes, and the Rust future it waited on is dropped.
  One started at document level lives as long as the document.
- **A failed `await` throws** an `Error` where the `await` is, which
  `try`/`catch` handles. One nothing catches is reported as a failing handler
  is.

See [Script, async](./07-script.md#async) for cancelling and the Rust side.

## Modules

### Two spellings, one meaning

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

`use` and `import` are both accepted and mean the same. A name alone imports
the **whole file**: a component's tag, or a module's namespace (`cart.add(p)`),
as Vue imports a component. **Braces pick what the file exports**, in both
spellings. Without braces the last segment of a `use` is always the file;
`use utils::money::format` looks for `utils/money/format.rux`.

**A type is imported with `type`**, never told by its capital letter:
`use type a::T`, `import type { T } from "./a"`. A type keeps its name, so
`as` on a type import is an error. A type imported without `type` loads with a
warning, and `rux fmt` rewrites it.

The string after `from` is the `use` path with `/`: an optional `./`, names,
an optional `.rux`. A file is found beside the importing file first, then at
the project root. No `super::`, and no `..` or `.` in a path. `rux fmt` can
rewrite every import to one form, chosen in `rux.toml`:

```toml
[fmt]
imports = "use"      # or "import"
```

Without the setting it leaves both alone. Imports and `export` belong at the
top level of a file's script; a handler or a binding cannot hold one.

### Script-only modules

A file with a `<script>` and no `<template>` is a module: functions, types and
state, no view. It may be written as a bare `.rux` file with no `<script>` tag
at all.

- **`export` makes a declaration visible to importers**: `export fn`,
  `export async fn`, `export type`, `export let`. Anything not exported is
  private to the module.
- A types file is a module that exports only types. A type imported that the
  module does not export is a warning, and `rux fmt` adds the `export`.
- A module is loaded once per document, however many files import it, and
  its top-level statements run then, before the document's, each module
  after the ones it imports. Modules importing each other in a circle are
  refused.
- A module holds no `computed`, `effect`, lifecycle block or `prop`: those
  belong to a component or a document.

### Stores

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
  module.** A binding that reads `cart.items` subscribes to it like any
  signal. Changing it from outside goes through the module's exported
  functions, which is what makes a store's rules live in one place.
- **`export let` has both meanings.** `export let RATE = 0.2` shares a
  constant, since a plain top-level `let` is already read-only.
- A component's own state is changed only inside the component.
- **A component's names are its own**: what it shares with the document or
  another component is a store, never the document's signals or functions.
- **An `async fn` a module exports belongs to the document** when it runs: it
  writes only the module's state, which outlives any caller.
- **Not built:** a factory that gives each caller its own copy (the "scoped"
  half).

### Values move by copy

Arrays, maps and records are **values**: `let b = a; b.push(1);` leaves `a` as
it was, and a record handed to a function is that function's own copy. This is
the rule JavaScript does not have, and it is what makes a signal's writes
visible: the only way to change state is to write to its name, directly or
through a field or an index, and every such write is tracked. The compiled
build keeps the same meaning with shared, copy-on-write storage, so a copy that
is never written costs nothing.

A **resource** from native code (see [Native code](#native-code)) is the one
thing passed by handle rather than by copy.

## Collections and the standard library

### Rux owns the API

The methods of Rux's built-in types are Rux's. Rust's are never inherited, so a
Rust release cannot change what `Array` offers. Names are JavaScript's, with
JavaScript's answers.

### Arrays

| Method | Does |
|---|---|
| `.length` | A property, an `int` |
| `map`, `filter`, `find`, `findIndex`, `findLast`, `findLastIndex`, `some`, `every`, `forEach`, `includes`, `indexOf`, `lastIndexOf`, `at`, `slice`, `join`, `flatMap` | Leave the receiver alone |
| `reduce(fn, init)` | JavaScript's order |
| `push(x)`, `pop(): T?`, `shift`, `unshift`, `splice(i, n)`, `insert(i, x)`, `remove(i)`, `fill` | Change the array in place, a write to its name. `pop` gives `none` on an empty array |
| `sort((a, b) => …)`, `reverse()` | Change the array in place and give it back |
| `toSorted`, `toReversed`, `concat(other)`, `flat()` | Give a new array |
| `a + b` | Concatenation |

`forEach` is called with `(item, index)` or `(item)`. `.len()` is retired (write
`.length`).

### Strings

`.length`, `charAt`, `at`, `includes`, `indexOf`, `lastIndexOf`, `slice`,
`substring`, `split`, `startsWith`, `endsWith`, `trim`, `trimStart`, `trimEnd`,
`padStart`, `padEnd`, `toLowerCase`, `toUpperCase`, `repeat`. `replace(a, b)`
replaces the first match and `replaceAll(a, b)` every match. Strings are
immutable; every method returns a new one, and `rux check` warns where a
statement throws the result away (`s.replace("a", "b");`).

Numbers have `toFixed`. `Math`, `JSON`, `Object` and `Array` are namespaces
written as JavaScript writes them (`Math.max(a, b)`, `JSON.stringify(v)`,
`Object.keys(m)`, `Array.from`); a file that declares its own keeps it.

### Maps

A `Map<string, T>`, which is also `{ [string]: T }`: `m[k]` (read with `?[`
unless `k in m` is known), `k in m`, `keys(m)`, `values(m)`. A map keeps its
keys sorted. **`m.set(k, v)`** adds a key; `m[k] = v` writes only a key that is
there, and `m.remove(k)` takes one out. **`.length` is on arrays and strings
only.**

**Not built:** `m.size` and `m.delete(k)`, and `Set<T>` with `Set([a, b])`,
`s.has(x)`, `s.add(x)`, `s.delete(x)` and `s.size`.

### Everything else global

| Call | Does |
|---|---|
| `print(x)`, `debug(x)` | Reaches the dev overlay. Not a warning; never fails a build |
| `emit("name")`, `emit("name", payload)` | A component telling its caller something happened |
| `navigate(path)`, `replace(path)`, `back()`, `forward()` | The router; recorded, applied after the handler |
| `pathFor("route", { id: x })` | The path of a named route |
| `query(selector)` | The tree, in a handler only; see [Script, reading the tree](./07-script.md#reading-the-tree) |
| `el.tap()`, `el.focus()`, `blur()`, `el.scrollIntoView()`, `el.setSelectionRange(a, b)`, `el.insertText(t)`, `el.clearHistory()` | Act on an element |
| `setInterval(ms) { }`, `clearInterval(h)`, `cancel(h)` | Intervals and async runs |
| `intDiv`, `parseInt`, `parseFloat`, `Number`, `isNaN`, `String` | Numbers and text; see [Numbers](#numbers) |
| `abs`, `min`, `max`, `sqrt`, `sin`, `cos`, `atan2`, `exp`, `ln`, `log10` | Maths. `abs`, `min` and `max` keep `int` where both arguments are `int`. `log` is base 10 |
| `now()`, `Date.now()`, `Date.time(ms)` and the other `Date` functions | A monotonic clock for timing, and the wall clock in the local time zone; see [Script](./07-script.md#built-in-methods) |
| `settings.get`, `settings.set`, `settings.remove` | The app's own saved values |
| `clipboard.writeText`, `clipboard.readText` | The system clipboard |
| `pickFile`, `pickFiles`, `pickSaveFile`, `File` | The platform's file dialogs, awaited, and the `File` they give |
| `loadSound`, `Sound` | Sounds, awaited |
| `close()` | Closes the app without asking `@close` again |

`host::name(…)` is retired and an error; reach Rust through a `native` module.
The calls from `settings` down are documented in [Script](./07-script.md).

## Native code

### Reaching it

The keyword is **`native`**, the same in both module forms:

```rux
use native::database;
import { database } from "native/database";
```

A native module is a `pub mod` of the app's `native/` crate (an ordinary
library beside `rux.toml`): `pub mod shop` is `native::shop`, and the crate
root is `native` itself. Every module on the way must be `pub`. The type import
is `use type native::shop::Product;`, as for a script module.

### Exporting from Rust

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

Three levels of exposure:

1. **Values.** Plain data (numbers, strings, lists, records of those) becomes a
   Rux `type` automatically and crosses the boundary without conversion. A
   value struct crosses whole, so every field is `pub`; one with private fields
   is refused with a message pointing at `#[rux::resource]`.
2. **Resources.** Anything holding pools, locks, handles or lifetimes is
   opaque: `Send + Sync + 'static`, held in an `Arc`, passed by handle, and
   equal to another only when it is the same resource. Rux sees its exported
   methods only; its methods take `&self`, and mutation is Rust's business (a
   `Mutex` inside). It is dropped when the last Rux value holding it is.
3. **`any`.** `rux::Any` is the boundary value (`None`, `Bool`, `Int`, `Float`,
   `Str`, `Array`, `Map`, `Resource`); a Rust function taking or giving it is
   typed `any`.

Rules:

- Rust `snake_case` becomes Rux camelCase (`discountedPrice`);
  `#[rux(name = "…")]` overrides. A method taking `&self` is a method in Rux;
  one without `self` is a function of the module under its own name.
- What crosses: `bool`; every Rust integer type as `int`, a value that does not
  fit throwing `kind` `"overflow"`; `f32` and `f64` as `float`;
  `String`/`&str` as `string`; `Option<T>` as `T?`; `Vec<T>`/`&[T]` as `T[]`;
  `HashMap`/`BTreeMap<String, T>` as `Map<string, T>`; `()` as `void`.
- A function returning Rust's `Result<T, E>` appears in Rux as returning `T`,
  and its `Err` is thrown as an `Error` whose `message` is `e.to_string()` and
  whose `kind` names the Rust error type. A panic is thrown with `kind`
  `"panic"` (not on the web, which aborts). An `async` Rust function is awaited
  from an `async fn`; calling one without `await` is an error.
- `pub` decides what an exported `impl` shows, and `#[rux(skip)]` keeps a
  `pub fn` out.
- `#[rux::init]` marks one function run once before the first document loads,
  for setting a spawner or opening a pool.
- A type Rux cannot represent in an exported signature is a **compile error at
  the export** saying why, never dropped silently.
- There is **no escape hatch for calling unexported Rust on a Rux value**. A
  missing capability is written as a small exported Rust function.
- Rust can push into a document with `rux::Signal` and `rux::Channel`; see
  [Script, native code](./07-script.md#native-code).

An async export's future runs off the UI thread, by default on a thread of its
own; an app with its own runtime installs it once with `rux::set_spawner`.
**Not built:** async exports on the web (no threads).

### The interface

Exports are read from the Rust **source** (with `syn`, in `rux-bindgen`), not
from a build. The checker, `rux check` and the editor (through `rux check`)
have them within a second of saving the Rust, without waiting for `cargo`.
There is no interface file. `rux native` prints the interface in Rux syntax:

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

`rux check` never builds or runs the app's Rust. `rux run` builds the `native/`
crate into the app and rebuilds and restarts the window when a file under
`native/src` changes. The generated wrapper crate is named `rux-app-<name>`.

## Templates

The template language, `r-*` directives, events and CSS are described in
[As built](./05-as-built.md). What [Types, templates](./10-types.md#templates)
checks is checked this way:

- `r-for` gives its variable the element type, and `r-for="(item, i) in list"`
  gives `i` an `int`; `r-if` narrows.
- A bound attribute is checked against what it takes (`:disabled` a `bool`,
  `:class` a map of `bool`s, a string or a string array).
- `r-model` must name something of the input's value type: `string` for text
  fields, `float` for number and slider, `bool` for checkbox and switch. A
  number field bound to an `int` writes a truncated value and is allowed.
- `event` has the type of its event. Gesture coordinates are `float`s.
- Props are checked at the tag.

A handler is a statement list in the same grammar as a function body, with
`event` bound. A binding is one expression.

## Security

- **Compiled app code has full trust**, as any compiled application does.
- **The interpreter is the sandbox** for anything untrusted: the playground and
  documents fetched at run time. A script that never finishes is stopped at a
  step budget, in the interpreter and in compiled code alike.
- **Native code is reached only through an import.** The playground and
  `rux check` have no native code, and a resource handle cannot be forged from
  script.
- **Not built:** a native module declaring the platform permissions it needs,
  and an Android manifest generated from those declarations.

## If you know JavaScript

1. **`int` and `float` are different types.** `/` always gives a `float`;
   `intDiv` truncates.
2. **Arrays and records are values.** Assigning one copies it.
3. **`none`**, not `null` or `undefined`.
4. **`let` at the top level without `signal` cannot be written.** `const` is a
   local that cannot be assigned.
5. **No object shorthand or spread in a map**: `{ a: a }`, never `{ a }`.
   Spread works in an array.
6. **`.length` on arrays and strings only**; a map has `keys(m)` and
   `values(m)`.
7. **`x++` is a statement.** `let a = x++` is not a thing.
8. **A handler cannot `await`.** It calls an `async fn`, which does.
9. **`if` is an expression**, and there is no `?:`.
10. **Single quotes hold one character**, which is a string of length one.
11. **A closure copies the local variables it uses.** It writes a signal, not
    a local of the function around it.

## Architecture

### The pipeline

```text
.rux ──▶ Rux parser ──▶ type checker ──▶ typed Rux IR ─┬─▶ Rux interpreter
                                                        │     dev, hot reload,
                                                        │     playground, sandbox
                                                        └─▶ Rust code ──▶ rustc
                                                              release: native + wasm
```

- **A typed IR** (`rux-ir`) sits between the checker and every backend. The
  language's meaning is defined once, there.
- **The interpreter** (`rux-script`) is Rux's own and runs the IR in
  development, in hot reload, in the playground and for anything untrusted.
  No other script engine is involved.
- **A release build compiles functions to Rust** (`rux-codegen`). Compiled code
  runs inside the interpreter, function by function: the interpreter still
  loads every file, and where a function has compiled code, keyed by a hash of
  the exact source it was generated from, it calls that instead of walking the
  tree. A function the generator cannot compile is interpreted, so a file is
  never all or nothing. Reactivity, tasks, components and hot reload are the
  interpreter's in both modes, and the step budget counts the same way.
- **Compiled:** functions (including closure bodies, `try`, `switch`, `?.`
  chains, field and index writes, callbacks written in line and `setInterval`
  starts), with typed `i64`/`f64`/`bool` bodies for number-only functions and
  records read by slot. **Interpreted:** the pieces in a template (bindings and
  handlers) and `async fn` bodies. Pieces measured at under 0.2% of a frame, so
  they stay interpreted.
- **The web** compiles the same Rust to wasm.
- **Values at the seam** are the interpreter's `V`. Only a local the IR types
  as `int`, `float` or `bool` is a plain Rust value inside one function.
- The generated code carries no `unsafe`, and every integer conversion at the
  native boundary is checked.

### Build order

| Step | What | State |
|---|---|---|
| 1 | This reference | Done |
| 2 | Rux's own parser (`rux-syntax`), with the type annotations | Done |
| 3 | The checker on the new AST: `int`/`float`, `none`, `Option`, `Result`, generics, modules | Done, except the rules marked **Not built** above |
| 4 | The typed IR (`rux-ir`), its verifier and printer | Done |
| 5 | Rux's interpreter on the IR | Done. A closure copies a local rather than sharing it, and an `any` entering typed code is not checked when it runs |
| 6 | `async fn` / `await`, `cancel(handle)` | Done |
| 7 | Script modules and stores; a component's names are its own | Done. A factory store (the "scoped" half) does not exist, a generic function exported from a module is checked no further than `any` at the call, and `rux check` on a component in a folder with no project root resolves its imports from the component's own directory |
| 8 | `#[rux::export]`, `native` modules, `rux-native`, `rux-bindgen`, `rux native` | Done. Async exports on the web (no threads; `rux::set_spawner` is the way in), permissions declared by a native module, and `--route` and `--preview` for `rux run` of an app with a `native/` crate are not built; a resource kept in a component instance's state comes back as text |
| 9 | Rust code generation for release builds, native and web | Done for functions. Template pieces and `async fn` bodies stay interpreted |
| 10 | Speed track on the compiled path: typed bodies, cheaper calls, records as slots, callbacks in line, frame and patch work | Done through the tracks measured. A bigger main-thread stack is parked |

The diary of each step, with its measurements and commits, is in the
[language build log](./archive/language-build-log.md).

## Decisions that still define the language

- **Rux owns the language; Rust is the substrate.** Rust's release cannot
  change what `Array` offers, and an author never meets ownership, lifetimes
  or `Arc`.
- **`switch` is the one matching form.** It is an expression, takes `a | b`
  alternatives, ranges and an `if` guard per arm, and must cover a union with
  no `_` arm. `match` is only a reserved word.
- **`export let` has both meanings.** `export let items = signal([])` shares
  state that only the module's exported functions change; `export let RATE =
  0.2` shares a constant. An importer cannot assign to what it imports.
- **Loops:** `loop { }`, `while`, `for x in c`, `for (x, i) in c`, `for i in
  0..n` and `0..=n`, `break` and `continue`. There is no C-style `for`, and
  `do { } while`, `do { } until` and `break value` parse and are not part of
  Rux. A handler or binding holding one is refused at load with "is not part
  of Rux"; a `<script>` body or function holding one is not refused, and the
  statement does nothing useful.
- **A `{ }` with no declared type shows its fields in the order written, and a
  record of a declared type in the order the type declares them.** A map keeps
  its keys sorted, gains a key with `m.set(k, v)`, and `m[k] = v` writes only
  a key that is there.
- **Writing a field a declared record type does not have is an error** from the
  checker, and a failure with `kind` `"type"` if it is reached through `any`.
- **`**` of two `int`s is an `int`:** exact, or an `overflow` failure, never a
  rounded `float`.
- **Native values.** A value struct crosses whole, so every field is `pub`; a
  resource is `Send + Sync + 'static`, held in an `Arc`, compared by identity,
  and mutated through Rust's own interior mutability. The boundary value is
  `rux::Any`, which is also the type of Rux's `any` when a Rust function
  takes or gives it.
- **`rux check` never runs the app's Rust.** It reads signatures from the
  source. A declared native function answers the empty value of its declared
  type (`0`, `""`, `none`, `[]`, a record of those, a placeholder resource)
  when a document's top level runs.
- **`native/` is never embedded in a build**, and a build honours
  `CARGO_TARGET_DIR`.
- **Compiled code never replaces the interpreter's meaning.** Both engines are
  held to the same value, state and failure on every script the project has,
  by the differential tests in `crates/rux-aot-tests`.

The full lists these came from, with the alternatives each was chosen over,
are in the [language build log](./archive/language-build-log.md).

