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
| Tapping a page while it is still transitioning | desktop mouse, slow motion | Passed. Tapped a crew row while the crew page was still sliding in, and it opened `/crew/grace`. The hit region followed the transform, which is what used to fail silently: the tap landed where the page had been |
| A keyed `r-for` row removed from the middle of a list | desktop, window | Passed. Tapped the middle of five: it faded and slid right **from where it sat**, while the two rows below it moved up behind it. Not from the end of the list |
| Two banners in an `r-if` / `r-else` chain crossing over | desktop, window | **Found that the example was wrong, not the engine.** They queued: "saved" arrived in place and "nothing to save" departed *below* it. Both branches are live during a swap, so the place the `r-else` would have had is after the `r-if`, and a departing box with no inset keeps the place it would have had. The router escapes this only because it builds the outgoing page first. A shared positioned box with the leaver pinned to its origin is what makes it a crossover |
| A route transition driven by a drag rather than the clock | desktop mouse | Passed. Dragged rightwards across the detail page: the navigation opened at the threshold, both pages moved with the hand, and it committed on release with `go forward` correctly lit. **Writing it found a bug** first: a swap handed `null` from the outset was never told its duration and ran for 0ms, so a tab-driven navigation cut instead of animating |
| Any of it on a touchscreen | **unverified** | Every run above was a mouse. A drag-driven swap is the case that most wants a real finger, and the axis claim has never met one |

### `<path>`, vector geometry (2026-08-19)

New in v0.7 and added mid-milestone at the user's request. Driven in the window
the same day, once the screen was unlocked. **Three of the first four cases
found a bug**, none of which the 615 passing tests had an opinion about, and two
of the three stopped something dead rather than making it slightly wrong.

| Case | Hardware | Outcome |
|---|---|---|
| Tap through square, circle and blob in `examples/morph.rux` | desktop, window | **Found a bug, and it needed eyes.** The square set off towards the circle and **froze partway**, and stayed frozen. `close` had no arm for path geometry, so it fell through to "not close" every frame; that comparison is what tells the animator's own last write apart from a fresh authored value, so every frame decided the author had changed the shape, restarted the track, and set the target to what it had itself just written. Adding a variant to `AnimValue` means extending that guard and nothing forces you to |
| Watch the fill and the outline during the walk | desktop, window | Passed: the blob arrives warm yellow with a thicker outline, and the colour and width travel with the geometry rather than in two stages |
| Look at the filled band under the line in `examples/chart.rux` | desktop, window | **Found a layout bug.** The line was drawn a whole band's height **below** its band, outside the frame and across the buttons. The static-position pass put every no-inset absolute box back in the flow *at once* to discover where each would have stood, which measures each against the others; a box that holds no space cannot push its sibling down. Now one goes back at a time |
| Tap "jolt them all" | desktop, window | Passed, after the two fixes above: every reading moves and the count does not, so the whole line travels to its new shape |
| Tap "add a reading" | desktop, window | Passed, and the **jump is the point**. The point count changes, so the sequences no longer match and the line cuts. That is the documented rule, and an example that only showed the walk would hide half of the contract |
| Read path data with arcs, smooth continuations and relative commands | desktop, window | Passed: the circle is four quarter-turn arcs and draws as a true circle, and the wave is `C` followed by `S` |
| Paste a real exported icon from a design tool | **open** | The grammar is complete and tested, but no genuine tool export has been through it |
| A path with `alt=` under a screen reader | **unverified** | It reports as an image with that label, and without `alt` it is left out as decoration. Never driven with assistive technology |
| Any of it on a phone | **unverified** | Geometry is in logical pixels like every other length, so it should scale with the scene. No device |

One example flaw worth recording separately, because it was authoring and not
the engine: the chart was written 480 logical pixels wide and the default window
is narrower, so it ran off the right edge. A drawing does not resize with its
box, deliberately, which means a fixed-coordinate path has to be written to fit.

### Slow motion, and what it exposed (2026-08-19)

| Case | Hardware | Outcome |
|---|---|---|
| Turn on slow motion in `examples/router.rux` and navigate | desktop, window | **Found a bug, and a silent one.** The navigation ran at full speed: `.stage.slow .page` never matched. A component tag expanded with an **empty ancestor chain**, so a document's simple selector (`.page`) reached a component root and a descendant selector did not. Rules cascading into components was settled in v0.7 and only half of it worked. The caller's chain is handed in now, and `<style scoped>` still keeps the caller out |
| The same navigation once it was fixed | desktop, window | **Partly. The incoming page animates over the full duration; the departing page is not something you can see leave.** Disputed by the user, who said they only ever saw the arrival, and they were right to. Measured rather than eyeballed afterwards: sampling the heading band shows brightness collapse within ~150ms of the click and then climb back over ~1400ms, which is the arrival alone |
| Does a departing page animate at all? | desktop, purpose-built probe | Yes. **The first probe was a bad test and the user said so**: page B was nearly empty, which controlled away the very thing under suspicion. Rebuilt with both pages equally prominent, overlapping exactly, and coloured into separate channels so each can be measured through the other. The departing page fades smoothly across the full duration, and it still does with a scroller, with `:r-transition` holding `null`, and with the duration coming from a `.stage.slow .page` override. So the mechanism is sound and the router example is doing something else |
| Navigate **more than once** | desktop, window, measured by colour channel | **Found the real bug, and only a sequence could.** The user: *"on the very first routing from home to crew after launching, the cross fade is perfect but any proceeding ones don't"*. Every check before this had navigated once, so the defect sat outside the test. Measured across four navigations: the first faded (255 to 99 over 800ms), and the second, third and fourth hit zero in ~100ms. **A finished track kept `from` at the value it set off from**, and `value_at` answers with `from` once a track is no longer active, so the next transition on that node was handed a stale start. A page that had faded in 0 to 1 was asked to leave and ran 0 to 0. The first navigation worked and hid it, because that page's track came from `Track::settled`, where `from` and `target` agree |
| The same sequence after the fix | desktop, window, five navigations | Passed, and identically each time: leaving 255, ~185 at 400ms, ~105 at 800ms, gone. **Not only route transitions**: anything that animated and then animated back (a hover ending, a class going off) was setting off from the wrong end |
| Where the leave ends | desktop, window | **Still short, and still open.** The departing page is removed at ~850ms of a declared 1400ms while around 39% opaque, so there is a small pop at the end of an otherwise correct fade. Separate from the bug above and not yet explained |

This is the fourth time this project has shipped a rule that applied to only
half of what it claimed (`rem` honored by half the box model, a swipe made
unreachable by a drag, decorations drawn outside their transform, and now the
cascade). Every one of them was silent, and every one was found by a person
looking rather than by the suite.

## Standing gaps

Cases nothing here can currently exercise. They are the shape of what v0.8 has
to prove.

- **Two or more fingers**: reported by the runtime, never yet produced.
- **Kinetic scrolling and inertial fling**: unimplemented, untestable here.
- **The axis claim in full**: a `@drag` claims the finger, but whether a scroll
  can take it back mid-gesture needs a real screen to have an opinion about.
- **Native pickers, safe areas, orientation, density**: no device.
- **A locked screen captures as pure black.** Not a standing gap, but worth
  knowing: on 2026-08-19 every screenshot came back black until the user
  unlocked the machine, including one of a known-good example. Run the control
  before concluding a feature is broken, and if the control is black too, it is
  the screen and not the code.
