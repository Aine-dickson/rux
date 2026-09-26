//! The app's `native/` crate: step 8.4 of `docs/11-next.md`.
//!
//! An app keeps its Rust in `native/` beside `rux.toml`, an ordinary library
//! crate depending on `rux-native` as `rux`. The CLI reads its source with
//! `rux-bindgen`, never builds it to learn anything:
//!
//! - `rux check` declares every export's signature, so documents importing
//!   `native::…` are checked against the Rust without waiting for `cargo`;
//! - `rux build` and `rux run` put the crate in the generated wrapper and
//!   register every export, by the Rust path the scan found, before the
//!   first document loads;
//! - `rux native` prints what Rux sees.

use std::path::{Path, PathBuf};

use rux_bindgen::{Found, Scan};
use rux_runtime::native::{registry, FieldSig, Interface, Item, ItemKind, Sig};

/// The directory an app's Rust lives in, beside `rux.toml`.
pub const DIR: &str = "native";

/// An app's native crate.
#[derive(Clone, Debug)]
pub struct NativeCrate {
    /// `…/native`.
    pub dir: PathBuf,
    /// Its library's root file, `src/lib.rs` unless `[lib] path` says.
    pub lib: PathBuf,
    /// Its package name, as a dependency is written.
    pub package: String,
    /// Its library's name as Rust code names it: `app_native`.
    pub ident: String,
}

/// The native crate of the project at `root`, when it has one.
pub fn find(root: &Path) -> Result<Option<NativeCrate>, String> {
    let dir = root.join(DIR);
    let manifest = dir.join("Cargo.toml");
    if !manifest.is_file() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&manifest).map_err(|e| format!("reading {}: {e}", manifest.display()))?;
    let value: toml::Table = text.parse().map_err(|e| format!("{}: {e}", manifest.display()))?;
    let package = value
        .get("package")
        .and_then(|p| p.get("name"))
        .and_then(|n| n.as_str())
        .ok_or_else(|| format!("{} has no [package] name", manifest.display()))?
        .to_string();
    let lib = value.get("lib");
    let ident = lib
        .and_then(|l| l.get("name"))
        .and_then(|n| n.as_str())
        .map(str::to_string)
        .unwrap_or_else(|| package.replace('-', "_"));
    let lib_path = lib.and_then(|l| l.get("path")).and_then(|p| p.as_str()).unwrap_or("src/lib.rs");
    Ok(Some(NativeCrate { lib: dir.join(lib_path), dir, package, ident }))
}

/// The project a file belongs to, for its native crate: the nearest
/// directory above it holding `native/Cargo.toml`.
pub fn find_above(file: &Path) -> Option<NativeCrate> {
    let start = if file.is_dir() { file.to_path_buf() } else { file.parent()?.to_path_buf() };
    let start = std::fs::canonicalize(&start).map(plain).unwrap_or(start);
    start.ancestors().find_map(|dir| find(dir).ok().flatten())
}

