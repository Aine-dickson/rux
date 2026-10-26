# Plan: the file input, then audio, then video

Status: **planned 2026-10-23, from [the file input proposal](12-file-input-proposal.md);
approved by the owner the same day, with every assumption and question below
answered as written.**
The owner asked for audio and video elements in the same plan, which answers
the proposal's decision 8 (an audio proposal now) and widens it to video.

Three parts, built in this order, because each one uses the one before it:

- **A. `<input type="file">` and the `File` value**, the proposal's four phases.
- **B. Audio**: a `loadSound` script call, then `<audio>`.
- **C. Video**: `<video>`, whose sound goes through B's output.

Every step ends the way steps here always end: the gate green, the change
seen in the window (or on the emulator), its cases written into
[08-user-tests.md](08-user-tests.md), and a local commit. Nothing is pushed
without the owner saying so.

## What this plan assumes about the open decisions

**Decided 2026-10-23: all as assumed.** The table stays as the record of what
each answer cost, and what would move if one were revisited.

| # | Assumed | If the owner decides otherwise |
|---|---|---|
| 1 | A built-in `File` with `name`, `size`, `type`, `url`, async `text()` | Renames only; A.1 changes, nothing after it |
| 2 | No bytes for now | A `Bytes` type is a language step of its own, before B.1 if audio wants raw buffers (it does not need them) |
| 3 | `pickFile` returns `File?`, `file.path` on the desktop | A.6 becomes "add `pickFileAsFile`" and Mava Studio changes nothing |
| 4 | The browser's look: "Choose file" + the name | A.3 draws a plain button; `::file-selector-button` stays quiet |
| 5 | No size limit on `text()` | A limit is one check in A.4 |
| 6 | `capture` in the Android phase | A.10 moves after part C |
| 7 | `@cancel` | Dropped from A.4 and A.8 |
| 8 | Audio and video planned now | Answered by the owner's request |

The roadmap held audio at v0.9 item 8 and video at v0.11, video "because it
wants mobile settled". Mobile is as settled as it will be before v1.0
(v0.8 is frozen, the Android input phases are built), so building video now
is the owner moving it forward, and the roadmap is updated to say so in A.0.

## What the code has today, and what that changes

Read before planning, because three facts shape the steps:

- **`<image :src>` reads paths only.** `rux-paint`'s `ImageCache` decodes
  whatever `rux_layout::read_image_bytes` returns, and the runtime's reader
  resolves a `src` as a project file (`rux-runtime/src/lib.rs`, `load_checked`).
  A `file:` URL, a `blob:` URL and a `content://` URI all fail today. So the
  proposal's "`File.url` is what `<image :src>` shows" needs the reader taught
  each scheme, and that is a step of its own (A.2), not a side effect.
- **`Value` has no object type.** `rux-reactive::Value` is numbers, text,
  booleans, lists, maps and `none`. A `File` cannot be a map, because
  `text()` is a method and the checker must know `photo.name` is a string.
  It becomes a built-in record type in the checker, carried at run time as
  a handle into a shell-side table of picked files (A.1).
- **Android already round-trips an activity result.** `RuxActivity.java`
  starts `PROCESS_TEXT` with `startActivityForResult` and answers through
  `onActivityResult` into a `native` callback. The file picker is a second
  request code on that same path, not new machinery.

## Part A: the file input

### A.0 Housekeeping

- Push `e2cddc4`, `4afcd8f`, `09f2629` and this plan **when the owner says**
  (the branch is three ahead of `origin/build/v0.9`).
- Roadmap: audio and video move into v0.9 as this plan's parts B and C.

### A.1 The `File` value (all platforms, no UI)

- `File` as a built-in type in the checker's builtins interface, beside the
  dialogs' declarations: `name: string`, `size: number`, `type: string`,
  `url: string`, `path: string?` (desktop only, `none` elsewhere), and
  `async fn text() -> string`.
- At run time a `File` is a handle (an id) into a table the shell owns, so a
  value copied through signals and props stays small, and the table entry
  goes when the last handle does (that is when a web object URL is revoked).
- `type` from the extension through one table, the same table `accept`
  uses in A.4, so the two never disagree.
- Tests: checker cases (`photo.name` types, `photo.nope` is an error,
  `photo.text()` without `await` is the usual async error); a runtime test
  that builds a `File` from a temp file.

