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
- **A document that builds but has dead CSS or a failing expression** gets a
  quieter amber panel listing what does nothing: unhonored properties, unknown
  pseudo-classes, undefined `var()`s, unsupported `@media` conditions, and
  **expressions that failed**, such as ``expression `dubble(2)` failed: there is
  no function `dubble` taking 1 argument``. Long lists are capped at six with a
  count of the rest; everything still goes to stderr. The panel is red when
  anything in it is an error.
- **Every finding carries its line**, numbered against the whole `.rux` file:
  a handler on its own attribute's line, an `r-if` or `r-for` on the directive,
  a `{{ }}` on its text run, a CSS warning on its rule. `rux check --format json`
  and the editor gutter place them the same way.
- **A finding raised inside an imported component names that component's file**
  and its own line, not the document that imported it.
- **Some of these are errors, even though the document built.** A load either
  works or does not; that is separate from whether what loaded is *right*. An
  expression that cannot resolve, a missing property (`{{ user.nmae }}`), a type
  that does not fit, an `@event` the runtime never dispatches and a value on a
  valueless directive are all definitely wrong, so `rux check` exits non-zero
  for them. The window still runs the app: blanking it over a typo mid-edit
  would be worse than useless. Type findings are described in [Types](https://github.com/Aine-dickson/rux/blob/main/docs/10-types.md);
  every page and every component is checked in its own file and scope, and a
  script that does not parse fails its load at its line.

  **Reading an undefined name is an error in the app's document and a warning
  in a component opened on its own.** The document has no caller, so a name it
  does not declare can come from nowhere, and neither can one in a component it
  builds. A component opened by itself (`rux check components/stat.rux`) may be
  handed names by a caller: one it declares with `prop` is owed and is not
  reported at all, and any other stays a warning. Declaring a `prop` makes a file
  a component whoever opens it.
- **Load-time findings stay listed.** The import, prop and attribute checks read
  the whole template once, at load, and are kept on the document and put back
  after each rebuild, so a bogus `@frob` on a page with a `mounted` block is
  still reported.
- **Tapping the panel dismisses it**, and it says so. The dismissal is remembered
  against *those* diagnostics, so it lasts exactly as long as the document's
  problems are the same ones: fix a warning, or introduce an error, and the
  panel comes straight back. A press landing on the panel does not reach the app
  under it either.
- **`print(x)` output waits in a corner badge.** Prints show as one line in the
  bottom right ("3 printed: …") that a tap opens into the panel, below the
  findings; a print never brings back a panel dismissed for warnings.

- **A handler or a `fn` that calls something which cannot exist** is reported
  at load, before anything is tapped, with the line of the call. Every `@event`,
  every route `guard` and every `fn` body in `<script>` is checked, including the
  ones behind a false `r-if` and the ones nothing calls yet.

  **Names, not argument counts.** A name that is registered nowhere, defined by
  no `fn` and built into nothing can never resolve under any state, so saying so
  cannot be a false alarm. A call with the wrong number of arguments to a `fn`
  is reported too, naming how many it takes.

  One case is reported by shape rather than by name: **`signal.set(x)` and
  `signal.get()`**, the old signal API, which ordinary assignment replaced.
  `tasks.set(0, "x")` on an array signal is legitimate and stays silent, while
  `searching.set(true)` is reported with the assignment to write instead.

Every shipped example is checked to load **warning-free**, so a noisy overlay in
`examples/` is a test failure.

**The browser playground shows the same diagnostics**, in the page rather than
only on the canvas. `rux-web`'s `diagnose(source)` sets the document and returns
the error (with line and column) and every warning (with its line) as JSON; the
page lists them under the editor and each one that knows its line is a button
that selects that line. It runs on load as well as on Run, since a shared link
carries its source in the URL hash.

---
