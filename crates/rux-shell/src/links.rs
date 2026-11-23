//! Handing over to other apps: `links.open`, `links.dial`, `links.sms`,
//! `links.directions`, `links.share` and `links.canOpen`, and the same calls
//! for the app's own Rust (`rux::links`).
//!
//! What may open, and how a number, a text and a place become a URL, is
//! `rux_native::links` (so the app's Rust holds to the same rules). This
//! module reads script's arguments and hands the platform a checked URL.
//!
//! The platforms: the desktop opens through the operating system's own verb.
//! Android and the web are built after it (UD-005 steps 2 and 3); until then
//! they throw `unsupported`.

use std::collections::{BTreeMap, HashMap};

use rux_runtime::native::links::{self, set_backend, Backend, Directions, Share};
use rux_runtime::native::{install, Any, Call, Error, Module};

fn option_error(message: String) -> Error {
    Error::new("option", message)
}

fn text_arg(call: &str, what: &str, arg: Option<&Any>) -> Result<String, Error> {
    match arg {
        Some(Any::Str(s)) => Ok(s.clone()),
        Some(other) => Err(Error::new("argument", format!("`links.{call}` takes {what} as text, not a {}", other.kind()))),
        None => Err(Error::new("argument", format!("`links.{call}` needs {what}"))),
    }
}

fn map_arg(call: &str, example: &str, args: Vec<Any>, allowed: &[&str]) -> Result<BTreeMap<String, Any>, Error> {
    let map = match args.into_iter().next() {
        Some(Any::Map(m)) => m,
        Some(other) => {
            return Err(option_error(format!("links.{call} takes its options as a map, not a {}", other.kind())))
        }
        None => return Err(option_error(format!("links.{call} takes its options: {example}"))),
    };
    for key in map.keys() {
        if !allowed.contains(&key.as_str()) {
            return Err(option_error(format!(
                "links.{call} has no option `{key}`. Its options are {}",
                allowed.join(", ")
            )));
        }
    }
    Ok(map)
}

fn optional_text(map: &BTreeMap<String, Any>, key: &str) -> Result<String, Error> {
    match map.get(key) {
        Some(Any::Str(s)) => Ok(s.clone()),
        Some(Any::None) | None => Ok(String::new()),
        Some(other) => Err(option_error(format!("`{key}` takes text, not a {}", other.kind()))),
    }
}

/// A `links.directions` call's options.
fn directions_from(args: Vec<Any>) -> Result<Directions, Error> {
    let map = map_arg(
        "directions",
        "links.directions({ lat: 0.33, lng: 32.6, label: \"Pickup\" })",
        args,
        &["lat", "lng", "label"],
    )?;
    let number = |key: &str| match map.get(key) {
        Some(Any::Int(n)) => Ok(*n as f64),
        Some(Any::Float(f)) => Ok(*f),
        Some(other) => Err(option_error(format!("`{key}` takes a number, not a {}", other.kind()))),
        None => Err(option_error(format!("links.directions needs a `{key}`"))),
    };
    Ok(Directions { lat: number("lat")?, lng: number("lng")?, label: optional_text(&map, "label")? })
}

/// A `links.share` call's options.
fn share_from(args: Vec<Any>) -> Result<Share, Error> {
    let map = map_arg("share", "links.share({ title: \"Track it\", text: \"https://track.example/42\" })", args, &["title", "text"])?;
    Ok(Share { title: optional_text(&map, "title")?, text: optional_text(&map, "text")? })
}

/// Install the calls for every document this process loads, and the same
/// for the app's own Rust (`rux::links`).
pub(crate) fn install_links() {
    set_backend(Backend {
        open: platform::open,
        directions: platform::directions,
        share: platform::share,
        can_open: platform::can_open,
    });
    let mut calls = HashMap::new();
    calls.insert(
        "open".to_string(),
        Call::sync(|args: Vec<Any>| {
            links::open(&text_arg("open", "a URL", args.first())?)?;
            Ok(Any::None)
        }),
    );
    calls.insert(
        "dial".to_string(),
        Call::sync(|args: Vec<Any>| {
            links::dial(&text_arg("dial", "a phone number", args.first())?)?;
            Ok(Any::None)
        }),
    );
    calls.insert(
        "sms".to_string(),
        Call::sync(|args: Vec<Any>| {
            let number = text_arg("sms", "a phone number", args.first())?;
            let text = text_arg("sms", "the message text", args.get(1))?;
            links::sms(&number, &text)?;
            Ok(Any::None)
        }),
    );
    calls.insert(
        "directions".to_string(),
        Call::sync(|args: Vec<Any>| {
            links::directions(directions_from(args)?)?;
            Ok(Any::None)
        }),
    );
    calls.insert(
        "share".to_string(),
        Call::sync(|args: Vec<Any>| {
            links::share(share_from(args)?)?;
            Ok(Any::None)
        }),
    );
    calls.insert(
        "canOpen".to_string(),
        Call::sync(|args: Vec<Any>| Ok(Any::Bool(links::can_open(&text_arg("canOpen", "a URL", args.first())?)?))),
    );
    let _ = install(Module { interface: rux_runtime::builtins::links_interface(), calls });
}

