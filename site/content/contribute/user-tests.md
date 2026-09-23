+++
title = "User test cases"
description = "What a person actually drove for each feature, on what hardware, and what those runs found."
weight = 6
+++

<!-- GENERATED FROM docs/08-user-tests.md BY site/sync-docs.sh. DO NOT EDIT HERE. -->


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
| Where the leave ends | desktop, window, four navigations, plus a harness running the shell's frame order | **Not a bug, and the earlier reading was the measurement's floor.** Recorded above as "removed at ~850ms of a declared 1400ms while around 39% opaque". That sample was taken through a band both pages occupy, so what fell to nothing at ~850ms was the band's contrast, not the departing page: the arrival's brightness was climbing through the same pixels. Driven again, the page being left is plainly readable at 850ms, still visible at 1340ms, and gone by 1880ms, over four navigations in a row. Measured exactly, a harness that calls `advance_swaps`, the animator and `settle_swaps` in the shell's own order puts it at 1.00 on the first frame, 0.50 at 700ms, and 0.0015 on the frame the swap commits, at 1408ms of a 1400ms declaration. The same harness against the tree before `320a9cb` puts it at 0.000 for every frame, so what was being watched was the stale-`from` bug, and nothing outlived its fix |

**Read a measurement's floor before believing what it says.** Sampling a band
that both halves of a crossfade occupy cannot tell a departure from an arrival:
peak brightness there is the greater of the two, so the departing page appears
to end the moment the arriving one passes it. Two of the readings in this table
were taken that way, and one of them was written up as an open bug that had
already been fixed. Give each half its own colour channel, or read the value the
animator wrote rather than the pixel it produced.

This is the fourth time this project has shipped a rule that applied to only
half of what it claimed (`rem` honored by half the box model, a swipe made
unreachable by a drag, decorations drawn outside their transform, and now the
cascade). Every one of them was silent, and every one was found by a person
looking rather than by the suite.

### Recipes (2026-08-19)

Every recipe was driven in the window before its page was written, which is the
point of writing them: three defects came out of three ordinary patterns.

| Case | Hardware | Outcome |
|---|---|---|
| `examples/recipes/message-list.rux`: send, and make a message arrive six times | desktop, window | **Found a bug in `scrollIntoView`.** The list did not follow its newest row and said nothing. The shell chose which scroller to move by asking whose *visible* rectangle held the element, so a row past the bottom belonged to no scroller and the reveal was dropped. Only reveals already a nudge from being visible worked. Matched against the scroller's content now, and the list follows |
| The same list, before the rows were wrapped | desktop, window | **Found an author trap, not a bug.** `query` hands back a path, which is a position among siblings. With the anchor written straight after the `r-for`, every new message pushed it along one, so the captured path named whatever slid into its place and the thread scrolled to the middle. It lands on a real element, just not the one asked for, so nothing reports it. Fixed by wrapping the rows so the anchor's position cannot move |
| Any of the three, first render | desktop, window | **Found a silent half-rule, again.** `align-items` defaults to `flex-start` rather than CSS's `stretch`, so a scroller with no `width: 100%` is as wide as its longest row: it works perfectly and looks broken. All three recipes now say the width out loud |
| `examples/recipes/tab-bar.rux`: tap along the bar, frame caught mid-navigation | desktop, window | Passed. Both pages on screen at once and **overlaid rather than queued**, the outgoing sliding left as the incoming slides in from the right, the bar outside the router unmoved, and `:current` following the route |
| `examples/recipes/modal.rux`: open it, tap the dialog, then tap the scrim | desktop, window | Passed, and **found what the swallow costs**. Tapping the dialog does not dismiss, which is the `@tap="0"` doing its job, but the dialog takes the focus ring and becomes a Tab stop that does nothing. There is no way to say "tappable but not focusable". Filed as an open author note. Tapping the scrim dismisses, and the page behind never moves |
| `position: fixed` on the scrim | desktop, window | **Not honored, and worse than a limit: silent.** `fixed` was mapped straight to `absolute`, so a cover scrolled away with its ancestor and nothing said a word. Chasing it found three more silent answers and a divergence, all fixed the same day; see below |

### `position`, all four values (2026-08-19)

Asked directly whether the limit could be lifted instead of embraced. It was not
a limit, it was four silent wrong answers and a divergence nobody had noticed.

| Case | Hardware | Outcome |
|---|---|---|
| What `position: fixed` actually did | reading the parser | `"absolute" \| "fixed" => Position::Absolute`. Silently absolute, so it scrolled away with its ancestor. `sticky` and `static` fell through to `relative`, so `static` even honored insets, and **so did every typo**: `position: absolut` was a box that quietly stayed where it was |
| Which box an `absolute` one is measured against | headless, `rux-layout` | **Found the divergence, and it was in a page shipped an hour earlier.** Screen (positioned, 600x400) > wrapper (unpositioned, 200x100) > cover with all insets `0` laid the cover out at 200x100. Rux measured against the **parent**, CSS measures against the nearest non-static ancestor. Invisible until then because the *default* was `relative`, so every box was a containing block and the two rules gave the same answer for a direct child. Every example that uses insets already wrote `position: relative` on the box it meant, out of CSS habit, and was being ignored |
| All four values after the fix | desktop, window, `examples/position.rux` | Passed. The cover skips a `static` wrapper and fills the `.frame` that claims it; an `absolute` badge inside a scroller rides the content out of sight; a `fixed` badge stays exactly put through eight wheel notches. `static` ignores its insets and `relative` honors them |
| `sticky` headings over a scrolling list | desktop, window, `examples/position.rux` | **Found a paint-order bug that every geometry test passed.** The heading pinned to the right pixel and the rows scrolled *straight through it*, because a sticky box was painted before its in-flow siblings and so sat under them. A positioned box paints over in-flow content; sticky children are visited last now, which also puts them on top for hit testing |
| Sticky headings written as flat siblings, no box per section | headless | **Found a second sticky bug, and the user's question found it.** Asked whether the hand-over was real or something the example arranged, and testing the answer showed the clamp used the scroller's *visible* box rather than its content box. A heading clamped to a 200px band that is itself sliding drifts instead of sticking: the first heading came back at -220 where it should have been pinned at 0. Only the flat case exposed it, because a heading inside a section is clamped to the section and never touched the wrong box |
| A sticky heading handing over to the next section | desktop, window | Passed. The first heading rides the scroller's top edge, then is pushed off by its own section ending rather than sitting over the next section's rows |
| A `transform` as a containing block | headless | Passed both ways: a transformed wrapper claims an `absolute` child it would otherwise pass over, and claims a `fixed` one that would otherwise reach the window. This is why `position: fixed` stops being fixed inside a transformed parent, in Rux as in a browser |
| The animation examples, which rely on inset-less absolutes | desktop, window | No regression. `:leave-to { position: absolute }` names no inset, so it keeps its static position and stays with its parent rather than travelling to a containing block. The tab-bar recipe's pages still overlay rather than queue |

### Route guards (2026-08-19)

| Case | Hardware | Outcome |
|---|---|---|
| Tap a guarded tab while it is locked, then unlock and tap again | desktop, window, `examples/recipes/tab-bar.rux` | Passed. Locked, the tab redirects to the sign-in page and `:current` stays off the tab that was refused, because the router really is somewhere else. Unlocked, the same tap goes through and the page crosses normally |
| Back and Forward through a shut guard | headless | Passed, and this is the half worth having a test for. A guard written on `navigate` alone protects nothing: Back reaches the same page without passing it, and Back is how anyone leaves a login screen. Leaving a guarded page is not the guard's business and is still allowed |
| A guard that is syntactically fine and blows up when it runs | headless | **Found a fail-open hole, from a question rather than a test.** Asked whether guards not being compiled at load was a problem or a choice; checking properly showed the compile gap was only half of it. `guard="user.is_admin"` with `user` null warned into the overlay **and let the navigation through**, so a broken auth guard admitted everyone and the app looked fine. A failing guard refuses now, the user's call, and it is the one place in Rux where a failing expression has no benign fallback |
| A refused navigation's warning | headless | **Found a gap while testing.** A refused navigation does no rebuild, and the rebuild is what drains the warning sinks, so a circle of redirects raised a warning that nothing would ever read and the screen simply did not change. The refusal path drains them itself now |

### The playground against the v0.7 candidate (2026-08-19)

Driving the site locally while building an app against the v0.7 candidate.

| Case | Hardware | Outcome |
|---|---|---|
| Paste `examples/recipes/message-list.rux` into `/playground/` | desktop, browser, local `zola serve` | **Found a mismatch that is live on the deployed site, not a local artifact.** `Unknown operator: '++'` at line 12. The playground is pinned to the latest *release* tag by `site.yml`; `/recipes/` and `/reference/` describe the **tip**. So a recipe copied from the page it is documented on fails in the tool sitting next to it |
| How the failure reads | desktop, browser | The error is large, red, and says the operator does not exist. The version, `v0.6.1`, and the words "error, showing last good" are small grey text in the opposite corner. Nothing connects the two, so it reads as a broken example rather than an old runtime |
| How much of the recipes section this covers | reading the three files | **All of it.** Every recipe has a `fn` body that writes a signal, which needs v0.7 lexical scoping; `message-list` also uses `++` and `query()`. Fixing the operator alone would not make one of them run. The recipes are not partly ahead of the playground, they are entirely ahead of it |
| Rebuild the bundle from the branch and reload | desktop, browser | Passed. `cargo build -p rux-web --target wasm32-unknown-unknown --profile wasm-release` then `wasm-bindgen --target web --no-typescript --out-dir site/static/wasm …` puts `0.7.0-dev` behind the badge and the recipe runs. `site/static/wasm/` is gitignored, so this is a local override and changes nothing about what deploys |

**Settled the same day, by the user: local builds track the tip, CI builds
track the tag.** `site.yml` had said the playground runs the last release "so it
demonstrates what /learn and /reference describe instead of unreleased work".
That reasoning holds for `/learn`, which tracks releases, and was false for the
other two: `/reference/` is generated from `docs/05-as-built.md`, which
describes the tip, and `/recipes/` tracks the tip by design.

`site/build-playground.sh` is now the one implementation of the build.
`--from-tag` is what CI passes, so the deployed site still shows what a visitor
can install; a developer running it bare gets the tip, so the recipes they are
writing run in the playground beside them. The two modes cannot drift on
anything except the flag, which is the point of there being one script.

Two things the script does that the inline CI step did not: it reads
wasm-bindgen's version out of **the lockfile of the tree being built** (under
`--from-tag` that is the tag's lockfile, not main's, and an older tag can
resolve an older wasm-bindgen), and it confirms the emitted binary actually
carries the version it was asked to build. A wasm-bindgen step that silently
reuses a stale `--out-dir` is otherwise indistinguishable from a successful one,
which is the exact confusion this whole arrangement exists to end.

### "The whole file" was not the whole file (2026-08-19)

Driving the message-list recipe in the playground on the v0.7 bundle. The user's
read was that the output "isn't what a desktop window would give us".

| Case | Hardware | Outcome |
|---|---|---|
| The recipe, as the page offers it, in the playground | desktop, browser | No bubbles, no thread panel, `.who` not blue, composer spread edge to edge. Nothing like the recipe it illustrates |
| **The same source in a desktop window** | desktop, window, `rux.exe` from the branch | **Identical, pixel for pixel.** So the playground is faithful and the renderer is fine; the source is what differs. This is the measurement that turned a suspected wasm-fidelity bug into a documentation bug in one step |
| The repo's `examples/recipes/message-list.rux` in a window | desktop, window | The recipe as intended: bubbles, panel, right-aligned `.mine`, blue `.who`, the reply button |
| Diffing the two sources | reading | The page's fence, headed **"The whole file"**, is **53 lines of a 179-line file**. No `.app` rule, no `.title` / `.lead`, no bubble backgrounds, no reply button. The prose three lines below said so, and nobody reads a caveat under a heading that says "whole" |
| Which other pages do this | reading all of `site/content` | Only this one. `modal` and `tab-bar` show snippets throughout and never claim otherwise, and the three `/learn/` fences are self-contained documents |

**Why it stopped being a nit.** It was a documentation imprecision for as long as
a fence was something you read. It became a defect the day every fence grew a
Copy button and a Try it link: both hand the fence straight to the playground,
so a reader copying "the whole file" gets an unstyled screen that looks exactly
like a broken renderer.

**Fixed**: `site/sync-examples.sh` fills any fence marked
`<!-- FROM: <path> -->` from that file, `--check` fails on drift, and
`gate_docs_synced` runs it. The examples are already under test, so a
hand-copied second version on the page was a copy of tested code that nothing
tested. Same drift this repo has now been bitten by three times: the extension's
void-tag list, the site's grammar copy, and this.

**Method note.** The window capture walked straight into the trap
[[driving-the-window-headlessly]] already records: a `$hwnd` named `$h` collides
with a `-H` height parameter, because PowerShell variable names are
case-insensitive. The window was sized to the handle value and clamped to 65535
tall, and the capture "succeeded".

### Writing a real app against the 0.4.0 extension (2026-08-19)

The user built a WhatsApp-shaped header in their own workspace, which is the
first time the extension has been driven by someone writing Rux rather than by
its own tests. Four findings, none of which any test had an opinion about.

| Case | Outcome |
|---|---|
| Typing a `justify-content` value | The values **were** offered, buried among `script`, `signal`, `slot`, `sticky`, `style`. All 31 snippets were contributed statically through `package.json`, which has no concept of a section, so every one was offered in every section. A working value list read as a broken one |
| A script error's squiggle | Drawn at **line 1 of the file** while the message said "(line 1, position 19)" and the error was on line 26. `rux check --format json` reported `"line": null`: the position existed only as prose inside the sentence, and it was **script-relative** |
| `signal()` with no argument | "Function not found: signal ()" for a function that plainly exists. `rux_phrasing` has translated this into Rux's vocabulary since it was written, and was applied to every *expression* failure and never to the *load* one |
| Completing a name the file declared | Nothing. The vocabulary knew every name the runtime provides and none the author had just written, so the list went quiet exactly where it should have been most useful |

**All four fixed.**

- Snippets are served from the completion provider, filtered by section, and a
  test asserts every one is placed. The `contributes.snippets` entry is gone.
- `ScriptError` carries rhai's position; `extract_imports` now leaves a blank
  line where it strips a `use` so the numbers still line up (it dropped the line
  before, shifting everything below by one); `Document::load_checked` adds
  `Sfc::script_line`, which had existed and never been read by anything. The
  mapping **refuses** to place a position past the end of the document's own
  script, because past that point the compiled text is appended component
  functions and a confident wrong number is worse than none.
- `explain` is applied to load errors, so it reads "there is no function
  `signal` taking 0 arguments" at 12:19.
- The editor scans the document for `let` / `computed` / `fn`, ranks those above
  everything the runtime provides, and offers them in `<script>`, in `{{ … }}`,
  in `:bound` attributes and in handlers. An `r-for` row variable comes with
  them. After a `.`, an element handle from `query()` offers its own members and
  everything else offers the string and array methods.

**An open design question for the user**, not a defect: `signal()` with no
argument is an error. `signal("")` is the fix and the message now says so, but
whether an empty `signal()` should mean an empty value is a call worth making
rather than leaving to whoever hits it next.

**Seen once and not reproduced**: one `cargo test` run reported 1 failure in a
12-test suite, and seven subsequent runs of the same combination were clean.
Not diagnosed. The warning sinks are process-wide and `check_file` already
clears them for exactly that reason, so a parallel-run race is the suspicion and
not more than that.

### The extension never activated (2026-08-20)

Found while the user was driving the 0.4.x extension in their own app. Six
rounds of "the fix does not work" against code that was demonstrably correct
when called directly.

**`activationEvents` was absent from `package.json`, and had been since 0.3.0,
the version published to the Marketplace.** With no activation event VS Code
never calls `activate()`, so completions, hover, diagnostics, formatting and
every command lived behind a function nothing invoked.

