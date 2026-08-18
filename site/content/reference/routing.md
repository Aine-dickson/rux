+++
title = "Routing"
description = "Routes, parameters, named routes, links, and the fact that the path is an ordinary signal."
weight = 13
+++

<!-- GENERATED FROM docs/05-as-built.md BY site/sync-docs.sh. DO NOT EDIT HERE. -->


A `<router>` renders the one `<route>` whose path matches, and a route maps a
path to a component, so a page is a component like any other:
```xml
<router>
  <route path="/"          view="home-page" />
  <route path="/crew"      view="crew-list" :crew="crew" />
  <route path="/crew/:id"  view="crew-detail" :crew="crew" />
  <route fallback          view="lost-page" />
</router>
```
Like `<slot>`, a router leaves **no box of its own** behind: the matched view
expands in its place. Routes are tried in the order written and the first match
wins, so a `fallback` can sit anywhere among them. A path nothing matches and no
fallback catches renders nothing, and warns.

**The path is an ordinary signal called `route`.** That is the whole design:
`{{ route }}`, `r-if="route == \"/about\""` and `:class` already understand
navigation, and a route change reconciles the router's subtree rather than
rebuilding the document.

**Parameters.** A `:name` segment matches anything and is handed to the view as
a prop, so `/crew/grace` reaches `crew-detail` with `id` set to `"grace"`. A
match must account for the whole path, not just its front, or `/` would match
everything. A trailing slash is not a difference.

**Nested routes.** A `<route>` may contain `<route>` children, and the parent's
view places a `<router-view />` where they render:
```xml
<router>
  <route path="/" view="home-page" />
  <route path="/crew" view="crew-list">
    <route path=""            view="crew-empty" />
    <route name="crew-detail" path=":id" view="crew-detail" />
  </route>
  <route fallback view="lost-page" />
</router>
```
A child path is **relative** unless it begins with `/`, so a section can be moved
by editing one line. `path=""` is the index route: it fills the outlet at the
parent's own path, and without one `/crew` renders the list with an empty outlet
rather than an error. `<router-view />` leaves no box of its own, like `<slot>`.

The parent **stays mounted** while the child changes under it, so a list keeps
its state and its scroll position as you move between the things it lists.

Parameters are **merged down the chain**: a child view sees what its parent
captured, and the `params` signal outside the router sees what a child captured.
A name resolves to its **full** path, built from its ancestors, so
`path_for("crew-detail", #{ id: "grace" })` returns `/crew/grace` from a name
written on the child.

A path that matches a parent but nothing under it is not a half match: the whole
branch fails and the next sibling is tried, ending at the fallback. That is why
`/crew/grace/extra` lands on `lost-page` rather than on the crew list.

Two mistakes are reported rather than rendered as silence: a route with children
whose view never places a `<router-view />`, and a `<router-view />` in something
that is not a route's view.

**Links.** `to="/path"` makes an element tap to that path, announce as a link
rather than a button, and match `:current` when it names the path you are on,
which is how a nav bar shows where you are:
```css
.tab:current { background: #89b4fa; color: #11111b; }
```
`:to="…"` is the computed form, for a list whose every row links somewhere
different (`:to="&quot;/crew/&quot; + member.id"`). An explicit `@tap` wins over
both, so a link can still do something else on the way.

**Parameters are also readable from outside the matched view**, as `params`:
```xml
<text r-if="params.id != ()">viewing: {{ params.id }}</text>
```
The view gets them as props, which is enough for the view. It is not enough for
a title bar or a breadcrumb, which sit in the document's own layout and are not
the matched view. `params` empties when a route captures nothing, rather than
keeping the last page's answer.

**History.** `navigate("/path")`, `replace("/path")`, `back()` and `forward()`
are callable from any handler. History is one list with a cursor, so going back
and then somewhere new drops what was ahead. Navigating to where you already are
is not a visit, or tapping the current tab would fill the history with repeats.
On the desktop, **Alt+Left / Alt+Right** and the mouse's side buttons walk it.

`replace` goes somewhere *instead of* where you are, overwriting the current
entry, and it is what a redirect needs rather than a nicety. Redirect with
`navigate` and the redirecting page stays in the history, so Back lands on it
and is redirected forward again: the Back button appears broken and nothing in
userland can fix it.

