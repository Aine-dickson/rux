# 02 — Spec

> ⚠️ **This is the v0.1 design surface, not the built surface.** Several things
> here were never implemented (non-text `<input>` types, `<image>` rendering,
> real scrolling) and some were implemented differently (the inline/block model
> was removed; grid was added; rhai functions can't mutate state). See
> **[05 — As Built](./05-as-built.md)** for the current reality.

The formal reference for Rux v0.1. This is the source of truth we architect and build against. It describes the language surface, not the runtime implementation
(that comes later). Every rule here traces to a law in the
[rationale](./01-rationale.md).

> **Status:** v0.1 design. Syntax is settled; anything marked _deferred_ is
> intentionally out of scope for the first build.

## Contents

- [File format — the SFC](#file-format--the-sfc)
- [Elements](#elements)
- [Roles](#roles)
- [Directives & bindings](#directives--bindings)
- [Events](#events)
- [Styling](#styling)
- [Scripting and the host](#scripting-and-the-host)
- [Reactivity](#reactivity)
- [Modules & reuse](#modules--reuse)
- [Hot-reload boundary](#hot-reload-boundary)

---

## File format — the SFC

A Rux program is one or more **single-file components** with the extension
`.rux`. Each file has up to three top-level sections, in any order, each at most
once:

```xml
<template>  <!-- semantic markup — the view tree --> </template>
<style>     /* literal CSS — how it looks and lays out */ </style>
<script>    // rhai (Rust-shaped) — logic, signals, handlers </script>
```

- `<template>` is **required** and must contain exactly one root element.
- `<style>` and `<script>` are optional.
- A file with a `<template>` is a **component**; its file name (kebab-cased)
  is the tag other files use to embed it. See [modules & reuse](#modules--reuse).

The application entry point is a component whose root is [`<screen>`](#elements).

---

## Elements

There are exactly **six** elements (Law 3). Everything else is a [role](#roles),
an [`<input type=>`](#input-types), a [directive](#directives--bindings), or CSS.

| Element | Is | Web analogue | Notable attributes |
|---|---|---|---|
| `<screen>` | root of one view | `<body>` | — |
| `<view>` | generic container / box | `<div>` | `role` |
| `<text>` | text run | `<span>` / `<p>` | `role`, `for` |
| `<image>` | bitmap or vector image | `<img>` | `src`, `alt` |
| `<button>` | tappable control | `<button>` | `disabled` |
| `<input>` | user input control | `<input>` | `type`, `r-model`, `:options`, `min`, `max`, `placeholder`, `id` |

Any element accepts: `class`, `id`, `role` (where semantic), style/structural
[directives](#directives--bindings), and the [events](#events) in its capability
set.

### Input types

`<input>` absorbs every form control via `type=` (Laws 3 & 4). There is no
`<select>`, `<option>`, or `<textarea>`.

| `type` | Control | Binds to |
|---|---|---|
| `text` (default) | single-line text | string |
| `textarea` | multi-line text | string |
| `number` | numeric text | number |
| `checkbox` | on/off box | bool |
| `switch` | on/off toggle | bool |
| `radio` | one-of within a `name` group | string |
| `slider` | range | number (`min`/`max`/`step`) |
| `select` | choice from a set | string; choices via `:options` |
| `date` | date picker | date string |

**Choices are data, not tags.** A `select` (or a `radio` group) takes its
options from a bound collection:

```xml
<input type="select" r-model="choice" :options="fruits" />
```

On platforms with a native picker (mobile), `select` and `date` should invoke
the platform control rather than a rendered dropdown — a
[host](#scripting-and-the-host) capability, not a browser emulation.

_Deferred:_ options that need custom per-item markup (icon + multiline) — a
"templated input" feature for a later version.

---

## Roles

`role=` gives an element semantic meaning for accessibility and intent **without
adding elements**. It never affects layout or behavior (Law 1/3). Screen readers
and assistive tech consume it; the visual result is still 100% CSS.

Recognized roles (extensible):

| Category | Roles |
|---|---|
| Structure | `section`, `header`, `footer`, `nav`, `main`, `aside` |
| Text | `heading`, `paragraph`, `label`, `link` |
| Collections | `list`, `listitem` |
| Forms | `form` |

Role-specific attributes:

- `role="link"` → `to="/path"` — a navigation intent handled by the router
  (ecosystem), not a URL fetch.
- `role="label"` → `for="<input id>"` — associates the label with an input;
  tapping the label focuses the input.
- `role="form"` → pairs with `@submit`.

```xml
<view role="nav" class="bar">
  <text role="link" to="/settings">Settings</text>
</view>
```

---

## Directives & bindings

Structural and binding features are attributes. Interpolation uses `{{ }}`;
everything structural is prefixed `r-` (one prefix, no collisions — this is what
lets loop-`r-for` and label-`for=` coexist).

| Syntax | Meaning | Vue analogue |
|---|---|---|
| `{{ expr }}` | text interpolation (in element bodies) | `{{ }}` |
| `:attr="expr"` | one-way bind an attribute to an expression | `:attr` |
| `r-model="target"` | two-way bind (form controls) | `v-model` |
| `@event="handler"` | bind a handler to a [capability](#events) | `@click` |
| `r-for="item in items"` | repeat the element per item | `v-for` |
| `r-if` / `r-elif` / `r-else` | conditional inclusion | `v-if` |
| `r-show="expr"` | toggle visibility, keep layout slot | `v-show` |

Notes:

- `{{ expr }}` and `:attr` / `r-model` / `r-for` accept **script expressions**
  evaluated in the component's [scope](#scripting-and-the-host).
- `r-for` supports an index form: `r-for="(item, i) in items"`.
- Elements in an `r-for` should carry a stable `:key` where identity matters
  (reordering, animation).
- `r-if` removes the element from the tree; `r-show` keeps it and toggles
  visibility — the same distinction Vue draws.

```xml
<view role="list" class="feed">
  <view role="listitem" class="tile"
        r-for="d in devices" :key="d.id" @tap="select(d)">
    <text role="heading">{{ d.name }}</text>
    <text role="paragraph" r-if="d.online">online</text>
    <text role="paragraph" r-else>offline</text>
  </view>
</view>
```

---

## Events

Each element publishes a fixed **capability set** — the events it can emit
(Law 2). Bind with `@event`. The vocabulary is gesture-first (device-honest);
`hover` is pointer-only and never fires on touch.

| Element | Capabilities |
|---|---|
| `<screen>` | `appear`, `disappear`, `key`, `back` |
| `<view>` | `tap`, `longpress`, `drag`, `swipe`, `scroll`¹, `hover`² |
| `<text>` | `tap`, `select` |
| `<image>` | `tap`, `load`, `error` |
| `<button>` | `tap`, `press`, `release`, `longpress`, `focus` |
| `<input>` | `input`, `change`, `focus`, `blur`, `submit` |

¹ `scroll` only fires if CSS gave the element `overflow: auto`/`scroll` —
capability follows style.
² `hover` is pointer-only; binding it on a touch device is legal but silent.

A handler receives an event object with gesture data (position, delta, key,
etc.); the exact shape is defined during runtime design.

```xml
<button @tap="select(d)" @longpress="pin(d)">…</button>
<view @swipe="onSwipe($event)">…</view>
```

---

## Styling

The `<style>` section is **literal CSS**, parsed by `lightningcss`. Selectors,
the cascade, and specificity work as on the web. Class and id selectors match
`class=`/`id=`; element selectors match the six element names; `[role=…]`
attribute selectors match roles.

### Honored subset (v0.1)

The runtime *parses* full CSS but only *honors* the properties below, mapped onto
`taffy` (layout) and the painter. Unhonored properties are ignored (and should be
reported in dev mode), not errors.

| Group | Properties |
|---|---|
| Box model | `width`, `height`, `min/max-width`, `min/max-height`, `padding`, `margin` |
| Flex layout | `display: flex`, `flex-direction`, `flex-wrap`, `flex`, `gap`, `justify-content`, `align-items`, `align-self` |
| Positioning | `position: relative/absolute`, `top/right/bottom/left`, `overflow`, `z-index` |
| Background/border | `background`, `background-color`, `border`, `border-radius`, `border-color`, `border-width` |
| Text | `color`, `font-size`, `font-weight`, `font-family`, `line-height`, `text-align`, `letter-spacing` |
| Effects | `opacity`, `box-shadow` |
| Lists | `list-style` (marker rendering for `role="list"`) |

Units: `px`, `%`, `rem`, `fr` (grid), and unitless where CSS allows. Colors: hex,
`rgb[a]()`, named. `display: grid` and its properties are **partially** honored
in v0.1 (single-axis tracks); full grid and `display: table` are _deferred_ (the
[table story](./01-rationale.md#the-element-audit)).

Styles may be inline in `<style>` or imported (see [modules](#modules--reuse)).

---

## Scripting and the host

Logic lives in two tiers (Law 4, and the
[decision](./01-rationale.md#two-tier-logic-rhai-script-over-a-compiled-rust-host)):

### Script tier — `<script>`, interpreted `rhai`

Rust-shaped syntax (`let`, `fn`, closures, `if`/`for`). Hot-reloads. Holds the
component's signals, handlers, and glue. Everything declared at the top level of
`<script>` is in scope for the template's expressions.

```rust
<script>
use components::device_tile;          // import another component (see modules)

let level   = signal(82);             // reactive state
let devices = signal(host::load_devices());

fn refresh() {                        // a handler bound via @tap
    level.set(host::read_battery());
}

fn select(d) {
    host::open(d.id);
}
</script>
```

### Host tier — compiled Rust, exposed as `host::…`

The **registry contract** between file and binary. The compiled app registers
named capabilities; the script calls them by name. This is the boundary where
native, heavy, or performance-critical work lives, and it's the one part that
requires a rebuild.

Conceptually, the host registers functions into the script's `host` namespace:

```rust
// compiled Rust (illustrative — final API defined in runtime design)
registry.function("read_battery", || sysinfo::battery_percent());
registry.function("load_devices", || db::all_devices());
registry.function("open", |id: Id| navigator::open(id));
```

Rules of the contract:

- The script may only call `host::` names that were registered; unknown names
  are a runtime error surfaced in the dev overlay.
- Host functions are the *only* way script reaches native capability (files,
  network, sensors, navigation, platform pickers).
- Types cross the boundary as `rhai`'s dynamic values; the host is responsible
  for validation.

---

## Reactivity

Reactivity is a **core primitive** (not ecosystem — see the
[decision](./01-rationale.md#reactivity-is-a-core-primitive-state-management-is-ecosystem)).
The model is fine-grained signals (Leptos/Solid style): a binding *is* a
subscription; there is no virtual-DOM diffing.

### `signal`

```rust
let count = signal(0);

count.get()               // read (also tracks a dependency when read in a binding)
count.set(5)              // replace
count.update(|c| c + 1)   // functional update
```

### How bindings subscribe

When the template compiler encounters `{{ count }}` (or `:attr="count"`), it
records that this node depends on `count`. When `count` changes, **only that
node** re-lays-out and repaints:

```
{{ device.battery }}  ──parse──►  node subscribes to device.battery
                                          │
     device.battery.set(80)  ───────► only that node updates
```

Derived values are computed from other signals and cache until a dependency
changes (a `computed`/memo primitive; exact spelling finalized in runtime
design). **Stores, routing, and persistence are ecosystem crates** built on
`signal` — not part of this spec.

---

## Modules & reuse

A `.rux` file *is* a component. Reuse mirrors Rust's module system (Law 4):
import with `use`, embed as a custom element named after the file.

```rust
<script>
use components::device_tile;   // ./components/device_tile.rux
</script>
```

```xml
<template>
  <view role="list" class="feed">
    <device-tile r-for="d in devices" :device="d" @tap="select(d)" />
  </view>
</template>
```

- The file `device_tile.rux` is embedded as `<device-tile>` (kebab-case of the
  file/component name).
- **Props** pass in as attributes (`:device="d"`), one-way bound from parent
  scope to child.
- A child communicates upward by emitting events its parent binds with `@`.
- `use` paths resolve relative to the importing file, following the same
  directory rules as Rust modules.

_Deferred:_ slots/children projection (passing template fragments into a
component) — a later version.

---

## Hot-reload boundary

What reloads live vs. what needs a rebuild — the practical contract for authors:

| You edit… | Result |
|---|---|
| `<template>` markup | ✅ live repaint |
| `<style>` CSS | ✅ live repaint |
| `<script>` logic (rhai) | ✅ live, state re-initialized |
| Which host fn a handler calls | ✅ live |
| The **host** (compiled Rust) — new capability, changed native fn | ❌ rebuild |

Parse/eval errors in any live section surface as a **dev overlay** in the window,
not a crash (the accepted cost of runtime documents — see the
[rationale](./01-rationale.md#runtime-documents-over-compile-time-components)).

---

Next: the [guide](./03-guide.md) builds a real screen using everything here.
