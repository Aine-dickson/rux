# Child windows (roadmap v0.9 item 8)

## Context

The roadmap's last open v0.9 design item is letting an app spawn a second desktop window from script. The owner settled the syntax on 2026-11-06:

1. **`window.open(path, options)`** opens another `.rux` file as its **own document** in its own OS window.
2. **Stores are process-wide.** `use stores::project;` in two windows is the same state.
3. **Starting data comes as props.** The opened file declares `prop id;` as a component does; the opener passes `{ props: { id: 3 } }`.
4. **`close()` in a child closes that child.** `@close` on the child's `<screen>` guards its own close button. `close()` in the main window closes the app and every child.
5. **The opener gets a handle:** `w.close()` (asks the child's `@close`), `w.focus()`, `w.isOpen` (no `send`: the shared stores carry state), and `{ onClose: fn }` in the options. Opening a path twice gives two windows.
6. **Owned by default:** the child stays above the main window, minimizes with it and has no taskbar button. `{ owned: false }` gives a top-level window with its own taskbar button.
7. **`window.*` means the window the code runs in.** In a child document, `window.minimize()` and `window.maximized` are the child's, so a custom title bar works in any window.
8. **Android and web:** `window.open` throws `unsupported`, and `window.canOpen` is `false` there (`true` on desktop). The author picks the fallback; Rux ships none (primitives, not effects).

```rux
<script>
  use stores::project;
  let inspector = signal(none);
  fn inspect() {
    if inspector == none || !inspector.isOpen {
      inspector = window.open("windows/inspector.rux",
        { title: "Inspector", width: 360, height: 600, props: { id: project.selected },
          onClose: fn() { inspector = none; } });
    } else { inspector.focus(); }
  }
</script>
```

Options: `title`, `width`, `height`, `min-width`/`minWidth` (spelled as the script spells keys; settle in step 4 to match `[window]`), `resizable`, `decorations`, `titleBar` (`"overlay"`), `maximized`, `owned`, `props`, `onClose`.

## What exists, and what this reuses

- `crates/rux-shell/src/lib.rs`: `App` (line ~2025) holds one window's state: `state: Option<RenderState>`, the `document`, and about 100 per-window fields. `ApplicationHandler for App` at ~8580 (`resumed` creates the one window at ~8651; `window_event` ignores `WindowId`).
- `crates/rux-shell/src/window_module.rs`: the `window` module, a process-wide `WINDOW` static and a `MAXIMIZED` native signal. Both must become per window.
- Native signals are the model for shared stores: `rux_native::signal_epoch()`, `Engine::provide_native_signals` (`crates/rux-script/src/lib.rs` ~2277) and `Document::refresh_native_signals` (`crates/rux-runtime/src/lib.rs` ~6232), which calls `apply_change(&moved)`.
- Stores: `crates/rux-runtime/src/modules.rs` ("loaded once per document"); a store signal's linked name is `stores/cart::items` (`engine.signal_value("stores/probe::x")` in tests ~8442). `Engine::task_is_a_modules` already tells a module's names apart.
- Component props: `prop label;` / `prop value = 0;` and the tag checks (`docs/05-as-built.md` ~3198). Today "a file that declares a prop is a component"; a `<screen>` root that declares props becomes a **window document**.
- `rux build` walks the whole project (`crates/rux-cli/src/build.rs` `collect`), so child files are already bundled.
- `[window]` settings: `crates/rux-cli/src/manifest.rs` `Window` and `window_settings()`; `rux_shell::WindowSettings`; `caption.rs` for the overlay title row.

## Steps (each its own commit; tests green before the next)

### 1. Process-wide stores (runtime only, no windows yet)
- A thread-local `StoreHub` in `rux-runtime`: linked name to `Value`, plus an epoch. The UI thread is the only thread that runs scripts, so a thread-local is enough.
- Push: wherever a document applies a change set (`apply_change` and its callers after handlers, tasks, timers, effects), every changed name that is a module signal is written to the hub and the epoch moves.
- Pull: `Document::refresh_store_signals()` beside `refresh_native_signals`, called from the same places (the shell's `render` start). It sets moved globals in the engine and runs `apply_change`.
- Load: a document whose module is already in the hub runs the module's init (functions and computeds need it) and then adopts the hub's values. Document the rule: **a store starts once per app; the second window sees the state as it is**. Init side effects (a `settings.get`) still run per document; say so in docs.
- Hot reload of a store file clears that module's hub entries.
- Tests in `rux-runtime`: two `Document`s over one project with `stores/probe.rux`. A write in A shows in B after refresh, B's binding patches, a computed in B re-reads, a write in B does not echo back into A as a change, and a list mutated in place propagates.
- **Escalate** if a store value cannot round-trip `interp::V` to `Value` and back without losing identity that script relies on (resources, closures in a store).

### 2. Window documents with props
- `Document::load_window(path, root, props: Vec<(String, Value)>)`: it resolves `components/` and `stores/` against the **main document's project root**, not the child file's folder, and puts the props in scope as a component instance's props are.
- Check rules: a `<screen>` root with `prop` declarations is a window document. Checked alone, its props are owed by the opener (as a component's are).
- `rux check` on the opener: `window.open` with a string literal path checks that the file exists, has a `<screen>` root, and that `props` names and types match its declarations (reuse the component tag prop check). A non-literal path warns that it cannot be checked.
- Tests: load with props, a missing required prop errors, a wrong type errors, unknown prop errors, root-relative store resolution.

### 3. The shell holds several windows (refactor, no new behaviour)
Keep `App` as **one window's** state, so the 14,800-line file barely changes. Add a `Shell` that owns `HashMap<WindowId, App>` plus the main id, and implements `ApplicationHandler<RuxEvent>`, forwarding by `WindowId`.
- Shared between windows: the `RenderContext` (one wgpu device; `Rc<RefCell<..>>`), the text engine (fonts once) and the image cache. Put each behind `Rc<RefCell<>>`, and borrow at the few use sites (`render`'s split borrow). If sharing the text engine fights the split borrows, keep it per window and note it.
- `RuxEvent`: variants that belong to one window carry its `WindowId`. accesskit events already carry one, and the shell routes picked files to the window that asked. The rest (native answers, file watcher, timers) go to every window.
- `about_to_wait` / deadlines: the shell takes the earliest deadline over all windows.
- Watchlist #41: no new fields on `Node`, and new interpreter arms are bare calls to `#[inline(never)]` fns. Run `cargo test --workspace --test new` after this step.
- Acceptance: the whole workspace's tests pass, and the existing probes drive exactly as before (`rux-harness\titlebar-overlay`, `ms010-dnd`, `ms012`).

### 4. `window.open`, the handle, and per-window `window`
- `window_module.rs`: replace the `WINDOW` static with a thread-local **current window id** that the shell sets around every call into an `App` (events, render, timers, task answers). `minimize`, `maximize`, `restore` and `toggleMaximize` act on the current window. `maximized` becomes per document: the shell provides each `App`'s own value (as the router provides `route`) rather than through one global signal.
- `window.open(path, options)` queues an open request and returns a native resource handle (`isOpen` getter, `close()`, `focus()`). The shell drains the queue after each event: it loads the document (step 2) from the same source the main document came from (disk, or the bundle), builds the winit window from the options with the same code that turns `[window]` into attributes, and inserts an `App`.
- Owned: on Windows, `with_owner_window(main HWND)` and `with_skip_taskbar(true)`; on macOS, the parent window relation; on Linux X11, transient-for. Write the macOS and Linux paths, and mark them not driven.
- `onClose`: check first that a script `fn` value can be handed to a native call and run later in the opener's scope (with the instance that called `open`). **If it cannot, stop and escalate**: the fallback is `isOpen` read reactively, which changes the owner's approved API.
- `window.canOpen`: `true` on desktop. On Android and the web, `open` throws `unsupported` and `canOpen` is `false`.
- Two opens of one path are two windows (no key).

### 5. Closing
- Child close button or `w.close()`: run the child's `@close` (`false` keeps it). Otherwise run its `unmounted`, drop its `App`, mark the handle closed, and run the opener's `onClose`.
- `close()` in a child closes that child without asking (as `close()` does now for the app).
- The main window closing (button, Alt+F4, `close()`): the main `@close` decides for the app. On close, every child's `unmounted` runs, then the main document's, then exit. Children are not asked their own `@close`. **Owner confirmed 2026-11-06**: children follow the main window, and each child's context is destroyed as the main's is (its components' `unmounted` deepest first, then its document's). State it in the docs.
- Hot reload: a child reloads on its own file. Its store edits reach every window through step 1's hub.

### 6. Docs and records
- `docs/05-as-built.md`, "The desktop window": `window.open`, the options table, the handle, shared stores, closing rules, platforms.
- `docs/07-script.md`: the window table gains `open`, `canOpen`, the handle; the store rule ("starts once per app").
- `docs/06-roadmap.md` item 8 moves to built. Add `docs/08-user-tests.md` rows, `docs/09-author-notes.md` rows (the two traps: init side effects run per window, and opening twice gives two windows), then `docs/HANDOFF.md`. Run `./site/sync-docs.sh` from Git Bash.
- Voice rules: no "I", no em dashes, messages say what to write.

## Verification

- Per step: `cargo test -p rux-runtime`, `-p rux-script`, `-p rux-shell`, `-p rux-cli`. Then `cargo test --workspace -j 2` and `cargo test --workspace --test new` (#41) before each commit.
- Driven on Windows 11 with a probe at `C:\Users\USER\Mobile\rux-harness\child-windows`, using `tools/drive.ps1` (click, keys, shot):
  1. Open the inspector; a counter in a store shows the same number in both windows, and incrementing in either updates the other.
  2. Props reach the child (`id` shown).
  3. An owned child stays above the main window, has no taskbar button and minimizes with it. `owned: false` gets a taskbar button.
  4. A child with `decorations = false` and its own title bar: `window.minimize()` and `toggleMaximize()` act on the child, and `window.maximized` follows the child.
  5. Child close button with `@close` returning `false` stays open. Otherwise it closes, `onClose` runs, and `isOpen` is `false`.
  6. Closing the main window closes the child, and both `unmounted` run (logged to a file).
  7. `rux check` errors on a missing file, a missing prop and a wrong prop type.
- Not driven, and recorded as such: macOS, Linux, web and Android (`canOpen` false there).
- Not pushed; commit locally with no co-author trailer.
