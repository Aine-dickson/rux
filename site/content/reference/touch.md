+++
title = "Touch"
description = "What a finger does today, and why @tap is the whole vocabulary."
weight = 9
+++

<!-- GENERATED FROM docs/05-as-built.md BY site/sync-docs.sh. DO NOT EDIT HERE. -->

A finger takes the same path as the mouse: it taps buttons and toggles, focuses
inputs, drags a scrollbar thumb, drags out a text selection, and scrolls content
directly when it grabs something that is none of those. A drag that stays inside
the tap slop is still a tap.

Touch went a long time doing only the scrolling half, because there was no touch
hardware here to try it on. It was found within a minute of the playground being
opened on a phone, so treat "no hardware" as a reason to be suspicious of a path
rather than a reason to call it done. In a browser the canvas also needs
`touch-action: none`, or the page claims the gesture and the runtime never sees
a drag.

**Kinetic scrolling, added 2026-09-21.** A finger that leaves the screen while
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
- **It stops at the ends rather than bouncing**, because there is no overscroll
  to bounce into yet.

**Not done:** no pinch zoom, and no overscroll or rubber-banding. Multi-touch is
*reported* (see the pointer vocabulary below) but nothing in the runtime
interprets a second finger yet. The reporting was confirmed on a phone on
2026-09-21: four simultaneous points, and a three-finger drag carrying all
three.

**A debug build under-flings, and it looks exactly like a broken fling.** The
lift velocity is timed by when the shell *processes* each move, because winit
carries no timestamp on a touch event. An app that cannot keep up spreads a
100 ms flick across a second of wall clock and concludes the finger was ten
times slower than it was. On the same phone and the same gesture, a debug build
coasted about one row and a release build ran the list to its end. **Judge the
feel of a fling in a release build**, and see `docs/08-user-tests.md`.
