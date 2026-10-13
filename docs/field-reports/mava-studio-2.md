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

## MS-003 Native code cannot push a change to the Rux side

- **Found:** 2026-10-13, Mava feature F05 (Studio shell, core bridge), `build/v0.8` at `812f71e`.
- **Mava record:** L-003.
- **Limit:** a native module is call and return only. Rust can answer a call a document makes, but has no handle with which to set a signal, send an event or wake a handler later. Mava's domain announces every change as an event, and the Studio's panels are meant to follow those events. That works only while every change starts with a Rux call. It breaks for a background save that finishes or fails, a file changed on disk, and, from Mava F11, a Prismal simulation producing frames at frame rate, which has to reach the screen without being asked each frame.
- **Reproduction:** a `#[rux::resource]` whose state changes on another thread (or from an `async` export that keeps running) has no way to make the screen show the change until the document next calls in.
- **Mava workaround:** temporary. Every call that can change what the Studio shows returns a whole view record, and the document assigns its signals from it. Polling from `setInterval` was rejected as costly and laggy.
- **Suggested fix:** a channel from native code into a document, for example an exported `rux::Sender<T>` (or a callback value) that Rust holds and that delivers `T` to a handler, or writes a signal, on the UI thread. Mava needs it by its F11 at the latest.
- **Status:** fixed 2026-10-13 on `build/v0.9`, as a signal. `#[rux::export] pub static FRAME: rux::Signal<Frame> = rux::Signal::new();` is `frame` in its module, typed `Frame?`; Rust calls `FRAME.set(frame)` from any thread, the window wakes, and every binding and `effect` that read it updates on the UI thread. The latest value wins, so frames at frame rate are drawn once per paint with the newest. There is no ordered event queue yet: a signal holding a revision number or a status covers "something changed, read again", and a queue can come if Mava needs every event in order. Documented under "Signals from Rust" in `docs/07-script.md`.

## MS-004 No application keyboard shortcuts

- **Found:** 2026-10-13, Mava F05, same build.
- **Mava record:** L-004.
- **Limit:** keys reach only a focused text field, where the shell handles Ctrl+A, C, X and V itself (`edit_focused`, `text_shortcut` in `rux-shell`). A document cannot bind a key or chord to a handler, so a desktop app cannot give Save, Undo and Redo their platform shortcuts (Ctrl+S, Ctrl+Z, Ctrl+Y or Ctrl+Shift+Z, and Cmd on macOS). Ctrl+Z inside a text field is also not undo there.
- **Reproduction:** in the Studio, make a change, click outside the title field, press Ctrl+Z: nothing happens.
- **Mava workaround:** toolbar buttons. The keyboard half of Mava's F05 done line waits on this.
- **Suggested fix:** document-level key handlers, for example `@key` on `<screen>` (and on any element, firing for focus inside it) with `event.key`, `ctrl`, `shift`, `alt`, `meta` and a way to stop the shell's own handling; or a declared shortcut table. Text fields should keep their own editing keys unless a handler claims them.
- **Status:** fixed 2026-10-13 on `build/v0.9`, as suggested. `@key` on any element and on the `<screen>`, with `event.key`, `ctrl`, `shift`, `alt`, `meta`, `primary` (Ctrl, or Cmd on macOS) and `repeat`; answering `false` takes the key. A focused text field handles the keys it uses before the `@key`s around it, so Ctrl+Z in the title field is the field's own undo and outside it reaches the Studio's. Text fields now have undo and redo (Ctrl+Z, Ctrl+Y, Ctrl+Shift+Z), grouped as a browser groups typing. Documented under "Keys" in `docs/07-script.md`.

## MS-005 No file dialogs

- **Found:** 2026-10-13, Mava F05, same build.
- **Mava record:** L-005.
- **Limit:** there is no way to ask the person for a file to open or a place to save. Mava's Studio opens and saves `.mava` projects, and imports images from Mava F10.
- **Reproduction:** none; there is no API.
- **Mava workaround:** the app's native crate opens the system dialogs itself with the `rfd` crate. It works on Windows, blocks the UI thread while the dialog is open, and pulls GTK 3 into a Linux build.
- **Suggested fix:** built-in open and save dialogs (filters, a suggested name, multiple selection), awaited from script, with the platform's own picker on Android and the browser's on the web.
- **Status:** fixed 2026-10-13 on `build/v0.9` for the desktop. `await pickFile(options)`, `pickFiles(options)` and `pickSaveFile(options)`, with `title`, `filters`, `folder` and `name`, give paths (or `none`, `[]` when cancelled). The window keeps painting while the dialog is open, and Linux uses the desktop portal, so no GTK. The web and Android throw `unsupported` until their pickers are built, since neither gives a path. The `rfd` dependency in Mava's native crate can go. Documented under "File dialogs" in `docs/07-script.md`.

## MS-006 Closing the window cannot be intercepted

- **Found:** 2026-10-13, Mava F05, same build.
- **Mava record:** L-006.
- **Limit:** `WindowEvent::CloseRequested => event_loop.exit()` in `rux-shell`: the window closes and the process exits at once. An app cannot ask "save changes?", cannot refuse the close, and gets no hook to flush state (it is not clear that `unmounted` runs).
- **Reproduction:** start a project in the Studio without saving it, close the window: it is gone without a question.
- **Mava workaround:** projects with a file are saved after every change, so only a never-saved project can be lost.
- **Suggested fix:** a close request a document can answer, for example `@closerequest` on `<screen>` with `event.preventDefault()` (or an awaited handler returning whether to close), and `unmounted` guaranteed to run on a normal close.
- **Status:** fixed 2026-10-13 on `build/v0.9`, desktop and Android. `@close` on the `<screen>` answers as a route guard does (`false` keeps the app open); `close()` closes without asking again; a save that waits is `@close` answering `false`, then `await` the save, then `close()`. `unmounted` now runs on every close, every component's first, then the document's: it never ran before, on a close or a hot reload, which answers the "not clear that `unmounted` runs". The web does not ask `@close` yet and says so in the console. Documented under "Closing the app" in `docs/07-script.md`.

## MS-007 No storage for an application's own settings

- **Found:** 2026-10-13, Mava F05, same build.
- **Mava record:** L-007.
- **Limit:** a desktop app has no Rux-provided place for small per-user state (Mava's recent projects list). The web build has session storage; the desktop shell has nothing.
- **Reproduction:** none; there is no API.
- **Mava workaround:** a file in the user's data folder, located with the `dirs` crate from the native crate.
- **Suggested fix:** a small key-value store per app id (`[app] id` in `rux.toml`), the same API on every target. Low priority for the Studio, higher for Mava's learner runtime, which may run on the web.
- **Status:** fixed 2026-10-13 on `build/v0.9`, as suggested. `settings.get(key)`, `settings.set(key, value)` and `settings.remove(key)`, with nothing to import, kept per `[app] id` in the platform's app-data folder (`%APPDATA%\<id>\settings.json` on Windows), the app's files on Android and `localStorage` on the web, each `set` written at once and safely. The `dirs`-based file in Mava's native crate can go. Documented under "Settings" in `docs/07-script.md`.
