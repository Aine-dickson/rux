# Proposal: `<input type="file">` and picking files everywhere

Status: **proposed 2026-10-23; every decision below DECIDED as proposed by the
owner on 2026-10-23, and decision 8 answered as "now, and video too".** The
build plan is [13-file-input-and-media-plan.md](13-file-input-and-media-plan.md).
The next session builds it, in the order of the phases below, once the
decisions at the end have answers.

## Why

A Rux app on the web or on Android cannot let anyone choose a file today: no
attachment, no photo, no import. `pickFile`, `pickFiles` and `pickSaveFile`
work on the desktop only (rfd, `crates/rux-shell/src/dialogs.rs`), and throw
an `unsupported` error elsewhere. And `<input type="file">`, the way every web
author expects to do it, is refused as an unknown input type.

Two more gaps sit under it:

- **A picked file cannot be read from a script.** `pickFile` returns a path,
  and Rux has no function that reads one, so even on the desktop the path is
  only useful to a native module.
- **The web and Android have no path to return.** A browser hands a page the
  file's contents, never where it lives; Android hands a `content://`
  address that only its `ContentResolver` can open. So the answer to "what
  did the person pick" has to be something other than a path on two of the
  three platforms.

## What it would look like to an author

```rux
<template>
  <screen>
    <input type="file" r-model="photo" accept="image/*" />
    <text r-if="photo">{{ photo.name }}, {{ photo.size }} bytes</text>
    <image r-if="photo" :src="photo.url" />
    <button @tap="send()">Send</button>
  </screen>
</template>

<script>
let photo = signal(none);

async fn send() {
  let text = await photo.text();
  // …
}
</script>
```

- `type="file"` with `r-model` holding the chosen file, or `none`, and the
  attributes below.
- **Drawn by Rux** as a browser draws one: a "Choose file" button and the
  chosen name beside it ("No file chosen" before). The button is
  `::file-selector-button`, which moves out of the CSS that passes quietly
  and becomes real; the rest is styled as the input. A tap opens the
  platform's own picker.
- **A `File` value**: `name`, `size` (bytes), `type` (media type, `""` when
  unknown), `url` (something `<image :src>` can show), and `await
  file.text()` to read it as text. See decision 2 for bytes.

## Its attributes and events

HTML's, so a web author writes what they know. `allow` is not one (it is an
`<iframe>`'s); `accept` is the one that says which files.

| Attribute | What it does | In Rux |
|---|---|---|
| `accept` | Which files the picker offers: extensions (`.png`), media types (`application/pdf`) and families (`image/*`, `video/*`, `audio/*`), comma-separated | Desktop pickers filter by extension, so media types go through a small table of the common ones (`image/*` → png, jpg, jpeg, gif, webp, …). The web passes it to the browser as written; Android passes the media types to the intent. Only a hint, as in HTML: a person can still pick another file on some platforms, and `File.type` says what was picked |
| `multiple` | More than one file | `r-model` holds a list, empty when nothing is chosen |
| `capture` | On a phone, open the camera or a recorder rather than the file picker: `environment` (back camera) or `user` (front), with `accept` saying photo (`image/*`), video (`video/*`) or sound (`audio/*`) | Android: `ACTION_IMAGE_CAPTURE`, `ACTION_VIDEO_CAPTURE` or `RECORD_SOUND_ACTION` into a file Rux owns, handed back as a `File`. The web passes it to the browser, which honours it on phones. The desktop ignores it and opens the picker, as desktop browsers do |
| `required` | Nothing chosen is invalid | Joins the form's validity, `:invalid` and `:user-invalid`, as other fields |
| `disabled`, `:disabled` | Cannot be opened | As other inputs |
| `name` | The field's name in a form | As other inputs |
| `webkitdirectory` | Pick a whole folder | **Not proposed.** Not standard, though every browser has it, and Android's folder picking is another intent with another value. Later, if an app needs it |

| Event | When | Notes |
|---|---|---|
| `@change` | A file (or files) was chosen, and `r-model` already holds it | After the binding is written, as other inputs do |
| `@cancel` | The picker closed with nothing chosen | Newer HTML. Lets an app tell "nothing chosen" from "still choosing", which a spinner needs |

## How each platform does it

| | Opening the picker | What comes back | Reading it |
|---|---|---|---|
| Desktop | rfd, as `pickFile` already does | a path | `std::fs`, on a thread so the window keeps painting |
| Web | a hidden DOM `<input type="file">`, clicked **inside the tap's own event handler**: browsers open a picker only during a real user gesture | a DOM `File` | `File.text()` / `arrayBuffer()`, a JS promise turned into an `await` |
| Android | an `ACTION_OPEN_DOCUMENT` intent from the Java shim (`RuxActivity.java`), result through `onActivityResult` | a `content://` URI | `ContentResolver.openInputStream` over JNI. No storage permission is needed for files the person picks |

