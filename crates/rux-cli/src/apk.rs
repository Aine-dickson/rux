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

use crate::android::{Abi, Toolchain, MIN_API};
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
    built: &[(Abi, PathBuf)],
    work: &Path,
    out: &Path,
) -> Result<(), String> {
    if built.is_empty() {
        return Err("an APK needs at least one ABI".to_string());
    }

    // Laid out as the APK wants it, because the tool that adds a library takes
    // the path to store it under from the path on disk.
    let staging = work.join("apk");
    // Removed rather than written over: a rename in `rux.toml`, or a build that
    // produced four ABIs followed by one that produced a single ABI, would
    // otherwise leave the previous libraries beside the new ones and pack them
    // all. An APK claiming an ABI whose library is a build old is worse than
    // one that never claimed it.
    let _ = std::fs::remove_dir_all(&staging);

    let mut entries = Vec::new();
    for (abi, from) in built {
        if !from.is_file() {
            return Err(format!("the generated crate built nothing at {}", from.display()));
        }
        let lib_dir = staging.join("lib").join(abi.name);
        std::fs::create_dir_all(&lib_dir)
            .map_err(|e| format!("creating {}: {e}", lib_dir.display()))?;
        // Debug symbols are most of the size of a Rust shared object and none of
        // them are any use on a device: 339 MB became 51 MB the first time this
        // ran. Stripping is not an optimisation here so much as the difference
        // between an APK that installs over adb in seconds and one that does
        // not, and with four ABIs it is that difference four times over.
        let library = lib_dir.join(from.file_name().unwrap_or_default());
        run(Command::new(toolchain.strip()).arg("-o").arg(&library).arg(from), "llvm-strip")?;
        entries.push(library_entry(
            *abi,
            &library.file_name().unwrap_or_default().to_string_lossy(),
        ));
    }

    let dex = build_dex(toolchain, &staging)?;

    let manifest_xml = staging.join("AndroidManifest.xml");
    std::fs::write(&manifest_xml, android_manifest(manifest))
        .map_err(|e| format!("writing {}: {e}", manifest_xml.display()))?;

    // One invocation for the whole tree, whatever is in it: `--dir` walks it and
    // emits a zip of compiled resources, so the number of densities never
    // becomes a number of processes.
    let resources = match manifest.icon_source()? {
        None => None,
        Some((foreground, background, splash)) => {
            let res = crate::icon::generate(&foreground, background, splash, &staging)?;
            let compiled = staging.join("res.zip");
            run(
                Command::new(toolchain.aapt2())
                    .arg("compile")
                    .arg("--dir")
                    .arg(&res)
                    .arg("-o")
                    .arg(&compiled),
                "aapt2 compile",
            )?;
            Some(compiled)
        }
    };

    let base = staging.join("base.apk");
    let mut link = Command::new(toolchain.aapt2());
    link.arg("link")
        .arg("--manifest")
        .arg(&manifest_xml)
        .arg("-I")
        .arg(toolchain.android_jar())
        .args(["--min-sdk-version", &MIN_API.to_string()])
        .args(["--target-sdk-version", &TARGET_API.to_string()])
        .arg("-o")
        .arg(&base);
    if let Some(compiled) = &resources {
        // **Positional, and NOT `-R`.** `-R` declares an *overlay*, and an
        // overlay may only replace a resource that already exists: the first
        // `values/` resource we add fails with "does not override an existing
        // resource", which reads like a spelling mistake and is not one. A
        // PNG-only icon links fine under `-R`, so the wrong flag survives every
        // test that does not add a colour. `--auto-add-overlay` silences it and
        // is the wrong fix, because then a genuine typo silently adds a
        // resource instead of failing.
        link.arg(compiled);
    }
    run(&mut link, "aapt2 link")?;

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
    let mut add = Command::new(toolchain.aapt());
    add.arg("add")
        .arg("base.apk")
        // `classes.dex` has to sit at the archive root under exactly that
        // name, which is where the platform looks for an app's code.
        .arg(dex_entry())
        .current_dir(&staging);
    for entry in &entries {
        add.arg(entry);
    }
    run(&mut add, "aapt add")?;
    debug_assert!(dex.is_file(), "the dex was added to the APK without existing");

    let aligned = staging.join("aligned.apk");
    run(
        Command::new(toolchain.zipalign()).args(["-f", "4"]).arg(&base).arg(&aligned),
        "zipalign",
    )?;

    // Deleted first: apksigner refuses to write over an existing output, and a
    // second build would otherwise fail on the leftovers of the first.
    let _ = std::fs::remove_file(out);
    let mut sign = Command::new(toolchain.apksigner());
    sign.arg("sign");
    match manifest.signing_key()? {
        Some((keystore, alias, store_password, key_password)) => {
            println!("rux: signing with {} ({alias})", keystore.display());
            sign.arg("--ks")
                .arg(&keystore)
                .args(["--ks-key-alias", &alias])
                // Passed as `pass:` rather than through a file, because the
                // alternative is writing the password to disk for the length of
                // a build. It is visible in this process's arguments either
                // way; a file would also leave it somewhere afterwards.
                .args(["--ks-pass", &format!("pass:{store_password}")])
                .args(["--key-pass", &format!("pass:{key_password}")]);
        }
        None => {
            let keystore = debug_keystore(toolchain)?;
            sign.arg("--ks")
                .arg(&keystore)
                .args(["--ks-pass", "pass:android"])
                .args(["--key-pass", "pass:android"]);
        }
    }
    run(sign.arg("--out").arg(out).arg(&aligned), "apksigner")?;

    Ok(())
}

