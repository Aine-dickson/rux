+++
title = "rux native"
description = "Print what Rux sees of the app's Rust: every module and export in native/, as Rux declarations."
weight = 6
+++

`rux native` prints what Rux sees of the app's Rust in `native/`, written as Rux
declarations: the functions a `.rux` file can call, with the types it calls them
with.

```bash
rux native                     # the native/ crate here or in a parent directory
rux native path/to/project     # or the one under a named directory
```

For the project `rux new` writes:

```text
// native::hello
export fn greet(name: string): string;
```

That is `use native::hello;` and `hello.greet(name)` from script, and it is the
same reading `rux check` and the editor use, so what this prints is what they
check calls against.

## Where it reads from

It reads the source, and builds nothing. `native/src/lib.rs` names the modules
(one `pub mod` per `native::<module>`), and each function marked
`#[rux::export]` in them is printed with its Rust types translated: `String` is
`string`, every integer type is `int`, `f32` and `f64` are `float`, `Option<T>`
is `T?` and `Vec<T>` is `T[]` (more under Native code in the
[script reference](@/reference/script.md#native-code)). Nothing is printed for a
function without the mark, because script cannot call it.

## What it reports

An export whose signature cannot cross into script is left out and reported as an
error with its file and line, in the `path:line: error: message` shape `rux check`
uses, and the command exits with `1`:

```text
native\src\hello.rs:30: error: a `Rc` cannot cross: a native call may run on another thread
```

With no `native/Cargo.toml` here or above, it says so and exits with `2`.

The Rust bodies are not checked: that is cargo's job, and
[`rux check --native`](@/tooling/check.md#the-app-s-rust) runs it.
