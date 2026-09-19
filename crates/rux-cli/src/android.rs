//! Finding the Android toolchain, and saying what is missing.
//!
//! **The diagnostic is the feature.** Rux's promise for v0.8 is mobile without
//! Android Studio and without Gradle, and the whole of what a Rux user installs
//! by hand is the Android command-line tools, a platform, the build tools and
//! an NDK. A promise like that is kept or broken entirely by what happens when
//! one of those is absent: "error: aapt2 not found" sends someone to a search
//! engine, and a line naming where it was looked for and the one command that
//! fixes it does not. So this is built to the same standard as a `rux check`
//! message, before anything can produce an APK, rather than bolted on after.
//!
//! Nothing here builds anything. It answers one question: could a build
//! happen, and if not, what exactly is missing.
//!
//! **It is written against paths, not against this machine.** Every lookup
//! takes the roots it should search, so the interesting case, a machine with
//! none of it installed, is an ordinary test against an empty directory. The
//! machine this was written on has the entire toolchain already, installed by
//! Flutter for unrelated work, so a passing run here proves nothing at all.

use std::path::{Path, PathBuf};

/// The API level Rux targets at its floor.
///
/// 26 (Android 8.0). The constraint is Vulkan driver dependability under
/// `wgpu`, not taste. Biased high on purpose: lowering a floor later gains
/// users in one line, while raising one drops devices from under people who
/// have already shipped.
pub const MIN_API: u32 = 26;

/// The ABI slice 4 builds first, and the one a phone almost certainly wants.
pub const FIRST_ABI: Abi = Abi {
    name: "arm64-v8a",
    rust_target: "aarch64-linux-android",
    clang_prefix: "aarch64-linux-android",
};

/// One Android ABI, under the three names it goes by.
///
/// Only arm64 exists here, because only arm64 is used yet. The other three
/// (`armeabi-v7a`, `x86_64`, `x86`) arrive when a build produces every ABI,
/// and one of them carries a trap worth knowing before then: the 32-bit ARM
/// Rust target is `armv7-linux-androideabi` while the NDK's clang driver for
/// the same architecture is spelled `armv7a-linux-androideabi`. The two names
/// differ by one letter and neither side is wrong.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Abi {
    /// What the APK calls it, and what the `.so` is packed under.
    pub name: &'static str,
    /// What `rustup target add` calls it.
    pub rust_target: &'static str,
    /// What the NDK calls the clang driver for it.
    pub clang_prefix: &'static str,
}

/// Where to look, which is the whole input to a survey.
///
/// Held as data rather than read from the environment inside the lookups, so
/// that the absent case is a test and not a story about a machine nobody has.
#[derive(Clone, Debug, Default)]
pub struct Search {
    /// The Android SDK root, and how it was found, for saying so.
    pub sdk: Option<PathBuf>,
    pub sdk_source: Option<String>,
    /// Every place an SDK was looked for, when none was found.
    pub sdk_looked: Vec<PathBuf>,
    /// `JAVA_HOME`, if it is set.
    pub java_home: Option<PathBuf>,
    /// `PATH`, as directories.
    ///
    /// Taken as data for the same reason as everything else here: this machine
    /// has a JDK on it, and a test that asked the real `PATH` would report a
    /// bare machine as having one. That is precisely the case the tests exist
    /// to cover.
    pub path: Vec<PathBuf>,
    /// The Rust targets `rustup` reports as installed, and `None` when rustup
    /// could not be asked at all, which is a different thing from none being
    /// installed and is reported differently.
    pub rust_targets: Option<Vec<String>>,
}

impl Search {
    /// What this machine says, right now.
    pub fn from_environment() -> Self {
        let mut looked = Vec::new();
        let mut sdk = None;
        let mut sdk_source = None;
        for name in ["ANDROID_HOME", "ANDROID_SDK_ROOT"] {
            let Some(value) = std::env::var_os(name) else { continue };
            let path = PathBuf::from(value);
            looked.push(path.clone());
            if sdk.is_none() && path.is_dir() {
                sdk = Some(path);
                sdk_source = Some(format!("${name}"));
            }
        }
        for candidate in default_sdk_paths() {
            looked.push(candidate.clone());
            if sdk.is_none() && candidate.is_dir() {
                sdk = Some(candidate);
                sdk_source = Some("the usual place for this platform".to_string());
            }
        }
        Self {
            sdk,
            sdk_source,
            sdk_looked: looked,
            java_home: std::env::var_os("JAVA_HOME").map(PathBuf::from),
            path: std::env::var_os("PATH")
                .map(|p| std::env::split_paths(&p).collect())
                .unwrap_or_default(),
            rust_targets: installed_rust_targets(),
        }
    }
}

