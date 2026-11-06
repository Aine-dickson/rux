# The script language

What goes inside `<script>`, and what a `{{ }}` binding or an `@tap` attribute
may contain.

Rux's script runs on **Rux's own interpreter**: a script is read by Rux's
parser, checked, lowered to a typed IR and run. A release build compiles the
entry document's functions to Rust. This document is the reference for the
language; [The Rux language](./11-next.md) is its design.

Where this document and [As Built](./05-as-built.md) overlap, they agree; As
Built is the wider tour of the whole language, this is the script half in depth.

## The three places script runs

Almost every rule below follows from which of these you are in.

| Where | Example | Runs |
|---|---|---|
| The top-level script | `let n = signal(0);` | Once, when the document loads |
| A binding | `{{ n * 2 }}`, `:class`, `r-if`, `r-for` | On every build, possibly many times a second |
| A handler | `@tap="bump()"` | When something happens |

A **binding is an expression that must not do anything**. It is re-evaluated on
every build and once per row of an `r-for`, so a side effect in one happens an
unpredictable number of times. Nothing enforces this for arbitrary code, but it
is why `query()` is rejected in a binding and why `navigate()` in one is a
mistake rather than a feature.

A **handler is a statement**, and may be several. It can write state, call
functions, navigate, and read the tree.

## State

`signal(v)` declares a reactive value. A binding that reads one *is* a
subscription to it.

```rux
let level = signal(82);
let items = signal(["a", "b"]);
```

`signal()` is identity: it returns what it was given, and its job is to mark the
declaration. `signal(82)` holds an `int` and `signal(82.0)` a `float`; see
[Values](#values).

**`computed name = expr;`** declares derived state, readable anywhere a signal
is. Refreshing is one pass in declaration order, so a computed may read one
declared above it and not below.

**`effect { … }`** runs when what it read changes, and once on load. It
subscribes to what it actually read on its last run, and is never woken by its
own writes.

Both work inside a component too, and there they run **per instance**, in that
instance's own scope. Two `<card>` tags are two instances, so each holds its own
computed value and each runs its own effect. A component computed may read a
store's signal (see [Modules](#modules)) and re-reads it when that signal moves; an instance's computeds
and effects are dropped when the instance is, so nothing left behind can be woken
by a signal later. See [As Built](./05-as-built.md) for the detail on both.

**`prop name;`** declares, in a component, something its caller passes on the
tag. `prop size = 16;` gives it a default, so the caller may leave it off;
without one the caller must pass it, and `rux check` says so at the tag. A prop
is read like any name and is not state: a handler in the component cannot write
it. The tag writes a two-word prop either way round, so `:id-of` reaches
`prop id_of;`. Any attribute on a component tag that is not a declared prop, a
listener or a directive is an error.

```rux
prop label;
prop done = false;
prop price: float = 1;
```

A prop may carry a type, as any declaration may: see
[Type annotations](#type-annotations).

## Lifecycle

**`mounted { … }`** runs once, after the first tree exists. **`unmounted { … }`**
runs when the document stops being the one on screen, which is the window
closing or a hot reload replacing it.

```rux
<script>
  use native::settings;
  let level = signal(0);
  mounted   { level = settings.lastSaved(); }
  unmounted { settings.save(level); }
</script>
```

Blocks, not functions, matching `effect { }`. A hook is not something the author
calls, and giving it a callable name would invite exactly that. Several blocks
of the same kind run in the order written.

`mounted` runs **after** the effects, so a hook reading a signal sees what the
effects decided, and it runs **exactly once**, which is the whole difference
between it and an `effect` that happens to fire on load. Whatever it writes
reaches the first frame: it deliberately runs after the tree exists rather than
during construction, or the screen would show the value the hook was written to
replace.

`unmounted` runs for its side effect. Nothing is done with what it writes,
because by then there is no tree for a write to reach. It runs at most once even
if teardown is reached twice, so a document closed by a reload and then by the
window does not save twice.

### Inside a component

A component may declare both, and they run **per instance**, in that instance's
own scope. Two `<card>` tags on a page are two instances, so each runs its own
`mounted` and each writes its own state.

```rux
<script>
  use stores::visits;

  let draft = signal("");

  mounted   { visits.open(); }
  unmounted { visits.leave(draft); }
</script>
```

An instance mounts the first time a build expands it, and unmounts when a build
stops reaching it: an `r-if` closing over it, its `r-for` row going away, or a
route being left. Leaving and coming back is a **new** instance, so the hooks run
again and the state starts fresh. Anything meant to outlive a visit belongs in a
store, which the component changes through the store's functions: a
component's names are its own, and the document's are not among them.

`unmounted` is the last moment the instance's state can be read, which is what
makes saving from it possible at all. What it writes to its own names goes
nowhere, since the instance is gone; what it hands a store is the point.

Two guarantees are worth stating, because both are cases the runtime has to go
out of its way to get right:

- **`unmounted` never runs without `mounted` having run.** An instance created by
  one build and dropped by the next, before either hook was reached, runs
  neither: it was never on screen in the sense the hooks are about.
- **Unmounts run before mounts** when one build swaps one component for another,
  so the leaver has saved before the arriver reads.

When the app closes, every component still on screen runs its `unmounted`,
deepest first, and then the document runs its own.

## Closing the app

**`@close` on the `<screen>`** runs when the person asks to close the app: the
window's close button, Alt+F4, or Android's Back with no page left to go back
to. It answers the way a route guard does: **`false` keeps the app open**, and
anything else, including saying nothing, lets it close. **`close()`** closes the
app for good without asking `@close` again, because the app has decided.

Together they make a "save changes?" out of the app's own markup:

```rux
<template>
  <screen @close="confirmClose()">
    <view class="dialog" r-if="asking">
      <text>Save changes before closing?</text>
      <button @tap="saveAndClose()">Save</button>
      <button @tap="close()">Don't save</button>
      <button @tap="asking = false">Cancel</button>
    </view>
  </screen>
</template>

<script>
  use native::project;
  let dirty = signal(false);
  let asking = signal(false);

  fn confirmClose() {
    if dirty { asking = true; return false; }
  }
  async fn saveAndClose() {
    await project.save();
    close();
  }
</script>
```

- **`@close` answers at once.** An `async fn` cannot refuse a close: it answers
  nothing until it is done, and by then the app is gone. One that starts an
  `async fn` and lets the app close is told its task was stopped at its first
  `await`. A save that has to wait is the shape above: answer `false`, await,
  then `close()`.
- **A `@close` that fails lets the app close**, and says so. A route guard that
  fails refuses, because letting someone into a page is the risk there; here
  the risk is a window nobody can close.
- **A save that does not wait can go in `unmounted`**, which runs on every
  close, `close()` included.
- **Android** asks `@close` when Back has no page left. When Android ends the
  app on its own, nothing can ask; the app comes back where it was instead.
- **The web does not ask `@close` yet.** A browser wants its answer inside its
  own callback, which the document cannot reach from there, so closing the tab
  closes it without a question; the console says so when a page has a
  `@close`. `close()` does nothing on the web, since a page cannot close its own
  tab.

## The desktop window

An app that sets `[window] decorations = false` in `rux.toml` has no title bar
and no border: it draws its own, from three pieces.

**`app-region: drag`** on a box makes it a handle for moving the window, and
`app-region: no-drag` takes a part of one back. It is inherited, as `cursor`
is. A box with a `@tap`, a gesture or a field is a control and never a handle,
so the buttons in a title row need no `no-drag`. Pressing a drag region moves
the window (on Windows it snaps to an edge as a native title bar does), and a
second press close to the first maximizes it, or restores it.

**`window`** needs no `use`, and has four calls and a value:

| | |
|---|---|
| `window.minimize()` | minimizes the window |
| `window.maximize()` | maximizes it |
| `window.restore()` | brings it back from minimized or maximized |
| `window.toggleMaximize()` | maximizes, or restores a maximized window |
| `window.maximized` | `true` while maximized, `false` otherwise, `none` before the window opens; it follows the window however that changed, so a button can show the right glyph |

Closing is `close()`, from "Closing the app" above. The calls act at once.
Android and the web have no such window: the calls throw `unsupported`.

```rux
<view class="bar">
  <text class="title">Studio</text>
  <button @tap="window.minimize()">_</button>
  <button r-if="window.maximized == true" @tap="window.toggleMaximize()">restore</button>
  <button r-else @tap="window.toggleMaximize()">max</button>
  <button @tap="close()">x</button>
</view>
<style>.bar { app-region: drag; display: flex; }</style>
```

A frameless window keeps what a frame gave it: a six pixel band round the edge
resizes it from that side or corner, with the matching cursor. A window that
is maximized has none. On Windows the shell also asks the system for the
window's drop shadow and rounded corners, so a frameless window has the
host's own, not a copy of them.

**Frameless is one way to put content in the title row, not the only one.** It
means the app owns the whole row, window buttons included. The other way keeps
the system's frame (its border, shadow, corners, resize edges and snap layouts)
and lets the app's content sit beside the window buttons, as VS Code's does:

```toml
[window]
title-bar = "overlay"   # "system", the default, is the ordinary title bar
```

The page then starts at the top of the window, under where the title bar was,
and the app draws its own row there. `env(titlebar-area-x)`, `-y`, `-width` and
`-height` give the part of the row the window buttons leave free, so the row
stops short of them:

```rux
<view class="bar">
  <text>Studio</text>
  <button @tap="open()">Open</button>
</view>
<style>
.bar {
  app-region: drag;
  display: flex;
  height: env(titlebar-area-height, 40px);
  width: env(titlebar-area-width, 100%);
}
</style>
```

`app-region: drag` works as it does without a frame, and a second press maximizes
or restores. **On Windows** the shell draws the three window buttons itself, in
the system's look (46 by 32, its glyphs, a red close on hover), because the
system cannot draw them over a GPU surface; the maximize button still opens
Windows 11's snap layouts, and the glyphs take a light or dark ink from the
colour behind them. The buttons are 32 logical pixels high, and the row below
them is the app's. **On macOS** the real traffic lights stay over the content,
and `env(titlebar-area-*)` leaves 78 pixels free at the left of a 28 pixel row.
Where there is no overlay (Linux, Android, the web, a `--preview`) the window
keeps its ordinary title bar and `env(titlebar-area-*)` is undefined, so each
use takes its fallback, as in CSS. `title-bar = "overlay"` and
`decorations = false` together are refused.

## Keys

**`@key`** runs when a key is pressed while focus is on the element or inside
it, on any element and on the `<screen>`. It is handed `event.key`, the key by
the browser's names (`"s"`, `"S"` with Shift, `"Enter"`, `"Escape"`,
`"ArrowUp"`, `"F2"`, `" "` for Space), `event.ctrl`, `shift`, `alt` and `meta`,
**`event.primary`** (Ctrl, or Cmd on macOS, so one test covers both), and
`event.repeat` while the key is held. **Answering `false` takes the key**: it
goes no further, as a guard's `false` refuses.

```rux
<screen @key="shortcut(event)">
```
```rux
fn shortcut(e) {
  if e.primary && e.key == "s" { save(); return false; }
  if e.primary && e.key == "z" { history.undo(); return false; }
}
```

A key goes, in order, to: the focused text field's own `@key`; **the field's own
editing**, for a key it uses (text, Backspace, the arrows, Enter, and Ctrl+A, C,
X, V, and Ctrl+Z and Y while it has a step to undo or redo); each enclosing element's `@key`, innermost first, then the
`<screen>`'s; and the shell's own handling (Tab, Enter on a button, scrolling,
Alt+Left and Right). So Ctrl+Z in a field undoes its typing and outside one
reaches the app's own undo, and Ctrl+S, which no field uses, reaches the app
from anywhere.

**A text field has undo and redo**: Ctrl+Z, and Ctrl+Y or Ctrl+Shift+Z. Typing
is grouped as a browser groups it, a run of typing in one place being one step;
moving the caret, a pause of a second, switching between typing and deleting,
and a paste each start a new one. The history belongs to the field as long as
the field is there, focused or not: leave it, come back, and Ctrl+Z still undoes
what was typed. It is dropped when the app's script writes the field's value,
since an undo would put back text the app replaced. An edit made with
`el.insertText(text)` is the exception: it is undone as a step of its own, as if
it had been typed. With nothing left to undo, Ctrl+Z goes on to the `@key`s
around the field, so an app's own undo can follow the field's.

**An app that keeps one undo for a whole editor** has three ways to line it
up with the fields' own:

- `@input`'s `event.kind` says what the edit was: `"type"` (keys and the
  keyboard's own input), `"undo"` or `"redo"` (the field's own history),
  `"paste"`, `"cut"`, `"insert"` (`insertText`), `"drop"` (text dragged in or
  out) or `"autofill"`. Leave `"undo"` and `"redo"` out of the app's history
  and it records only what was done, not the field stepping back through it.
