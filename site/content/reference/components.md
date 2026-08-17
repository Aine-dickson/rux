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
Component instances are isolated (only props are visible inside). Their CSS styles
their own subtree. Editing a component hot-reloads.

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
