+++
title = "Rux"
# The top-level pages (why, how-it-works) carry a `weight`, not a date. Sorting
# the root section by date would drop them from `section.pages` and warn.
sort_by = "weight"
template = "index.html"

[extra]
headline = "Write a native app the way you write a web page."
lede = "Rux is a pure-Rust UI language: familiar template / style / script sections and literal CSS, laid out by a real flexbox and grid engine and painted on the GPU. No browser. No webview. No JavaScript."
note = "Early and experimental (0.x) · Apache-2.0 or MIT · desktop today, mobile next"
shot_caption = "examples/showcase.rux, running in a native window"
+++

## The whole app, in one file

That window is this file, all of it. No layout widgets, no JSX, no `.slint` DSL,
no JavaScript. `<template>` says *what the content is*, `<style>` is literal CSS
that says *where it goes and how it looks*, `<script>` holds the state.

```rux
<template>
  <screen class="app">
    <view class="card">
      <text class="eyebrow">RUX</text>
      <text class="title">Hello, {{ name }}</text>

      <input class="field" r-model="name" placeholder="your name" />

      <view class="row">
        <button class="btn" @tap="taps = taps + 1">
          <text class="btn-label">Tap me</text>
        </button>
        <text class="count">{{ taps }}</text>
      </view>

      <view class="tags">
        <view class="tag" r-for="t in tags">
          <text class="tag-label">{{ t }}</text>
        </view>
      </view>
    </view>
  </screen>
</template>

<style>
  .app  { display: flex; justify-content: center; padding: 28px; background: #181825; }
  .card {
    display: flex;
    flex-direction: column;
    gap: 14px;
    width: 100%;
    padding: 24px;
    background: linear-gradient(160deg, #313244, #1e1e2e);
    border-radius: 16px;
    box-shadow: 0 18px 40px rgba(0, 0, 0, 0.45);
  }
  .title { color: #cdd6f4; font-size: 28px; font-weight: 700; }
  .btn   { padding: 10px 18px; background: #89b4fa; border-radius: 10px; cursor: pointer; }
  .count { color: #a6e3a1; font-size: 28px; font-weight: 700; }
  .tags  { display: flex; flex-wrap: wrap; gap: 8px; }
  .tag   { padding: 5px 11px; background: #45475a; border-radius: 999px; }
</style>

<script>
  let name = signal("world");
  let taps = signal(0);
  let tags = signal(["no browser", "no JS", "literal CSS", "hot reload"]);
</script>
```

Type in the field and the heading changes as you type. `r-model` binds it to the
signal, and `{{ name }}` reads it back. Tap the button and the count moves. Edit
the file with the window open and it reloads live.

[The full file is `examples/showcase.rux`](https://github.com/Aine-dickson/rux/blob/main/examples/showcase.rux)
(the CSS above is trimmed a little for length).

## Run it

```bash
cargo install ruxlang
rux run app.rux
```

That installs a `rux` command from crates.io and needs no clone. To run the
example above and the rest of them, clone the repo instead:

```bash
git clone https://github.com/Aine-dickson/rux
cd rux
cargo run -p ruxlang -- examples/showcase.rux
```

Either way it opens a real native window. There is no browser and no webview anywhere in
the stack: [taffy] lays it out, [parley] shapes the text, [vello] paints it on the
GPU, [winit] holds the window.

## What works today

<ul class="chips">
  <li>flexbox + CSS grid</li>
  <li>the full box model</li>
  <li>px / % / rem / vw / vh</li>
  <li>gradients</li>
  <li>box-shadow</li>
  <li>transform</li>
  <li>fonts + text shaping</li>
  <li>text inputs + caret</li>
  <li>selection + clipboard</li>
  <li>select / textarea</li>
  <li>checkbox / radio</li>
  <li>keyboard focus + Tab</li>
  <li>scrolling + scrollbars</li>
  <li>images</li>
  <li>signals &amp; bindings</li>
  <li>fine-grained updates</li>
  <li>r-for / r-if / r-model</li>
  <li>:class / :style bindings</li>
  <li>:hover / :focus / :checked</li>
  <li>custom properties + var()</li>
  <li>@media queries</li>
  <li>components + slots</li>
  <li>component events</li>
  <li>a router</li>
  <li>keyed r-for</li>
  <li>computed + effects</li>
  <li>external stylesheets</li>
  <li>hot reload</li>
  <li>dev overlay</li>
  <li>accessibility tree</li>
  <li>HiDPI</li>
  <li>runs in a browser</li>
  <li>rux fmt</li>
  <li>rux check</li>
  <li>soft keyboard + IME</li>
  <li>transitions + enter/leave</li>
  <li>route transitions</li>
  <li>touch gestures</li>
  <li>SVG paths + morphing</li>
  <li>mounted / unmounted</li>
  <li>nested routes + guards</li>
  <li>element access from script</li>
  <li>position: sticky / fixed</li>
</ul>

The exact honored-CSS set lives in [the reference](@/reference/_index.md), the
authoritative "what actually works" doc. What's missing is written down just as
plainly. The biggest gap is **true inline text flow**: two `<text>` elements
cannot share a line, so bold inside a sentence is not expressible. After that:
`r-for` gives you the item but no index, a closure passed to a *method* cannot
capture the surrounding scope, there are no promises and no async anything, the
router is desktop-only until `rux build` can bundle components for the web, and
text editing lacks word-wise movement and triple-click selection.

> **Rux is 0.x and experimental.** It is not trying to replace Flutter, React
> Native, or Slint. Those are mature, and if you need to ship an app this
> quarter you should use one of them. Rux exists to find out whether one specific
> corner (CSS-authored, Rust-native, no DSL, no JS) is a nicer place to build.
> [The honest version of that argument, including where Rux isn't novel, is
> here.](@/why.md)

## Read on

- [**Learn**](@/learn/_index.md): build a task list from an empty file, in
  about half an hour. Start here if you want to write some Rux.
- [**Reference**](@/reference/_index.md): the authoritative list of what works:
  honored CSS, elements, directives, and the honest gaps.
- [**Why Rux?**](@/why.md): the thesis, the four laws, and where Rux genuinely
  isn't novel.
- [**How it works**](@/how-it-works.md): the pipeline from `.rux` file to pixels.
- [**Roadmap**](@/roadmap/_index.md): what's next, and what is deliberately not
  being built.
- [**Contribute**](@/contribute/_index.md): the architecture tour, for anyone
  who wants to work on the runtime itself.
- [**Blog**](@/blog/_index.md): release notes, written every release.

[taffy]: https://github.com/DioxusLabs/taffy
[parley]: https://github.com/linebender/parley
[vello]: https://github.com/linebender/vello
[winit]: https://github.com/rust-windowing/winit
[as-built]: https://github.com/Aine-dickson/rux/blob/main/docs/05-as-built.md
