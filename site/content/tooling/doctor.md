+++
title = "rux doctor"
description = "Check this machine for the Android toolchain: what is here, where it was looked for, and the one command that installs whatever is not."
weight = 8
+++

```bash
rux doctor
```

Nothing is built. It answers one question: could an Android build happen here,
and if not, what exactly is missing.

```text
rux: the Android toolchain

  found    Android SDK          $ANDROID_HOME
                                /home/you/Android/Sdk
  found    command-line tools   sdkmanager
                                /home/you/Android/Sdk/cmdline-tools/latest/bin/sdkmanager
  MISSING  NDK                  its clang is the linker for every Android build
                                looked in /home/you/Android/Sdk/ndk
                                fix: sdkmanager "ndk;28.2.13676358"

rux: 1 of 8 missing. An Android build needs all of them.
```

Exit code 0 when everything is there, 1 when something is not, so a setup
script can use it and not only a person reading it.

What to install, and in what order: [setting up for
Android](@/tooling/android.md).

## What it looks for

| | Why |
|---|---|
| Android SDK | Everything else is a path inside it. `$ANDROID_HOME`, then `$ANDROID_SDK_ROOT`, then the usual place for the platform |
| command-line tools | `sdkmanager`, which installs the rest. Nothing else needs it |
| build-tools | `aapt2` packs the resources, `zipalign` aligns the APK, `apksigner` signs it |
| platform | `android.jar`, which the manifest and the resources are compiled against |
| platform-tools | `adb` installs and runs the app on a device or an emulator |
| NDK | Its `clang` is the linker for every Android build |
| JDK | `apksigner` and the SDK's own tools are java programs |
| Rust target | The Rust half of the app is cross-compiled to it |

Android Studio is not on that list and never will be. Neither is Gradle. The
whole of what a Rux user installs by hand is the command-line tools, a
platform, the build tools and an NDK, and `rux doctor` exists so that the
day you are missing one of them is not the day you learn what any of them are.

## Why the diagnostic is the feature

"Mobile without Android Studio" is kept or broken entirely by what happens when
something is absent. `error: aapt2 not found` sends someone to a search engine.
A line that names **where it looked** and **the one command that fixes it** does
not, and the difference is the whole promise.

So every finding carries both. A tool that is installed somewhere else reads
differently from one that is not installed at all, which are the same message
in most build systems and are not the same problem.

There is a third answer besides found and missing: **present and unusable**. A
platform older than the API level Rux targets is installed, correct, and no use,
and the fix is a different command from the one for having none at all.

## The floor

Rux targets **API 26** (Android 8.0) and up. The constraint is Vulkan driver
dependability under `wgpu` rather than taste, and it is deliberately biased
high: lowering a floor later gains users in one line, while raising one drops
devices from under people who have already shipped.

`rux build --target android` does not exist yet. This does, because the
toolchain can be got ready before there is anything to build with it.