- `el.clearHistory()` forgets a field's undo and redo, for when the app's
  own undo has moved past them.
- A field's own `@key` sees Ctrl+Z before the field does, so answering
  `false` there keeps the key from the field and the app owns undo outright:

```rux
<input r-model="body" type="textarea" @key="editorKey(event)" />
<script>
  fn editorKey(e: KeyEvent) {
    if e.primary && e.key == "z" && !e.shift { undo(); return false; }
    if e.primary && (e.key == "y" || e.key == "Z") { redo(); return false; }
  }
</script>
```

On a component's tag, `@key` is a listener for an `emit("key")`, as every
`@name` there is.

## The pointer vocabulary

Beyond `@tap`, five events report what a finger or a button is doing (and two more, `@contextmenu` and `@doubletap`, are below):

| Attribute | Fires |
|---|---|
| `@press` | the moment a finger or button goes down on the element |
| `@release` | when it comes up, whether or not it stayed still enough to be a tap |
| `@longpress` | once, after it has been held still for half a second |
| `@swipe` | once at the end of a press that travelled far enough, fast enough |
| `@drag` | at the start of a drag, on every move, and at the end |

`@tap` is deliberately not one of them. It is the finished gesture rather than
raw pointer traffic: a keyboard activation produces one, and so does `tap()`
from script, neither of which has a pointer at all.

```rux
<view class="pad"
      @press="held = true"
      @release="held = false"
      @swipe="if event.direction == &quot;left&quot; { next() }"
      @drag="offset = event.totalX">
```

`@swipe` adds `event.direction`, one of `left`, `right`, `up` or `down`, chosen
by the dominant axis so a slightly diagonal flick still means what the hand
meant.

`@drag` adds `event.phase`, one of `start`, `move` or `end`. The start is not
the press: a press that never moves is not a drag.

Both carry **two distances**, named so they cannot be mistaken for one another:

| Field | Measured from |
|---|---|
| `event.totalX`, `event.totalY` | where the press landed |
| `event.moveX`, `event.moveY` | where the pointer was on the previous event |

`totalX` is what following a finger wants: assign it and the thing tracks the
hand with no bookkeeping. `moveX` is what velocity and flick detection want.
Every pointer event also says where the pointer is now: `event.x`, `event.y`
in the element, `event.pageX`, `event.pageY` in the window, and the
element's `width` and `height` (the table under "A function that takes the
event").

**A drag that ends as a flick also fires `@swipe`.** The two are not rivals: a
page that follows the finger still has to be told, at the end, whether the hand
meant to throw it, and that is exactly how a reversible page transition decides
whether to commit.

**A `@drag` claims the finger.** While a drag is running, the page under it does
not scroll. An explicit handler beats an implicit gesture, which is the simple
half of that rule; whether the scroll can take the finger back mid-gesture is
not decided, and waits for real touch hardware to argue with.

**A long press is exclusive with the other two.** A press that moved is not
resting, so once the pointer wanders past the tap slop the long press is off.

### Buttons, menus and double taps

`event.button` says which button went down, numbered as a browser numbers
them: `0` the primary, `1` the middle, `2` the secondary. A finger is always
`0`. The middle and secondary buttons fire `@press` and `@release` and nothing
else from the table above: `@tap`, `@drag`, `@swipe` and `@longpress` are the
primary button's, as a browser's `click` is.

Two more events are finished gestures, like `@tap`, and each goes to the
nearest element that wrote it. Writing one never takes a `@press` or a `@drag`
away from an element around it.

| Attribute | Fires |
|---|---|
| `@contextmenu` | on a secondary click, on a touch press held still for half a second, and on the Menu key or Shift+F10 with focus on the element |
| `@doubletap` | after the second of two taps close together in time and place, a double-click or a double-tap |

```rux
<view class="row"
      @contextmenu="menu = { row: id, x: event.pageX, y: event.pageY }"
      @doubletap="rename(id)">
```

`@contextmenu` hands over `event.x` and `event.y` in the element, and
`event.pageX` and `event.pageY` in the window, which is where an anchored menu
wants to open. On a keyboard it reports the middle of the focused element.
Where the platform opens its menus (Windows on the release, everywhere else on
the press) the event follows suit. On a touchscreen `@longpress` fires first.

`@doubletap` runs after both taps' `@tap`, in the order a browser's `dblclick`
comes, and a third tap starts counting again.

### Drag and drop

Picking something up in one place and putting it down in another is four
questions Rux answers and an author cannot: which element a press picks up,
which target the pointer is over, which slot among that target's items it is
at, and when each of those changes. Everything else is written in the app:
what the thing looks like in the air, the gap it leaves, a row sliding aside,
and what a drop does to the data. Rux draws nothing for a drag.

