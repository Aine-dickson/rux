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

## Standing gaps

Cases nothing here can currently exercise. They are the shape of what v0.8 has
to prove.

- **Two or more fingers**: reported by the runtime, never yet produced.
- **Kinetic scrolling and inertial fling**: unimplemented, untestable here.
- **The axis claim in full**: a `@drag` claims the finger, but whether a scroll
  can take it back mid-gesture needs a real screen to have an opinion about.
- **Native pickers, safe areas, orientation, density**: no device.
