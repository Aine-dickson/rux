//! `settings.get`, `settings.set` and `settings.remove`: a small store of an
//! app's own per-person state, the same three calls on every target (Mava
//! Studio's MS-007).
//!
//! Kept per app, by the `[app] id` of its `rux.toml` ([`set_app_id`]; a bare
//! `rux run file.rux` uses the file's name):
//!
//! - Windows: `%APPDATA%\<id>\settings.json`
//! - macOS: `~/Library/Application Support/<id>/settings.json`
//! - Linux: `$XDG_DATA_HOME/<id>/settings.json`, or `~/.local/share/<id>/`
//! - Android: the app's own files directory
//! - the web: `localStorage`, every key prefixed with the id
//!
//! **Written through on every `set`**, whole, to a file beside it and then
//! renamed over it, so a crash in the middle leaves the old settings rather
//! than half of the new. Settings are small; a store that wants to be big is
//! a database, which is the app's Rust's business.

use std::collections::BTreeMap;
use std::sync::Mutex;

use rux_runtime::native::{install, Any, Call, Error, Module};

static APP_ID: Mutex<Option<String>> = Mutex::new(None);

/// Which app this process is, for where its settings live: the `[app] id` of
/// its `rux.toml`. Set before the first document loads; the first one wins.
pub fn set_app_id(id: &str) {
    let mut slot = APP_ID.lock().unwrap_or_else(|e| e.into_inner());
    if slot.is_none() && !id.trim().is_empty() {
        *slot = Some(id.trim().to_string());
    }
}

/// The app's id, or `fallback` (a bare file's name) when nothing set one.
/// Only the desktop's `run_windowed` runs a bare file.
#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
pub(crate) fn app_id_or(fallback: &str) -> String {
    set_app_id(fallback);
    app_id()
}

fn app_id() -> String {
    APP_ID.lock().unwrap_or_else(|e| e.into_inner()).clone().unwrap_or_else(|| "rux".to_string())
}

/// Install the three calls for every document this process loads.
pub(crate) fn install_settings() {
    let interface = rux_runtime::builtins::settings_interface();
    let mut calls = std::collections::HashMap::new();
    calls.insert(
        "get".to_string(),
        Call::sync(|args: Vec<Any>| {
            let key = key_of(&args)?;
            Ok(store::read().get(&key).cloned().map(from_json).unwrap_or(Any::None))
        }),
    );
    calls.insert(
        "set".to_string(),
        Call::sync(|mut args: Vec<Any>| {
            let key = key_of(&args)?;
            let value = to_json(args.pop().unwrap_or_default())?;
            let mut all = store::read();
            all.insert(key, value);
            store::write(&all)?;
            Ok(Any::None)
        }),
    );
    calls.insert(
        "remove".to_string(),
        Call::sync(|args: Vec<Any>| {
            let key = key_of(&args)?;
            let mut all = store::read();
            if all.remove(&key).is_some() {
                store::write(&all)?;
            }
            Ok(Any::None)
        }),
    );
    let _ = install(Module { interface, calls });
}

fn key_of(args: &[Any]) -> Result<String, Error> {
    match args.first() {
        Some(Any::Str(s)) => Ok(s.clone()),
        Some(other) => Err(Error::wrong("a string key", other)),
        None => Err(Error::new("type", "settings are kept by a string key")),
    }
}

/// A value on its way to being kept. What only lives in a running program (a
/// resource) cannot be.
fn to_json(a: Any) -> Result<serde_json::Value, Error> {
    use serde_json::Value as J;
    Ok(match a {
        Any::None => J::Null,
        Any::Bool(b) => J::Bool(b),
        Any::Int(i) => J::from(i),
        Any::Float(f) => serde_json::Number::from_f64(f).map_or(J::Null, J::Number),
        Any::Str(s) => J::String(s),
        Any::Array(items) => J::Array(items.into_iter().map(to_json).collect::<Result<_, _>>()?),
        Any::Map(m) => J::Object(m.into_iter().map(|(k, v)| Ok((k, to_json(v)?))).collect::<Result<_, Error>>()?),
        Any::Resource(_) => {
            return Err(Error::new("type", "a resource lives only while the app runs, so it cannot be kept in settings"))
        }
    })
}

/// A kept value, back. An integer comes back an `int`.
fn from_json(j: serde_json::Value) -> Any {
    use serde_json::Value as J;
    match j {
        J::Null => Any::None,
        J::Bool(b) => Any::Bool(b),
        J::Number(n) => n.as_i64().map(Any::Int).unwrap_or_else(|| Any::Float(n.as_f64().unwrap_or(0.0))),
        J::String(s) => Any::Str(s),
        J::Array(items) => Any::Array(items.into_iter().map(from_json).collect()),
        J::Object(m) => Any::Map(m.into_iter().map(|(k, v)| (k, from_json(v))).collect()),
    }
}

