//! `rux run --device`: build, install, start, in one command.
//!
//! **This is part of the promise, not a convenience.** Without it `rux build`
//! emits an APK that someone then has to install by hand, with `adb`, which is
//! one of the tools v0.8 says they need not learn. A build step that ends in
//! "now run this other tool you have never used" is the promise leaking.
//!
//! The same command drives an emulator and a phone, because to `adb` they are
//! the same thing. That matters for developing without a device: nothing here
//! knows or cares which one answered.

use std::path::Path;
use std::process::Command;

use crate::android::Toolchain;
use crate::build::{Options, Target};
use crate::manifest::Manifest;

pub fn run(args: &[String]) -> i32 {
    let mut rux_source = None;
    let mut release = false;
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--device" => {}
            "--release" => release = true,
            "--rux-source" => match rest.next() {
                Some(value) => rux_source = Some(std::path::PathBuf::from(value)),
                None => {
                    eprintln!("rux: `--rux-source` needs a directory");
                    return 2;
                }
            },
            // Named rather than swept into "unknown option": both are real
            // options of `rux run` that simply cannot mean anything here, and
            // saying why is the difference between a typo and a misunderstanding.
            "--preview" => {
                eprintln!(
                    "rux: `--preview` and `--device` ask for opposite things.\n\n\
                     `--preview` makes a desktop window pretend to be a phone. \
                     `--device` uses a real one."
                );
                return 2;
            }
            "--route" => {
                eprintln!(
                    "rux: `--route` does not reach a device yet.\n\n\
                     On a device a route arrives as an Intent, which is not built. \
                     The app starts at its entry document."
                );
                return 2;
            }
            flag => {
                eprintln!("rux: unknown option `{flag}`");
                return 2;
            }
        }
    }

    let toolchain = match Toolchain::resolve() {
        Ok(toolchain) => toolchain,
        Err(why) => {
            eprintln!("rux: {why}");
            return 2;
        }
    };

    // Asked before a three minute build, rather than after one.
    let serial = match one_device(&toolchain) {
        Ok(serial) => serial,
        Err(why) => {
            eprintln!("rux: {why}");
            return 2;
        }
    };

    let cwd = std::env::current_dir().unwrap_or_default();
    let manifest = match Manifest::find(&cwd) {
        Ok(manifest) => manifest,
        Err(why) => {
            eprintln!("rux: {why}");
            return 2;
        }
    };

    // Built for what is actually plugged in, rather than for the emulator this
    // all started on. It is the answer to the divergence the ABI choice was
    // taken with its eyes open about: develop on x86_64 and ship arm64, until a
    // phone is attached, at which point the build follows the phone.
    let abis = match device_abi(&toolchain, &serial) {
        Ok(abi) => Some(vec![abi]),
        Err(why) => {
            eprintln!("rux: {why}");
            return 2;
        }
    };

    let apk = match crate::build::run(Options {
        release,
        target: Target::Android,
        rux_source,
        abis,
    }) {
        Ok(apk) => apk,
        Err(why) => {
            eprintln!("rux: {why}");
            return 1;
        }
    };

    if let Err(why) = install(&toolchain, &serial, &apk) {
        eprintln!("rux: {why}");
        return 1;
    }
    if let Err(why) = start(&toolchain, &serial, &manifest) {
        eprintln!("rux: {why}");
        return 1;
    }
    println!("rux: {} is running on {serial}", manifest.name);
    0
}

