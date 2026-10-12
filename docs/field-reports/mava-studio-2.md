# Field report: Mava Studio 2

Mava Studio 2 (`github.com/Mava-labs/mava_studio_2`, private) is a course authoring studio whose whole interface is written in Rux, with its domain in Rust behind a `native/` crate. It is the first application of this size built on Rux outside this repository. Every limit it hits is recorded here as it is found, and mirrored in Mava's own `docs/upstream-limits.md` under the same number.

Each entry says what Mava needed, what happened, how to reproduce it, what Mava does meanwhile, and its status here.

## MS-001 `rux build` from a `-dev` CLI names unpublished crate versions

- **Found:** 2026-10-12, Mava feature F01 (workspace and crate layout), `build/v0.8` at `cb7e026` (CLI installed from the checkout).
- **Mava record:** L-001.
- **Limit:** for an app with a `native/` crate outside this repository, `rux run` builds against the checkout the CLI was built from (`native::run` passes `default_rux_source()`), but `rux build` does not: `build_command` starts with `rux_source: None`, so the wrapper manifest names `rux-shell = "0.8.0-dev"` and `rux-runtime = "0.8.0-dev"` from crates.io, which never has a `-dev` version. The build fails with `no matching package named rux-native found`.
- **Reproduction:** any project with a `native/` crate depending on `rux = { package = "rux-native", version = "0.8.0-dev" }`, outside this repository; run `rux build` in it.
- **Mava workaround:** `rux build --rux-source "C:/Users/USER/Mobile/UI research"`.
- **Suggested fix:** let `build_command` default `rux_source` to `default_rux_source()` as `rux run` does, so both commands agree on where the Rux crates come from.
- **Status:** fixed 2026-10-12, as suggested. `rux run --device` had the same gap and takes the same default. A released `rux` still names published versions, and `--rux-source` still overrides. The Mava workaround can go.

## MS-002 The window title and initial size cannot be set by the app

- **Found:** 2026-10-12, Mava feature F01, same build.
- **Mava record:** L-002.
- **Limit:** the desktop shell always titles the window `Rux · <entry file name>` and opens it at 420 × 640 logical pixels (`rux-shell/src/lib.rs`, where the window attributes are built). `[app] name` in `rux.toml` is not used for the title, and there is no way to ask for a desktop-sized, resizable-from-larger, minimum-size or maximised window. A desktop authoring application needs its own name in the title bar and a window sized for a canvas, panels and an inspector.
- **Reproduction:** `rux build` any app whose `rux.toml` sets `[app] name = "Mava Studio"`; run it. The title reads `Rux · app.rux` and the window is phone-sized.
- **Mava workaround:** none yet; the author resizes the window by hand. Needed by Mava feature F05 (Studio shell).
- **Suggested fix:** take the title from `[app] name` (falling back to today's title for a bare `rux run file.rux`), and accept window settings in `rux.toml`, for example `[window] width`, `height`, `min_width`, `min_height`, `maximized`.
- **Status:** fixed 2026-10-12. The title is `[app] name` whenever a `rux.toml` is found, under `rux run` and in a build alike. A `[window]` table takes `width`, `height`, `min-width`, `min-height` and `maximized`, every one optional. The keys use dashes like the rest of the manifest, and the underscore spelling is refused by name. `--preview` still sizes the window as its device. A file with no manifest keeps `Rux · <file>` at 420 × 640. Documented under "The desktop window" in `site/content/tooling/build.md`. One behaviour change: `rux run` now reads the `rux.toml` above the file it runs, so a broken one stops `rux run` as it already stopped `rux build`.
