# 06 — Roadmap

Where Rux goes next. Written 2026-07-15, on branch `build/m0-window` (7 commits
ahead of the last docs commit, **not pushed**).

For *what works today*, read [05 — As Built](./05-as-built.md). This document is
only about what is **not done yet**, and in what order.

---

## Where we are

The runtime works end to end and has now been **driven, not just tested**:
windows open, text lays out, inputs edit, lists scroll, images draw, files
hot-reload. 56 tests pass.

That last sentence matters more than the number. **Every real bug found in the
last few sessions was invisible to the test suite and obvious within seconds of
using the app:**

| Bug | Test suite said | The window said |
|---|---|---|
| Text re-wrapped and spilled over its siblings | green | last word of a line breaks and collides |
| A hugging box burst out through its parent | green | green boxes hanging out of the card |
| The caret stayed in the input you left | green | two carets on screen |
| The checkbox tick was a font glyph | green | reads as a letter, not a control mark |
| vello 0.9 renders Rgba8 into a Bgra8 surface | **compiled** | panic on launch |

**So the rule for v0.1 is: a feature is not done until it has been driven in the
window.** Tests protect against regression; they do not tell you the thing works.

---

## v0.1 — the shake-down (next up)

The goal is not new features. It is to make v0.1 mean something.

### 1. Make the examples worth testing
Nearly every example is fixed-width (`320px` cards, fields, lists), so **resizing
the window proves almost nothing**. Only `dashboard.rux` (a `1fr 1fr 1fr` grid)
actually re-flows.

- `list.rux` → responsive (`width: 100%; max-width: 520px`).
- `gallery.rux` → a `flex-wrap` grid of thumbnails, so content must re-flow.
- Keep one fixed-width example on purpose (`battery.rux`) as the control.

### 2. Drive every example against this checklist
Type and click around both fields · scroll the list, tap a row, scroll again ·
toggle the checkbox and both radios · resize wide · resize **very narrow** ·
minimize and restore · hot-reload each file with the window open · drag between
monitors of different DPI, if available.

### 3. Watch specifically for
- **Text escaping its box at narrow widths** — the wrap invariant (a text box is
  never narrower than the text measured) breaking under a new width.
- **Scroll offsets stranding content after a resize** — they are re-clamped per
  layout, but that path is untested against a *changing* viewport.
- **A panic on minimize** — the surface goes to zero; wgpu now reports occluded /
  timed-out frames as a status we skip, but that is unverified.
- **`ScaleFactorChanged`** — we handle `Resized` but not this. Layout reads the
  scale factor every frame so it *should* be fine. Unverified.
- **Any ephemeral UI state that does not survive a rebuild** (see below).

### 4. Then tag `v0.1`
Only once the above is clean.

---

## v0.2 — the spine

### 1. Fine-grained reactivity  ← *start here*
A signal write currently **rebuilds the whole tree**. It is correct, and at these
screen sizes you cannot perceive the cost — so it is tempting to file this as a
performance nicety and do it last. **That would be a mistake.**

The real cost is structural. Because we throw the tree away on every change, every
piece of *ephemeral UI state* has to be manually restored afterwards. We already
have two such hand-written passes:

- `apply_focus` in `rux-runtime` — puts the caret back.
- scroll offsets in `rux-shell` — keyed by the scroller's index in tree order.

**The stale-caret bug was one instance of this category, not a one-off.** Each new
piece of interactive state (selection, hover, drag, animation) adds another
restore-me-after-the-rebuild pass and another chance to get it wrong. Per-binding
subscriptions — which is what [04 — Architecture](./04-architecture.md) always
described — delete the *category*.

This is the only real divergence left between the architecture doc and the code.

### 2. Text selection + clipboard
Shift-arrows, drag-select, copy/paste/cut. parley 0.11 already models selection
(`PlainEditor`, `Selection`), so this is mostly wiring now that the upgrade is in.

### 3. The last two input types
`type="select"` and `type="textarea"` — the only elements the spec promises and
the runtime does not have. Also: make checkbox/radio keyboard-reachable (today
they are tap-only).

### 4. Scrolling polish
Scrollbars, drag/touch, keyboard (arrows/PageUp/Home), horizontal scrolling, and
scroll-into-view. Today it is wheel-only and vertical-only.

### 5. The CSS long tail
`box-shadow`, `position`/`top`/`left`, per-corner radius, per-side border
*colours*, and CSS variables (which would let the checked-state palette be
themeable rather than hard-coded per class).

---

## Known ceilings (not scheduled — they need a decision first)

- **True inline text flow.** Two `<text>` siblings stack; they cannot share a
  line. taffy has no inline formatting context, so this needs our own line-breaker
  over parley — a real project, not a patch.
- **Error surfacing.** Unknown CSS is silently ignored; a bad `.rux` file falls
  back to an empty screen. There is no dev overlay. Fine for a prototype, not for
  anyone else's hands.
- **`:checked` and other pseudo-classes.** Today a checked box gets a synthetic
  `checked` *class* — deliberately, to avoid new selector machinery. If pseudo-
  classes arrive (`:hover`, `:focus`, `:disabled`), that hack should be retired
  with them.
- **Accessibility.** `role=` is honored for selectors and semantics but is wired
  to nothing. parley 0.11 pulls in `accesskit`; that door is open.
