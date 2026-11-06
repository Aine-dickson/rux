# Removed from 05-as-built.md, Components to the end

Text taken out on 2026-11-06 because it described behaviour that has since changed,
or narrated how a bug was found. Each block keeps the reason it carried.

## Components: nested imports (removed 2026-11-06 from 05-as-built.md)

Until v0.7.1 a component's `use` lines were parsed and then thrown away. Only
the root document's imports existed, in one flat map, so a component could not
use a component: the tag matched nothing, expanded to nothing, and said nothing.
Found on a real project where `pages/home.rux` carried `use components::task;`
and rendered `<task r-for="t in tasks">`, `app.rux` imported only the pages, and
the list came up empty in a way that read as *"no tasks yet"*. The flat map had a
second failure nobody had hit yet: two files importing different components under
one tag, where the second import silently replaced the first.

Functions are the deliberate exception and stay shared across every file:
a component may call a `fn` its caller declared, which is the older rule and what
`set_is_fragment` exists to keep checkable.

It can never render anything, which is the same test `view=` uses. It was silent
until v0.7.1, and that silence is what hid the nested-import bug above for the
whole of v0.7: a tag is the one name in a template that had no other way to fail.

(Functions are no longer shared: a component's names are its own.)

## Components: a `use` path with a `-` (removed 2026-11-06 from 05-as-built.md)

Until 2026-09-26 it warned and resolved anyway, because `use`
lines were lifted out of the script before anything parsed them; the project
owner ruled that a name that would be subtraction anywhere else in the section
is not a name here either.

Reported 2026-09-15 as *"why am I forced to write `new_task` as `new-task` in
`view=`?"*, against a `view="new_task"` whose `use pages::new_task;` sat three
lines below it. Until then the answer was a message telling the author to go and
write the other spelling of a name they had already given correctly.

## Components: one root element (removed 2026-11-06 from 05-as-built.md)

Writing several is reported, with the line of the second: it used to keep the
first and drop the rest in silence, and a first root carrying an `r-if` that
happened to be false rendered the whole component as nothing at all.

## Components: the old scope rule (removed 2026-11-06 from 05-as-built.md)

Superseded when a component's names became its own (the document's signals are no
longer visible inside a component, and shared state lives in a store).

The isolation is about **declarations, and it runs one way**. A component's
`<script>` executes in a scope of its own, so its `let`s are private: the
document cannot read them, and the same name declared on both sides is two
different variables, the component's winning inside it.

What the component's **template and handlers** see is wider. They are evaluated
against the document's scope with the instance's own names pushed on top, so a
document signal the component does not shadow is visible to `{{ }}` and can be
assigned in a `@tap`:
```rux
<!-- components/card.rux: `theme` is the document's, not this file's -->
<view @tap="theme = &quot;dark&quot;"><text>theme is {{ theme }}</text></view>
```
This is deliberate and the router depends on it: `{{ route }}` works inside a
route view, which is a component. It is also the coupling a component author
should be aware of, since a component reading a name it never declared will only
work in an app that happens to declare it. Anything a component means to be told
should come in as a **prop**, and anything it means to report should go out as an
**event**. Reaching for a document signal by name is available, not recommended.

Only `fn` definitions are shared with the document's engine, because a function
is code and state is not.

A change to instance state **rebuilds** rather than patches, since the state is
not a signal and the binding registry has nothing to look it up by. A component
is a subtree, so it is bounded, but it is coarser than a signal change.

(Instance state is now patched in place; `route`, `params` and `query` are
provided to every file by the runtime.)

A computed is declared in the instance's script as a
placeholder rather than as its own expression, because creating an instance runs
that script in a scope without the document's signals: a computed reading one
would fail there, and a failed script takes the instance's whole state with it.
The real value is computed at mount, before the tree that shows it is built.