```rux
<view class="lessons" drop-group="outline"
      @dragover="place(section.id, event)"
      @drop="save()">
  <view r-for="l in section.lessons" r-key="l.id" class="row"
        :drag-data="l.id" drag-group="outline"
        @dragend="if !event.dropped { undo() }">
    <view drag-handle>≡</view> {{ l.title }}
  </view>
</view>
```

| Attribute | On | Means |
|---|---|---|
| `drag-data` (or `:drag-data="expr"`) | a source | this element can be picked up, and carries this value |
| `drag-group` | a source | which targets take it; none means those with no group |
| `drag-handle` | inside a source | a press here picks the source up; a source with a handle is picked up by a handle only |
| `drop-group` | a target | the group of source it takes |
| `@dragstart` | the source | it was picked up |
| `@dragmove` | the source | every pointer move of the drag: where to draw its copy |
| `@dragenter`, `@dragover`, `@dragleave` | a target | the dragged thing came over it, is at a slot (on entering and each time the slot changes), left it |
| `@drop` | a target | it was let go over it |
| `@dragend` | the source | the drag is over, dropped or not |

A target is any element with `drop-group` or one of `@dragenter`, `@dragover`,
`@dragleave` and `@drop`. A press moves a source after the pointer has gone a
few pixels, as `@drag` does, and the source's own `@drag`, `@swipe` and
`@longpress` stay out of it. On a finger a source with a handle is picked up
by the handle at once, and one without is picked up by holding it still for
half a second, so a scroll still scrolls; a source picked up by a long press
does not also open its `@contextmenu`. Escape lets go: nothing is dropped.

`event` in all seven is a `DropEvent`:

| Field | Is |
|---|---|
| `data` | what the source's `drag-data` held |
| `group` | its `drag-group`, or `null` |
| `index` | the slot in the target the pointer is at, **counted without the dragged item**, so splicing the data at it is right; `-1` on the source's events and when over no target |
| `before` | the `drag-data` of the item it would land in front of, or `null` at the end |
| `dropped` | on `@dragend`: whether it was dropped on a target |
| `grabX`, `grabY` | where in the source the press landed |
| `x`, `y`, `pageX`, `pageY`, `width`, `height`, `touches` | as for `@tap`; `x` and `y` are in the element the handler is on |

The items of a target are the sources whose nearest target is it, so an
outline's items are its sections and a section's are its lessons, and the
deepest target under the pointer that takes the group is the one it is over.
A target inside the thing being dragged is never the one: a section cannot be
dropped into itself. The slot is read from where the items are: a column by
the pointer's height against each item's middle, a row by its width, and a
run that wraps in reading order. An empty target that has a size takes a drop
at slot 0.

`:dragging` matches the elements carrying what is being dragged, found by
their `drag-data` and not their place, so it stays on the row when a live move
has rebuilt the list around it. `:drag-over` matches the target the pointer is
over. Both are ordinary selectors:

```css
.row:dragging { opacity: 0.35; }
.lessons:drag-over { background: #1b2033; }
```

**To move the data as the pointer travels** (the other rows then make room by
themselves), do it in `@dragover`, which runs when the slot changes, and undo
it in `@dragend` when `event.dropped` is false. **To move it only on a drop**,
do it in `@drop`, and draw the gap from `index` in `@dragover` if one is wanted.
**The copy under the pointer** is a box of the app's own, `position: fixed` and
`pointer-events: none`, put at `event.pageX` and `event.pageY` by `@dragmove`.
**A row sliding aside** is a keyed row's `transition: transform` (see "A keyed
row slides when it moves" in [as built](./05-as-built.md)). Driven in
`examples/sortable.rux`.

A source's position is read from where the layout put it: a source inside
something `transform`ed has its untransformed box. A box drawn over a source
(a menu, a backdrop) keeps a press from it.

## What an event hands you

Every handler above, and `@tap` too, has an `event` in scope:

```rux
<view class="card" @tap="mark(event.x, event.y)">
```

`event.x` and `event.y` are **relative to the element the handler is on**, in
logical pixels, because that is the frame you are thinking in: half way across a
card is `event.x > width / 2` wherever the card sits on screen.

`event.pageX` and `event.pageY` are the same point in the **window's** frame,
for when the element cannot be the frame of reference: a drag whose element is
moving under the finger, or anything comparing two elements' positions.

`event.touches` is every finger that is down, each with its own `id`, `x` and
`y` in that same frame:

```rux
<view @tap="report = event.touches.length + \" fingers\"">
```

**A list even when there is one finger**, and a mouse counts as one finger with
`id` 0, so a handler written for a phone reads the same on a desktop. A laptop
touchpad reaches the app as a mouse, so it reports one finger too however many
are on the pad: only real touch hardware fills this list. That shape
is deliberate: a two-finger gesture arrives later without changing what any
handler already written reads. A finger outside the element has negative
coordinates rather than being left out.

**A function that takes the event annotates it by name**, and every script
can name these without a `use`:

| Type | Handed over by | Adds to `x`, `y`, `pageX`, `pageY`, `width`, `height`, `button`, `touches` |
|---|---|---|
| `TapEvent` | `@tap`, `@press`, `@release`, `@longpress`, `@contextmenu`, `@doubletap` | `button` |
| `DragEvent` | `@drag` | `totalX`, `totalY`, `moveX`, `moveY`, `phase` |
| `DropEvent` | `@dragstart`, `@dragmove`, `@dragenter`, `@dragover`, `@dragleave`, `@drop`, `@dragend` | `data`, `group`, `index`, `before`, `dropped`, `grabX`, `grabY` (see "Drag and drop") |
| `SwipeEvent` | `@swipe` | `totalX`, `totalY`, `moveX`, `moveY`, `direction` |
| `KeyEvent` | `@key` | none of those, only `key`, `ctrl`, `shift`, `alt`, `meta`, `primary`, `repeat` |
| `SelectEvent` | `@select` on a field | none of those, only `value`, `start`, `end`, `caret` |
| `InputEvent` | `@input` on a text field | none of those, only `value` and `kind` (see "An app that keeps one undo" above) |

`TextMark` is one range of a field's `:marks`, `{ start: int, end: int,
class: string }`, so a function that builds them from `[]` can say what it
returns: `fn marks(src: string): TextMark[]`.

```rux
<view @key="onKey(event)">
<script>
  fn onKey(e: KeyEvent) { if e.primary && e.key == "Enter" { send(); } }
</script>
```

`rux check` names the one to write when a parameter is left without a type.

## Intervals

`setInterval(ms) { … }` runs a block on a period and hands back a handle.
`clearInterval(handle)` stops it.

```rux
<script>
  let seconds = signal(0);
  let timer = signal(0);

  mounted {
    timer = setInterval(1000) {
      seconds++;
      if seconds >= 5 { clearInterval(timer); }
    }
  }
</script>
```

The handle is why this is a call and not a declaration like `effect { }`: a timer
that cannot be stopped on a condition cannot be used, and unlike a hook, an
interval has something to hand back. Clearing a handle that names nothing is
harmless, so restarting needs no guard.

**An interval belongs to whoever started it.** Started inside a component, it
belongs to that instance and stops when the instance goes, with nothing to
remember to clean up: a timer outliving its component would run a body against
state nobody can reach. Started at document level it lives as long as the
document. Starting one from `unmounted` warns, since there is no longer an
instance for it to belong to.

A period has to be more than zero, and a running interval is one of the clocks
the window wakes for. Nothing is running between ticks, so an app whose timers
are all stopped goes back to using no CPU at all.

The block is a block, not a callback. A function value called later writes to its
own captured copies and cannot move a signal, which for an interval would defeat
the purpose; the body is carried as text and run exactly as a handler is, the
same as `@tap` and the lifecycle hooks.

## Functions

A function sees the scope it was written in, and can read and write it.

```rux
let level = signal(82);

fn drain() { level-- }          // writes the signal, the screen follows
fn is_low() { level < 20 }      // reads it
```

`@tap="drain()"` is a handler like any other and the write is tracked.

**A method call on a function of the file is that function called with the
receiver first**: `thing.helper(a)` is `helper(thing, a)`. It is tried after
the language's own methods (`items.push(x)`) and a native value's.

**A function sees its own file's names**: the file's signals, functions and
props, what the file imports, and, in a component, the instance's state. Not
a caller's local, and not another file's: a component's functions cannot see
the document's, and the document's cannot see a component's. Pass a name in
instead: `@tap="pick(item)"` and `fn pick(item: Task)`. State two files share
lives in a store both import ([Modules](#modules)). See
[Types](./10-types.md#a-typed-function-cannot-read-its-callers-locals).

**Anything heavy belongs in Rust**, reached as a native module
([Native code](#native-code)). Script describes what the UI does; it is not
where work gets done.

## Modules

A `.rux` file with no `<template>` is a **module**: functions, types and state
for other files to import. It is written as a `<script>` alone, or as bare
script with no tags at all.

```rux
// stores/cart.rux
export type Item = { name: string, qty: int };

