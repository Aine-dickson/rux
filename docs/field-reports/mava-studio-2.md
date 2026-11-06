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
- **Changed 2026-10-23 (breaks Mava's call sites):** `pickFile` gives a `File?` and `pickFiles` a `File[]`, the value an `<input type="file">` binds, so one way to pick a file works on every platform once the web's and Android's are built. **Where Mava used the result as a path, write `.path`** (`project.open(p)` becomes `project.open(p.path)`); `rux check` points at each place. A `File` also has `name`, `size`, `type`, `url` (what `<image :src>` shows, for an imported image) and `await file.text()`. `pickSaveFile` still gives a path.

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

## MS-008 A script function takes over a native resource method of the same name

- **Found:** 2026-10-14, moving Mava's Studio to `build/v0.9` at `f2bde76`.
- **Mava record:** L-009.
- **Limit:** a document with a script `fn save()` and a native resource `studio` whose methods include `save` cannot call `studio.save()`. `rux check` takes the method call for the script function and refuses it: `a <script> function calls save with 1 argument, and save takes 0 arguments, so the call fails the moment it runs`. A store's `box.save()` and a built-in method such as `parts.join("-")` beside a script `fn join()` are not affected, so it is specific to methods on a native value. An app's own actions and its resource's methods are naturally the same words (save, open, close, undo), so this will recur in every app with a native resource.
- **Reproduction:** in Mava's `studio/app.rux`, rename the script's `saveProject` to `save` and its callers to match, then `rux check studio`. Whether the call also fails in the window was not driven.
- **Mava workaround:** the script functions are named `saveProject` and `saveProjectAs`.
- **Suggested fix:** resolve `value.name(...)` on a native value to the value's method before any script function, in the checker and the runtime alike; if a script function of that name is meant, it is called as `name(value, ...)`.
- **Status:** fixed 2026-10-14 on `build/v0.9`, as suggested. The type checker and the runtime already took the native method; the refusal came from `rux check`'s name-only arity check, which took `studio.save()` for a call to the script's `fn save()` (and counted the receiver as an argument). That check now leaves a method call alone when its receiver is a native value: a global by what it holds, anything else when some native type has a method of that name, which the type checker then holds to its Rust signature. Driven in a window: the native-shop example with a script `fn total()` beside `till.total()`, both answering their own. A second case was found at run time and fixed the same day: where the receiver's type is only known when it runs (a handler's own expressions), a script `fn` whose parameters fit the receiver plus the arguments was tried before the native method, so `studio.save(path)` beside `fn save(s, p)` called the script. The native method is now tried first there too. `saveProject` can go back to `save`. Documented in `docs/07-script.md` beside `#[rux::resource]`.

## MS-009 `<icon>` exists only on an unmerged branch

- **Found:** 2026-11-06, Mava F06 (learning navigator), `build/v0.9` at `f2bde76`.
- **Mava record:** L-013.
- **Limit:** `<icon name="..." size="1em" />` (Tabler, outline and filled, only the icons an app names are built in) is on `origin/build/v0.9-icons` (`8826335`, `395fe70`, `5585800`, `e524d8b`, `78431e1`, 2026-09-20), which is 7 commits ahead of and 365 behind `origin/build/v0.9`. `build/v0.9` has no `<icon>` and `docs/06-roadmap.md` still lists icons as decided and unscheduled. An application cannot use icons on the line Mava Studio 2 is pinned to.
- **Reproduction:** `git grep 'name: "icon"' origin/build/v0.9 -- crates/rux-cli/src/vocab.rs` gives no match; the same on `origin/build/v0.9-icons` gives one.
- **Mava workaround:** none. Text labels stand in until the pin carries `<icon>`; icons are not drawn by hand from `<path>`.
- **Suggested fix:** merge or rebase `build/v0.9-icons` into `build/v0.9` and move the roadmap entry from unscheduled to done.
- **Status:** fixed 2026-11-06, as suggested. `build/v0.9-icons` is merged into `build/v0.9` (`20939f2`); `<icon>`, `rux-icons` and the build's icon tree-shaking are in, and the roadmap lists icons as built. Mava's pin needs to move to a commit at or after `20939f2`.

