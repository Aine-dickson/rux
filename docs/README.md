# Rux

A pure-Rust UI language for devices. You author screens the way you author for the web — semantic markup, literal CSS, a script section — but nothing here is a browser. Rux renders natively (Rust → GPU) and targets desktop first, then mobile and embedded from the same `.rux` file.

Rux exists because of one frustration: in widget-tree toolkits like Flutter, spacing, centering, and scrolling are *objects you nest*. In Rux — as on the web — they are *properties you set*. See the [rationale](./01-rationale.md) for the laws that follow from that.

## The 60-second picture

```xml
<!-- battery.rux -->
<template>
  <view role="section" class="card">
    <text role="paragraph" class="label">Battery</text>
    <text class="value">{{ level }}%</text>
    <button class="btn" @tap="level = host::read_battery()">Refresh</button>
  </view>
</template>

<style>
    .card  { 
        display: flex; 
        flex-direction: column; 
        align-items: center;
        gap: 8px; 
        padding: 16px; 
        background: #1e1e2e; border-radius: 12px; 
    }
    .label { color: #9399b2; font-size: 14px; }
    .value { color: #a6e3a1; font-size: 28px; font-weight: 600; }
</style>

<script>
  // State changes go inline in the handler: rhai `fn`s cannot mutate globals.
  // Script `fn`s are pure; heavy work lives behind `host::`. See 05 — As Built.
  let level = signal(82);
</script>
```

Three sections, six element types, real CSS, gesture events, signals. No layout wrappers. Edit the file and it repaints live.

## The docs

| Doc | Read it for |
|---|---|
| [01 — Rationale](./01-rationale.md) | *Why* Rux is shaped this way — the laws and the tradeoffs we accepted. Start here to understand the constraints before proposing changes. |
| [02 — Spec](./02-spec.md) | *What* Rux is — the formal reference for the SFC grammar, elements, roles, directives, events, CSS subset, script/host contract, and reactivity. The source of truth we architect and build against. |
| [03 — Guide](./03-guide.md) | *How* to build with Rux — a tutorial that assembles a small app screen by screen and validates the developer experience. |
| [04 — Architecture](./04-architecture.md) | *How the runtime works* — the parse→cascade→reactive→layout→paint pipeline, crate layout, the milestone plan, and open questions. The plan for building it. |
| **[05 — As Built](./05-as-built.md)** | **What actually works today** — running it, honored CSS, gotchas, and gaps. Authoritative where it contradicts 01–04. Start here if you're writing `.rux` code. |
| [06 — Roadmap](./06-roadmap.md) | *What's next* — the v0.1 shake-down, then the v0.2 spine (fine-grained reactivity first, and why). Start here if you're picking the work up. |

## Status

> ⚠️ **The runtime is BUILT (M0–M9 complete), plus scrolling, images, a real
> input caret, checkbox/radio, opacity, and the full flex model.** Docs 01–04
> below describe the original *design intent* and have **drifted from the
> implementation** in places (notably: rhai functions can't mutate state, the
> inline/block model was removed, grid was added). For **what actually works
> today**, read **[05 — As Built](./05-as-built.md)** — where they disagree, it
> wins. For **what's next**, read **[06 — Roadmap](./06-roadmap.md)**.
>
> Renderer: **vello 0.9** / **parley 0.11** / **taffy 0.7** / **rhai** /
> **lightningcss**.

The intended pipeline (this is what got built):

```
.rux file ──► parse template (XML) + style (lightningcss) + script (rhai)
                       │
   file watcher ──►  cascade ──► taffy (layout) ──► painter (vello/wgpu)
        ▲                                                    │
        └──────────────── repaint on change ─────────────────┘   (winit window)
```

Only the compiled **host** (native Rust capabilities) needs a rebuild; template, style, and script all hot-reload.

## Glossary quick-reference

- **SFC** — single-file component: one `.rux` file, three sections.
- **Host** — the compiled-Rust side that exposes native capabilities as `host::…`.
- **Signal** — a reactive value; a binding to it *is* a subscription.
- **Role** — a semantic/accessibility label on an element; never affects layout.
- **Directive** — an `r-`-prefixed structural attribute (`r-for`, `r-if`, …).
