+++
title = "rux build"
description = "Turn a project into one executable you can hand to someone. Reads rux.toml, embeds the documents, and writes to dist/."
weight = 5
+++

`rux build` turns a project into something you can give to another person.

```bash
rux build                      # a dev build: reads the project from disk
rux build --release            # embeds the documents into one executable
rux build --target desktop     # the default, and the only target so far
```

The result lands in `dist/`, as a single executable named after the app.

## It needs a `rux.toml`

A build needs to know what the app is called and what an operating system
should file it under, and neither of those can be guessed from a document. So
`rux build` reads a manifest, found here or in any parent directory, the same
way `rux run` finds an entry point.

```toml
[app]
name = "Task List"
id = "dev.example.tasks"
version = "0.1.0"
```

`name` is for a person to read, and the executable is named after it in a
safer spelling: `Task List` builds `task-list`.

`id` is reverse-DNS, and it is the one field worth getting right the first
time. It is what an operating system files the app under, so changing it later
produces a different app rather than an update. Android refuses an id with no
dot in it, and so does `rux build`.

`version` is the app's own version, which has nothing to do with the version
of Rux that built it.

`entry` is optional. Without it the build opens the same `app.rux` or
`index.rux` that `rux run` would, so a project following the convention writes
nothing.

Nothing else is accepted yet. Icons, splash screens and signing all have
manifest keys waiting for them, and each one lands in the release that makes it
do something rather than ahead of it.

## Dev builds and release builds differ in one way

A **dev build** points the executable at the project directory. Documents are
read from disk as the app runs, so hot reload works exactly as it does under
`rux run`. This is the build to use while working.

A **release build** embeds every document into the executable. Nothing is read
from disk, the artifact carries its own contents, and what ships cannot drift
from what was tested. Move it to another machine with no Rux and no project
directory and it still runs.

The same split is what Android will use: documents served as assets and
reloaded over `adb` while developing, embedded for release.

## What it needs installed

`rux build` writes a small Rust crate and compiles it, so it needs a Rust
toolchain. If `cargo` is not on the path it says so and points at
[rustup.rs](https://rustup.rs).

The generated crate lives in `.rux-build/`, inside the project. It is
disposable and is rewritten on every build, so there is nothing in it worth
editing and nothing in it worth committing.

## Known gap

A release build does not yet paint images. The layout is correct and an
`<image>` still takes its natural size, but the pixels are read through a path
that does not exist inside the executable, so the image draws empty. Documents,
stylesheets, components and every other part of an app embed correctly. Use a
dev build where images matter until this is closed.
