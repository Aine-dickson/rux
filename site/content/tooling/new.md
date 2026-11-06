+++
title = "rux new"
description = "Create a project: the entry point, a page, a component, the Rust half in native/, and the directories the runtime already looks in."
weight = 1
+++

`rux new` creates a project that runs as it stands.

```bash
rux new my-app
cd my-app
rux run
```

Run at a terminal with no name, it asks for one (`Project name:`) and carries on
as if the name had been given. A script or CI job has nobody to answer, so there
it still stops with an error.

```
rux new [name] [--template minimal|tasks] [--router|--no-router] [--no-native]
```

## The router question

At a terminal, the `minimal` template asks `Include a router? [Y/n]`. Enter means
yes. `--router` or `--no-router` answers it and skips the question. Where nobody
can answer (a script, CI) the router is on, so those runs are unchanged.

With a router, `app.rux` has a tab bar and two routes, `/` and `/about`, plus a
fallback, and `pages/about.rux` is the second page. Without one, `app.rux` shows
`pages/home.rux` as `<home />`, there is no `pages/missing.rux` or `about.rux`,
and the project still checks clean. A router can be added later by writing the
`<router>` and a `<route>` for each page.

The `tasks` template is a list, a form and a page per task, so it is always
routed, and `--no-router` with it is an error.

## What it writes

```
my-app/
  app.rux              the frame and the routes
  pages/
    home.rux           the home page, which calls Rust
    missing.rux        the fallback, for a path nothing matches
  components/
    message-card.rux   a card that takes props
  native/              the app's Rust, always here, beside rux.toml
    Cargo.toml         its dependencies
    src/lib.rs         one `pub mod` per `native::<module>`
    src/hello.rs       the greeting on the home page
  assets/              images
  rux.toml             name, id and version
  README.md
  .gitignore
  .vscode/settings.json
```

That is the `minimal` template, and the default: a frame, one page, one
component, and nothing to delete before the first line of your own. The README
it writes describes exactly this tree and is written for the choices made, so it
never names a directory that is not there.

`--template tasks` writes the larger worked example instead: a small task list
with four pages, a store in `stores/`, a form and a component. It is the answer
to "how is a bigger project split", and uses every idea the language has: state
with `signal`, a two-way bound `<input>` with `r-model`, a `@tap` handler that
calls a named function, a list with `r-for` and `r-key`, and a component
receiving props.

`pages/`, `components/` and `assets/` are conventions the runtime already
followed, which is the reason they are the ones scaffolded:

- `use components::message_card;` in a `<script>` names the file
  `components/message-card.rux`. An underscore in the path becomes a hyphen in
  the tag, so `use components::crew_detail;` gives you `<crew-detail>`.
- `<image src="assets/logo.png">` resolves **relative to the `.rux` file that
  names it**, not relative to the directory you ran `rux` from. An app launched
  from anywhere finds its own images.

## The Rust half

**An app's Rust lives in `native/`, beside `rux.toml`, in every project.** It is
an ordinary library crate, so the convention is one a person learns once and
finds in the same place every time. It is on unless `--no-native` turns it off,
which also leaves out `.vscode/settings.json`, the file that points
rust-analyzer at a crate that is not at the project root.

A function marked `#[rux::export]` in `native/src/hello.rs` is `hello.greet(...)`
after `use native::hello;`, and `rux check` reads its signature from the source
without building anything. `rux native` prints what Rux sees.

Crates from crates.io go in `native/Cargo.toml`, added with `cargo add` from
inside `native/`. They are compiled into the app for every target, and not every
crate builds for Android or the web; one that links OpenSSL or starts threads is
the usual trouble. The first `rux run` compiles the app's Rust together with
Rux, which takes several minutes, and later runs rebuild only when something
under `native/` changes.

The `rux-native` dependency is written with the version of the `rux` that
created the project. It has to be the version that builds the app, because a
second copy would be a second registry, and the exports would land in the one
nobody reads.

The generated `.gitignore` covers what a build writes (`dist/`, `.rux-build/`,
`target/`) and not `locks/`, which is meant to be committed. See
[`rux build`](@/tooling/build.md) for where builds land and why the locks exist.

## The manifest it writes

**`rux run` and `rux check` need nothing but a directory containing `app.rux`
or `index.rux`**, and that has not changed. `rux build` needs more, because it
has to say what the app is called and what an operating system should file it
under, and neither can be guessed from a document. So the scaffold writes a
`rux.toml` with the three keys a build cannot infer:

```toml
[app]
name = "my-app"
id = "dev.example.my_app"
version = "0.1.0"
```

**Change the `id` before you ship anything.** `dev.example` is a placeholder.
An operating system files the app under that name, so changing it later
produces a different app rather than an update, and no store offers a way back.

`entry` is deliberately absent: `app.rux` is what the lookup prefers anyway, and
writing it down would turn a convention into configuration on the first day.
An icon and a signing key are absent for the same kind of reason, since they
name things a new project does not have yet. See
[`rux build`](@/tooling/build.md) for both.

## What it refuses

An existing directory with anything in it. A scaffold that silently overwrote
an `app.rux` someone had been working in would be unforgivable, and there is
no way to ask.

Names are checked more strictly than the filesystem would: letters, digits,
`-`, `_` and `.` only. The name is also what you type after `cd`, and one day a
plausible default for a binary and a window title, so a space or a quote in it
is a papercut in several places at once.

## What comes out is already clean

`rux check` reports nothing on a fresh project, and `rux fmt` changes nothing,
because the scaffold is written the way the formatter writes. Both are asserted
by tests, so the first `rux fmt` in a new project cannot rewrite the file the
scaffold had just created.
