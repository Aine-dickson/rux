# rux-audio

Rux's sounds on the desktop and Android: `symphonia` to decode, `cpal` to play,
and a mixer between them that sums every playing sound into one output stream.

The stream is opened by the first sound played, never by one only loaded, so an
app that makes no sound never touches an audio device. Short sounds are decoded
whole and play at once; long ones are read a little ahead of where they play, on
a thread of their own, so a song is never held decoded in memory. Formats are
WAV, MP3, AAC in MP4, FLAC and Ogg Vorbis. The web plays through the browser and
does not use this crate.

## Internal to Rux

This is one crate of the [Rux](https://ruxlang.dev) workspace. It is published
so the toolchain builds from crates.io, not as a library to depend on: it makes
no stability promise of its own and its API moves whenever the runtime needs it
to. The supported entry point is
[`ruxlang`](https://crates.io/crates/ruxlang).
