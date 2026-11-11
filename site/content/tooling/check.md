+++
title = "rux check"
description = "Report what is wrong with a file or a tree, without opening a window. The same loader the runtime uses."
weight = 3
+++

`rux check` reports what is wrong without opening a window.

```bash
rux check                      # every .rux under the current directory
rux check examples             # or a named file or directory
rux check --deny-warnings .    # warnings fail too, which is what CI wants
rux check --format json .      # for an editor to turn into squiggles
rux check --native             # also cargo check the app's native/ crate
```

Output is `path:line:col: severity: message`, the shape every compiler emits
and every editor and CI log already knows how to parse.

## It cannot disagree with the runtime

Documents load through the same code the window uses. A checker with its own
parser would eventually accept a file the runtime rejects, or the reverse, and
the failure would land on whoever trusted it.

**Errors** are failures to load, and carry a line and column. **Warnings** are
the things the dev overlay lists: unhonored CSS, unknown pseudo-classes,
undefined `var()`, expressions that did not evaluate.

## Handlers are checked at load

Every event handler is compiled and type-checked when the document loads, so a
broken one is reported before anyone taps it, not as a button that looks right
and does nothing. Handlers inside branches that are not currently rendered are
checked too, since a false `r-if` is where a broken handler hides longest.

A handler that calls a function that does not exist, or writes a value of the
wrong type, is an error:

```text
app.rux:3: error: the `@tap` handler calls `nope`, which does not exist, so it will do nothing at that point when it runs
app.rux:3: error: `@tap` on <button>: this is `string`, where `int` is expected
```

A handler that will not compile at all (a stray character, an operator the
language does not have) is a warning that says it will never run.

## The app's Rust

`rux check` reads the signatures of the functions `native/` exports, from the
source and without building it, so a call from a `.rux` file with the wrong
arguments is reported like any other. The Rust bodies themselves are cargo's to
check:

```bash
rux check --native
```

runs `cargo check` on `native/` as well and reports its errors in the same
`path:line` form. It compiles Rux's dependencies the first time, which can take
several minutes, so it is off unless asked for.

## Windows a script opens

A `window.open("windows/inspector.rux", { ... })` whose path is written out is
checked too: the file must exist, its root must be a `<screen>`, the `props` it
is given must be ones the file declares, with the right types, and every option
must be one `window.open` takes. A path built at run time is a warning, because
it cannot be followed.

## Every file is checked, each in its own context

A bare `rux check` reads every `.rux` file in the project, from any folder in
it. The folder is the one with `rux.toml`; `target/`, `node_modules/`, `locks/`,
`dist/` and `native/` are left out.

A **component** is checked on its own. What its `prop`s declare is what its
template may use: a declared prop is owed by whoever uses the component, keeps
the type it was declared with, and says nothing, while any other name it reads is
an error. A **store** is checked as the script it is, whether or not anything
imports it.

```bash
rux check components/task.rux   # or just one of them
```

## Pages are checked through the app

A router builds the matching route and nothing else, so loading a document
reaches exactly one page: the one at `/`. The checker visits the rest, every
`<route>` in turn, with `rux-check` standing in for each `:parameter` and one
visit to nowhere when there is a `fallback`.

That is the only way a page can be checked at all. A page reads the app's
signals and calls its functions, and those are shared into the engine by the
document that declares them, so a page read on its own reports every one of
them as missing. Asking about a page therefore checks the app:

```bash
rux check pages/home.rux
rux: pages/home.rux checked through app.rux, where the app's signals and functions are in scope
```

You get what is wrong with the page and with the app, not with the app's other
pages. A parameter is filled with `rux-check` rather than a real value, so a
page that assumes its parameter names something that exists says so. A deep
link carries whatever was typed, so that is a finding and not an artefact.

## Where warnings land

CSS warnings carry a line, in the file's own numbering rather than the
`<style>` block's. An unhonored property reports the line of the *declaration*,
so an expanded rule sends you to the property rather than to its selector.
Selector-level warnings report the line of the rule.

There is no column. The CSS parser locates a rule and gives the declarations
inside it no position of their own, so the line is recovered by scanning
forward from the rule. That finds the line but not the offset within it.

## For an editor

`--format json` prints the diagnostics as JSON, and `--types` adds the types the
checker worked out, which is what hover and completion in the VS Code extension
read. `rux vocab` is the other half: what the runtime understands, for an editor
that wants to complete it.

## Exit codes

`0` clean, `1` problems found, `2` the request itself was wrong. Warnings alone
do not fail unless `--deny-warnings` says so.
