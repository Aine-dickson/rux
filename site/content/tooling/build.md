+++
title = "rux build"
description = "Turn a project into one executable you can hand to someone. Reads rux.toml, embeds the documents, and writes to dist/."
weight = 5
+++

`rux build` turns a project into something you can give to another person.

```bash
rux build                      # a dev build: reads the project from disk
rux build --release            # embeds the documents into one executable
rux build --target desktop     # the default
rux build --target android     # an APK, in dist/, ready to install
```

The result lands in `dist/`, as a single executable named after the app, or as
an APK for Android.

## Android

```bash
rux build --target android     # writes dist/<app>.apk
rux run --device               # builds it, installs it, starts it
```

No Gradle and no Android Studio, at any point. What the build actually runs is
a handful of command-line tools that come with the SDK and the JDK: `javac` and
`d8` build the one Java class an app carries, `aapt2` writes the base APK from a
generated manifest, the native library and that class are added to it,
`zipalign` aligns it and `apksigner` signs it. A debug keystore is generated
once, at `~/.rux/debug.keystore`, and reused for every project on the machine,
which is what lets a reinstall replace a previous build instead of being
refused.

### The one Java class

A Rux app contains exactly one class, and Rux writes it. It exists because a
plain `NativeActivity` cannot answer questions that are asked as method
overrides, which native code has no way to provide: how much of the display
belongs to the status bar and the gesture bar, and what an input method should
attach to. It depends on nothing but the Android platform, so there is no AAR,
no AndroidX and no resources to merge, which is the step that would drag Gradle
back in.

That class is why safe areas reach your stylesheet and why a soft keyboard can
compose text. It is compiled from source that ships with Rux, so you can read
it, and it is the same class in every Rux app.

Nothing about it is yours to maintain, and nothing about it changes what you
write. It is mentioned here because an APK that says `android:hasCode="true"`
usually means a Java project, and this one does not.

`rux doctor` says what an Android build needs, what is here, and the one command
that installs whatever is not. Start there; see [Android](@/tooling/android.md).

A few things are true of an Android build today and are worth knowing before you
meet them:

- **The documents are embedded whether or not you pass `--release`.** There is
  no filesystem inside an APK to read them back from, so a dev build for Android
  does not hot reload yet.
- **Safe areas are opt-in, and a phone is where that first shows.** A desktop
  window has no unsafe edges, so `env(safe-area-inset-*)` is zero there and a
  layout that never mentions it looks correct. On a device the same layout draws
  under the status bar. Pad the bars that need it, as
  `examples/safe-area.rux` does.
- **A soft keyboard works, composition included.** The one Java class provides
  the input connection an input method attaches to, so typing, backspace,
  autocorrect and suggestions all behave as they do in any other Android app.
- **A dev build produces one ABI and a release build produces all four.** Four
  compiles of the same code is four times the wait, and three of those artifacts
  are for machines you are not about to run it on. A release cannot make that
  trade: an APK carrying one ABI installs on a fraction of the devices it claims
  to support, and Android does not explain why to the person holding the phone.
- **`rux run --device` builds for whatever is plugged in.** It asks the device
  what it runs and compiles that one ABI, so an emulator gets `x86_64` and a
  phone gets `arm64-v8a` without you choosing.

The four are `x86_64`, `arm64-v8a`, `armeabi-v7a` and `x86`. A release build
needs a Rust target installed for each, and says which are missing before it
starts compiling rather than partway through:

```bash
rustup target add aarch64-linux-android armv7-linux-androideabi i686-linux-android
```

## Signing a release

Every APK is signed, because Android refuses to install one that is not. Without
a `[signing]` block Rux uses a debug key it generates once and shares between
your projects, which installs anywhere and is accepted by no store. A release
build says so rather than letting you find out later.

To sign with your own key, name it:

```toml
[signing]
keystore = "release.jks"
alias = "upload"
```

