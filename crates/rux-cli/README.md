# Rux

A pure-Rust, web-flavored UI language that renders natively on the GPU, with no browser and no webview.

You write a single `.rux` file with familiar `<template>` / `<style>` /
`<script>` sections and **literal CSS**. Rux lays it out with a real flexbox and
grid engine, paints it in a native window, and reloads live as you edit.

```bash
cargo install ruxlang     # installs a `rux` command
rux run app.rux
```

```rux
<template>
  <screen class="app">
    <text class="count">{{ n }}</text>
    <button class="btn" @tap="n = n + 1">
      <text>Add one</text>
    </button>
  </screen>
</template>

<style>
  .app { display: flex; flex-direction: column; align-items: center; gap: 16px; padding: 32px; }
  .count { color: #a6e3a1; font-size: 48px; font-weight: 700; }
  .btn { padding: 12px 20px; background: #313244; border-radius: 8px; cursor: pointer; }
</style>

<script>
  let n = signal(0);
</script>
```

## The toolchain

Neither of these needs a window or a GPU, so both work in CI:

```bash
rux check .                   # path:line:col: severity: message
rux check --deny-warnings .   # warnings fail too
rux check --format json .     # for an editor
rux fmt .                     # format in place
rux fmt --check .             # verify only, exits non-zero
```

## What this is

The bet is one specific combination: the web's authoring ergonomics (literal CSS, a handful of HTML-like elements) but pure Rust, GPU-native, with no JavaScript and no new DSL. The guiding rule is that **layout never appears in markup**: there is no `<Column>`, `<Padding>` or `<Center>`, because those are `display: flex`, `padding` and `justify-content` on a `<view>`.

**Rux is 0.x and experimental.** If you need to ship an app this quarter, use Flutter, React Native or Slint. Rux exists to find out whether that corner is a nicer place to build.

- [ruxlang.dev](https://ruxlang.dev) · [try it in a browser](https://ruxlang.dev/playground/)
- [Learn](https://ruxlang.dev/learn/): build a task list from an empty file
- [Reference](https://ruxlang.dev/reference/): what actually works today, and the honest gaps
- [Blog](https://ruxlang.dev/blog/): release notes, written every release

## Licence

Dual licensed under [Apache-2.0](https://github.com/Aine-dickson/rux/blob/main/LICENSE-APACHE) or [MIT](https://github.com/Aine-dickson/rux/blob/main/LICENSE-MIT), at your option.
