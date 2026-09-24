# Types

The type system for Rux script: what an annotation says, what the checker does
with it, and what it costs at run time.

**Status: designed, being built.** Every rule below was decided on 2026-09-24
and none of it is open. The table says which parts run today; a part that does
not run yet is a design, and [Script](./07-script.md) stays the reference for
what the language does now. As each part lands, its rules move into Script and
the row below says so.

| Part | State |
|---|---|
| Annotations parsed and erased (`: T`, `type`, `prop x: T`, `use types::X`) | **Built** 2026-09-24. Accepted and ignored at run time; a malformed one is a syntax error. See [Script](./07-script.md#type-annotations) |
| The checker: primitives, arrays, records, dictionaries, `T?`, inference | **Built** 2026-09-24 for a file's `<script>`: its `let`s, functions (parameters, results, calls) and closures. Handlers, bindings and templates are not checked yet |
| Unions, narrowing, `switch` exhaustiveness, forced `?.` | **Built** 2026-09-24 in a file's `<script>`, with `?[` for a dictionary. `r-if` narrowing waits for templates, and `await` for async |
| Functions: typed and untyped, the caller-local rule | Pending |
| Templates: `r-for`, `r-if`, bindings, `r-model`, events, props at the tag | Pending |
| Boundaries: prop checks at run time, `x is T` | Pending |
| Editor: hover, completion, the parameter quick-fix | Pending |

## The shape of it

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

  fn add(title: string) {
    tasks = tasks + [{ id: tasks.length, title: title, done: false }];
  }
</script>
```

Three things decide almost everything else:

1. **Annotations are TypeScript's**: `name: T` after a name, `): T` after a
   parameter list.
2. **Types are erased.** An annotation costs nothing when the program runs. The
   exceptions are the **boundaries**, where a value arrives from somewhere the
   checker cannot see: a prop passed on a tag, and any value tested with
   `x is T`. Those are checked at run time, in release builds too.
3. **A program with no annotations still runs exactly as before.** What the
   checker cannot infer becomes `any` and is reported as a warning, never an
   error. A name that starts with a value takes that value's type, though, so
   an unannotated program can still contradict itself: a signal that starts
   as `0` and is later set to `null` is an error, and the message gives the
   fix, `let x: number? = signal(0)`.

## Where an annotation goes

| Form | Example |
|---|---|
| A `let` or `const` | `let count: int = 0;` |
| A signal | `let filter: Filter = signal("all");` |
| A computed | `computed total: number = items.length * price;` |
| A function parameter | `fn add(title: string) { … }` |
| A function's result | `fn label(t: Task): string { … }` |
| An arrow's parameter | `let add = (a: number, b: number) => a + b;` |
| A prop | `prop label: string;`, `prop price: number = 1;` |
| A named type | `type Filter = "all" \| "open" \| "done";` |

**On a signal, the annotation describes the value**, not a wrapper around it.
Signals read as plain values everywhere, so `filter` above is a `Filter`, and
`filter = "open"` is checked as an assignment of a `Filter`. There is no
`Signal<T>` for an author to write.

A `type` declaration belongs at the top level of `<script>`. It may appear
anywhere there, before or after its first use, and it declares nothing at run
time: `type` is a word only at the start of a statement followed by a name and
`=`, so `let type = 1;` still means what it did.

**A declared type's name starts with a capital letter.** `Task`, `Filter`,
`RowState`. The built-in types are all lower case, so the two can never
collide, and an import can tell a type from a component by its spelling (see
[Sharing types between files](#sharing-types-between-files)).

## The types

| Type | Holds |
|---|---|
| `number` | Any number. Numbers are `f64` at run time, as [Values](./07-script.md#values) describes |
| `int` | A whole number. A subtype of `number`: an `int` goes anywhere a `number` does, not the other way round |
| `string` | Text |
| `bool` | `true` or `false` |
| `null` | The empty value, `null` or `()` |
| `any` | Anything, unchecked. What a name becomes when nothing says otherwise |
| `"all"` | Exactly that string. Useful in a union |
| `T[]` | An array of `T` |
| `{ id: int, title: string }` | A record: a map with exactly these fields |
| `{ note?: string }` | A record whose `note` may be absent |
| `{ [string]: T }` | A dictionary: any string keys, every value a `T` |
| `T?` | `T` or `null`; the same as `T \| null` |
| `A \| B` | Either |
| `(Task, int) => bool` | A function, where one is a value |
| `Task` | A name declared with `type`, or imported with `use` |

**`int` exists for indexes, ranges and lengths.** `.length` is an `int`, a
range variable (`for i in 0..n`) is an `int`, and `to_int()` returns one. An
integer literal is a `number`, not an `int`, because otherwise
`let n = signal(0); n = n / 2;` would be an error for no reason anyone could
see. `int` comes only from an annotation, `.length`, `to_int()` or a range.
Arithmetic keeps it where it can: `int + int`, `int - int`, `int * int` and
`int % int` are `int`, and `/` is always `number`, because `3 / 2` is `1.5`.
A whole-number literal goes wherever an `int` is expected, and beside an `int`
it counts as one, so `count + 1`, `count += 1` and `count++` keep an `int` an
`int`.

**A record and a dictionary are both written with braces**, and so are map
values since `{ a: 1 }` became a map. The difference is the square brackets:
`{ [string]: bool }` is a dictionary, and anything else in braces is a record.
A record type is structural: any map with the right fields is one, whatever it
was built as. An object **literal** with a field its target type does not have
is an error, because that field is almost always a misspelling of one it does
have.

Not in the first release, and so errors if written: generics of any kind
(`Array<T>` included, where `T[]` is the spelling), number and boolean literal
types, intersections (`A & B`), tuples, and anything class-shaped.

## Inference

Most names need no annotation, because what they start as says what they are.

| Written | Inferred |
|---|---|
| `signal(0)`, `1.5` | `number` |
| `signal("")`, `"all"` | `string`: a literal widens, as in TypeScript |
| `true` | `bool` |
| `[1, 2]` | `number[]` |
| `{ id: 1, name: "ada" }` | `{ id: number, name: string }` |
| `items.length`, `x.to_int()` | `int` |
| `tasks.filter(t => t.done)` | `Task[]` when `tasks` is a `Task[]` |
| a call to a function | what that function returns |

A string literal stays a literal type only where something asks for one: an
annotation, a field of an annotated record, or a comparison against a literal
union. `let f = signal("all")` is a `string`; `let f: Filter = signal("all")`
is a `Filter`.

**What cannot be inferred becomes `any`, with a warning** naming the place and
suggesting the annotation. The common cases:

- `signal([])`, which says nothing about what the array will hold. This is
  the one the whole design was started for: a `task` inside a later `filter`
  had no completions, because nothing knew what `tasks` held.
- `signal(null)` or `signal({})`.
- A parameter with no annotation.
- A value from `host::` whose function was registered without a signature.

`any` is never an error on its own. It turns checking off for that value and
everything computed from it, which is exactly what an unannotated program had
before, and the warning is how the gap gets found.

**A closure's parameters come from where it is used.** In
`tasks.filter(t => t.done)`, `t` is a `Task` because `filter` on a `Task[]`
calls its argument with one. No annotation is needed, and none is expected. The
built-in methods (`map`, `filter`, `find`, `reduce`, `forEach`, `every`,
`findIndex`, `sort`) all hand their callbacks typed arguments this way. An
arrow that is not an argument, `let add = (a, b) => a + b`, has no context and
needs its parameters annotated.

## Optional fields and `?.`

A field written `note?: string` may be absent, and **it must always be read
with `?.`**. A plain `t.note` is an error:

```rux
{{ t.note }}              <!-- error: `note` may be absent; read it as t?.note -->
{{ t?.note ?? "none" }}   <!-- a string -->
```

This is the reason Rux has `?.` at all. Strict property access is on for every
document, so reading a field that is not there raises, and `?.` is how a
program says "absent is a legitimate answer here". The checker now knows which
fields those are and holds every read to it. `t?.note` has the type
`string?`.

**Narrowing relaxes it.** Inside a region where the field is known to be
present, a plain read is allowed, because the region would not be running
otherwise:

```rux
if t?.note != null {
  summary = t.note;       // allowed: this block only runs when it is there
}
```

The same holds after `"note" in t`, after an early return on its absence, and
inside an `r-if` that tested it. Outside such a region, `?.` is still required.

**A dictionary is read the same way.** Any key of a `{ [string]: T }` may be
missing, and a missing key raises exactly as a missing field does, so
`m[k]` is an error unless `k in m` has been established. `m?[k]` reads it as a
`T?`: the fork's `?[` guards a missing key, and an index past the end of a
list, as its `?.` guards a missing property. `m.k` is held to the same rule,
and read as `m?.k`. The loop variable of `for k in keys(m)` is known to be in
`m`.

A record indexed by a literal union needs none of this when every member is a
field: with `labels: { all: string, open: string, done: string }` and
`f: Filter`, `labels[f]` is a `string`.

## Unions and narrowing

A union is written with `|`, and may begin with one so a long union lines up:

```rux
type Load =
  | { state: "idle" }
  | { state: "loading", since: number }
  | { state: "failed", reason: string }
  | { state: "done", rows: Task[] };
```

A field that every member has with a different literal type, `state` above, is
a **discriminant**. Testing it narrows the whole value.

A union is only as useful as what can be done with it, so these narrow the type
of a name for the rest of the region they guard:

| Form | Narrows |
|---|---|
| `x == "open"` | `x` to `"open"`, and `x != "open"` removes it |
| `x != null` | `x` to everything but `null` |
| `if x` | `x` without `null` inside the block. The `else` learns nothing about a `string?` or `number?`, since `""` and `0` are falsy too |
| `load.state == "done"` | `load` to the member whose `state` is `"done"` |
| `type_of(x) == "string"` | `x` to `string`. Also `"bool"`, `"array"`, `"map"`, `"()"`, and `"f64"` or `"i64"` for `number` |
| `"note" in t` | `t` to the members where `note` is present, and `t.note` to present |
| `x is Task` | `x` to `Task`, checked at run time (see [Boundaries](#boundaries)) |
| an early `return` | the rest of the function, to whatever the guard excluded |
| `switch` | each arm, to the case it matched |

In a template, `r-if`, `r-elif` and `r-else` narrow the subtree under them the
same way, so inside `<view r-if="load.state == &quot;done&quot;">` a binding
may read `load.rows`.

**Comparing against a literal a union does not contain is an error.** With
`filter: Filter`, `filter == "al"` can never be true, and it is almost always a
typo for `"all"`. The same goes for any comparison between types with nothing
in common, such as a `string` and a `number`.

**A fact about a signal does not outlive a call that writes it.** After
`if load.state == "done" { refresh(); … }`, `load` is back to the whole union
if `refresh` writes `load`, because it may no longer be `"done"`. The checker
knows which functions write which signals, since a function's writes are in
its body; a function that calls one that writes counts as writing too. An
`await` ends every narrowing of a signal the same way, since anything may have
run while it waited. A local that nothing else can write keeps its narrowing
across both.

### `switch` is checked for every case

A `switch` on a union-typed value with no `_` arm must name every member, and
an error lists the ones it leaves out:

```rux
switch filter {
  "all"  => tasks,
  "open" => tasks.filter(t => !t.done),
}
// error: this switch does not handle "done"
```

That is what makes adding `"archived"` to `Filter` safe: every `switch` that
must now handle it says so. A `_` arm opts out, deliberately. A `switch` on a
discriminant narrows the value in each arm, so `switch load.state { "done" =>
load.rows.length, … }` is allowed.

## Functions

**A function is typed when every parameter is annotated.** A function with no
parameters is typed, since it has none left unannotated. A typed function's
body is checked against its parameters, and its calls are checked against its
signature.

```rux
fn label(t: Task): string { t.title }       // typed
fn refresh() { load = host::load(); }        // typed: no parameters
fn old(x) { x + 1 }                          // untyped: `x` is any, and warned
```

**The result type is inferred** from the body, and may be written with
`): T` when the author wants it stated. It must be written for a function that
calls itself, directly or through another, because inferring it would need the
answer to find the answer.

**Parameters are never inferred from calls.** A parameter's type comes from its
annotation or it is `any`. Inferring it from call sites would make a function's
meaning change with who calls it, and one new call somewhere else could break
a function that nobody touched. The editor offers the inference instead: on an
unannotated parameter, a quick-fix writes in the type the calls agree on, and
the author reads it before keeping it.

There are no user generics. A function type, `(Task) => bool`, is written only
where a function is a value: a parameter that takes a callback, a field that
holds one, or a `let` that holds an arrow.

### A typed function cannot read its caller's locals

Under the fork, a plain call runs in its **caller's** scope (divergence 4 in
`crates/rux-rhai/DIVERGENCE.md`), so a function body can read a `let` of the
function that called it. That cannot be typed: the same name would have a
different type, or no type, depending on the caller, and nothing at the
definition says which.

**So a typed function sees only its own parameters and locals, the document's
signals, computeds and functions, and its component's props and state.** A name
from a caller is an error in a typed function. Writing a signal is unaffected,
because signals are not the caller's locals: `fn drain() { level-- }` works as
it always did.

An untyped function keeps today's behaviour exactly, caller locals included,
and is where code that relies on it can stay. Because a function with no
parameters is typed, this rule reaches functions that were never touched by an
annotation. Before it is switched on, every existing function that reads a
caller's local is found and listed, so none breaks without being named.

## Templates

A template is checked against the script it binds to.

- **`r-for`** gives its loop variable the element type: in
  `r-for="t in visible()"`, `t` is a `Task`, and an index variable is an `int`.
- **`r-if`, `r-elif` and `r-else`** narrow the subtree under them, as in
  [Unions and narrowing](#unions-and-narrowing).
- **A binding** (`{{ }}`, `:class`, `:style`, any bound attribute) is checked as
  an expression. A bound attribute of a built-in element is checked against
  what that attribute takes: `:disabled` a `bool`, `:options` a `string[]`,
  `:src` a `string`.
- **A `:class` map** is a `{ [string]: bool }`; a value that is not a `bool`
  is an error, which catches `{ active: item }` meant as `{ active: item.on }`.
- **`r-model` must name something of the input's value type**: `string` for
  text, password, textarea, select and date; `number` for number and slider;
  `bool` for checkbox and switch. A radio's `r-model` has the type of its
  `value`s.
- **`event` in a handler has the type of that event**: `event.value` in a
  number field's `@input` is a `number`, `event.direction` in `@swipe` is
  `"left" | "right" | "up" | "down"`, and `event.values` in `@submit` is a
  `{ [string]: any }`.
- **Props are checked at the tag**, against the component's declaration: a
  missing required prop, an unknown attribute (both already errors) and now a
  value of the wrong type.

## Props

```rux
prop label: string;
prop price: number = 1;
prop kind: "primary" | "quiet" = "quiet";
```

A prop's annotation is its contract with every tag that uses the component.
With a default and no annotation, the default's type is inferred, as for a
`let`. With neither, the prop is `any` and warned.

A plain attribute, `label="Save"`, passes a `string`, so a `number` prop must be
bound: `:price="3"`. Passing text to it is an error at the tag.

**Props are also checked when the program runs, in release builds as well as
in development.** A prop is a boundary: its value can come from a route
parameter, from another file checked separately, or from a value the checker
saw as `any`. A prop whose value does not fit its declared type is reported
with the tag and the prop named, and the component is not built with it. The
check is a comparison against the declared type, generated once per component
and run when a tag's props change, not on every read.

## Boundaries

Types are erased, so a value from outside is whatever it turns out to be. A
value from `host::` without a signature, parsed JSON, or the result of an
`await` is `any` until something says otherwise.

**`x is T` checks it at run time, and narrows.**

```rux
let raw = host::read_settings();
if raw is Settings {
  settings = raw;           // a Settings from here on
} else {
  print("settings file is not the expected shape");
}
```

`is` walks the value against the type: every required field present and of the
right type, every array element, every union member tried in turn. It answers
`true` or `false` and never raises. The compiler generates a validator only
for types that appear on the right of an `is` and for props, so a program pays
for exactly the checks it asks for.

Assigning an `any` to an annotated name is allowed without a check, since
checking there would put a cost on every assignment. `is` is where the author
says the check is worth paying for.

## Sharing types between files

A type declared in one file is used in another with `use`:

```rux
use types::Task;
use types::Filter;
```

`use types::Task` names the `type Task` declared in `types.rux`, found the way
a component import is: beside the file first, then at the project root. The
last segment's capital letter is what makes it a type: `use components::task`
still imports the component in `components/task.rux`, and
`use components::task::Row` imports the type `Row` declared in that file.

A file used only for its types holds a `<script>` and nothing else: no
`<template>` and no `<style>`. Its script may declare types and nothing
besides, since nothing would ever run a function or a statement in it, and
`rux` refuses to run the file, since it has nothing to show. `rux check`
checks it like any other. Only `type` declarations can be imported this way; a
function or a signal cannot.

`use Task;`, with no file in front of the type, is an error that says where
the type has to come from.

## What is an error and what is a warning

**Errors**, which make `rux check` exit 1: a value that does not fit where it
goes, including a value that does not fit what a name started as, a missing or misspelled field, a plain read of an optional field outside
a narrowed region, an impossible comparison, a `switch` that misses a case, a
typed function reading a caller's local, an annotation naming no type, and a
wrong prop at a tag.

**Warnings**, which do not: a name that became `any` because nothing could be
inferred, including every unannotated parameter.

That line is deliberate. An untyped program is not wrong, and a warning is how
it is told where checking stops; a typed program that contradicts itself is
wrong, and says so.

## How it is built

The fork parses annotations and discards them before evaluation, so an
annotated program runs through the same evaluator as an unannotated one. Each
annotation is kept in a side table beside the AST, keyed by the position of the
name it annotates, which leaves the AST itself in upstream's shape. That change
is recorded as item 8 in `crates/rux-rhai/DIVERGENCE.md`.

The checker lives outside the engine, in `rux-script`, and reads the AST and
the side table. It runs where the other load-time checks run: `rux check`, the
dev overlay, and the editor. The only parts that reach a running program are
the prop checks and the `is` validators.