**Why it stayed hidden for three versions:** the *declarative* contributions
need no activation. The TextMate grammar coloured every file, and
`contributes.snippets` answered completions from a static JSON list. Together
those look exactly like a working, if thin, extension. The user's first report
this session, "no CSS completions, just `script`/`signal`/`slot`/`sticky`", was
that static snippet list being the **only** thing answering. Removing the static
snippets in favour of a section-aware provider took even that away, which is why
the symptom got worse right after a fix that was correct.

**The lesson is about the evidence, not the bug.** Every check that was run
(unit tests, driving the provider through a stand-in for the VS Code API,
inspecting the installed files on disk) tested code that was never reached. The
one check that would have caught it was asking whether `activate()` ran at all.
A provider verified in isolation says nothing about whether the editor calls it.

Also fixed in the same pass, all found by the user:

| Finding | Fix |
|---|---|
| A dot on a value of unknown type offered `charAt`, `map`, `join`. `let handle = setInterval(2000) { … }` is a timer handle | The receiver's kind is inferred from its declaration, and **unknown offers nothing**. Guessing endorses calls that cannot work, the same failure as offering an unhonored CSS property |
| An older `rux` on PATH silently stripped capabilities the extension shipped with | `current()` was `live \|\| baked`, all or nothing. It merges per field now, so a binary built before a feature cannot remove it |
| `computed` was not colourable as a keyword | It sat in the same grammar rule as `signal(` and `query(`, which ends in a `(?=\s*\()` lookahead. A declaration has no parenthesis. `effect { … }` had it too |
| A template literal's text and its `${…}` looked identical | Backtick strings were not strings at all in the grammar. Now a string, with the holes highlighted as code |
| CSS property help said only "honored by the runtime" | All 97 describe what they do, with a worked example, gated so none can be added undocumented |
| No selector completion in `<style>` | `.` and `#` offer what the template actually uses; a bare word offers element tags |
| `use` and selectors gave no hover | Both answer now, and a selector says when **nothing in the template has it**, which is the silent dead-rule case |

**Method note, recorded because it cost the most.** Editing JavaScript through a
shell heredoc turned `` in a regex into a literal backspace (U+0008) three
separate times. The file parsed, the regex compiled, and it matched nothing.
`editors/vscode/test/sources.test.js` fails on any stray control character now.

### Three extension symptoms still open at 0.4.8 (2026-08-20)

Left open deliberately rather than closed on an assumption. The user reports,
after a confirmed 0.4.8 install: component tags still coloured as elements,
`length` still showing the `query()` text, and no hover on a declared variable.

**None of the three reproduces here.** Driven through the registered providers
against the user's own file, `search_item` hovers as a signal holding an array,
`length` reads from the value methods, and the grammar scopes `<side-panel>` as
a component and `<view>` as an element.

**The clue worth starting from:** the component colour is pure TextMate grammar.
It needs no activation, no binary, and no provider. A grammar change failing to
land while a behavioural one apparently lands says the editor is not loading the
artifact that was installed, and points away from the features entirely.

Ruled out along the way, each having been a genuine cause earlier:
`activationEvents` absent since 0.3.0; a same-version reinstall not replacing a
running extension; an uninstall leaving its directory behind; and a stale `rux`
on PATH overriding the extension's vocabulary per field.

**What this cost, and why:** more than six rounds of "the fix does not work"
against fixes that were real. Every verification ran against the working tree or
an isolated provider call, and neither is what the editor loads. No evidence was
ever collected from the user's side until a diagnostic command was written, far
too late. `scripts/dev-install.sh` now rebuilds the binary, regenerates the
vocabulary, bumps the version, wipes the old install and reinstalls in one step,
and `--check` reports whether the four moving parts agree.

### The three extension symptoms, all closed (2026-08-20)

The first of the three above is closed, and the answer rules out the theory the
entry above it was built on.

The grammar had been scoping component tags as `entity.name.tag.component.rux`
since 0.4.8, and that is a correct, distinct TextMate scope. It was reaching the
editor the whole time. **A theme resolves a scope through its prefixes**, and no
stock theme has ever heard of `.component`, so every one of them matched the
`entity.name.tag` rule underneath and painted `<header/>` exactly the colour of
`<screen>`. In Abyss, the theme in use, both came out `#225588`.

So the artifact was loading. The distinction existed in the grammar file, in the
crate's scope table, and on the website. It existed nowhere on screen, in the
one place it had been asked for.

Components are now scoped `support.class.component.rux`, which is what Vue and
Svelte use and what themes colour on its own.

**Why every test passed through this.** `rux-highlight` resolves a scope by
longest matching prefix in its own table, where `entity.name.tag.component` duly
beat `entity.name.tag`. Rux's renderer and a VS Code theme resolve the same
scope by opposite rules, so the crate's tests could not have caught it and the
site was genuinely right the whole time. There is now a test that reads the
grammar directly and refuses a component scope that begins with the element
scope, whatever either happens to be.

**The other two, and they were one bug.** No hover on a declared variable and no
completion of one had the same cause, found by sweeping hover across every
offset of the user's real files rather than sampling a few: `declarations()`
returned nothing at all on either file.

The three declaration patterns in `locals.js` are anchored with `$`. In
JavaScript, `$` without the `m` flag matches at the end of the string or before
a final newline, and never before a carriage return; `.` does not match one
either, so the lazy group in the `let` pattern could not step over it to reach
the anchor. The section body was split on the newline alone, so every line of a
CRLF file arrived with a carriage return still on it and **not one declaration
in the file matched**. Completion offered only the globals, hover answered
nothing on a name the author had just typed, and no error was raised anywhere.
`.rux` files on Windows are CRLF, which is to say this was the normal case and
not an edge of it.

The outline kept working throughout, and that was the tell nobody read: it scans
with `/m` and no `$`, so it was the one feature the carriage return could not
reach.

**Why the whole suite passed.** Every fixture in it was written in a JS string
literal with `
`. A fixture set that only spells documents one way cannot test
the way the users' documents are actually spelled. `test/line-endings.test.js`
now runs declarations, inference, completion, hover and loop scoping over the
same document in both endings, and asserts the two agree; six of its cases fail
without the fix.

**And the verification was worse than the bug.** This session reported the two
as not reproducing, on the strength of a harness run that normalised the endings
away: a string replace meant to disable that normalisation silently did not
match, the output looked the same as the run before it, and it was read as
proof. An unasserted edit to a test harness is not evidence. The instruction is
now: reproduce from the user's file bytes, and assert that the bytes under test
are the bytes on disk.

**A second lying instrument, found on the way.** `dev-install.sh --check` reported
0.4.8 installed while VS Code was running 0.4.9. It listed the extensions
directory and took the first entry, and an install leaves the previous version's
directory behind. It now reads VS Code's own `extensions.json`, and names the
leftovers rather than being fooled by them.

## v0.7.1

### The unhonored-property message, split three ways (2026-08-25)

Driven with `rux check` on one file carrying all three cases at once, which is
the only way to see that they now read as three different problems.

| Case | Hardware | Outcome |
|---|---|---|
| `outline: 1px solid red`, real CSS Rux has not built | desktop, `rux check` | Passed: "real CSS that Rux does not honor yet", and it promises nothing more |
| `paddding: 8px`, a typo | desktop, `rux check` | Passed: "is not a CSS property Rux knows", and it offered `padding` |
| `colour: #fff`, the other spelling | desktop, `rux check` | Passed: offered `color`. Worth having, since it is the mistake an author makes once a week |
| `florble: 3`, invented | desktop, `rux check` | Passed: reported unknown and suggested nothing, rather than reaching for the nearest unrelated name |
| All five in one file | desktop, `rux check` | Passed: five warnings, each on its own line number |

**Found while writing the reference for it:** the first chapter of `/learn` told
the reader to add `line-height: 2` and watch the terminal warn. `line-height`
has been honored since v0.5, so the demonstration printed nothing at all and had
been wrong through two releases. Nobody drove the tutorial's own instruction.

### Broken sections, argument counts, and percentages (2026-09-15)

| Case | Hardware | Outcome |
|---|---|---|
| `<template>` opened, never closed | desktop, `rux check` | Passed: "opened here and never closed", at 1:1. Said "missing <template> section" before, with the author looking straight at the tag |
| A file with only a `<style>` | desktop, `rux check` | Passed: genuinely missing, and says what a template is for |
| `<template` with no `>` anywhere | desktop, `rux check` | Passed: "never finished: no `>` after it" |
| `<style>` opened, never closed | desktop, `rux check` | Passed: an error at line 4. Silently meant "this file has no styles" before, which looks exactly like CSS that does not work |
| `two(1)` and `two(1,2,3)` where `fn two(a, b)` | desktop, `rux check` | Passed: names the count given and the count wanted |
| `none(5)` where `fn none()` | desktop, `rux check` | Passed |
| `two(1, 2)` and `none()` | desktop, `rux check` | Passed by staying silent |
| `bump_it(2)` in a component, `fn bump_it` in its parent | desktop, `rux check` | **Whole app: silent** (correct, `fn`s are shared). **Component alone: a warning, not an error** (correct, its parent may define it) |
| `padding: 10%`, `margin: 10%`, `gap: 5%`, `border-radius: 50%` | desktop, `rux check` | Passed: each says it is ignored and why. **All four were silently dropped before** |
| `width: 50%` | desktop, `rux check` | Passed by staying silent: layout resolves it, so it works |
| All 47 files in `examples/` | desktop, `rux check` | Clean |

**The percentage report was four properties, not one.** `border-radius: 50%` was
reported; `padding`, `margin` and `gap` had the same defect and nobody had tried
them. The cause is two functions in one file disagreeing about what a length is:
the interpreter reads these with `parse_px`, which has no percentage, while
`warn_unparseable_lengths` validated with `parse_len`, which does. So the value
was thrown away and then pronounced fine **by the warning added to stop values
being thrown away in silence**.

**Still not driven in the window**: whether a radius clamped by a hug-sized
button's own text is the whole of the other half of that report. It is correct
behaviour by the CSS rule, and the author note stays open until someone looks.

### Severity, unknown events, and which file (2026-09-15)

| Case | Hardware | Outcome |
|---|---|---|
| `{{ nope }}` in a page | desktop, `rux check` | Passed: **error**, exit 1. Was a warning, exit 0 |
| `{{ label }}` in a component checked on its own | desktop, `rux check` | Passed by staying a **warning**: props are not declared, so it cannot be told from a typo |
| `@class="big"` on a `<view>` | desktop, `rux check` | Passed: error, listing the six events that exist |
| `@sent="x"` on a component tag | desktop, `rux check` | Passed by staying silent: that is a listener for an emitted event |
| `r-else=""` | desktop, `rux check` | Passed: error. Needed `Attr::has_value`, since the value is empty either way |
| `r-else` on its own | desktop, `rux check` | Passed by staying silent |
| A warning raised inside an imported component | desktop, `rux check` | Passed: names `components/badge.rux:3`, not `app.rux:3`. It reported the **importer's** name with the **component's** line until this landed |
| All 47 files in `examples/` | desktop, `rux check` | **Found a real bug**, see below, then clean |

**Two live bugs found by the checks on their first run over the corpus, which is
the argument for both of them:**

1. **`examples/recipes/message-list.rux` carried `@submit="send()"` on its
   `<input>`.** An `<input>` raises no events of its own, so pressing Enter had
   never once sent a message. The send button beside it works, which is what
   kept it hidden through every release the recipe has shipped in.
2. **`rux new` scaffolded a project that failed its own `rux check`.** The
   scaffolded `components/task.rux` reads two props and declares nothing, so
   making undefined names an error everywhere turned the tool's own output into
   two errors. That is what produced the page/fragment rule, and it is the
   strongest argument on record for giving props a declaration.

**Also worth recording as a near miss.** Adding line numbers to template
warnings the day before had made the multi-file case *more* dangerous, not less:
warnings from a component carried its line number and the importing document's
name, so they pointed confidently at an innocent line of the wrong file. It was
found by asking what happens with an imported template, not by any test.

### Where a template warning points (2026-09-15)

Driven with `rux check --format json`, which is what the editor reads, against
a file whose line numbers were chosen to be awkward.

| Case | Hardware | Outcome |
|---|---|---|
| `{{ nope }}` on its own line | desktop, `rux check` | Passed: the text run's line. Was unplaced, so it landed on line 1 |
| `@tap` three lines below its `<button` | desktop, `rux check` | Passed: the attribute's line, not the tag's |
| `:class` two lines below its `<view` | desktop, `rux check` | Passed: the attribute's line |
| `r-if` on a child | desktop, `rux check` | Passed: the directive's line. Read by the *parent's* loop, so it reported the parent's line until it was placed |
| `r-for` on a child | desktop, `rux check` | Passed, same shape |
| `{{ }}` inside an `r-for` row | desktop, `rux check` | Passed: the row's text line, once, not once per item |
| A document opening with `<script>` and `<style>`, template starting on line 9 | desktop, `rux check` | Passed: reported line 11, the file's line, not the template section's line 3 |
| All 47 files in `examples/` | desktop, `rux check` | Passed, still no warnings at all |

**The old behaviour is worth naming precisely:** every template warning arrived
with no line, so every consumer fell back to the top of the file. That put the
squiggle on the `<` of `<template>`, which is the one place in a document where
nothing is ever wrong.

### Calls that can never resolve (2026-09-15)

Driven with `rux check` on files carrying each case, and the silent half matters
more than the loud half: the whole value of this check is that it is worth
reading.

| Case | Hardware | Outcome |
|---|---|---|
| `@tap="alert(1)"` | desktop, `rux check` | Passed: names the call and says it does not exist. Silent before |
| `@tap="searching.frobnicate()"` | desktop, `rux check` | Passed |
| `@tap="searching.set(true)"` | desktop, `rux check` | Passed: named as the pre-v0.3 signal API, with the assignment that replaced it. **Not** reported as a missing function, because `set` is registered for arrays and maps and saying otherwise would be false |
| `@tap="n = searching.get()"` | desktop, `rux check` | Passed, same shape |
| `@tap="count.update(...)"` | desktop, `rux check` | Passed, caught by name: `update` is registered nowhere |
| `@tap="tasks.set(0, &quot;x&quot;)"` on an array signal | desktop, `rux check` | **Passed by staying silent.** This is the registered two-argument `set` and it works |
| `@tap="tasks.push(&quot;x&quot;)"`, `count += tasks.len()`, `bump()` | desktop, `rux check` | Passed by staying silent |
| `fn helper() { alert("x") }`, never called | desktop, `rux check` | Passed: a `fn` nobody calls is the quietest place a typo can sit |
| `fn bump() { helper() }` with `helper` declared **below** it | desktop, `rux check` | Passed by staying silent: that resolves at run time, so warning would be wrong |
| **All 47 files in `examples/`** | desktop, `rux check` | **Passed with no new warnings**, which is the false-positive measurement this check lives or dies by |
| 300 handlers in one document | desktop, debug build | 0.37s end to end, so no caching was added for a cost that is not there |

**The `.set()` case is not a typo an author invented.** `docs/02-spec.md` taught
`count.get()` / `.set()` / `.update()`, and that file was billed as the reference
until 2026-08-25. Someone who read the docs and wrote it got a handler that did
nothing and no reason why.

### The `r-for` tuple form (2026-08-25)

| Case | Hardware | Outcome |
|---|---|---|
| `r-for="(pot, index) in pots"` | desktop, `rux check` | Passed: says the form is unsupported and names `pot` and `index` as the two things that do not exist because of it. Before this it only said `index` was undefined and advised declaring it as a signal |
| The same file's follow-on warnings | desktop, `rux check` | Passed: the two undefined-name warnings still appear and now read as consequences of the first, rather than as the whole story |
| `r-for="pot in pots"`, the ordinary form | desktop, `rux check` | Passed: silent, which is the half that would turn the fix into noise if it were wrong |