export let items: Item[] = signal([]);

export fn add(name: string) {
  items.push({ name: name, qty: 1 });
}

export fn count(): int { items.length }
```

```rux
<script>
  use stores::cart;                     // the whole module, as `cart`
  use stores::cart::{add as put};       // one name it exports
  use type stores::cart::Item;          // a type, marked with `type`
</script>
```

`import` is the same statement spelled the other way: `import cart from
"./stores/cart";`, `import { add as put } from "./stores/cart";`, `import type {
Item } from "./stores/cart";`. A module is found the way a component is, beside
the importing file, then from the project root, and `rux fmt` writes every
import one way when `rux.toml` asks, `[fmt] imports = "use"` or `"import"`.

- **`export` is what another file sees.** `export fn`, `export async fn`,
  `export type` and `export let`; the rest is the module's own. Importing a
  name it does not export is an error that lists what it does.
- **A module is loaded once per document**, however many files import it, and
  its top level runs then, before the document's, each module after the ones
  it imports. Two modules that import each other in a circle are refused.
- **Its top-level signals are a store.** Every file that imports it reads the
  same `cart.items`, and a binding that reads it follows it, whether in the
  document or in any component instance.
- **Only the module changes its state.** `cart.items = []` or
  `cart.items.push(x)` in another file is an error: write the change as a
  function the module exports, and call it, `cart.add("tea")`. A store's rules
  live in one place that way.
- **An imported name is the import everywhere in the file**, so declaring it
  again is an error; `as` renames the import instead.
- A module holds no `computed`, `effect`, lifecycle block or `prop`, which
  belong to a component or a document; an exported function can work out what
  a `computed` would.
- An `async fn` a module exports belongs to the document when it runs: it
  writes only the module's state, which outlives any component that called it.

**A component's names are its own.** Its template, handlers, functions,
`computed`s, `effect`s and lifecycle blocks see its state, its props, what it
imports and what the runtime provides every file (`route`, `params`, `query`),
and not the document's signals or functions. Two components may each declare a
`fn` of one name. What a page and the app share is a store:

```rux
<!-- pages/home.rux -->
<template>
  <view r-for="t in todo.tasks"><text>{{ t.label }}</text></view>
</template>
<script>
  use stores::todo;
</script>
```

## Async

Work that takes a while (a request, a file, a database) happens in Rust, and a
page waits for it in an `async fn`:

```rux
<script>
  use native::people;
  use type native::people::User;
  let status = signal("idle");

  async fn load(id: int) {
    status = "loading...";
    let user: User = await people.lookup(id);
    status = `${user.name}, ${user.age}`;
  }
</script>

<button @tap="load(3)">Load</button>
```

- **A handler calls an `async fn`; it never awaits.** `@tap="load(3)"` starts
  `load` and returns at once. What `load` writes before its `await` renders
  with the tap; what it writes after renders when the answer comes, with no
  input and without the window waking for anything else in between.
- **`await` is only written inside an `async fn`**, and only in front of a
  call to another `async fn` or to an `async` native function. It may sit anywhere an
  expression can, `total += await price(id)` included, but not inside a
  closure, which is not `async`.
- **The result type is the value awaited**: `async fn price(id: int): float`
  gives a `float` to `await price(1)`. A call without `await` gives a
  **handle** (an `int`, as `setInterval`'s is), since nothing has come back
  yet: keep it to [cancel](#cancelling) the call.
- **Everything between two `await`s runs without interruption.** There is one
  thread of script. A signal can change under a function only at an `await`,
  so whatever a condition established about a signal before one is not known
  after it, and the checker says so.
- **A failed `await` throws** where it is, and `try`/`catch` takes it. One
  nothing catches is reported as a failing handler is, naming the function and
  the line of the `await`.
- **An `async fn` belongs to whoever started it**, as an interval does. Started
  in a component, it stops at its `await` when the instance goes, and the
  answer that comes later writes nothing. A document reloaded or left drops its
  own the same way.
- **A binding or a `computed` cannot start one**, since either runs again on
  every change. A handler, `mounted` and an `effect` can. What the function
  reads is never a dependency: an `effect` that starts one is subscribed to
  what the effect itself read.

### Cancelling

**`cancel(handle)` stops an `async fn`** that was started without `await`,
as `clearInterval(handle)` stops an interval. It stops at the `await` it is
waiting at, nothing after that runs, and the Rust future it waits on is
dropped (see the end of this section). A timeout is an interval that cancels:

```rux
let job = signal(0);
let timer = signal(0);

async fn send(url: string) {
  status = "sending";
  res = await http.send(url);
  status = "done";
  clearInterval(timer);
}

fn onSend() {
  job = send(url);
  timer = setInterval(10000) {
    cancel(job);
    status = "timed out";
    clearInterval(timer);
  }
}

fn onCancel() {
  cancel(job);
  clearInterval(timer);
  status = "cancelled";
}
```

- **The handle names one run.** Starting `send` again gives a new one; a
  `cancel` of the old one stops only the old run.
- **One already finished, already cancelled, or `0`, is ignored**: cancelling
  twice, or after the answer came, is ordinary rather than a mistake.
- **A run may cancel itself** with its own handle; it stops at its next
  `await`.
- **Every `await` in the run stops with it**, an `async fn` it awaited
  included: there is one run, and it is cancelled whole.
- A file with a `fn cancel(x)` of its own keeps it; the built-in is then out
  of reach in that file.

What a page awaits is an `async fn` in the app's Rust, exported to Rux
([Native code](#native-code)):

```rust
#[rux::export]
pub async fn lookup(id: u32) -> Result<User, LookupError> {
    // …; an Err is thrown at the `await`, with the error type's name as `kind`
}
```

Its future runs off the UI thread: on a thread of
its own by default, or on the app's own runtime once it hands one over with
`rux::set_spawner`. There is no `Future` or `Promise` value on the Rux side,
only the handle a start gives, and no `fetch` or `delay` built in.

**When the run is cancelled, or the component that started it goes, the
future is dropped** where it waits, which is how a Rust future is cancelled: a request it was making goes
no further, and a socket or file it held is closed by its own `Drop`. Nothing
answers, and nothing runs after the `await`. A call that blocks its thread
inside the future (a blocking HTTP client, `std::thread::sleep`) cannot be
reached there, and finishes first; write it with an async client, or check a
flag of its own between steps.

## File dialogs

**`pickFile`, `pickFiles` and `pickSaveFile` ask the person for a file**, with
the platform's own dialog, and are awaited from an `async fn`. They are Rux's
own, so nothing is imported:

```rux
async fn openProject() {
  let file = await pickFile({
    title: "Open project",
    filters: [{ name: "Mava project", extensions: ["mava"] }],
  });
  if file != none { await project.open(file.path); }
}

async fn saveAs() {
  let path = await pickSaveFile({ title: "Save project", name: "untitled.mava" });
  if path != none { await project.saveAs(path); }
}
```

- **`pickFile(options): File?`**, **`pickFiles(options): File[]`**,
  **`pickSaveFile(options): string?`**. Cancelling gives `none`, or `[]`.
  A `File` is what an `<input type="file">` binds: `name`, `size`, `type`,
  `url`, `path`, and `await file.text()`. A save dialog names a file that is
  not there yet, so it gives a path.
- **The options** are all optional: `title`, `filters` (each a `name` and its
  `extensions`, with or without the dot), `folder` to open in, and `name`, the
  file name a save dialog suggests. A field the dialogs do not have is an
  error, as for any record.
- **The window keeps going while the dialog is open**: it is the platform's
  dialog run off the window's thread, and the path comes back as an `await`'s
  answer does. On Linux it is the desktop portal's dialog, not GTK's.
- **`file.path` is the desktop's**, for the app's Rust to open; `file.text()`
  reads it from script, everywhere.
- **On the web and on Android** `pickFile` and `pickFiles` open the picker an
  `<input type="file">` opens (the browser's; the system's document picker),
  `filters` becoming its `accept`, and give the same `File`s, with no `path`.
  `title` and `folder` have nothing to set there. A browser opens a picker
  only during a person's gesture, so call one **in a tap handler, before
  any other `await`**: anywhere else it throws an `Error` of `kind`
  `"gesture"` rather than a picker that never opens. One picker at a time:
  a second call while one is open throws `"busy"`. `pickSaveFile` throws
  `"unsupported"` there, since neither has a save dialog that gives a path.
- Written out, they are `use native::rux::{pickFile};`, which is what a script
  that calls one gets without writing it.

## Sounds

**`loadSound(src)` loads a sound**, awaited, and gives a `Sound`:

```rux
let click: Sound? = signal(none);
let song: Sound? = signal(none);