### A.2 `url` that `<image :src>` can show

- The image reader learns `file:` (desktop): read the path, no project
  resolution.
- `blob:` (web) and `content://` (Android) are added in A.7 and A.9, each
  into this same reader, so `rux-paint` never knows about schemes.
- The cache keys by `src`, so two picks of the same file at the same path
  must not show the old image: the `url` carries the pick's id
  (`file:///…?rux=7` or similar), which the reader strips.
- Test: `<image :src="f.url">` paints the picked image (a `rux-layout`
  paint test with a `file:` URL), and the old-image case above.

### A.3 The element and its look (desktop)

- The parser and `rux check` accept `type="file"`; the vocabulary
  (`vocab.rs`, the extension's `vocabulary.json`) lists it and its attributes.
- Drawn as a browser draws it: a button, then the name, "No file chosen"
  before a pick, "3 files" for `multiple` (as Chrome says it). The button is
  `::file-selector-button`, which leaves `NOTHING_TO_ACT_ON` and becomes a
  real pseudo-element in `rux-style`, so Tailwind's `file:` utilities apply.
- Accessibility: one node, role button, label "Choose file" plus the chosen
  name.
- Seen in the window in light and dark, and with a Tailwind `file:` probe.

### A.4 Picking (desktop)

- A tap (or Enter/Space when focused) opens rfd, as `pickFile` does; while it
  is open, a second tap does nothing.
- `accept` to rfd filters: extensions as written, media types and
  `image/*`, `video/*`, `audio/*` through the A.1 table.
- `r-model` writes the `File`, or a list with `multiple`; then `@change`
  fires. Nothing chosen fires `@cancel` and leaves the value as it was (a
  browser keeps the old file too).
- `required` joins the form's validity, `:invalid`, `:user-invalid`;
  `disabled` and `name` as other inputs.
- `text()` reads on a thread (`std::fs`), decodes as UTF-8 with
  replacement, as a browser's `File.text()` does.
- Driven by hand (rfd cannot be scripted): pick, cancel, pick again,
  `multiple`, a filter, a non-UTF-8 file.

### A.5 Desktop close-out

- `docs/02-spec.md`, `03-guide.md`, `05-as-built.md` and the site reference
  for the element and `File`.
- `docs/09-author-notes.md`: on the web, picking only works from a tap
  (written now, true from A.7).

### A.6 `pickFile` returns `File` (decision 3)

- `pickFile` → `File?`, `pickFiles` → `[File]`. `pickSaveFile` stays a path:
  a save dialog names a file that does not exist yet.
- A field report note to Mava Studio: `p` becomes `p.path`. The checker
  points at every place that breaks, which is the reason this is safe.

### A.7 The web

- **Spike first:** confirm the browser opens a picker from where the web
  shell handles a tap. A picker needs transient user activation; if the
  shell handles taps inside the DOM event, it simply works; if it handles
  them a frame later, Chrome's activation window still covers it, but
  Firefox and Safari must be tried. The answer decides whether the hidden
  `<input>`'s `click()` is called from the shell's DOM listener directly.
- A hidden DOM `<input type="file">` carrying `accept`, `multiple` and
  `capture` as written; its `change` and `cancel` events become A.4's.
- `File` from the DOM `File`: `text()` is its promise turned into an
  `await`. `url` is `URL.createObjectURL`, revoked when the handle goes.
- The image reader learns `blob:`: on a pick the shell reads the bytes once
  (`arrayBuffer()`) into the table, since the painter's reader is
  synchronous and a fetch is not.
- Driven in Edge and Brave, then with CDP touch emulation for `capture`'s
  attribute (the camera itself only on a phone browser).

### A.8 Android: picking

- `ACTION_OPEN_DOCUMENT` (`CATEGORY_OPENABLE`, `EXTRA_MIME_TYPES` from
  `accept`, `EXTRA_ALLOW_MULTIPLE`) on a new request code beside
  `REQUEST_PROCESS_TEXT`; the result URIs go back through a new `native`
  callback, nothing chosen as `@cancel`.
- `name` and `size` from `OpenableColumns`, `type` from `getType`.
- `text()` and the image reader both read through
  `ContentResolver.openInputStream` over JNI, on a thread.
- **The activity can die while the picker is open** (low memory, or
  "don't keep activities"). Phase 7's restore brings the app back, and the
  pending pick must come back with it: which input asked is saved state, and
  the result is delivered after restore. Driven with "don't keep
  activities" on, as phase 7 was.
- Remember: the shim is `include_str!`ed, so the CLI is rebuilt after every
  Java change.

### A.9 Android: `content://` as a `url`

- The image reader resolves `content://` through the same JNI read.
- Persistable permission is not taken: a picked URI is good for this run,
  which is what a browser gives too. Written in the docs.

### A.10 Android: `capture` (decision 6)

- `image/*` → `ACTION_IMAGE_CAPTURE`, `video/*` → `ACTION_VIDEO_CAPTURE`,
  `audio/*` → `RECORD_SOUND_ACTION`, writing into a file in the app's cache
  directory through a `FileProvider`. That needs a `<provider>` in the
  generated manifest and a paths XML in the resources: both go through the
  existing two-call aapt2 pipeline (mind the `-R` overlay trap).
- No `CAMERA` permission is declared, so none is needed: Android only
  demands it from an app that declares it.
- Many phones have no app for `RECORD_SOUND_ACTION`; then the input falls
  back to the document picker with `audio/*`, as Chrome on Android does.
- Driven on the emulator (its camera is a test scene).

### A.11 `pickFile` / `pickFiles` on the web and Android

- The same plumbing, called from a script. On the web, only from a tap
  handler before any `await`; otherwise an `Error` of kind `"gesture"`
  that says so, rather than a picker that silently never opens.

## Part B: audio

### B.0 Choices to confirm with the owner

- **Desktop and Android: `symphonia` to decode and `cpal` to play**, both
  pure Rust, one code path for both (cpal reaches Android through AAudio).
  Formats: WAV, MP3, AAC in MP4, FLAC, Ogg Vorbis. **Not Opus** (symphonia
  has no Opus decoder); a WebM or `.opus` file is an `error` event.
- **The web: the browser's own audio** (an `HTMLAudioElement` per sound,
  never shown), which plays every format the browser does.