**The spec was where the tuple form came from.** `docs/02-spec.md` said `r-for`
"supports an index form" and used `:key` in its example, neither of which was
ever built. An author who reads the reference and writes what it says is not
making a mistake. Both lines are corrected.

### The document's own interval, beside a component (2026-09-15)

Found while walking the v0.7.0 release post claim by claim, not by any test.

| Case | Hardware | Outcome |
|---|---|---|
| Document `mounted` starts `setInterval`, a component with a `mounted` hook is on the first build, the component is then dropped | desktop, `rux run` | **Failed before the fix.** The document's interval stopped, the window went to sleep at zero CPU, and the screen froze with the last frame on it. No error, no warning, no panic |
| The same, through a `<router>`: the landing route's view declares `mounted`, then navigate away | desktop, `rux run` | **Failed before the fix**, and this is the shape an author meets first. The router rendered nothing and the app stopped responding to its own clock |
| The same component declaring only `unmounted` | desktop, `rux run` | Passed: the trigger is a `mounted` hook, because that is what the mounts pass drains the queue for |
| The component mounting *later*, after the interval already existed | desktop, `rux run` | Passed, and this is what made the bug look intermittent |
| A component's own interval, started in its `mounted` | desktop, `rux run` | Passed both before and after: it still dies with the instance, which is the half a fix here could easily have broken |

**The cause was attribution, not pruning.** Timer requests queue in one place for
the whole document. `settle_lifecycle` attributes whatever is pending to the
instance that just mounted, and on the first build it ran *before* the document's
own request was drained, so the document's clock was handed to the first
component on screen and pruned with it. The drain moved to directly after
`mounted` runs.

**The frozen app reads as a crash and is not one.** The process sat at zero CPU,
`Responding=True`, with the window still painting its last frame. Nothing was
spinning and nothing had panicked: the runtime had simply stopped asking the
event loop to wake it. Anything that looks like a hang here is worth measuring
before it is debugged as one.

### How a `<route>` names its view (2026-09-15)

Found while walking the v0.7.0 release post claim by claim: the probe app for
the guard claims would not load, and the reason it gave was one the file had
already done.

| Case | Hardware | Outcome |
|---|---|---|
| `view="page_a"` with `use components::page_a;` present | desktop, `rux check` | **Failed before the fix.** Said "which is not imported; add `use components::page_a;` to the script", which is the line sitting two lines below it. Doing what the message says gets the same message back |
| `view="page-a"` with nothing imported | desktop, `rux check` | **Failed before the fix.** Suggested `use components::page-a;`. Pasting it turns the warning into a hard error looking for `page-a.rux`, a filename no convention here produces |
| `view="page-a"` with `use components::page_a;` | desktop, `rux check` | Passed, silent. The control, and the spelling that works |

**Two spellings for one component is the trap, and it is real rather than
avoidable.** A view is named the way its tag is, in kebab, and the `use` that
brings it in names the file, in snake. Both messages now say which spelling the
place they are complaining about wants, and the import they suggest is one that
parses.

### `query()` against the root (2026-09-15)

From the claim walk: the post says a handler can read the screen, and the
reference says `query()` "is the stylesheet's own matcher". It was not, at
exactly one node.

| Case | Hardware | Outcome |
|---|---|---|
| `query(".app .row")` with `.app` on the `<screen>` root | desktop, `rux run` | **Failed before the fix.** Empty, while the identical selector in the stylesheet painted the row red. Two answers about one document |
| `query(".app > .wrap")`, `query("screen > .wrap")` | desktop, `rux run` | **Failed before the fix**, same cause |
| `query(".wrap > .row")`, `query(".row > .t")`, `view > text` | desktop, `rux run` | Passed all along: `>` works, which is what made the bug look like something subtler than it was |
| `query(".row + .row")`, `query(".row ~ .row")` | desktop, `rux run` | Passed all along |
| `query(".app")`, `query("screen")` | desktop, `rux run` | Passed all along: the root was always **findable**, just never an **ancestor** |

**The root's path is empty, and the ancestor chain was rebuilt from depth 1.**
So the one node an author is most likely to anchor a selector at was the one
node that could never appear above anything. `query(".app")` returning 1 while
`query(".app .row")` returned 0 is the tell, and it is why this reads as a
selector-support gap rather than an off-by-one.

### Reaching a component in a parent directory (2026-09-15)

Reported by the user as the thing that catches them out most often, with a
screenshot: `pages/home.rux` in a project whose components live at the root.

| Case | Hardware | Outcome |
|---|---|---|
| `use components::task;` in `pages/home.rux`, component at `components/task.rux` | desktop, `rux check` + `rux run` | **Failed before the fix**, and the only way out was a second copy of the component beside the page. Now resolves from the project root, and the page renders both rows in the window |
| The same import with a component **also** at `pages/components/task.rux` | desktop, `rux check` | Passed: the near copy wins. This is the half that had to keep working, or a document that resolves today would quietly start meaning a different file |
| The same layout with no `app.rux` or `index.rux` anywhere above | desktop, `rux check` | Passed by still failing: nothing marks the top of a project, so there is no root and only the relative form applies |
| `use components::nope;`, nowhere at all | desktop, `rux check --format json` | Passed: `"line": 6`, and the message names both directories it looked in. It used to be a bare OS error with `"line": null` |
| `use components::;`, half typed after a completion | desktop, `rux check` | Passed: "this `use` names no component: a path segment is empty". It used to report `reading component .../components/.rux: The system cannot find the path specified`, which describes a path the author never wrote and blames the disk |

**The squiggle was the visible half.** Every one of these errors came back with
no position, so VS Code drew it on line 1 and pointed at `<template>` for a
mistake on the last line of `<script>`. `LoadError::at_line` places them now,
and the import carries the line it was written on.

### A template with more than one root (2026-09-15)

Reported as "`rux run` too doesn't give me the expected UI", against a project
whose `pages/home.rux` was written as four siblings.

| Case | Hardware | Outcome |
|---|---|---|
| A component with three root elements | desktop, `rux run` | **Failed before the fix, in silence.** Only the first root drew. No warning, no overlay, nothing on stderr, and `rux check` clean |
| The reported project: four roots, the first an `r-if` over an empty list | desktop, `rux run` | **Failed worse.** The first root was the one that rendered, and its condition was false, so the page rendered **nothing at all** and the window showed only the title from `app.rux` |
| The same, after the fix | desktop, `rux check` | Passed: names the file, the line of the second root, how many there are, the tag that starts the dropped run, and to wrap them in a `<view>` |
| The error raised through an import | desktop, `rux check app.rux` | Passed: reported against `pages/home.rux`, the file that is actually wrong, not the importer |
| Every `.rux` in the repo | desktop | Passed: **zero** multi-root templates, so making this an error breaks nothing that exists |

**The rule was real and written down nowhere a reader would look.**
`docs/02-spec.md` says a template "must contain exactly one root element", and
that file was reframed in this same patch line as design history, checked
against nothing. The reference said nothing at all. So the runtime enforced a
rule by dropping markup, and the only document stating it was the one that
announces it does not describe the runtime.

### `r-if` riding on `r-for` (2026-09-15)

Found while writing a routing example to answer a question, which is its own
small lesson: the example was wrong and the runtime said nothing.

| Case | Hardware | Outcome |
|---|---|---|
| `<text r-for="n in nums" r-if="n > 2">` over `[1, 2, 3, 4]` | desktop, `rux run` | **Failed before the fix, in silence.** All four rows rendered. The loop expands the element and the condition is never read, so the filter does nothing and nothing says so |
| The same after the fix | desktop, `rux check` | Passed: names the directive, says the condition is never read, and gives both ways out (filter with a `computed`, or move it to a child) |
| All 47 files under `examples/` | desktop, `rux check` | Passed with no new warnings, which is the false-positive measurement this check lives or dies by |

**Reported rather than honored, deliberately.** Which of the two should win is a
real design question, and Vue has answered it both ways across two major
versions. Quietly picking an answer here would change what existing documents
render, which is not a patch's business.

### Four things a first router app ran into (2026-09-15)

Reported from a tasker app written in the editor: a screenshot of a squiggle
under `<input>`, one of `Ctrl+/` writing `//` into a template, and the question
of why `view="new_task"` was accepted when nothing imported it.

| Case | Hardware | Outcome |
|---|---|---|
| `<view><input type="text"></view>` | desktop, VS Code + `rux check` | **Failed before the fix.** Parse error, and it named the wrong tag: "mismatched closing tag: expected `</input>`, found `</view>`", against a file whose only mistake was being written the way HTML is |
| The same after the fix | desktop, `rux check` | Passed: the void tags close themselves, and `<input></input>` now says "`<input>` holds nothing, so it has no closing tag" rather than blaming the enclosing element |
| `Ctrl+/` in `<template>` | desktop, VS Code | **Failed before the fix, in silence.** Wrote `// <view>`, which is not a comment in markup. In `<style>` it wrote `//`, which is not a comment in CSS either and is dropped by the parser without a word, so a "commented out" rule stayed in force |
| `Ctrl+/` in each section after the fix | desktop, VS Code | Passed: `<!-- -->` in the template and between sections, `/* */` in the style, `//` in the script |
| `<route path="/new" view="new_task" />` with no `use` for it | desktop, `rux check` | **Failed before the fix.** Exited 0. The route was not the one being rendered, and only the matched route's view was ever resolved, so the broken page waited for the first navigation to it |
| The same after the fix | desktop, `rux check` | Passed: an **error** on the `view=`'s own line, said once rather than twice for the route you happen to be standing on |
| A `to="/typo"` beside a `to="/new"` and a `to="/task/7"` | desktop, `rux check` | Passed: only the typo reported, against `pages/home.rux` and its own line, with the `:id` route matching the concrete path |
| `view="new_task"` beside its own `use pages::new_task;` | desktop, VS Code | **Failed before the fix, by design.** Accepted the name only in kebab, and told the author to write the other spelling of a name they had already given correctly. Reported as "why am I forced to write new_task as new-task in view=?" |
| The same after the fix, both spellings, plus `<new_task />` and `<new-task />` | desktop, `rux check` | Passed: either spelling resolves in the template, both file under the one canonical name, and `netask` is still an error |
| The completion list, against a `new-task.rux` | desktop, VS Code | **Failed before the fix.** Offered the stem verbatim, so it wrote `use new-task;`. That resolves only because `use` lines are lifted out before rhai sees them; one line further down the same text is `new` minus `task`. Reported as "would rust agree with having `-` in any naming" |
| The same after the fix | desktop, VS Code + `rux check` | Passed: the list inserts `new_task`, `use new_task;` finds `new-task.rux`, and a hyphenated path still resolves but says it reads as subtraction |
| All 47 files under `examples/` | desktop, `rux check` | Passed with no new findings. The dead-link check found two at first, both in files that link to `/nowhere` **on purpose** to demonstrate `<route fallback>`; running it through the router's own matcher rather than a flattened list of patterns is what fixed that |

**Three of the four were silent, and the fourth pointed at the wrong line.**
That is the shape worth noticing: none of them was a missing feature. The
information existed in every case — the void list was already in the formatter,
the section boundaries were already in the editor's scanner, the imports were
already in the script — and nothing was asking it at the moment it mattered.

### A component could not use a component (2026-09-15)

Found while testing the fix above, on a copy of the user's own project. Not
reported by them: their task list was empty, so the symptom read as "no tasks
yet" rather than as a bug.

| Case | Hardware | Outcome |
|---|---|---|
| `components/task.rux` deliberately broken, `app.rux` importing only the pages | desktop, `rux check` | **Failed before the fix, in total silence.** Exit 0. The file was never parsed: `home.rux` imported it and a component's `use` lines were thrown away, so `<task>` matched nothing and expanded to nothing |
| The same, with `use components::task;` added to `app.rux` | desktop, `rux check` | The breakage appears at once, which is what isolated the cause: the component is only ever loaded through the **document's** imports |
| `<definitely-not-an-element />` | desktop, `rux check` | **Failed before the fix.** Reported nowhere. This is the silence that hid the one above: a tag was the only name in a template with no way to fail |
| Both, after the fix | desktop, `rux check` | Passed: a component's imports are followed, and a tag naming nothing is an error that lists Rux's own elements |
| Two pages, each importing a different `task.rux`, each writing `<task>` | desktop, `rux check` + render | Passed: each renders its own. Under the old flat map the second import replaced the first and one page rendered the other's component |
| A tag the *document* imports, written inside a component that does not | desktop, `rux check` | Passed: reported. A namespace that leaks is not a namespace |
| Two files importing each other | desktop, `rux check` | Passed: loads and renders. A worklist, so a cycle is a map lookup rather than a stack overflow |
| A nested route (`/crew/:id` inside `/crew`) | desktop, `cargo test` | **Caught by the existing suite, not by hand.** Making tags per-file broke it: a route's `view` is named in the file that wrote the `<router>`, and the `<router-view />` placing it is several components deep. Four router tests failed in one run and named the cause |
| All 47 files under `examples/`, plus all 21 components checked individually | desktop, `rux check` | Passed with no new findings, which is the false-positive measurement the unknown-tag error lives or dies by |

**The silence was load-bearing.** Three separate defects sat on top of one
missing diagnostic. Nothing reported an unknown tag, so a tag that resolved to
nothing looked exactly like a tag that resolved to an empty component, which
looked exactly like an empty list. The fix that matters most here is the
smallest one.

### An input that could not be typed into, and a border that was not drawn (2026-09-15)

Reported together, from one file: *"I have an issue with my input elements. They
are uninteractive. Even the css isn't being applied."* Two unrelated defects,
both silent, and each one made the other harder to see.

| Case | Hardware | Outcome |
|---|---|---|
| `<input placeholder="Task title" />`, no `r-model` | desktop, `rux run` | **Failed before the fix, in silence.** The box paints and the placeholder renders, and a tap reaches nothing: the layout makes a focus region only for an input carrying a model, so there is no caret, no keystroke and no value |
| The same, after the fix | desktop, `rux check` | Passed: an error naming the line and what to write |
| `input { border-bottom: 0.1rem #00f solid; }` | desktop, `rux run` | **Failed before the fix.** Nothing drawn. The cascade computed `bottom: 1.6`, and paint read `border.top` |
| `border: 6px solid` (uniform), as the control | desktop, `rux run` | Drawn correctly, which is what isolated the cause: only the uniform case ever worked |
| `border-top: 6px solid` | desktop, `rux run` | **Failed the other way**: drawn on all four sides |
| All three after the fix | desktop, `rux run` + screenshot | Passed: bottom-only, four-sided, top-only, each as written |
| All 47 files under `examples/` | desktop, `rux check` | Passed with no new findings. Every example binds its inputs, and none of them sets an uneven border |

**The cascade was never the problem, which is why it looked like one.** A probe
printing the computed style showed `border-bottom: 1.6` sitting on the node
exactly as written, so "the CSS is not applied" was false and the real fault was
one layer further down, where three of the four sides were dropped on the way
into `PaintRect`. Reading the computed style is not the same as looking at the
window, and this is the case that says so.

### An input inside a component took no text

Reported as "my inputs do not receive values", against `pages/new-task.rux` in
the user's own tasker app: the first keystroke seemed to register as a space and
nothing after it arrived at all. A routed `view=` is a component instance, and
that turned out to be the whole of it.