type All = BTreeMap<String, serde_json::Value>;

/// Where the settings live: one JSON file per app.
#[cfg(not(target_arch = "wasm32"))]
mod store {
    use super::*;
    use std::path::PathBuf;

    static DIR: Mutex<Option<PathBuf>> = Mutex::new(None);

    /// Android's files directory, which only the activity knows.
    #[cfg_attr(not(target_os = "android"), allow(dead_code))]
    pub(crate) fn set_dir(dir: PathBuf) {
        *DIR.lock().unwrap_or_else(|e| e.into_inner()) = Some(dir);
    }

    fn file() -> Option<PathBuf> {
        if let Some(dir) = DIR.lock().unwrap_or_else(|e| e.into_inner()).clone() {
            return Some(dir.join("settings.json"));
        }
        let env = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
        let base = if cfg!(windows) {
            env("APPDATA")
        } else if cfg!(target_os = "macos") {
            env("HOME").map(|h| h.join("Library").join("Application Support"))
        } else {
            env("XDG_DATA_HOME").or_else(|| env("HOME").map(|h| h.join(".local").join("share")))
        }?;
        Some(base.join(app_id()).join("settings.json"))
    }

    pub(crate) fn read() -> All {
        file()
            .and_then(|f| std::fs::read_to_string(f).ok())
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub(crate) fn write(all: &All) -> Result<(), Error> {
        let file = file().ok_or_else(|| Error::new("io", "there is nowhere to keep settings: no home folder"))?;
        let text = serde_json::to_string_pretty(all).map_err(Error::from_rust)?;
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir).map_err(Error::from_rust)?;
        }
        let part = file.with_extension("json.part");
        std::fs::write(&part, text).map_err(Error::from_rust)?;
        std::fs::rename(&part, &file).map_err(Error::from_rust)
    }
}

/// Where the settings live: the page's `localStorage`, one entry per key.
#[cfg(target_arch = "wasm32")]
mod store {
    use super::*;

    fn storage() -> Option<web_sys::Storage> {
        web_sys::window()?.local_storage().ok().flatten()
    }

    fn prefix() -> String {
        format!("rux:settings:{}:", app_id())
    }

    pub(crate) fn read() -> All {
        let mut all = All::new();
        let Some(s) = storage() else { return all };
        let prefix = prefix();
        let n = s.length().unwrap_or(0);
        for i in 0..n {
            let Some(k) = s.key(i).ok().flatten() else { continue };
            let Some(name) = k.strip_prefix(&prefix) else { continue };
            if let Some(v) = s.get_item(&k).ok().flatten().and_then(|t| serde_json::from_str(&t).ok()) {
                all.insert(name.to_string(), v);
            }
        }
        all
    }

    pub(crate) fn write(all: &All) -> Result<(), Error> {
        let s = storage().ok_or_else(|| Error::new("io", "this page may not keep anything: localStorage is off"))?;
        let prefix = prefix();
        let n = s.length().unwrap_or(0);
        let gone: Vec<String> = (0..n)
            .filter_map(|i| s.key(i).ok().flatten())
            .filter(|k| k.strip_prefix(&prefix).is_some_and(|name| !all.contains_key(name)))
            .collect();
        for k in gone {
            let _ = s.remove_item(&k);
        }
        for (k, v) in all {
            let text = serde_json::to_string(v).map_err(Error::from_rust)?;
            s.set_item(&format!("{prefix}{k}"), &text)
                .map_err(|_| Error::new("io", "localStorage refused it: full, or a private window"))?;
        }
        Ok(())
    }
}

#[cfg(target_os = "android")]
pub(crate) use store::set_dir;

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    #[test]
    fn values_round_trip_and_an_int_stays_one() {
        let v = Any::Array(vec![Any::Int(3), Any::Float(1.5), Any::Str("a".into()), Any::None, Any::Bool(true)]);
        let back = from_json(to_json(v.clone()).unwrap());
        assert_eq!(back, v);
    }

    #[test]
    fn set_get_and_remove_keep_to_a_file() {
        let dir = std::env::temp_dir().join(format!("rux_settings_{}", std::process::id()));
        store::set_dir(dir.clone());
        let mut all = store::read();
        all.insert("recent".into(), to_json(Any::Array(vec![Any::Str("a.mava".into())])).unwrap());
        store::write(&all).unwrap();
        assert_eq!(from_json(store::read()["recent"].clone()), Any::Array(vec![Any::Str("a.mava".into())]));
        assert!(!dir.join("settings.json.part").exists(), "the part file was renamed over");
        let _ = std::fs::remove_dir_all(dir);
    }
}
