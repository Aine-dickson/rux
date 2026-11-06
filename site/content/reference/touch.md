+++
title = "Touch"
description = "What a finger does: taps, presses, drags and swipes, and what is not built."
weight = 9
+++

<!-- GENERATED FROM docs/05-as-built.md BY site/sync-docs.sh. DO NOT EDIT HERE. -->

A finger takes the same path as the mouse: it taps buttons and toggles, focuses
inputs, drags a scrollbar thumb, drags out a text selection, and scrolls content
directly when it grabs something that is none of those. A drag that stays inside
the tap slop is still a tap.

In a browser the canvas needs `touch-action: none`, or the page claims the
gesture and the runtime never sees a drag.

**Kinetic scrolling.** A finger that leaves the screen while
still moving throws the scroller it was dragging, and the content coasts to a
stop. Touch only, and only on a box that was actually scrolled: a lift that
moved nothing has nothing to throw, and a tap never flings.

- **Velocity is measured over the last 100 ms of the finger's path**, not from
  the final pair of events. Measuring the last move alone throws a list that the
  hand had already brought to a stop, because the closing delta can be a stale
  jump over a tiny interval. A window reports roughly zero for a finger that
  had stopped, which is the answer that matters.
- **The decay is time-based, not per-frame.** Velocity falls by `1/e` every 325
  ms, and each step travels the integral of that curve rather than
  `velocity * elapsed`. A per-frame multiplier is the usual shortcut and it ties
  how far a list is thrown to the refresh rate, which phones vary while running.
- **A new finger stops it**, which is how a long throw is caught.
- **It stops at the ends rather than bouncing.** Rux draws no overscroll effect.
- **A scroller with snap points** (`scroll-snap-type`) glides to the box nearest
  where the throw would have stopped instead of coasting.

**Not built:** pinch zoom, rotate and rubber-banding. Multi-touch is *reported*
(see the pointer vocabulary below, where `touches` lists every finger down) but
nothing in the runtime interprets a second finger.

**A debug build under-flings, and it looks exactly like a broken fling.** The
lift velocity is timed by when the shell *processes* each move, because winit
carries no timestamp on a touch event. An app that cannot keep up spreads a
100 ms flick across a second of wall clock and concludes the finger was ten
times slower than it was. On the same phone and the same gesture, a debug build
coasted about one row and a release build ran the list to its end. **Judge the
feel of a fling in a release build**, and see `docs/08-user-tests.md`.

**The Android surface is not the app's to keep.** Leaving the app destroys the
activity's surface, and returning gives it a **different** one, in the same
process with the activity never recreated. So `RenderState` (the window, the
surface and the scene) is dropped in `suspended` and rebuilt by `resumed`, which
is the pattern winit asks for. Nothing the person sees is lost, because the
document, the signals and the scroll offsets live on `App` rather than in the
render state, and the app returns to the row it was left on.

**The renderer is kept across the suspend.** Building one compiles shaders,
which showed as up to three seconds of black on every return. A renderer is built
from the **device**, not from the surface, and the device survives, so it is set
aside on suspend and picked back up on resume if the device still matches. On
Android `resumed` fires on every return, so it must not assume a first start.