| Case | Where | Result |
|---|---|---|
| `<input r-model>` on a page (`<screen>` root), as the control | desktop, `rux run` + SendKeys | Passed, before and after. The signal updated on every keystroke |
| `<input r-model>` inside a component | desktop, `rux run` + SendKeys | **Failed before the fix.** The caret painted in the field, three keys went in, nothing was entered. One error on stderr, printed once rather than per keystroke |
| The same, after the fix | desktop, `rux run` + SendKeys + screenshot | Passed: `Hello` typed, shown in the field, and the component's own signal reads it back |
| The user's `pages/new-task.rux`, through the router | desktop, `rux run --route /new` + SendKeys | Passed after the fix: `Buy milk` typed into the field that had taken nothing |
| Two instances of one component | `cargo test` | **Failed before the fix**, and differently: writing one changed what the other read, because the captured build scope was matched on `(model, row)` only |
| `<input r-model>` bound to a **prop** | `cargo test` | Passed: the edit is dropped rather than half-kept, the same rule handlers follow |
| All 47 files under `examples/` | desktop, `rux check` | Passed with no new findings |

**Two things were being driven for the first time here.** No `r-model` test in
the runtime had ever used anything but a `<screen>` root, so the component case
had no coverage at all; and typing had never been driven headlessly. Injected
clicks still do not reach the window, but **`SendKeys` does**, which with a
`mounted` hook calling `focus()` is a complete typing harness.

**The one-line cause sat under the plumbing.** `rux-style` builds an `<input>`
in a branch of its own, and that branch never set `node.instance`, though the
general element branch always had. Every input reported "no instance", so
nothing downstream could have known whose state its model named.

### A name inside a `fn` body that nothing declares

Found in the user's own `pages/new-task.rux` while chasing the input bug: `fn
addUser() { Have }` passed `rux check` clean, as a page as well as a skipped
component. The undefined-name check runs when an expression is *evaluated*, and
a `fn` nobody has called yet is never evaluated.

| Case | Where | Result |
|---|---|---|
| `fn add() { Have }` | desktop, `rux check` | **Failed before the fix**: "no problems found". Now a warning naming `Have` |
| The user's own `pages/new-task.rux` | desktop, `rux check <file>` | Passed after the fix: the same warning, against the file the `fn` is actually in |
| A `fn` reading a **local of the function that called it** | desktop, `rux run` + `print` | Legal, and driven to be sure: `outer` declares `helper_local = 42`, `inner` reads it, the signal ends at 42. Must stay silent, and does |
| A `fn` reading a **handler's** local | `cargo test` | Silent: a handler is a caller like any other |
| A `fn` reading an `r-for` **row variable** | `cargo test` | Silent: the row is in scope for anything the row's handler calls |
| A component's `fn`, while checking the document | desktop, `rux check` | Silent, deliberately: the component's functions ride in the same compiled text, and the warning would carry the document's name against another file's line |
| All 47 files under `examples/`, and each of the 21 components alone | desktop, `rux check` | Passed with no new findings, which is the false-positive measure |

**The obvious check would have been wrong**, and that is the finding worth
keeping. Divergence 4 in the rhai fork makes a call run in the scope it was
written in, so a `fn` sees its caller's locals. Checking a body against its own
parameters and `let`s would report the single most useful thing the fork exists
to allow. The question the check actually asks is the weaker, answerable one:
**is this name declared anywhere at all?** A name that is no signal, no
parameter, no `let` and no loop variable anywhere in the document cannot be in
scope under any caller, because scope is made of declarations and there is no
declaration of it to be in.

It is an **error**, decided by the user while looking at the squiggle: a name
declared nowhere cannot resolve under any caller, and calling it a caution let
`rux check` exit 0 on a document that cannot work. It carries the line the name
is read on; reported without one it was drawn at the top of the file, pointing
at `<template>` for a mistake in `<script>`.

## v0.8

### The operating system's colour scheme, and a box that would not hide (2026-09-19)

`prefers-color-scheme` was wired to the window and driven by flipping Windows
between light and dark while the app was running. The probe was a single box,
red with no query and green under `@media (prefers-color-scheme: dark)`, with a
`LIGHT` and a `DARK` label switched by `display: none`. Colour rather than
timing, so a screenshot answers the question on its own.

| Case | Where | Result |
|---|---|---|
| Machine in dark mode, app launched | desktop, `rux run` | Green, `DARK`. The startup read is right |
| Flipped to light with the app running | desktop, screenshot | Red, `LIGHT`, within a second. `WindowEvent::ThemeChanged` arrives and the environment is rebuilt |
| Flipped back to dark | desktop, screenshot | Green again. It is not a one-way switch |
| `display: none` on a `<text>`, no query involved | desktop, `rux run` | **Failed**: both labels drawn, one on top of the other |

**The probe found a defect that had nothing to do with the feature.**
`display: none` reached taffy, which correctly gave the box no size and no
layout slot, and then the node was painted anyway. On a container that is
invisible, because a zero-sized box with a background paints nothing anyone can
see, which is why this survived every example in the repo. On a `<text>` it is
not: glyphs are drawn from the node's origin whether or not it has a box, so a
hidden label drew its words at its parent's origin, over whatever was really
there. Both labels of the probe rendered as one unreadable overlap.

The fix takes the same road as `r-show="false"`, which already meant "paints
nothing, hit-tests as nothing, and takes its subtree with it". The only
difference between the two, whether a layout slot is reserved, was already
settled correctly one step earlier.

**The harness lied once, and the tell was an off-by-one.** Flipping the theme
by writing the registry and broadcasting `WM_SETTINGCHANGE` by hand reported the
*previous* answer every time: dark at start, still dark after a flip to light,
light after a flip back to dark. Nothing was stale in the runtime. winit asks
uxtheme's `ShouldAppsUseDarkMode`, which caches per process and refreshes when
uxtheme itself handles that broadcast, and a synthetic broadcast can reach the
window before the cache has caught up. Sending it twice, a second apart, gives
the right answer every time. A value that trails one step behind the truth is a
cache that has not been told, not a wire that is not connected.

**Not yet driven:** the flip made through the Settings app by hand rather than
by a synthetic broadcast, and reduced motion, `rux build` and the source
provider from the session before this one.

### The `<screen>` correction, driven both halves (2026-09-19)

Two things were separated: what `<screen>` lays out, and what decides whether a
file is a page. They had been the same line of code.

| Case | Where | Result |
|---|---|---|
| A `<screen>` inside a 240x120 box with `overflow: hidden` | desktop, `rux run` | Covers the whole window: out of the clip, out of the box, out of the root's 60px padding |
| The same, as a test | `cargo test` | `(0, 0, 800, 600)` where it was `(60, 60, 240, 130)`. Proven both ways, since the change is invisible to every existing document |
| `rux check` over a `rux new` project | desktop | **Was**: "checked 1 file, no problems found" over six files. **Now**: 1 document, 4 pages through it, 1 component skipped |
| A mistake in each of the four pages, one behind the `fallback` | desktop | All four reported, each against its own file and line |
| `rux check pages/home.rux` | desktop | **Was**: `tasks is not defined`, against code that works. **Now**: checked through `app.rux`, clean |
| `rux check pages/` | desktop | **Was**: "no .rux files found". **Now**: 4 pages through `app.rux` |
| `rux check components/task-row.rux` | desktop | Unchanged: checked on its own, props reported as warnings and not errors |
| The scaffolded app in the window | desktop, `rux run` | Renders as before: header, list from the app's signals, tab bar. The runtime does not care how the checker classifies |
| All 47 documents under `examples/` | desktop, `rux check --deny-warnings` | Clean, and 13 routed pages were checked that never had been |

**The corpus found the one thing the design gives up.** Classifying by who
imports what means a component nobody uses is checked like a document, and
`examples/components/stat.rux` was exactly that: written for the M9
component-import demo, orphaned ever since, and reporting its two props as
undefined the moment anything looked at it. That is a true finding about dead
code rather than a false one about a component, and the file is gone.

**The harness lied twice, in the same way both times.** A pattern written with
`\n` matches nothing in a CRLF file, and `crates/rux-cli/src/files.rs` is CRLF
while `crates/rux-runtime/src/lib.rs` is LF, in the same repository and the
same commit. Both times it read as "the anchor has moved". Every scripted edit
here now goes through one helper that asks the file which ending it uses.

The second: a fixture written to prove the classifier failed, and the classifier
was right. `<row label="hi" />` does not pass a prop; only the bound `:label`
form does. The component really was being used wrongly by the fixture, and the
error naming `row.rux` was the checker reaching into a component through its
caller, which is the thing being built.

### Safe areas, and a device to see them on (2026-09-19)

`env(safe-area-inset-*)` is the surface that makes the inset mean anything, and
`rux run --preview` is the only way to see a non-zero one without a phone. They
were built and driven together, because either alone is unobservable.

`examples/safe-area.rux` ships as the case: a bar that pads itself clear of the
top edge, a dock that pads itself clear of the bottom, and nothing in the file
that knows which device it is on.

| Case | Where | Result |
|---|---|---|
| `rux run examples/safe-area.rux` | desktop | No padding above the bar or below the dock. Zero is the right answer for a window that owns its whole surface |
| The same file, `--preview phone` | desktop | A 59px strip above `Inbox` and a 34px strip below the dock labels, at the device's density. The file is byte for byte the same |
| `--preview tablet` on a 1280x720 logical monitor | desktop | Capped to 820 by 648 with a line saying so, rather than opening half off the bottom of the desktop |
| `--preview nonsuch` | desktop | Exits 2 and prints all four profiles with what each is for |
| An inset that moves while running | `cargo test` | Re-cascades. `set_environment` used to return early whenever every `@media` query still answered the same way, which was true right up until something read the environment directly |
| `env(safe-area-inset-top, 20px)` on a desktop | `cargo test` | 0, not 20. A fallback covers a name Rux cannot answer, not a known name answering zero |
| `env(nonsense)` | desktop, `rux check` | Two warnings: the name, and the declaration it cost. Exactly what an undefined `var()` already does |

**The window was where the first attempt looked broken, twice, and neither was
the feature.** A probe with `height: 120px` on the padded bar hid what the
padding did, because a height in Rux is the border box and the padding grows
inwards. And the stripes hugged their text instead of spanning the window,
which is the `align-items` divergence still unreverted on this branch. Both read as "the inset did nothing".

### `rux fmt` re-indented a file because it contained an apostrophe

Found writing the example above, in a `<text>` reading `this bar's own
padding`. The formatter treats `'` as the start of a string and skips to the
next one; with none on the line, it swallowed the `</text>` that closed the
element, so **every line below was indented one level deeper, and the next
apostrophe added another.**

| Case | Where | Result |
|---|---|---|
| `<text>the dog's bowl</text>` then a sibling | desktop, `rux fmt` | **Failed**: the sibling and everything under it shifted one level per apostrophe. Now unchanged |
| Two apostrophes on one line (`it's Bob's`) | `cargo test` | Worked before, by accident: the pair looked like a closed string. Still works |
| A brace inside a real string, `let s = "a { brace"` | `cargo test` | Still hidden from the nesting count, which is what the skipping is for |
| All 68 `.rux` files in the repo | desktop, `rux fmt --check .` | The fix changes none of them. The 17 that would be reformatted are unrelated drift that predates this |

`rux fmt` writes in place, so this was a tool corrupting files rather than
merely misreporting them. The fix is one rule: a quote opens a string only if
it closes on the same line. Nothing is lost by it, because neither rhai nor CSS
lets a string span a newline.

### The machine has the whole toolchain, which is the problem (2026-09-19)

`rux doctor` is the Android toolchain's diagnostic, and the machine it was
written on has every piece of that toolchain already, installed by Flutter for
unrelated work. So **a passing run here proves nothing**, and the cases worth
driving are the absent ones.

| Case | Where | Result |
|---|---|---|
| This machine | desktop, `rux doctor` | 7 of 8 found, and it caught a real absence: `aarch64-linux-android` is not installed. Exit 1 |
| This machine, after `rustup target add x86_64-linux-android` | desktop, `rux doctor` | 8 of 8 found, and it says so: "everything an Android build needs is here." Exit 0. The run above is the same machine before the target was installed, kept because the interesting half of this command is what it says when something is absent |
| `ANDROID_HOME` at an empty directory | desktop, `rux doctor` | The SDK is found and all six things inside it are missing, each with its own path and its own fix. Exit 1 |
| No SDK anywhere | `cargo test` | One finding, not eight: there is no point listing six paths inside a directory that does not exist. It names every place it looked and says to set `ANDROID_HOME` |
| A platform older than the floor | `cargo test` | Reported **unusable**, not missing: `android-21` is installed, correct and no use, and its fix is a different command |
| `9.0.0` beside `35.0.0` in build-tools | `cargo test` | 35.0.0 wins. Sorted by version component, because every string comparison puts `9.0.0` last and picking it would hand a build a toolchain seven years too old |
| `rux build --target android` | desktop | Exits 2 and points at `rux doctor`, rather than only refusing. **Superseded:** it builds an APK now |

## The first APK, 2026-09-20

The milestone for this slice was an app on a phone driven by a finger. It was
met on the emulator, which is the point: no Android device was attached to this
machine at any stage, and `adb` cannot tell the difference.

| Case | Where | Result |
|---|---|---|
| `rux-shell` for Android | `cargo check --target x86_64-linux-android` | Compiles warning-clean, and so do desktop and wasm. The whole stack links: wgpu, vello, winit and `android-activity` in one cdylib exporting `android_main` |
| `counter.rux` as an APK | emulator (Pixel 6, API 35, `-gpu host`) | It runs. Vulkan comes up on the ranchu driver, the activity displays in 2.25 seconds, and the count reads 0 in green on the dark background |
| Three taps on Add one | emulator, `adb shell input tap` | The count reads 3, and the button carries its focus ring. Signals, handlers, layout, text and paint all work unchanged on Android |
| `rux run --device` on a fresh `rux new` project | emulator | One command builds, packs, signs, installs and starts. The whole scaffolded Tasks app appears: list, checkboxes, tab bar |
| Tapping the Add tab | emulator | Routing works by finger. The page changes to New task, the tab selection follows, and the form renders with its inputs and a disabled button |
| A second `rux run --device` over the first | emulator | Reinstalls in place, because both builds are signed by the same cached debug key |
| The library entry path | emulator | **Found a defect.** The APK built, signed and installed, then died at launch: `unable to find native library counter_app`. `Path::join` on Windows had written the zip entry with backslashes, and a zip separator is `/` on every platform, so the library arrived as one oddly named file at the archive root rather than inside a directory Android looks in. Nothing before the device could have caught it: every earlier step was happy |
| The status bar | emulator | **Found a gap.** The header draws under the status bar. `env(safe-area-inset-*)` answers on desktop under `--preview`, and nothing populates it on Android yet, so a real notch is still unhandled on the one platform that has one |

## The Java shim, and safe areas on a phone, 2026-09-20

An APK now carries one Java class of its own, compiled by `javac` and dexed by
`d8`. No Gradle, no AAR, no AndroidX, no resources.

