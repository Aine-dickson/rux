# Rux VS Code syntax highlighting

Editor support for `.rux` files:

- **Syntax coloring**: the `<template>`, `<style>` and `<script>` sections and the
  Rux-specific tokens inside them (`{{ }}` interpolation, `r-for` / `r-if` /
  `r-model` directives, `@tap` handlers, `:prop` bindings, `signal(...)`).
- **Snippets**: type `rux` for a full component scaffold; also `template`, `style`,
  `script`, `signal`, `fn`, `rfor`, `rif`, `rmodel`, `tap`, `interp`, and element
  tags (`text`, `view`, `button`).
- **Folding** of the three sections, HTML-style tag indentation, and bracket/quote
  auto-close.
- **Format Document** (`Shift+Alt+F`): a basic re-indenter. It fixes nesting
  indentation across tags, braces, brackets and parens, and unifies mixed indent
  widths to your editor's `tab_size`. It does **not** touch spacing inside a line,
  wrap, or reorder. That's the job of the planned `rux fmt`.
- **File icon**: `.rux` files show the Rux mark, when your active file-icon theme
  falls back to language icons (VS Code's default "Seti" does; some themes override
  it).

> **Note:** `Shift+Ctrl+F` is VS Code's *Search across files*, not format. Format
> Document is **`Shift+Alt+F`** (or right-click → Format Document, or enable
> `"editor.formatOnSave": true`).

### Formatter limitations (deliberate: it's an indenter, not `rux fmt`)

- Multi-line continuations of a single statement (a wrapped attribute, text
  content on its own lines, or a multi-line array literal) are indented to
  structural depth, not hand-aligned to the opener.
- Lines inside a multi-line comment are left exactly as written.
- The real formatter (`rux fmt`: parse → pretty-print via the actual Rux parser)
  will supersede this and handle alignment properly. See the "Dev tooling" section
  of `docs/06-roadmap.md`; `rux check` diagnostics and a language server are on the
  same track.

## Install locally

```
cd editors/vscode
npx @vscode/vsce package     # produces rux-<version>.vsix
code --install-extension rux-0.1.0.vsix
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
