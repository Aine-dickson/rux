# Removed from docs/05-as-built.md, first part (intro through "Where Rux differs from CSS")

## Header (removed 2026-11-06 from 05-as-built.md)

Last updated: 2026-08-19, for **v0.7**. The original M0–M9 milestones that built
the runtime are all complete; everything since has shipped in the v0.2 through
v0.6 releases, which the [Roadmap](./06-roadmap.md) lists.

## Starting a project (removed 2026-11-06 from 05-as-built.md)

Each key landed in the commit that made it do something.

**`rux new` writes one**, with `name`, `id` and `version`, so a scaffolded
project builds without anything being added by hand. It used to not, on
reasoning that expired when `rux build` landed.

## Running it (removed 2026-11-06 from 05-as-built.md)

**Bare `rux` prints the usage**, the way `cargo` and `git` do. A tool that
launches a GUI when invoked with no arguments is a surprise, and this one used
to do something worse: it defaulted to `examples/battery.rux`, a path that
exists only in a checkout of this repo, so the first thing typed after
`cargo install ruxlang` was a panic out of the file watcher.

Portrait only so far. Rotation moves an inset from one edge to another, and the
runtime handles that (an inset that moves re-cascades), but there is no way to
ask for it from the command line yet.

## A frame that only scrolls (removed 2026-11-06 from 05-as-built.md)

Measurements, release build:

- A list of 1,000 rows: 16.8 ms of layout a scrolled frame, then 4.2 ms (about 4x faster).
- 2,000 rows with the clipped-away rows passed over: 7.2 ms of layout a scrolled frame, then
  0.9 ms (about 8x faster), and 8,003 paints a frame down to 107.
- 2,000 `content-visibility: auto` rows: 8.9 ms a scrolled frame, then 1.0 ms
  (about 9x faster), with a frame of about 10 ms each time the shown boxes change.
- The self-checks (each kept layout laid out again whole, each lone list built again, each
  reused row built again from nothing) cost, in an app's debug build, 400 ms of layout a frame
  on the same list against 12.7 ms without them.

**A window's GPU memory.** It held about 475 MB: the renderer's scratch buffers were fixed at
about 150 MB, and the GPU allocator reserved memory in 256 MB blocks to hold 10 MB. The buffers
are now sized to what the scene draws, at about 1.6 ms a frame for reading back what it needed,
and the allocator takes small blocks (see `vendor/README.md`).

A row scrolled up out of its list used to lie over the header above it, invisible, and take the
header's taps.

## List rows are reused (removed 2026-11-06 from 05-as-built.md)

Before reuse, every keyed row was styled again on every change: a list of 300 rows with one row
moving a frame spent 12 ms a frame building rows that had not changed.

Measured with the script-cost benchmark, release, desktop:

| benchmark | before | after |
|---|---|---|
| 300 rows, one moves a frame: patch | 13.7 ms | 6.8 ms |
| same, whole frame | 33.0 ms | 25.7 ms |
| filter 2,000 records to 30 rows: whole frame | 20.4 ms | 20.0 ms |

## Formatting (removed 2026-11-06 from 05-as-built.md)

This is the one implementation. The VS Code extension used to carry its own copy
in JavaScript and the two drifted: the JS list of non-nesting tags came from
HTML, which has `img` but not Rux's `<image>`, so everything after an `<image
src="…">` without a self-closing slash was indented one level too deep. The
extension now shells out to `rux fmt`.

## Checking a file without opening a window (removed 2026-11-06 from 05-as-built.md)

Nothing compiled a handler until it was tapped, so a syntax error used to reach the window as a
button that looked right and did nothing at all.

It used to skip the files whose template root was not `<screen>`, which let a
layout choice decide whether a file was opened at all. A page under a router
has a `<view>` root, so a project's pages were filed as components and never
looked at, and `rux check` said "checked 1 file, no problems found" over a
project of six.

Anything from a component's CSS was once unplaced: A component's rules live in a *different*
file, and a warning carried a line but not a file, so a line from the component's numbering would
point confidently at the wrong part of whichever document imported it. A wrong line is worse than
none.

## Checking the machine, not the document (removed 2026-11-06 from 05-as-built.md)

**Every finding carries where it was looked for and the one command that fixes
it.** That is the whole feature. "Mobile without Android Studio" is kept or
broken by what happens when a piece is absent: `error: aapt2 not found` sends
someone to a search engine, and a line naming the path and the command does
not. A tool installed somewhere else and a tool not installed at all are the
same message in most build systems and are not the same problem.

**API 26 is the floor**, decided for Vulkan driver dependability under `wgpu`
rather than taste, and biased high on purpose: lowering a floor later gains
users in one line, while raising one drops devices from under people who have
already shipped.

It is **written against paths, not against the machine it was written on**.
Every lookup takes the roots to search, so the case that matters, a machine
with none of this installed, is an ordinary test against an empty directory.
The machine this was built on has the whole toolchain already, put there by
Flutter for unrelated work, so a passing run on it proves nothing. It still
found something real: the Rust target was not installed.

## Telling an editor what the runtime understands (removed 2026-11-06 from 05-as-built.md)

That gate exists because this exact drift already shipped: the extension's own void-tag list was
inherited from HTML, which has `img` and not Rux's `<image>`, and over-indented everything after an
`<image src="…">` for two releases.

## Elements (removed 2026-11-06 from 05-as-built.md)

Until v0.7.1 the parser demanded the closing tag and, not finding it, named whichever closing tag
it found next, so `<view><input></view>` was reported as "expected `</input>`, found `</view>`",
against a file whose only mistake was being written the way HTML is written.

## Layout (removed 2026-11-06 from 05-as-built.md)

> **DIVERGENCE from docs 01–04.** The inline/block-by-role model was **built and
> then deliberately removed**. taffy has no inline text-flow, so inline elements
> hugged inside flex parents but filled inside block ones (full-width buttons),
> confusing. It's gone.

Flex cross-axis defaulted to `flex-start` until v0.8, a preference taken for ergonomics and
reverted because Rux follows CSS's defaults. Until v0.9 `repeat()` and every grid name were
silently not grids at all: the track list was misread.

## Where Rux differs from CSS (removed 2026-11-06 from 05-as-built.md)

**The rule, set 2026-09-18:** Documenting a divergence does not pay for it; only necessity does,
and "better ergonomics" is not necessity.

| Difference | Kind | Standing |
|---|---|---|
| ~~Flex cross-axis defaults to `flex-start`, not `stretch`~~ | a preference, taken for ergonomics | **Reverted in v0.8.** The default is `stretch`, as in CSS |
| ~~`box-sizing` defaults to `border-box`, not CSS's `content-box`~~ | found 2026-10-23, never chosen: Taffy's default, taken without a decision | **Reverted 2026-10-23, the owner's call.** The default is `content-box`, as in CSS: `width: 100px` with `padding: 20px` is 140px wide. `<input>` and `<button>` are `border-box` unless written otherwise, as browsers' own sheets make their controls, and the root is the window's size whatever its padding. The examples now open with `* { box-sizing: border-box; }`, the reset web authors write |
