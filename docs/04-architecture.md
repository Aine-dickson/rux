# 04. Architecture

How the Rux runtime turns a `.rux` file into pixels and keeps them live. The [spec](./02-spec.md) defines *what the language is*; this defines how the runtime is built.
Everything here is downstream of the [rationale](./01-rationale.md), especially [Law 4](./01-rationale.md#law-4-stay-close-to-rust-dont-pop-the-balloon): **reuse mature Rust crates; write only the glue that is uniquely Rux's.**

## Contents

- [The pipeline at a glance](#the-pipeline-at-a-glance)
- [What Rux reuses and what it builds](#what-rux-reuses-and-what-it-builds)
- [Stage 1: Parse](#stage-1-parse)
- [Stage 2: Cascade](#stage-2-cascade)
- [Stage 3: Reactive graph](#stage-3-reactive-graph)
- [Stage 4: Layout](#stage-4-layout)
- [Stage 5: Paint & present](#stage-5-paint--present)
- [Input & event routing](#input--event-routing)
- [The script tier & host bridge](#the-script-tier--host-bridge)
- [Hot-reload](#hot-reload)
- [Crate layout](#crate-layout)
- [Plan and open work](#plan-and-open-work)

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

The important property: a state change does **not** re-run parse or cascade. The runtime records which signals each binding reads and which a handler writes, so a handler patches the nodes the written signals reach and reconciles the lists whose shape changed. A full rebuild of the tree is the fallback, not the path. Parse and cascade run at load and on hot-reload.

## What Rux reuses and what it builds

Law 4 in one table. The "Rux builds" column is the actual surface area of the project.

| Concern | Reused (crate) | Rux builds |
|---|---|---|
| Windowing / input events | `winit` | adapter to Rux's event model (`rux-shell`) |
| GPU rendering | `wgpu`, through `vello` | none |
| Vector painting | `vello` (vendored, scratch buffers sized to the scene), `vello_svg` for SVG images | scene builder from resolved paint items (`rux-paint`) |
| Text shaping and layout | `parley` | glyph drawing and caching (`rux-text`) |
| CSS parsing | `lightningcss` | cascade, property to style-struct mapping (`rux-style`) |
| Flexbox, grid and block layout | `taffy` 0.15, vendored with a `calc()` resolver (`vendor/`) | style to taffy-node translation, paint and hit regions (`rux-layout`) |
| Images | `image` | decoding, caching, drawing |
| SVG parsing | `usvg` | none |
| Audio | `cpal` (output), `symphonia` (decoding) | mixer and element behaviour (`rux-audio`) |
| Video | the platform's own decoder (Media Foundation, `MediaCodec`) | frame reader (`rux-video`) |
| Accessibility | `accesskit`, `accesskit_winit` | accessibility tree from layout |
| Clipboard, file dialogs | `arboard`, `rfd` | none |
| File watching | `notify` | reload orchestration |
| HTTP | `ureq` (remote sources, desktop) | none |
| Script language | none | parser, checker, typed IR, interpreter, Rust codegen (`rux-syntax`, `rux-script`, `rux-ir`, `rux-codegen`) |
| Template parsing | none | XML plus directive parser (`rux-parser`) |
| Reactive graph | none | signals and dependency tracking (`rux-reactive`) |
| Native Rust bridge | `syn` (reading exports) | `#[rux::export]`, resources, native modules (`rux-native`, `rux-bindgen`) |

The genuinely new code is the script language, the template parser, the reactive graph and the document model. Everything else is integration.

---

## Stage 1: Parse

Input: the raw `.rux` text. Output: three parsed artifacts bundled into a **Document** (`rux-runtime`).

1. **Split the SFC.** Extract `<template>`, `<style>`, `<script>` regions (`rux-parser`). Cheap pre-pass; each section then goes to its own parser.
2. **Template → node tree.** Rux's own XML-shaped parser, which recognizes the Rux attributes specially:
   - `{{ expr }}` interpolations in text and attribute values → **binding nodes**, not literal strings.
   - `r-for`, `r-if`/`r-elif`/`r-else`, `r-show` → **structural directives** attached to the node, not attributes.
   - `:attr`, `r-model`, `@event` → **binding metadata** on the node.
   - Custom element tags (kebab-case, not a built-in element) → **component instantiation** points. Components come from `use` imports, each loaded as its own `.rux` file (`rux-runtime`).

   Output is a tree where every dynamic piece is already distinguished from static text. Directive expressions are kept as source text here; they are checked and lowered against the script in Stage 3.
3. **Style → stylesheet.** The CSS goes to `lightningcss` (`rux-style`); a `<style>` that uses Tailwind directives is compiled first by the `tailwindcss` CLI (`rux-runtime`'s `tailwind` module). No evaluation yet.
4. **Script → program.** Rux's own parser (`rux-syntax`) reads the script into an AST. The checker (`rux-script`) resolves names and types, and lowering turns it into the typed IR (`rux-ir`) that the interpreter runs. Top-level `let`s and `fn`s are catalogued so bindings can resolve names.

Parse errors do **not** panic. They become a `LoadError`, or `Diagnostics` (errors and warnings, collected rather than printed, since a CLI, a window and a browser all load the same document). The [hot-reload](#hot-reload) layer renders them as the dev overlay, the accepted cost of [runtime documents](./01-rationale.md#runtime-documents-over-compile-time-components).

## Stage 2: Cascade

Input: the node tree + the parsed stylesheet. Output: a **resolved style** per node (`rux-style`).

- Match selectors against each node: class, id, element, attribute (`[role="list"]`), pseudo-classes, and the combinators.
- Apply the cascade: specificity + source order, as CSS defines. `lightningcss` supplies the parsed representation; the cascade itself is Rux's.
- Map matched declarations onto the `Style` struct in `rux-layout`, which holds only the honored subset of CSS (the list is in [as-built](./05-as-built.md#honored-css)). Properties Rux does not honor are dropped and reported in the dev overlay and on stderr.
- The struct's **layout props** feed Stage 4 and its **paint props** feed Stage 5.

Cascade re-runs on style hot-reload or when a node's classes change reactively, never on a plain signal value change.

## Stage 3: Reactive graph

This is the heart. Input: the node tree + the checked script. Output: a live document where value changes reach exactly the affected nodes. The value types live in `rux-reactive`; the state and dependency tracking live in `rux-script`'s `Engine`.

- **State** is the script's top-level `let n = signal(v)` declarations, plus `computed` values derived from them. Writing a signal from a handler or an effect is a change; the [script reference](./07-script.md) has the language.
- **Dependency tracking.** Each `{{ expr }}`, `:attr`, `r-if`, `r-for`, etc. is evaluated by the interpreter, which records what it reads. A binding depends on the state it read, and a handler is known by the state it writes.
- **Patching.** After a handler runs, the runtime patches the text and attribute bindings the written state reaches, mounts or unmounts `r-if` subtrees, and reconciles `r-for` lists by `r-key`. A change to structure the patch cannot express falls back to a full rebuild of the tree.
- **No virtual DOM.** The binding is the subscription; there is no whole-tree diff.

The structural directives are reactive computations with side effects on tree shape:

| Directive | Reactive effect |
|---|---|
| `{{ }}` / `:attr` | update a node's text / attribute |
| `r-if`/`r-elif`/`r-else` | mount/unmount a subtree |
| `r-show` | toggle a visibility flag (node stays, layout slot kept) |
| `r-for` | keyed reconcile of child instances against the data |
| `r-model` | two-way: bind value in, write signal on input event |

## Stage 4: Layout

Input: the styled tree. Output: absolute paint items plus the regions the shell needs (`rux-layout`).

- Each node's layout props become a `taffy` style in a parallel `taffy` tree. Taffy is the flexbox and grid engine, so `display: flex`, `display: grid`, `gap`, `justify-content`, `align-items`, `padding`, `margin`, sizing and `overflow` map almost 1:1. The workspace patches Taffy 0.15 with a resolver for `calc()` (see `vendor/README.md`).
- Text leaves are sized through a `measure` callback: `rux-text` shapes the text with `parley`, and that measurement is the leaf's content size. `rux-layout` itself has no font dependency.
- Taffy produces the boxes, and `rux-layout` flattens them to absolute rects in one coordinate space.
- Alongside the paint items come hit regions, scroll regions, focus order, selectable text, hover and active state regions, and the accessibility nodes. `overflow: auto`/`scroll` produces a clipping, scrollable region.

## Stage 5: Paint & present

Input: paint items. Output: pixels on the window.

- `rux-paint` turns the paint items into a `vello` scene: rounded rects with per-corner radii and per-side borders, gradients, box shadows, glyph runs (via `rux-text`), images, SVG, scrollbars, the selection highlight, and clips for scroll regions. It decides nothing about how a document should look; every judgement call is upstream.
- `vello` renders the scene on `wgpu`; `winit` owns the window and surface (`rux-shell`).
- There is one renderer. The browser build (`rux-web`) runs the same shell on a canvas.

## Input & event routing

The reverse path: `winit` raw input → Rux's event model → script handlers. All of it lives in `rux-shell`.

1. `winit` delivers pointer, touch, keyboard and IME events. The web build feeds the same paths from the canvas.
2. **Gesture recognition** turns raw pointer streams into Rux's gesture-honest events: a press and release within slop and time is `@tap`; held is `@longpress`; moved is `@drag` or `@swipe`; wheel or touch-move over an `overflow` node is a scroll. A finger is not routed down the mouse path: a drag moves the caret where a mouse drag selects. `hover` only exists when a pointer device is present.
3. **Hit-testing** uses the layout's hit regions to find the target node; the event bubbles up ancestors until a handler consumes it. Keys go to the focused element, then outward through `@key` handlers.
4. If the node bound that event with `@event`, the handler runs in the [script tier](#the-script-tier--host-bridge). Handlers write signals, and the change re-enters Stage 3. The loop closes.

Accessibility runs the same loop in reverse: layout's accessibility nodes become an `accesskit` tree, published to the platform's screen reader.

## The script tier & host bridge

Two tiers, one boundary. See the [decision](./01-rationale.md#two-tier-logic-script-over-a-compiled-rust-host) (the interpreter in that decision is Rux's own now, not rhai).

- **Script.** Rux's own typed language. `rux-syntax` parses it, `rux-script` checks it and lowers it to the IR in `rux-ir`, and the interpreter in `rux-script` runs the component's handlers and holds its state. Interpreted, so it hot-reloads. A release build (`rux build --release`) also writes Rust for a file's functions (`rux-codegen`); that code runs inside the interpreter, and only for text it was generated from, so a hot-reloaded file is interpreted as before.
- **Native Rust.** An app's Rust lives in a `native/` crate that depends on `rux-native` as `rux`. `#[rux::export]` marks what script may call, and `pub mod`s become `native::name` modules. `rux-bindgen` reads the crate's source for the signatures, so `rux check` and the editor know them without a build. `rux-native-macros` provides the attributes.
- **Boundary rules.** Script reaches native code only through the modules the app registers; an unknown name is a diagnostic. Values cross as `Any` and are converted by `FromRux` and `IntoRux`, which throw instead of wrapping on overflow. See [Native code](./07-script.md#native-code).
- **Threading.** Script runs on one thread. An `async` export's future runs where `rux_native::spawn` runs it, its answer is queued, and the shell is woken to resume the script's `await` on the script thread, so the render loop never stalls and script never races.

## Hot-reload

The feature that drove the whole [runtime-document decision](./01-rationale.md#runtime-documents-over-compile-time-components).

- `notify` watches the `.rux` file's directory recursively, so edits to imported components count, and so do edits to `.css` files a `<style src>` includes.
- On change the shell loads the whole document again (`Document::load`) and swaps it in. The window's own state survives: its size and pointer state, and the page the app was on. Script state starts afresh, because the reloaded document is a new one; the old one is shut down first, so its `unmounted` bodies run.
- Editing the compiled Rust in `native/` is not hot: it needs a rebuild.
- A load that fails keeps the last good frame on screen under an **overlay** naming the error; it is never a crash and never a blank window.
- On a device, `rux run --device` pushes the changed documents to the installed dev build over a connection that both ends authenticate with a shared secret, and the app reloads from them the same way.

## Crate layout

A workspace of focused crates so pieces stay swappable and testable without a GPU. Each crate is internal except the CLI and `rux-native`; the supported entry point is the `ruxlang` crate.

```
crates/
├─ rux-parser         # SFC split, template XML + directive parser
├─ rux-style          # lightningcss integration, cascade onto the template tree
├─ rux-reactive       # signal values (no dependencies)
├─ rux-syntax         # the script language's lexer, parser and AST
├─ rux-ir             # the typed IR and the type language
├─ rux-script         # checker, lowering, interpreter, state, native modules
├─ rux-codegen        # Rust generated from the IR, for release builds
├─ rux-layout         # styled tree → taffy → absolute paint items and regions
├─ rux-text           # parley shaping and layout, vello glyph drawing
├─ rux-paint          # paint items → vello scene
├─ rux-runtime        # the Document model: loading, components, bindings, patching
├─ rux-shell          # winit window, GPU surface, input, focus, clipboard, a11y, frame loop
├─ rux-audio          # symphonia decoding, cpal mixer
├─ rux-video          # platform video decoders (Media Foundation, MediaCodec)
├─ rux-icons          # bundled Tabler icon path data
├─ rux-native         # #[rux::export], resources, native modules (an app's `rux`)
├─ rux-native-macros  # the attribute macros behind rux-native
├─ rux-bindgen        # reads an app's native/ crate for its exports and signatures
├─ rux-fmt            # re-indentation for .rux files
├─ rux-highlight      # TextMate grammar interpreter for syntax colouring
├─ rux-web            # the shell compiled to WebAssembly, drawing to a canvas
├─ rux-cli            # `rux run | check | fmt | build | new | vocab | doctor`
└─ rux-aot-tests      # compiled functions run beside the interpreter (tests only)
```

`rux-reactive`, `rux-parser`, `rux-syntax`, `rux-ir` and `rux-layout` have no GPU or OS dependencies. `rux check` builds a `Document` and discards the tree, so checking a file needs no window and runs in CI.

## Plan and open work

What is built is described in [as-built](./05-as-built.md); what is next and what is open is in the [roadmap](./06-roadmap.md).

---

Back to: [README](./README.md) · [rationale](./01-rationale.md) ·
[spec](./02-spec.md) · [guide](./03-guide.md).
