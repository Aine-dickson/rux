//! Turning a built shared object into an APK, with four command-line tools.
//!
//! **No Gradle, and no Android Studio.** That is the whole promise of v0.8's
//! packaging half, and this is where it is either kept or broken. The pipeline
//! is: write an `AndroidManifest.xml`, `aapt2 link` it into a base APK, add the
//! native library, `zipalign`, then `apksigner`. Android Studio runs a far
//! larger version of the same four steps.
//!
//! **Signing is not optional, debug builds included.** Android refuses to
//! install an unsigned APK at all, so a keystore is generated on first use and
//! cached. Someone building an app for the first time never thinks about
//! signing, and someone shipping one thinks about it once.
//!
//! Nothing here knows what a Rux document is. It takes a built `.so` and a
//! manifest and produces a file, which keeps the Android-specific half small
//! and separate from the generator that made the crate.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::android::{Toolchain, DEV_ABI, MIN_API};
use crate::manifest::Manifest;

/// The API level the app declares it was built against.
///
/// Distinct from [`MIN_API`], which is the oldest Android that will run it.
/// Google Play requires a recent target, and targeting an old one silently opts
/// an app into compatibility behaviour it never asked for.
const TARGET_API: u32 = 35;

/// Build `out` from the shared object at `built`.
pub fn pack(
    manifest: &Manifest,
    toolchain: &Toolchain,
    built: &Path,
    work: &Path,
    out: &Path,
) -> Result<(), String> {
    if !built.is_file() {
        return Err(format!("the generated crate built nothing at {}", built.display()));
    }

    // Laid out as the APK wants it, because the tool that adds the library
    // takes the path to store it under from the path on disk.
    let staging = work.join("apk");
    let lib_dir = staging.join("lib").join(DEV_ABI.name);
    // Removed rather than written over: a rename in `rux.toml` would otherwise
    // leave the previous `.so` beside the new one, and both would be packed.
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&lib_dir).map_err(|e| format!("creating {}: {e}", lib_dir.display()))?;

    // Debug symbols are most of the size of a Rust shared object and none of
    // them are any use on a device: 339 MB became 51 MB the first time this
    // ran. Stripping is not an optimisation here so much as the difference
    // between an APK that installs over adb in seconds and one that does not.
    let library = lib_dir.join(built.file_name().unwrap_or_default());
    run(Command::new(toolchain.strip()).arg("-o").arg(&library).arg(built), "llvm-strip")?;

    let manifest_xml = staging.join("AndroidManifest.xml");
    std::fs::write(&manifest_xml, android_manifest(manifest))
        .map_err(|e| format!("writing {}: {e}", manifest_xml.display()))?;

    let base = staging.join("base.apk");
    run(
        Command::new(toolchain.aapt2())
            .arg("link")
            .arg("--manifest")
            .arg(&manifest_xml)
            .arg("-I")
            .arg(toolchain.android_jar())
            .args(["--min-sdk-version", &MIN_API.to_string()])
            .args(["--target-sdk-version", &TARGET_API.to_string()])
            .arg("-o")
            .arg(&base),
        "aapt2 link",
    )?;

    // Relative to the staging directory, because that relative path is exactly
    // what the entry is named inside the APK. Run from anywhere else and the
    // library lands at a path Android does not look in.
    //
    // **Built as a string with forward slashes, never with `Path::join`.** A
    // zip entry's separator is `/` on every platform, and `aapt add` stores the
    // name it is handed verbatim. On Windows `Path::join` gives backslashes, so
    // the entry was written as `lib\x86_64\libapp.so`, which is not a directory
    // to Android but a single oddly named file at the root. The APK built,
    // signed and installed, and the activity died on launch with "unable to
    // find native library". Found by running it; nothing earlier could have
    // caught it, because every step before the device was happy.
    let inside = library_entry(&library.file_name().unwrap_or_default().to_string_lossy());
    run(
        Command::new(toolchain.aapt())
            .arg("add")
            .arg("base.apk")
            .arg(&inside)
            .current_dir(&staging),
        "aapt add",
    )?;

    let aligned = staging.join("aligned.apk");
    run(
        Command::new(toolchain.zipalign()).args(["-f", "4"]).arg(&base).arg(&aligned),
        "zipalign",
    )?;

    let keystore = debug_keystore(toolchain)?;
    // Deleted first: apksigner refuses to write over an existing output, and a
    // second build would otherwise fail on the leftovers of the first.
    let _ = std::fs::remove_file(out);
    run(
        Command::new(toolchain.apksigner())
            .arg("sign")
            .arg("--ks")
            .arg(&keystore)
            .args(["--ks-pass", "pass:android"])
            .args(["--key-pass", "pass:android"])
            .arg("--out")
            .arg(out)
            .arg(&aligned),
        "apksigner",
    )?;

    Ok(())
}

