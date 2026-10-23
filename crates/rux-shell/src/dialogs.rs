//! The file dialogs a script awaits: `pickFile`, `pickFiles`,
//! `pickSaveFile` (Mava Studio's MS-005). The first two give `File`s, as a
//! file input does (`crate::files`); a save dialog gives a path.
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
//! On the web and on Android `pickFile` and `pickFiles` open the picker a
//! file input opens, and give the same `File`s it does, with no `path`.
//! `pickSaveFile` throws there: neither has a save dialog that gives a path.

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
    #[cfg(not(target_arch = "wasm32"))]
    calls.extend(crate::sound::calls());
    // A browser plays a sound itself, through its own audio element.
    #[cfg(target_arch = "wasm32")]
    calls.extend(crate::web_sound::calls());
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
    let file = |h: &rfd::FileHandle| crate::files::from_path(h.path());
    Ok(match kind {
        Pick::File => d.pick_file().await.and_then(|h| file(&h)).unwrap_or(Any::None),
        Pick::Files => Any::Array(d.pick_files().await.unwrap_or_default().iter().filter_map(file).collect()),
        Pick::Save => {
            if let Some(n) = &o.name {
                d = d.set_file_name(n);
            }
            d.save_file().await.map_or(Any::None, |h| path(&h))
        }
    })
}

/// On the web and Android, the picker a file input opens (the browser's,
/// the system's document picker), its answer through a
/// [`crate::files::Slot`]. `filters` become `accept`; `title` and `folder`
/// have nothing to set there. A save dialog has no counterpart that gives a
/// path, so `pickSaveFile` throws.
#[cfg(any(target_arch = "wasm32", target_os = "android"))]
fn pick(kind: Pick, given: Any) -> crate::files::Slot {
    use crate::files::Slot;
    let here = if cfg!(target_os = "android") { "on Android" } else { "on the web" };
    let multiple = match kind {
        Pick::File => false,
        Pick::Files => true,
        Pick::Save => {
            return Slot::failed(Error::new("unsupported", format!("`pickSaveFile` gives a path, and there is none {here}")))
        }
    };
    let accept = options(given)
        .filters
        .iter()
        .flat_map(|(_, exts)| exts.iter().map(|e| format!(".{e}")))
        .collect::<Vec<_>>()
        .join(",");
    // A browser opens a picker only during a person's gesture, and one that
    // is refused never answers: an error now says why instead.
    #[cfg(target_arch = "wasm32")]
    if !crate::web_user_activation() {
        let name = if multiple { "pickFiles" } else { "pickFile" };        return Slot::failed(Error::new(
            "gesture",
            format!("`{name}` opens the browser's picker only from a tap: call it in a tap handler, before any `await`"),
        ));
    }
    let slot = match crate::files::script_pick(multiple) {
        Ok(slot) => slot,
        Err(e) => return Slot::failed(e),
    };
    #[cfg(target_arch = "wasm32")]
    crate::web_open_files(&accept, multiple, None);
    #[cfg(target_os = "android")]
    crate::android_pick_files(&crate::files::accept_mime_types(&accept), multiple, None);
    slot
}
