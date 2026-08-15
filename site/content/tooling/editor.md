+++
title = "Editor setup"
description = "The VS Code extension: syntax coloring, completions from the runtime's own vocabulary, tag auto-closing, formatting and diagnostics."
weight = 6
+++

VS Code has an extension for `.rux` files. It lives in
[`editors/vscode`](https://github.com/Aine-dickson/rux/tree/main/editors/vscode)
in the repository.

## Installing it

```bash
git clone https://github.com/Aine-dickson/rux
cd rux/editors/vscode
npx @vscode/vsce package
code --install-extension rux-*.vsix
```

Then open any `.rux` file.

Formatting and diagnostics shell out to the `rux` binary, so install that
first:

```bash
cargo install ruxlang
```

Set `rux.path` if it lives somewhere other than your PATH. Completions and tag
auto-closing do **not** need the binary: the extension ships with the
vocabulary baked in, so they work on a machine where `cargo install` has not
finished yet.

## What it does

**Syntax coloring** for all three sections and the Rux-specific tokens inside
them: `{{ }}` interpolation, the `r-` directives, `@tap` handlers, `:prop`
bindings, `signal(...)`.

**Completions**, offered from what the runtime actually understands:

- In `<template>`: the elements, the directives, each element's own attributes,
  and **the components this file imported**. A `use components::crew_detail;`
  in the script offers `<crew-detail>` in the template, with the underscore to
  hyphen change the runtime makes.
- In `<style>`: only the CSS properties Rux honors. This is the completion a
  general CSS extension cannot give you correctly, because Rux honors a subset
  and warns about the rest.
- In `<script>`: `signal`, `computed`, `effect`, `query`, the router calls.

The lists come from [`rux vocab`](@/tooling/vocab.md), which reads them out of
the runtime. If the editor offers it, it works.

**Tag auto-closing.** Finishing `<view>` writes `</view>` and leaves the cursor
between them; typing `</` completes the nearest tag still open. Elements that
never nest, `<image>` and `<input>`, never get a closing tag. Turn it off with
`rux.autoClosingTags`.

**Snippets.** Type `rux` for a full component scaffold; also `template`,
`style`, `script`, `signal`, `fn`, `rfor`, `rif`, `rmodel`, `tap`, `interp`,
and the element tags.

**Format Document**, which runs [`rux fmt`](@/tooling/fmt.md) at your editor's
own tab size.

**Diagnostics** from [`rux check`](@/tooling/check.md), as squiggles, when you
open and save a file.

**Folding** of the three sections, tag indentation, and bracket auto-close.

## Format Document is Shift+Alt+F

`Shift+Ctrl+F` is VS Code's *Search across files*, not format. Format Document
is **`Shift+Alt+F`**, or right-click and pick it, or set
`"editor.formatOnSave": true`.

## Settings

| | |
|---|---|
| `rux.path` | Where the `rux` binary is. Defaults to finding it on your PATH |
| `rux.check.enable` | Report problems from `rux check` on open and save |
| `rux.autoClosingTags` | Write the closing tag when you finish an opening one |

## Diagnostics refresh on save, not on every keystroke

`rux check` reads the file from disk on purpose: it resolves `use` imports
relative to the file's own directory, which a buffer piped over stdin no longer
has.

## Not yet

There is no language server, so there is no go-to-definition, no rename, and no
hover on a component telling you its props. The extension is not yet on the
Marketplace, which is why the install above starts with a clone.

Other editors have nothing beyond what the TextMate grammar in the repository
gives them. It is a standard `.tmLanguage.json` and works anywhere that reads
one.