## MS-010 No drag and drop between containers

- **Found:** 2026-11-06, Mava F06 (learning navigator), `build/v0.9` at `f2bde76`.
- **Mava record:** L-014.
- **Limit:** reordering and re-parenting rows of a tree by dragging needs: a drag started from a handle on a row; a copy of the row that follows the pointer; a drop target that knows which container and slot the pointer is over; the other rows moving apart as it travels; one end event with the source container, target container, old index and new index. Rux has `@drag` (phase, totals, deltas) and layout rectangles readable from script, but no drop target, no cross-container drag and no drag copy. The behaviour wanted is SortableJS as used in the ilovestem outline (`ils/src/components/lesson/SidebarOutline.vue`): handle-only drag, a ghost left in the slot, a floating copy with a shadow, 150 ms sibling animation, a shared group so items pull from one list and put into another, an empty list accepting a drop.
- **Reproduction:** none; there is no API.
- **Mava workaround:** none. Move up, move down, indent and outdent commands until Rux has it; no drag built from `@drag` and hit-testing.
- **Suggested fix:** a drag-source and drop-container pair of attributes with a shared group name, an event at the end carrying source, target, old index and new index, a drag copy owned by the shell, and sibling reflow through the existing `transition` on `transform`. The shape of SortableJS's `onEnd` is a good model.
- **Status:** fixed 2026-11-06 on `build/v0.9`, as primitives rather than the suggested effects: Rux answers which element a press picks up, which target the pointer is over and which slot it is at, and the author writes the rest. `drag-data`, `drag-group` and `drag-handle` make a source (a handle-only pickup, and a long press on a finger without one); `drop-group` and `@dragenter`, `@dragover`, `@dragleave` and `@drop` make a target; `@dragstart`, `@dragmove` and `@dragend` run on the source; all seven hand over a `DropEvent` with `data`, `index` (counted without the dragged item), `before` and `dropped`. `:dragging` and `:drag-over` style the row and the target. Escape lets go. There is no drag copy, gap or auto-scroll from Rux: the copy is a `position: fixed` box the app places from `@dragmove`, and a keyed row with `transition: transform` slides aside when the list changes (built for this, and general). Nested targets work (the deepest accepting one wins, never one inside the dragged source), and an empty list takes a drop. Documented under "Drag and drop" in `docs/07-script.md` and driven in `examples/sortable.rux` on Windows (see `docs/08-user-tests.md`); touch, the web, macOS, Linux and Android are not. Not built: `scrollBy` for scrolling a list from `@dragmove` near its edge.

## MS-011 No right-click and no double-click event

- **Found:** 2026-11-06, Mava F06 (learning navigator), `build/v0.9` at `0e43a16`.
- **Mava record:** L-015.
- **Limit:** a desktop app cannot tell which pointer button went down, and cannot hear a double-click or double-tap. `@press` carries `x`, `y`, `pageX`, `pageY`, `width`, `height` and `touches`; `@tap` and `@longpress` carry no button either. A context menu on a row (right-click) and "double-click to open or rename" cannot be written. Anchored popups exist (`position: fixed`, `anchor-name`, `position-anchor`), so only the trigger is missing.
- **Reproduction:** none; there is no API.
- **Mava workaround:** none for the trigger. The menu opens from a button and every operation has a key.
- **Suggested fix:** `event.button` on `@press` and `@release` (0 primary, 1 middle, 2 secondary), an `@contextmenu` that also fires on a long press on touch, and `@dbltap` (or `event.count` on `@tap`) with the platform's double-click interval. `@contextmenu` should be able to take the event (`false`) so the platform's own menu does not show.
- **Status:** fixed 2026-11-06 on `build/v0.9`, with two changes from the suggestion. The double-tap is `@doubletap` (named like `@longpress`), and there is no `false` answer on `@contextmenu`: a desktop window has no platform menu to hide, and declaring the event is enough (the web has not been driven yet, and is the one place it could matter). `event.button` (0 primary, 1 middle, 2 secondary) is on every pointer event, and the middle and secondary buttons fire `@press` and `@release` only, so `@tap`, `@drag`, `@swipe` and `@longpress` stay the primary button's. `@contextmenu` fires on a secondary click (on the release on Windows, the press elsewhere), on a touch press held for half a second after `@longpress`, and on the Menu key or Shift+F10 with focus on the element, where it reports the middle of that element. Each of `@contextmenu` and `@doubletap` goes to the nearest element that wrote it, and takes nothing from a `@press` or `@drag` around it. Documented under "Buttons, menus and double taps" in `docs/07-script.md`. Driven on Windows; the touch long press, the web and macOS and Linux are not (see `docs/08-user-tests.md`).

