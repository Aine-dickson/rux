# Rux — VS Code syntax highlighting

Editor support for `.rux` files:

- **Syntax coloring** — the `<template>`, `<style>` and `<script>` sections and the
  Rux-specific tokens inside them (`{{ }}` interpolation, `r-for` / `r-if` /
  `r-model` directives, `@tap` handlers, `:prop` bindings, `signal(...)`).
- **Snippets** — type `rux` for a full component scaffold; also `template`, `style`,
  `script`, `signal`, `fn`, `rfor`, `rif`, `rmodel`, `tap`, `interp`, and element
  tags (`text`, `view`, `button`).
- **Folding** of the three sections, HTML-style tag indentation, and bracket/quote
  auto-close.

Deeper tooling (a `rux fmt` formatter, `rux check` inline diagnostics, and a
language server) is planned as CLI subcommands the extension shells out to — see
the "Dev tooling" section of `docs/06-roadmap.md`.

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
numbers, `signal(...)`). Close, not exact — rhai-only constructs won't be perfect.
The `<style>` section covers the common CSS surface these files use (selectors,
combinators, `@media`, pseudo-classes, colors, units, `var()`); the interior of a
`@media (...)` condition is left uncolored. This is deliberate: the grammar is
fully self-contained (no `source.css` / `source.rust` / `text.html.basic`
includes) so it renders identically in VS Code and Giallo regardless of what
either host bundles.