/// Where the SDK sits when nobody has said otherwise.
fn default_sdk_paths() -> Vec<PathBuf> {
    let home = std::env::var_os(if cfg!(windows) { "LOCALAPPDATA" } else { "HOME" });
    let Some(home) = home.map(PathBuf::from) else { return Vec::new() };
    if cfg!(windows) {
        vec![home.join("Android").join("Sdk")]
    } else if cfg!(target_os = "macos") {
        vec![home.join("Library").join("Android").join("sdk")]
    } else {
        vec![home.join("Android").join("Sdk"), home.join("Android").join("sdk")]
    }
}

/// Ask rustup what is installed, or `None` if it cannot be asked.
fn installed_rust_targets() -> Option<Vec<String>> {
    let out = std::process::Command::new("rustup")
        .args(["target", "list", "--installed"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).lines().map(|l| l.trim().to_string()).collect())
}

/// What one lookup found, or did not.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    Found {
        /// The version or the file, whichever tells the reader more.
        detail: String,
        at: PathBuf,
    },
    Missing {
        /// Every path that was tried, so "it is installed" and "it is not where
        /// you are looking" stop being the same message.
        looked: Vec<PathBuf>,
        /// The one command that fixes it, ready to paste.
        fix: String,
    },
    /// Present, but not usable for what Rux needs.
    Unusable {
        why: String,
        fix: String,
    },
}

impl Outcome {
    pub fn is_found(&self) -> bool {
        matches!(self, Outcome::Found { .. })
    }
}

/// One line of the report.
#[derive(Clone, Debug)]
pub struct Finding {
    pub what: String,
    /// What a build would do with it, so that a missing piece explains its own
    /// consequence rather than only its name.
    pub needed_for: &'static str,
    pub outcome: Outcome,
}

/// Everything the survey found.
#[derive(Clone, Debug)]
pub struct Survey {
    pub findings: Vec<Finding>,
}

impl Survey {
    pub fn missing(&self) -> usize {
        self.findings.iter().filter(|f| !f.outcome.is_found()).count()
    }

    pub fn ready(&self) -> bool {
        self.missing() == 0
    }
}

/// The executable name a tool goes by on this platform.
///
/// The SDK ships `.bat` wrappers for the java-based tools and `.exe` for the
/// native ones, and neither is named that anywhere else, so the suffix is per
/// tool and not per platform.
fn exe(stem: &str, kind: Kind) -> String {
    match (cfg!(windows), kind) {
        (true, Kind::Native) => format!("{stem}.exe"),
        (true, Kind::Script) => format!("{stem}.bat"),
        (true, Kind::Cmd) => format!("{stem}.cmd"),
        (false, _) => stem.to_string(),
    }
}

#[derive(Clone, Copy)]
enum Kind {
    Native,
    Script,
    Cmd,
}

/// The host tag the NDK files its prebuilt toolchain under.
fn ndk_host() -> &'static str {
    if cfg!(windows) {
        "windows-x86_64"
    } else if cfg!(target_os = "macos") {
        "darwin-x86_64"
    } else {
        "linux-x86_64"
    }
}

/// Subdirectories of `dir`, newest version first.
///
/// Sorted by version *component*, not as text: `9.0.0` is older than `35.0.0`
/// and sorts after it in every string comparison there is.
fn versions_in(dir: &Path) -> Vec<(Vec<u64>, PathBuf)> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut out: Vec<(Vec<u64>, PathBuf)> = entries
        .flatten()
        .filter(|e| e.path().is_dir())
        .map(|e| (version_key(&e.file_name().to_string_lossy()), e.path()))
        .collect();
    out.sort_by(|a, b| b.0.cmp(&a.0));
    out
}

