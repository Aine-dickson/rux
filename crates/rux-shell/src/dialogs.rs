//! The file dialogs a script awaits: `pickFile`, `pickFiles`,
//! `pickSaveFile` (Mava Studio's MS-005).
//!
//! Installed as Rux's own native module, [`rux_runtime::builtins`]'s
//! interface with code behind it, so a script calls them by bare name and
//! awaits them like any `async` native export: the checker, the `await` and
//! the "an async call must be awaited" error are the native path's own.
//!
//! **The platform's own dialog, and it does not block.** `rfd`'s async
//! dialog runs it where the platform wants it run (a thread of its own on
//! Windows, the desktop portal on Linux, so no GTK), the window keeps
//! painting, and the path comes back as an `await`'s answer does.
//!
//! On the web and on Android a dialog gives no path (a browser gives the
//! contents, Android a `content://` address), so there each one throws an
//! `Error` of `kind` `"unsupported"` until those are built.

use rux_runtime::native::{install, Any, Call, Error, Module};

#[derive(Clone, Copy)]
enum Pick {
    File,
    Files,
    Save,
}

/// Install the dialogs for every document this process loads.
pub(crate) fn install_builtins() {
    let interface = rux_runtime::builtins::builtin_interface();
    let mut calls = std::collections::HashMap::new();
    for (name, kind) in [("pickFile", Pick::File), ("pickFiles", Pick::Files), ("pickSaveFile", Pick::Save)] {
        calls.insert(
            name.to_string(),
            Call::future(move |mut args: Vec<Any>| {
                let options = args.pop().unwrap_or_default();
                pick(kind, options)
            }),
        );
    }
    calls.extend(crate::files::calls());
    // Twice (a second window, a reload) is harmless: the same code again.
    let _ = install(Module { interface, calls });
}

/// What the options record asked for. Every field may be left out.
#[cfg_attr(any(target_arch = "wasm32", target_os = "android"), allow(dead_code))]
#[derive(Default)]
struct Options {
    title: Option<String>,
    filters: Vec<(String, Vec<String>)>,
    folder: Option<String>,
    name: Option<String>,
}

#[cfg_attr(any(target_arch = "wasm32", target_os = "android"), allow(dead_code))]
fn options(a: Any) -> Options {
    let Any::Map(m) = a else { return Options::default() };
    let text = |k: &str| match m.get(k) {
        Some(Any::Str(s)) => Some(s.clone()),
        _ => None,
    };
    let filters = match m.get("filters") {
        Some(Any::Array(fs)) => fs
            .iter()
            .filter_map(|f| {
                let Any::Map(f) = f else { return None };
                let name = match f.get("name") {
                    Some(Any::Str(s)) => s.clone(),
                    _ => String::new(),
                };
                let exts = match f.get("extensions") {
                    Some(Any::Array(es)) => es
                        .iter()
                        .filter_map(|e| match e {
                            // `".png"` and `"png"` alike.
                            Any::Str(s) => Some(s.trim_start_matches('.').to_string()),
                            _ => None,
                        })
                        .collect(),
                    _ => Vec::new(),
                };
                Some((name, exts))
            })
            .collect(),
        _ => Vec::new(),
    };
    Options { title: text("title"), filters, folder: text("folder"), name: text("name") }
}

#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
async fn pick(kind: Pick, given: Any) -> Result<Any, Error> {
    let o = options(given);
    let mut d = rfd::AsyncFileDialog::new();
    if let Some(t) = &o.title {
        d = d.set_title(t);
    }
    for (name, exts) in &o.filters {
        d = d.add_filter(name, exts);
    }
    if let Some(f) = &o.folder {
        d = d.set_directory(f);
    }
    let path = |h: &rfd::FileHandle| Any::Str(h.path().to_string_lossy().into_owned());
    Ok(match kind {
        Pick::File => d.pick_file().await.map_or(Any::None, |h| path(&h)),
        Pick::Files => Any::Array(d.pick_files().await.unwrap_or_default().iter().map(path).collect()),
        Pick::Save => {
            if let Some(n) = &o.name {
                d = d.set_file_name(n);
            }
            d.save_file().await.map_or(Any::None, |h| path(&h))
        }
    })
}

#[cfg(any(target_arch = "wasm32", target_os = "android"))]
async fn pick(kind: Pick, _given: Any) -> Result<Any, Error> {
    let name = match kind {
        Pick::File => "pickFile",
        Pick::Files => "pickFiles",
        Pick::Save => "pickSaveFile",
    };
    let here = if cfg!(target_os = "android") { "on Android" } else { "on the web" };
    Err(Error::new("unsupported", format!("`{name}` is not built {here} yet: it gives a path, and a path is a desktop's")))
}