/// Where the native library sits inside the APK, as a zip entry name.
///
/// A string rather than a `Path`, and that is the whole point of it existing:
/// zip entries are separated by `/` on every platform, while `Path::join` on
/// Windows gives `\`. The backslash version builds, signs and installs without
/// complaint, then fails at launch with "unable to find native library",
/// because Android is looking inside a `lib/x86_64/` directory that the archive
/// does not have.
fn library_entry(file_name: &str) -> String {
    format!("lib/{}/{file_name}", DEV_ABI.name)
}

/// The debug keystore, generated once and kept.
///
/// Beside the SDK rather than in the project, so that every Rux project on this
/// machine is signed by the same debug key. That is what lets `rux run
/// --device` reinstall over a previous build instead of failing on a signature
/// mismatch, and it is the same reasoning behind Android's own
/// `~/.android/debug.keystore`.
///
/// The password is the constant `android`, which is what every Android debug
/// keystore has used for fifteen years. It is not a secret and it is not
/// protecting anything: a debug key says only "the same machine built both of
/// these". A release build names a real keystore, and that is `rux.toml`'s job
/// when release signing lands.
fn debug_keystore(toolchain: &Toolchain) -> Result<PathBuf, String> {
    let dir = dirs_home().join(".rux");
    let keystore = dir.join("debug.keystore");
    if keystore.is_file() {
        return Ok(keystore);
    }
    std::fs::create_dir_all(&dir).map_err(|e| format!("creating {}: {e}", dir.display()))?;
    println!("rux: generating a debug keystore at {}", keystore.display());
    run(
        Command::new(toolchain.keytool())
            .arg("-genkeypair")
            .arg("-keystore")
            .arg(&keystore)
            .args(["-storepass", "android"])
            .args(["-keypass", "android"])
            .args(["-alias", "androiddebugkey"])
            .args(["-keyalg", "RSA"])
            .args(["-keysize", "2048"])
            .args(["-validity", "10000"])
            .args(["-dname", "CN=Rux Debug, O=Rux, C=US"]),
        "keytool",
    )?;
    Ok(keystore)
}

fn dirs_home() -> PathBuf {
    let key = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    std::env::var_os(key).map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."))
}

/// The `AndroidManifest.xml` for a Rux app.
///
/// Three lines carry the weight:
///
/// - **`android:hasCode="false"`.** There is no Java and no dex file in this
///   APK at all. Without this the platform looks for a class it will not find.
/// - **`android.app.lib_name`.** How `NativeActivity` knows which `.so` to
///   load, given without the `lib` prefix or the `.so` suffix.
/// - **`configChanges`.** Every one of these is a change a native app handles
///   by being told about it. Leaving them out means Android destroys and
///   recreates the activity on a rotation, which for a GPU surface means
///   tearing down the swapchain to redraw the same thing.
fn android_manifest(manifest: &Manifest) -> String {
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<!-- Generated by `rux build`. Edits are overwritten on the next build. -->
<manifest xmlns:android="http://schemas.android.com/apk/res/android"
    package="{id}"
    android:versionCode="1"
    android:versionName="{version}">
    <uses-sdk android:minSdkVersion="{MIN_API}" android:targetSdkVersion="{TARGET_API}" />
    <application
        android:label="{label}"
        android:hasCode="false"
        android:extractNativeLibs="true">
        <activity
            android:name="android.app.NativeActivity"
            android:exported="true"
            android:configChanges="orientation|keyboardHidden|screenSize|screenLayout|density|uiMode">
            <meta-data android:name="android.app.lib_name" android:value="{lib}" />
            <intent-filter>
                <action android:name="android.intent.action.MAIN" />
                <category android:name="android.intent.category.LAUNCHER" />
            </intent-filter>
        </activity>
    </application>
</manifest>
"#,
        id = manifest.id,
        version = manifest.version,
        label = escape(&manifest.name),
        lib = manifest.artifact_stem().replace('-', "_"),
    )
}