/// Every run of digits in a name, in order. `android-35` is `[35]`, `35.0.0` is
/// `[35, 0, 0]`, and anything with no digits sorts last.
fn version_key(name: &str) -> Vec<u64> {
    let mut out = Vec::new();
    let mut digits = String::new();
    for c in name.chars() {
        if c.is_ascii_digit() {
            digits.push(c);
        } else if !digits.is_empty() {
            out.push(digits.parse().unwrap_or(0));
            digits.clear();
        }
    }
    if !digits.is_empty() {
        out.push(digits.parse().unwrap_or(0));
    }
    out
}

/// How to get `sdkmanager` to install something, spelled for this platform.
fn sdkmanager_fix(package: &str) -> String {
    format!("sdkmanager \"{package}\"")
}

/// Survey a machine whose SDK is at `sdk`.
///
/// Split from [`Search`] so a test can hand it any directory at all, including
/// one with nothing in it.
pub fn survey(search: &Search) -> Survey {
    let mut findings = Vec::new();

    let Some(sdk) = search.sdk.clone() else {
        findings.push(Finding {
            what: "Android SDK".into(),
            needed_for: "everything below comes out of it",
            outcome: Outcome::Missing {
                looked: search.sdk_looked.clone(),
                fix: "install the Android command-line tools and set ANDROID_HOME to the \
                      directory holding them. Rux never needs Android Studio; see \
                      https://ruxlang.dev/tooling/android/"
                    .into(),
            },
        });
        // Everything else is a path inside the SDK, so there is nothing further
        // to say that would not be the same sentence four more times.
        return Survey { findings };
    };

    findings.push(Finding {
        what: "Android SDK".into(),
        needed_for: "everything below comes out of it",
        outcome: Outcome::Found {
            detail: search.sdk_source.clone().unwrap_or_else(|| "found".into()),
            at: sdk.clone(),
        },
    });

    findings.push(sdkmanager(&sdk));
    findings.push(build_tools(&sdk));
    findings.push(platform(&sdk));
    findings.push(platform_tools(&sdk));
    findings.push(ndk(&sdk));
    findings.push(jdk(search));
    findings.push(rust_target(search, FIRST_ABI));

    Survey { findings }
}

fn sdkmanager(sdk: &Path) -> Finding {
    let at = sdk.join("cmdline-tools").join("latest").join("bin").join(exe("sdkmanager", Kind::Script));
    Finding {
        what: "command-line tools".into(),
        needed_for: "installing everything else, and nothing else needs it",
        outcome: if at.is_file() {
            Outcome::Found { detail: "sdkmanager".into(), at }
        } else {
            Outcome::Missing {
                looked: vec![at],
                fix: "download the command-line tools from \
                      https://developer.android.com/studio#command-line-tools-only and unzip \
                      them so that the `bin` directory is at <sdk>/cmdline-tools/latest/bin"
                    .into(),
            }
        },
    }
}

fn build_tools(sdk: &Path) -> Finding {
    let root = sdk.join("build-tools");
    let wanted = [
        exe("aapt2", Kind::Native),
        exe("zipalign", Kind::Native),
        exe("apksigner", Kind::Script),
    ];
    for (key, dir) in versions_in(&root) {
        if wanted.iter().all(|tool| dir.join(tool).is_file()) {
            let version =
                key.iter().map(|n| n.to_string()).collect::<Vec<_>>().join(".");
            return Finding {
                what: "build-tools".into(),
                needed_for: "aapt2 packs the resources, zipalign aligns the APK, apksigner signs it",
                outcome: Outcome::Found { detail: version, at: dir },
            };
        }
    }
    Finding {
        what: "build-tools".into(),
        needed_for: "aapt2 packs the resources, zipalign aligns the APK, apksigner signs it",
        outcome: Outcome::Missing {
            looked: vec![root],
            fix: sdkmanager_fix("build-tools;35.0.0"),
        },
    }
}

