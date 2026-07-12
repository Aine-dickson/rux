# 03 — Guide

Learn Rux by building a small screen: a **device dashboard** that lists devices, shows a battery card, filters with a search box, and adds a device through a form. By the end you'll have touched every part of the language.

This is a design-stage tutorial — the runtime isn't built yet, so treat the code as the *authoring experience we're committing to*. If something here feels awkward to write, that's a signal to fix the [spec](./02-spec.md) before we build.

New to the ideas? Skim the [rationale](./01-rationale.md) first — especially that **layout is CSS, never markup**.

## Contents

1. [Hello screen](#1-hello-screen)
2. [Styling with literal CSS](#2-styling-with-literal-css)
3. [State with signals](#3-state-with-signals)
4. [Events](#4-events)
5. [Lists with `r-for`](#5-lists-with-r-for)
6. [Conditionals](#6-conditionals)
7. [Forms and inputs](#7-forms-and-inputs)
8. [Talking to the host](#8-talking-to-the-host)
9. [Splitting into components](#9-splitting-into-components)
10. [The finished screen](#10-the-finished-screen)

---

## 1. Hello screen

Every app starts at a `<screen>`. Create `app.rux`:

```xml
<template>
  <screen>
    <text>Hello device</text>
  </screen>
</template>
```

`<screen>` is the root of one view (like `<body>`). `<text>` is a text run.
That's a complete, runnable component. Save it and the window shows the text — no build step for the template.

## 2. Styling with literal CSS

There are no layout elements. To center that text in a padded card, you add a `<view>` for structure and do all the spacing in `<style>`:

```xml
<template>
  <screen>
    <view class="card">
      <text class="title">Hello device</text>
    </view>
  </screen>
</template>

<style>
  .card {
    display: flex;
    flex-direction: column;
    align-items: center;
    padding: 24px;
    background: #1e1e2e;
    border-radius: 12px;
  }
  .title { color: #cdd6f4; font-size: 24px; }
</style>
```

If you know CSS, you already know this — the property names are identical. Notice what you *didn't* write: no `<Padding>`, no `<Center>`, no `<Column>`. `padding`, `align-items`, and `flex-direction` did all three. That's [Law 1](./01-rationale.md#law-1--content-vs-spatial).

## 3. State with signals

Dynamic values are **signals**. Declare them in `<script>` (Rust-shaped `rhai`) and interpolate with `{{ }}`:

```xml
<template>
    <screen>
        <view class="card">
            <text class="title">Battery</text>
            <text class="value">{{ level }}%</text>
        </view>
    </screen>
</template>

<script>
  let level = signal(82);
</script>
```

`{{ level }}` doesn't just print once — the binding *subscribes* to `level`. When `level` changes, only that text node repaints ([reactivity](./02-spec.md#reactivity)).

## 4. Events

Elements emit only the events in their [capability set](./02-spec.md#events). A `<button>` can `tap`; bind a handler with `@tap`:

```xml
<template>
    <screen>
        <view class="card">
            <text class="value">{{ level }}%</text>
            <button class="btn" @tap="drain()">Drain 1%</button>
        </view>
    </screen>
</template>

<script>
  let level = signal(82);
  fn drain() { level.update(|v| v - 1); }
</script>
```

Tapping the button runs `drain`, which updates the signal, which repaints the value. You bound a capability the button already had — you didn't reach for a new widget ([Law 2](./01-rationale.md#law-2--capabilities-not-widgets)).

## 5. Lists with `r-for`

A list is not a `<list>` element. It's a `role="list"` container plus a **loop** over data — repetition is data, not markup:

```xml
<template>
    <view role="list" class="feed">
        <view role="listitem" class="tile" r-for="d in devices" :key="d.id">
            <text role="heading">{{ d.name }}</text>
            <text role="paragraph">{{ d.battery }}%</text>
        </view>
  </view>
</template>

<style>
    .feed { 
        display: flex; 
        flex-direction: 
        column; gap: 8px;
        overflow-y: auto; /* scrolling is CSS, not a widget */ 
    }        
    .tile { 
        padding: 12px; 
        background: #313244; 
        border-radius: 8px; 
    }
</style>

<script>
  let devices = signal([
    #{ id: 1, name: "Thermostat", battery: 82 },
    #{ id: 2, name: "Door Lock",  battery: 47 },
    #{ id: 3, name: "Camera",     battery: 12 },
  ]);
</script>
```

To make the list scroll, you gave `.feed` `overflow-y: auto`. No `<SingleChildScrollView>`, no `<ListView>` — just CSS. `:key="d.id"` gives each row a stable identity for correct updates on reorder.

## 6. Conditionals

Show something only when a condition holds with `r-if` / `r-else`:

```xml
    <view role="listitem" class="tile" r-for="d in devices" :key="d.id">
        <text role="heading">{{ d.name }}</text>
        <text role="paragraph" r-if="d.battery < 20" class="warn">Low: {{ d.battery }}%</text>
        <text role="paragraph" r-else>{{ d.battery }}%</text>
    </view>
```

`r-if` adds/removes the node from the tree. Use `r-show="expr"` instead when you want to keep the layout slot and only toggle visibility.

## 7. Forms and inputs

Every control is `<input type=…>` — there's no `<select>` or `<textarea>`. A form is `role="form"` with `@submit`. Two-way binding is `r-model`:

```xml
<template>
    <view role="form" class="add" @submit="add()">
        <text role="label" for="name">Name</text>
        <input id="name" type="text" r-model="draft.name" placeholder="Device name" />

        <text role="label" for="kind">Type</text>
        <input id="kind" type="select" r-model="draft.kind" :options="kinds" />

        <button class="btn" @tap="add()">Add device</button>
    </view>
</template>

<script>
  let kinds = signal(["Sensor", "Lock", "Camera"]);
  let draft = signal(#{ name: "", kind: "Sensor" });

  fn add() {
    devices.update(|list| list.push(#{
      id: next_id(), name: draft.get().name, battery: 100,
    }));
    draft.set(#{ name: "", kind: "Sensor" });
  }
</script>
```

The `select`'s choices come from the bound `:options="kinds"` collection — no `<option>` tags. On a phone, this `select` opens the **native picker**. The `for="name"` on the label associates it with the input (tap-to-focus), and it doesn't collide with `r-for` because loops are `r-`-prefixed.

## 8. Talking to the host

Signals hold app state, but reading a real battery, hitting a database, or navigating needs **native capability** — that's the compiled Rust `host`. The script calls registered `host::` functions:

```rust
<script>
  let devices = signal(host::load_devices());   // from the host, at startup

  fn refresh() {
    devices.set(host::load_devices());
  }

  fn open(d) {
    host::open(d.id);                            // native navigation
  }
</script>
```

`host::load_devices` and `host::open` are registered on the compiled side (the [host contract](./02-spec.md#scripting-and-the-host)). Editing the *script* above hot-reloads; adding a *new* host function is the one thing that needs a rebuild.

## 9. Splitting into components

A `.rux` file is a component. Extract the tile into `components/device_tile.rux`:

```xml
<!-- components/device_tile.rux -->
<template>
    <view role="listitem" class="tile">
            <text role="heading">{{ device.name }}</text>
            <text role="paragraph">{{ device.battery }}%</text>
    </view>
</template>

<style>
  .tile { padding: 12px; background: #313244; border-radius: 8px; }
</style>
```

Import it Rust-style and embed it as `<device-tile>`, passing the prop as an attribute:

```xml
<script>
  use components::device_tile;
</script>

<template>
    <view role="list" class="feed">
        <device-tile r-for="d in devices" :key="d.id" :device="d" @tap="open(d)" />
    </view>
</template>
```

`:device="d"` binds the parent's `d` into the child's `device` prop, one-way. The child raises `tap`; the parent handles it with `@tap`.

## 10. The finished screen

Putting it together — `app.rux`:

```xml
<script>
  use components::device_tile;

  let devices = signal(host::load_devices());
  let kinds   = signal(["Sensor", "Lock", "Camera"]);
  let draft   = signal(#{ name: "", kind: "Sensor" });
  let query   = signal("");

  fn shown() {
    devices.get().filter(|d| d.name.contains(query.get()))
  }

  fn add() {
    devices.update(|list| list.push(#{
      id: next_id(), name: draft.get().name, battery: 100,
    }));
    draft.set(#{ name: "", kind: "Sensor" });
  }

  fn open(d) { host::open(d.id); }
</script>

<template>
    <screen>
        <view role="header" class="bar">
            <text role="heading" class="h1">Devices</text>
            <input type="text" r-model="query" placeholder="Search…" />
        </view>

        <view role="list" class="feed">
            <text role="paragraph" r-if="shown().len() == 0">No devices match.</text>
            <device-tile r-for="d in shown()" :key="d.id" :device="d" @tap="open(d)" />
        </view>

        <view role="form" class="add" @submit="add()">
            <input type="text"   r-model="draft.name" placeholder="Device name" />
            <input type="select" r-model="draft.kind" :options="kinds" />
            <button class="btn" @tap="add()">Add</button>
        </view>
    </screen>
</template>

<style>
  screen{ 
    display: flex; 
    flex-direction: column; 
    height: 100%;
    background: #11111b; 
}
  .bar { display: flex; align-items: center; gap: 12px; padding: 16px; }
  .h1 { color: #cdd6f4; font-size: 20px; font-weight: 600; }
  .feed { display: flex; flex-direction: column; gap: 8px; padding: 16px; overflow-y: auto; flex: 1; }
  .add { display: flex; gap: 8px; padding: 16px; }
  .btn { padding: 8px 16px; background: #89b4fa; color: #11111b; border-radius: 8px; }
  .warn { color: #f38ba8; }
</style>
```

Everything you learned, on one screen:

- **Six elements**, structured with `<view>` + `role`.
- **All layout in CSS** — flex, gap, padding, `overflow-y: auto` for scroll, `flex: 1` to make the feed fill space. No layout widgets anywhere.
- **Signals** (`devices`, `query`, `draft`) driving the view; a derived `shown()` filtering live as you type.
- **Directives** — `r-for`, `r-if`, `r-model`, `:key`, `:device`.
- **Events** — `@tap`, `@submit`.
- **`<input type=>`** covering text and select, options as data.
- **A component** (`device-tile`) imported Rust-style and embedded.
- **The host** for the one thing script can't do itself (`load_devices`, `open`).

## Where to go next

- The precise rules behind anything above: the [spec](./02-spec.md).
- Why it's shaped this way, and what's deferred: the [rationale](./01-rationale.md).
- Building the runtime that makes this file render is the next project phase —
  the pipeline is sketched in the [README](./README.md#status).
