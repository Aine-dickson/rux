+++
title = "Components"
description = "Importing a file as a tag, props, slots, events, and what a component cannot see."
weight = 12
+++

<!-- GENERATED FROM docs/05-as-built.md BY site/sync-docs.sh. DO NOT EDIT HERE. -->

```rust
<script> use components::stat; </script>       // → components/stat.rux
```
```xml
<stat :label="title" :value="level" />         // props evaluated in caller scope
```
```rust
<script>                                        // in components/stat.rux
  prop label;                                   // the caller must pass it
  prop value = 0;                               // the caller may leave it off
</script>
```
Component instances are isolated (only props are visible inside). Their CSS styles
their own subtree. Editing a component hot-reloads.

**A component declares its props with `prop`.** `prop label;` is one the caller
must pass, `prop value = 0;` one it may leave off, and `prop a, b;` declares two.
A prop is an input, not state: it is not a `let`, and a handler inside the
component cannot write it. A default is an expression, evaluated where the
document's names are visible and the caller's `r-for` variables are not, since
it was written in the component.

A prop is passed as `:name="expr"`, evaluated in the caller's scope, or as a
plain `name="text"`, which passes the text as HTML would. A two-word prop is
written the tag's way or the script's, so `:id-of` and `:id_of` both reach
`prop id_of;`: tags are kebab and scripts are snake, and the attribute follows
the tag.

**What a component tag takes, and nothing else:** its declared props, `@event`
listeners, and the directives `r-if`, `r-elif`, `r-else`, `r-for`, `r-key` and
`r-transition`. Any other attribute is an error at the tag, with the props the
component does declare and the nearest one if it looks like a misspelling. That
includes `class`, `style`, `id`, `to` and `r-show`: a component tag is not an
element, so there is nothing for them to land on, and until props were declared
all five were dropped without a word. Put them on an element inside the
component, wrap the tag in a `<view>`, or declare a prop of that name, in which
case it is simply a prop. A declared prop without a default that the tag does
not pass is an error at the tag too.

A `<route>` stands in for its view's tag. Its bound attributes are the view's
props (`<route path="/crew" view="crew-list" :crew="crew" />`), checked the same
way, and a path segment `:id` passes `id`, so a view needing a prop that neither
the path captures nor the route passes is an error on the `<route>`. A route's
plain attributes (`path`, `name`, `guard`) are its own and never reach the view.

A file that declares a prop is a component whoever opens it. Checked on its own,
its declared props are owed by a caller and say nothing, and a name it reads
without declaring is a warning rather than an error, since a component may still
read a document signal by name.

**Rux's own elements take a fixed set of attributes too.** Each element accepts
the attributes Rux reads on it (the globals `class`, `id`, `style`, `role`,
`label`, `to` and the `r-` directives, plus its own, such as `src` on `<image>`),
and a bound `:name` form only where one is honored: `:class`, `:style`, `:to`
and `:r-transition` everywhere, `:src` on `<image>`, `:d` on `<path>`, and
`:options`, `:disabled`, `:readonly`, `:required` on `<input>`, `:disabled` on
`<button>`. Anything else is an error: an invented attribute, a near miss like
`:clas`, and a bound form that does not exist, like `:id` or `:placeholder`,
which used to be evaluated and thrown away. `key` and `v-if` are named as the
Vue habits they are, with `r-key` and `r-if` offered in their place.

**One component, two spellings, and the template takes either.**
`use components::crew_detail;` names the file `components/crew_detail.rux` and
contributes the tag `<crew-detail>`. The `use` **has** to be snake, because it
is a path and `crew-detail` is not a path segment anyone can write; the tag
**has** to be kebab, because that is what a custom element looks like. Both are
forced, at opposite ends of the same file, and the author was left holding the
difference. So in a `<template>` — as a tag and as a `<route view="…">` alike —
`crew_detail` and `crew-detail` are the same component and both resolve. In
`<script>` the `use` path stays strict: it is naming a file.

Nothing is ambiguous, because the import maps `_` to `-` and no import can ever
contribute a tag with an underscore in it. Whichever spelling is written, the
component is filed under its **canonical** kebab name, so one component written
both ways in one file is still one instance with one set of state, and not two
halves of one.