async fn start() {
  click = await loadSound("sounds/click.wav");
  song = await loadSound(picked.url);
}

fn toggle() {
  if song != none {
    if song.playing { song.pause(); } else { song.play(); }
  }
}
```

```rux
<text>{{ song?.currentTime ?? 0 }} of {{ song?.duration ?? 0 }}</text>
<button @tap="toggle()">Play</button>
```

- **`src`** is a path beside the document, read as an `<image src>` is (so a
  release build plays the sounds it embeds), or a picked `File`'s `url`.
- **Methods**: `play()` from where it is, or from the start once it has
  ended; `pause()`; `stop()`, which pauses and goes back to the start;
  `seek(seconds)`.
- **Properties**, which follow the sound as it plays, so a `{{ }}` reading
  one updates by itself: `volume` (0 to 1), `loop`, `currentTime` (writing it
  seeks, as HTML's does), `duration` (`none` while the file has not said)
  and `playing`. The last two are read-only.
- **Formats**: WAV, MP3, AAC in MP4, FLAC and Ogg Vorbis. Not Opus: loading
  one throws an `Error` of `kind` `"unsupported"`, as does a file that is not
  a sound.
- **One output for the whole app**, mixing every sound playing, opened by the
  first `play()`: an app that only loads sounds touches no audio device. A
  short sound is decoded whole and plays at once; a long one is read a little
  ahead of where it plays, so a song is never held in memory.
- **When the app is put away** (Android), every sound pauses and the output
  is let go; nothing starts again on the way back, and the next `play()`
  carries on from where it was. A call or another app's music pauses Rux's
  too: a sound holds Android's audio focus while it plays, a long one the
  whole of it (another app's music pauses), a short one only enough to lower
  theirs for its moment. A sound playing at volume 0 holds none, so a
  muted one never pauses the person's music. On the desktop, a new output (headphones pulled, or
  another chosen) is followed, and the sound carries on there.
- **In a browser** a sound plays through the browser's own audio, so in
  every format the browser plays (Opus too), and the properties follow it as
  the browser reports, about four times a second. A path the build did not
  embed is fetched relative to the page. **A browser plays only after a
  tap**: a `play()` before the person has touched the page does nothing, and
  says so on the console.
- **`<audio>`** is the same sound as an element, with drawn controls and
  `@play`, `@pause`, `@ended`, `@timeupdate` and `@error` (see the
  elements reference).

## Settings

**`settings.get(key)`, `settings.set(key, value)` and `settings.remove(key)`**
keep an app's own small values (recent files, a chosen theme, a window's last
size) between runs. They are Rux's own, so nothing is imported, and they are
plain calls rather than `await`s: settings are small, and read at start.

```rux
let recent: string[] = signal(settings.get("recentProjects") ?? []);

fn remember(path: string) {
  recent.insert(0, path);
  recent.truncate(10);
  settings.set("recentProjects", recent);
}
```

- **`get` gives `any?`**: `none` for a key never set. Annotate what you read,
  as above.
- **A value is anything that crosses to Rust**: text, numbers (an `int` comes
  back an `int`), booleans, `none`, arrays, maps and records. A resource lives
  only while the app runs and cannot be kept.
- **Every `set` is written at once**, to a file beside the settings that is
  then renamed over them, so a crash leaves the old values, never half of the
  new.
- **Kept per app, by `[app] id`** in `rux.toml`: `%APPDATA%\<id>\settings.json`
  on Windows, `~/Library/Application Support/<id>/` on macOS,
  `$XDG_DATA_HOME/<id>/` or `~/.local/share/<id>/` on Linux, the app's own
  files on Android, and `localStorage` under the id on the web. A bare
  `rux run file.rux` with no `rux.toml` uses the file's name.
- **Not a signal.** It is a store: read it into a signal, as above, for what
  the screen shows.
- A file with a `settings` of its own (a `use native::settings;`, a `let`)
  keeps it; the built-in is then `use native::rux::settings;`.

## Clipboard

**`clipboard.writeText(text)`** puts text on the system clipboard, the one a
text field's Copy and Paste use, and **`await clipboard.readText()`** gives
what it holds. Like `settings`, they are Rux's own, so nothing is imported.

```rux
fn copyCurl() {
  clipboard.writeText(`curl -X ${method} ${url}`);
  notice = "Copied";
}

async fn pasteUrl() {
  url = await clipboard.readText();
}
```

- **`writeText` is a plain call**, so a `@tap` can make it, and it is not
  awaited. On the web it starts the browser's write and goes on.
- **`readText` is awaited**: a browser asks the person before it hands the
  clipboard over. It gives `""` when the clipboard holds no text (an image,
  or nothing).
- **Both throw** when there is no clipboard to reach, and `readText` throws on
  the web when the person refuses, or the page is not focused. Catch it with
  `try`, as for any native call.
- Every target: Windows, macOS, Linux, Android's clipboard manager, and the
  browser's `navigator.clipboard` (which needs the page served over https).
- A file with a `clipboard` of its own keeps it; the built-in is then
  `use native::rux::clipboard;`.

## Native code

An app's Rust lives in a `native/` crate beside `rux.toml`, an ordinary
library depending on `rux-native` under the name `rux`. What Rux may call is
marked `#[rux::export]`; each `pub mod` is a module Rux imports as
`native::name` (the crate root is `native` itself):

```rust
// native/src/shop.rs, with `pub mod shop;` in native/src/lib.rs
#[rux::export]
pub struct Product { pub id: u32, pub name: String, pub price: f64 }

#[rux::export]
impl Product {
    pub fn with_tax(&self, rate: f64) -> f64 { self.price * (1.0 + rate) }
}

#[rux::resource]
pub struct Till { cents: Mutex<i64> }

#[rux::export]
impl Till {
    pub fn ring(&self, p: Product) -> Result<u32, TillError> { … }
    pub async fn settle(&self) -> Result<String, TillError> { … }
}

#[rux::export]
pub fn open_till() -> Till { … }
```

```rux
<script>
  use native::shop;
  use type native::shop::Product;

  let till = shop.openTill();
  fn add(p: Product) { count = till.ring(p); }
</script>
```

- **Names become camelCase** (`open_till` is `openTill`); `#[rux(name = "…")]`
  gives another, `#[rux(skip)]` keeps a `pub fn` out.
- **A struct marked `#[rux::export]` is a value**: a record type of the same
  name, every field `pub`, copied across whole. **A `#[rux::resource]` is
  opaque**: Rux holds it, keeps it in state, passes it back and calls its
  methods, and never sees inside. Its methods take `&self`, since Rux may hold
  it in several places; a change goes through a lock inside it.
- **A native value's method is its own, whatever the script names.**
  `studio.save()` calls the resource's `save` even beside a `<script>`'s
  `fn save()`, which a plain `save()` still calls. An app's actions and its
  resource's methods are naturally the same words.
- **A resource may have properties**, read as `value.name` and, when
  writable, written as `value.name = x` (or `+=`), each through Rust: a
  getter and a setter, never a copy. What reads one, a `{{ }}` binding or an
  `effect`, reads it again whenever Rust says something moved
  (`rux::notify()`), so `{{ sound.currentTime }}` follows a playing sound.
  `?.` on one that is `none` is `none`. Writing a read-only one is an error
  that says so. A module written by hand declares them with
  `Module::property`; `#[rux::export]` does not make them yet.
- **Types cross as `docs/11-next.md` lists them**: every integer is an `int`,
  and one that does not fit throws `kind` `"overflow"` rather than wrapping;
  `Option<T>` is `T?`; `Vec<T>` is `T[]`; a `Result`'s `Err` is thrown. A type
  with no Rux meaning is a compile error at the export, saying why.
- **An `async` export is awaited**, from an `async fn`; calling one without
  `await` is an error.
- **A panic in a native call is thrown** as an `Error` of `kind` `"panic"`,
  not a crash (except on the web, which aborts).
- **`rux check` and the editor read the Rust source**, not a build, so a typo
  in a native name or a wrong argument type is reported in seconds. A check
  never runs the app's Rust: a native call made while a document is checked
  answers the empty value of its type. **`rux run` compiles the crate in**, and
  builds and restarts the window whenever something under `native/` changes;
  `.rux` files still hot reload. `rux native` prints what Rux sees.
- **A debug build runs Rux optimised.** Everything the app depends on is
  compiled at `opt-level = 2`, and only the `native/` crate as written, for a
  debugger. A debug window scrolls as fast as a release one (a list of 1,000
  rows scrolls about 7.5 times faster than unoptimised); the price is the first
  build, which takes several minutes once.

### Signals from Rust