#[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
mod platform {
    use super::*;

    /// Give `url` to the operating system's own opener, as one argument: no
    /// shell reads it.
    pub(super) fn open(url: &str) -> Result<(), Error> {
        open_with_os(url)
    }

    #[cfg(windows)]
    fn open_with_os(url: &str) -> Result<(), Error> {
        use windows_sys::Win32::UI::Shell::ShellExecuteW;
        use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
        let wide: Vec<u16> = url.encode_utf16().chain(std::iter::once(0)).collect();
        let verb: Vec<u16> = "open".encode_utf16().chain(std::iter::once(0)).collect();
        // Safety: both strings are NUL-terminated and outlive the call; the
        // other arguments are null, which the function documents as "none".
        let result = unsafe {
            ShellExecuteW(std::ptr::null_mut(), verb.as_ptr(), wide.as_ptr(), std::ptr::null(), std::ptr::null(), SW_SHOWNORMAL)
        };
        // Greater than 32 is success; a smaller value is an error code.
        if (result as usize) > 32 {
            Ok(())
        } else {
            Err(Error::new("notfound", "nothing on this computer handles that link"))
        }
    }

    #[cfg(not(windows))]
    fn open_with_os(url: &str) -> Result<(), Error> {
        let program = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
        std::process::Command::new(program)
            .arg(url)
            .spawn()
            .map(|_| ())
            .map_err(|_| Error::new("notfound", format!("`{program}` is not available here, so nothing can open the link")))
    }

    pub(super) fn directions(to: &Directions) -> Result<(), Error> {
        open_with_os(&to.web_url())
    }

    pub(super) fn share(_what: &Share) -> Result<(), Error> {
        Err(Error::new("unsupported", "a desktop has no share sheet: copy the text instead"))
    }

    pub(super) fn can_open(_url: &str) -> bool {
        // The desktop cannot ask the registry cheaply; the open itself says
        // `notfound` where nothing handles it.
        true
    }
}

#[cfg(any(target_os = "android", target_arch = "wasm32"))]
mod platform {
    use super::*;

    fn unsupported() -> Error {
        Error::new("unsupported", "this build cannot open other apps yet")
    }

    pub(super) fn open(_url: &str) -> Result<(), Error> {
        Err(unsupported())
    }

    pub(super) fn directions(_to: &Directions) -> Result<(), Error> {
        Err(unsupported())
    }

    pub(super) fn share(_what: &Share) -> Result<(), Error> {
        Err(unsupported())
    }

    pub(super) fn can_open(_url: &str) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(pairs: &[(&str, Any)]) -> Vec<Any> {
        vec![Any::Map(pairs.iter().map(|(k, v)| ((*k).to_string(), v.clone())).collect())]
    }

    #[test]
    fn directions_and_share_options_are_read_or_refused_in_their_own_words() {
        let d = directions_from(map(&[("lat", Any::Float(0.33)), ("lng", Any::Int(32)), ("label", Any::Str("Pickup".into()))])).unwrap();
        assert_eq!((d.lat, d.lng, d.label.as_str()), (0.33, 32.0, "Pickup"));
        assert_eq!(directions_from(map(&[("lat", Any::Float(0.33))])).unwrap_err().message, "links.directions needs a `lng`");
        assert!(directions_from(map(&[("lat", Any::Str("x".into())), ("lng", Any::Int(1))])).unwrap_err().message.contains("takes a number"));
        assert!(directions_from(map(&[("lat", Any::Int(1)), ("lng", Any::Int(1)), ("zoom", Any::Int(3))])).unwrap_err().message.contains("no option `zoom`"));
        assert!(directions_from(vec![]).unwrap_err().message.contains("takes its options"));

        let s = share_from(map(&[("text", Any::Str("hi".into()))])).unwrap();
        assert_eq!((s.title.as_str(), s.text.as_str()), ("", "hi"));
        assert!(share_from(map(&[("body", Any::Str("hi".into()))])).unwrap_err().message.contains("no option `body`"));
        assert_eq!(text_arg("open", "a URL", Some(&Any::Int(3))).unwrap_err().kind, "argument");
    }
}