| Case | Where | Result |
|---|---|---|
| A dex in the APK at all | emulator | Proven first with a five-line subclass that did nothing, before any of it mattered. The app installed and ran identically, so packaging a dex was ruled out as a suspect before the real class existed |
| Soft keyboard on a text field | emulator | **It already worked, and that was the surprise.** Tapping a field raises Gboard, because `rux-shell` already calls `set_ime_allowed` and winit maps that to `show_soft_input`, which NativeActivity does implement |
| Typing `hi` on the soft keyboard | emulator | Both letters land, `r-model` updates, and the disabled Add task button goes live. Gboard falls back to plain key events when nothing offers it an InputConnection, and Rux handles key events already |
| Backspace | emulator | Deletes one character: `hi` becomes `h` |
| What is still missing | reading the crates | winit's Android backend never emits `Ime::Preedit` or `Ime::Commit`, and `android-activity` has `set_text_input_state` as a literal `NOP: Unsupported` on NativeActivity. So composition, autocorrect and swipe typing are absent, and the keyboard shows no suggestion strip. That is what the InputConnection in this class is for, and it is not written yet |
| `RuxActivity` loading the library | emulator | **Found a defect, and a subtle one.** The first build crashed with `UnsatisfiedLinkError: No implementation found for nativeSafeArea`, while `llvm-nm` showed the symbol exported from the very `.so` that was loaded. `NativeActivity` does not use `System.loadLibrary`; it `dlopen`s the file directly, which finds `ANativeActivity_onCreate` and registers with no class loader, so JNI cannot resolve anything against it. The class now loads the library itself, before `super.onCreate` |
| The activity name in two places | emulator | **Found a second defect**, caught by its own error message: `rux run --device` still started `android.app.NativeActivity` while the generated manifest named ours. Packaged and installed fine, refused to start. Both now read one constant |
| `examples/safe-area.rux` on a device | emulator | **The insets are real.** The bar pads below the status bar and the dock clears the gesture bar, on a screen where the scaffolded app overlaps both. Nothing in the file changed between the desktop and the phone |
| The scaffolded app under the status bar | emulator | Still overlaps, and it is **correct**: `env(safe-area-inset-*)` is opt-in and the template never asks. Logged as an author-side trap, since the first Android app a new user builds inherits the fault |
| The scaffold, after padding its own bars | emulator | **Fixed.** `rux new` now pads the screen by the top and bottom insets, and the header and the tab bar both clear the system bars. `rux check` on the fresh scaffold reports no problems, so `env()` in a shorthand-free longhand is honored rather than warned about. `calc()` does not exist yet, which is why the insets go on the screen rather than being added to the header's own padding |

## `rux build --target web`, 2026-09-20

| Case | Where | Result |
|---|---|---|
| A multi-file project to wasm | desktop, then Edge over `http://localhost` | The bundle loads and the app starts. The console shows `rux: canvas 750x485 css, surface 750x485 physical, dpr 1`, which the shell only prints after the GPU surface is created |
| **Did the documents load from memory?** | the same run | **Yes, and the absence is the evidence.** `start_web_app` logs `rux: <error>` and falls back to an empty document when `Document::load` fails, and that line is printed before the canvas line. The canvas line appears and the error does not, so a project of nine files resolved its entry, components, pages and stylesheet out of a `MemorySource` with no filesystem anywhere |
| A wasm-bindgen version mismatch | desktop | **Found by running it.** The tool was 0.2.126 and the generated crate resolved 0.2.128, after three minutes of compiling. The generated crate now pins the dependency to the version of the tool on `PATH`, so they agree by construction; the check that caught it stays as a guard |
| A rendered frame on screen | **not verified** | Headless WebGPU does not composite into a screenshot, with hardware or with swiftshader, and screen capture is unavailable in this session. The bundle initialises correctly and no frame has been seen |
| A size-tuned release bundle | desktop, then Edge | **35 MB becomes 7.0 MB**, in 4m 26s, and it still initialises: same surface line, no panic. The generated web crate carries its own release profile now (`opt-level = "z"`, LTO, one codegen unit, `panic = "abort"`, `strip`), matching what the playground already uses. The build prints the size, because it is the number that decides whether a bundle is deployable and a visitor pays it before anything appears |

## Release signing, 2026-09-20

| Case | Where | Result |
|---|---|---|
| A `[signing]` block with a real keystore | desktop, then `apksigner verify` | The APK carries `CN=Counter App Release, O=Example, C=UG` where a default build carries `CN=Rux Debug, O=Rux, C=US`. The two are visibly different keys, which is the whole point |
| A signing block with no password exported | desktop | Refused **before compiling**, naming `RUX_KEYSTORE_PASSWORD` and saying why it is not in the manifest |
| A password written into `rux.toml` | desktop | Refused by name, for all four spellings someone might reach for, with the environment variable to use instead. A manifest is a file you commit |
| A keystore that is not there | `cargo test` | Named, with the note that the path is relative to the manifest |
| Half a signing block | `cargo test` | Names the missing half rather than failing at signing time |
| `--release` with no `[signing]` block | desktop | Says the release is signed with the shared debug key and that no store will accept it, before it starts compiling. A debug-signed APK installs perfectly well, which is exactly why it is easy to ship by accident |

## Four ABIs, 2026-09-20

| Case | Where | Result |
|---|---|---|
| `rux run --device` against the emulator | emulator | Asks `ro.product.cpu.abi`, gets `x86_64`, builds exactly that and says so: `wrote ...apk [x86_64]`. A phone would get its own ABI the same way, which is what closes the develop-on-x86_64 divergence |
| A missing Rust target | `cargo test` | Named before any compiling starts, with one `rustup target add` line per missing ABI, rather than on the third of four builds |
| A four-ABI release build at a very long path | desktop | **Failed, and not because of the ABIs.** `LNK1104: cannot open file`, on a path of about 270 characters where Windows allows 260. Most of that is the scratchpad these tests run in; a project at `C:/Users/Name/projects/app` is around 150 and has room. Worth knowing because Android's generated crate path is deeper than desktop's, and because the error names a crate rather than the real cause |
| A four-ABI release build | desktop, then the emulator | **16m 34s**: 4m 39s, 3m 58s, 3m 46s, 4m 11s. The four times being nearly equal is the proof that nothing is shared between target triples. The APK carries all four libraries and is 27.3 MB |
| Installing the four-ABI APK | emulator | Android picks the right one out of the four by itself: `/proc/<pid>/maps` shows `lib/x86_64/libcounter_app.so` loaded, and the app runs with its safe areas intact |

## The input connection, 2026-09-20

The composing half of text input, in the same Java class. Every row below was
driven on the emulator, and four of them are defects that only a device could
have shown.

| Case | Where | Result |
|---|---|---|
| Typing through the input connection | emulator | **Works.** Tapping `h` then `i` puts `hi` in the field, `r-model` updates and the Add task button goes live. The connection reports the whole editing state, not a keystroke |
| Autocorrect and suggestions | emulator | **The thing that was structurally missing.** Gboard now shows a suggestion strip, `hi / Hi / HI`, which only exists when an input method has a real `InputConnection` to talk to |
| The whole flow by finger | emulator | Tap the tab, tap the field, type, tap Add task, tap back to the list: the new task `hi` is in it and the counter reads 1 of 3 done |
| Touch under a full-window overlay | emulator | Unaffected. The view that receives typing covers the whole app, and a field, a button and a tab all still reach Rux, because a plain view with no click listener does not consume a touch |
| Nested classes in the dex | emulator | **Found a defect.** `javac` writes one class file per class, including nested ones, and `d8` was handed only the outer one. The app started and died with `ClassNotFoundException` on the view it needed. Collected by walking now |
| Calling an activity method from Rust | emulator | **Found a defect.** `ndk_context` holds the **Application** object, not the Activity, so an activity method called on it fails with `NoSuchMethodError`, which the error policy logs and swallows. The symptom was a keyboard that never opened and an empty log. The activity hands itself to Rust now |
| The restart loop | emulator | **Found a defect.** An edit rewrote the field, which recomputed focus, which restarted input, which built a connection, which reported an edit. **178 reports of an empty field from one tap**, and a field that could never hold a character. Android is told only when the answer changes |
| A one-pixel editor | emulator | **Found a defect, and the one that cost the most.** An input method declines to open for an editor that small, and `showSoftInput` still returns `true`, because true means the request was delivered and never that a keyboard appeared. The failure reads as success in every log. At full size the same code opens the keyboard every time |
| Does Latin typing compose? | emulator, logging the composing span | **No, and this matters.** Gboard commits every character outright: the composing range is `-1..-1` for `h`, `he`, `hel`. So typing Latin text, autocorrect and the suggestion strip all work **without ever exercising the composing path**. The code that handles a composing range is written and is unproven |
| A CJK input method | emulator | **Could not be tested here.** This Gboard build has 145 subtypes and no Japanese, Chinese or Korean among them, and the AVD is a `google_apis` image with no Play Store to install one from. Switching the device locale to `ja-JP` and restarting the framework did not produce a subtype that does not exist. Left unproven rather than claimed |
| Composing, with a keyboard that composes | emulator, AnySoftKeyboard 1.13.8175 from F-Droid | **The composing path works, and is now proven rather than assumed.** Typing `h`, `e`, `l` gives spans `0..1`, `0..2`, `0..3`, and Rux renders the preedit underlined. Gboard could not show this because it never composes |
| Committing a composed word | emulator, AnySoftKeyboard | Space resolves it: `text=[he'll] compose=-1..-1`, then `[he'll ]`. Autocorrect **replaced** the composed `hel` with `he'll`, which is exactly what was structurally impossible before an input connection existed |
| A surrogate pair, and offsets after it | emulator, AnySoftKeyboard | **The part of our own code most likely to be wrong, and it is right.** An emoji is two UTF-16 units and four UTF-8 bytes. After inserting one, typing reports `compose=8..9`, counted in UTF-16, and the app stays alive, so `utf16_to_byte` mapped 8 to byte 10 correctly. A wrong conversion slices mid-character and panics. The underline covers exactly `ah` and not the emoji |

**A test caught the caveat in the act.** The first version of the empty-SDK
test asserted that everything under the SDK was missing, and it failed: the JDK
was found, because this machine has one on `PATH` and the lookup asked the real
`PATH`. That is precisely the failure the whole module is shaped to avoid, so
`PATH` became data like everything else, and the test now describes a machine
rather than this machine.

## The launcher icon, 2026-09-20

Spiked before it was built, on the user's call: one hand-made PNG through
`aapt2 compile` into an APK, on the emulator, before any density generation or
manifest surface existed. The spike reassembled around the library, dex and
manifest left in `.rux-build` by the previous build, so it needed **no cargo
build at all** and each attempt cost seconds.

| Case | Where | Result |
|---|---|---|
| One PNG, by hand, through `aapt2` | emulator | **The no-Gradle claim survives resources.** Two extra calls, both `aapt2`, which the pipeline already runs. `aapt2 compile --dir` takes the whole tree in one invocation, so five densities are not five processes |
| The icon on the launcher | emulator | **Visible, and the control was free.** Three other Rux apps on the same device still showed the default robot, so the difference was the resource pipeline and nothing else |
| A square PNG, as Android's own docs describe a legacy icon | emulator | **Found the real problem.** The launcher shrank it onto a white plate rather than showing it: our violet square became a small stamp. `MIN_API` is 26 and adaptive icons arrived in 26, so **every device Rux supports masks the icon**, and an app shipping a plain square looks smaller and paler than everything beside it |
| The adaptive icon, two layers | emulator | Fills the mask edge to edge. This is what the manifest keys were then designed around: `icon` is the foreground and `icon-background` is the plate |
| `-R` for the compiled resources | build only | **Found a defect before it was written.** `-R` declares an *overlay*, and an overlay may only replace a resource that already exists. A PNG alone links fine, so the wrong flag survives any test that does not add a `values/` resource; the first `<color>` fails with "does not override an existing resource", which reads like a typo and is not one. The flats go as positional arguments |
| `rux build --target android` with two manifest keys | emulator | **Driven end to end.** One 432px source file became all five density buckets, the adaptive XML and the colour resource; the icon is on the launcher and the app starts from it and renders |
| An app with no icon at all | emulator | Still builds, installs and runs, and shows the platform default. The `android:icon` attribute is absent rather than naming a resource that was never compiled, which `aapt2` would refuse to link |
| Two tests sharing one staging directory | host | **The generator's cleanup proved itself by accident.** The icon tests keyed their temp directory on the process id, so they shared one; `generate` clears the tree it is about to write, and parallel tests deleted each other's output. The product was right and the tests were wrong |

## The splash screen, 2026-09-20

Started by looking rather than by building, because Android 12 and up draw a
splash for every app whether or not it asks, and Rux targets 35. **So the
question was not "how do we add one" but "what does an app do today".**

| Case | Where | Result |
|---|---|---|
| What a cold start looks like now | emulator, frames captured across a launch | **Black. The whole way.** Launcher, then a flat black screen, then the app. Re-checked at 10x animation scale in case a frame was being missed; still black |
| Is a splash created at all? | emulator, logcat | **Yes, and this is why looking beat assuming.** `SplashScreenView: Icon: ... size: 504` says the platform builds one. It was being created and thrown away before anything could see it |
| The theme, packaged | build only | `values/themes.xml` and `values-v31/themes.xml`, the activity carrying `android:theme`. Confirmed in the APK's resource table, both configurations under one style |
| The splash, on screen | emulator | **Violet plate, white icon, full screen.** Caught only by starting the capture loop *before* the launch: it lasts about 660ms and a `screencap` takes about 400ms, so sampling after `am start` lands either side of it |
| What follows the splash | emulator, timestamped frames | **Found the defect. 1.1 seconds of black**, measured between the splash going and the first Rux frame arriving. The splash was correct and useless: it covered the first half second of a two second start |
| Holding the splash by keeping its view | emulator | **Does not work, and the log says it did.** `setOnExitAnimationListener` hands over the splash view and the app removes it when ready; the listener fired, the view was held, the release came 1.05s later exactly as designed, **and the screen was black throughout**. A `NativeActivity` renders into the window surface itself, so a view the platform hands back is never composited |
| Holding the first draw instead | emulator | **Works.** An `OnPreDrawListener` that returns false until Rux has presented keeps the platform from reporting a first frame, so the splash is never asked to leave. Frames across a launch are now splash, splash, splash, app |
| The app after the change | emulator | Re-driven rather than assumed: routing by finger, a task detail page, the Add tab, the field, the keyboard, `hi` typed into it and the button going live. Safe areas still clear both bars |
| An app with no icon | build only | No `android:icon` and no `android:theme`, so no reference to a resource that was never compiled, which `aapt2 link` would refuse |

**The trap that cost the most, and it is not an Android one.**
`RuxActivity.java` is `include_str!`ed into the `rux` binary, so **editing the
Java changes nothing until `rux` itself is rebuilt**. Two APKs were built,
installed and driven against the old class, and the second of those looked like
proof that the exit-listener approach worked when it had never run. The
symptom is silence: no error, no warning, just the previous behaviour. Probe
logging is what found it, by printing nothing at all.

## The scaffold writes a manifest, 2026-09-20

| Case | Where | Result |
|---|---|---|
| `rux new` then `rux build --target android`, nothing edited | emulator | **Works, and did not before.** A scaffolded project could be run and not built: the build stopped at a missing `rux.toml` and the author had to write one by hand before the tool they had just been told about would do anything |
| The scaffolded app on a device | emulator | Installs on the shared debug key, starts, renders, routes by finger. The whole path from `rux new` to an app on a screen is now unbroken |
| A project name with a dash | host, and it would have failed on a device | **Found a defect before it bit.** A dash is legal in a project name and illegal in an Android package segment. `dev.example.my-app` passes our own manifest check, which only looks for dots, and `aapt2` rejects it at the end of a full Android build. The scaffold now derives `my_app`, and a leading digit gains a prefix rather than being dropped |

**The reasoning expired without the code noticing**, which is the second time in
two days. The module said a manifest would commit decisions `rux build` owned
and had not made; `rux build` has since made all of them. `docs/05-as-built.md`
had the same shape of staleness and was corrected alongside the icon work.

## The native picker, 2026-09-20

`<input type="select">` opened a drawn dropdown on every platform. The spec has
asked for the platform control on mobile since before there was a phone to run
it on.

| Case | Where | Result |
|---|---|---|
| Tapping a select | emulator | **The platform's own dialog**, Material, with a scrim, and the current value already selected, which proves the value round-trips out as well as back |
| Choosing an option | emulator | Writes back through the same `apply_edit_in` the drawn dropdown uses: the field reads `mango` and the echo follows it |
| Dismissing with Back | emulator | Leaves the value alone. Dismissal is reported as an answer of -1 rather than as silence, because a shell still waiting for a reply would never open that picker again |
| The desktop dropdown | host | Unchanged. The drawn one is right there and is what `--preview` still shows |

**`jni::Env` by value in an `extern "system"` fn is not FFI-safe, and the
compiler said so.** The warning was read as noise because the surrounding JNI
functions carry a similar one. They do not: they take raw pointers. Taking
`Env` by value shifts the argument slots, so the picker's answer arrived as a
number no option had.

