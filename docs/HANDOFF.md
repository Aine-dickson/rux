# Handoff

Read this first. Keep it short; history goes to an archive, not here.
Last updated 2026-11-06.

## Objective

Ship v0.9 from `build/v0.9`. The line is nearly all built (see the v0.9 section
of [the roadmap](./06-roadmap.md)); what is left is listed there.

## State

- Branch `build/v0.9`, **local only, not pushed** (the owner pushes; a push runs CI).
- 2026-11-06: `build/v0.9-icons` was found never to have been merged, and is now
  in the tip (merge commit `20939f2`). `<icon>`, `rux-icons` and the build's
  icon tree-shaking are in. Two fixes were needed to merge it: `<icon>` had no
  entry in the parser's attribute tables, and its block inside
  `build_node_inner` overflowed the debug main-thread stack, so it is now
  `build_icon` in `rux-style`. All 138 test suites pass on the merged tree
  (`cargo test --workspace -j 2`; at full parallelism the machine runs out of
  memory compiling test files).
- Deleted locally on 2026-11-06 as superseded: `wip/g4-kept-tree` (landed as
  `812f71e`) and `site/launch` (the site sync has moved far past it). The
  remote `origin/site/launch` was left alone. `spike/c0-frames` is kept on
  purpose: a measuring harness, results recorded in `docs/13`.
- `build/v0.9-icons` and its worktree `../rux-icons` are fully merged and can go.
- Docs corrected to the code on 2026-11-06 (every doc was checked against the
  crates and `rux check` probes, about 350 commits of changes since v0.7.1
  considered). Rewritten: `05-as-built`, `07-script`, `10-types`, `11-next`
  (2,900 to 1,000 lines), `04-architecture`, and `06-roadmap` (3,300 to 211
  lines: open work and the decisions that still bind only). Removed text lives in
  `docs/archive/` (one file per doc; `roadmap-history.md` holds the whole old
  roadmap). Not reviewed in this pass: `02-spec`, `03-guide` (both known to be
  history, still published to the site), `08-user-tests` (a log), `09-author-notes`
  (19 open rows, statuses not re-checked), field reports.
- The full audit record (method, what was left unverified per doc, questions
  for the owner) is `docs/archive/docs-audit-2026-11-06.md`. Read it before
  auditing the docs again.
- The owner will decide the docs and site redesign. Open questions for it:
  `07-script`, `10-types` and `11-next` overlap heavily and could become one
  document; much of the CSS section of `05-as-built` is not CSS (r-transition,
  computed/effect, keyed lists, Tailwind, `env()`); `03-guide` is false as
  written; `02-spec` is design history.

## Decisions not to revisit

- `native/` is on by default for `rux new`, beside `rux.toml`; locks are per
  target (`locks/<target>.lock`); the Rux app stays at the project root.
- Superseded branches above stay deleted; do not resurrect them.

## Known issues and open work

- Still open in v0.9: physics (springs), live regions, pinch and rotate, the
  `rux doctor` device survey (those two need a phone), `hyphens: auto`, video on
  macOS and Linux.
