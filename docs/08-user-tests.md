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