fn platform(sdk: &Path) -> Finding {
    let root = sdk.join("platforms");
    let what = "platform".to_string();
    let needed_for = "android.jar, which the manifest and the resources are compiled against";
    let mut best: Option<(u64, PathBuf)> = None;
    for (key, dir) in versions_in(&root) {
        let api = key.first().copied().unwrap_or(0);
        if dir.join("android.jar").is_file() && best.as_ref().is_none_or(|(b, _)| api > *b) {
            best = Some((api, dir));
        }
    }
    match best {
        Some((api, dir)) if api >= MIN_API as u64 => Finding {
            what,
            needed_for,
            outcome: Outcome::Found { detail: format!("android-{api}"), at: dir },
        },
        Some((api, _)) => Finding {
            what,
            needed_for,
            outcome: Outcome::Unusable {
                why: format!(
                    "the newest platform installed is android-{api}, and Rux targets API \
                     {MIN_API} and up"
                ),
                fix: sdkmanager_fix("platforms;android-35"),
            },
        },
        None => Finding {
            what,
            needed_for,
            outcome: Outcome::Missing {
                looked: vec![root],
                fix: sdkmanager_fix("platforms;android-35"),
            },
        },
    }
}

fn platform_tools(sdk: &Path) -> Finding {
    let at = sdk.join("platform-tools").join(exe("adb", Kind::Native));
    Finding {
        what: "platform-tools".into(),
        needed_for: "adb installs and runs the app on a device or an emulator",
        outcome: if at.is_file() {
            Outcome::Found { detail: "adb".into(), at }
        } else {
            Outcome::Missing { looked: vec![at], fix: sdkmanager_fix("platform-tools") }
        },
    }
}

fn ndk(sdk: &Path) -> Finding {
    let root = sdk.join("ndk");
    let what = "NDK".to_string();
    let needed_for = "its clang is the linker for every Android build";
    // The exact driver a build invokes, rather than the directory it lives in:
    // an NDK unpacked for another host, or one too old to have the API level
    // Rux targets, has the directory and not the file.
    let driver = exe(&format!("{}{}-clang", FIRST_ABI.clang_prefix, MIN_API), Kind::Cmd);
    let mut looked = Vec::new();
    for (key, dir) in versions_in(&root) {
        let bin = dir.join("toolchains").join("llvm").join("prebuilt").join(ndk_host()).join("bin");
        let at = bin.join(&driver);
        if at.is_file() {
            let version = key.iter().map(|n| n.to_string()).collect::<Vec<_>>().join(".");
            return Finding {
                what,
                needed_for,
                outcome: Outcome::Found { detail: version, at: bin },
            };
        }
        looked.push(at);
    }
    if looked.is_empty() {
        looked.push(root);
    }
    Finding {
        what,
        needed_for,
        outcome: Outcome::Missing { looked, fix: sdkmanager_fix("ndk;28.2.13676358") },
    }
}

fn jdk(search: &Search) -> Finding {
    let what = "JDK".to_string();
    let needed_for = "apksigner and the SDK's own tools are java programs";
    let mut looked = Vec::new();
    if let Some(home) = &search.java_home {
        let at = home.join("bin").join(exe("java", Kind::Native));
        if at.is_file() {
            return Finding {
                what,
                needed_for,
                outcome: Outcome::Found { detail: "$JAVA_HOME".into(), at },
            };
        }
        looked.push(at);
    }
    if let Some(at) = on_path(&search.path, &exe("java", Kind::Native)) {
        return Finding {
            what,
            needed_for,
            outcome: Outcome::Found { detail: "on PATH".into(), at },
        };
    }
    Finding {
        what,
        needed_for,
        outcome: Outcome::Missing {
            looked,
            fix: "install a JDK (17 or newer) and set JAVA_HOME to it".into(),
        },
    }
}

fn rust_target(search: &Search, abi: Abi) -> Finding {
    let what = format!("rust target {}", abi.rust_target);
    let needed_for = "the Rust half of the app is cross-compiled to it";
    let fix = format!("rustup target add {}", abi.rust_target);
    match &search.rust_targets {
        Some(installed) if installed.iter().any(|t| t == abi.rust_target) => Finding {
            what,
            needed_for,
            outcome: Outcome::Found { detail: abi.name.into(), at: PathBuf::from("rustup") },
        },
        Some(_) => Finding {
            what,
            needed_for,
            outcome: Outcome::Missing { looked: vec![PathBuf::from("rustup")], fix },
        },
        None => Finding {
            what,
            needed_for,
            outcome: Outcome::Unusable {
                why: "rustup could not be asked which targets are installed".into(),
                fix,
            },
        },
    }
}

/// Look for `name` in these directories.
fn on_path(path: &[PathBuf], name: &str) -> Option<PathBuf> {
    path.iter().map(|dir| dir.join(name)).find(|p| p.is_file())
}