**And the file may be named either way too.** `use new_task;` finds
`new_task.rux`, and failing that `new-task.rux`. Exact spelling first at both
bases, so nothing that resolves today moves; the hyphenated candidate can only
turn a hard error into a working import. That matters because a file is named
by a person, and somebody who has been writing `<new-task>` all morning names it
`new-task.rux`.

**A component may use a component, and a tag is a local name.** Every file's
`use` lines are read, not just the document's, and each file's markup may write
only the tags that file imported. So `components/task.rux` can `use` its own
`components/avatar.rux` without the document knowing, and two pages may each
`use` a different `task.rux` and each write `<task>` meaning their own.

Until v0.7.1 a component's `use` lines were parsed and then thrown away. Only
the root document's imports existed, in one flat map, so a component could not
use a component: the tag matched nothing, expanded to nothing, and said nothing.
Found on a real project where `pages/home.rux` carried `use components::task;`
and rendered `<task r-for="t in tasks">`, `app.rux` imported only the pages, and
the list came up empty in a way that read as *"no tasks yet"*. The flat map had a
second failure nobody had hit yet: two files importing different components under
one tag, where the second import silently replaced the first.

Imports are followed as a worklist rather than by recursion, so **two files
importing each other terminates** rather than overflowing a stack: a file already
loaded contributes its tag to the importing file's namespace and is not walked
again. Functions are the deliberate exception and stay shared across every file:
a component may call a `fn` its caller declared, which is the older rule and what
`set_is_fragment` exists to keep checkable.

**A tag that names nothing is an error.**
```
there is no element or component called `<netask>`. Rux's own elements are
<screen> <view> <text> <image> <path> <button> <input> <slot> <router> <route>
<router-view>; anything else is a component, and needs a `use` for it in this
file's <script>
```
It can never render anything, which is the same test `view=` uses. It was silent
until v0.7.1, and that silence is what hid the nested-import bug above for the
whole of v0.7: a tag is the one name in a template that had no other way to fail.

**A route's `view` is a name in the file that wrote the `<router>`.** Not in
whatever component the `<router-view />` sits in, which by the time a nested
route renders is usually several files away from the one that named it.

**A `use` path with a `-` in it is an error.** `-` is the minus operator in
script, so no name in it has one, and a `use` path is no exception: `use
new-task;` fails to load, at its line, and the message names `use new_task;`,
which finds the same file whether it is named `new_task.rux` or
`new-task.rux`. Until 2026-09-26 it warned and resolved anyway, because `use`
lines were lifted out of the script before anything parsed them; the project
owner ruled that a name that would be subtraction anywhere else in the section
is not a name here either.

Reported 2026-09-15 as *"why am I forced to write `new_task` as `new-task` in
`view=`?"*, against a `view="new_task"` whose `use pages::new_task;` sat three
lines below it. Until then the answer was a message telling the author to go and
write the other spelling of a name they had already given correctly.

**A `<template>` takes exactly one root element**, in a document and in a
component alike. Wrap siblings in a `<view>`. Writing several is reported, with
the line of the second: it used to keep the first and drop the rest in silence,
and a first root carrying an `r-if` that happened to be false rendered the whole
component as nothing at all.

**An import is looked for beside the file first, then from the project root.**
`use components::stat;` in `pages/home.rux` tries `pages/components/stat.rux`,
and if nothing is there, `components/stat.rux` next to the project's `app.rux`
or `index.rux`. So a page in a subdirectory can share the components at the root
rather than keeping a copy of each one beside it.

Beside-first is deliberate: a component that resolves today goes on meaning the
same file, and the root is only ever a fallback. Outside a project, where no
`app.rux` or `index.rux` marks the top, there is no root and only the relative
form applies.

There is still no `super::` and no `..`. The root fallback covers what those
were being reached for, and a path that can climb is a path that can escape the
project.

**Components are a desktop feature today.** `use components::stat;` names a
*file*, and the web build has no filesystem to read it from: a document run in
a browser is handed no components, so every component tag renders nothing and
every `<route>` warns that its view is not imported. Bundling components into a
web build is `rux build`'s job. Nothing about the component model itself is
web-specific, so this is a packaging gap rather than a design one.