The path is relative to `rux.toml` unless it is absolute.

**The passwords are not in there, and will not be accepted there.** `rux.toml`
is a file you commit, and a keystore password committed beside the keystore it
opens is the same as having no password. They come from the environment:

```bash
export RUX_KEYSTORE_PASSWORD=...   # the keystore's password
export RUX_KEY_PASSWORD=...        # the key's own, if it differs
```

Writing `password` into `[signing]` is an error that says where it goes instead,
rather than a key quietly read from a file you are about to push.

A keystore that is not where the manifest says, or a password that was never
exported, is reported before anything compiles, not after sixteen minutes.

If you have no keystore yet, `keytool` comes with the JDK you already installed:

```bash
keytool -genkeypair -keystore release.jks -alias upload   -keyalg RSA -keysize 2048 -validity 9000
```

Keep it, and keep it backed up. An app updated with a different key is not an
update: every store treats it as a different app, and there is no way back.

## What a build costs

Worth knowing before you wait for one. Measured on an ordinary laptop, for the
project `rux new` writes:

| Build | Time |
|---|---|
| First Android build, one ABI | about 3 minutes |
| Rebuild after editing a document | 10 to 30 seconds |
| Release, all four ABIs, cold | about 16 minutes |

**The slow part is the first build, not the loop.** Rux sits on a GPU renderer
and real text shaping, which is roughly 250 crates of dependencies, and those
compile once per ABI per project. After that only the small generated wrapper
recompiles, which is why editing a document and seeing it on the device is tens
of seconds rather than minutes.

Nothing is shared between ABIs: a target triple is a separate compilation all
the way down, so four ABIs is close to four times one. That is the whole reason
a dev build produces one and only a release produces four.

## It needs a `rux.toml`

A build needs to know what the app is called and what an operating system
should file it under, and neither of those can be guessed from a document. So
`rux build` reads a manifest, found here or in any parent directory, the same
way `rux run` finds an entry point.

```toml
[app]
name = "Task List"
id = "dev.example.tasks"
version = "0.1.0"
```

`name` is for a person to read, and the executable is named after it in a
safer spelling: `Task List` builds `task-list`.

`id` is reverse-DNS, and it is the one field worth getting right the first
time. It is what an operating system files the app under, so changing it later
produces a different app rather than an update. Android refuses an id with no
dot in it, and so does `rux build`.

`version` is the app's own version, which has nothing to do with the version
of Rux that built it.

`entry` is optional. Without it the build opens the same `app.rux` or
`index.rux` that `rux run` would, so a project following the convention writes
nothing.

`[signing]` names the key a release is signed with; see below. Icons and splash
screens have manifest keys waiting for them, and each one lands in the release
that makes it do something rather than ahead of it.

## Dev builds and release builds differ in one way

A **dev build** points the executable at the project directory. Documents are
read from disk as the app runs, so hot reload works exactly as it does under
`rux run`. This is the build to use while working.

A **release build** embeds every document into the executable. Nothing is read
from disk, the artifact carries its own contents, and what ships cannot drift
from what was tested. Move it to another machine with no Rux and no project
directory and it still runs.

**Android is the exception, and always embeds.** There is no filesystem behind
an APK to read documents back from, so a dev build for Android carries them too
and does not hot reload yet.

## What it needs installed

`rux build` writes a small Rust crate and compiles it, so it needs a Rust
toolchain. If `cargo` is not on the path it says so and points at
[rustup.rs](https://rustup.rs).

The generated crate lives in `.rux-build/`, inside the project. It is
disposable and is rewritten on every build, so there is nothing in it worth
editing and nothing in it worth committing.

## What embeds

Everything the project is made of: the entry document, every component, every
stylesheet, and every asset, images included. A release build is walked from
the project directory rather than traced through imports, so an asset named
only in CSS is carried too.

The one thing not carried is the manifest. `rux.toml` describes the build; it
is not part of the app.