One thing is specific to instances: a computed or effect that writes only
instance state has moved nothing the change pipeline reasons about, so it forces
the rebuild itself rather than leaving the old value on screen.

## Components: instance lifetime (removed 2026-11-06 from 05-as-built.md)

A hidden component used to keep its state for the life of the process and hand
it back on the way in, and the instance map only ever grew. Now an instance that
a build does not reach is gone, as a route view always was.

## Components and routing on the web (removed 2026-11-06 from 05-as-built.md)

**Components are a desktop feature today.** `use components::stat;` names a
*file*, and the web build has no filesystem to read it from: a document run in
a browser is handed no components, so every component tag renders nothing and
every `<route>` warns that its view is not imported. Bundling components into a
web build is `rux build`'s job. Nothing about the component model itself is
web-specific, so this is a packaging gap rather than a design one.

> **A `<router>` cannot render a route view on the web yet.** A route's view is
> a component, a component is loaded from a file, and a browser has no
> filesystem: the web entry point is handed no components at all, so every
> `<route>` warns that its view is not imported and the router renders nothing.
> The URL half above is built and works, and `route` is an ordinary signal, so
> `r-if="route == &quot;/about&quot;"` does work on the web today. What is
> missing is the bundling of components into a web build, which is what
> `rux build` is for. Until then, treat the router as desktop-only.

(Superseded: `rux build --target web` embeds every document through a memory
source. The playground, which runs one document, still has none.)

## Routing: checks at load (removed 2026-11-06 from 05-as-built.md)

A route is expanded when its path is the one you are on, so a `view` naming nothing
used to be silent on every page but its own: the document loaded, `rux check`
exited 0, and the mistake waited for the first navigation there. A dead `to=` is
silent by construction: tapping it navigates, the router matches nothing, and the
screen goes blank with no more explanation than an empty screen.

## Routing: guards that fail (removed 2026-11-06 from 05-as-built.md)

Before a failing guard refused, `guard="user.is_admin"` with `user` still loading
admitted everybody, warned into the overlay, and looked exactly like a working
app.

## Errors: template warnings carried no line (removed 2026-11-06 from 05-as-built.md)

**Template warnings carry their line too**, from v0.7.1. Every one of them
used to arrive unplaced, so the overlay, `rux check --format json` and the
editor gutter all fell back to the top of the file: the `<` of `<template>`,
which is the one place in a document where nothing is ever wrong. The
template parser now records a file line for each element, each attribute and
each text run, and a warning is attributed to the most specific one that
applies. A handler lands on its own attribute's line rather than on the tag's,
which matters because elements are routinely written across several lines; an
`r-if` or `r-for` lands on the directive rather than on the parent element
whose loop reads it; and a `{{ }}` lands on the text run rather than on the
element containing it.

**A warning raised inside an imported component names that component's file**,
not the document that imported it. Errors have done this since components
landed; warnings had nowhere to put it, so they carried the component's line
number and the importer's name. That pairing reads as a precise location and
is not one, which is worse than saying nothing at all.

**Load-time findings stay listed.** Every rebuild used to replace the list, so a
document whose `mounted`, `computed` or `effect` ran lost every one of those
errors before the overlay or `rux check` saw it: a bogus `@frob` on a page with a
`mounted` block checked clean.

## Errors: calls to nothing (removed 2026-11-06 from 05-as-built.md)

rhai looks a function name up when the call *runs*, so until v0.7.1
`@tap="alert(1)"` compiled clean and did nothing when pressed, while the
identical mistake inside `{{ }}` was reported during the build. A dead handler
looks exactly like a handler that never fired, so this was the worst place in the
language to be quiet.

**Names, not argument counts.** Argument counts are a separate question with real
traps in them, and a check that flags working code is worse than the silence it
replaced. (Argument counts to a `fn` are checked now.)

`set` and `get` escape a name check because they really are registered, on arrays,
maps, blobs and strings. What identifies the old signal API is the arity: every
registered `set` takes two arguments after its receiver.

