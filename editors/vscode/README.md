# Rux VS Code support

Editor support for `.rux` files:

- **Syntax coloring**: the `<template>`, `<style>` and `<script>` sections and the
  Rux-specific tokens inside them (`{{ }}` interpolation, `r-for` / `r-if` /
  `r-model` directives, `@tap` handlers, `:prop` bindings, `signal(...)`).
- **Snippets**: type `rux` for a full component scaffold; also `template`, `style`,
  `script`, `signal`, `fn`, `rfor`, `rif`, `rmodel`, `tap`, `interp`, and element
  tags (`text`, `view`, `button`).
- **Folding** of the three sections, HTML-style tag indentation, and bracket/quote
  auto-close.
- **Format Document** (`Shift+Alt+F`): runs `rux fmt`. It re-indents the
  `<template>` and `<script>` sections and formats the CSS in `<style>`, at your
  editor's own `tab_size`. Nothing inside a template or script line is rewritten,
  wrapped or reordered.
- **Diagnostics**: problems from `rux check` appear as squiggles when you open
  and save a file. Errors are failures to load and point at a line and column;
  warnings are the things the dev overlay lists (unhonored CSS, unknown
  pseudo-classes, undefined `var()`, failed expressions) and carry only a file so
  far, so they sit on line 1.
- **File icon**: `.rux` files show the Rux mark, when your active file-icon theme
  falls back to language icons (VS Code's default "Seti" does; some themes override
  it).

> **Note:** `Shift+Ctrl+F` is VS Code's *Search across files*, not format. Format
> Document is **`Shift+Alt+F`** (or right-click → Format Document, or enable
> `"editor.formatOnSave": true`).

## Requires the `rux` binary

Formatting and diagnostics shell out to it:

```
cargo install ruxlang        # puts a `rux` command on your PATH
```

Set `rux.path` if it lives somewhere else, and `rux.check.enable` to `false` to
turn the squiggles off. The extension says so once, rather than on every
keystroke, if it cannot run the binary.

This used to be a re-indenter written in JavaScript here. Two implementations of
the same rules drifted within a week: the JS copy inherited HTML's void-tag list,
which has `img` but not Rux's `<image>`, so an `<image src="...">` written without
a self-closing slash over-indented everything after it, and it never formatted
CSS at all. An editor that formats differently from the project's own tool is
worse than one that asks you to install the tool.

### What the formatter still does not do

- Multi-line continuations of a single statement (a wrapped attribute, text
  content on its own lines, or a multi-line array literal) are indented to
  structural depth, not hand-aligned to the opener.
- Lines inside a multi-line comment are left exactly as written.
- It re-indents rather than reprints: a full parse to a tree and back is still
  the eventual plan. See "Dev tooling" in `docs/06-roadmap.md`, where a language
  server is on the same track.

### Diagnostics refresh on open and save, not as you type

`rux check` reads the file from disk on purpose: it resolves `use` imports
relative to the file's own directory, which a buffer piped over stdin no longer
has. Live diagnostics are a job for the language server.

## Install locally

```
cd editors/vscode
npx @vscode/vsce package     # produces rux-<version>.vsix
code --install-extension rux-0.2.0.vsix
```

Then open any `.rux` file. Publishing to the Marketplace is optional and needs a
publisher account.

## One grammar, two consumers

`syntaxes/rux.tmLanguage.json` is a TextMate JSON grammar. The **same file** is
copied to `site/syntaxes/rux.tmLanguage.json`, where Zola 0.22's highlighter
(Giallo) reads it to color ` ```rux ` fences on the website. There is one source
of truth; **when you edit one copy, copy it to the other:**

```
cp editors/vscode/syntaxes/rux.tmLanguage.json site/syntaxes/rux.tmLanguage.json
```

## Known imprecision

The `<script>` section is rhai, which has no standard TextMate grammar, so it is
colored with self-contained Rust-ish patterns (keywords, comments, strings,
numbers, `signal(...)`). Close, not exact: rhai-only constructs won't be perfect.
The `<style>` section covers the common CSS surface these files use (selectors,
combinators, `@media`, pseudo-classes, colors, units, `var()`); the interior of a
`@media (...)` condition is left uncolored. This is deliberate: the grammar is
fully self-contained (no `source.css` / `source.rust` / `text.html.basic`
includes) so it renders identically in VS Code and Giallo regardless of what
either host bundles.