**Nothing failed loudly.** The dialog opened, the choice was made, Java logged
the correct index, `nativeSelectChosen` returned cleanly, no exception, no
`UnsatisfiedLinkError`, and the field did not change. It reads exactly like an
event that was never delivered, which is where the search started and is the
wrong place. The Java-side probe is what proved the answer had left Java, and
that turned the question from "why is the event lost" into "what is it
arriving as".

## Orientation and density, on a device, 2026-09-20

`@media (orientation)` has existed since v0.4 and had never been rotated. The
claim that it worked was an inference from a unit test, which is the kind of
claim this file exists to stop.

| Case | Where | Result |
|---|---|---|
| Rotating the device | emulator, `user_rotation` 0 then 1 | **Works, and live.** Portrait green, landscape orange, without the activity being recreated: the manifest declares `configChanges` for orientation, so the surface resizes and the environment is rebuilt underneath |
| `min-resolution: 2dppx` | emulator, Pixel 6 at 2.625 | Matches |
| `min-resolution: 3dppx` | emulator | Does not match |
| `max-resolution: 3dppx` | emulator | Matches. The three together pin the density between 2 and 3 rather than just asserting that something happened |

**The axis list in the range parser is not a formality.** `min-resolution: 2dppx`
never reaches the `name: value` path at all: lightningcss normalizes it to the
range form `resolution >= 2dppx` first. The range parser decides which side is
the axis from a list of names, and an unlisted name is read as the **value**
instead, so the failure was a warning that `resolution` is not a length. The
message names the axis, which is precisely the wrong end to start looking at.

## A physical phone, and why adb could not see it, 2026-09-21

The first attempt to attach real hardware to this project. No Rux code ran on
the device, because nothing could reach it.

| Case | Where | Result |
|---|---|---|
| Tecno Spark 20, USB debugging on | Windows 11, adb 37.0.0 | **`adb devices` lists nothing.** The emulator on the same server is listed throughout, so adb itself is healthy |
| What Windows sees | `Get-PnpDevice` | The phone enumerates as a MediaTek composite, `VID_0E8D / PID_201D`: MI_00 is MTP, MI_01 is ADB |
| The interface class | compatible IDs | `Class_ff & SubClass_42 & Prot_01`, the ADB signature exactly. A phone exposes that interface only when USB debugging is on, so debugging was never in doubt |
| The bound driver | `DEVPKEY_Device_Service` | `WINUSB`, from `winusb.inf`, problem code 0. Nothing is broken, missing or unsigned |
| The interface GUID | `Device Parameters` | **Empty.** No `DeviceInterfaceGUIDs` value, and that absence is the entire failure |
| Writing the GUID by hand | elevated registry write | The value takes and persists, and the interface registers under the ADB class, but its `Control` subkey never appears, so the interface is never linked |
| `pnputil /restart-device`, then a physical replug | both | Both report success. The instance id comes back identical and the interface is still not active |

**adb's Windows backend finds devices by interface GUID, not by interface
class.** `usb_windows.cpp` works through `AdbWinUsbApi.dll`, which enumerates
the ADB device interface class `{F72FE0D4-CAE5-11D3-A5C4-0050BF3B4E1E}`. A
device can therefore be plugged in, powered, driver-bound and advertising the
ADB interface class, and still be invisible, because none of that is what gets
enumerated.

**A generic WinUSB binding publishes no GUID.** The device carries an
`ExtPropDescSemaphore`, so Windows configured WinUSB from the device's own MS OS
extended-properties descriptor, and that descriptor names no interface GUID. The
descriptor path wins over a hand-written registry value, which is why writing
the value registered the interface without ever activating it. The Google USB
driver is what normally supplies the GUID on a device the inbox INF does not
cover, and it was not installed, because it ships with Android Studio and this
milestone exists to avoid Android Studio.

**The failure reads as "no phone".** `rux run --device` prints "no device is
attached. Plug in a phone with USB debugging turned on." Both halves of that
sentence were already true. Everything above is machine-readable, and none of it
is read, which is what put device diagnostics into the roadmap as v0.8 item 9.

## The pointer vocabulary, on a real screen, 2026-09-21

The same phone, reached over wireless debugging. **The first arm64 APK Rux has
ever built**: `rux run --device` read `ro.product.cpu.abi` off the phone and
compiled `aarch64-linux-android` without being told, which is the whole point of
reversing `DEV_ABI` to the emulator's x86_64. 3m 03s cold, and **7.9s to rebuild
after editing the document**, on a device none of the four ABIs had ever run on.

Driven on a TECNO Spark 20, Android 13, 720x1612 at density 2.0.

| Case | How | Result |
|---|---|---|
| `@press`, `@release` | real finger | Both fire |
| Coordinates | `input tap` at known points | **Exact.** A tap at physical 360,200 reported `rel 164,50 page 180,100`; at 360,600, `rel 164,250 page 180,300`. Predicted to the pixel at density 2.0, in both axes |
| `@drag` | `input swipe`, 1000ms | `drag end, total 0,185`. The swipe crossed 370 physical pixels, which is 185 logical. `totalX` 0 |
| `@swipe` | `input swipe`, 80ms | `swipe up, totalX 0 totalY -185`, **and** a `drag end` from the same gesture. Confirms on hardware that a drag ending as a flick fires both, which was a design decision no screen had yet tested |
| `@longpress` | real finger, held still | **Fires.** A synthetic `input swipe X Y X Y 1200` did *not* produce one, so the injector is not a substitute for a hand here |
| **Two or more fingers** | real hand | **Works, and this is the item's whole premise.** Four simultaneous points reported, and a three-finger drag fired `@drag` carrying all three. `touches` being a list from the start was the right shape: nothing in the vocabulary had to change to meet a second finger |
| Timers, with no input at all | `examples/interval.rux` | Counts 0 to 5 unaided, so `ControlFlow::WaitUntil` wakes the loop on Android. The same five clocks drive caret blink and the animator, so both are covered by this one run |

**The example that demonstrates the vocabulary could not demonstrate it.**
`examples/gestures.rux` read `event.touches.length` only in `@press`, and a
press reports one finger by construction, so the file written to show multi-touch
showed 1 on a four-finger hand. Fixed the same day by reading the count in
`@drag` as well, and driven: a two-finger drag now reports **2**. The lead text
was wrong in the same way and says what actually happens now.

**A coordinate on a touchscreen is fractional.** The same drag reported
`total 90.5,6.5` and a press at `165.5,307`, and an earlier one reported
`-70.82003784179688`. Android's `MotionEvent` carries sub-pixel positions, so
dividing by the scale factor lands anywhere, and a value that is tidy on a
desktop is seventeen digits on a phone. Logged in `docs/09-author-notes.md`.

**Two wrong conclusions were reached and corrected, both by evidence rather than
by argument, and both are the reason this file exists.**

1. **A press reported `y` of 49 three times running, which read as a scaling
   bug.** Injected taps at known coordinates proved every value exact. The
   presses had simply landed near the top of the pad. **A suspicious number is
   not a defect until something with known inputs disagrees.**
2. **A screenshot came back black and was read as a failed render**, which sent
   the search into `gralloc4: Unrecognized and/or unsupported format 0x38` and
   the `AHardwareBuffer` failures under it. The user said the app was on screen
   and running. The capture had caught the splash transition, and those gralloc
   lines are vendor noise that appears while the app renders correctly. **On
   this device `adb exec-out screencap` is reliable only once the app has
   settled**, and the earlier lesson about a black capture applies again in a
   new form: run the control, and ask the person holding the phone.

## The axis claim, settled on the phone, 2026-09-21

The rule had been half-settled since v0.7 for one reason: no screen to argue
with. The argument took about five minutes once there was one.

The probe is a scrolling list of sixteen rows with one `@drag` box in the middle
of it, which is the shape every real app has: a swipe-to-delete row, a slider, a
carousel, a drag handle.

| Case | Before | After |
|---|---|---|
| Vertical drag starting on a plain row | Page scrolls | Page scrolls |
| **Vertical drag starting on the `@drag` box** | **`@drag` fires six times and the page does not move at all** | **`@drag` does not fire, the page scrolls** |
| **Horizontal drag starting on the `@drag` box** | `@drag` fires | **`@drag` fires five times, `totalY 0`, page unmoved** |

The middle row is the defect, and it is the whole argument for the change: a
draggable element inside a scrolling list was a **dead zone**, where a thumb
that happened to land on it could not scroll the page. Scrolling is the primary
gesture on a phone, and no desktop could show this because a mouse scrolls by
wheel.

The bottom row is the half that proves the rule is a rule and not just "the
scroller always wins": nothing can scroll sideways here, so the element keeps
the horizontal axis, and a carousel inside a vertical page works with no CSS
written at all.

**The question was filed wrong, and the hardware is what showed it.** It had
been recorded as "can a scroll take the finger back mid-gesture", which assumes
the decision is about time. Nothing is taken back. The decision happens earlier
and on direction, which is what every platform does and what `touch-action`
already expresses on the web.

**A desktop regression was nearly shipped with it.** `move_gesture` is shared
with `CursorMoved`, and the mouse path has no drag-scroll at all, so a scroller
"winning" a mouse drag would have handed the gesture to something with nothing
to do with it, and drags would have stopped working in a desktop window while
every touch test stayed green. It is gated behind `from_touch`.

**Two vocabulary gates earned their keep.** `touch-action` was honored and
undescribed, and then described with no worked example; both were caught by
tests rather than by review, which is the arrangement working as designed.

## Kinetic scrolling, and a build that hid it, 2026-09-21

An 80-row list, then a 30-row one, on the phone. The gesture is the same drag
every time, 700 physical px, and only its **speed** changes: a slow one should
stop where the finger did, a fast one should carry on.

| Case | Build | Result |
|---|---|---|
| Slow drag, 600 ms | debug | Row 1 to row 7 at the top. Tracks the finger |
| **Fast flick, 100 ms** | debug | **Row 7 as well.** Identical, so no coast at all |
| Slow drag, 600 ms | **release** | Row 1 to row 8, about 126 px past the finger |
| **Fast flick, 100 ms** | **release** | **Runs the list to its end** |

**The debug build made a working fling look broken**, and it is worth knowing
exactly how. The lift velocity is timed by when the shell *processes* each move,
because winit carries no timestamp on a touch event. A build that cannot keep up
spreads a 100 ms flick over about a second of wall clock and concludes the
finger was moving ten times slower than it was. Everything downstream is then
correct and tiny.

**The arithmetic is what identified it, before the release build confirmed it.**
A fling coasts `TAU * (v0 - MIN_V)` before it stalls. Rebuilding with
`TAU = 2500` and `MIN_V = 0.001` sent the same flick to the end of the list,
which puts `v0` near 0.33 px/ms; the finger had actually moved 350 px in 100 ms,
which is 3.5. A tenfold gap is not a tuning problem, and that is what pointed at
the clock rather than at the constants.

**An 80-row list in a debug build barely scrolled at all**, moving about one row
for a 350 px drag, because Android coalesces touch events an app is too slow to
consume. That is not a scrolling defect either, and it is the same cause wearing
different clothes. Thirty rows tracked the finger correctly.

## The app came back black, 2026-09-21

Reported by the user, not by a test: leave the app and return to it, or let the
screen go off and wake it, and the app is **black**. It had been driven for a
whole day without anyone leaving it and coming back.

| Case | Before | After |
|---|---|---|
| Home, then return to the app | **Black**, status bar and scrollbar edge still drawn | Renders, **and keeps its scroll position** |
| Screen off, then on | Black | Goes through the same path; left for the user to confirm, since the phone is locked |

**Android takes the activity's surface away and hands back a different one.**
The log says so plainly: returning to the app produced a fresh
`outSurfaceControl` for the same process, so the activity was never recreated
and only its surface was. The shell was still drawing into the dead one, which
is why the app looks crashed while it is in fact working perfectly into a
surface nobody is showing.

**The bug was one line, and it was a correct line on a desktop.** `resumed`
begins `if self.state.is_some() { return; }`, which is right where `resumed`
fires once for the life of a process. On Android it fires on every return, so
the stale state made the guard skip the rebuild forever. There was **no
`suspended` handler at all**. Adding one that drops the render state turns that
same guard back into "build the state when there is none".

**Nothing the person sees is lost**, and that is a property of where state
lives rather than luck: the document, the signals and the scroll offsets are on
`App`, and only the window, surface, renderer and scene are in `RenderState`.
The list came back at the row it was left on.

**The first fix was correct and too slow, and only the user could say so.** With
the whole of `RenderState` dropped, the app came back right but showed **up to
three seconds of black**, sometimes after a half-second flash of the old frame,
which is Android's task snapshot shown before our empty surface takes over.
Rebuilding a renderer compiles shaders, and that was most of it. A renderer is
built from the device rather than the surface, and the device survives a
suspend, so it is now set aside and picked back up. The user's verdict on the
second version: instant, with no black screen visible at all.

**Measurement could not have settled the second half.** Screen capture over
wireless adb takes about 800 ms, so the floor is coarser than the thing being
judged: the first capture after returning already showed a rendered frame both
before and after the renderer cache. A person watching the screen resolved it in
one try. Prefer the eye for anything this short.

**This is the failure mode a whole day of testing could not find**, because
every test launched the app and drove it without ever leaving. Worth a standing
case: leave the app and come back, on every platform that can take a surface
away.

## Back, and what closing an app costs, 2026-09-21

Reported by the user from the phone: **the phone's Back button does not close a
Rux app.** Worked out on the emulator, because the phone was not attached at the
time, and then **driven on the Spark 20 itself** once wireless debugging came
back: arm64, API 33, 720x1612 at 2.0dppx.

| Case | Before | After |
|---|---|---|
| Back on an inner page | Nothing at all | Pops to the previous page |
| Back at the root | Nothing at all | Closes the app, whatever was behind it comes forward |
| Reopen after closing | (unreachable: nothing could close it) | Opens normally, on the phone and twice in a row on the emulator |
| Back while a guard refuses | (unreachable) | Stays put, app does not close (under test, not driven by hand) |

**The key arrives and is thrown away twice over.** A `NativeActivity` takes the
window's input queue, so a key reaches native code before any view. winit then
reports every key it decodes back to Android as handled, excepting only the
volume keys, which means the platform never runs the stage that would call
`onBackPressed`. Overriding `onKeyDown` in the Java, or `onBackPressed`, or
registering an `OnBackInvokedCallback`, would each wait for a call that never
comes. The key was there the whole time, decoded as `BrowserBack`, and nothing
was listening.

**Closing an app is not `finish()`, and believing otherwise cost two rebuilds.**
The first version asked Java to finish the activity. Back worked, the launcher
came forward, and it looked finished. It was not: winit 0.30 leaves
`MainEvent::Destroy` as a literal `warn!("TODO")`, so the Rust event loop ran on
while the Java activity was torn down. `onDestroy` waited ten seconds for a
thread that was never going to return, logged `Activity destroy timeout`, and
the next launch reused the wedged process and sat on the splash screen.

**The second version fixed the wrong thing and proved it in the log.** Exiting
the event loop makes `android_main` return, `android-activity` finishes the
activity itself, and the destroy is clean with no timeout. Reopening still
failed, and this time said why: `create event loop: RecreationAttempt`. winit
refuses a second `EventLoop` in one process, and Android had kept the process
cached and built the new activity inside it.

**So closing a Rux app ends its process**, by a `process::exit(0)` after the
loop returns. Every launch is then a cold start, which is the only kind winit
supports. Nothing is lost that was not already gone: the activity was closing.

**Three failures, three different logcat lines, and the eye would have called
all three the same thing.** "The app doesn't reopen properly" was a splash
screen, then a launcher bounce, and the cause was different each time. The
screenshots agreed; only the log distinguished them. Read logcat even when the
screen has already told you what happened.