/// The Java source for the one class a Rux app carries.
///
/// Carried as text and compiled on every build rather than shipped as a
/// prebuilt `classes.dex`. Compiling costs about a second, and both tools it
/// needs are already required: `javac` comes with the JDK that `apksigner` runs
/// on, and `d8` is in the build-tools beside `aapt2`. The alternative, a dex
/// blob committed to the repo, would be a binary nobody can read in a diff and
/// a build artifact checked into source, to save a step that costs a second.
const ACTIVITY_JAVA: &str = include_str!("../java/RuxActivity.java");

/// The fully-qualified name of that class, as the manifest names it.
///
/// Fixed rather than derived from the app's id, so the Java is one constant
/// file rather than something generated per project. An activity class does not
/// have to live in the application's own package, and giving every Rux app the
/// same activity class means the Java is compiled from source nobody has to
/// read twice.
pub(crate) const ACTIVITY_CLASS: &str = "dev.ruxlang.shell.RuxActivity";

/// Every `.class` under `dir`, which is more files than there are sources.
///
/// `javac` writes one file per class, and a nested class is a class: one
/// `.java` here produces `RuxActivity.class` beside
/// `RuxActivity$RuxInputView.class` and `RuxActivity$RuxInputConnection.class`.
/// Collecting by walking rather than by naming is the same choice `rux build`
/// makes about a project's files, for the same reason: the naming version is
/// exact right up until it silently misses something.
fn collect_classes(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries = std::fs::read_dir(dir).map_err(|e| format!("reading {}: {e}", dir.display()))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("reading {}: {e}", dir.display()))?;
        let path = entry.path();
        if path.is_dir() {
            collect_classes(&path, out)?;
        } else if path.extension().is_some_and(|e| e == "class") {
            out.push(path);
        }
    }
    Ok(())
}

/// The dex's entry name, which the platform requires to be exactly this.
fn dex_entry() -> &'static str {
    "classes.dex"
}