/// A canonical path without Windows' `\\?\` prefix, which cargo cannot read
/// in a dependency's path.
fn plain(p: PathBuf) -> PathBuf {
    match p.to_str().and_then(|s| s.strip_prefix(r"\\?\")) {
        Some(rest) if !rest.starts_with("UNC") => PathBuf::from(rest),
        _ => p,
    }
}

/// What the scan found, as the registry holds it.
pub fn interfaces(scan: &Scan) -> Vec<Interface> {
    scan.modules
        .iter()
        .map(|m| Interface {
            path: m.path.clone(),
            items: m
                .items
                .iter()
                .map(|f| match f {
                    Found::Fn { name, params, result, is_async } => Item {
                        name: name.clone(),
                        kind: ItemKind::Fn(Sig { params: params.clone(), result: result.clone(), is_async: *is_async }),
                    },
                    Found::Method { on, name, params, result, is_async } => Item {
                        name: name.clone(),
                        kind: ItemKind::Method {
                            on: on.clone(),
                            sig: Sig { params: params.clone(), result: result.clone(), is_async: *is_async },
                        },
                    },
                    Found::Record { name, fields } => Item {
                        name: name.clone(),
                        kind: ItemKind::Record(
                            fields
                                .iter()
                                .map(|(n, t, o)| FieldSig { name: n.clone(), ty: t.clone(), optional: *o })
                                .collect(),
                        ),
                    },
                    Found::Resource { name } => Item { name: name.clone(), kind: ItemKind::Resource },
                })
                .collect(),
        })
        .collect()
}

/// A problem in the Rust, as `rux check` reports one.
pub fn diagnostics(scan: &Scan) -> Vec<crate::check::Diagnostic> {
    scan.problems
        .iter()
        .map(|p| crate::check::Diagnostic {
            file: p.file.clone(),
            line: (p.line > 0).then_some(p.line),
            column: None,
            severity: crate::check::Severity::Error,
            message: p.message.clone(),
        })
        .collect()
}

/// Read the native crate of every project `files` belong to, declare what
/// each exports for the checker, and give back what stops an export.
pub fn declare_for(files: &[PathBuf]) -> Vec<crate::check::Diagnostic> {
    registry::forget_declared();
    let mut seen: Vec<PathBuf> = Vec::new();
    let mut out = Vec::new();
    for file in files {
        let Some(c) = find_above(file) else { continue };
        if seen.contains(&c.dir) {
            continue;
        }
        seen.push(c.dir.clone());
        let scan = rux_bindgen::scan(&c.lib);
        out.extend(diagnostics(&scan));
        for i in interfaces(&scan) {
            if let Err(e) = registry::declare(i) {
                out.push(crate::check::Diagnostic {
                    file: c.lib.clone(),
                    line: None,
                    column: None,
                    severity: crate::check::Severity::Error,
                    message: e,
                });
            }
        }
    }
    out
}

/// The dependency line the wrapper crate gets for the native crate.
pub fn dependency(c: &NativeCrate) -> String {
    let dir = c.dir.display().to_string().replace('\\', "/");
    format!("{} = {{ path = \"{}\" }}\n", c.package, dir.replace('"', "\\\""))
}

/// The Rust that registers every export, run first in the wrapper's entry:
/// each `#[rux::init]`, then one module per Rust module, each export by the
/// path the scan found. Refuses when the scan found a problem, which would
/// otherwise surface as a compile error in code nobody wrote.
pub fn registration(c: &NativeCrate, scan: &Scan) -> Result<String, String> {
    if let Some(p) = scan.problems.first() {
        return Err(format!(
            "{}:{}: {}{}",
            p.file.display(),
            p.line,
            p.message,
            if scan.problems.len() > 1 { format!(" (and {} more; `rux check` lists them)", scan.problems.len() - 1) } else { String::new() }
        ));
    }
    let mut out = String::from("    // The app's native/ crate: every #[rux::init], then every export.\n");
    for init in &scan.inits {
        out.push_str(&format!("    {}::{init}();\n", c.ident));
    }
    for m in &scan.modules {
        out.push_str("    {\n");
        out.push_str(&format!("        let mut m = rux_runtime::native::Module::new({:?});\n", m.path));
        for reg in &m.registrations {
            out.push_str(&format!("        for e in {}::{reg}() {{ m.add(e); }}\n", c.ident));
        }
        out.push_str("        m.install();\n    }\n");
    }
    Ok(out)
}

/// Where the Rux crates come from for an app's own build. A released `rux`
/// names published versions; one built from a checkout (a `-dev` version,
/// which crates.io never has) names the checkout it was built from, when it
/// is still there.
pub fn default_rux_source() -> Option<PathBuf> {
    if !env!("CARGO_PKG_VERSION").ends_with("-dev") {
        return None;
    }
    let checkout = Path::new(env!("CARGO_MANIFEST_DIR")).parent()?.parent()?.to_path_buf();
    checkout.join("crates").join("rux-shell").is_dir().then_some(checkout)
}

/// `rux run` for an app with a `native/` crate: build it as a dev build (its
/// documents are read from disk and hot reload as ever), run it, and build and
/// run it again whenever its Rust changes.
pub fn run(entry: &Path, c: &NativeCrate) -> std::process::ExitCode {
    use notify::Watcher;
    use std::process::ExitCode;
    use std::sync::mpsc;
    use std::time::Duration;

    let root = c.dir.parent().unwrap_or(Path::new(".")).to_path_buf();
    if let Err(e) = std::env::set_current_dir(&root) {
        eprintln!("rux: cannot work in {}: {e}", root.display());
        return ExitCode::from(2);
    }
    let _ = entry;
    let (tx, rx) = mpsc::channel::<()>();
    let mut watcher = match notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        let Ok(event) = event else { return };
        let rust = event.paths.iter().any(|p| {
            !p.components().any(|c| c.as_os_str() == "target")
                && matches!(p.extension().and_then(|e| e.to_str()), Some("rs" | "toml"))
        });
        if rust && (event.kind.is_modify() || event.kind.is_create() || event.kind.is_remove()) {
            let _ = tx.send(());
        }
    }) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("rux: cannot watch {}: {e}", c.dir.display());
            return ExitCode::from(2);
        }
    };
    if let Err(e) = watcher.watch(&c.dir, notify::RecursiveMode::Recursive) {
        eprintln!("rux: cannot watch {}: {e}", c.dir.display());
        return ExitCode::from(2);
    }
    let wait_for_change = |rx: &mpsc::Receiver<()>| {
        let _ = rx.recv();
        // A save is often several events; take them as one.
        std::thread::sleep(Duration::from_millis(150));
        while rx.try_recv().is_ok() {}
    };
    loop {
        let options = crate::build::Options {
            release: false,
            target: crate::build::Target::Desktop,
            rux_source: default_rux_source(),
            abis: None,
        };
        eprintln!("rux: building the app with its native/ crate");
        let exe = match crate::build::run(options) {
            Ok(exe) => exe,
            Err(e) => {
                eprintln!("rux: {e}\nrux: waiting for a change under {}", c.dir.display());
                wait_for_change(&rx);
                continue;
            }
        };
        let mut child = match std::process::Command::new(&exe).spawn() {
            Ok(child) => child,
            Err(e) => {
                eprintln!("rux: could not start {}: {e}", exe.display());
                return ExitCode::from(1);
            }
        };
        while rx.try_recv().is_ok() {}
        loop {
            if let Ok(Some(status)) = child.try_wait() {
                return ExitCode::from(u8::try_from(status.code().unwrap_or(1)).unwrap_or(1));
            }
            if rx.recv_timeout(Duration::from_millis(200)).is_ok() {
                std::thread::sleep(Duration::from_millis(150));
                while rx.try_recv().is_ok() {}
                eprintln!("rux: {} changed; rebuilding", c.dir.display());
                let _ = child.kill();
                let _ = child.wait();
                break;
            }
        }
    }
}

