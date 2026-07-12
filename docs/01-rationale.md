# 01 — Rationale

Why Rux is shaped the way it is. Read this before proposing a change to the [spec](./02-spec.md): most additions that feel necessary are already answered by one of the laws below, and the whole design holds together only as long as the laws do.

## The origin frustration

Widget-tree toolkits make *spacing, alignment, and scrolling into objects you nest*. A centered, padded column in Flutter:

```dart
Padding(
  padding: EdgeInsets.all(16),
  child: Center(
    child: Column(children: [
      Text("Battery"),
      SizedBox(height: 8),
      Text("82%"),
    ]),
  ),
)
```

Four wrapper widgets before any content appears. This is the thing that keeps capable people from becoming mobile developers — not the hard parts, the *ceremony*. The web never had this problem: you write `padding: 16px`, not a `<Padding>` element. Rux is the wager that we can keep the web's authoring model while rendering natively.

## The laws

Everything in the [spec](./02-spec.md) is downstream of these four. When in doubt, a proposed feature has to pass all four.

### Law 1 — Content vs. spatial

> **Markup says *what the content is*. CSS says *how it looks and lays out*. > Layout primitives never appear in markup.**

There is no `<column>`, `<row>`, `<padding>`, `<center>`, `<spacer>`, `<scroll>`, or `<sizedbox>`. Those are all CSS on a `<view>`: `flex-direction`, `padding`, `justify-content`, `gap`, `overflow`. This is the law that directly kills the Flutter nesting. If a feature would put a spatial concern into markup, it's wrong.

### Law 2 — Capabilities, not widgets

> **An element is a fixed set of *capabilities* (the events it can emit). The > author *binds* to the ones they need.**

A `<button>` can `tap`, `press`, `longpress`, `focus`. A `<view>` can `tap`, `drag`, `swipe`, `scroll`. You don't reach for a new widget to get behavior; you bind a handler to a capability the element already has. See [events](./02-spec.md#events) for the capability tables.

### Law 3 — Minimal elements, `role` for meaning

> **Six elements. Everything semantic beyond them is a `role`, not a new tag.**

`section`, `header`, `footer`, `heading`, `paragraph`, `nav`, `link`, `label`, `list`, `listitem`, `form` — all of these are `role=` on a `<view>` or `<text>`, not elements. **`role` carries semantics and accessibility only — never layout or behavior.** The moment `role="scroll"` turns on scrolling, we've reinvented `<SingleChildScrollView>` under a new name, and Law 1 is breached.

The stress test that validated the element count: every "missing" tag someone asks for (`ul`, `ol`, `li`, `select`, `option`, `textarea`, `table`) collapses into a role, a loop, an `<input type=>`, or CSS — never a seventh element. See [the element audit](#the-element-audit) below.

### Law 4 — Stay close to Rust; don't pop the balloon

> **Reuse existing, Rust-shaped tools instead of inventing. Every invented > concept is a feature we now have to design, document, and maintain.**

Concretely: CSS parsing is `lightningcss`, not our own. Layout is `taffy` (a flexbox engine), not our own. Script is `rhai` (Rust-shaped syntax), not a new language. Imports mirror Rust's `use`. Reactivity follows the proven
Leptos/Solid signal model. This law is what keeps Rux a buildable project instead of a research career.

## Key decisions and the tradeoffs we accepted

Every decision below cost us something. Recording the cost so we don't relitigate it, and so we know what to revisit if the cost ever stops being worth it.

### Runtime documents over compile-time components

**Decision:** template, style, and script are *data loaded at runtime*, not compiled into the binary.

**Won:** true hot-reload — save the file, the window repaints, no rebuild. This was the single feature we ranked highest for a research/iteration tool.

**Cost:** a typo in the template is a *runtime* error, not a compile error. We mitigate by surfacing parse errors as an overlay in the window rather than a crash, but we gave up the compiler's guarantees over the markup. Given hot-reload led, this was the right trade.

### Literal CSS over a cleaned-up dialect

**Decision:** real CSS property names (`padding` `border-radius`, `justify-content`), parsed by `lightningcss`.

**Won:** web knowledge transfers 1:1; no new vocabulary to learn; we don't write a CSS parser.

**Cost:** we inherit some of CSS's verbosity and quirks, and we must clearly document *which subset* we honor (see [the CSS subset](./02-spec.md#styling)) so authors aren't surprised when an obscure property is ignored.

### Two-tier logic: `rhai` script over a compiled Rust host

**Decision:** app glue lives in an interpreted, Rust-shaped script section that hot-reloads; native/heavy/fast work lives in compiled Rust exposed as `host::…`.

**Won:** logic hot-reloads along with markup and CSS — all three sections are live. And the script stays close to Rust (Law 4), so there's little new to learn.

**Cost:** `rhai` is Rust-*like*, not Rust — no borrow checker, dynamic types, interpreter overhead. The escape hatch: promote hot or heavy logic down into the compiled host when it stabilizes. See [the host contract](./02-spec.md#scripting-and-the-host).

### Reactivity is a core primitive; state management is ecosystem

**Decision:** the `signal` primitive and dependency-tracked `{{ }}` bindings are built into the runtime. Stores, routers, and persistence are *not* — they are ecosystem crates built on top of `signal`.

**Why the split:** these two pull apart and both are right. Fine-grained auto-update *cannot* be a pure library — something has to own the subscription primitive, exactly as Vue owns `ref`/`reactive` while Pinia and vue-router build on it. So the primitive is core; the *patterns* are userland. This keeps the
core tiny (Law 4) while leaving room for a real ecosystem.

### Gesture-honest events over mouse-first events

**Decision:** the event vocabulary is touch/gesture-first (`tap`, `longpress`, `drag`, `swipe`), and `hover` is a pointer-only capability that simply never fires on touch.

**Why:** browsers were built mouse-first and bolted touch on afterward, which is why touch on the web is full of hacks. Rux is device-first, so it doesn't inherit that debt. You may still bind `hover`; the runtime knows it's desktop-conditional and won't pretend otherwise.

## The element audit

The table that proves Law 3. Every commonly-requested "missing element" and where it actually goes:

| Requested | Resolution | Which law |
|---|---|---|
| `column`, `row`, `padding`, `center`, `spacer`, `scroll` | CSS on `<view>` | Law 1 |
| `a` / link | `role="link"` + `to=` (router is ecosystem) | Law 3 |
| `ul` / `ol` / `li` | `role="list"` + `r-for` loop + CSS `list-style` | Laws 1 & 3 |
| `label` | `role="label"` + `for=` | Law 3 |
| `form` | `role="form"` + `@submit` (validation in script) | Laws 2 & 3 |
| `select`, `option`, `textarea`, checkbox, radio | `<input type=…>` with options as **bound data** | Laws 3 & 4 |
| `table` | *deferred* — `display: grid` + row/col roles cover most; true data-grids are ecosystem | Law 1 |

The only case still open is rich tables and custom-templated option lists — both explicitly deferred, not because they're impossible but because building them now would pop the balloon (Law 4).

## What would make us revisit a law

- If runtime parse errors prove too costly in practice, we add an *optional* compile/validation step — without removing hot-reload.
- If `rhai`'s dynamism causes real bugs, we grow the compiled-host surface and shrink the script's responsibilities.
- If `role` proliferation starts encoding behavior by the back door, that's a Law-1 breach and the feature belongs in CSS or as a capability instead.

Next: the [formal spec](./02-spec.md).
