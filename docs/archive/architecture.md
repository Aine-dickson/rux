# Archive: docs/04-architecture.md

## Milestone plan

removed 2026-11-06 from docs/04-architecture.md

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

The [guide's](./03-guide.md) finished dashboard is the acceptance test for M9:
when that file renders and behaves, v0.1 is real.

## Open questions

removed 2026-11-06 from docs/04-architecture.md

Deferred to build-time decisions, flagged so we don't pretend they're solved:

- **Async/host concurrency**: the exact model for non-blocking host calls and
  how their results re-enter the signal graph.
- **State preservation policy on hot-reload**: how aggressively to carry signal
  values across a reload before resetting.
- **Text/`parley` vs `cosmic-text`**: pick after M4 measures both for our needs.
- **Vello maturity on the CPU/embedded path**: may push `tiny-skia` earlier for
  the eventual `thumbv7em` target.
- **Event object shape**: the concrete gesture payload passed to handlers
  (`$event`), finalized alongside M6.
- **Grid/table**: still [deferred](./01-rationale.md#the-element-audit); revisit
  only after v0.1 ships.

## Status, reuse table and script tier (v0.1 proposal wording)

removed 2026-11-06 from docs/04-architecture.md

> **Status:** v0.1 architecture proposal. Concrete crate choices are recommendations, not commitments. The milestone plan is designed so each can be swapped without redesign.

Reuse table rows: vector painting `vello` (or `tiny-skia` CPU fallback); text `parley` + `swash` (or `cosmic-text`); script interpreter `rhai`; reactive graph "Leptos-inspired". A `tiny-skia` CPU backend was proposed as the fallback for targets without a usable GPU "and the likely path for the embedded target later". Script tier was `rhai` with a `HostRegistry` injected as the `host` module:

```rust
let mut host = HostRegistry::new();
host.function("load_devices", || db::all());
engine.register_static_module("host", host.into_module());
```

Hot-reload state preservation was proposed as "best-effort: carry signal values forward by name; reset to initial when the shape changed too much".
