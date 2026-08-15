+++
title = "Tooling"
description = "The rux command and the editor support: what each command does, and how to set them up."
weight = 2
sort_by = "weight"
template = "docs-section.html"
page_template = "docs.html"
+++

One binary, `rux`, does everything: creates a project, runs it, checks it,
formats it. There is no separate build tool, no package manager, and nothing to
configure before the first window opens.

```bash
cargo install ruxlang
```

That puts a `rux` command on your PATH. It takes a few minutes, because it
builds a GPU renderer and a text shaper from source. Nothing else is needed:
no Node, no npm, no browser, no system GUI toolkit.

## The commands

| | |
|---|---|
| [`rux new`](@/tooling/new.md) | Create a project |
| [`rux run`](@/tooling/run.md) | Open a window |
| [`rux check`](@/tooling/check.md) | Report what is wrong, without opening one |
| [`rux fmt`](@/tooling/fmt.md) | Re-indent, and format the CSS |
| [`rux vocab`](@/tooling/vocab.md) | Print what the runtime understands, for editors |

Bare `rux` prints the usage, the way `cargo` and `git` do.

## Editors

[VS Code](@/tooling/editor.md) has an extension: syntax coloring, completions
drawn from the runtime's own vocabulary, tag auto-closing, formatting and
diagnostics.

## Exit codes

Every command uses the same three, so a shell script or a CI job can act on
them without special cases.

| Code | Meaning |
|---|---|
| `0` | Clean |
| `1` | Problems found |
| `2` | The request itself was wrong: no such path, an unknown flag |