The web's gesture rule decides one thing for all three: the picker opens from
the tap on the input, synchronously, not from a script that runs later. A
`pickFile()` called from an `async fn` after an `await` would be refused by
the browser. So on the web `pickFile` works only when called directly in a
tap handler, and the docs say so.

`pickFile` and `pickFiles` get the same plumbing on the web and Android once
the input has it, which is what makes the return type a decision (3).

## Media: what this starts, and what it does not

The file input is where media enters Rux, and the `File` value is designed
here as what media will play, so audio and video do not need a second idea
of a file later.

**What this proposal gives media:**

- **Getting media in.** A photo, a video or a recording from the gallery or
  the file system, and with `capture`, straight from the phone's own camera or
  recorder. Recording is the platform app's, so Rux records nothing itself.
- **A source a player can open on every platform.** `File.url` is a `file:`
  path on the desktop, an object URL (`blob:`) on the web and a `content://`
  URI on Android: each is exactly what that platform's own player accepts.
  So the rule for this proposal is that **`url` must be playable as well as
  showable**: an `<image :src>` today, an `<audio :src>` or `<video :src>`
  when they exist, and the same value an `https:` address will be.
- **`type`**, the media type, which a player needs to choose a decoder and an
  app needs to tell a video from a photo.

**What it does not give: playback.** That is the roadmap's, and stays split
the way the roadmap splits it:

- **Audio** (roadmap, v0.9 item 8) is an output stream, and probably a script
  call before it is an element, since a sound effect is an action and not a
  box: `let s = await loadSound(file.url); s.play();`, then an `<audio>` with
  controls for a player. It is the smaller of the two: a decoder and an
  output device, the same shape on every platform.
- **Video** (roadmap, v0.11) is decoding plus frames into the wgpu textures
  Rux draws with, through each platform's own decoder (Media Foundation,
  AVFoundation, Android's `MediaCodec`, the browser's `<video>`), and is the
  item most able to eat a milestone alone.

Neither is part of this proposal. What this one owes them is the `File`
value and its `url`, built so that they can take it as it is. A separate
proposal for audio can be written whenever the owner wants it, now or when
the roadmap reaches it.

## Phases

1. **The `File` value and the desktop input.** The element, its look,
   `accept`, `multiple`, `r-model`, `@change`, the accessibility role
   (button) and the `File` methods, on the desktop, where rfd already works.
   Tests: the build and the checker headless; the dialog itself is driven by
   hand, as rfd cannot be scripted.
2. **The web.** The hidden input, the gesture, the DOM `File`, `url` as an
   object URL (revoked when the value goes).
3. **Android.** The intent, the activity result, `ContentResolver` reads,
   `url` from the content URI, and `capture`'s camera and recorder intents.
   Driven on the emulator, whose camera is a test scene; a phone when there
   is one.
4. **`pickFile` / `pickFiles` on the web and Android**, on the same
   plumbing, with whatever decision 3 says they return.

Drag and drop of files onto the input, and `pickSaveFile` beyond the
desktop (a browser can only download, Android uses `ACTION_CREATE_DOCUMENT`),
are later and not in this proposal.

## Decisions for the owner

1. **The value's name and shape.** Proposed: a built-in type `File` with
   `name`, `size`, `type`, `url` and async `text()`. Is `File` the name, and
   is that the set?
2. **Bytes.** Rux has no bytes type (`rux-reactive`'s `Value` has numbers,
   text, booleans, lists, maps and `none`). Reading an image or a zip needs
   one. Options: (a) leave bytes out for now: `text()` and `url` cover text
   files and showing images, and uploads wait for a networking story;
   (b) `bytes()` returning a list of numbers, simple and slow for anything
   big; (c) a real `Bytes` type, which is a language change and so yours.
   Proposed: (a) now, (c) when networking needs it.
3. **What `pickFile` returns.** Today a path (`string?`), which Mava Studio
   uses. Options: (a) keep it, desktop only, and add `File`-returning
   versions that work everywhere; (b) change it to return `File?` (a `File`
   on the desktop also carries its `path`), which breaks code that treats
   the result as text. Proposed: (b), with `file.path` kept for the desktop,
   because two ways to pick a file is the thing worth avoiding. Mava would
   change `p` to `p.path`.
4. **The control's default look.** Proposed: the browser's ("Choose file"
   button, then the name), in the root's colour scheme, so a Tailwind
   `file:` utility styles what it styles in a browser. The alternative is a
   plain button with no name beside it.
5. **Large files.** `text()` reads the whole file into memory. Proposed: no
   limit, a warning in the docs, and streaming left for when there is a
   need. A video is never read this way: a player opens its `url`.
6. **`capture` in the first build?** It is the phone's camera and recorder for
   the price of three intents, and the start of media for an app. Proposed:
   yes, in phase 3 with the rest of Android.
7. **`@cancel`?** Proposed: yes; it is cheap on every platform (the dialog
   returns nothing) and a loading state needs it.
8. **An audio proposal now, or when the roadmap reaches audio?** Nothing here
   waits on it.
