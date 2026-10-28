# rux-video

Rux's video decoding: each platform's own decoder, read into RGBA frames with
their times, one at a time on whatever thread asks. Windows decodes through
Media Foundation, which plays what the machine has codecs for (H.264 always;
HEVC, VP9 and AV1 with their store extensions). Android's `MediaCodec` is the
next platform; the web plays through the browser and does not use this crate.
Elsewhere opening a video is an `"unsupported"` error.

## Internal to Rux

This is one crate of the [Rux](https://ruxlang.dev) workspace. It is published
so the toolchain builds from crates.io, not as a library to depend on: it makes
no stability promise of its own and its API moves whenever the runtime needs it
to. The supported entry point is
[`ruxlang`](https://crates.io/crates/ruxlang).
