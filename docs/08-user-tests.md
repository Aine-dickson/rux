# 08. User test cases

**Every feature and every release records the cases a person actually drove, and
what those runs found.** Written down here, not in a chat log and not only in a
commit message.

The reason is narrow and evidence-backed: the tests in this repo are good at
what they were written for and blind to what nobody thought of, and almost every
expensive bug in Rux so far was found by a person using the thing rather than by
CI. Touch spent releases half-built because there was no hardware here; it broke
within a minute of a phone opening the playground. A swipe was unreachable on
any element that also declared a drag, and the suite was green through all of
it. **As mobile approaches this gets worse, not better**, because the gap
between what the machine here can exercise and what a user's device does is
about to widen.

## How to use this file

One section per feature or release. For each, the case as a person would perform
it, on what hardware, and the outcome. A case that **found** something is worth
more than a case that passed, so record those first and say what changed.

State the hardware explicitly. "Works" on a desktop with a mouse says nothing
about a phone, and this document exists to stop that sentence being written.

An unverifiable case is recorded as unverifiable, with the reason. A gap that is
written down is a gap someone can close; a gap that is assumed to be covered is
the one that ships.

**Write the cases before the release, not after it.** Standing rule as of
2026-08-19. Every capability a release adds gets its cases written here *while
it is being built*, pointing at what the new thing can now do, so there is a
list for a person to drive **before** the release goes out rather than a record
assembled afterwards from what happened to be tried. A case written afterwards
only ever documents what somebody thought to look at; a case written up front
is the thing that gets looked at.

The cases below are therefore a mix: outcomes where the run has happened, and
**open** where it is waiting for a person. Open cases are release-blocking in
the sense that shipping with them untouched is a decision, not an oversight.

---

## v0.7

### Lifecycle hooks, per instance (2026-08-17)

| Case | Hardware | Outcome |
|---|---|---|
| Two `<session>` cards mount, each numbering itself | desktop, window | Passed: instance 1 and 2, document counter at 2 |
| A card leaves and writes its state out on the way | desktop, window | Passed: the leaving instance's `unmounted` reached the screen |
| A component that mounts and is pruned before either hook ran | desktop, window | **Found a bug.** The mount body ran with no instance to scope it to and reported every one of the component's own names as undefined. Fixed by the pairing rule: `unmounted` never fires unless `mounted` fired |
| A hook that writes a signal *and* its own state | desktop, window | **Found a bug older than the feature.** Only the document's half reached the screen, because a patch is chosen by which document signals moved and instance state is not among them |

### `setInterval` (2026-08-17)

| Case | Hardware | Outcome |
|---|---|---|
| A counter runs 0 to 5 a second apart and stops itself from inside its own body | desktop, window | Passed |
| CPU while every timer is stopped | desktop, 5s sample | Passed: 0 ms, so the window really does sleep |

### Component `computed` and `effect` (2026-08-18)

| Case | Hardware | Outcome |
|---|---|---|
| Three rows, each deriving its own line total from its own quantity | desktop, window | Passed |
| A computed reading a document signal | desktop, window | Passed, after a fix: instance creation runs the script in a scope without the document's signals, so the computed is declared as a placeholder and evaluated at mount |

### Nested routes (2026-08-18)

| Case | Hardware | Outcome |
|---|---|---|
| `/crew/ada` keeps the list on screen with the panel in its outlet | desktop, window | Passed |
| `/crew` fills the outlet with the index route | desktop, window | Passed |
| `params` outside the router reads a *child's* capture | desktop, window | Passed |
| `path_for` on a name written on a child route | desktop, window | Passed: resolved to `/crew/ada` |
| A parent view that forgets its `<router-view />` | test | **Found a gap.** The warning existed but never reached the overlay: only a full rebuild drained the warning sink, and a navigation reconciles instead |

### The pointer vocabulary (2026-08-18)

| Case | Hardware | Outcome |
|---|---|---|
| Press, release and long press on a pad | desktop touchpad | Passed |
| Drag: start, move, end, with distances | desktop touchpad | Passed |
| Swipe | desktop touchpad | **Found a bug.** Unreachable: swipe and drag were exclusive, so any element declaring both could never swipe. A drag that ends as a flick now fires `@swipe` too |
| Which frame the coordinates are in | desktop touchpad | **Found a design gap.** Only element-local and cumulative distances existed; `pageX` / `pageY` and per-event `moveX` / `moveY` were added, and `dx` / `dy` renamed to `totalX` / `totalY` because `d`-anything reads as "since last" to half its readers |
| More than one finger | **unverified** | A laptop touchpad reaches the app as a mouse and reports one finger however many are on it. Needs a touchscreen, or Edge with CDP touch emulation against the wasm build |

### Enter, leave and route transitions (2026-08-18/19)

Everything here is new in v0.7. The first five were driven and each one found
something; the rest are written for a person and are **open**.

