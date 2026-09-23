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

use std::collections::HashMap;
use std::io::Write;
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::process::Command;
use std::sync::mpsc;
use std::time::Duration;

use crate::android::Toolchain;
use crate::build::{Options, Target};
use crate::manifest::Manifest;

pub fn run(args: &[String]) -> i32 {
    let mut rux_source = None;
    let mut release = false;
    let mut watch = true;
    // adb's own variable, so a shell already set up for adb needs nothing new.
    let mut wanted = std::env::var("ANDROID_SERIAL").ok().filter(|s| !s.is_empty());
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--device" => {}
            "--release" => release = true,
            // Install, start and leave, as before hot reload: for a script,
            // or anyone who wants their terminal back.
            "--no-watch" => watch = false,
            "--serial" => match rest.next() {
                Some(value) => wanted = Some(value.clone()),
                None => {
                    eprintln!("rux: `--serial` needs a device, as `adb devices` names it");
                    return 2;
                }
            },
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
    let serial = match one_device(&toolchain, wanted.as_deref()) {
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
    // A release build embeds its documents for good and has no socket to
    // reload through, so there is nothing to stay for.
    if release || !watch {
        return 0;
    }
    match serve(&toolchain, &serial, &manifest) {
        Ok(()) => 0,
        Err(why) => {
            eprintln!("rux: hot reload stopped: {why}");
            1
        }
    }
}

/// Hot reload: watch the project and send the app whatever changed.
///
/// **The app dials in, not the other way round.** `adb reverse` makes this
/// port a port on the device's own loopback, and that is the one direction
/// that works alike on an emulator and a phone over USB or wireless. A
/// forward would need the app to listen, and an app that listens is a server
/// anything on the network could find.
///
/// Every connection starts with the whole project, so an app that was
/// restarted, or built from older files, catches up at once; it reloads only
/// if something differs. After that, each save sends what changed.
///
/// Only documents and assets: a `host::` function is Rust, compiled into the
/// app, and changing one still means running this again.
fn serve(toolchain: &Toolchain, serial: &str, manifest: &Manifest) -> Result<(), String> {
    let port = crate::build::dev_port(manifest);
    let listener = TcpListener::bind(("127.0.0.1", port))
        .map_err(|e| format!("could not listen on port {port}: {e}"))?;
    let spec = format!("tcp:{port}");
    let output = Command::new(&toolchain.adb)
        .args(["-s", serial, "reverse", &spec, &spec])
        .output()
        .map_err(|e| format!("could not run adb: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "adb reverse failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    let (changed_tx, changed) = mpsc::channel();
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        if event.is_ok() {
            let _ = changed_tx.send(());
        }
    })
    .map_err(|e| format!("could not watch the project: {e}"))?;
    use notify::Watcher;
    watcher
        .watch(&manifest.root, notify::RecursiveMode::Recursive)
        .map_err(|e| format!("could not watch {}: {e}", manifest.root.display()))?;

    let (connected_tx, connected) = mpsc::channel();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            if connected_tx.send(stream).is_err() {
                return;
            }
        }
    });

    println!("rux: hot reload is on; save a file and the app reloads. Ctrl+C stops it.");
    let mut app: Option<TcpStream> = None;
    // What the connected app holds, as far as this end knows.
    let mut held: HashMap<String, Vec<u8>> = HashMap::new();
    loop {
        while let Ok(stream) = connected.try_recv() {
            // The newest connection is the app; an older one is an app that
            // has since been restarted.
            app = Some(stream);
            held.clear();
            sync(manifest, &mut app, &mut held);
        }
        match changed.recv_timeout(Duration::from_millis(200)) {
            Ok(()) => {
                // An editor saves in several steps (a temporary file, a
                // rename), so the burst is let finish before anything is read.
                std::thread::sleep(Duration::from_millis(80));
                while changed.try_recv().is_ok() {}
                sync(manifest, &mut app, &mut held);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return Err("the watcher stopped".into()),
        }
    }
}