- Sources: a project path (through the runtime's source reader, so an
  embedded build plays its own files), a `File.url`. `https:` waits for a
  networking story, as uploads do.

### B.1 `loadSound` (script, no element)

- `let s = await loadSound(src)` returns a `Sound`: `play()`, `pause()`,
  `stop()`, `seek(seconds)`, and `volume`, `loop`, `duration`,
  `currentTime`, `playing` as reactive fields, so a `{{ s.currentTime }}`
  updates as it plays.
- One output stream per app, mixing every playing sound, opened on the first
  `play()` and not before (no audio device touched by an app that makes no
  sound).
- A short sound (a tap's click) is decoded whole; a long one streams from
  its source, so a song is not held in memory.
- Tests: decode each format from a fixture; a mixer test that sums two
  sounds without a device. Heard by hand on the desktop.

### B.2 The app lifecycle

- Android: audio focus requested on `play()` and given back on pause, so a
  call or another app's music pauses Rux's; everything pauses on suspend
  and does not resume by itself.
- Desktop: a lost output device (headphones pulled) moves to the new default.
- **Decided while building (2026-10-24), for the owner to confirm:** a short
  sound (under 10 s, the line between decoded whole and streamed) asks only
  for `AUDIOFOCUS_GAIN_TRANSIENT_MAY_DUCK`, so a tap's click lowers the
  person's music for its moment instead of stopping it; a long one asks for
  `AUDIOFOCUS_GAIN`. The focus is also given back when the last sound ends
  by itself, and after it is lost, since nothing is then playing.
- Background playback (sound with the app away) is **not** in this plan:
  it needs an Android foreground service and is its own proposal.

### B.3 `<audio>`

- `src`, `controls`, `autoplay`, `loop`, `muted`, `preload`, as HTML.
- Without `controls` it is a box of no size, as in a browser. With them,
  Rux draws them: play/pause, the time, a seek bar (the existing slider's
  drawing and dragging), and mute. Icons from the Tabler set.
- Events: `@play`, `@pause`, `@ended`, `@timeupdate`, `@error`. Script reach
  to `play()` and `pause()` through `query()`, as other elements.
- `autoplay` follows the browser's rule on every platform: allowed when
  `muted`, otherwise after the person's first interaction. One rule
  everywhere beats a desktop app that works and a web build that does not.
- Accessibility: the controls as a group of a button, a slider and a button.
- **Built 2026-10-26 (desktop driven; the web and Android are B.4).** As
  built: the element's sound is the shell's, a `Sound` kept under the
  element's key and loaded through `loadSound`, so the element is what a
  script could have built; its events are found by looking at the sound
  after every wake, so a play from anywhere fires them. Decided while
  building, **for the owner to confirm**: `muted` is the volume at nothing
  (the mute button's state, and on the web the element's `muted` flag too,
  which is what lets a muted `autoplay` play there); an `autoplay` that waits
  does not start at the first tap, as in a browser; the controls are 300 by
  40 unless CSS says otherwise, with no background of their own; a sound of
  unknown length only pauses at its end, as there is no end flag both
  platforms share.

### B.4 Audio on the web and Android, driven

- Web in Edge and Brave; Android on the emulator, including focus loss
  (an incoming call from the emulator's controls) and suspend.
- **Driven 2026-10-26** (`docs/08-user-tests.md`). Everything passed but
  two things the driving found:
  - **Fixed:** on Android a muted sound (`<audio muted autoplay>`, or one at
    volume 0) took the audio focus, so a sound nobody could hear would pause
    the person's music. The focus now follows what can be heard
    (`rux_audio::audible`): none at volume 0, taken back on unmuting.
  - **For the owner:** B.3's rule ("`autoplay` allowed when `muted`") does
    not hold for audio in Chromium. Edge and Brave refuse `play()` on any
    audio element before a press, muted or not, attached to the page or not:
    Chromium's muted allowance is for `<video>` only. So today a muted
    `autoplay` plays at once on the desktop and Android and never in those
    browsers. Options: (a) keep it, and on the web start a muted `autoplay`
    at the first press, which is silent so nobody hears the difference;
    (b) one rule everywhere, Chromium's: `autoplay` only after a press,
    muted or not; (c) leave it as it is, written down.

## Part C: video

### C.0 Spike: one frame path, measured

Before any element, prove frames can reach the screen fast enough:

- Decode on a thread to RGBA, upload each frame as a vello image, paint it.
  Measure 720p and 1080p at 30 and 60 fps on the dev machine and the
  emulator, reported as real times per frame.
- If the per-frame upload is the cost, the next step is importing the
  decoder's texture into wgpu directly; that is per platform and is only
  built where the measurement says it is needed.
- **A janky video is worse than none** (the roadmap's own words), so the
  spike's numbers decide whether C goes ahead in this milestone. They go to
  the owner before C.1.

**Measured 2026-10-26** (the spike is branch `spike/c0-frames`, not merged).
A thread stands in for the decoder, writing synthetic RGBA frames at the
video's rate; the shell draws the newest over the page every frame, scaled
to fit, by two roads: **blob** (a new vello image per video frame, uploaded
by vello into its atlas) and **texture** (one texture registered with
`Renderer::register_texture`, each frame written with `queue.write_texture`,
copied into the atlas on the GPU). Five-second windows, medians with p95 in
brackets, real times per frame.

*Dev machine*: Ryzen 5 PRO 4650U, its Radeon, a 60 Hz display, release
build, Windows.

| | page alone | 720p30 tex | 720p60 tex | 1080p30 tex | 1080p60 tex | 1080p60 blob |
|---|---|---|---|---|---|---|
| presents per second | 60 | 60 | 60 | 60 | 60 | 60 |
| video frames never shown | | 0 of 151 | 0 of 301 | 1 of 151 | 1 of 301 | 0 of 301 (64 of 301 with `sync`) |
| writing the frame in (CPU) | | 0.59 ms (0.69) | 0.72 (0.87) | 1.30 (2.01) | 1.78 (2.02) | inside render |
| vello render and submit (CPU) | 1.00 (1.33) | 0.96 (1.36) | 1.03 (1.48) | 1.01 (1.44) | 1.15 (1.58) | 1.98 (4.22) |
| until the GPU is done (`sync`) | 1.88 (2.58) | 2.38 (2.98) | 2.65 (3.10) | 3.28 (4.01) | 3.75 (4.29) | 4.41 (7.20) |
| time between presents | 16.6 (17.5) | 16.7 (17.5) | 16.6 (17.5) | 16.7 (17.7) | 16.6 (17.5) | 16.7 (19.1) |

So on the desktop **the texture road holds 1080p60 with room to spare**:
about 2 ms of CPU to write a frame in and about 2 ms more GPU than the page
alone, against a 16.7 ms frame. The blob road is worse in every column and
drops frames once the CPU waits on the GPU, so it is not the road.

*Emulator* (`rux_test`, x86_64, Android's GL translator on the same
Radeon; optimised build): **no video is possible there, and that says
nothing about a phone.** The page alone presents only 30 times a second
(vello's render takes 28 ms), and writing one frame in takes 29 ms at 720p
and 60 ms at 1080p (the emulator's pipe to the host GPU moves about
130 MB/s). Every case falls to 12 to 20 presents a second and shows
between a quarter (1080p60) and three fifths (720p30) of the video's frames,
by either road. A phone shares memory
between CPU and GPU and has no such pipe, but **there is no phone to
measure on**.

What it means for C, for the owner to decide:

- Windows, through the texture road, can go ahead: the per-frame upload is
  not the cost, so importing a decoder's own texture is not needed there.
- Android is unmeasured. The emulator cannot answer, and on Android the
  plan's next step (MediaCodec decoding into a hardware buffer imported
  into wgpu, no CPU copy) would have to be built blind and judged on the
  emulator, where even the page alone does not reach 60. Measuring needs a
  phone.
- The web was not part of C.0; its frames go to the GPU by
  `copyExternalImageToTexture` and never through the CPU.

### C.1 Decoders, one per platform

- **Windows: Media Foundation** (through the `windows` crate), which decodes
  H.264, HEVC (where installed), VP9 and AV1 (with the store extensions) in
  hardware, at no binary size.
- **Android: `MediaCodec`** with `MediaExtractor`, over the NDK.
- **Web: the browser's `<video>`**, never shown, its frames copied into the
  canvas's WebGPU texture with `copyExternalImageToTexture`, which accepts a
  video element directly.
- **Linux and macOS: not in this plan.** Linux has no decoder every machine
  has (GStreamer or FFmpeg are both large, separately installed, and a
  choice for the owner); macOS is AVFoundation and wants a Mac to drive.
  On both, `<video>` shows its poster and fires `@error` of kind
  `"unsupported"`.

### C.2 `<video>`

- `src`, `poster`, `controls`, `autoplay`, `loop`, `muted`, `playsinline`
  (accepted, and what Rux always does), sized by the video's own size until
  CSS says otherwise; `object-fit` and `object-position` as for `<image>`.
- The same controls as `<audio>` over the picture, plus fullscreen, hidden
  after a few idle seconds and shown on a tap.
- The same events and script reach as `<audio>`, plus `@loadedmetadata`
  with `videoWidth` and `videoHeight`.

### C.3 Sound and sync

- The video's sound plays through B's mixer, and the audio clock is the
  master: a frame is shown when its time comes on that clock, and dropped if
  it is late, as every player does. A video with no sound runs on the
  frame clock.
- Off screen (scrolled away, or under `content-visibility`) it keeps playing
  its sound and stops uploading frames.

### C.4 Driven

- Desktop with H.264 and VP9 files, a long file seeking, a picked
  `File.url`; the web in Edge and Brave; Android on the emulator, with
  suspend and resume. Frame times recorded against C.0's.

## Questions the plan raised (decided 2026-10-23)

The owner approved each as written: symphonia + cpal with Opus left out,
Rux-drawn controls, video on Windows, the web and Android with Linux left
until someone asks for it (no GStreamer or FFmpeg), and video in v0.9 only
if C.0's numbers allow it.

1. **symphonia + cpal for audio** (B.0), and Opus left out?
2. **Rux-drawn controls** for `<audio>` and `<video>`, styled like the file
   button, rather than leaving players to the app?
3. **Video's platforms**: Windows, the web and Android now, Linux and macOS
   later? And for Linux, GStreamer, FFmpeg, or nothing until asked?
4. **Video in v0.9 at all**, which depends on C.0's numbers.

## Order and size

| Part | Steps | Roughly |
|---|---|---|
| A, desktop | A.1-A.6 | the largest unknown is the `File` handle in the checker and runtime |
| A, web | A.7 | small after the spike |
| A, Android | A.8-A.10 | the Java and the manifest, driven on the emulator |
| A, scripts | A.11 | small |
| B | B.1-B.4 | new crates, but one code path for two platforms |
| C | C.0-C.4 | the largest by far; gated on C.0 |
