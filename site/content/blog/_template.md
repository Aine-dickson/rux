+++
# Copy this file to `vX-Y-Z.md` (dashes, not dots) for a new release post.
# Delete these comments once you've filled it in.
title = "Rux vX.Y.Z — the one-line story of the release"
description = "A sentence or two for the post list and social cards: what landed, concretely."
date = 2026-01-01                       # the real release date
draft = true                            # flip to false to publish

[extra]
version = "vX.Y.Z"
+++

<!--
  The shape of a Rux release post — see v0-1-0.md and v0-2-0.md for the tone.
  It is NOT a changelog. Order: what landed → what it cost → what's still broken.

  Two paragraphs are mandatory (see RELEASING.md):
    · the bug you only found by *looking* (every release has had one)
    · what this release is NOT — the honest limits, as plainly as the wins
-->

One or two sentences that say what this release is about — the theme, not a list.

## What landed

The features, grouped by theme. For each, say what it does *and* the one
interesting thing about how it works or what it cost. Prose, not bullets of
verbs.

## The bug I only found by looking

The defect the tests were green through, obvious the moment the window opened.
Name it, and name why the suite missed it. This is the most re-read paragraph in
every post.

## What this release is not

The limits, stated as plainly as the wins. What still doesn't work, what's
unverified, what you'd reasonably expect that isn't here yet.

## Next

The one or two things the next release is about.