**A `#[rux::export] pub static` of type `rux::Signal<T>` is a value Rust sets
and Rux follows.** Script reads it like a module's signal, and whatever read it
updates when Rust sets it, from any thread. Nothing else wakes the window:

```rust
// native/src/sim.rs
#[rux::export]
#[derive(Clone)]
pub struct Frame {
    pub tick: u32,
}

#[rux::export]
pub static FRAME: rux::Signal<Frame> = rux::Signal::new();

fn on_step(frame: Frame) {
    FRAME.set(frame);   // any thread
}
```

```rux
<script>
  use native::sim;
</script>
<text>{{ sim.frame?.tick ?? 0 }}</text>
```

- **Its name follows the `static`'s**, lowercased and camelCased (`FRAME` is
  `frame`, `SAVE_STATE` is `saveState`), or `#[rux(name = "…")]`. Its type is
  `T?`: it holds `none` until Rust first sets it.
- **What it holds is `Clone`.** Every read copies the value out to Rux, so
  `T` needs `#[derive(Clone)]`, and the compiler's error says so at the
  `static`.
- **Only Rust writes it.** Assigning to `sim.frame` in script is an error
  that says so; to change it from script, export a function that sets it.
- **The latest value wins.** `set` stores the value and wakes the window, and
  the document reads what is there when it next settles: a simulation setting
  a frame five hundred times between two paints is drawn once, with the
  newest. A status works the same way, and an `effect` reading it runs when it
  changes. For values that must each arrive, use a
  [channel](#channels-from-rust).
- **A handler reads what the Rust it called just set.** After every call into
  Rust, and when an `await` on one comes back, the signals are read again if
  anything set one. So `studio.newProject(); if mava.view.dirty { … }` tests
  the view the call left, not the one from before it. Between two calls into
  Rust the value holds still: a set made on another thread in the meantime is
  read at the next call, or when the document next settles.
- `get`, `clear` and `update` are there too. A check does not run the app's
  Rust, so a signal read while a document is checked holds `none`.

### Channels from Rust

**A `#[rux::export] pub static` of type `rux::Channel<T>` delivers every
value Rust pushes, in order**: a socket's messages, a log, a benchmark's
samples, where a signal would keep only the newest.

```rust
// native/src/feed.rs
#[rux::export]
pub static EVENTS: rux::Channel<Event> = rux::Channel::new();

fn on_message(event: Event) {
    EVENTS.push(event);   // any thread; `extend` pushes several
}
```

```rux
<script>
  use native::feed;
  let log: Event[] = signal([]);

  effect {
    for e in feed.events { log.push(e); }
  }
</script>
```

- **Script reads it as a list, `T[]`, of what was pushed since the document
  last looked**, which is once a frame. An `effect` that reads it runs once for
  each list with anything in it, so every value reaches it exactly once, and a
  burst of a thousand is one update rather than a thousand.
- **Read it in an `effect`** to keep what arrives. Between arrivals it still
  holds the last list, so a binding shows the latest batch; the batch before it
  is gone.
- A check does not run the app's Rust, so a channel read while a document is
  checked is the empty list.

A program that runs the shell itself registers a module with
`rux_runtime::native::Module`, which is what the attribute generates.

## Type annotations

Rux script takes TypeScript's annotations. They are checked by `rux check`,
the dev overlay and the editor, and erased before anything runs, so a program
with them runs exactly as it would with them deleted:

```rux
type Filter = "all" | "open" | "done";
type Task = { id: int, title: string, done: bool, note?: string };

let tasks: Task[] = signal([]);
let filter: Filter = signal("all");
computed left: int = tasks.filter(t => !t.done).length;

fn label(t: Task, n: int): string { `${n}. ${t.title}` }
let add = (a: float, b: float) => a + b;

type Page<T> = { items: T[], next: string? };
fn first<T>(items: T[]): T? { items?[0] }
```

A type or a function may take type parameters, as `Page<T>` and `first<T>`
do. A call never names them; they are what its arguments make them, so
`first(tasks)` is a `Task?`. `Array<T>`, `Option<T>` and `Map<string, T>` are
the long spellings of `T[]`, `T?` and `{ [string]: T }`.

A function that returns nothing may say so with `: void`. For an error that is
an answer rather than a failure, a function returns a `Result<T, E>`, built
with `Ok(value)` or `Err(error)` and read by its `ok`:

```rux
fn parseAge(s: string): Result<int, string> {
  let n = parseInt(s);
  if n == none { return Err("not a number"); }
  Ok(n)
}

let r = parseAge(text);
if r.ok { age = r.value; } else { problem = r.error; }
```

`r.unwrap()` gives the value, or throws the error.

In a component, `prop label: string;` and `prop price: float = 1;`. A type
declared in one file is used in another with `use type types::Task;` (or
`import type { Task } from "./types";`), which names the `type Task` in
`types.rux`; the word `type` is what tells a type import from a component's. A file holding only a
`<script>` of `type` declarations is a types file, and `rux` will not run it.

`type` is a keyword only at the start of a statement followed by a name, its
type parameters if any, and `=`, so `let type = 1;` still works. An annotation that is not a type is a
syntax error at the place it goes wrong.

**What is checked so far** is a file's `<script>` and its template. In the
script, every `let` against its
annotation or its starting value, every call against the function's
parameters, every result against a declared `): T`, fields read and written
against their record, and a closure handed to `filter` or `map` against the
list's element type. An optional field (`note?: string`), and a field of a
value that may be `none` (`sel: Task?`), must be read with `?.`, and a
dictionary's keys with `?.` or `?[`, except where a check has
already ruled out their absence: inside `if t?.note != none`, after
`"note" in t`, after an early `return`, in the arms of a `switch`, and under
an `r-if` that tested it. A
`switch` on a literal union with no `_` arm must handle every member, and a
comparison that can never be true is an error. A name nothing can type
(`signal([])`, a parameter with no annotation) is a warning and is not checked
further; a contradiction is an error:

```text
app.rux:9: error: "al" is not `Filter`, which is one of "all", "open", "done"; did you mean "all"?
app.rux:14: error: `/` always makes a `float`, where `int` is expected; `intDiv(a, b)` divides to an `int`
app.rux:16: warning: `x` has no type, so what `old` is given there is not checked. Annotate it: `fn old(x: T)`
```

In the template, an `r-for` variable is one element of its list, and every
`{{ }}`, bound attribute and handler is checked as an expression: a bound
attribute against what it takes (`:disabled` a `bool`, a `:class` map's
values `bool`s), a handler with `event` typed by what sends it
(`event.direction` in `@swipe` is one of four strings, a form's
`event.values` a record of its fields), `r-model` against its field's value
type, and a prop against its declaration, at the tag:

```text
app.rux:21: error: `kind` on <btn>: "big" is not `"primary" | "quiet"`, which is one of "primary", "quiet"
```

### `is`, and props at run time

Annotations are erased, so a value from outside (an `any`, a native function
typed `rux::Any`) is whatever it turns out to be. **`x is T`** asks, and
answers `true` or `false` without raising. Inside the check `x` is a `T`:

```rux
let raw = settings.read();   // a native function giving `any`
if raw is Settings {
  settings = raw;
}
```

Every required field must be there with its type, every element and every
union member is tried, an extra field is allowed, and a whole number is an
`int`. `is` binds as `<` does, on both sides: `a + b is int`, `(x is T) && y`,
and `a == b is int` is `a == (b is int)`.

**Props are checked as the component is built**, in release builds too,
since a value from a route or an `any` never met the checker. One that does
not fit is reported with the tag and prop named, and the component is built
without it: the prop's default stands in, or it is left out. A route
parameter is text and becomes what its prop takes, so `prop id: int` on
`/task/:id` gets the number `42` from `/task/42`, and `/task/abc` is reported.

[Types](./10-types.md) is the full reference for the type system.

## Values

**A number is an `int` or a `float`.** A literal without a point or an
exponent is an `int` (`42`), and one with either is a `float` (`42.0`, `2e3`).
An `int` goes wherever a `float` is wanted, and `1 + 0.5` is a `float`; a
`float` becomes an `int` only by asking, with `x.trunc()`, `x.round()`,
`x.floor()` or `x.ceil()`, and an `int` says `n.toFloat()` for the other way.
`+`, `-`, `*` and `%` of two `int`s make an `int`. **`/` always makes a
`float`**, so `10 / 3` is `3.333…`; `intDiv(10, 3)` is `3`, cutting toward
zero. A list is indexed by an `int`. So `let n = signal(0)` holds an `int`, and
`n = n / 2` is an error whose fix is to say which: `let n: float = signal(0)`,
or `intDiv(n, 2)`. There is no `number` type; `rux fmt` rewrites it to
`float`.

The checker keeps the two apart, and so does the interpreter: an `int` is a
whole 64-bit number and `int` arithmetic that goes past its limits is an error,
not a wrapped or rounded answer. The runtime still holds one kind of number, so
a value that goes out to the tree and comes back (a component's state, an
`r-for` row, `event`) comes back a `float` holding the same number. Division by
zero of `float`s gives `Infinity` or `NaN` as in JavaScript, and those display
under those names.

Strings are double-quoted. **Single quotes hold one character**, which is a
string of one character (`'x' == "x"`), so `'#note'` is a syntax error. This is
the most common thing to trip over. A selector or any other text needs `"…"`.
Inside a `@tap="…"` attribute there is no room for double quotes, which is the
practical reason to name a handler and call it:

```rux
<!-- wrong: '#note' is more than one character in single quotes -->
<button @tap="query('#note')[0].focus()">

<!-- right -->
<button @tap="focus_note()">
```

Object literals are **`{ key: value }`**, as in JavaScript. A key can be a name
or a string (`{ "x-y": 1 }`), and `{}` is an empty map. A `{` whose first thing is
not `key:` is a block, as it is after `if`:

```rux
let row = { id: 1, name: "ada" };      // a map
let total = { let t = 0; t + 1 };      // a block, whose value is 1
rows.map(r => { id: r.id });           // returns a map; JS needs ({ … }) here
let noop = () => {};                   // an empty body, not a map
```

Two things JavaScript has that this does not: shorthand (`{ a, b }` for
`{ a: a, b: b }`) and spread in a map (`{ ...o, a: 1 }`). `#{ key: value }` is
read as a map too, and `rux fmt` rewrites it to `{`.

`none` is the empty value. It is a literal rather than a variable, so it cannot
be shadowed and nothing can subscribe to it. `none` is a keyword, so it cannot
name anything. `null` and `()` are errors that say to write `none`, and `rux
fmt` rewrites both.

### Truthiness

**JavaScript's rules exactly.** Falsy: `0`, `NaN`, `""`, `none`. Everything
else is truthy, **including an empty array and an empty map**.

```rux
r-if="items"           <!-- "items exists", NOT "there are items" -->
r-if="items.length"    <!-- what you meant -->
```

## Operators

| Operator | Notes |
|---|---|
| `===` / `!==` | Mean what `==` / `!=` mean. Both spellings are the strict comparison; there is no loose `==` to choose between |
| `x++` / `x--` | Statement position only, desugared to `+= 1` / `-= 1` |
| `?.` | Guards an absent base **and a missing property** |
| `?[` | Guards an absent base, **a missing key and an index past the end** |
| `??` | The value on the right when the left is absent |
| `=>` | Arrow functions, in all four shapes: `x =>`, `() =>`, `(x) =>`, `(a, b) =>` |

An arrow kept in a variable is called like a function, as in JavaScript:
`let add = (a, b) => a + b;` then `add(2, 3)`. It sees and writes what is
around it, like any closure. A `fn` of the same name wins over the variable.

`?.`, `?[` and `??` are not conveniences. Rux turns on strict property access
for every document, so a typo like `user.nmae` raises instead of quietly
evaluating to nothing, and a missing key read with `[` raises the same way.
These are the way to say "absent is a legitimate answer here":

```rux
{{ user?.nickname ?? "none" }}
```

## Built-in methods

JavaScript's names, with JavaScript's answers:

- **Arrays:** `map`, `filter`, `reduce`, `forEach`, `find`, `findIndex`,
  `findLast`, `findLastIndex`, `some`, `every`, `includes`, `indexOf`,
  `lastIndexOf`, `at`, `slice`, `concat`, `flat`, `flatMap`, `join`,
  `toReversed`, `toSorted`, and the ones that change the array: `push`, `pop`,
  `shift`, `unshift`, `splice`, `sort`, `reverse`, `fill`. Also
  `insert(i, x)`, `remove(i)`, `truncate(n)` and `clear()`.
- **Strings:** `split`, `charAt`, `at`, `repeat`, `startsWith`, `endsWith`,
  `includes`, `indexOf`, `lastIndexOf`, `slice`, `substring`, `trim`,
  `trimStart`, `trimEnd`, `padStart`, `padEnd`, `replace`, `replaceAll`,
  `toLowerCase`, `toUpperCase`.
- **Numbers:** `toFixed`, so `(1.4123).toFixed(1)` is `"1.4"`.
- **`Math`, `JSON`, `Object` and `Array`**, as namespaces: `Math.max`,
  `Math.min`, `Math.floor`, `Math.ceil`, `Math.round`, `Math.trunc`,
  `Math.abs`, `Math.sign`, `Math.sqrt`, `Math.pow`, `Math.log` and the rest,
  with `Math.PI` and `Math.E`; `JSON.stringify(v)`, `JSON.stringify(v, none,
  2)` indented, and `JSON.parse(text)`; `Object.keys`, `Object.values` and
  `Object.entries`; `Array.from` and `Array.isArray`. A file that declares a
  `Math` of its own keeps it.
- **`Date`, as functions on a time**: `Date.now()` is the wall clock, an
  `int` of milliseconds since 1970 as JavaScript's is, and each of the rest
  takes such a time and reads it in the computer's own time zone.
  `Date.time(ms)` is `"14:02"` and `Date.date(ms)` is `"30 Oct 2026"`;
  `Date.hours`, `Date.minutes`, `Date.seconds`, `Date.day` (of the month),
  `Date.month` (**1 to 12**, where JavaScript's `getMonth` counts from 0),
  `Date.year` and `Date.weekday` (0 for Sunday to 6, as `getDay`) are `int`s.
  How long ago is arithmetic: `intDiv(Date.now() - sentAt, 60000)` minutes.
  There is no `Date` value and no `new`: a time is its milliseconds.

A string's methods **return** a new string and leave their receiver alone, as
JavaScript's do: `name.replace("-", " ")` replaces the first `-` and gives the
result back. `rux check` says so where a statement throws the new value away:
`s.replace("a", "b");` is answered with "write `s = s.replace("a", "b")`". An
array's `sort`, `reverse` and `fill` change the array, as JavaScript's do, and
also give it back; `toSorted` and `toReversed` leave it alone.

**Spread in an array**, as JavaScript's: `[...a, x]` is `a`'s items and
then `x`, `[...a, ...b]` both in order, and `[..."hi"]` the characters. It
makes a new array and leaves `a` alone. Spreading anything but an array or a
string is an error.

Not there: spread in a map or record, `{ ...o, a: 1 }`, and in a call's
arguments, `f(...xs)`; `Math.random`; and a `Date` value with methods (`Date.now()`
and the functions above take its place).

`forEach` is called with `(item, index)` and falls back to `(item)`, and
`sort((a, b) => a - b)` sorts by the comparison and **returns** the sorted
array.

**Text to a number**, under JavaScript's names and with its answers:
`Number("2")` is 2, `Number("")` is 0, `Number("12px")` is `NaN`;
`parseInt("42px")` is 42 and `parseFloat("12.5px")` is 12.5, reading only what
leads. Where JavaScript's parsers say `NaN`, these say `none`: `parseInt` gives
an `int?` and `parseFloat` a `float?`, so `parseInt(s) ?? 0` is the usual way
to read one. `isNaN(x)` asks of a `float`. `String(x)` goes the other way. A
route parameter is always text, so this is how `/task/:id` meets a list whose
ids are numbers: `tasks.find(t => t.id == parseInt(params.id))`.

**`now()` is a clock**: milliseconds, as a `float`, on a clock that only goes
forward. Only the difference between two reads means anything, so it times
things rather than telling the time (`Date.now()` does that):

```rux
fn send() {
  let started = now();
  // …
  took = now() - started;
}
```

**`.length` is a property, not `len()`.** Arrays and strings only: JavaScript has
no `length` on a plain object, and inventing one for maps would be making up a
rule rather than matching a known one. Use `keys(m)` and `values(m)` for maps.

**A record keeps its fields in the order its type declares them.** `${item}`,
`{{ item }}`, `keys(item)` and `values(item)` go in that order, as JavaScript,
Dart and Kotlin show an object, and a `{ }` that no type describes keeps the
order it was written in. A map (`{ [string]: T }`) keeps its keys sorted, and
gains a key with `m.set("k", v)`: `m["k"] = v` writes a key that is already
there, and says so when it is not. An
optional field never given reads as `none` (`bob.pet`, on a `User` made
without a `pet`), and a field its type marks optional is left out where the
record is shown or walked while it holds `none`, so `${bob}` is `name: bob`.
Writing a field a declared record type does not have is an error the checker
reports; reached anyway, through `any`, it throws with `kind` `"type"`. Two
records are equal when their fields are, whatever order they were written in.

## Talking to the outside

| Call | Does |
|---|---|
| `print(x)`, `debug(x)` | Printf-debugging. Reaches the **dev overlay**, not just stderr, because nobody running a GUI is watching stderr: a badge in the bottom right corner with the last line, which a tap opens into the full list. A print never brings back a panel dismissed for warnings |
| `module.name(…)` | A native function, from `use native::module;`. See [Native code](#native-code) |
| `emit("name")`, `emit("name", payload)` | A component telling its caller something happened |
| `navigate(path)`, `replace(path)` | Router. `replace` is the only way to redirect: `navigate` leaves the redirecting page in the history, so Back returns to it and redirects again |
| `back()`, `forward()` | Walk the history |
| `path_for("route")`, `path_for("route", { id: x })` | Build a path from a named route |

**`log` is a logarithm**, not a logging function: `log(2)` returns `0.301`
(base 10), `log(x, base)` takes the base, and `Math.log` is the natural one.
Use `print`.

`emit`, `navigate` and the history verbs record an intent rather than acting
immediately. What they mean belongs to the runtime, and they are applied once the
handler and everything it set off have finished, so a handler that navigates and
then writes a signal does not render the old route with the new state in it.

## Reading the tree

**`query(selector)`** returns the elements matching a CSS selector, in document
order. It takes the same selectors the stylesheet takes, because it is the
stylesheet's own matcher: tags, ids, classes, `role`, and the `>`, `+` and `~`
combinators.

```rux
fn count() {
  report = query(".card").length + " cards";
}
```

**It works in a handler and nowhere else.** In a `{{ }}` binding, a `:style` or
an `r-if` it raises and the overlay says why. A binding that read the tree would
have to invalidate whenever layout changed, and invalidating it rebuilds and
relayouts, which never settles. Every real use of it is handler-shaped.

**It matches the tree, not the template.** A `<view r-if="open">` that is closed
is not there to be found. **A selector that does not parse is an error**, not an
empty list, so a typo cannot look like "nothing matched".

Each result carries:

| Property | Notes |
|---|---|
| `tag`, `id`, `classes` | `id` is absent rather than empty when there is none |
| `x`, `y`, `width`, `height` | The laid-out box, in absolute window pixels |

**Geometry is one frame stale**, exactly as `getBoundingClientRect` is in a
browser: a handler runs before the next layout, so it reads what the last one
produced. It is **absent rather than zero** when there is no frame to read, which
covers both a node hidden by `r-show="false"` and a document that has not been
laid out at all. Under `rux check` it is always absent, since checking runs with
no window and no GPU on purpose.

```rux
let card = query(".card")[0];
report = card.width ?? "not laid out yet";
```

## Acting on an element

None of these edits the tree, which is why they can exist at all. They move host
state, and the next build produces the tree it would have produced anyway.

| Call | Does |
|---|---|
| `el.tap()` | Presses the element as a finger would |
| `el.focus()` | Puts the caret in a text input |
| `blur()` | Drops focus entirely |
| `el.scrollIntoView()` | Scrolls the containing scroller until it is visible |
| `el.setSelectionRange(start, end)` | Selects characters `start` to `end` of a text field, the caret at `end`, and gives it focus |
| `el.insertText(text)` | Replaces a text field's selection with `text` as typing would: one undo step, `@input` after it, the caret after the text |
| `el.clearHistory()` | Forgets a text field's undo and redo; its text and focus stay as they are |
| `el.play()`, `el.pause()` | Starts or pauses an `<audio>` or `<video>` |

**A text field's selection** is `el.selection`, `{ start, end, caret }` in
characters, as strings index, the caret at one end of it. It is where the
selection is while the field has focus and where it was left after, so a button
beside the field reads it although tapping the button moved focus away. A field
that has not had focus, and anything that is not a text field, has `none`.
`focus()` on a text field puts that selection back; a field never focused gets
its caret at the end. `@select` on the field runs each time the caret or the
selection comes to a new place, with `event.start`, `event.end`, `event.caret`
and `event.value`.

```rux
fn complete(name: string) {
  let q = query("#query")[0];
  let s = q.selection;
  if s != none {
    q.setSelectionRange(wordStart(text, s.caret), s.caret);
    q.insertText(name);
  }
}
```

`insertText` replaces where the field's selection is, or was left; a
`setSelectionRange` earlier in the same handler sets what it replaces.

**`el.tap()` is the whole gesture, not a way to call the handler.** It presses the
centre of the element through the same dispatch a real pointer goes through, so
it runs the `@tap` handler, follows a `to=` link, toggles a checkbox or radio,
opens a `type="select"` dropdown, moves keyboard focus and puts the caret in a
text input. It cannot drift from a real press because it is one.

Two consequences follow from that and are worth stating:

- **It hit-tests.** The topmost element at that point wins, so tapping something
  covered by an open dropdown taps the dropdown, as a finger would. A box with
  no `@tap` counts too: a tap on a button under a backdrop is the backdrop's,
  and runs the `@tap` of the backdrop or its nearest ancestor that has one.
- **An element with no box cannot be tapped**, and says so.

A tap runs a handler that can tap again; the chain is cut off after eight rounds,
the same bound as an `emit` chain, so a button that taps itself stops rather than
hanging the window.

**`focus()` is not a tap.** It only puts the caret in a text input: no handler,
no link, no toggle. Only a text input can take focus, because focus is keyed by
`r-model`, and asking anything else says so rather than half-focusing.

`blur()` is free-standing rather than a method, because there is only one focused
element; asking a particular one to blur would either do nothing or take focus
from something else.

## Errors

Every event handler is **compiled when the document loads**, so a syntax error in
a `@tap` is reported rather than waiting to be discovered by whoever taps it.
Handlers in branches that are not currently rendered are checked too, since a
false `r-if` is where a broken handler hides longest. `rux check` goes on to
check a handler's names and types against the script (see [Type
annotations](#type-annotations)).

**A syntax error is Rux's own.** Every script, binding and handler is read by
Rux's parser, so what is wrong is said in this language's words, at the line and
column where it goes wrong: ``expecting `;` to end this statement, found `let` ``.

A failing expression is **reported, not swallowed**. It reaches the dev overlay
and `rux check`: a missing property, an undefined variable, a missing function
and a reserved word all say what they mean in this language's vocabulary.

`print` output is kept apart from warnings. A leftover `print` is not something
wrong with the document and must not fail a build.

## Reserved words

These cannot name a variable, a function or a parameter. A map key that is
one is written as a string, `{ "for": 1 }`.

```text
as async await break catch const continue do else export false fn for if
import in let loop none private return switch throw true try until while _
```

These are not keywords, and are still not names. `use` and `is` are a
statement and an operator that already work; `match` and `yield` are held for
the language. `this` is reserved. `null`, `nil`, `var` and `new` are other
languages' spellings, and their error says what Rux writes instead (`none`
for `null`, which `rux fmt` writes).
`void` is a type. `print`, `debug`, `type_of`, `eval`, `call`, `curry`,
`is_def_var` and `Fn` are built-in functions: they can be called, not declared.

```text
use is match yield null nil this var new void
print debug type_of eval call curry is_def_var Fn
```

`signal`, `computed`, `effect`, `mounted`, `unmounted` and `prop` mean
something only at the start of a top-level statement, so `let effect = 1;`
inside a function is an ordinary name.

Everything else is a name, including words other languages reserve:
`default`, `case`, `static`, `spawn`, `module` and `with` are all free.

## Deliberately absent

- **Tree mutation.** No setting a property, no adding or removing children. A
  state change regenerates the affected tree from the template, so any such edit
  would be overwritten by the next patch, reconcile or rebuild, silently, at a
  moment decided by an unrelated handler. The tree is a function of state, and
  state is how it changes.
- **`setTimeout`.** Animation absorbs the cases behind it.
- **A free-floating repeating timer.** A callback on a period that nothing
  owns never lets the app idle again, which nobody notices on a desktop and
  which is the whole battery story on a phone. What Rux has instead is
  [`setInterval(ms) { … }`](#intervals): a block, owned by the component
  instance that started it and stopped when that instance goes.
- **Bare `{ }` object literals.** See above.

## If you know JavaScript

The things most likely to surprise, in the order they usually do:

1. **`'x'` holds one character**, and `'ab'` is a syntax error.
2. **Object literals have no shorthand**: `{ a: a }`, never `{ a }`.
3. **`.length` on arrays and strings only**, not on objects.
4. `let` declares state only at the top level of `<script>`; `signal()` marks it.
5. **`/` always makes a `float`**: `10 / 3` is `3.333…`, and `intDiv(10, 3)`
   is `3`.
6. `x++` is a statement, so `let a = x++` is not a thing.
7. There is no `undefined` distinct from `none`.
8. **A handler cannot `await`.** It calls an `async fn`, which does, and
   there is no `Promise` to `.then`.
