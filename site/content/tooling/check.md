+++
title = "rux check"
description = "Report what is wrong with a file or a tree, without opening a window. The same loader the runtime uses."
weight = 3
+++

`rux check` reports what is wrong without opening a window.

```bash
rux check                      # every .rux under the current directory
rux check examples             # or a named file or directory
rux check --deny-warnings .    # warnings fail too, which is what CI wants
rux check --format json .      # for an editor to turn into squiggles
```

Output is `path:line:col: severity: message`, the shape every compiler emits
and every editor and CI log already knows how to parse.

## It cannot disagree with the runtime

Documents load through the same code the window uses. A checker with its own
parser would eventually accept a file the runtime rejects, or the reverse, and
the failure would land on whoever trusted it.

**Errors** are failures to load, and carry a line and column. **Warnings** are
the things the dev overlay lists: unhonored CSS, unknown pseudo-classes,
undefined `var()`, expressions that did not evaluate.

## Handlers are compiled at load

Every event handler is compiled when the document loads, and one that will not
compile is a warning.

Nothing compiled a handler until it was tapped, which meant a syntax error
reached the window as a button that looked correct and did nothing at all.
Handlers inside branches that are not currently rendered are checked too, since
a false `r-if` is where a broken handler hides longest.

It is syntax only. A handler naming an `r-for` local or a component's own state
is fine, because those are runtime lookups and not compile errors.

## Walking a directory skips components

A component's `{{ prop }}` values come from whoever uses it, so loading one on
its own would report every prop as undefined. A component is a file that
something else in the project writes as a tag, and those are skipped when
walking.

Naming a component explicitly checks it anyway, because that was asked for on
purpose, and there its undeclared names are warnings rather than errors:

```bash
rux check components/task.rux
```

A file that nothing uses is a document and is checked like one. Nobody can
supply a name it never declares, so a name it never declares is wrong.

## Pages are checked through the app

A router builds the matching route and nothing else, so loading a document
reaches exactly one page: the one at `/`. The checker visits the rest, every
`<route>` in turn, with `rux-check` standing in for each `:parameter` and one
visit to nowhere when there is a `fallback`.

That is the only way a page can be checked at all. A page reads the app's
signals and calls its functions, and those are shared into the engine by the
document that declares them, so a page read on its own reports every one of
them as missing. Asking about a page therefore checks the app:

```bash
rux check pages/home.rux
rux: pages/home.rux checked through app.rux, where the app's signals and functions are in scope
```

You get what is wrong with the page and with the app, not with the app's other
pages. A parameter is filled with `rux-check` rather than a real value, so a
page that assumes its parameter names something that exists says so. A deep
link carries whatever was typed, so that is a finding and not an artefact.

## Where warnings land

CSS warnings carry a line, in the file's own numbering rather than the
`<style>` block's. An unhonored property reports the line of the *declaration*,
so an expanded rule sends you to the property rather than to its selector.
Selector-level warnings report the line of the rule.

There is no column. The CSS parser locates a rule and gives the declarations
inside it no position of their own, so the line is recovered by scanning
forward from the rule. That finds the line but not the offset within it.

## Exit codes

`0` clean, `1` problems found, `2` the request itself was wrong. Warnings alone
do not fail unless `--deny-warnings` says so.