## Text input, against a keyboard that was not the emulator's, 2026-09-21

Reported by the user from the phone, in the new-task form: **Backspace does not
work, and typing in one input after another attaches the first one's contents.**
Driven on the Spark 20 with Gboard, by tapping the real keys rather than
injecting key events, because injection never reaches an input method at all.

| Case | Before | After |
|---|---|---|
| Type into an input | Correct | Correct |
| Backspace inside the word being composed | Correct | Correct |
| Backspace anywhere else | **Nothing at all**, four presses, no change | Deletes, across the committed boundary and down to empty |
| Tap a second input and type | Second input reads **`abcs`**: the first field's buffer, including a letter deleted from it | Second input holds only what was typed into it |
| The moment of switching | First field's text appears in the second for about **half a second** | Not seen in a three-frame burst; left for the eye to confirm |

**Three defects, and the first one found was the least of them.**

**1. The guard asked the wrong question.** `android_set_text_input` took a
`bool` and skipped its work when the answer had not changed. Moving between two
inputs is "yes" then "yes", so `restartInput` was never called and Gboard went
on editing the connection built for the *previous* field, whose `Editable` still
held that field's text. It now takes which field has focus, as model, row and
instance. Typing does not change that triple, so the report loop the `bool` was
really there to cut is still cut.

**2. `restartInput` is asynchronous, and the outgoing connection gets the last
word.** Between the tap and the new connection arriving, the input method still
holds the old one and finishes its composition through it. That report carries
the old text and lands in the field that now has focus. Each focus now carries a
token, stamped on the connection when it is built and returned with every
report; a report under a stale token is dropped.

**This one is invisible to a screenshot and was reported by the user watching
the screen.** Fixing (1) made the end state correct, so every capture agreed
with the fix while the behaviour was still wrong for half a second. A burst of
three on-device captures did not catch it either. Where a defect is measured in
hundreds of milliseconds, the eye is the instrument.

**3. Backspace never reached the text.** `BaseInputConnection.sendKeyEvent`
does not edit the editable: it dispatches the key to the target view, assuming
a `TextView` that will act on it. Rux's view exists only to hold focus and
returns false from `onKeyDown`, so `KEYCODE_DEL` reached nothing, and the report
that followed carried text that had not changed. The connection now deletes for
itself, by code point rather than by `char` so one press takes a whole emoji.

**Why it looked intermittent.** While an input method composes a word it edits
through `setComposingText`, which does maintain the editable, so the first
Backspace of a fresh word works. Every one after a space, a suggestion or a
change of field does not. Testing Backspace on a single freshly typed character
passes and proves nothing; the user said so directly, and was right.

**4. A textarea's Enter did nothing, and the keyboard was right.**
Asked afterwards by the user, and worth asking: `EditorInfo` was a constant,
`TYPE_CLASS_TEXT` with `IME_ACTION_DONE`, for every input there is. It is the
only thing an app ever tells an input method about what is being edited, so a
`type="textarea"` was declaring itself one line deep. Driven: type `ab`, press
the action key, type `c`, and the field reads `abc` on one line, because Gboard
sent an editor action rather than a newline, exactly as it had been asked to.
The field's kind now reaches the connection, a textarea asks for
`TYPE_TEXT_FLAG_MULTI_LINE` with `IME_FLAG_NO_ENTER_ACTION`, and the action key
is a return arrow that inserts a line. **The documented behaviour of a shipped
type was true on a desktop and false on a phone**, which is the shape to look
for in everything else the spec claims.

**The emulator could not have found any of this.** It runs AnySoftKeyboard and
the phone runs Gboard, and the two use different halves of the
`InputConnection` protocol. Every keyboard test before today was on the
emulator. **An input method is a second implementation, not a detail:** what
the text stack is really being tested against is the keyboard, not the device.

## `password` and `search`, on the phone, 2026-09-22

Built after the user asked whether the keyboard could tell one input type from
another. It could not, and the deeper answer was that neither could Rux: four
`type=` values were acted on and everything else was silently a plain text
field. Driven on the Spark 20 with Gboard.

| Case | Result |
|---|---|
| Type into `type="password"` | Shows bullets, one per character |
| The keyboard it raises | Gboard's **incognito** mode: suggestion strip replaced by the no-learning indicator, dictation crossed out, number row added |
| Selection toolbar on that field | **Paste and Select all only.** Copy and Cut are gone and the bar has narrowed to fit |
| `type="search"` | The action key reads Search |

**Masking is display only, and that is the whole design.** The bound signal,
every handler read, and the text handed to the input method all keep the real
value; only the painted string is substituted, at the one place the shown text
is computed. Masking anywhere else breaks editing outright, because an input
method that is handed bullets will compose against bullets.

**The keyboard's part is not the masking.** Rux draws its own text, so
`TYPE_TEXT_VARIATION_PASSWORD` changes nothing on screen. What it buys is
everything an input method would otherwise do with the text: autocorrect, the
suggestion strip, and the personal dictionary. **The realistic leak it closes
is not someone reading over a shoulder. It is the keyboard learning the
password and offering it as a suggestion in a different app.** The incognito
indicator in the screenshot is Gboard confirming it will not.

**A mask that can be copied is decoration**, so Copy and Cut are refused, and
the toolbar drops them rather than showing buttons that do nothing. Paste
stays: text arriving is not text leaving.

**Bullets are counted by `char`, not by grapheme, and that is deliberate.** A
grapheme is the better unit in the abstract, but the shell steps the caret and
Backspace by scalar, so a grapheme mask would paint one bullet where the caret
has two places to stand. **A second way of counting inside one field is worse
than a coarse one used everywhere.** Whether editing should be grapheme-aware
is a real question and a codebase-wide one; a test records this so that
whoever answers it moves this too.

**The first version put the caret in the wrong place, and the user's
description of it named the bug.** Typing into a password field left the caret
part way along and it could never reach the end; the report added that "the
longer the value, the further forward it moves, about one character for every
three". That ratio *is* the defect: a bullet is three bytes in UTF-8, a caret is
a byte offset into the real value, and reading one as the other divides by
three. Six characters typed, caret after two bullets.

**It was not where it looked.** The obvious suspects were the shell's
`caret_geometry` calls, which measure text to place a caret; those were wrong
too and were fixed, but they drive the IME cursor rectangle and the
scroll-to-caret, not the caret anyone can see. The visible one comes from
`apply_focus_in` in the runtime, which attached the offset to the painted text
with `.min(len)` -- a clamp, where a conversion was needed. Selection and the
composing region had the same fault.

**Every offset that meets painted text now maps through one pair of functions**
(`masked_offset` and `unmasked_offset`, beside `mask` itself), and the
arithmetic the user observed is a test.

**`search` is one line, and that is the point.** It sets `IME_ACTION_SEARCH`
and nothing else, because a search field is a text field with a different key
in the corner. Building it proved the distinction the design rests on: **a
`type=` is a different control, and a keyboard is not a control.** `email`,
`tel` and `url` belong with `search` on the hint side, and are deliberately
still refused until that hint attribute exists.

## The clipboard, and a textarea driven by hand, 2026-09-23

Phase 2 of the inputs plan: Android's clipboard wired through
`ClipboardManager`, where it had been a stub. The stub was a data-loss bug,
not a missing feature: the drawn toolbar offered Cut on the phone, and Cut
removed the selection after a write that stored it nowhere. Cut now removes
nothing until the clipboard write has succeeded, on every platform.

Driven on the Spark 20 with Gboard, against a four-field test app (text,
textarea, password, search), each field's signal echoed on screen.

| Case | Result |
|---|---|
| Copy from one field, Paste into another from the toolbar | Pastes |
| Select all, Cut, Paste | **Doubled the text** when the Paste came from Gboard's clipboard strip, not the toolbar. Fixed, re-driven: once |
| Tap or select inside a scrolled textarea | **Landed one scroll-distance above the finger.** Fixed, re-driven: lands under it |
| Long press in an empty field | **No toolbar, so nothing to Paste with.** Fixed: Paste is offered |
| Select text in the first field on the page | **Toolbar drawn under the status bar**, hard to hit. Fixed: goes below the field |
| Drag inside an overflowing textarea | **Moved the caret; only the scrollbar scrolled.** Fixed: the text scrolls under the finger and flings |
| Type past the last visible line of a textarea | **Last line stayed half hidden.** Fixed, re-driven: fully visible |
| Paste a copied emoji; copy out to another app | Not reported separately |

**The doubled paste was the input connection's copy of the text going
stale.** An Android input connection keeps its own `Editable`, and only the
keyboard edited it. Everything Rux did to a field by itself (toolbar Cut,
Paste, Select all, a tap moving the caret) left that copy as it was. After Cut,
Gboard's copy still held the whole text with the caret at the end, and its
paste appended to it. The same fault put typing after a tap wherever the
keyboard last had the caret. The web shell has always resynced its hidden
input (`sync_web_ime`); Android now does the equivalent, rebuilding the
connection when the text changed and calling `updateSelection` when only the
selection did. **Copy never showed it, because Copy changes no text.**

**The textarea tap was the field's own scroll, left out.** The layout records
a field's text box before it shifts the field's children by their scroll, and
the tap-to-text conversion added only a one-line field's horizontal scroll. So
at the top of the field it was right, and after scrolling it was wrong by
exactly the distance scrolled, which read as coming and going.

**The hidden last line had two causes, and fixing one proved nothing.** The
shell scrolled until the line met the box's edge rather than its padding, and
the layout's maximum scroll ended at the last child without the container's
end padding, so no amount of asking could reach the right place. Scrollable
overflow now includes the end padding, as css-overflow-3 and every current
browser have it. **That changes every scroller with bottom padding**, which
can now scroll far enough to show it.

**The toolbar fixes are for the drawn toolbar**, which Android replaces with
`ActionMode` in phase 5. The web keeps the drawn one, so they are not wasted.

## Field attributes and events, 2026-09-23

Phase 3 of the inputs plan: `disabled`, `readonly`, `maxlength`, `inputmode`,
`enterkeyhint`, `autofocus`, and `@input` / `@change` / `@focus` / `@blur`.
The build side is under test in `crates/rux-runtime/tests/field_attributes.rs`
and `maxlength`'s trim in `fit_length_tests`. Everything the shell does with
them (keystrokes, focus, the keyboard) is not, and is driven here.

**Driven on the desktop without hands, and only partly.** A probe with an
`autofocus` field, a `readonly` one, a `disabled` one and a `:disabled`
button: the autofocused field took focus and its `:focus` ring at load, and
`@focus` ran and printed; the disabled field and button drew their `:disabled`
colour. **No keystroke could be delivered**: `SendKeys` was refused the
foreground, and messages posted straight to the window were ignored. The
control, a plain field focused by `mounted`, took no keys through the same
harness either, so this says nothing about the code. Typing is a hand case.

Written before the phone session. Results are filled in as driven.

| Case | Where | Expect |
|---|---|---|
| Open a page with an `autofocus` field | phone, desktop | Caret in it and the keyboard up, with no tap |
| Type 7 letters into `maxlength="5"` | phone, desktop | Stops at 5; nothing is swallowed from the end |
| Home, then type in a full `maxlength` field | desktop | Nothing inserted, the end is kept |
| Paste 10 characters into `maxlength="5"` with 2 in it | phone | First 3 of the paste land |
| Tap a `readonly` field | phone | Caret and selection work, **no keyboard**, toolbar offers only Copy and Select all |
| Type or Backspace in a `readonly` field | desktop | Nothing changes |
| Tap a `disabled` field, its label, a `disabled` button | phone, desktop | Nothing at all |
| `inputmode` numeric, decimal, tel, email, url | phone | Digits; digits and a point; phone pad; `@` to hand; `/` to hand |
| `type="password" inputmode="numeric"` | phone | A PIN pad, and Gboard suggests nothing |
| `enterkeyhint` go, send, search, next, done | phone | The action key's label says so |
| Action key on `next` | phone | Focus moves to the next field, keyboard stays |
| Action key on `done` | phone | Field loses focus, keyboard goes down |
| Type, then tap another field | phone | `@change` then `@blur` on the first, `@focus` on the second, in that order |
| Leave a field without changing it | phone | `@blur` only, no `@change` |
| Enter in a changed one-line field | desktop | `@change` once; leaving afterwards does not fire it again |
| `@input` that writes the field in upper case | phone | Text is upper-cased as typed and the keyboard does not double it |
| Choose a different option in a select | phone | `@change` with the option; choosing the same one does not fire |
| Toggle a checkbox with `@change` | phone | `event.value` is the new state |

**Driven on the Spark 20 the same day**, against a sign-up showcase (a
two-step form using every attribute above, events logged on screen). The user
reports every case working. Seen in their screenshots: `autofocus` raising the
keyboard at launch, the email keyboard with `@` and a Next action key, the
`readonly` referral code selected with a toolbar of only Copy and Select all,
the textarea's `@input` counter at "60 characters left", and `@change` on the
select logging `Tanzania`. The desktop keystroke cases remain undriven, for the
harness reason above.

**The same session reopened the selection toolbar**, by setting Rux's beside
WhatsApp's on the same phone. Three differences, all phase 5 work:

- **Highlight colour.** WhatsApp tints the selection in its accent (green at
  partial alpha); Rux uses one fixed blue-grey for every app.
- **Handles.** WhatsApp draws a teardrop handle at each end of a selection,
  and a single one under the caret after a tap or a long press on empty text.
  Rux draws none, so a selection cannot be adjusted by finger.
- **The toolbar itself.** The system's floating toolbar is a rounded dark
  surface with no dividers, offers Autofill and an overflow arrow, and matches
  every other app on the phone. Rux's drawn one has borders, dividers and a
  different type, and reads as foreign.

## Numbers, switches, sliders and dates, 2026-09-23

Phase 4 of the inputs plan: `type="number"`, `switch`, `slider` and `date`.
The build and the document are under test in
`crates/rux-runtime/tests/new_controls.rs`, including the slider's generated
script run through the real engine (a tap, a drag, a fractional step, a tap
with no pointer). The number and date parsers are under test in the shell.
The keyboard, the finger, the axis claim and the platform picker are not, and
are driven here, against `rux-harness/phase4-controls` (six numbered sections,
events logged at the top).

Written before the phone session. Results are filled in as driven.

| Case | Where | Expect |
|---|---|---|
| Tap the number field | phone | A number keyboard with a minus sign and a decimal point |
| Type `-`, then `1.`, then `1.5` | phone, desktop | The field shows each as typed; `qty` stays 2 through `-` and `1.`, then reads 1.5; `type` says `f64` |
| Type a letter into the number field | desktop | It stays in the field; `qty` does not change |
| Clear the number field, then tap elsewhere | phone | Empty while focused (placeholder shows), then the last number comes back |
| `24,000` on an English phone | phone | `qty` reads 24000; `2,5` reads 25; `1.234,5` stays a draft |
| `@input` and `@change` on the number | phone | `input` fires only when the number changes; `CHANGE` once on leaving |
| `inputmode="numeric"` on a number | phone | A digits-only pad |
| Tap each switch | phone, desktop | The thumb jumps to the other end, the track turns blue (green for the second); `CHANGE wifi true/false` |
| Tap the disabled switch | phone | Nothing |
| Tap along the volume slider | phone, desktop | The thumb lands under the finger, snapped to whole steps; one `input` and one `CHANGE` |
| Drag the volume thumb sideways | phone, desktop | The value follows the finger; `input` per step, `CHANGE` once on lifting |
| Drag the pink slider | phone | 0.1 steps, reading `0.3` and never `0.30000000000000004`; the swatch fades |
| Swipe up or down starting on a slider, and on a switch | phone | The page scrolls; the slider does not move |
| Tap the first date | phone | The platform date picker, on 15 October 2026, with days before September and after December greyed out |
| Choose a day, then dismiss with Back | phone | The day is written and `CHANGE due` fires; Back leaves the day alone |
| Tap the empty date | phone | The picker opens on today |
| Tap the read-only date | phone | No picker |
| Type `2026-9-3` into a date | desktop | Written back as `2026-09-03` on leaving; a half-typed date never reaches the signal |
| A screen reader on each control | phone | Announced as a number field, a switch, a slider with its value, a date |