/// XML-escape a value going into an attribute.
///
/// An app called `Bob's Tools` would otherwise close the attribute early and
/// produce a manifest that does not parse, which `aapt2` reports as a line and
/// column in a file the author never wrote.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(c),
        }
    }
    out
}

/// Run one tool, and say which one failed in its own words.
///
/// The tool's own output is kept and returned rather than let through to the
/// terminal, so a failure reads as one message about one step instead of a
/// wall of output with the useful line somewhere inside it.
fn run(command: &mut Command, what: &str) -> Result<(), String> {
    let output = command
        .output()
        .map_err(|e| format!("could not run {what}: {e}\n\nRun `rux doctor` to see the toolchain"))?;
    if output.status.success() {
        return Ok(());
    }
    let mut detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if detail.is_empty() {
        detail = String::from_utf8_lossy(&output.stdout).trim().to_string();
    }
    if detail.is_empty() {
        detail = format!("it exited with {}", output.status);
    }
    Err(format!("{what} failed:\n{detail}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(name: &str) -> Manifest {
        Manifest {
            root: PathBuf::from("."),
            name: name.into(),
            id: "dev.example.tasks".into(),
            version: "0.1.0".into(),
            entry: PathBuf::from("app.rux"),
        }
    }

    #[test]
    fn the_manifest_names_the_library_cargo_will_actually_build() {
        // `Task List` is `task-list` as a filename and `task_list` as a library,
        // and the activity loads it by the second name. Getting this wrong gives
        // an app that installs, starts and finds nothing to run.
        let xml = android_manifest(&manifest("Task List"));
        assert!(xml.contains(r#"android:value="task_list""#), "{xml}");
    }

    #[test]
    fn an_apostrophe_in_a_name_does_not_break_the_manifest() {
        let xml = android_manifest(&manifest("Bob's Tools"));
        assert!(xml.contains("Bob&apos;s Tools"), "{xml}");
        assert!(!xml.contains("Bob's Tools"), "{xml}");
    }

    #[test]
    fn there_is_no_java_in_the_apk_and_the_manifest_says_so() {
        // Without `hasCode="false"` the platform looks for a dex file that no
        // part of this pipeline produces.
        let xml = android_manifest(&manifest("Task List"));
        assert!(xml.contains(r#"android:hasCode="false""#), "{xml}");
    }

    #[test]
    fn the_library_entry_is_a_zip_path_and_not_a_windows_one() {
        // This one cost a full build, install and launch to find. A zip entry
        // is separated by `/` everywhere, and `Path::join` on Windows is not,
        // so the library went in as one file called `lib\x86_64\libapp.so` and
        // the activity died looking for a directory that was never there.
        let entry = library_entry("libcounter_app.so");
        assert_eq!(entry, "lib/x86_64/libcounter_app.so");
        assert!(!entry.contains('\\'), "a zip entry never contains a backslash: {entry}");
    }

    #[test]
    fn the_floor_and_the_target_are_both_declared_and_are_not_the_same() {
        let xml = android_manifest(&manifest("Task List"));
        assert!(xml.contains(&format!(r#"android:minSdkVersion="{MIN_API}""#)), "{xml}");
        assert!(xml.contains(&format!(r#"android:targetSdkVersion="{TARGET_API}""#)), "{xml}");
        assert!(MIN_API < TARGET_API, "the floor must be below the target");
    }
}
