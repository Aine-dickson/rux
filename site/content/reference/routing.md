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

**Every route's `view` is checked at load, not on arrival.** A route is expanded
when its path is the one you are on, so a `view` naming nothing used to be
silent on every page but its own: the document loaded, `rux check` exited 0, and
the mistake waited for the first navigation there. Whether a name is imported is
a fact about the file rather than about where you are standing in it, and it is
reported as an **error** — a page that can never render is wrong, not merely
dead.

**A `to=` that matches no route is reported too.** A dead link is silent by
construction: tapping it navigates, the router matches nothing, and the screen
goes blank with no more explanation than an empty screen. The address is written
in the markup and so are the routes, so the two are compared before anyone taps.
Only the written-out `to=` — `:to` is built from a row's own data, and a path
that exists for row 3 and not for row 4 is a data problem rather than a markup
one. The check runs through the router's own matcher, so `<route fallback>`
answers for everything and a document with no `<router>` says nothing at all
(which is what a component holding links needs).

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

**After Android kills the app.** Android ends a backgrounded app whenever it
wants the memory, and a phone of ordinary size wants it within seconds of Home.
The activity keeps the whole history, each entry with its offsets, and the
focused field with its text and caret, and puts them back before the first
frame, so coming back looks like the app never left. The page on screen gets
its scroll back even with `restore-scroll="false"`: that flag is about Back and
Forward, and here the person never left the page. The guards are asked again
about that page, because the new process has only the first values of its
signals, and one that refuses or redirects sends the app there as an arrival,
with the rest of the old history dropped. A password field comes back focused
and empty. **Signals are not kept**: an app's data is its own to save.

**After a phone's browser discards the tab.** A browser does to a background
tab what Android does to a background app, and brings it back by loading the
page again. A web app that owns its address (`start` was given a `base`, as a
built app's page is) keeps the same state in the tab's session storage, and
puts it back when the page is reloaded, walked back or forward to, or restored
after a discard. A link followed or an address typed is a new visit and starts
fresh, and so does a page whose URL no longer names the route that was on
screen. The playground keeps nothing, since it runs whatever is typed into it.

**Route guards.** `guard="expr"` on a `<router>` runs on every navigation; on a
`<route>` it runs whenever that route is part of what matched, so a guard on a
section covers every page inside it without being written on each one. Outermost
first: a section's guard is the coarser question, and answering it second would
mean running the finer one for a place you were never going to reach.

The answers are vue-router's: **`false` cancels**, **a string redirects to that
path**, and **anything else allows**. Anything else includes `()`, which is what
a guard body with no explicit answer evaluates to, so the usual shape is object
or say nothing:

```rux
<route path="/sent" view="outbox" guard="gate()" />
```
```rux
fn gate() {
  if !signed_in {
    return "/login";
  }
}
```

`to` and `from` are in scope, along with whatever parameters the guard's own
level captured, so a guard on `/crew/:id` can read `id` and decide about that
member rather than only about the section.

**A guard runs before the history moves**, which is the whole reason it is here
rather than in a page: a refused navigation leaves no entry behind and opens no
route transition, and by the time a page could refuse to render itself both have
already happened. It follows that **Back, Forward and a deep link go through
guards too**. A guard written on `navigate` alone would protect nothing, since
Back reaches the same page without passing it, and Back is how anyone leaves a
login screen.

A guard that redirects to the path it was asked about has allowed it. A circle of
redirects is cut off after eight and reported, the same bound and the same reason
as an `emit` chain.

**A guard that fails to evaluate refuses**, and this is the one place in Rux
where a failing expression does not fall back to something harmless. Everywhere
else the document carries on with a benign default: an `r-if` goes false, a
`{{ }}` goes empty. A guard has no benign default, because its two answers are
"let them in" and "do not", and the reason anyone writes one is the second.
`guard="user.is_admin"` with `user` still loading is syntactically fine, so the
load-time check passes; before this it admitted everybody, warned into the
overlay, and looked exactly like a working app. The cost is that the route is
unreachable until the guard is fixed, and the overlay names the expression and
the reason.

**Guards are synchronous.** There are no promises in the script language, so a
guard cannot await a network answer; it decides from state that is already there.
Fetch first, then navigate.

A guard is compiled at load, on the same terms as a `@tap` handler, so a syntax
error in one is reported without anyone having to navigate. It hides longer than
a handler otherwise would: nobody taps a guard, so a broken one is found by
whoever navigates, and what they see is a link that does nothing.