## MS-012 No custom title bar

- **Found:** 2026-11-06, Mava F06 (chrome), `build/v0.9` at `0e43a16`.
- **Mava record:** L-016.
- **Limit:** an editor-style window (menus, a search field and the window buttons in one title row, an activity rail below) cannot be written. `[window]` has no option to drop the native decorations; script cannot minimize, maximize, restore or toggle the window; there is no drag region and a borderless window has no resize edges or snap behaviour; `env(titlebar-area-*)` is not supported.
- **Reproduction:** none; there is no API.
- **Mava workaround:** none for the title bar. The menu bar sits under the native one.
- **Suggested fix:** `[window] decorations = false` (or `titlebar = "custom"`), `window.minimize()`, `window.toggleMaximize()` and `window.close()` from script, an `app-region: drag` style (or `@windowdrag`) for the title row, system resize edges and snap kept for the borderless window, and `env(titlebar-area-*)` for the space the platform buttons would take. The window buttons themselves can stay the app's own boxes.
- **Status:** fixed 2026-11-06 on `build/v0.9`, as suggested except for `env(titlebar-area-*)` and `window.close()`. `[window] decorations = false` opens a window with no title bar or border. `app-region: drag` (inherited; `no-drag` and any box with a `@tap`, a gesture or a field are controls) makes a box move the window, and a second press on it maximizes or restores it. `window.minimize()`, `maximize()`, `restore()`, `toggleMaximize()` and `window.maximized` (a value that follows the window) need no import; closing stays `close()`, so there is one way to do it. winit gives an undecorated Windows window no resize border, so the shell draws a six pixel band round the edge with the right cursors. On Windows the system's own rounded corners and drop shadow are asked for. `env(titlebar-area-*)` is not built, since a frameless window keeps no platform buttons; it belongs to the other model, a native frame with the app's content extended into the title row (the way VS Code's title row sits beside the system's own buttons), which is not built and is noted in the roadmap. Documented under "The desktop window" in `docs/07-script.md` and in `site/content/tooling/build.md`. Driven on Windows (see `docs/08-user-tests.md`); snap, macOS, Linux, the web and Android are not.

## MS-013 A percent border radius cannot draw an ellipse

- **Found:** 2026-11-06, Mava F08 (page canvas), `build/v0.9` at `0e43a16`.
- **Mava record:** L-017.
- **Limit:** an ellipse node is a box with `border-radius: 50%`. Rux resolves a percent radius against the shorter side and draws one radius per corner, so a box of 200 by 100 is drawn as a pill, not an ellipse. The as-built document records this as a deliberate difference from CSS; it still leaves no way to draw an ellipse that is not a circle without drawing a `<path>`.
- **Reproduction:** `<view style="width: 200px; height: 100px; border-radius: 50%; background: #ccc"></view>`
- **Mava workaround:** none; the pill is accepted until Rux changes.
- **Suggested fix:** elliptical corner radii (`border-radius: 50% / 50%` resolving each axis against its own side), or a shape primitive (`<ellipse>`) that takes the box.
- **Status:** open.
