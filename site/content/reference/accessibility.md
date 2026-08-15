+++
title = "Accessibility"
description = "The real accessibility tree, the roles elements map to, and what a screen reader is told."
weight = 14
+++

<!-- GENERATED FROM docs/05-as-built.md BY site/sync-docs.sh. DO NOT EDIT HERE. -->


Rux publishes a real accessibility tree through **accesskit**, so a screen reader
(Narrator/UI Automation on Windows, AT-SPI on Linux, NSAccessibility on macOS)
can enumerate and describe the UI. Roles are resolved during the build, where the
tag and `type=` are still known:

| Markup | Role |
|---|---|
| `<text>` | Label (`role="heading"` → Heading) |
| `<view @tap>` / `<button>` | Button, **named by the text inside it** |
| `to="/path"` on anything | Link, so navigating is announced as going somewhere |
| `<input>` | TextInput · `type="textarea"` → MultilineTextInput · `type="select"` → ComboBox |
| `<input type="checkbox">` / `="radio"` | CheckBox / RadioButton, with live **checked** state |
| `<image alt="…">` | Image |
| a scrolling box | ScrollView |
| `role="…"` on anything | that role, overriding the implicit one |

**The accessible name** comes from, in order: an authored `label="…"` (or `alt=`
on an image), then a `<text for="…">` pointing at the element's `id`, then, for
inputs only, the `placeholder` as a last resort. A hint never outranks a real
label. Controls also expose their **value**, and the platform's focus follows the
focused input.

```xml
<text for="email">Email address</text>
<input id="email" r-model="email" placeholder="you@example.com" />
<!-- announces as: "Email address, edit" -->
```

Plain layout boxes are **not** exposed, a tree full of anonymous groups is worse
than a short one. `r-show="false"` elements are absent from the tree, not merely
invisible. The whole tree is rebuilt per frame but only published while assistive
technology is actually attached, so it costs nothing otherwise.

**Not done:** accesskit *action requests* (a screen reader asking to click or
focus an element) are received but not yet dispatched into the app; there is no
nesting/landmark structure (the tree is flat under the window); and live-region
announcements are unimplemented.
