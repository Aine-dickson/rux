# Rux next: a typed language over Rust

**Status: decided 2026-09-26, nothing built.** This document records the
direction taken after the performance work of 2026-09-25 showed where Rux's
time goes, and after the design was tested in a long brainstorm. It is not a
reference: [As Built](./05-as-built.md) and [Script](./07-script.md) stay
authoritative for what runs today, and [Types](./10-types.md) for the type
system as built. Where this document disagrees with those, this is the plan
and they are the present.

Every decision below is marked **(decided)** when the project owner took it,
or **(proposed)** when it was delegated and may still be overruled.

## What changes, in one paragraph

Rux script becomes a checked, statically typed language that Rux owns: its own
parser, its own type system, its own intermediate representation. A release
build lowers that representation to Rust and compiles it with `rustc`, for
native targets and for wasm alike. Development, hot reload, the playground and
anything untrusted run the same representation in an interpreter Rux owns.
Rhai, and the fork of it, go away. Rust code reaches Rux through generated,
typed interfaces rather than a dynamic bridge. The release rush is paused, and
shipping apps without a Rust toolchain is no longer a goal.

## Principles

1. **Rux owns the language; Rust is the substrate.** Rux types are the
   contract a Rux author sees. Rust types are how the compiler represents them.
   A Rux author never needs ownership, lifetimes, traits or `Arc`.
2. **Borrow famous syntax, exactly, and only where it earns its place.**
   `async`/`await`, `try`/`catch`, `<T>`, `import`, `reduce(fn, init)` are
   taken as the world already knows them. Rux is not "JavaScript compiled to
   Rust": it keeps `signal`, `let`, `mounted { }` and its template language.
3. **One way to say a thing.** Where two spellings would mean the same, one is
   chosen. The deliberate exception is `use` and `import` (see Modules).
4. **The boundary to Rust is typed and costs nothing at run time.** It is a
   compile-time boundary, not a runtime bridge.

## The language

### Types (decided)

- **Primitives, lowercase:** `int` (64-bit integer), `float` (64-bit float),
  `string`, `bool`, `void`, `any`. `number` is retired.
- **Checked, not erased.** Every expression has a type the compiler knows.
  `any` is the one dynamic type: explicit, checked when the program runs, and
  the only place dynamic behaviour lives.
- **A name whose type cannot be worked out** is a warning in the interpreter and
  an **error in a release build** (decided). Writing `: any` opts out.
- **Return types stay `: T`:** `fn total(items: Item[]): float` (decided).
- **`type` is the only way to declare a type** (decided). No `interface`, no
  classes. Records, unions and string literal unions are as
  [Types](./10-types.md) built them.

### Numbers (proposed)

- An integer literal is an `int`; a literal with a point is a `float`.
- An `int` widens to `float` wherever a `float` is expected or the two are
  mixed (`1 + 0.5` is `float`).
- **`/` always produces a `float`**, so `7 / 2` is `3.5` as in JavaScript and
  Python 3. `intDiv(7, 2)` is `3`. Truncating division by accident is the bug
  this avoids.

### The empty value (decided)

- **`none`**, not `null`.
- **`T?` is the short spelling of `Option<T>`**, as in Swift and Kotlin, so the
  forced `?.` and narrowing already built keep working unchanged.

### Errors (decided)

- `try { … } catch e { … }` and `throw`.
- `Result<T, E>` is a Rux type for values an author keeps on purpose.
- A native function that returns a Rust `Result<T, E>` appears in Rux as
  returning `T`, and its `Err` is thrown. That is how the boundary lets
  `try`/`catch` and Rust's `Result` meet without either leaking into the other.
- A failed `await` throws the same way.

### Generics (decided in shape)

- Plain `<T>` on functions and on `type`s: `fn first<T>(items: T[]): T`,
  `type Page<T> = { items: T[], next: string? }`.
- Built-in generic types: `Array<T>` (the same type as `T[]`), `Map<K, V>`,
  `Set<T>`, `Option<T>`, `Result<T, E>`.
- **Not in the first version:** explicit type arguments at a call
  (`first<User>(x)`, which is ambiguous with `<` and `>`), `extends`
  constraints, conditional and mapped types. Type arguments are always
  inferred.
- Compiled as Rust compiles generics: one copy per type used.

### Collections and the standard library (decided in principle)

- Rux owns the API of its built-in types. Rust's methods are never inherited:
  a Rust release cannot change what `Array` offers.
- **One name per operation, JavaScript's camelCase** (proposed): `.length`,
  `push`, `pop`, `map`, `filter`, `find`, `forEach`, `reduce`, `startsWith`,
  `toUpperCase`. Writing a Rust name (`.len()`) is an error with a quick fix to
  the Rux one. Names that leak from Rhai today (`to_int()`) get Rux names as
  part of the move.
- **`reduce` takes JavaScript's order** (proposed):
  `items.reduce((sum, x) => sum + x.price, 0.0)`.
- No iterable protocol is visible to authors (decided). `for x in c` works on
  whatever the compiler knows how to iterate.

### Async (decided)

```rux
async fn loadUser(id: int): User {
  let user = await api.getUser(id);
  return user;
}
```

- **`async fn` and `await`**, and nothing else. No `Future`, `Promise` or
  task type is ever visible to an author.
- **The return type is the value awaited** (proposed): `: User`, not a
  wrapper. `async` already says the function waits.
