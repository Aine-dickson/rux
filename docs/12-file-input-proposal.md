# Proposal: `<input type="file">` and picking files everywhere

Status: **proposed 2026-10-23, for the owner to decide before it is built.**
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

- `type="file"` with `r-model` holding the chosen file, or `none`.
  `multiple` makes it a list. `accept` takes what HTML's does: extensions
  (`.png`) and media types (`image/*`, `application/pdf`). `disabled`,
  `required` and `@change` as other inputs have them.
- **Drawn by Rux** as a browser draws one: a "Choose file" button and the
  chosen name beside it ("No file chosen" before). The button is
  `::file-selector-button`, which moves out of the CSS that passes quietly
  and becomes real; the rest is styled as the input. A tap opens the
  platform's own picker.
- **A `File` value**: `name`, `size` (bytes), `type` (media type, `""` when
  unknown), `url` (something `<image :src>` can show), and `await
  file.text()` to read it as text. See decision 2 for bytes.

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

## Phases

1. **The `File` value and the desktop input.** The element, its look,
   `accept`, `multiple`, `r-model`, `@change`, the accessibility role
   (button) and the `File` methods, on the desktop, where rfd already works.
   Tests: the build and the checker headless; the dialog itself is driven by
   hand, as rfd cannot be scripted.
2. **The web.** The hidden input, the gesture, the DOM `File`, `url` as an
   object URL (revoked when the value goes).
3. **Android.** The intent, the activity result, `ContentResolver` reads,
   `url` from the content URI. Driven on the emulator; a phone when there is
   one.
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
   need.