/// Render the survey the way `rux check` renders diagnostics: something a
/// person reads top to bottom and can act on without going anywhere else.
pub fn render(survey: &Survey) -> String {
    let mut out = String::from("rux: the Android toolchain\n\n");
    let width = survey.findings.iter().map(|f| f.what.len()).max().unwrap_or(0);
    for finding in &survey.findings {
        match &finding.outcome {
            Outcome::Found { detail, at } => {
                out.push_str(&format!(
                    "  found    {:width$}  {}\n           {:width$}  {}\n",
                    finding.what,
                    detail,
                    "",
                    at.display(),
                    width = width
                ));
            }
            Outcome::Missing { looked, fix } => {
                out.push_str(&format!(
                    "  MISSING  {:width$}  {}\n",
                    finding.what,
                    finding.needed_for,
                    width = width
                ));
                for path in looked {
                    out.push_str(&format!("           {:width$}  looked in {}\n", "", path.display(), width = width));
                }
                out.push_str(&format!("           {:width$}  fix: {fix}\n", "", width = width));
            }
            Outcome::Unusable { why, fix } => {
                out.push_str(&format!(
                    "  UNUSABLE {:width$}  {why}\n           {:width$}  fix: {fix}\n",
                    finding.what,
                    "",
                    width = width
                ));
            }
        }
    }

    let missing = survey.missing();
    out.push('\n');
    if missing == 0 {
        out.push_str("rux: everything an Android build needs is here.\n");
        return out;
    }
    out.push_str(&format!(
        "rux: {missing} of {} missing. An Android build needs all of them.\n",
        survey.findings.len()
    ));
    // Most of the fixes above are `sdkmanager` commands, and `sdkmanager` is
    // itself one of the things that can be missing. Said once, here, rather
    // than qualified on every line it applies to.
    let no_sdkmanager = survey
        .findings
        .iter()
        .any(|f| f.what == "command-line tools" && !f.outcome.is_found());
    let wants_sdkmanager = survey.findings.iter().any(|f| {
        matches!(&f.outcome, Outcome::Missing { fix, .. } if fix.starts_with("sdkmanager"))
    });
    if no_sdkmanager && wants_sdkmanager {
        out.push_str(
            "rux: the `sdkmanager` fixes need the command-line tools, so do that one first.\n",
        );
    }
    out
}