- **A handler only calls an async function** (decided): `@tap="loadUser(3)"`
  starts it and returns at once. Its writes render as they happen, before and
  after each `await`.
- `await` is allowed only inside an `async fn`.

### Modules (decided)

- **`use` and `import` are both accepted and mean the same**, as are the type
  forms: `use types::Task` and `import type { Task } from "./types"`.
- **A script-only file is a module**: functions, types and state, no
  template.
- **A module's top-level `let` signals are shared state** (proposed reading of
  the owner's "Pinia-like" intent). Every importer sees the same instance, and
  the module's exported functions are the only way to change it. That is a
  store, with no new keyword:

  ```rux
  // stores/cart.rux
  let items: CartItem[] = signal([]);
  export fn add(p: Product) { items.push({ product: p, qty: 1 }); }
  export fn total(): float { return items.reduce((s, i) => s + i.product.price * i.qty, 0.0); }
  ```

  A component's own state is still changed only inside the component. A
  factory that gives each caller its own copy can come later.

### Kept as they are (decided)

`signal(…)`, `let` (no `const`), `mounted { }` and the other lifecycle blocks,
the template language, `r-*` directives, CSS.

## Native code: Rust behind a typed interface

### Reaching it (proposed keyword, decided shape)

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
   opaque. Rux sees its exported methods only.
3. **`any`.** The explicit escape into dynamic values.

Rules (proposed):

- Rust `snake_case` names become Rux camelCase (`discountedPrice`);
  `#[rux(name = "…")]` overrides.
- `#[rux::hide(…)]` and `#[rux::only(…)]` trim what Rux sees.
- A type Rux cannot represent in an exported signature is a **compile error at
  the export** saying why, never dropped silently.
- There is **no escape hatch for calling unexported Rust on a Rux value**
  (the brainstorm's `native items.reserve()`). A missing capability is written
  as a small exported Rust function.

### The interface file (proposed)

Exports are read from the Rust **source** (with `syn`), not from a build, into
a generated interface file per crate. The checker, `rux check` and the editor
read that file, so completion, hover and errors on native calls work within
seconds of saving the Rust, without waiting for `cargo`. UniFFI and
wasm-bindgen work the same way.

## Architecture (proposed)

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
- **Rhai is removed completely.** Development still needs an interpreter,
  because hot reload cannot wait for `rustc` and a browser cannot run it. That
  interpreter is Rux's own and runs the same IR, so development and release
  share one set of semantics. The fork and its eleven divergences retire with
  it.
- **The web** already runs the runtime as wasm. With generated Rust, scripts
  compile into that wasm too. The playground keeps the interpreter.
- A separate machine-code backend (Cranelift) is not needed.

## Security (proposed)

- **Compiled app code has full trust**, as any compiled application does.
- **The interpreter is the sandbox** for anything untrusted: the playground,
  fetched or over-the-air documents, plugins. It carries step and memory
  limits and has no `native` access unless granted.
- **A native module declares the platform permissions it needs**, and the
  Android manifest is generated from those declarations, never written beside
  them.
- The rest of the posture in the 2026-09-24 proposal (supply chain, dev
  channel, saved state) stands.

## What existing code meets

| Today | Next |
|---|---|
| `number` | `int` or `float` |
| `null` | `none` |
| `T?`, `?.`, narrowing | unchanged (`T?` is `Option<T>`) |
| `host::f()` | `native::…` modules |
| unannotated, uninferable names | error in release builds |
| Rhai names such as `to_int()` | Rux names |
| `Array<T>` refused | accepted, same as `T[]` |

`rux fmt` rewrites the mechanical ones (`null` to `none`, `host::` where a
module maps directly) so existing apps move with one command.

## Build order (proposed)

1. **Spec first.** Merge this document's decisions into a language reference,
   and settle the open questions below.
2. **Rux's own parser**, producing a Rux AST, replacing the fork's parser. The
   type annotations built on the fork move over with it.
3. **The checker on the new AST**, extended to the decided types: `int`/`float`,
   `none`, `Option`/`Result`, generics, modules.
4. **The typed IR.**
5. **Rux's interpreter on the IR**, replacing Rhai for everything, with the
   test suite as the proof that behaviour did not move. Rhai is deleted here.
6. **`async fn` / `await`** in the interpreter.
7. **Script modules and stores.**
8. **`#[rux::export]`, the interface file, `native` modules.**
9. **Rust code generation** for release builds, native then wasm.

Steps 2 to 5 change nothing an author sees except the decided syntax, which is
what makes them safe to take in order.

## Settled 2026-09-26 (the former open questions)

1. **`catch e` binds a Rux `Error`** (decided): `type Error = { message:
   string, kind: string }`. A native `Err` arrives as an `Error` whose `kind`
   names the Rust error; `throw` in Rux throws an `Error` too.
2. **`{ [string]: T }` stays** (decided) as a second spelling of
   `Map<string, T>`: the same type, not a different one. A named exception to
   the one-spelling rule, like `T[]` for `Array<T>`.
3. **`rux fmt` can normalise imports** (decided). Which form it writes is a
   project setting in `rux.toml` (proposed: `imports = "use"` or `"import"`),
   and without the setting it leaves both alone.
4. **The interpreter gets its own benchmarks from its first commit** (decided),
   beside the `RUX_PROFILE` ones in `rux-harness/script-cost/`, so development
   speed is measured, not assumed.
