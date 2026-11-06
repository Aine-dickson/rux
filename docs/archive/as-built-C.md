## Touch: the renderer is kept across the suspend
removed 2026-11-06 from docs/05-as-built.md

Dropping the whole of `RenderState` is correct and was visibly slow: building a renderer compiles shaders, and the app showed up to three seconds of black on every return, sometimes after a half-second flash of the old frame, which is Android's own task snapshot being shown before our empty surface takes over. A renderer is built from the device, not from the surface, and the device survives, so it is set aside on suspend and picked back up on resume if the device still matches. With it, returning to the app is immediate.

The failure this fixes reads as a crash: an app that drew into the dead surface came back black while working perfectly. It hid behind `resumed`'s `if self.state.is_some() { return; }`, which is correct on a desktop, where `resumed` fires once for the life of a process, and wrong on Android, where it fires on every return.

## Scrolling: content-visibility benchmark
removed 2026-11-06 from docs/05-as-built.md

Long-list benchmark (2,000 rows of three texts in a scroller, scrolled a little each frame, release, desktop), measured when it landed:

| | every row laid out | content-visibility: auto |
|---|---|---|
| layout, a scrolled frame | 23.8 ms | 7.7 ms, 3.1x faster |
| scene, a scrolled frame | 0.75 ms | 0.33 ms, 2.3x faster |
| the first frame layout | 127 ms | 18.8 ms, 6.7x faster |
| building the document | 104 ms | 109 ms, the same |

Since then a scrolled frame is 0.9 ms without content-visibility and 1.0 ms with it.

## Touch: why hardware matters
removed 2026-11-06 from docs/05-as-built.md

Touch went a long time doing only the scrolling half, because there was no touch hardware to try it on. It was found within a minute of the playground being opened on a phone, so treat "no hardware" as a reason to be suspicious of a path rather than a reason to call it done. Multi-touch reporting was confirmed on a phone on 2026-09-21: four simultaneous points, and a three-finger drag carrying all three.