/// `rux native [dir]`: what Rux sees of the project's Rust.
pub fn print(args: &[String]) -> i32 {
    let root = args.first().map(PathBuf::from).unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
    let Some(c) = find_above(&root.join("x")) else {
        eprintln!("rux: no {DIR}/Cargo.toml in {} or above it", root.display());
        return 2;
    };
    let scan = rux_bindgen::scan(&c.lib);
    for i in interfaces(&scan) {
        println!("{}", i.render());
    }
    for d in diagnostics(&scan) {
        eprintln!("{}:{}: error: {}", d.file.display(), d.line.unwrap_or(0), d.message);
    }
    i32::from(!scan.problems.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_crate_is_found_and_registered_by_path() {
        let dir = std::env::temp_dir().join(format!("rux-cli-native-{}", std::process::id()));
        let src = dir.join(DIR).join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(dir.join(DIR).join("Cargo.toml"), "[package]\nname = \"app-native\"\n").unwrap();
        std::fs::write(
            src.join("lib.rs"),
            "pub mod shop;\n#[rux::init]\npub fn setup() {}\n",
        )
        .unwrap();
        std::fs::write(src.join("shop.rs"), "#[rux::export]\npub fn total(a: f64) -> f64 { a }\n").unwrap();
        let c = find(&dir).unwrap().unwrap();
        assert_eq!(c.ident, "app_native");
        let scan = rux_bindgen::scan(&c.lib);
        let code = registration(&c, &scan).unwrap();
        assert!(code.contains("app_native::setup();"), "{code}");
        assert!(code.contains("Module::new(\"shop\")"), "{code}");
        assert!(code.contains("for e in app_native::shop::__rux_export_total() { m.add(e); }"), "{code}");
        assert!(find_above(&dir.join("app.rux")).is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
