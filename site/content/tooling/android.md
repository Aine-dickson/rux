+++
title = "Setting up for Android"
description = "The four things a Rux user installs by hand to build for Android, and nothing else: no Android Studio, no Gradle."
weight = 9
+++

Rux builds an APK itself. It does not generate a Gradle project, and it does
not want Android Studio installed. What it needs is the four pieces of the
Android SDK that do the actual work, plus a JDK to run two of them.

Run [`rux doctor`](@/tooling/doctor.md) at any point. It says what is here,
where it looked, and the one command that installs whatever is not.

## 1. A JDK

Version 17 or newer. `apksigner` and the SDK's own command-line tools are java
programs. Any build of OpenJDK will do; set `JAVA_HOME` to it.

```bash
java -version      # should say 17 or higher
```

## 2. The command-line tools

This is the one download that is not itself installed by a command, because it
is what installs everything else.

Take **"Command line tools only"** from
[developer.android.com/studio](https://developer.android.com/studio#command-line-tools-only),
which is further down that page than the Android Studio button. Unzip it so the
`bin` directory ends up at:

```text
<sdk>/cmdline-tools/latest/bin
```

`<sdk>` is wherever you want the SDK to live. Then point `ANDROID_HOME` at it
and put `<sdk>/cmdline-tools/latest/bin` on your `PATH`.

The `latest` directory in the middle is not decoration: `sdkmanager` refuses to
run when it is unpacked one level higher, with a message about its own location
that is hard to act on.

## 3. The SDK pieces

```bash
sdkmanager "platform-tools" "platforms;android-35" "build-tools;35.0.0"
sdkmanager "ndk;28.2.13676358"
```

| | What it is for |
|---|---|
| `platform-tools` | `adb`, which installs and runs the app on a device or an emulator |
| `platforms;android-35` | `android.jar`, which the manifest and the resources are compiled against |
| `build-tools;35.0.0` | `aapt2`, `zipalign` and `apksigner`: pack, align, sign |
| `ndk;…` | Its `clang` is the linker for every Android build |

Any recent version of each works; those are known-good ones. `sdkmanager
--list` shows what is available.

## 4. The Rust target

```bash
rustup target add x86_64-linux-android
```

That is the emulator's target, and the emulator is where an app is developed
before there is a phone to put it on. It is enough for `rux run --device`
against an emulator, and `rux run --device` against a real phone installs
whichever target that phone needs.

A **release** build produces all four ABIs, so it needs all four targets:

```bash
rustup target add aarch64-linux-android armv7-linux-androideabi i686-linux-android
```

`rux build --release --target android` says which of them are missing before it
starts compiling, rather than on the third of four builds.

## Then

```bash
rux doctor
```

Everything found, exit code 0, and the machine is ready.

## 5. A phone, when you want one

An emulator is enough to build and run, and most work happens in the desktop
window. This is for when the app needs a real screen and a real finger.

**Turn on USB debugging.** Settings, About phone, tap Build number seven times,
then Developer options, USB debugging. Plug the phone in and accept the
"Allow USB debugging?" prompt, which names this computer:

```bash
adb devices
```

One line with `device` after the serial means everything below will work.
`unauthorized` means the prompt has not been accepted yet: unlock the phone and
look for it.

### When `adb devices` shows nothing at all

The phone is plugged in, debugging is on, and adb lists nothing. **Use wireless
debugging instead of hunting for the cause**, because it avoids the whole class
of problem:

Developer options, Wireless debugging, turn it on, then **Pair device with
pairing code**. That dialog shows an address and a six digit code, and its port
is **not** the one on the screen behind it:

```bash
adb pair 192.168.1.10:41234 123456
```

Once paired, the phone is usually found automatically over mDNS, and
`adb devices` lists it as a long `adb-...._adb-tls-connect._tcp` name. Both
devices have to be on the same network.

The USB cause worth knowing, since it is invisible and common: on Windows, adb
finds devices by a specific driver interface, and a phone that Windows binds to
its generic driver never advertises it. The device works perfectly and is simply
not listed. Vendors outside the inbox driver list, which includes many phones
built on MediaTek, hit this. Installing Google's USB driver is the USB-side fix;
wireless debugging routes around it entirely.

### Then

```bash
rux run --device
```

builds for the ABI that phone actually reports, installs, and starts it. Rux
does not pick between devices, so **stop the emulator if one is running**.

**The installed app does not need any of this.** An APK carries its documents
and its compiled library, so once it is installed the phone can leave the
network, and the app starts from its launcher icon like any other. The
connection is only for installing a new build and for reading `adb logcat`.

**A dev build for Android does not hot reload.** There is no filesystem behind
an APK, so both dev and release builds embed their documents, and changing a
`.rux` file means running `rux run --device` again. That is a few seconds once
the Rust side is warm, because only the generated wrapper is rebuilt.

**Judge anything about feel in a release build.** A debug build on a phone can
be slow enough that Android drops touch events it cannot deliver, which makes
scrolling and flinging look broken when they are not.

## What is deliberately absent

**Android Studio.** It is an IDE, and installing an IDE to produce a file is a
cost Rux does not intend to pass on. Nothing here needs it, including the
emulator, which the command-line tools can install and run on their own.

**Gradle.** Rux generates a wrapper crate, builds it per ABI with the NDK's
clang as the linker, and drives `aapt2`, `zipalign` and `apksigner` directly.
Four command-line tools, Gradle in none of them.

**A device.** Most Rux development happens in the desktop window, and
[`rux run --preview`](@/tooling/run.md) turns that window into a phone-shaped
one with a notch and a home indicator. A physical device is for a short list of
things that only exist on one: GPU performance and frame pacing, gestures
against a real hand, multi-touch, battery.
