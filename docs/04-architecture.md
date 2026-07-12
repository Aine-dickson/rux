# 04 — Architecture

How the Rux runtime turns a `.rux` file into pixels and keeps them live. The [spec](./02-spec.md) defines *what the language is*; this defines *what we build*.
Everything here is downstream of the [rationale](./01-rationale.md) — especially [Law 4](./01-rationale.md#law-4--stay-close-to-rust-dont-pop-the-balloon): **reuse mature Rust crates; write only the glue that is uniquely ours.**

> **Status:** v0.1 architecture proposal. Concrete crate choices are recommendations, not commitments — the milestone plan is designed so each can be swapped without redesign.

## Contents

- [The pipeline at a glance](#the-pipeline-at-a-glance)
- [What we reuse vs. what we build](#what-we-reuse-vs-what-we-build)
- [Stage 1 — Parse](#stage-1--parse)
- [Stage 2 — Cascade](#stage-2--cascade)
- [Stage 3 — Reactive graph](#stage-3--reactive-graph)
- [Stage 4 — Layout](#stage-4--layout)
- [Stage 5 — Paint & present](#stage-5--paint--present)
- [Input & event routing](#input--event-routing)
- [The script tier & host bridge](#the-script-tier--host-bridge)
- [Hot-reload](#hot-reload)
- [Crate layout](#crate-layout)
- [Milestone plan](#milestone-plan)
- [Open questions](#open-questions)

---

## The pipeline at a glance

One frame's worth of data flow, source file to screen:

```
                    ┌──────────────────────── hot-reload ────────────────────────┐
                    │                                                             │
   .rux file        ▼                                                             │
  ┌─────────┐   ┌─────────┐   ┌──────────┐   ┌──────────┐   ┌──────────┐   ┌──────┴─────┐
  │ template│──►│  PARSE  │──►│ CASCADE  │──►│ REACTIVE │──►│  LAYOUT  │──►│  PAINT +   │
  │  style  │   │ 3 srcs  │   │ CSS→style│   │  GRAPH   │   │  taffy   │   │  PRESENT   │
  │  script │   │ →1 doc  │   │ per node │   │ signals  │   │ boxes    │   │ vello/wgpu │
  └─────────┘   └─────────┘   └──────────┘   └────┬─────┘   └──────────┘   └──────┬─────┘
                                                  │                               │
                                    signal change─┘        winit window ◄─────────┘
                                    dirties only            input events ─────────┐
                                    affected nodes                                │
                                                                                  ▼
                                                                          event routing
                                                                          → script handlers
```

The important property: a signal change does **not** re-run parse or cascade. It marks the specific nodes that subscribed to it dirty, and only those nodes re-layout/repaint. Parse and cascade run at load and on hot-reload; the reactive loop runs every interaction.

## What we reuse vs. what we build

Law 4 in one table. The "build" column is the actual surface area of the project.

| Concern | Reuse (crate) | We build |
|---|---|---|
| Windowing / input events | `winit` | thin adapter to our event model |
| GPU surface | `wgpu` | — |
| Vector painting | `vello` (or `tiny-skia` CPU fallback) | scene builder from our render tree |
| Text shaping / layout | `parley` + `swash` (or `cosmic-text`) | glyph → paint integration |
| CSS parsing | `lightningcss` | property → our style-struct mapping |
| Flexbox/grid layout | `taffy` | style → taffy-node translation |
| Script interpreter | `rhai` | host bindings, signal integration |
| File watching | `notify` | debounce + reload orchestration |
| **Template parsing** | *(none — ours)* | XML+directive parser → node tree |
| **Reactive graph** | *(ours; Leptos-inspired)* | signals, subscriptions, dirty tracking |
| **The glue** | — | the document model tying all stages |

Roughly: **template parser + reactive graph + the document model** are the genuinely new code Everything else is integration.

---

## Stage 1 — Parse

Input: the raw `.rux` text. Output: three parsed artifacts bundled into a **Document**.

1. **Split the SFC.** Extract `<template>`, `<style>`, `<script>` regions. Cheap pre-pass; each section then goes to its own parser.
2. **Template → node tree.** Our own parser (the one piece with no off-the-shelf answer). It' XML-shaped but must recognize our attributes specially:
   - `{{ expr }}` interpolations in text and attribute values → **binding nodes**, not literal strings.
   - `r-for`, `r-if`/`r-elif`/`r-else`, `r-show` → **structural directives** attached to the node, not attributes.
   - `:attr`, `r-model`, `@event` → **binding metadata** on the node.
   - Custom element tags (kebab-case, not one of the six) → **component instantiation** points Output is a tree of `TemplateNode`s where every dynamic piece is already distinguished from static text. Directive expressions are stored as *unparsed source strings* here; they compile against the script scope in Stage 3.
3. **Style → stylesheet.** Hand the CSS to `lightningcss`; keep its parsed rule list. No evaluation yet.
4. **Script → program.** Hand the script to `rhai`'s parser; keep the compiled AST. Top-level `let signal(...)` declarations and `fn`s are catalogued so the template compiler can resolve names.
Parse errors do **not** panic. They produce a `DiagnosticSet` that the [hot-reload](#hot-reload) layer renders as a dev overlay — the accepted cost of [runtime documents](./01-rationale.md#runtime-documents-over-compile-time-components).

## Stage 2 — Cascade

Input: the node tree + the parsed stylesheet. Output: a **resolved style struct** per node.

- Match selectors against each node. Selector kinds we honor:
  class (`.x`), id (`#x`), element (`view`, `text`, …), and attribute for roles
  (`[role="list"]`), plus descendant/child combinators.
- Apply the cascade: specificity + source order, exactly as CSS defines. We lean
  on `lightningcss`'s parsed representation; the cascade *algorithm* is small and
  ours.
- Map matched declarations onto a fixed `ComputedStyle` struct holding only the
  [honored subset](./02-spec.md#styling). Unhonored properties are dropped and
  logged in dev mode.
- Split `ComputedStyle` conceptually into **layout props** (feed Stage 4) and
  **paint props** (feed Stage 5).

Cascade re-runs on style hot-reload or when a node's classes change reactively — never on a plain signal value change.

## Stage 3 — Reactive graph

This is the heart, and it's ours (modeled on Leptos/Solid — proven in Rust, per Law 4). Input: the node tree + compiled script. Output: a live graph where value changes propagate to exactly the affected nodes.

- **Signals** are the leaves: `signal(v)` registers a reactive cell in the runtime. `rhai`'s host functions expose `signal`, `.get()`, `.set()`, `.update()` into script scope.
- **Binding compilation.** Each `{{ expr }}`, `:attr`, `r-if`, `r-for`, etc. is compiled once into a *reactive computation*: a closure that reads signals and produces a value. The read establishes the subscription — reading a signal inside a binding records "this binding depends on that signal."
- **Dirty propagation.** `signal.set()` marks its subscribers dirty and schedules a frame. On the frame, dirty computations re-run; a text binding updates its node's text, an `r-if` adds/removes a subtree, an `r-for` diffs its keyed list.
- **No virtual DOM.** The binding *is* the subscription; there is no whole-tree diff. Only dirty computations do work.

The structural directives are reactive computations with side effects on tree shape:

| Directive | Reactive effect |
|---|---|
| `{{ }}` / `:attr` | update a node's text / attribute |
| `r-if`/`r-elif`/`r-else` | mount/unmount a subtree |
| `r-show` | toggle a visibility flag (node stays, layout slot kept) |
| `r-for` | keyed reconcile of child instances against the data |
| `r-model` | two-way: bind value in, write signal on input event |

## Stage 4 — Layout

Input: layout props + tree shape. Output: a box (x, y, w, h) per node.

- Translate each node's `ComputedStyle` layout props into a `taffy` style and
  build a parallel `taffy` tree. `taffy` *is* a flexbox/grid engine, so
  `display: flex`, `flex-direction`, `gap`, `justify-content`, `align-items`,
  `padding`, `margin`, sizing, and `overflow` map almost 1:1.
- Text nodes need intrinsic sizing: `parley` shapes the text to measure it, and
  that measurement feeds `taffy` as a leaf's content size.
- Run `taffy` to produce absolute rects. Cache them; only re-run for the dirty
  subtree when a reactive change altered size-affecting props or tree shape.
- `overflow: auto`/`scroll` produces a clipping+scrollable region and enables the
  node's `scroll` [capability](./02-spec.md#events).

## Stage 5 — Paint & present

Input: paint props + layout rects. Output: pixels on the window.

- Walk the laid-out tree building a **scene** for `vello`: rounded rects
  (`background`, `border-radius`, `border`), text runs (from `parley`/`swash`
  glyphs), images, shadows, opacity, clips for scroll regions.
- `vello` renders the scene on `wgpu`; `winit` owns the window/surface.
- A `tiny-skia` CPU backend is the fallback for platforms/targets without a
  usable GPU (and the likely path for the embedded target later).
- Only the dirty region repaints where the backend allows; otherwise repaint the
  frame but skip re-layout of clean subtrees.

## Input & event routing

The reverse path: `winit` raw input → our event model → script handlers.

1. `winit` delivers pointer/touch/keyboard events.
2. **Gesture recognition** turns raw pointer streams into our gesture-honest
   [capabilities](./02-spec.md#events): a press+release within slop/time = `tap`;
   held = `longpress`; moved = `drag`/`swipe`; wheel/touch-move over an
   `overflow` node = `scroll`. This is where "not a browser" is enforced —
   `hover` only exists when a pointer device is present.
3. **Hit-testing** uses the layout rects to find the target node; the event
   bubbles up ancestors (like DOM bubbling) until a handler consumes it.
4. If the node bound that capability with `@event`, the handler runs in the
   [script tier](#the-script-tier--host-bridge). Handlers mutate signals; signal
   changes re-enter Stage 3. The loop closes.

## The script tier & host bridge

Two tiers, one boundary — the [decision](./01-rationale.md#two-tier-logic-rhai-script-over-a-compiled-rust-host).

- **Script (rhai).** Runs the component's handlers and holds its signals. Each
  component instance gets a `rhai` scope seeded with its top-level `let`s/`fn`s.
  Interpreted, so it hot-reloads.
- **Host (compiled Rust).** A `HostRegistry` maps names → Rust closures, injected
  into the `rhai` engine as the `host` module. This is the registry contract:

  ```rust
  // illustrative — final shape decided during build
  let mut host = HostRegistry::new();
  host.function("load_devices", || db::all());
  host.function("read_battery", || sensors::battery());
  host.function("open", |id: i64| nav::open(id));
  engine.register_static_module("host", host.into_module());
  ```

- **Boundary rules.** Script may only call registered `host::` names (unknown →
  diagnostic). All native capability (fs, net, sensors, navigation, native
  pickers) crosses here. Values marshal as `rhai` dynamics; the host validates.
- **Threading.** Host functions that block (I/O) run off the UI thread and
  resolve back onto it before touching signals, so the render loop never stalls.
  (Async model is an [open question](#open-questions).)

## Hot-reload

The feature that drove the whole [runtime-document decision](./01-rationale.md#runtime-documents-over-compile-time-components).

- `notify` watches the `.rux` file(s); a short debounce coalesces editor saves.
- On change, re-run **only the stages the edit affects**:

  | Edited section | Re-run from |
  |---|---|
  | `<style>` | Cascade (Stage 2) → relayout/repaint |
  | `<template>` | Parse template → Cascade → rebuild reactive graph |
  | `<script>` | Reparse script → re-seed scopes → rebuild bindings |
  | host (Rust) | **rebuild required** — not hot |

- **State preservation** is best-effort: on a script/template reload we attempt
  to carry signal values forward by name; when the shape changed too much we
  reset to initial. (Exact policy is an [open question](#open-questions).)
- Parse/eval failures show the `DiagnosticSet` as an **overlay** over the last
  good frame — never a crash, never a blank window.

## Crate layout

A workspace of focused crates so pieces stay swappable (and testable without a
GPU):

```
rux/
├─ rux-parser     # template XML+directive parser → TemplateNode tree   (ours)
├─ rux-style      # lightningcss integration, cascade, ComputedStyle    (ours+reuse)
├─ rux-reactive   # signals, subscriptions, dirty scheduling            (ours)
├─ rux-script     # rhai engine, host registry, scope wiring            (ours+reuse)
├─ rux-layout     # ComputedStyle → taffy, text measwith parley         (integration)
├─ rux-paint      # render tree → vello scene; tiny-skia fallback       (integration)
├─ rux-runtime    # the Document model; owns the pipeline + hot-reload  (ours)
├─ rux-shell      # winit window, input→event, frame loop               (integration)
└─ rux-cli        # `rux run app.rux`, dev overlay, watcher             (glue)
```

`rux-reactive` and `rux-parser` have zero GPU/OS deps and are unit-testable in
isolation — deliberately, since they're the novel logic.

## Milestone plan

Each milestone is independently demoable and de-risks the next. The ordering
front-loads the thesis (layout-is-CSS, hot-reload) before the elaborate parts.

| # | Milestone | Proves | Rough surface |
|---|---|---|---|
| **M0** | Blank `winit`+`wgpu` window, clear color | GPU/window path works on Windows | `rux-shell` |
| **M1** | Hardcoded node tree → `taffy` → `vello` paints rects | layout+paint pipeline | `rux-layout`, `rux-paint` |
| **M2** | Parse a static `.rux` template+style → render it | our parser + cascade + literal CSS | `rux-parser`, `rux-style` |
| **M3** | `notify` watcher → edit `.rux`, window repaints | **hot-reload thesis** | `rux-runtime`, `rux-cli` |
| **M4** | Text via `parley`; real content sizing | text is a first-class citizen | `rux-layout` |
| **M5** | Signals + `{{ }}` bindings update on change | reactive graph | `rux-reactive` |
| **M6** | Input → gestures → `@tap` handler mutates a signal | full interaction loop | `rux-shell`, `rux-script` |
| **M7** | `r-for`, `r-if`, `r-model`, `<input>` controls | directive set | across |
| **M8** | Host registry; `host::` calls from script | native-capability boundary | `rux-script` |
| **M9** | Component import + embed (`<device-tile>`) | reuse/module system | `rux-parser`, `rux-runtime` |

The [guide's](./03-guide.md) finished dashboard is the acceptance test for M9 —
when that file renders and behaves, v0.1 is real.

## Open questions

Deferred to build-time decisions, flagged so we don't pretend they're solved:

- **Async/host concurrency** — the exact model for non-blocking host calls and
  how their results re-enter the signal graph.
- **State preservation policy on hot-reload** — how aggressively to carry signal
  values across a reload before resetting.
- **Text/`parley` vs `cosmic-text`** — pick after M4 measures both for our needs.
- **Vello maturity on the CPU/embedded path** — may push `tiny-skia` earlier for
  the eventual `thumbv7em` target.
- **Event object shape** — the concrete gesture payload passed to handlers
  (`$event`), finalized alongside M6.
- **Grid/table** — still [deferred](./01-rationale.md#the-element-audit); revisit
  only after v0.1 ships.

---

Back to: [README](./README.md) · [rationale](./01-rationale.md) ·
[spec](./02-spec.md) · [guide](./03-guide.md).
