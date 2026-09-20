+++
title = "rux new"
description = "Create a project: the entry point, a component, and the two directories the runtime already looks in."
weight = 1
+++

`rux new` creates a project that runs as it stands.

```bash
rux new my-app
cd my-app
rux run
```

## What it writes

```
my-app/
  app.rux              the entry point
  components/
    task.rux           imported by `use components::task;`
  assets/              images
  README.md
  .gitignore
```

Both directory names are conventions the runtime already followed, which is the
reason they are the ones scaffolded:

- `use components::task;` in a `<script>` names the file `components/task.rux`.
  An underscore in the path becomes a hyphen in the tag, so
  `use components::crew_detail;` gives you `<crew-detail>`.
- `<image src="assets/logo.png">` resolves **relative to the `.rux` file that
  names it**, not relative to the directory you ran `rux` from. An app launched
  from anywhere finds its own images.

The generated app is a small task list. It is deliberately not a hello world:
it uses every idea the language has, so the file answers "how do I do this"
for the five things people reach for first. State with `signal`, a two-way
bound `<input>` with `r-model`, a `@tap` handler that calls a named function,
a list with `r-for` and `r-key`, and a component receiving props.

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