/// `rux doctor`. Returns the process exit code.
pub fn doctor() -> i32 {
    let survey = survey(&Search::from_environment());
    print!("{}", render(&survey));
    // Non-zero when something is missing, so this is usable in a script that
    // sets a machine up, and not only by a person reading it.
    i32::from(!survey.ready())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A machine with nothing installed is the case that matters, and it is the
    /// one no amount of running this on a developer's laptop can reach: the
    /// machine this was written on has the whole toolchain already, put there
    /// by Flutter for unrelated work.
    #[test]
    fn a_machine_with_nothing_says_where_it_looked() {
        let search = Search {
            sdk: None,
            sdk_source: None,
            sdk_looked: vec![PathBuf::from("/nowhere/Android/Sdk")],
            java_home: None,
            path: Vec::new(),
            rust_targets: Some(Vec::new()),
        };
        let survey = survey(&search);
        assert!(!survey.ready());

        let text = render(&survey);
        assert!(text.contains("MISSING"), "{text}");
        assert!(text.contains("looked in"), "it has to say where: {text}");
        assert!(text.contains("ANDROID_HOME"), "and what to do about it: {text}");
        // No SDK means no point listing seven things inside one.
        assert_eq!(survey.findings.len(), 1, "{text}");
    }

    /// An SDK directory that exists but is empty: every tool reports itself
    /// missing, with its own fix, rather than one error about the first one.
    #[test]
    fn an_empty_sdk_reports_every_tool_with_its_own_fix() {
        let dir = fixture("empty-sdk");
        let search = Search {
            sdk: Some(dir.clone()),
            sdk_source: Some("$ANDROID_HOME".into()),
            sdk_looked: vec![dir],
            java_home: Some(PathBuf::from("/nowhere/jdk")),
            path: Vec::new(),
            rust_targets: Some(Vec::new()),
        };
        let survey = survey(&search);
        let text = render(&survey);

        // The SDK itself is found; everything under it is not.
        assert_eq!(survey.missing(), survey.findings.len() - 1, "{text}");
        for wanted in [
            "sdkmanager \"build-tools",
            "sdkmanager \"platforms;android-35\"",
            "sdkmanager \"platform-tools\"",
            "sdkmanager \"ndk;",
            "rustup target add aarch64-linux-android",
            "JAVA_HOME",
        ] {
            assert!(text.contains(wanted), "no fix line for {wanted}:\n{text}");
        }
    }

    /// A full SDK is recognised, and the *newest* build-tools wins.
    ///
    /// By version component and not as text: `9.0.0` sorts after `35.0.0` in
    /// every string comparison there is, and picking it would hand a build a
    /// toolchain seven years too old.
    #[test]
    fn a_full_sdk_is_found_and_the_newest_version_wins() {
        let dir = fixture("full-sdk");
        write_tool(&dir.join("cmdline-tools/latest/bin"), &exe("sdkmanager", Kind::Script));
        for version in ["9.0.0", "35.0.0"] {
            let bt = dir.join("build-tools").join(version);
            for tool in [
                exe("aapt2", Kind::Native),
                exe("zipalign", Kind::Native),
                exe("apksigner", Kind::Script),
            ] {
                write_tool(&bt, &tool);
            }
        }
        write_tool(&dir.join("platforms/android-35"), "android.jar");
        write_tool(&dir.join("platform-tools"), &exe("adb", Kind::Native));
        let bin = dir
            .join("ndk/28.2.13676358/toolchains/llvm/prebuilt")
            .join(ndk_host())
            .join("bin");
        write_tool(&bin, &exe(&format!("{}{MIN_API}-clang", FIRST_ABI.clang_prefix), Kind::Cmd));

        let search = Search {
            sdk: Some(dir.clone()),
            sdk_source: Some("$ANDROID_HOME".into()),
            sdk_looked: vec![dir],
            java_home: None,
            path: Vec::new(),
            rust_targets: Some(vec![FIRST_ABI.rust_target.to_string()]),
        };
        let survey = survey(&search);
        let text = render(&survey);

        let build_tools = survey.findings.iter().find(|f| f.what == "build-tools").unwrap();
        match &build_tools.outcome {
            Outcome::Found { detail, .. } => assert_eq!(detail, "35.0.0", "{text}"),
            other => panic!("build-tools not found: {other:?}"),
        }
        for what in ["Android SDK", "command-line tools", "platform", "platform-tools", "NDK"] {
            let finding = survey.findings.iter().find(|f| f.what == what).unwrap();
            assert!(finding.outcome.is_found(), "{what} not found:\n{text}");
        }
    }

    /// A platform older than Rux's floor is present and useless, which is a
    /// third answer and not a variety of "missing": the fix is different, and
    /// so is what the reader has to understand.
    #[test]
    fn a_platform_below_the_floor_is_unusable_rather_than_missing() {
        let dir = fixture("old-platform");
        write_tool(&dir.join("platforms/android-21"), "android.jar");
        let search = Search {
            sdk: Some(dir.clone()),
            sdk_source: Some("$ANDROID_HOME".into()),
            sdk_looked: vec![dir],
            java_home: None,
            path: Vec::new(),
            rust_targets: Some(Vec::new()),
        };
        let survey = survey(&search);
        let platform = survey.findings.iter().find(|f| f.what == "platform").unwrap();
        match &platform.outcome {
            Outcome::Unusable { why, .. } => {
                assert!(why.contains("android-21"), "{why}");
                assert!(why.contains(&MIN_API.to_string()), "{why}");
            }
            other => panic!("expected unusable, got {other:?}"),
        }
    }

    /// Version ordering is numeric per component, which is the whole reason
    /// this is not a string sort.
    #[test]
    fn versions_sort_by_number_and_not_by_text() {
        assert!(version_key("35.0.0") > version_key("9.0.0"));
        assert!(version_key("android-35") > version_key("android-9"));
        assert!(version_key("29.0.14033849") > version_key("28.2.13676358"));
    }

    fn fixture(name: &str) -> PathBuf {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/android-fixtures")
            .join(name);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create fixture");
        dir
    }

    fn write_tool(dir: &Path, name: &str) {
        std::fs::create_dir_all(dir).expect("create fixture dir");
        std::fs::write(dir.join(name), b"fixture").expect("write fixture tool");
    }
}
