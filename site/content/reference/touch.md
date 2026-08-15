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

**Not done:** no kinetic or inertial fling after the finger lifts, no
multi-touch, and no pinch zoom.

**Touch text has its own gestures**, added in v0.5.1 and confirmed on hardware.
Until then a finger dragged across text *selected*, because touch was routed
down the same press/drag/release path a mouse takes, which is the desktop model
and not what a phone does:

- **drag** on text moves the caret along the path;
- **long press** (500 ms) selects the word under the finger;
- **long press then drag** extends the selection from that word.

The long press is therefore the only gesture that selects, which is what frees a
drag to mean something else. The decision is one-way: a press that moves before
the timer is a caret drag and cannot become a selection however long the finger
then rests, so a drag never turns into a selection halfway through. A resting
finger raises no events, so the press deadline is a second clock in
`about_to_wait` beside the caret blink. The mouse is unchanged and still
drag-selects.