/// The one device to talk to, or why there is not exactly one.
///
/// `adb` defaults to the only device when there is one and fails confusingly
/// when there are two, so the count is checked here and the serial is named on
/// every later call. The failures are the interesting part: "no device" and
/// "which of these three" are different problems with different fixes.
fn one_device(toolchain: &Toolchain) -> Result<String, String> {
    let output = Command::new(&toolchain.adb)
        .args(["devices"])
        .output()
        .map_err(|e| format!("could not run adb: {e}"))?;
    let text = String::from_utf8_lossy(&output.stdout);
    let mut ready = Vec::new();
    let mut offline = Vec::new();
    for line in text.lines().skip(1) {
        let mut parts = line.split_whitespace();
        let (Some(serial), Some(state)) = (parts.next(), parts.next()) else { continue };
        match state {
            "device" => ready.push(serial.to_string()),
            // `unauthorized` is a phone that has not been told to trust this
            // machine, and it is the single most common first-time failure.
            _ => offline.push(format!("{serial} ({state})")),
        }
    }
    match ready.len() {
        1 => Ok(ready.remove(0)),
        0 if offline.is_empty() => Err(
            "no device is attached.\n\n\
             Plug in a phone with USB debugging turned on, or start an emulator with \
             `emulator -avd <name>`. `adb devices` lists what it can see."
                .to_string(),
        ),
        0 => Err(format!(
            "a device is attached but not ready: {}.\n\n\
             `unauthorized` means the phone has not been told to trust this computer yet: \
             unlock it and accept the prompt.",
            offline.join(", ")
        )),
        _ => Err(format!(
            "there is more than one device attached: {}.\n\n\
             Rux does not pick for you. Disconnect the ones you do not mean, or stop the \
             emulators you are not using.",
            ready.join(", ")
        )),
    }
}

/// What this device runs, as an ABI Rux can build.
///
/// `ro.product.cpu.abi` is the device's own primary ABI, which is the one to
/// build: a 64-bit phone will run a 32-bit library, but slower and with a
/// second copy of every system library loaded to do it.
///
/// An architecture Rux has no target for is reported as exactly that. The
/// alternative is building something the device cannot load and letting Android
/// explain it, which it does with `INSTALL_FAILED_NO_MATCHING_ABIS`.
fn device_abi(toolchain: &Toolchain, serial: &str) -> Result<crate::android::Abi, String> {
    let output = Command::new(&toolchain.adb)
        .args(["-s", serial, "shell", "getprop", "ro.product.cpu.abi"])
        .output()
        .map_err(|e| format!("could not run adb: {e}"))?;
    let name = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if name.is_empty() {
        return Err(format!("{serial} did not say what architecture it is"));
    }
    crate::android::Abi::by_android_name(&name).ok_or_else(|| {
        format!(
            "{serial} runs {name}, which Rux does not build for.\n\nRux builds {}.",
            crate::android::ABIS.iter().map(|a| a.name).collect::<Vec<_>>().join(", ")
        )
    })
}

fn install(toolchain: &Toolchain, serial: &str, apk: &Path) -> Result<(), String> {
    println!("rux: installing on {serial}");
    // `-r` reinstalls over a previous build and keeps its data, which is what
    // makes this usable as a loop rather than as a one-off.
    let output = Command::new(&toolchain.adb)
        .args(["-s", serial, "install", "-r"])
        .arg(apk)
        .output()
        .map_err(|e| format!("could not run adb: {e}"))?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    // adb install exits 0 while printing a failure, which is why the text is
    // read rather than the status trusted.
    if output.status.success() && !text.contains("Failure") {
        return Ok(());
    }
    if text.contains("INSTALL_FAILED_UPDATE_INCOMPATIBLE") {
        return Err(format!(
            "the app already on {serial} was signed with a different key.\n\n\
             Uninstall it first: `adb -s {serial} uninstall <id>`"
        ));
    }
    Err(format!("installing failed:\n{}", text.trim()))
}

fn start(toolchain: &Toolchain, serial: &str, manifest: &Manifest) -> Result<(), String> {
    // Taken from the one place that defines it, rather than written out again.
    // It was written out again once, and the result was a build that packaged
    // and installed correctly and then refused to start, because this string
    // and the generated manifest had stopped agreeing.
    let component = format!("{}/{}", manifest.id, crate::apk::ACTIVITY_CLASS);
    let output = Command::new(&toolchain.adb)
        .args(["-s", serial, "shell", "am", "start", "-n", &component])
        .output()
        .map_err(|e| format!("could not run adb: {e}"))?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    if output.status.success() && !text.contains("Error") {
        return Ok(());
    }
    Err(format!("starting it failed:\n{}", text.trim()))
}
