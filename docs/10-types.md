# Types

The type system for Rux script: what an annotation says, what the checker does
with it, and what it costs at run time.

Everything below runs. The checker reads a file's `<script>` and its template, annotations are erased when the program runs, and the exceptions are the boundaries described below. [The Rux language](./11-next.md) is the full language reference; this document is the type system's part of it. [Script](./07-script.md#type-annotations) covers annotations and [`is` and props at run time](./07-script.md#is-and-props-at-run-time) from the script side.

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
   as `0` and is later set to `none` is an error, and the message gives the
   fix, `let x: int? = signal(0)`.

## Where an annotation goes

| Form | Example |
|---|---|
| A `let` or `const` | `let count: int = 0;` |
| A signal | `let filter: Filter = signal("all");` |
| A computed | `computed total: float = items.length * price;` |
| A function parameter | `fn add(title: string) { … }` |
| A function's result | `fn label(t: Task): string { … }` |
| An arrow's parameter | `let add = (a: float, b: float) => a + b;` |
| A prop | `prop label: string;`, `prop price: float = 1;` |
| A named type | `type Filter = "all" \| "open" \| "done";` |

**On a signal, the annotation describes the value**, not a wrapper around it.
Signals read as plain values everywhere, so `filter` above is a `Filter`, and
`filter = "open"` is checked as an assignment of a `Filter`. There is no
`Signal<T>` for an author to write.

A `type` declaration belongs at the top level of `<script>`. It may appear
anywhere there, before or after its first use, and it declares nothing at run
time: `type` is a word only at the start of a statement followed by a name and
`=`, so `let type = 1;` is an ordinary `let`.

**A declared type's name starts with a capital letter.** `Task`, `Filter`,
`RowState`. The built-in types are all lower case, so the two can never
collide, and an import can tell a type from a component by its spelling (see
[Sharing types between files](#sharing-types-between-files)).

## The types

| Type | Holds |
|---|---|
| `int` | A 64-bit whole number |
| `float` | A 64-bit float. An `int` goes anywhere a `float` does, not the other way round |
| `string` | Text |
| `bool` | `true` or `false` |
| `none` | The empty value. `null` and `()` read as it too, but are errors that name `none`; `rux fmt` rewrites them |
| `any` | Anything, unchecked. What a name becomes when nothing says otherwise. A union with `any` in it is `any` |
| `"all"` | Exactly that string. Useful in a union |
| `T[]`, `Array<T>` | An array of `T`; two spellings of one type |
| `{ id: int, title: string }` | A record: a map with exactly these fields |
| `{ note?: string }` | A record whose `note` may be absent |
| `{ [string]: T }`, `Map<string, T>` | A dictionary: any string keys, every value a `T` |
| `T?`, `Option<T>` | `T` or `none`; the same as `T \| none` |
| `A \| B` | Either |
| `(Task, int) => bool` | A function, where one is a value |
| `Task` | A name declared with `type`, or imported with `use` |
| `Page<int>` | A declared type that takes type parameters, given them |
| `Result<T, E>` | `T`, or an error `E` kept as an answer. See below |
| `void` | What a function returns when it returns nothing |

**A whole-number literal is an `int`, and one with a point or an exponent a
`float`** (see [Numbers](./11-next.md#numbers)). `.length`, a range
variable (`for i in 0..n`), `trunc()`, `round()`, `floor()`, `ceil()` and
`intDiv(a, b)` are `int`s; `toFloat()` makes a `float`. `int + int`,
`int - int`, `int * int` and `int % int` are `int`, `int ** 2` too, and with a
`float` on either side any of them is a `float`. `/` is always a `float`,
because `3 / 2` is `1.5`, so `let n = signal(0); n = n / 2;` is an error that
names `intDiv`, and `let n: float = signal(0)` is the other way out. A list is
indexed by an `int`. `number` is no type now; `rux fmt` rewrites it to `float`.
An `int` is a whole number when the script runs too, and a number field bound
to one writes the number cut toward zero.

**A record and a dictionary are both written with braces**, and so are map
values (`{ a: 1 }`). The difference is the square brackets:
`{ [string]: bool }` is a dictionary, and anything else in braces is a record.
A record type is structural: any map with the right fields is one, whatever it
was built as. An object **literal** with a field its target type does not have
is an error, because that field is almost always a misspelling of one it does
have.

**A type or a function may take type parameters** (see [Declaring](./11-next.md#declaring-kept-with-generics)): `type Page<T> = {
items: T[], next: string? };`, used as `Page<Task>`, and
`fn first<T>(items: T[]): T? { items?[0] }`. A call never names them: they are
what the arguments make them, so `first(tasks)` is a `Task?`, and a closure
handed over after the list it works on gets that list's element type
(`pluck(tasks, t => t.id)`). Inside the function a type parameter is a type
nothing is known about: its value can be passed on, stored and compared, and
reading a field of it, adding to it or testing it with `is` is an error. A
generic type named without its arguments, or with the wrong number, is an
error, and so is one given arguments it does not take. A `Map`'s keys are
`string`, and `Set` is not a type; each is an error that says so.

**`Result<T, E>`** is `{ ok: true, value: T } | { ok: false, error: E }`,
declared in every file. `Ok(v)` and `Err(e)` build one, `if r.ok` narrows it
(so `r.value` reads plainly inside, and `r.error` in the `else`), and
`r.unwrap()` gives the value or throws the error. **`void`** is what a function
declared `: void` returns: its last statement is not a result, `return x;` in
it is an error, and its call is nothing to use. A callback typed
`(T) => void` takes a function that returns anything.

Not in the language, and so errors if written: number and boolean literal
types, intersections (`A & B`), tuples, `extends` constraints, type arguments
written at a call, and anything class-shaped.

### Types every file can name

These need no `use` and no declaration:

| Type | Is |
|---|---|
| `File` | A file the person chose: `{ name: string, size: int, type: string, url: string, path: string? }`. What `<input type="file">` binds (`File?`, or `File[]` with `multiple`), and what `pickFile` (`File?`) and `pickFiles` (`File[]`) give. `path` is the desktop's alone. `file.text()` is an async method giving the contents as a `string` |
| `Sound` | What `loadSound(src)` gives. Opaque: its methods and properties are used, its inside is not seen |
| `TapEvent`, `DragEvent`, `SwipeEvent`, `KeyEvent`, `SelectEvent`, `InputEvent` | What `event` is in the handlers of that name; see [Templates](#templates). Annotate a function's parameter with one to call it from a handler |
| `TextMark` | One range of a field's `:marks`: `{ start: int, end: int, class: string }` |

[Script](./07-script.md#what-an-event-hands-you) lists the fields of each event. A file's own `type` of the same name takes the place of these.

**A type from Rust is declared by the app's `native/` crate.** A struct marked `#[rux::export]` is a record type, and a `#[rux::resource]` is opaque: only a value of that same resource fits it. Both are read from the Rust source, so `rux check` and the editor know them without a build; `use type native::shop::Product;` brings one in. A native function's parameters and result are checked against its Rust signature (`Option<T>` is `T?`, `Vec<T>` is `T[]`, every integer an `int`). The `Clone` the signal and channel exports ask of `T` is Rust's, and is covered with them in [Script](./07-script.md#signals-from-rust). `settings.get(key)` gives `any?`.

## Inference

Most names need no annotation, because what they start as says what they are.

| Written | Inferred |
|---|---|
| `signal(0)`, `42` | `int` |
| `signal(0.0)`, `1.5` | `float` |
| `signal("")`, `"all"` | `string`: a literal widens, as in TypeScript |
| `true` | `bool` |
| `[1, 2]` | `int[]` |
| `{ id: 1, name: "ada" }` | `{ id: int, name: string }` |
| `items.length`, `x.trunc()` | `int` |
| `parseInt(s)`, `parseFloat(s)` | `int?`, `float?`: text that is no number gives `none` |
| `tasks.filter(t => t.done)` | `Task[]` when `tasks` is a `Task[]` |
| a call to a function | what that function returns |

A string literal stays a literal type only where something asks for one: an
annotation, a field of an annotated record, or a comparison against a literal
union. `let f = signal("all")` is a `string`; `let f: Filter = signal("all")`
is a `Filter`.

**What cannot be inferred becomes `any`, with a warning** naming the place and
suggesting the annotation. The common cases:

- `signal([])`, which says nothing about what the array will hold. This is
  the one that matters most: without an annotation, a `task` inside a later
  `filter` has no completions, because nothing knows what `tasks` holds.
- `signal(none)` or `signal({})`.
- A parameter with no annotation.
- A value from `JSON.parse`, or from a component's `@name` event.

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
program says "absent is a legitimate answer here". The checker knows which
fields those are and holds every read to it. `t?.note` has the type
`string?`.

**Narrowing relaxes it.** Inside a region where the field is known to be
present, a plain read is allowed, because the region would not be running
otherwise:

```rux
if t?.note != none {
  summary = t.note;       // allowed: this block only runs when it is there
}
```

The same holds after `"note" in t`, after an early return on its absence, and
inside an `r-if` that tested it. Outside such a region, `?.` is still required.

**So is a value that may be `none`.** `sel.title` on a `sel: Task?` raises
when `sel` is `none`, so it is an error unless something has ruled `none` out:
`if sel != none`, an early `return` when it is, an `r-if` that tested it, or
reading it as `sel?.title`, which is a `string?`.

**A dictionary is read the same way.** Any key of a `{ [string]: T }` may be
missing, and a missing key raises exactly as a missing field does, so
`m[k]` is an error unless `k in m` has been established. `m?[k]` reads it as a
`T?`: `?[` guards a missing key, and an index past the end of a
list, as `?.` guards a missing property. `m.k` is held to the same rule,
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
  | { state: "loading", since: float }
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
| `x != none` | `x` to everything but `none` |
| `if x` | `x` without `none` inside the block. The `else` learns nothing about a `string?` or `int?`, since `""` and `0` are falsy too |
| `load.state == "done"` | `load` to the member whose `state` is `"done"` |
| `type_of(x) == "string"` | `x` to `string`. Also `"bool"`, `"array"`, `"map"`, `"()"`, and `"f64"` or `"i64"` for an `int` or a `float` |
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
in common, such as a `string` and an `int`.

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
fn refresh() { load = settings.get("load"); } // typed: no parameters
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

A function type, `(Task) => bool`, is written only where a function is a
value: a parameter that takes a callback, a field that holds one, or a `let`
that holds an arrow. A generic function's type parameters are the one thing
worked out at a call, and only for that call.

### A typed function cannot read its caller's locals

A name read from the caller cannot be typed: the same name would have a
different type, or no type, depending on the caller, and nothing at the
definition says which.

**So a typed function sees only its own parameters and locals, the document's
signals, computeds and functions, and its component's props and state.** A name
from a caller is an error in a typed function. Writing a signal is unaffected,
because signals are not the caller's locals: `fn drain() { level-- }` works as
it always did.

An untyped function can still read a name it does not declare from the handler
or component that ran it (a row's `r-for` variable, the handler's own `let`, a
component's state), which is looked up when it runs. A `let` of a *function*
in between is not reached. Because a function with no parameters is typed,
this rule reaches functions that were never touched by an annotation.
See also [What a function can see](./11-next.md#what-a-function-can-see-changed).

A caller's local is any name the function cannot see itself that something
could have in scope when it calls: an `r-for`'s variable, a handler's own
`let`, or a `let`, parameter or loop variable of another function. The fix the
error suggests is the one that makes the function honest, passing the name in:

```text
app.rux:4: error: `pick` reads `item`, which only whoever calls it has. A function whose parameters are all typed, or that has none, sees its own names and the document's, not its caller's: pass `item` in as a parameter
```

## Templates

A template is checked against the script it binds to.

- **`r-for`** gives its loop variable the element type: in
  `r-for="t in visible()"`, `t` is a `Task`. `r-for="(t, i) in visible()"`
  also binds the index `i`, an `int`. An `r-if` on the same element is read
  per row, with both names in scope.
- **`r-if`, `r-elif` and `r-else`** narrow the subtree under them, as in
  [Unions and narrowing](#unions-and-narrowing).
- **A binding** (`{{ }}`, `:class`, `:style`, any bound attribute) is checked as
  an expression. A bound attribute of a built-in element is checked against
  what that attribute takes: `:disabled` (on `<input>` and `<button>`),
  `:readonly` and `:required` (on `<input>`) a `bool`; `:controls`,
  `:autoplay`, `:loop` and `:muted` on `<audio>` and `<video>` a `bool`;
  `:options` a `string[]`, `:marks` a `TextMark[]`, `:src` (on `<image>`,
  `<audio>` and `<video>`), `:poster`, `:d` and `:to` a `string`,
  `:style` a `string` or a `{ [string]: any }`, and `:r-transition` a
  `float?`, the progress of a swap or nothing.
- **A `:class` map** is a `{ [string]: bool }`; a value that is not a `bool`
  is an error, which catches `{ active: item }` meant as `{ active: item.on }`.
  `:class` also takes a `string` or a `string[]`.
- **`r-model` must name something of the input's value type**: `string` for
  text, password, textarea, search and date; `float` for number and slider,
  which may also write into an `int`; `bool` for checkbox and switch. A select shows its target as text and
  writes one of its options, so a `Filter` may be bound to one. A radio
  writes its `value`, so its target must take that string. A file input
  writes a `File?`, or a `File[]` with `multiple`.
- **`event` in a handler has the type of that event.** Every gesture's
  (`@tap`, `@press`, `@release`, `@longpress`, `@drag`, `@swipe`) has
  `x`, `y`, `pageX`, `pageY`, `width`, `height` and `touches`. `@drag` adds
  `phase`, `"start" | "move" | "end"`, and the four distances, `totalX`,
  `totalY`, `moveX` and `moveY`; `@swipe` adds those distances and
  `direction`, `"left" | "right" | "up" | "down"`. `@key` hands over a
  `KeyEvent`. A field's `@select` hands over a `SelectEvent` (`value`,
  `start`, `end`, `caret`). A text field's `@input` hands over an
  `InputEvent`: `value`, and `kind`, the sort of edit, one of `"type"`,
  `"paste"`, `"cut"`, `"undo"`, `"redo"`, `"insert"`, `"drop"` or
  `"autofill"`. A field's `@change`, `@focus` and `@blur` hand over
  `event.value`: a `float` from a number field bound with `r-model`, `any`
  from a checkbox, switch or slider, a `File` (or `File[]` with `multiple`)
  from a file input, and text from the others. An `<audio>` or `<video>`
  hears `@play`, `@pause`, `@ended` and `@timeupdate` with `currentTime` and
  `duration`, `@loadedmetadata` with `duration`, `videoWidth` and
  `videoHeight`, and `@error` with `kind` and `message`; an `<image>` hears
  `@load` with `width` and `height`, and `@error` with `message`. A
  component's `@name` hands over whatever it emits, which is `any`.
- **A form's `event.values` is a record of its fields**, under each field's
  `name`, or the name of what it binds when it has none. The fields are in the
  markup, so `event.values.email` reads plainly and `event.values.emial` is an
  error. A field under an `r-if` or an `r-for` may be missing, or may be
  several, so it is optional and `any`; a form holding a component, which may
  hold fields of its own, has a `{ [string]: any }` instead. `@invalid`'s
  `event.errors` is a `{ [string]: string }`, because it holds only the fields
  that failed, and is read with `?.`.
- **Props are checked at the tag**, against the component's declaration: a
  missing required prop, an unknown attribute, and a value of the wrong type
  are each an error. A plain attribute passes its text, so
  `kind="big"` is checked as the string `"big"`.

A finding in a template is on the template's line and names where it is:

```text
app.rux:9: error: `:disabled` on <button>: this is `int`, where `bool` is expected
app.rux:10: error: `r-model`: the field writes `float` into `name`, which holds `string`
```

**What is checked is the file being checked.** A document's template is
checked with the document; a component's template is checked when the
component is the file, with its props typed as it declares them.

## Props

```rux
prop label: string;
prop price: float = 1;
prop kind: "primary" | "quiet" = "quiet";
```

A prop's annotation is its contract with every tag that uses the component.
With a default and no annotation, the default's type is inferred, as for a
`let`. With neither, the prop is `any` and warned.

A plain attribute, `label="Save"`, passes a `string`, so a `float` prop must be
bound: `:price="3"`. Passing text to it is an error at the tag.

**Props are also checked when the program runs, in release builds as well as
in development.** A prop is a boundary: its value can come from a route
parameter, from another file checked separately, or from a value the checker
saw as `any`. A prop whose value does not fit its declared type is reported
with the tag and the prop named, and the component is built without it: the
prop takes its default if it has one, and is otherwise left out, as a
required prop the tag forgot is. The check is a comparison against the
declared type, parsed once per component and run when the tag is built, not on
every read:

```text
app.rux:12: error: `<stat>` was given the text "lots" for `:value`, which is not the `float` `prop value` takes, so it is built without it, and it takes its default
```

**A route parameter is text, and becomes what its prop takes.** For
`<route path="/task/:id" view="detail" />` and `prop id: int;`, the path
`/task/42` passes the number `42`. The same holds for `float`, `bool` and a
literal union; a segment that cannot be one (`/task/abc` for an `int`) is a
prop that does not fit, left out and reported.

A `none` passed to a prop arrives as `none`, so `prop task: Task? = none`
reads `task?.title` safely whether the tag passed nothing or passed `none`.

## Boundaries

Types are erased, so a value from outside is whatever it turns out to be. A
value from `JSON.parse`, from `settings.get` (`any?`), or from a component's
`@name` event is `any` until something says otherwise. A native function and
an `async fn` give the type their signature declares.

**`x is T` checks it at run time, and narrows.**

```rux
let raw = settings.get("settings");
if raw is Settings {
  settings = raw;           // a Settings from here on
} else {
  print("settings file is not the expected shape");
}
```

`is` walks the value against the type: every required field present and of the
right type, every array element, every union member tried in turn. A field the
type does not mention is allowed, and an optional field may be `none`. A
number is an `int` when it is whole. It answers `true` or `false` and never
raises. A name that is no type is an error from the checker, and at run time
fits nothing.

`is` binds as `<` does, on both sides, so `a + b is int` tests the sum,
`x is T && y` is `(x is T) && y`, and `a == b is int` is `a == (b is int)`. Where it held, `x` is a `T`; in the `else`, a union loses the
members that are all `T`, so `if v is string { … } else { v.n }` reads the
record's field. The compiler generates a validator only
for types that appear on the right of an `is` and for props, so a program pays
for exactly the checks it asks for.

Assigning an `any` to an annotated name is allowed without a check, since
checking there would put a cost on every assignment. `is` is where the author
says the check is worth paying for.

## Sharing types between files

A type declared in one file is used in another with `use type`, or
`import type`, which means the same:

```rux
use type types::{Task, Filter};
import type { Task, Filter } from "./types";
```

`use type types::Task` names the `type Task` declared in `types.rux`, found
the way a component import is: beside the file first, then at the project
root. The word `type` is what makes it a type import: `use components::task`
imports the component in `components/task.rux`, and
`use type components::task::Row` imports the type `Row` declared in that file.
Without `type`, a capitalised name (`use types::Task;`) still loads, with a
warning, and `rux fmt` rewrites it.

**The declaration is marked `export`**: `export type Task = { … };`. A type
the file does not export is a warning at the import (`rux fmt` adds the
`export`).

A file with a `<script>` and no `<template>` is a module, so a file used only
for its types is an ordinary one: it holds `export type` declarations and
nothing else. `rux check` checks it like any other. `use type` imports
only types; a module's functions and signals are imported with `use`, as
described in [Script](./07-script.md).

`use Task;`, with no file in front of the type, is an error that says where
the type has to come from.

## What is an error and what is a warning

**Errors**, which make `rux check` exit 1: a value that does not fit where it
goes, including a value that does not fit what a name started as, a missing or misspelled field, a plain read of an optional field outside
a narrowed region, an impossible comparison, a `switch` that misses a case, a
typed function reading a caller's local, an annotation naming no type, `null`
or `()` written for `none`, `number` written for a type, and a wrong prop at
a tag.

**Warnings**, which do not: a name that became `any` because nothing could be
inferred, including every unannotated parameter and a prop with neither a
type nor a default; a type imported without `type` or not marked `export`.
`rux check --deny-warnings` exits 1 on warnings as well.

That line is deliberate. An untyped program is not wrong, and a warning is how
it is told where checking stops; a typed program that contradicts itself is
wrong, and says so.

## Editor

The extension shows the checker's errors and warnings from this page as
diagnostics on their lines, when a file is opened and saved. The types exist
only in the checker, so the editor does not work them out. It asks for them: `rux check --format json --types` prints an object instead of
the usual array, with the diagnostics beside two lists.

- `types`: every name read, field read, `let`, parameter and function call the
  checker gave a type, with its file, line, column, the type as written
  (`Task?`, or a function's signature), whether it may be `none`, and the
  fields a `.` after it may read.
- `guesses`: for each unannotated parameter, the type its calls hand it, when
  every call hands it something known.

The extension already runs `rux check` when a file is opened and saved, so it
asks for the table on the same run and keeps it per file. There is no language
server and no second process.

- **Hover** shows `name: Type` above what hover already said, and a function's
  signature on a call.
- **Completion** after `x.` offers the fields of `x`'s record. Choosing an
  optional field, or any field of a value that may be `none`, turns the `.` into
  `?.`, so the line written is one the checker accepts.
- **The quick-fix** on "`s` has no type" writes `: T` after the parameter, `T`
  being its guess. Checking never uses a guess (see [Functions](#functions));
  this only writes an annotation the author then reads.

Every answer is as old as the last save. Entries are matched by name and line,
not by exact position, so an edit moves them only as far as the lines moved,
and a name that is no longer on its line gets no type rather than a wrong one.
Only the file being checked has a table: a page checked through its app, or a
component a document uses, is answered when it is the file opened and saved.

An older `rux` refuses `--types` as an unknown option. The extension then asks
again without it, keeps the diagnostics, and stops asking for the session.

## How it is built

The checker (`crates/rux-script`) reads Rux's own AST (`crates/rux-syntax`),
which keeps every annotation on the node it belongs to, and the script is
lowered from it to a typed IR (`crates/rux-ir`). It runs where the other
load-time checks run: `rux check`, the dev overlay, and the editor. The only
parts that reach a running program are the prop checks and the `is`
validators.
