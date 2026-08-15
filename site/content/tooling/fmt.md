+++
title = "rux fmt"
description = "Re-indent templates and scripts, format the CSS, and rewrite nothing else."
weight = 4
+++

```bash
rux fmt                        # every .rux under the current directory, in place
rux fmt app.rux                # or a named file or directory
rux fmt --check .              # change nothing; exit non-zero if a file would
rux fmt --indent 4 app.rux     # one indent level: spaces, or `tab` (default 2)
rux fmt --stdout app.rux       # write the result out instead of back
rux fmt -                      # read stdin, write stdout, which is what an editor uses
```

## What it changes, and what it leaves alone

`<template>` and `<script>` are **only re-indented**. Nothing on a line is
rewritten, wrapped or reordered.

That restraint is deliberate. A `@tap` handler is a script expression, and
rearranging someone's expressions is not a formatter's business. The cost of
being wrong there is silently changing what a program does, which is a much
worse failure than an ugly line.

`<style>` **is** formatted, because CSS has one obvious shape and no such
hazard. One space before `{`, long rules broken one declaration per line, short
ones of up to three declarations kept inline.

Line endings are preserved.

## One implementation

The VS Code extension used to carry its own re-indenter, a port of this one in
JavaScript. The two drifted within a week: the JavaScript copy inherited HTML's
list of tags that never nest, which has `img` but not Rux's `<image>`, so an
`<image src="…">` written without a self-closing slash over-indented everything
after it.

The extension now shells out to this binary. An editor that formats differently
from the project's own tool is worse than one that says it cannot find the tool.

## The shipped examples are not formatted to this

All of the examples in the repository differ from what `rux fmt` would write,
about half on indent width and half on CSS, which inlines short rules that the
examples write expanded.

Running the formatter over them is a real decision rather than an oversight:
the expanded form may read better when teaching. It has not been made.

Projects created by [`rux new`](@/tooling/new.md) *are* formatted to this, and
a test asserts it, so the first `rux fmt` in a new project changes nothing.