/// Send the app every file that differs from what it holds, then `reload`.
///
/// The same walk `rux build` embeds from, so what hot reload sends is exactly
/// what the next build would carry. A broken connection drops the app; it
/// dials again on its own.
fn sync(manifest: &Manifest, app: &mut Option<TcpStream>, held: &mut HashMap<String, Vec<u8>>) {
    let Some(stream) = app.as_mut() else { return };
    let Ok(files) = crate::build::collect(&manifest.root) else { return };
    let mut wire = Vec::new();
    let mut now: HashMap<String, Vec<u8>> = HashMap::new();
    for file in &files {
        let Ok(bytes) = std::fs::read(manifest.root.join(file)) else { continue };
        let key = crate::build::key(file);
        if held.get(&key) != Some(&bytes) {
            wire.extend_from_slice(format!("put {} {key}\n", bytes.len()).as_bytes());
            wire.extend_from_slice(&bytes);
        }
        now.insert(key, bytes);
    }
    for gone in held.keys().filter(|k| !now.contains_key(*k)) {
        wire.extend_from_slice(format!("del {gone}\n").as_bytes());
    }
    if wire.is_empty() {
        return;
    }
    let first = held.is_empty();
    wire.extend_from_slice(b"reload\n");
    if stream.write_all(&wire).and_then(|()| stream.flush()).is_err() {
        *app = None;
        return;
    }
    if !first {
        let count = now.iter().filter(|(k, v)| held.get(*k) != Some(*v)).count()
            + held.keys().filter(|k| !now.contains_key(*k)).count();
        println!("rux: sent {count} changed file{}", if count == 1 { "" } else { "s" });
    } else {
        println!("rux: the app is connected");
    }
    *held = now;
}

/// The one device to talk to, or why there is not exactly one.
///
/// `adb` defaults to the only device when there is one and fails confusingly
/// when there are two, so the count is checked here and the serial is named on
/// every later call. The failures are the interesting part: "no device" and
/// "which of these three" are different problems with different fixes.
fn one_device(toolchain: &Toolchain, wanted: Option<&str>) -> Result<String, String> {
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
    if let Some(wanted) = wanted {
        return if ready.iter().any(|s| s == wanted) {
            Ok(wanted.to_string())
        } else {
            Err(format!(
                "`{wanted}` is not an attached device. Attached: {}.",
                if ready.is_empty() { "none".to_string() } else { ready.join(", ") }
            ))
        };
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
        _ => choose(toolchain, &ready),
    }
}

/// More than one device: ask which, when there is someone to ask.
///
/// **Asked, not refused.** A phone on wireless debugging reconnects by
/// itself, so "disconnect the others" was advice that stopped working a
/// minute later. Without a terminal (a script, an editor task) there is no
/// one to ask, so the answer is the list and the flag that names one.
fn choose(toolchain: &Toolchain, ready: &[String]) -> Result<String, String> {
    use std::io::IsTerminal;
    let named: Vec<String> = ready.iter().map(|s| describe(toolchain, s)).collect();
    if !std::io::stdin().is_terminal() {
        return Err(format!(
            "there is more than one device attached:\n{}\n\n\
             Name one with `--serial <serial>`, or set ANDROID_SERIAL.",
            named.iter().map(|n| format!("  {n}")).collect::<Vec<_>>().join("\n")
        ));
    }
    println!("rux: more than one device is attached:");
    for (i, name) in named.iter().enumerate() {
        println!("  {}. {name}", i + 1);
    }
    loop {
        print!("rux: which one? [1-{}] ", ready.len());
        let _ = std::io::stdout().flush();
        let mut answer = String::new();
        if std::io::stdin().read_line(&mut answer).map_or(true, |n| n == 0) {
            return Err("no device was chosen".to_string());
        }
        match answer.trim().parse::<usize>() {
            Ok(n) if (1..=ready.len()).contains(&n) => return Ok(ready[n - 1].clone()),
            _ => println!("rux: a number from 1 to {}, please", ready.len()),
        }
    }
}

/// A device as a person would recognise it: its model, and whether it is an
/// emulator, beside the serial `--serial` takes.
fn describe(toolchain: &Toolchain, serial: &str) -> String {
    let model = Command::new(&toolchain.adb)
        .args(["-s", serial, "shell", "getprop", "ro.product.model"])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|m| !m.is_empty());
    let kind = if serial.starts_with("emulator-") { "emulator" } else { "device" };
    match model {
        Some(model) => format!("{serial}  ({model}, {kind})"),
        None => format!("{serial}  ({kind})"),
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
    // **Started as the launcher starts it**, with its action and category. A
    // bare `-n` makes a task whose root intent the launcher's icon does not
    // match, so tapping the icon later stacked a second, fresh activity on the
    // task instead of bringing it back, and an app Android had killed came back
    // at its first page with its saved state never read.
    let output = Command::new(&toolchain.adb)
        .args([
            "-s",
            serial,
            "shell",
            "am",
            "start",
            "-a",
            "android.intent.action.MAIN",
            "-c",
            "android.intent.category.LAUNCHER",
            "-n",
            &component,
        ])
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
