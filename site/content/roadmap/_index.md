+++
title = "Roadmap"
description = "Where Rux goes next: milestones, the release cadence, and what is deliberately not being built."
weight = 4
sort_by = "weight"
template = "docs-section.html"
page_template = "docs.html"
+++

<!-- GENERATED FROM docs/06-roadmap.md BY site/sync-docs.sh. DO NOT EDIT HERE. -->


Where Rux goes next. For what works today, read [As Built](/reference/).
This document is only about what is **not done yet**, and in what order. The
milestones that are finished (v0.1 to v0.8), the tooling tiers, the Icons and
Tailwind arguments and the unplanned-surface audit are in
[the roadmap history](./archive/roadmap-history.md).

## Release cadence

Every release is a git tag and a blog post. No post, no tag.

A milestone (v0.9, v0.10, ...) spans several Friday point-releases, and each
Friday ships whatever coherent slice has moved. The slices are not numbered or
scoped in advance; what ships is decided by how far the milestone got that
week, and it has to stand up on its own and be worth a post. A new minor opens
only once the previous milestone's whole scope is done.

## Where we are

The runtime, the script tier, the CLI, the Android and web targets, the editor
extension and the site are built and driven in a window, and `cargo test
--workspace` passes. A feature is not done until it has been driven in the
window: tests protect against regression and do not show that the thing works.

**v0.9 stands as follows.** Items 1 to 6, 8 (audio, and video on Windows,
Android and the web) and 10 are done. Built beside them, not on the original
list: the file input and `pickFile`, Tailwind directives in `<style>`, network,
`data:` and SVG images, `@key`, settings, native signals and channels, `now()`,
`async fn` handles, and `<icon>`. **Still open:** item 7 (physics), item 9
(live regions), item 11 (pinch and rotate, and the `rux doctor` device survey,
both waiting on a phone), `hyphens: auto`, video on macOS and Linux, and the
0.9 publish itself, which has to include `rux-native`, `rux-native-macros` and
`rux-bindgen` before a scaffolded `native/` crate can build outside a
checkout. [As Built](/reference/) is the record of what each built item
does.

---

## v0.9: what remains

1. **Physics.** Kinetic scrolling is built. The rest is momentum,
   rubber-banding and spring easing. The animator has two drivers, `Clock` and
   `Bound`, and a spring is a third, so the architecture takes it without a
   redesign.
2. **Live regions.** The accessibility tree is published, and nothing can
   announce a change through it, so a screen-reader user gets no notice of a
   toast, a validation message or a list that finished loading. Related and
   also open: accesskit action requests (a screen reader asking to click or
   focus) are received and not dispatched, and the tree is flat under the
   window, with no landmarks.
3. **Pinch and rotate.** Touch reporting carries every finger and `@drag` was
   driven with three; nothing yet turns two fingers into a pinch or a rotate.
   Waits for a phone to be judged on.
4. **The `rux doctor` device survey.** `rux doctor` reports the toolchain, not
   the attached device. A phone that is present, offers ADB and cannot be
   claimed by adb (a generic WinUSB binding with no ADB interface GUID, found
   on Windows; a missing udev rule on Linux) looks identical to "nothing is
   plugged in". The survey has to separate those cases and name the repair,
   and to be written so a second platform is another case rather than a second
   implementation. Wireless debugging belongs in the same output. Waits for a
   phone.
5. **`hyphens: auto`.** Not done, by the owner's decision. parley draws no
   hyphen where a line breaks at a soft hyphen, so even `hyphens: manual`
   needs that first; `auto` then needs a hyphenation dictionary per language (a
   new dependency, about 1 MB each) and a `lang` attribute, which Rux does not
   have.
6. **Video on macOS and Linux.** `<video>` plays on Windows, Android and the
   web; elsewhere `@error` fires with `kind` `"unsupported"`.
7. **The 0.9 publish**, including `rux-native`, `rux-native-macros` and
   `rux-bindgen`.
8. **Child windows: built 2026-11-06** (`window.open`, see `07-script`). Driven
   on Windows only; macOS and Linux are not driven, and Android and the web
   have no child windows.

## CSS left out, each needing a decision

- Table properties need tables.
- `user-select` has nothing to act on: static text outside a field cannot be
  selected.
- `writing-mode`: vertical text is a layout model of its own.
- `column-span` (the rest of `columns` is built).
- `hyphens: auto`, above.

## v0.10: data, and the outside world

Async was placed before the component set on purpose: it is the last big
API-shaping change, it moves contracts a component library would sit on, and 1.0
means the API stops breaking, so the largest remaining break wants slack in
front of it. Built so far from this milestone: `async fn` handles, tasks, a
settings store (`settings.get`, `set`, `remove`), `pickFile`, and network
images. Open:

1. **Error boundaries.** A failing expression is reported and the document
   carries on; an author cannot catch one and show a fallback for that subtree.
   Once a document talks to a network, the common failure is no longer a typo.
2. **A capability model for native code.** Compiled app code has full trust.
   The interpreter is the sandbox for anything untrusted (the playground,
   fetched or over-the-air documents), with step and memory limits and no
   native access unless granted. A native module declaring the platform
   permissions it needs, from which the Android manifest is generated, is not
   built; it waits for the first mobile API that needs one. The default an
   unmarked file gets is a 1.0 question, because it is the promise being
   frozen.
3. **Whatever else "the outside world" turns out to mean** once the above is
   in use. File dialogs other than the file picker are not built.

## v0.11: authoring