**`can_go_back` and `can_go_forward`** are signals, so a history button can grey
itself out:
```xml
<view class="step" :class="#{ dead: !can_go_back }" @tap="back()">
```
Signals rather than functions because what they are for is disabling a control,
and disabling a control is a class, and a class reads signals.

**Query strings** are read through a `query` map, and are not part of the path:
```xml
<text>looking for {{ query.q }}</text>   <!-- /search?q=dark+mode -->
```
`route` stays `/search`, so every `route == "/search"` already written keeps
meaning what it says. A query is an argument to a page rather than a different
page, so it takes no part in matching either. The history stores the whole
address, so going back to a search restores what was being searched for. `+` is
a space and `%xx` is decoded; a key with no `=` is present and empty; a repeated
key keeps the first.

**Named routes.** A path is written into every link that leads to it, so a URL
scheme that can never be changed afterwards is not much of a scheme. Name a
route and build its path with `path_for`:
```xml
<route name="crew-detail" path="/crew/:id" view="crew-detail" />
...
<view :to="path_for(&quot;crew-detail&quot;, #{ id: member.id })">
```
It returns a **string**, so it composes with `to`, `:to`, `navigate` and
`replace` rather than needing a second form of each. Values matching a `:name`
segment fill it; whatever is left over becomes a query string, which is what
makes `path_for("search", #{ q: "rust" })` work for a route with no parameters
at all. Values are escaped on the way in and unescaped on the way out, so an id
containing a `/` survives the round trip. A missing parameter or an unknown name
warns, and produces a path that visibly does not work: landing on the fallback
page is a bug you can see, and landing on the wrong record is not.

`route`, `params`, `query`, `can_go_back` and `can_go_forward` are all provided,
and all reserved: a script declaring one is warned rather than quietly
overwritten.

**A route's view starts fresh when you return to it.** Instance state is keyed by
template position, so *keeping* it across a visit is what would happen by
accident; anything meant to outlive a visit belongs in a document signal. Driven
in `examples/router.rux`.

**An app can open on a page other than its first one**, which is what a link
someone shared arrives as. On the desktop that is a flag:
```text
rux run app.rux --route /crew/grace
```
The arrival page is the *first* page, not the second: there is no `/` behind it,
because no one visited one, so Back has nowhere to go. Saving the file while a
page other than `/` is showing now reloads onto that page instead of jumping
home, so an edit to a page three taps in can actually be seen.

**On the web the URL bar is the app's address bar**, if the page hands it over:
```js
start(canvas, source, "/");     // served at the root of a domain
start(canvas, source, "/app/"); // served from a subdirectory
start(canvas, source);          // leave the URL alone
```
The base is subtracted from the URL, so an app is written the same way wherever
it is deployed: the route is `/crew`, the URL is `/app/crew`. With a base given,
opening a URL opens that route, navigating adds a history entry, and the
browser's own Back and Forward walk the app, including a long-press that jumps
several entries at once. Each entry carries its position in the history, which
is what makes a multi-entry jump one move rather than a guess about direction.

Passing no base leaves the URL untouched, and that is the default on purpose:
the playground runs documents written by whoever is typing into them, and one of
them containing a `<router>` must not be able to rewrite the address of the page
hosting it.

> **A `<router>` cannot render a route view on the web yet.** A route's view is
> a component, a component is loaded from a file, and a browser has no
> filesystem: the web entry point is handed no components at all, so every
> `<route>` warns that its view is not imported and the router renders nothing.
> The URL half above is built and works, and `route` is an ordinary signal, so
> `r-if="route == &quot;/about&quot;"` does work on the web today. What is
> missing is the bundling of components into a web build, which is what
> `rux build` is for. Until then, treat the router as desktop-only.

**Scroll restoration** is on, and `<router restore-scroll="false">` turns it off.
The flag means **remember**, not *always restore*: a page you open starts at the
top, and a page you go **back** to comes back where you left it. Which of the
two you get is decided by how you arrived rather than by a preference, which is
what every platform does. A flag meaning "always restore" would drop you into
the middle of a page you had just opened for the first time, which reads as a
bug. Turned off, every arrival is the top. A redirect through `replace` is an
arrival, not a return, so it lands at the top too.

Offsets are stored on the **history entry**, not on the route. A scroll region
is identified by its position among the scrolling boxes in tree order, so those
ids only line up when the tree has the same shape, and an entry is always one
route: by the time the offsets are read back, the shape is the one they were
recorded against.

Not built: nested routes (a layout component with a `<router />` in its slot
covers most of that), and route guards.