| Case | Hardware | Outcome |
|---|---|---|
| A panel opening and closing with `r-transition` | desktop, window | Passed: caught mid-leave, faded and moved, with the card below sliding up behind it |
| A card dragged sideways under `:r-transition`, released past the threshold | desktop mouse | Passed: commits and the card goes |
| The same, released short of the threshold | desktop mouse | **Found two bugs.** The reversal set off from the far end rather than from where the finger left it, because the track's deadline had long expired; and driven progress was being run through the CSS easing, so the card outran the hand. Both fixed |
| A navigation with `r-transition` on the `<router>` | desktop, window | **Found a bug.** The incoming page appeared at its final position and only the outgoing half moved: the settling rebuild ran before the paint, so the `:enter-from` frame was never drawn and the animator's first sight was the final style |
| A page mid-navigation, looked at closely | desktop, window | **Found a bug.** The incoming page was invisible at `opacity: 0` but its scrollbar drew at full strength over the outgoing page. Scrollbars, focus rings and hit regions were all drawn outside the content's transform and opacity |
| Dragging the card with the panel above it closed | desktop mouse | **Found a bug.** The old jump came back: the animator names an unkeyed node by its sibling index, so a sibling leaving renamed the card and dropped its track mid-drag |
| A departing element's space, and what fills it | desktop, window | **Found a bug.** The space was handed over at the *start* of the swap, so what followed jumped up while the element was still on screen. Now the author decides, and the default is to keep the place until it commits |
| Tapping a page while it is still transitioning | **open** | The fix is in (hit regions follow the transform) but nobody has tapped a moving page. It failed silently before: the tap landed where the page used to be |
| A keyed `r-for` row removed from the middle of a list | **open** | Built and tested, never watched. The row should animate out where it sat, not from the bottom |
| Two banners in an `r-if` / `r-else` chain crossing over | **open** | They are authored to leave in opposite directions and did not appear to, because both held their space; expected to read as a crossover now |
| A route transition driven by a drag rather than the clock | **open** | `:r-transition` works on `<router>`, so a navigation can follow a finger. Never tried; no example does it yet |
| Any of it on a touchscreen | **unverified** | Every run above was a mouse. A drag-driven swap is the case that most wants a real finger, and the axis claim has never met one |

### `<path>`, vector geometry (2026-08-19)

New in v0.7 and added mid-milestone at the user's request. **Every case here is
open**, and for one reason worth stating rather than burying: **screen capture
from the agent environment returned pure black on this machine today**, for a
known-good example as well as for the new one, so nothing below has been *seen*
by anybody yet. What has been done instead is to assert it, which is not the
same thing and is why these are open rather than passed.

| Case | Hardware | Outcome |
|---|---|---|
| Open `examples/morph.rux` and tap through square, circle, blob | **open** | The shape should *walk* between the three, not cut. It is the whole claim of `transition: d`. A cut means the command sequences stopped matching |
| Watch the fill and the outline during that walk | **open** | The colour and the stroke width animate alongside the geometry, on the blob's `:class`. They are ordinary transitions on ordinary properties and should arrive together, not in two stages |
| Open `examples/chart.rux` and tap "jolt them all" | **open** | Every reading moves and the count does not, so the line should travel to its new shape rather than jump |
| Tap "add a reading", then "drop the last" | **open** | The point count changes, so the sequences no longer match and the line is *expected to jump*. Confirming the jump matters as much as confirming the walk: it is the documented rule, not a bug |
| Look at the filled band under the line | **open** | The band and the line are two paths over the same points. They should stay registered with one another through a redraw, and the fill should sit under its own stroke |
| Paste real path data from a design tool into a `<path>` | **open** | Arcs, smooth continuations and relative commands all parse, and the tests say the geometry is right. Whether a genuine exported icon *looks* right has never been tried |
| A path with `alt=` under a screen reader | **unverified** | It reports as an image with that label, and without `alt` it is left out of the tree as decoration. Never driven with assistive technology |
| Any of it at a non-1x DPI, or on a phone | **unverified** | Geometry is in logical pixels like every other length, so it should scale with the scene. No device |

What is asserted rather than seen, so that the gap is legible: 19 tests over the
`d` grammar, including that an arc really lands on its ellipse and that a square
and a circle share a command sequence; and two tests that load the shipped
examples, drive them the way the buttons do, and check the geometry that comes
out. See `crates/rux-layout/tests/path_data.rs` and the two path cases in
`crates/rux-runtime/tests/examples.rs`.

## Standing gaps

Cases nothing here can currently exercise. They are the shape of what v0.8 has
to prove.

- **Two or more fingers**: reported by the runtime, never yet produced.
- **Kinetic scrolling and inertial fling**: unimplemented, untestable here.
- **The axis claim in full**: a `@drag` claims the finger, but whether a scroll
  can take it back mid-gesture needs a real screen to have an opinion about.
- **Native pickers, safe areas, orientation, density**: no device.
- **Looking at the window at all, from the agent environment**: screen capture
  came back pure black on 2026-08-19, for a known-good example as much as for a
  new one, so it is the capture path and not any one feature. Every visual claim
  made that day is an assertion, not an observation.