## Errors: playground deployment and known limits (removed 2026-11-06 from 05-as-built.md)

> The deployed page is built from `main` while the runtime it loads is pinned to
> the latest **tag**, so the page has to keep working against a build that
> predates its own features. It feature-detects `diagnose` and falls back to the
> older `setSource`, which reports an error and no warnings. The fallback is
> only removable once a deployed build actually carries `diagnose`, which means
> after the tag exists *and* the site has been rebuilt against it: pushing a tag
> deploys nothing on its own, since the workflow fires on pushes to `main`.

**Known limits:** rhai returns `()` for a missing *map property*, rather than
erroring, so `{{ user.nmae }}` still renders empty with nothing reported (a
missing *function* or variable does report). That one is rhai's semantics, not
ours, it is tracked as a motivator for the planned rhai fork. Expression
failures and anything from a component's CSS are still reported without a line.

(All three are closed: a missing property is an error, and expression failures
and component CSS warnings carry their line.)

## Gotchas (removed 2026-11-06 from 05-as-built.md)

2. **`use` must be alone on its own line** in `<script>`. (Declarations are found
   by parsing the script now; `let a = 1; use components::m; let b = 2;` imports.)
3. **A `fn` called in method style cannot see the surrounding scope.**
   `helper(thing)` reaches the state around it; `thing.helper()` does not. This
   is all that is left of what used to be the single biggest trap here, "rhai
   `fn`s can't touch globals", which v0.7 removed (see above). (The interpreter
   that replaced rhai gives a method call the file's names; docs/11-next.md
   lists this as dropped fork divergence 4.)

## Known gaps (removed 2026-11-06 from 05-as-built.md)

- **The soft keyboard has not been tried on real phone hardware.** It is driven
  and passing under browser touch emulation, tap through to committed CJK (see
  "Text input and composition"), so the mechanism works: the hidden `<input>` is
  created on demand, focused by the tap, and laid over the field. What emulation
  cannot show is a keyboard physically rising, since that is the OS's decision.
  Still worth thirty seconds on a real phone. Note it only reaches ruxlang.dev
  once a tag carrying it has been deployed, because the playground is built from
  the latest release rather than from `main`, and a tag push does not itself
  trigger a deploy.
- ~~Text editing: no word-wise movement (Ctrl+arrows), no triple-click line-select,
  no drag-and-drop of selected text.~~ Built in v0.9.
- ~~Scrolling: no track-click paging, no kinetic touch fling, no scrollbar
  hover/fade, and `overflow-x` / `overflow-y` can't differ from each other.~~
  Built: fling in v0.8, the rest in v0.9.
- CSS: `box-shadow`, `position`/`top`/`left`, per-corner radius, per-side border
  *colors*. (All built.)
- ~~True inline text-flow.~~ Built in v0.9: a `<text>` inside a `<text>`, and
  `<image>`/`<path>` in a line.
- `r-for` rebuilds more rows than a keyed diff would; effects and computed
  values are still absent from the reactive tier. (Both closed: rows are reused
  and effects and computed values exist.)

> Fine-grained reactivity **shipped in v0.3**: a signal change now patches only
> the bindings that read it, and the wholesale rebuild no longer fires. This list
> claimed otherwise until 2026-07-26. If a gap here reads as more pessimistic
> than the [release blog](/blog/), trust the blog and fix this
> file.

## Where the design docs are still right (removed 2026-11-06 from 05-as-built.md)

The [rationale](./01-rationale.md)'s core laws still hold and still guide changes:
**layout lives in CSS, not markup** (no `<Padding>`/`<Center>` widgets); **reuse
mature crates**; **keep the element set tiny**. The [architecture](./04-architecture.md)
pipeline (parse → cascade → layout → paint → present, with a file watcher) is
exactly what got built. Only the *reactive graph* stage is simpler than described.
