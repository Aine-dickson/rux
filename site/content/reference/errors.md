+++
title = "Errors"
description = "What happens when a document will not load, and what the overlay shows."
weight = 15
+++

<!-- GENERATED FROM docs/05-as-built.md BY site/sync-docs.sh. DO NOT EDIT HERE. -->


Mistakes are shown **in the window**, not only on a stderr nobody running a GUI
app is watching.

- **A file that won't load** opens the window with a red panel naming the file and
  the failure. Parse errors carry a **line and column**, numbered against the whole
  `.rux` file (not the `<template>` section), so they line up with the editor
  gutter: `parse error at line 6, column 16: mismatched closing tag: expected
  </view>, found </vieww>`.
- **A hot-reload that fails keeps the last good UI on screen** and says so
  (*"showing the last version that loaded"*), so a typo mid-edit neither blanks
  the window nor passes unnoticed. Fixing the file clears the overlay; the window
  keeps its size and pointer state across the reload.
- **A document that builds but has dead CSS** gets a quieter amber panel listing
  what does nothing: unhonored properties, unknown pseudo-classes, undefined
  `var()`s, unsupported `@media` conditions, and **expressions that failed**, `expression \`dubble(n)\` failed: Function not found: dubble`. Long lists are
  capped at six with a count of the rest; everything still goes to stderr.
  CSS warnings are prefixed with the line they are on (`line 11: …`).
- **Tapping the panel dismisses it**, and it says so. The panel covers the app it
  is describing, which was a problem when the thing you needed to look at was
  underneath. The dismissal is remembered against *those* diagnostics, so it
  lasts exactly as long as the document's problems are the same ones: fix a
  warning, or introduce an error, and the panel comes straight back. A press
  landing on the panel does not reach the app under it either.

Every shipped example is checked to load **warning-free**, so a noisy overlay in
`examples/` is a test failure.

**The browser playground shows the same diagnostics**, in the page rather than
only on the canvas. `rux-web`'s `diagnose(source)` sets the document and returns
the error (with line and column) and every warning (with its line) as JSON; the
page lists them under the editor and each one that knows its line is a button
that selects that line. It runs on load as well as on Run, since a shared link
carries its source in the URL hash.

> The deployed page is built from `main` while the runtime it loads is pinned to
> the latest **tag**, so the page has to keep working against a build that
> predates its own features. It feature-detects `diagnose` and falls back to the
> older `setSource`, which reports an error and no warnings. The fallback is
> only removable once a deployed build actually carries `diagnose`, which means
> after the tag exists *and* the site has been rebuilt against it: pushing a tag
> deploys nothing on its own, since the workflow fires on pushes to `main`.

**Known limits:** rhai returns `()` for a missing *map property*, rather than
erroring, so `{{ user.nmae }}` still renders empty with nothing reported (a
missing *function* or variable does report). That one is rhai's semantics, not
ours, it is tracked as a motivator for the planned rhai fork in
[Roadmap](/roadmap/) (Further out → *Script documentation*). Expression
failures and anything from a component's CSS are still reported without a line,
for the reasons under "Checking a file without opening a window".

---
