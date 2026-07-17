+++
title = "Why Rux?"
description = "The one bet Rux is making, the four laws that follow from it, and an honest account of where Rux is not novel."
template = "page.html"
weight = 1
+++

Native UI toolkits keep asking you to learn a new way to say "put this next to
that." The web already has one, and a lot of people already know it: CSS. Rux's
whole bet is that you can keep that authoring model and drop everything else the
browser brings with it.

> **The web's authoring ergonomics — literal CSS, a handful of HTML-like elements
> — but pure Rust, GPU-native, with no JavaScript and no new DSL.**

That sentence is the product. Everything below is either a consequence of it or an
honest admission about it.

## Law 1: layout never appears in markup

This is the load-bearing rule, and the one most likely to change how your code
looks. **Markup says what content *is*. CSS says where it goes and how it looks.**

There is no `<Column>`, no `<Row>`, no `<Padding>`, no `<Center>`, no `<Spacer>`,
no `<SizedBox>`. Those aren't elements — they're `display: flex`,
`flex-direction`, `padding`, `justify-content`, and `gap` on a plain `<view>`.

A widget-tree toolkit makes you build this to center a padded column of two
things:

```
Center(
  child: Padding(
    padding: EdgeInsets.all(24),
    child: Column(
      mainAxisAlignment: MainAxisAlignment.center,
      children: [
        Text("Hello"),
        SizedBox(height: 16),
        Text("World"),
      ],
    ),
  ),
)
```

Rux makes you write this:

```rux
<view class="stack">
  <text>Hello</text>
  <text>World</text>
</view>
```
```css
.stack {
  display: flex;
  flex-direction: column;
  justify-content: center;
  gap: 16px;
  padding: 24px;
}
```

The markup is the *content* — two pieces of text — and it stays that way no matter
how the design changes. Restyling doesn't re-shape the tree. That's the ceremony
Law 1 is meant to kill, and it's the main thing Rux is actually claiming.

## The other three laws

**Law 2 — capabilities, not widgets.** Elements emit a fixed set of events; you
bind the ones you need. There's no sprawling widget catalogue to learn, and no
`FancyButton` that's just a button with opinions.

**Law 3 — few elements, `role=` for meaning.** Six elements (`screen`, `view`,
`text`, `image`, `button`, `input`) cover the ground. Semantics come from `role=`,
never from layout.

**Law 4 — stay close to Rust.** Rux is glue over the best crates in the ecosystem
rather than a from-scratch engine: [taffy] for layout, [parley] for text, [vello]
for painting, [lightningcss] for CSS, [rhai] for script, Leptos-style signals for
state. Fewer things to reinvent, and each one is maintained by people who care
about it more than we could.

## Where Rux is not novel

This deserves saying plainly, because it's the first thing an experienced person
will notice.

**"GPU-rendered UI without a browser" is not a new idea.** Flutter does it. Slint
does it. Lynx does it. If you came here expecting a novel rendering architecture,
there isn't one — Rux composes existing Rust crates, and every one of those
engines is more mature, more tested, and more complete than Rux is or will be for
a long time.

The claim is narrower, and it's entirely about *ergonomics*:

|            | authoring model                         | language | renderer            |
|------------|-----------------------------------------|----------|---------------------|
| **Rux**    | **literal CSS + 6 elements**            | **Rust** | own, GPU ([vello])  |
| Flutter    | widget tree (layout *is* nested widgets) | Dart     | own, GPU            |
| React Native | React tree → native widgets            | JS/TS    | platform widgets    |
| Slint      | its own `.slint` DSL (QML-like)          | Rust/C++/JS | own, GPU/software |
| Lynx       | web-like, multi-framework                | JS       | own                 |

Read the first column, not the last one. Every row but Rux asks you to learn a
layout system that exists only in that tool. Rux asks you to use the one you
already know. **That's the whole difference, and it's a bet, not a proven win.**

## When you should not use Rux

- **You need to ship a real app soon.** Use Flutter, React Native, or Slint. Rux
  is `0.x`; it has no accessibility story yet, no mobile backend yet, and its CSS
  support has real holes (variables, `@media`, `:hover`).
- **You need a rich widget catalogue.** There are six elements. Everything else you
  build.
- **You want native platform look-and-feel.** Rux draws its own pixels; it doesn't
  wrap platform widgets.
- **You need text selection across arbitrary content.** Selection works inside
  inputs, not across the whole document.

## When Rux might be for you

- You write Rust, and every UI option asks you to leave it.
- You already know CSS and resent re-learning layout per toolkit.
- You want hot reload without a JS runtime in the process.
- You find the idea interesting enough to poke at something early.

## The honest status

Rux runs real windows today: flexbox and grid, the box model, gradients, shadows,
transforms, fonts, inputs with a caret and selection, scrolling with scrollbars,
images, components, and live hot reload. It is also missing things you would
reasonably expect, and the gaps are written down as plainly as the wins in
[`05-as-built.md`][as-built] and [`06-roadmap.md`][roadmap].

The largest known gap is internal: **a signal change rebuilds the whole tree.**
It's imperceptible at these sizes, but it forces every piece of ephemeral state
(the caret, the selection, scroll offsets) to be restored by hand afterwards. Fine
grained reactivity is the next real piece of work.

[taffy]: https://github.com/DioxusLabs/taffy
[parley]: https://github.com/linebender/parley
[vello]: https://github.com/linebender/vello
[lightningcss]: https://lightningcss.dev/
[rhai]: https://rhai.rs/
[as-built]: https://github.com/Aine-dickson/rux/blob/main/docs/05-as-built.md
[roadmap]: https://github.com/Aine-dickson/rux/blob/main/docs/06-roadmap.md
