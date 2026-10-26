+++
title = "Audio"
description = "A sound as an element: drawn controls, autoplay as a browser allows it, and its events."
weight = 17
+++

<!-- GENERATED FROM docs/05-as-built.md BY site/sync-docs.sh. DO NOT EDIT HERE. -->

```rux
<audio src="sounds/song.mp3" controls @ended="next()" />
```
The sound is read as `loadSound` reads one (see Sounds in the script
reference): a path from the project, built into a release, or a picked
`File`'s `url`, in WAV, MP3, AAC, FLAC or Ogg Vorbis (and anything a browser
plays, in a browser). `:src` binds an expression; a new value is a new sound,
from its start.

**Without `controls` it is nothing on screen**, as in a browser: no box, and
the sound still plays. **With `controls`** Rux draws them: play/pause, the
time (`1:05 / 3:40`), a seek bar to tap or drag, and mute, Tabler's icons in
the element's `color` and the bar in its `accent-color`. 300 by 40 unless CSS
says otherwise, and laid out as a flex row, so `gap` and `padding` on the
element space them. To the accessibility tree they are a group of a button, a
slider and a button.

| Attribute | |
|---|---|
| `controls` | draw the controls |
| `autoplay` | play once loaded, **only when `muted` or once the person has pressed something in the app** |
| `loop` | start again at the end, with no `@ended` |
| `muted` | the volume at nothing; the mute button turns it back on |
| `preload` | `none` reads nothing until the first play; `auto` and `metadata` (the default) read at once |

All but `preload` have a bound form (`:muted="quiet"`).

**`autoplay` follows the browser's rule on every platform.** A browser plays a
sound by itself only when it is muted, or after the person has used the page;
a desktop app that plays and the same app's web build that does not would be
the worse surprise, so Rux has the one rule everywhere. An `autoplay` that
waits says so, and does not start later on its own: the first tap is the
place to play it.

**Events**: `@play` and `@pause` as it starts and stops, whatever started it
(its controls, `play()`, `autoplay`, a script); `@ended` after the `@pause` at
its end (not with `loop`); `@timeupdate` about four times a second while it
plays and on a seek, each with `event.currentTime` and `event.duration`
(`none` when not known); `@error` when it could not be loaded, with
`event.kind` (`"io"`, `"unsupported"`, `"decode"`) and `event.message`.
`query("audio")[0].play()` and `.pause()` reach it from a handler, as
`focus()` reaches an input.

A sound of unknown length has no end Rux can see, so it only pauses there.
What is written between `<audio>` and `</audio>` is never shown, as a
browser shows it only when it has no audio at all, and Rux says so.