**First phone results, same day.** The date picker opened on today for the
empty field. Dragging the volume slider back and forth logged one `input` per
whole step and one `CHANGE` on release, and the thumb **hops from step to
step**, as the user put it "just like the switch". That is the step doing its
job: HTML's range input and Android's discrete `SeekBar` both snap while
dragging, and `step="any"` is the continuous one. The switch's jump is the
missing slide animation, a gap already written down.

**Second round, same day, from the user's screenshots.** Typing `-0.356`
digit by digit logged `input 0 · -0.3 · -0.35 · -0.356` with `type f64`
throughout, and `-23.3` in the second field gave `age + 1 = -22.3`: the draft
rule holds. Two drafts that are not numbers behaved as decided, and are worth
knowing about. `,959494956262.22` showed as typed while `age` held
`0.959494956262`, the last text that *was* a number (a leading comma read as
the point). `284846,65946564,659594` held `284846.65946564` for the same
reason. Nothing marks such a field as not holding what it shows; that is
`:invalid`, phase 6. **The comma rule guessed**: `24,000` typed as a thousand
read as 24, and caught the user out. HTML and Android both refuse `24,000`
in a number field; the user chose the forgiving answer instead, and it is
built: the decimal separator is the device language's, and the other one is a
thousands separator before the point. Swiping
up and down starting on a slider scrolled the page and left the slider alone.

The keyboard on this phone is not Gboard, and it offered the same keys (minus,
comma, point, space) for `type="number"` and for `inputmode="numeric"`. What
Android is told differs; which keys appear is the keyboard's decision.

**The screen reader case cannot pass yet**, and asking for it here was a
mistake in the table. Rux publishes its accessibility tree only on the desktop:
accesskit is not built for Android or the web, so TalkBack finds nothing inside
a Rux app at all. That is a gap far wider than this phase, and it is recorded
as one.

**A defect the log itself showed, outside this phase.** The event log is a
`<text>` with `height: 72px; overflow: clip`, and its lines ran down over the
whole page. `overflow` clipped a node's *children* and never its own glyphs,
and a text has no children. Fixed in `rux-layout` (the glyphs are painted
inside the clip) and pinned by `crates/rux-runtime/tests/text_overflow.rs`,
which fails without the fix. Re-driven on the phone: twenty taps on a switch
fill the log and it stops at its border.

## Native selection on Android, 2026-09-23

Phase 5 of the inputs plan: the platform's text menu, selection handles,
`PROCESS_TEXT`, `::selection` and the platform highlight. The cascade is under
test in `crates/rux-runtime/tests/selection_style.rs` and the handle geometry
in the shell; the finger, the menu and the other apps are driven here, against
`rux-harness/phase5-selection` (seven numbered fields). Rows marked *adb* were
driven over `adb shell input` and read from `screencap`; the rest are for a
hand.

| Case | Where | Expect | Result |
|---|---|---|---|
| Long press a word | 1 | The whole word selected, a teardrop at each end, the platform menu: Cut, Copy, Paste, Share, then an overflow | adb: pass, "brown" selected. Before a fix it took "br": the first move after the press cut the word back to the finger |
| Overflow | 1 | Select all, then the `PROCESS_TEXT` apps | adb: pass, Select all and "Ask Gemini" |
| Drag the end handle right | 1 | The selection grows by the finger, the start stays, the menu steps aside and returns on lift | adb: pass, "brown" became "brown fox" |
| Drag a handle past the other | 1 | The ends swap sides; the selection never collapses to nothing | |
| Back with a selection | 1 | The selection goes; the app stays open | adb: pass |
| Plain tap on text | 3 | Caret only, no handle, no menu | adb: pass |
| Long press after the last word | 3 | Caret at the end, the caret handle, Paste and Select all; all gone after about 5 seconds idle | adb: pass, at 3 seconds; the user then asked for 5 as 3 was too short |
| Tap the caret handle | 3 | The menu toggles; the handle's 5 seconds restart | |
| Drag the caret handle | 1 | The caret follows the finger, the field scrolls when it reaches an edge | |
| `::selection` and `accent-color` | 3 | Gold highlight, dark letters, orange handles | adb: pass |
| Default colours | 1 | Highlight and handles in the phone's own accent, as in WhatsApp's fields | |
| Password | 4 | The run of bullets selects as one; menu offers Paste only (Select all is gone once all is selected) | adb: pass for the selection; Select all fix re-driven below |
| Read-only | 5 | Copy, Share, Select all and the apps; no keyboard | adb: pass |
| Copy | 1 | Text on the clipboard, the selection lets go, caret at its end | |
| Translate or Gemini on a selection | 1 | The app opens on the selected text; if it answers, the answer replaces the selection | |
| Share | 1 | The share sheet with the selected text | |
| Paste into a list row | 6 | Stays in that row. Before this phase paste, cut and a committed composition refocused a field with no row, so an input in an `r-for` or a component lost focus mid-edit | adb: pass after a second fix. The user found cut copying but the field keeping its text: `r-model="item.name"` wrote to the loop variable, a copy, so no edit in a row ever stuck. Now Cut empties row 2 and Paste after "gamma" gives "gammabeta" in row 3 |
| Password with a space, long press on the bullets | 4 | All of it selected, not the part on one side of the space | adb: pass ("hunter2 hunter2") |
| Password, long press past the bullets | 4 | Caret at the end, its handle, Paste and Select all | adb: pass |
| Long press in the empty field | 7 | Paste (if the clipboard has text), the caret handle | |
| Caret menu contents | 3 | Paste only with something on the clipboard, Select text, Select all | adb: pass. First build: the menu never showed when the long press also focused the field, because the keyboard attaching restated the focus and that closed the menu; now only a caret that moves closes it |
| Select text | 3 | Takes the word before the caret: "letters" | adb: pass |
| Handles in the textarea | 2 | Across lines; a handle whose end scrolls out of the field disappears | |

**Seen while driving, outside this phase:** a field low on the page is
focused under the keyboard and the page does not scroll it into view; and Cut
or Paste raises the keyboard even when it was down, because a text change
rebuilds the input connection through the call that also shows the keyboard.
Both are on the watchlist.

**The menu sits below the selection near the top of the screen.** On field 1
there is no room between the status bar and the text, so the platform's own
placement rule puts it under the handles, over the next field. That is
`FloatingToolbar`'s decision, the same one it makes for a `TextView`.

## Forms, 2026-09-23

Phase 6 of the inputs plan: `role="form"`, submitting, the checks and the
pseudo-classes that show them, the action key, and a field the keyboard
covers. What a submission checks and sends is under test in
`crates/rux-runtime/tests/forms.rs`; the keys, the keyboard and the screen are
driven here, against `rux-harness/phase6-forms` (form A, fields 1 to 7 and
Join; field B outside any form; form C, a search; field 8 low on the page).
*desktop* rows were driven in the window with SendKeys.

| Case | Where | Expect | Result |
|---|---|---|---|
| Action key on each field | A 1 to 5, 7 | Next on 1, 2, 3 and 4; Go on 7. The textarea (6) shows Enter | |
| Next | A 1 | The caret moves to 2, then 3, then 4, then 6; the checkbox is passed over; the keyboard stays up | desktop: pass, with Tab-style Enter before the Enter rule changed |
| Next into the textarea, then out | A 6 | Enter makes a new line; tapping 7 moves on | |
| Go with bad values | A 7 | Refused: 2, 3, 5 go red, the caret goes to the first bad field, `errors` lists why, `tries` counts 1 | desktop: pass |
| Nothing red before it is touched | A | A fresh form shows no red; a field goes red or green only after it was changed and left | desktop: pass |
| Join with good values | A | `taps` and `tries` count 1, `sent` shows every value, `age` a number, `terms` true | desktop: pass |
| Age below 18 | A 4 | Refused with "Value must be 18 or more." | desktop: pass |
| Tap the label "1. name" | A | The caret goes to field 1 | desktop: pass |
| The button's label | A | "Join" shows, written without a `<text>` | |
| Keyboard Enter in field 1 | A, desktop or a hardware keyboard | Submits (refused, since the form is empty); Tab moves to 2 | |
| Done outside a form | B | The key says Done and closes the keyboard | |
| Search | C | The key says Search; with text it submits and `searched` shows it; empty, it is refused and the caret stays | |
| A field under the keyboard | 8 | Scroll so 8 is low on the screen, tap it: the page moves it above the keyboard | adb: pass on the second build. The user found the first covered: it waited for the window to shrink, and from Android 11 `adjustResize` never shrinks it (the frame stayed 720x1612 with the keyboard up). Now the keyboard's inset is read and the page is laid out above it |
| Next after typing a word | A 1 | The caret moves on, and the typed word stays | adb and by hand: pass on the third build. The first two never moved: the input method's Enter is dispatched from no device and never reaches the window, so Next had never worked, only been reported as working. Now Enter comes over JNI |
| Close the keyboard after a reveal | C | The page goes back to where it was before the field was moved; a page the person scrolled meanwhile stays put | adb: pass. Asked for by the user. Once, the tap that raised the keyboard also showed the caret handle and Paste; not reproduced |
| Autofill suggestions | D email | Tap it: the password manager's accounts appear under the field or in the keyboard strip; the caret menu offers Autofill | adb: pass, both the dropdown under the field and Gboard's inline strip |
| Pick a suggestion | D email | That field fills, the caret after the text; form A below is untouched | adb: pass on the third build. The first offered every field on screen and Google also filled A's name and email; the second filled A but wiped D, because the keyboard's connection was restarted without a new token |
| Sign in, then save | D | "Save password to Google?" with the address and the password typed | adb: pass. Answered Not now, since Save would store a test password in the user's account |
| `autocomplete="off"` | any | No suggestions, no Autofill item | |

## Coming back after Android kills the app, 2026-09-23

Phase 7 of the inputs plan. Android reclaims a backgrounded app whenever it
wants the memory, and on the Spark 20 it wants it within a second of Home, so
"switch away and come back" used to mean "start again at the first page". The
activity now keeps the history (each entry with its scroll) and the focused
field (text and caret) in `onSaveInstanceState`, and puts them back before the
first frame. Signals are not kept: an app's data is the app's. The history
half is under test in `crates/rux-runtime` (`a_restored_app_is_where_it_was`,
`a_restored_history_goes_through_the_guard`). Driven against
`rux-harness/phase7-restore`: a nav bar, a list of 80 rows, an item page of 60
lines, and a form page with a text field, a password, a number and a textarea.
*adb* rows were driven over wireless adb: Home, `am kill` (which ends only a
backgrounded process, as the low-memory killer does), then the launcher intent.

| Case | Where | Expect | Result |
|---|---|---|---|
| Page and scroll | item page | Open the list, scroll, open a row, scroll it, Home, kill: reopening shows the same item at the same line | adb: pass (`/item/20` at line 14, in a new process) |
| Back after a restore | item page | Back lands on the list at its own scroll | adb: pass |
| Focused field and its text | form, note | Type, Home, kill: reopening shows the text, the field focused, the keyboard up | adb: pass |
| Caret inside the text | top-level field | Put the caret after the first letter, kill: typing after the restore goes in there | adb: pass ("h\|it", then "s" made "hsit") |
| A password | form, secret | Comes back focused and **empty**, with the password keyboard | adb: pass |
| A textarea | form, long | Two lines come back with the line break | adb: pass |
| A number half typed | form, age | "42." comes back as typed, with the number keyboard | adb: pass |
| Other signals | form | `typed` and the button's count start again; one `@input` runs for the restored text | adb: as designed |
| A guarded page | any | Signed out in the new process, the guard turns the restore away | runtime test only |
| "Don't keep activities" | any | The activity is destroyed with the process alive; coming back must not crash | Not provoked: neither the phone nor the emulator applied the setting when it was set over adb |
| The activity rebuilt in the same process | form | Change the system font size with the app open: it keeps working | emulator: **failed, then fixed.** Android rebuilt the activity in the same process, the app showed its last frame and was killed as not responding. The manifest now claims every configuration change, so nothing rebuilds it; re-driven, the font size changed twice and the app kept taking taps in the same process |
| Reopened from Recents | list | Kill in the background, tap the app's card: the list comes back at its scroll | emulator: pass (a new process, `/list` at row 12) |
| Reopened from the launcher icon, by hand | any | As above, from the real icon rather than the adb intent | |
| Tap to move the caret in a component field | form, note | The next letter goes in where the tap put the caret | emulator: pass. The phone's miss was a lost tap |

Found alongside, not yet fixed: tapping inside a word the keyboard is still
composing moves the caret, but AnySoftKeyboard keeps composing the old word, and
the next letter leaves it doubled ("hello", tap after the h, "z" gives
"hzelloo"). Any field, not only a component's. And adb's injected keys garble a
field on the phone under Gboard but type cleanly on the emulator under
AnySoftKeyboard, so that one needs the phone again.

Three defects came out of driving this, none of them phase 7's own logic:

- **`rux run --device` started the app with a bare `am start -n`.** The task's
  root intent then did not match the launcher's, so reopening stacked a fresh
  activity on the task and the saved state was never read. It now starts the
  app as the launcher does.
- **A field inside a component with an `@input` lost every keystroke.**
  Handlers were baked with the instance's own state as it was at the last
  build, so `@input="typed += 1"` wrote the field's previous text back over
  what had just been typed. Every platform, not only Android; `tests/input_in_a_component.rs`
  now covers it with the handler taken from the laid-out tree, and fails
  without the fix.
- **A configuration change Android was not told the app handles** (font
  size, language, a Bluetooth keyboard) rebuilt the activity in the same
  process, which a Rux app cannot survive: winit stays on the old one. The
  generated manifest now lists every change API 26 knows.

## Standing gaps

Cases nothing here can currently exercise. They are the shape of what v0.8 has
to prove.

- ~~**Two or more fingers**: reported by the runtime, never yet produced.~~
  **CLOSED 2026-09-21**, above: four at once, and a three-finger drag.
- ~~**Kinetic scrolling and inertial fling**: unimplemented.~~ **CLOSED
  2026-09-21**, above. Built and driven, and judge it in a release build.
- ~~**The axis claim in full**: a `@drag` claims the finger, but whether a
  scroll can take it back mid-gesture needs a real screen to have an opinion
  about.~~ **CLOSED 2026-09-21**, above. The screen had an opinion, and it was
  that the question was the wrong one.
- ~~**Native pickers, safe areas, orientation, density**: no device.~~ Driven on
  the emulator 2026-09-20, and **not yet re-driven on the phone**.
- ~~**IME on real hardware**: composition is proven only against
  AnySoftKeyboard on the emulator.~~ Partly closed 2026-09-21, above:
  typing, Backspace and moving between fields are driven against Gboard on
  the phone. **Composition proper is still not**, because English Gboard
  composes a word rather than a character; CJK and a dead key remain
  emulator-only.
- **Every `type=` that is not one of the five is silently a text field.**
  The only values any code inspects are `radio`, `checkbox`, `textarea`
  and `select`, and nothing validates the rest. `<input type="password">`
  parses, renders and accepts typing **as visible plain text**, with no
  masking and no warning; `email`, `number` and `search` likewise become
  plain text. Not a missing feature but a silence, and the password case
  is one an author would not notice until it mattered.
- **A locked screen captures as pure black.** Not a standing gap, but worth
  knowing: on 2026-08-19 every screenshot came back black until the user
  unlocked the machine, including one of a known-good example. Run the control
  before concluding a feature is broken, and if the control is black too, it is
  the screen and not the code.