/// Compile and dex `RuxActivity.java`.
///
/// Two commands, and neither of them is Gradle. `javac` is pinned to Java 8
/// bytecode because that is what `d8` and the platform expect, and a JDK 17
/// left to its own defaults emits class files too new for either.
fn build_dex(toolchain: &Toolchain, staging: &Path) -> Result<PathBuf, String> {
    let java_dir = staging.join("java").join("dev").join("ruxlang").join("shell");
    std::fs::create_dir_all(&java_dir)
        .map_err(|e| format!("creating {}: {e}", java_dir.display()))?;
    let source = java_dir.join("RuxActivity.java");
    std::fs::write(&source, ACTIVITY_JAVA)
        .map_err(|e| format!("writing {}: {e}", source.display()))?;

    let classes = staging.join("classes");
    std::fs::create_dir_all(&classes).map_err(|e| format!("creating {}: {e}", classes.display()))?;
    run(
        Command::new(toolchain.javac())
            // Java 8 bytecode. `--release` rather than `-source`/`-target`,
            // which is the pair that compiles against the running JDK's own
            // library and then fails at runtime on a method that did not exist
            // in 8.
            .args(["--release", "8"])
            .arg("-classpath")
            .arg(toolchain.android_jar())
            .arg("-d")
            .arg(&classes)
            .arg(&source),
        "javac",
    )?;

    // **Every class file, not just the one named after the source.** A nested
    // class compiles to its own file, `RuxActivity$RuxInputView.class` and so
    // on, and handing `d8` only the outer one produces a dex that loads and
    // then dies at the first `new` with `ClassNotFoundException`. Which is what
    // it did: the app started, created the activity, and crashed reaching for
    // the view that provides the input connection.
    let mut compiled = Vec::new();
    collect_classes(&classes, &mut compiled)?;
    if compiled.is_empty() {
        return Err(format!("javac produced no class files in {}", classes.display()));
    }
    compiled.sort();
    let mut d8 = Command::new(toolchain.d8());
    d8.arg("--lib")
        .arg(toolchain.android_jar())
        .args(["--min-api", &MIN_API.to_string()])
        .arg("--output")
        .arg(staging);
    for class in &compiled {
        d8.arg(class);
    }
    run(&mut d8, "d8")?;

    let dex = staging.join(dex_entry());
    if !dex.is_file() {
        return Err(format!("d8 produced no {} in {}", dex_entry(), staging.display()));
    }
    Ok(dex)
}