- The 0.9 publish must include `rux-native`, `rux-native-macros`, `rux-bindgen`
  (and `rux-icons`, already in `scripts/release.sh`'s first layer); crates.io has
  `ruxlang` only up to 0.7.1, so a scaffolded `native/` crate cannot build for
  anyone outside a checkout until then.
- `main` has diverged from `build/v0.9`: it holds the v0.7.0 and v0.7.1 release
  commits, which `build/v0.9` does not descend from. A release-process question.
- `cargo clippy -D warnings` fails in `rux-syntax` (four needless borrows); not
  touched yet.
- Mava Studio 2 (`docs/field-reports/mava-studio-2.md`): MS-009, MS-011 and
  MS-012 are fixed on this branch (2026-11-06, commits `66b4536` and
  `ee2eeec`). **MS-010 (drag and drop between containers) is the one open
  entry**: it needs an Opus plan first, since its design sets how drag works
  in general. Mava's pin must move to a commit at or after `20939f2` for
  `<icon>`.
- 2026-11-06 additions: `@contextmenu`, `@doubletap` and `event.button`
  (MS-011); `[window] decorations = false`, `app-region`, and the `window`
  module (MS-012). Driven on Windows only: touch long press, the web, macOS,
  Linux, Android and Windows snap are not (rows in `docs/08-user-tests.md`).
  Probes and drivers: `C:\Users\USER\Mobile\rux-harness\ms011` and `ms012`.
- **Title row beside the system's buttons (roadmap item 8): built 2026-11-06.**
  `[window] title-bar = "overlay"` (owner's syntax) keeps the native frame and
  lets the page into the title row; `env(titlebar-area-x|y|width|height)` is the
  free part of the row (undefined without an overlay, so fallbacks apply). On
  Windows a subclass on winit's window procedure (`rux-shell/src/caption.rs`)
  takes the caption's height back in `WM_NCCALCSIZE` and answers `WM_NCHITTEST`
  (top edge, the three buttons; `HTMAXBUTTON` gives the Windows 11 snap flyout),
  and the shell draws the buttons itself at a fixed 32 px, with the ink picked
  from the box behind them. macOS uses winit's full-size content view (written,
  not run). Driven on Windows 11 at 150% only; the rows and the gaps are in
  `docs/08-user-tests.md`. Known gaps: the drawn buttons have no accessibility
  nodes; `min-width`/`min-height` count the caption's height on Windows.
  Probe and driver: `C:\Users\USER\Mobile\rux-harness\titlebar-overlay`.
- **Child windows (roadmap item 8): designed and approved 2026-11-06, steps 1
  to 3 built (`db718e2`, then the Shell refactor).** Plan: `C:\Users\USER\.claude\plans\optimized-shimmying-aho.md`
  (copy to `docs/archive/child-windows-plan.md` when done). Owner's choices:
  `window.open(path, options)` opens a `.rux` file as its own document; stores
  are process-wide; the opener passes `props`; `close()` in a child closes that
  child; the handle has `close()`, `focus()`, `isOpen`, and `onClose` in the
  options; owned by default (`owned: false` for a taskbar button); `window.*`
  is the window the code runs in; `window.canOpen` is false on Android and web.
  - **Done**: (1) store hub `crates/rux-script/src/shared.rs` holds `V` values
    (not `Value`: ints and records survive); `Engine::join_stores` /
    `adopt_stores`, pulled by `Document::refresh_native_signals`; a hot reload
    calls `reset_shared_stores()`. (2) `Document::load_window(path, props)`;
    props become `let`s on the script's first line.
  - **Done (3)**: `Shell` in `rux-shell/src/lib.rs` owns `HashMap<WindowId, App>`,
    `main: Option<WindowId>` and `parked: Option<App>` (the main window's state
    while it has no window: before `resumed`, Android suspended, the web before
    `SurfaceReady`; `Shell::seat` re-files it under its window's id). `App` is
    one window's state; `context`, `text` and `images` are `Shared<T>` =
    `Rc<RefCell<T>>` (render holds the guards for the frame). `App::tick`
    replaces `App::about_to_wait` and returns its deadline; the Shell takes the
    earliest. Routing: `window_event` by id; `Answered`, `VideoFrame` and
    `Reload` go to every window; `PickedFiles` goes to the window with a
    `pending_file`, else main; `Access` by its `window_id`; the Android and web
    events go to main. `Shell::after_event` (end of every handler) runs
    `refresh_native_signals` on every window and redraws the ones that moved.
    Only the main window exists yet, so nothing observable changed.
  - **Next**: (4) `window.open`, the handle, per-window `window` module
    (`window_module.rs` has a global `WINDOW` static and `MAXIMIZED` signal;
    `caption.rs` also keeps a thread-local `WINDOW`, and `window_event` calls
    `caption::set_active` / `set_metrics` and `window_module::follow`, all
    process-wide: make them per window), and the `rux check` of literal
    `window.open` paths/props. To add a child: build its `App`, assign it the
    main's `context`/`text`/`images` Rc clones, insert it in `Shell::windows`
    under its window id (no helper exists yet). Check in 4: `settle_tasks` per
    document under a broadcast `Answered` (each engine collects answers from
    the host queue; make sure a task answer reaches only its owner). (5)
    closing: `close_now` still calls `event_loop.exit()`, right only for main.
    (6) docs.
  - **Decided (owner, 2026-11-06)**: when the main window closes, every child
    closes with it and its context is destroyed as the main's is: components'
    `unmounted` deepest first, then the document's; children are not asked
    their own `@close`.
  - **Check first in 4**: a script `fn` value must be passable to a native call
    and run later in the opener's scope (`onClose`); if not, stop and ask.
  - **Known**: `what_the_scaffold_writes_checks_clean` overflows the stack on
    unmodified `HEAD` too (watchlist #41): 6 of 8 runs alone on 2026-11-06,
    against 5 of 6 with step 3; not new. A whole-workspace run with
    `--no-fail-fast` passed all 138 suites. The wasm32 build already fails at
    this commit in code step 3 did not touch (`window_settings` in
    `overlay_active`, and `slides` taking `std::time::Instant`); the Shell's
    wasm and Android paths are written but not compiled (no NDK clang here).
    Init side effects in a store file still run once per window.
- `Style` has a size budget (744 bytes, watchlist #41): a new rarely used
  style property goes in `Rare`, as `app_region` does. One byte over it
  overflowed `rux check` on the tasks scaffold in a debug build.
- Android and web builds of a scaffold with `native/` have not been driven.
- Code defects found by the docs pass (none fixed yet):
  - `throw "x"` is caught as the string, not as an `Error`; `do`, `break value`
    and the bitwise operators are silently inert in a `<script>` body; `trunc`
    of NaN does not throw; an `any` is not checked where it enters a typed name.
  - `[1,2].for_each(...)` passes `rux check` but the interpreter has no
    `for_each`. Rhai-era names (`len`, `retain`, `drain`, `dedup`, `parse_int`,
    snake_case string methods) still run and are undocumented: document or remove.
  - Stale text in code: `vocab.rs` says a component declaring `mounted` is
    warned (it runs per instance); the hover text for `border`, `outline` and
    `position` is out of date (that text feeds the generated css-properties page);
    the hover text for `trim` mentions rhai; `rux-ir/src/types.rs` says `Set` and
    non-string `Map` keys "come with Rux's own interpreter"; `lexer.rs` and many
    `crates/*/src/lib.rs` module docs still cite the fork or "milestones M2 to M9".
  - `rux check` warns that `font-size: calc(100% + 2px)` is ignored while a unit
    test says it resolves; `transition: opacity 1s linear(0,1)` is reported
    badly (calls `opacity` a function); `rux new --help` is rejected.
  - Two doc statements rest on one grep only: slider arrow keys, a drawn desktop
    date picker and dropping a file on the input are listed as not built.

## MS-010 in progress (2026-11-06)

Plan approved by the owner: `C:\Users\USER\.claude\plans\twinkly-floating-pebble.md`
(copied to `docs/archive/ms010-drag-drop-plan.md`). **Owner rule: Rux ships primitives, not
effects.** So there is no shell-drawn drag copy and no auto-scroll feature: the
author draws the copy from `@dragmove` (every pointer move) and scrolls with a
`scrollBy` call (not built; script has `scrollIntoView` only).

- **Step 1 done and committed** (`7c3edc3`, docs `22b68fb`): a keyed `r-for` row
  whose `transition` names `transform` slides when it moves
  (`LayoutState::set_nudges`, `crates/rux-shell/src/slides.rs`). Driven on
  Windows (`rux-harness\ms010`).
- **MS-010 is built and committed** (steps 1 and 2, `7c3edc3`, `d5c141e`; then
  `scrollBy`). Rux ships primitives only: `drag-data`, `drag-group`,
  `drag-handle`, `drop-group`, seven events with one `DropEvent`, `:dragging`,
  `:drag-over`, `el.scrollBy(dx, dy)`, and keyed rows that slide
  (`transition: transform`). Geometry is pure and tested in
  `crates/rux-shell/src/dnd.rs`; press and session state is `dnd_session.rs`.
  Driven on Windows only (`rux-harness\ms010`, `ms010-dnd`, `ms010-scroll`);
  not driven: touch (handle pickup, long press), a box over a source, an empty
  target, auto-scroll from `@dragmove`, web, macOS, Linux, Android. Spots are
  read from the Node tree and boxes from the last frame's metrics
  (untransformed). A target is any element with `drop-group` or an
  enter/over/leave/drop handler (the plan said `@drop` required).
- **`rux check` on the tasks scaffold is one frame from a stack overflow in a
  debug build** (watchlist #41, `what_the_scaffold_writes_checks_clean`). It
  tipped twice in MS-010: a field on `Node` (moved into `Style::rare`) and a
  match arm in the interpreter's `method` (made a bare call to an `inline(never)`
  fn). Also the recursive style builders. A debug build gives every match arm
  its own locals. It passes alone and can fail under `--workspace`, so run
  `cargo test --workspace --test new` after touching any of those. Worth
  deciding: run `rux check` on a thread with a larger stack.
- Python heredocs lose backslashes: use Edit for docs with Windows paths.

## Next action

One item left, in its own session on Opus plan mode (it sets design):

1. **Child windows**, step 4 (`window.open`) onward; see the
   child windows entry above. The syntax is settled.
2. ~~MS-013~~ done 2026-11-06 (`3a2c96c`): percent radii per axis, `a / b`,
   two-value longhands; drawn by an elliptical path in `rux-paint`
   (`rounded_rect` takes `radius_y`). Driven in no window yet; tests only.
   Gaps: outer `box-shadow` on an elliptical corner uses the shorter radius;
   `transition: border-radius` animates horizontal radii only. Boxes that
   relied on the old pill must use `9999px`.

After those, decide with the owner when to push `build/v0.9` and cut v0.9.
MS-012's status line in the Mava report still says the overlay is not built.

## Verified on 2026-11-06

Step 3: `cargo test --workspace -j 2 --no-fail-fast` 138 suites, 0 failed (the
scaffold flake hit once in a run without `--no-fail-fast`). Probes driven on
Windows with the new build, same rects and screenshots as before:
`ms012`, `titlebar-overlay`, `ms010-dnd` (shots in the scratchpad, not the
harness folders).

Trial merge built and tested in a scratch worktree; `rux new` + `rux check` on
the `tasks` template passes after the stack fix; the roadmap was checked against
`git log` since the v0.8 freeze. An `<icon>` has not been driven in a window
since the merge.
