# Rux

A pure-Rust UI language for devices. You author screens the way you author for the web, with semantic markup, literal CSS and a script section, but nothing here is a browser. Rux renders natively (Rust → GPU) and targets desktop first, then mobile and embedded from the same `.rux` file.

Rux exists because of one frustration: in widget-tree toolkits like Flutter, spacing, centering, and scrolling are *objects you nest*. In Rux, as on the web, they are *properties you set*. See the [rationale](./01-rationale.md) for the laws that follow from that.

## The 60-second picture

```xml
<!-- battery.rux -->
<template>
  <view role="section" class="card">
        <text role="paragraph" class="label">Battery</text>
        <text class="value">{{ level }}%</text>
        <button class="btn" @tap="refresh()">Refresh</button>
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
  // A function sees the state around it and can write it, so a handler has a
  // name. Heavy work still lives behind `host::`. See 07, Script.
  let level = signal(82);

  fn refresh() { level = host::read_battery() }
</script>
```

Three sections, six element types, real CSS, gesture events, signals. No layout wrappers. Edit the file and it repaints live.

## The docs

| Doc | Read it for |
|---|---|
| [Rationale](./01-rationale.md) | *Why* Rux is shaped this way: the laws and the tradeoffs we accepted. Start here to understand the constraints before proposing changes. |
| [Design surface (v0.1)](./02-spec.md) | *What Rux was going to be*: the original v0.1 design, kept as history. **Not a reference, and parts of it are false.** Read for the reasoning, not for the language. |
| [Guide](./03-guide.md) | *How* to build with Rux: a tutorial that assembles a small app screen by screen and validates the developer experience. |
| [Architecture](./04-architecture.md) | *How the runtime works*: the parse→cascade→reactive→layout→paint pipeline, crate layout, the milestone plan, and open questions. The plan for building it. |
| **[As Built](./05-as-built.md)** | **The reference.** What actually works today: running it, honored CSS, gotchas, and gaps. Authoritative wherever anything else disagrees. Start here if you're writing `.rux` code. |
| **[Script](./07-script.md)** | **The script language in depth**: state, functions, values, the element API, and every way it differs from rhai and from JavaScript. Rux forks rhai, so rhai's own docs are no longer correct on their own. |
| [Types](./10-types.md) | *The type system, being built*: annotations, inference, unions and narrowing, and what is checked at run time. Decided in full; a status table at the top says which parts run yet. Until a part runs, [Script](./07-script.md) is the reference. |
| [Roadmap](./06-roadmap.md) | *What's next*: the v0.1 shake-down, v0.2 (inputs and polish), v0.3 (fine-grained reactivity). Start here if you're picking the work up. |
| [User test cases](./08-user-tests.md) | *What a person actually drove*, per feature and per release, on what hardware, and what those runs found. Every feature records its cases here; almost every expensive bug in Rux was found this way rather than by CI. |
| [Author notes](./09-author-notes.md) | *What is not a bug and has to be taught*: behaviour that is correct and still catches authors out, each tracked with the page it has to be explained on. Every entry is a v1.0 blocker. |

## Which document is the reference

**[As Built](./05-as-built.md) and [Script](./07-script.md), and only those
two.** Together they are the language as it exists. Every element, attribute,
directive, gesture, pseudo-class, script global and CSS property the editor
offers is checked to appear in them on every `cargo test`, so a feature cannot
ship undocumented the way `<path>` did in v0.7.

**Docs 01–04 are design records, not references.** They describe what Rux was
going to be, they have drifted from the implementation, and they are not
checked against anything. That is deliberate: two documents both claiming to
describe the language is how the drift happened, and the cure is one reference
rather than two that agree for a while.

> **The runtime is BUILT (M0–M9 complete)**, plus scrolling, images, a real
> input caret, checkbox/radio, opacity, and the full flex model. For **what's
> next**, read **[Roadmap](./06-roadmap.md)**.
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

- **SFC**: single-file component: one `.rux` file, three sections.
- **Host**: the compiled-Rust side that exposes native capabilities as `host::…`.
- **Signal**: a reactive value; a binding to it *is* a subscription.
- **Role**: a semantic/accessibility label on an element; never affects layout.
- **Directive**: an `r-`-prefixed structural attribute (`r-for`, `r-if`, …).
