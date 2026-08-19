+++
title = "Author notes"
description = "Behaviour that is correct and still surprises, tracked with the page each one has to be explained on. Every entry is a v1.0 blocker."
weight = 7
+++

<!-- GENERATED FROM docs/09-author-notes.md BY site/sync-docs.sh. DO NOT EDIT HERE. -->


**A register of things the runtime does correctly and an author will still get
wrong.** Each one is behaviour that is right, defensible, and surprising, which
means the fix is not a code change but a sentence in the documentation that does
not exist yet.

**Every entry here is a v1.0 blocker.** Not because the code is wrong, but
because shipping a language whose correct behaviour reliably surprises people is
the same defect wearing a different coat. A trap that is written down can be
taught; a trap that lives only in the head of whoever hit it last gets hit
again by everybody else.

## Why this file exists

These kept turning up as a by-product of hunting real bugs. Someone drives a
feature, something looks wrong, and the investigation ends with "the engine is
right, the example was wrong". That is a genuine finding and it used to
evaporate: the example got fixed, the session ended, and the *reason* went
nowhere. The next person to write that pattern hits the same wall.

The user's framing, 2026-08-19: a fix that does not belong to the language but
to the author's side is still something a developer needs to know when using the
feature, and before v1.0 all of them have to be handled in the official
developer documentation. So they get tracked, with a destination.

## How to use this file

One row per trap. What an author writes, what actually happens, why that is
right, and **where the explanation has to land**. A trap with no destination is
not finished being thought about.

`Status` is `open` until the destination page actually says it. Closing a row
means going and writing the sentence, not deciding it is obvious.

`/learn` is the tutorial and tracks the latest **release**, so a trap in an
unreleased feature waits for that release before its `/learn` half can land. The
reference half does not wait: `docs/05-as-built.md` describes the tip.

---

## Open

### Enter, leave and transitions

| Trap | What actually happens, and why it is right | Destination | Status |
|---|---|---|---|
| Two `r-if` / `r-else` branches authored to cross over **queue instead**, one below the other | Both branches are really on screen during a swap, so the place the `r-else` *would have had* is after the `r-if`, and a departing box with no inset keeps the place it would have had. Correct, and the consequence is not obvious. A crossover needs a shared positioned box with the leaver pinned to its origin. The `<router>` escapes it only because it builds the outgoing page first | `/reference/css/` enter-leave section, and a `/learn` callout | open |
| A departing element leaves a hole, or fails to | Left alone it keeps its place until the swap commits. `position: absolute` on `:leave-to` hands the space over at the *start* instead. Both are wanted, in different situations: a page swap wants the space handed over, a dropped list row does not | `/reference/css/` | partly, `05-as-built` says it, `/learn` does not |
| An element that animates and then animates **back** | Fixed in the engine on 2026-08-19, so no longer a trap. Kept here as the reason the next row exists | none | closed |
| A swap's duration comes from the element's own `transition`, so **two rules at different specificities change how long a swap lasts** | The duration is read off the computed style, which is the single place a duration is written. An override like `.stage.slow .page` therefore changes the swap as well as the walk, which is the intent and still surprises | `/reference/css/` | open |

### `<path>`

| Trap | What actually happens, and why it is right | Destination | Status |
|---|---|---|---|
| Setting `width` on a `<path>` does not scale the drawing | The geometry is in the element's own coordinates; a width changes the box the drawing sits in. Scaling is `transform`, which every other element already uses for the same thing. The alternative was a `viewBox`, deliberately not built | `/reference/paths/` says it; `/learn` has no path chapter | open |
| A fixed-coordinate path **runs off a narrow window** | Follows from the above and bit the first chart written. A drawing does not reflow, so its coordinates have to be chosen for the smallest box it will sit in, or bound with `:d` and computed | `/reference/paths/`, and a recipe | open |
| Two shapes refuse to morph | They interpolate only when their command sequences match. Resampling is deliberately not attempted, because guessing a correspondence folds as often as it morphs. **Also**: both shapes must start at the same point and run the same way round, or the morph turns the shape inside out | `/reference/paths/` has the rule, **not the start-point half** | open |
| A `<path>` with no `alt` is silent to a screen reader | Deliberate: a drawing without a description is decoration, and announcing an unnamed graphic is worse than skipping it | `/reference/accessibility/` | open |

### Script

| Trap | What actually happens, and why it is right | Destination | Status |
|---|---|---|---|
| A closure passed to `filter` / `map` **cannot see the surrounding scope** | A method call passes its receiver by reference and the scope cannot also be borrowed, so method dispatch does not capture. A plain call does. The workaround is a loop, or lifting what the closure needs into a parameter | `/reference/script/` documents the rule for functions; **not for closures inside method calls** | open |
| There is no ternary | `cond ? a : b` does not exist. Use `:class='#{ name: expr }'` or `r-if` | `/reference/script/` | open |

### Layout and events

| Trap | What actually happens, and why it is right | Destination | Status |
|---|---|---|---|
| A box that exists only to **swallow** a tap takes a focus ring and a Tab stop | A tap goes to the topmost element with a handler and stops there, so a scrim that dismisses needs the dialog over it to be a tap target of its own or the tap falls through. That much is right. The cost is that a `@tap` box is an interactive element by definition, so the swallow is focusable, and there is no way to say "tappable but not focusable". Found writing the modal recipe | `/reference/touch/`, and the modal recipe says it | open |
| An element with only `@drag` does not respond to a tap | `@tap` is the finished gesture and is what a keyboard activation produces, so a drag-only element is deliberately not a tap target and does not swallow a tap meant for what is under it | `/reference/touch/` says it; worth an example | partly |
| A document grows past the window and simply clips | Nothing scrolls unless told to. `overflow-y: auto` is the answer, and the example that outgrew its window is the argument for saying so early | `/reference/scrolling/`, and `/learn` | open |
| A cover fills the wrapper it is written in rather than the panel it was meant for | `absolute` is measured against the nearest ancestor that is **not** `position: static`, which is CSS's rule, and `static` is the default. So an unpositioned wrapper in between is passed over, and the box you mean has to say `position: relative`. Right, and the reverse of what Rux did until 2026-08-19, when every box was a containing block | `/reference/css/`, and the modal recipe says it | open |
| Sticky headings **pile up** instead of handing over to each other | Two sticky boxes never interact: each is clamped to its own parent. The hand-over in a list of sections is emergent, because one section's bottom edge is where the next section's heading begins. Written as flat siblings of the rows they share the scroller as their parent, so they all pin at the same edge and stack. Right, and the opposite of what it looks like | `/reference/css/` says it; worth a recipe | open |
| A component and its caller sharing a class name now collide | Document rules style the components they use, chosen deliberately in v0.7. `<style scoped>` is the opt-out, and means the same thing from either side | `/reference/components/` | open |

---

## The pattern worth noticing

Most rows here are the same shape: **a rule that is right in isolation, whose
consequence appears somewhere the author was not looking.** A departing box
keeps its place, which is right, and the consequence lands on the element below
it. Paint is CSS, which is right, and the consequence is that specificity
changes an animation's duration.

That suggests the documentation these need is not a list of rules. It is a list
of *consequences*, written from where the author is standing when they get
surprised. That is what the recipes section is for, and it is why the recipes
are the piece of v0.7 still outstanding.