1. **`rux-lsp`.** Go-to-definition, hover, completion and `rux check`'s
   diagnostics in the editor. It never needed a frozen language; diagnostics
   exist as `rux check` and the extension is published.
2. **A component set, with flavours.** The components page carries a switch
   (or a tab) so a reader picks the styling and takes that version; shadcn and
   flowbite are the reference points. Tailwind is one flavour next to plain
   Rux CSS, so it is a choice made by the reader and not by the project.
   Building the set is also the test the language has not had: it uses every
   feature together at the scale someone else would, and it surfaces the
   patterns that should change before 1.0 freezes them. Icons, `z-index`,
   anchor positioning and the CSS pass it needed are built.

## iOS: blocked on hardware, not on a date

Every step after compiling is Apple-only: the SDK, the linker, `codesign`,
`xcodebuild`, the Simulator and the `.ipa` all run on macOS. A cloud macOS
runner is the only path with no hardware, and it conflicts with how this
project decides something works (defects were repeatedly invisible to the suite
and obvious within seconds of using the app, so shipping a platform from CI
that nobody has used is not acceptable). iOS is therefore gated on a Mac. It
will owe a second packaging pipeline (signing, provisioning profiles,
entitlements), an answer to the UIKit equivalent of the one Java class an
Android app carries, and App Store distribution. Nothing in the runtime blocks
it: winit and wgpu support iOS, the source provider abstracts the filesystem,
and the environment struct already carries safe areas, density and orientation.

## v1.0: freeze

1. **`rux build`, the rest.** What stays is what needs a language that has
   stopped moving: installer formats, an app bundle, and a platform matrix
   wider than the one machine Rux is developed on.
2. **`rux-lsp`**, if it has not landed in v0.11.
3. **Harden native code against the capability model** designed in v0.10.
4. **Close the author-notes register.** [`09-author-notes.md`](./09-author-notes.md)
   is behaviour that is correct and still surprises. Every entry is a 1.0
   blocker; closing a row means writing the sentence, not deciding it is
   obvious.
5. **A test story for app authors.** The runtime has many test binaries;
   somebody writing a `.rux` file has nothing. "How do I test this screen"
   needs a better answer than "open it and look".
6. **Remove the synthetic `checked` class** kept beside `:checked` for
   compatibility.

## Worth deciding early

- **`docs/03-guide.md` should be deleted.** It opens by saying it does not work
  as written and is excluded from the site build; `/learn` covers the same
  ground against the runtime that exists.

## Decisions that still bind

- **A feature is done when it has been driven in the window** (2026-07-15), and
  the driven cases are logged in [User tests](./08-user-tests.md).
- **Controlled state is an opt-in** (2026-07-18): uncontrolled defaults stay
  automatic and survive a rebuild; an author who wants to own a value binds it.
  See [Rationale](/reference/rationale/#ephemeral-ui-state-automatic-by-default-controllable-by-opt-in).
- **Match CSS** unless a divergence is written down. The flex cross-axis default
  is `stretch`, as in CSS (2026-09-18, reverted in v0.8 after a brief
  `flex-start` default).
- **Transitions are authored in CSS** (2026-08-18): `r-transition` marks the
  element and the two sides are `:enter-from` and `:leave-to`. No wrapper
  element and no convention-based class names. Enter and leave keep both
  branches live while a swap is pending, so a cancelled swipe can be reversed.
- **Paths are an element** (2026-08-19): geometry in `d` (`:d` to bind), the
  whole SVG grammar including arcs, paint in CSS.
- **Object literals are `{ }`.** `#{ }`, the spelling chosen on 2026-08-11, is still read, and `rux fmt` rewrites it.
- **`setTimeout` is declined** (the animation tier answers the cases that use
  it); a repeating timer is owned by the runtime and tied to the component
  instance, never a free-floating handle someone must clear.
- **Rux's own interpreter replaced rhai** (owner, 2026-09-26), and `host::` is
  retired in favour of `native` modules behind `#[rux::export]`.
- **Anchored overlays use CSS** (2026-10-13): `anchor-name`, `position-anchor`,
  `anchor()` and `anchor-size()` with `position: fixed`, not a portal element.
  `z-index` follows CSS's whole stacking-context order.
- **Packaging is designed once for web, desktop and mobile** (2026-08-19), with
  embedded a consideration and not a target. A Rux user must never be taken
  through Android Studio or Gradle (2026-08-18). A distribution that needs no
  Rust toolchain was dropped (2026-09-26).
- **The spec is not re-derived** (2026-08-25). [`02-spec.md`](/reference/spec/) is
  design history; [As Built](/reference/) and [Script](/reference/script/) are
  the reference, and a test fails if a name the editor offers is missing from
  them.
- **Async comes before the component set** (2026-08-19), and the author writes
  it, not a Rust programmer on their behalf.
- **Icons are Tabler** (2026-08-21), and a build embeds only the names a project
  writes. **Tailwind is a flavour of the component set** (2026-08-19), and its
  directives in `<style>` follow what Vite does (2026-10-26): the block is
  handed to the `tailwindcss` CLI, which is needed only when a directive is
  present.
- **A milestone closes when its scope is done**, and what is not ready moves to
  the next one rather than holding a release. v0.8 was closed by freezing it,
  with no work that was not v0.8's going in.
- **A touch fling's feel is judged in a release build**; a debug build
  under-flings because winit gives a touch event no timestamp.

## History

Finished milestones, the syntax-coloring and dev-tooling tiers, the Icons and
Tailwind arguments, the Known-ceilings pool and the unplanned-surface audit are
in [the roadmap history](./archive/roadmap-history.md).