/// Where the native library sits inside the APK, as a zip entry name.
///
/// A string rather than a `Path`, and that is the whole point of it existing:
/// zip entries are separated by `/` on every platform, while `Path::join` on
/// Windows gives `\`. The backslash version builds, signs and installs without
/// complaint, then fails at launch with "unable to find native library",
/// because Android is looking inside a `lib/x86_64/` directory that the archive
/// does not have.
fn library_entry(abi: Abi, file_name: &str) -> String {
    format!("lib/{}/{file_name}", abi.name)
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
/// - **`android:hasCode="true"`, and the activity is ours.** It was `false`
///   until an app needed to answer questions only a real Java class can be
///   asked: window insets, and soon the input connection a soft keyboard
///   attaches to. The APK now carries exactly one class, in one `classes.dex`,
///   compiled from one file Rux ships. See [`ACTIVITY_JAVA`].
/// - **`android.app.lib_name`.** How `NativeActivity` knows which `.so` to
///   load, given without the `lib` prefix or the `.so` suffix. It is inherited
///   by the subclass, so naming our own activity changes nothing about it.
/// - **`configChanges`.** Every one of these is a change a native app handles
///   by being told about it. Leaving them out means Android destroys and
///   recreates the activity on a rotation, which for a GPU surface means
///   tearing down the swapchain to redraw the same thing.
/// - **`windowSoftInputMode="adjustResize"`**, which tells the app it has less
///   room while the keyboard is up. It briefly also carried `stateHidden`,
///   because the view that receives an input connection holds focus from the
///   moment the app opens and Android reads a focused text editor as a reason
///   to raise the keyboard. That is now answered properly, by the view saying
///   it is not an editor until Rux focuses a field, and `stateHidden` turned
///   out to suppress the keyboard afterwards as well.
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
        android:label="{label}"{icon}
        android:hasCode="true"
        android:extractNativeLibs="true">
        <activity
            android:name="{activity}"{theme}
            android:exported="true"
            android:windowSoftInputMode="adjustResize"
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
        // Absent entirely when the app has no icon, rather than named and left
        // to resolve to nothing: `aapt2` fails a link naming a resource that
        // was never compiled, so an app with no art would not package at all.
        icon = match manifest.icon {
            Some(_) => format!("\n        android:icon=\"{}\"", crate::icon::manifest_reference()),
            None => String::new(),
        },
        // The splash screen and nothing else. An app with no icon keeps the
        // themeless activity it has always had, because the theme exists to
        // carry splash attributes and a splash screen is the icon on a plate.
        theme = match manifest.icon {
            Some(_) => format!("\n            android:theme=\"{}\"", crate::icon::theme_reference()),
            None => String::new(),
        },
        activity = ACTIVITY_CLASS,
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
            signing: None,
            icon: None,
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
    fn an_app_with_no_icon_names_neither_an_icon_nor_a_theme() {
        // Both would be references to resources that were never compiled, and
        // `aapt2 link` fails on one of those rather than ignoring it, so an app
        // without art would not package at all.
        let xml = android_manifest(&manifest("Task List"));
        assert!(!xml.contains("android:icon"), "{xml}");
        assert!(!xml.contains("android:theme"), "{xml}");
    }

    #[test]
    fn an_app_with_an_icon_names_the_resources_that_will_be_generated() {
        let mut with_icon = manifest("Task List");
        with_icon.icon = Some(crate::manifest::Icon {
            foreground: PathBuf::from("icon.png"),
            background: "#7c3aed".into(),
            splash: "#7c3aed".into(),
        });
        let xml = android_manifest(&with_icon);
        // Against the generator rather than a literal: the names are written in
        // two files, and that is exactly how the activity class drifted once.
        assert!(xml.contains(&crate::icon::manifest_reference()), "{xml}");
        assert!(xml.contains(&crate::icon::theme_reference()), "{xml}");
    }

    #[test]
    fn the_apk_declares_its_one_class_and_names_our_activity() {
        // These two go together and are wrong apart. `hasCode="true"` without a
        // dex makes the platform look for code that is not there, and naming
        // our activity without the dex is the same failure by another road.
        let xml = android_manifest(&manifest("Task List"));
        assert!(xml.contains(r#"android:hasCode="true""#), "{xml}");
        assert!(xml.contains(&format!(r#"android:name="{ACTIVITY_CLASS}""#)), "{xml}");
    }

    #[test]
    fn the_activity_class_matches_the_java_that_is_shipped() {
        // The class name is written twice, in the manifest and in the Java, and
        // a mismatch is an app that installs and dies on launch. The JNI symbol
        // in `rux-shell` is a third copy of the same name, which is why the
        // package is checked here rather than only the class.
        assert!(ACTIVITY_JAVA.contains("package dev.ruxlang.shell;"), "package moved");
        assert!(ACTIVITY_JAVA.contains("class RuxActivity"), "class renamed");
        assert_eq!(ACTIVITY_CLASS, "dev.ruxlang.shell.RuxActivity");
        assert!(ACTIVITY_JAVA.contains("nativeSafeArea"), "the inset callback is gone");
        // `rux-shell` calls this one by name through JNI, and a failed lookup is
        // logged and swallowed by the error policy, so renaming it here would
        // show up only as a splash screen that hangs for five seconds on a
        // device. Nothing else would say a word.
        assert!(ACTIVITY_JAVA.contains("ruxFirstFrame"), "the splash release is gone");
    }

    #[test]
    fn the_library_entry_is_a_zip_path_and_not_a_windows_one() {
        // This one cost a full build, install and launch to find. A zip entry
        // is separated by `/` everywhere, and `Path::join` on Windows is not,
        // so the library went in as one file called `lib\x86_64\libapp.so` and
        // the activity died looking for a directory that was never there.
        let entry = library_entry(crate::android::DEV_ABI, "libcounter_app.so");
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