**Slots.** A `<slot />` in a component's template renders whatever the caller
wrote between the tags, so a component can wrap markup it has never seen:
```xml
<!-- components/panel.rux -->
<view class="panel">
  <text>{{ title }}</text>
  <slot><text>nothing here yet</text></slot>   <!-- children = the default -->
</view>
```
```xml
<panel :title="&quot;stats&quot;">
  <text class="stat">{{ count }}</text>        <!-- this file's signal, this file's CSS -->
</panel>
```
Slot content belongs to the **caller**: it reads the caller's signals (the
component cannot see the caller's own instance state), is styled by the
caller's stylesheet, and its
handlers run in the caller's scope. Only its position comes from the component.
An unfilled slot falls back to its own children, as in HTML. A `<slot>` emits no
box of its own, so a component adds no wrapper nobody wrote.

Before this, children written between the tags were **silently dropped**, which
made every component a fixed shape: no cards, panels, modals or layout wrappers.
Driven in `examples/slots.rux`.

**A component has its own state.** Its `<script>`'s top-level `let`s run **once
per instance**, so three `<counter>` elements are three counts:
```rux
<!-- components/counter.rux -->
<view @tap="count = count + step"><text>{{ count }}</text></view>
<script> let count = signal(0); </script>   <!-- private to each instance -->
```
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
is code and state is not. A handler carries its instance from the cascade to the
shell, so the identical handler text in two instances still writes to the right
one. Props are re-derived from the caller on every build and are **not**
writable from inside: assigning to one would look like it worked and be
forgotten on the next build.

A change to instance state **rebuilds** rather than patches, since the state is
not a signal and the binding registry has nothing to look it up by. A component
is a subtree, so it is bounded, but it is coarser than a signal change. Driven
in `examples/component-state.rux`.

**An instance lives as long as it is on screen.** A component closed over by an
`r-if`, or a row that leaves an `r-for`, loses its state, and shows up new if it
comes back. Every build walks the whole template, so what a build does not reach
is what has gone. This is the same rule a route view already followed, and until
now it was the *only* place that followed it: a hidden component used to keep
its state for the life of the process and hand it back on the way in, and the
instance map only ever grew. Anything meant to outlive being hidden belongs in a
document signal.

**Events.** A component tells its caller that something happened with `emit`,
and the caller listens with `@event` on the tag:
```rux
<!-- components/stepper.rux -->
<view @tap="count = count + 1; emit(&quot;change&quot;, 1)"><text>{{ count }}</text></view>
<script> let count = signal(0); </script>
```
```xml
<stepper @change="total = total + event" />    <!-- payload arrives as `event` -->
```
The body of a listener is the **caller's** code and runs in the caller's scope,
the same rule slot content follows: a component with its own `total` cannot be
written to by mistake. `emit` with no payload leaves `event` undeclared rather
than defining it empty. An event nobody listens to is ignored, so a component
can offer more events than any one caller wants. An `emit` outside a component
has no caller and warns.

A listener is carried as text and never evaluated at build time, which is why it
is `@event` and not a prop: a prop is evaluated on every build, and a statement
that ran once per build would be the opposite of an event. A payload is read
where `emit` is written, so `emit("change", 0 - count); count = 0` reports the
count it had. A chain of components emitting at each other is stopped after 8
rounds with a warning.

Together with props this closes the loop: state can stay in the component that
owns it instead of being hoisted into the document so the document can see it
change. Driven in `examples/events.rux`.

`computed` and `effect` work inside a component, per instance and in that
instance's own scope. A computed is declared in the instance's script as a
placeholder rather than as its own expression, because creating an instance runs
that script in a scope without the document's signals: a computed reading one
would fail there, and a failed script takes the instance's whole state with it.
The real value is computed at mount, before the tree that shows it is built.

The rest is dependency bookkeeping the document already does, kept per instance:
a computed re-reads when what it read moves, an effect re-runs on the same terms
and is never woken by its own writes, and both are dropped when the instance is.
One thing is specific to instances: a computed or effect that writes only
instance state has moved nothing the change pipeline reasons about, so it forces
the rebuild itself rather than leaving the old value on screen.

`mounted` and `unmounted` are supported, and run per instance in that instance's
own scope. The build is the only place that knows an instance has appeared or
gone, and the wrong place to act on it, so it reports both and the runtime runs
the bodies after the tree is in place. An instance dropped before either hook was
reached runs neither, and when one build swaps two components the leaver's
`unmounted` runs before the arriver's `mounted`. Driven in
`examples/lifecycle.rux`.
