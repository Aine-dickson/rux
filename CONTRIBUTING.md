# Contributing to Rux

Thanks for looking. Rux is early, and the most useful thing you can do right now
is **use it and tell us where it broke**.

## Before anything: what stage this is at

Rux is `0.x`. The language spec is **not frozen** — element names, directives,
and the honored-CSS set can change between point releases, and they have. If you
build something on Rux today, expect to fix it on upgrade.

That has a direct consequence for contributions:

| | Status |
|---|---|
| **Bug reports** | Open. Always welcome, the more specific the better. |
| **Feature requests** | Open. Check the [roadmap](https://ruxlang.dev/roadmap/) first. |
| **Pull requests** | **Limited**, to issues labelled `good-first-issue` or `help-wanted`. |

Pull requests are gated on purpose, and not because outside code isn't wanted.
Merging a change into a language whose design is still moving creates an
obligation to somebody whose work may have to be broken two releases later.
Until the spec settles, that promise is only made where the shape of the answer
is already known — which is what those two labels mean.

If you want to work on something outside that set, **open an issue first** and
say what you have in mind. A "yes, and here's the constraint you'll hit" is much
cheaper for both of us than a rejected diff.

## Reporting a bug

The single most valuable thing you can include is **the `.rux` file**. Rux is a
language runtime; a description of a layout bug is rarely enough to reproduce it,
and a 20-line file almost always is.

Include the OS, the GPU if you can find it (rendering is `wgpu`-backed, so
driver differences are real), and what you expected versus what you saw. A
screenshot of the window beats a paragraph describing the window.

## Working on the runtime

Start with the [architecture tour](https://ruxlang.dev/contribute/) — it walks
the pipeline from `.rux` file to pixels and says which crate owns which stage.
The [reference](https://ruxlang.dev/reference/) is the authoritative account of
what currently works.

```bash
cargo run -p ruxlang -- examples/form.rux   # run an example
cargo test                                  # the suite
cargo build                                 # must be warning-clean
```

### The one rule that isn't obvious

**Drive your change in the window before you call it done.** Not "the tests
pass" — open the app and look at it.

This is not ceremony. Every release so far has shipped with at least one bug
that the test suite was fully green through and that was obvious within seconds
of actually looking at the window: layout that computes correct numbers and
paints wrong, input that reports the right state and shows the wrong caret. A UI
runtime can be wrong in ways that only exist as pixels. The test suite protects
against regressions; it does not tell you the feature works.

Release posts name the bug that was only found by looking, every time. It is the
most useful paragraph in them.

## Style

Match the code around you. The one thing worth stating explicitly: comments here
explain **why**, not what. A comment restating the line beneath it will be asked
about in review; a comment explaining a non-obvious constraint, a workaround, or
a decision that looks wrong until you know the reason is exactly right.

## Licence

Rux is MIT. By contributing you agree your work ships under it. There is no CLA.
