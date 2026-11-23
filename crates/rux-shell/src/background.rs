//! Background work: `background.start`, `background.stop` and
//! `background.active` in script, and `rux::background` in the app's Rust.
//!
//! On Android the app's process is kept alive by a foreground service
//! (`RuxActivity$Background`, which `rux.toml`'s `[background]` declares in the
//! manifest), so the app's threads go on while it is not on screen. See
//! `rux-native/src/background.rs` for what the app's Rust sees.
//!
//! [`start`] checks, in this order: that `[background]` is declared, that the
//! channel for the permanent notification is declared, that
//! `[permissions.notifications]` is declared, and then (on Android) that the
//! person has allowed notifications and whatever a `location`, `camera` or
//! `microphone` service type needs. Android 14 refuses a service of those types
//! otherwise, and says so only as a security error in the log.
//!
//! **Desktop** has no such thing as a process that outlives its window, so
//! `start` and `stop` succeed and `active` follows them: the app runs anyway,
//! and closing the main window still quits. **The web** cannot.

use std::collections::HashMap;

use rux_runtime::native::background::{set_active, set_backend, Backend, Options, ACTIVE_SIGNAL};
use rux_runtime::native::{install, Any, Call, Error, Module};

fn option_error(message: String) -> Error {
    Error::new("option", message)
}

/// A `background.start` call's options.
fn options_from(args: Vec<Any>) -> Result<Options, Error> {
    let options = match args.into_iter().next() {
        Some(Any::Map(m)) => m,
        Some(other) => {
            return Err(option_error(format!("background.start takes its options as a map, not a {}", other.kind())))
        }
        None => {
            return Err(option_error(
                "background.start takes its options: background.start({ channel: \"shift\", title: \"On shift\" })"
                    .into(),
            ))
        }
    };
    for key in options.keys() {
        if !["channel", "title", "text", "route"].contains(&key.as_str()) {
            return Err(option_error(format!(
                "background.start has no option `{key}`. Its options are channel, title, text and route"
            )));
        }
    }
    let text = |key: &str, required: bool| -> Result<Option<String>, Error> {
        match options.get(key) {
            Some(Any::Str(s)) if !s.is_empty() => Ok(Some(s.clone())),
            Some(Any::Str(_)) | Some(Any::None) | None if !required => Ok(None),
            Some(Any::Str(_)) | Some(Any::None) | None => {
                Err(option_error(format!("background.start needs a `{key}`, the text of one")))
            }
            Some(other) => Err(option_error(format!("`{key}` takes text, not a {}", other.kind()))),
        }
    };
    let route = text("route", false)?;
    if let Some(route) = &route {
        if !route.starts_with('/') {
            return Err(option_error(format!("`route` is a path such as \"/board\", not \"{route}\"")));
        }
    }
    Ok(Options {
        channel: text("channel", true)?.unwrap_or_default(),
        title: text("title", true)?.unwrap_or_default(),
        text: text("text", false)?.unwrap_or_default(),
        route,
    })
}

/// The declaration checks every platform shares. `Ok` carries the declared
/// service, for the platform to start.
fn checked(options: &Options) -> Result<crate::notifications::Background, Error> {
    let Some(background) = crate::notifications::background() else {
        return Err(Error::new(
            "undeclared",
            "this app declares no background service. Add it to rux.toml:\n\n[background]\ntypes = [\"dataSync\"]\n\n\
             `types` says what the service is for (location, dataSync, mediaPlayback, ...); Android refuses one that does not say",
        ));
    };
    if crate::notifications::channel(&options.channel).is_none() {
        let name = &options.channel;
        return Err(Error::new(
            "undeclared",
            format!(
                "`{name}` is not a declared channel, and the service's notification needs one. Add it to rux.toml:\n\n[channels.{name}]\nname = \"What the person sees in settings\"\nimportance = \"low\""
            ),
        ));
    }
    crate::permissions::ensure_declared("notifications")?;
    Ok(background)
}

fn start(options: Options) -> Result<(), Error> {
    let background = checked(&options)?;
    platform::start(&options, &background)
}

fn stop() -> Result<(), Error> {
    platform::stop()
}

/// Install the calls for every document this process loads, and the same two
/// for the app's own Rust (`rux::background`).
pub(crate) fn install_background() {
    set_backend(Backend { start, stop });
    let mut calls = HashMap::new();
    calls.insert(
        "start".to_string(),
        Call::sync(|args: Vec<Any>| {
            start(options_from(args)?)?;
            Ok(Any::None)
        }),
    );
    calls.insert("stop".to_string(), Call::sync(|_args: Vec<Any>| stop().map(|()| Any::None)));
    calls.insert("active".to_string(), Call::sync(|_args: Vec<Any>| ACTIVE_SIGNAL.__read()));
    let _ = install(Module { interface: rux_runtime::builtins::background_interface(), calls });
    // Not running at start-up. Only the first time: a second start of the shell
    // must not report a running service as stopped.
    if ACTIVE_SIGNAL.get().is_none() {
        set_active(false);
    }
}

#[cfg(target_os = "android")]
mod platform {
    use super::*;
    use crate::notifications::Background;

    const FINE: &str = "android.permission.ACCESS_FINE_LOCATION";
    const COARSE: &str = "android.permission.ACCESS_COARSE_LOCATION";

    fn denied(what: &str, ask: &str) -> Error {
        Error::new(
            "permission",
            format!("a `{what}` service needs {ask} allowed first: ask with `permissions.request(\"{ask}\")` from a tap"),
        )
    }

    pub(super) fn start(options: &Options, background: &Background) -> Result<(), Error> {
        let allowed = |name: &str| crate::android_permission_flag("granted", name);
        if !allowed("android.permission.POST_NOTIFICATIONS") {
            return Err(Error::new(
                "permission",
                "notifications are not allowed, and the service's notification would not show: ask with `permissions.request(\"notifications\")` from a tap first",
            ));
        }
        for ty in &background.types {
            match ty.as_str() {
                "location" if !(allowed(FINE) || allowed(COARSE)) => return Err(denied("location", "location")),
                "camera" if !allowed("android.permission.CAMERA") => return Err(denied("camera", "camera")),
                "microphone" if !allowed("android.permission.RECORD_AUDIO") => {
                    return Err(denied("microphone", "microphone"))
                }
                _ => {}
            }
        }
        crate::android_background_start(
            &crate::notifications::channel_spec(),
            &options.channel,
            &options.title,
            &options.text,
            options.route.as_deref().unwrap_or(""),
            &background.types.join(","),
            background.keep_awake,
        )
        .map_err(|why| {
            // The activity says `kind: why`.
            let (kind, message) = match why.split_once(": ") {
                Some((kind @ ("background" | "permission" | "service"), message)) => (kind, message.to_string()),
                _ => ("service", why),
            };
            let message = if kind == "background" {
                format!("Android starts a background service only while the app is in front: call `background.start` from a tap ({message})")
            } else {
                message
            };
            Error::new(kind, message)
        })
    }

    pub(super) fn stop() -> Result<(), Error> {
        crate::android_background_stop();
        Ok(())
    }
}

#[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
mod platform {
    use super::*;
    use crate::notifications::Background;

    pub(super) fn start(_options: &Options, _background: &Background) -> Result<(), Error> {
        set_active(true);
        Ok(())
    }

    pub(super) fn stop() -> Result<(), Error> {
        set_active(false);
        Ok(())
    }
}

#[cfg(target_arch = "wasm32")]
mod platform {
    use super::*;
    use crate::notifications::Background;

    fn unsupported() -> Error {
        Error::new("unsupported", "a web page cannot run in the background: it stops with its tab")
    }

    pub(super) fn start(_options: &Options, _background: &Background) -> Result<(), Error> {
        Err(unsupported())
    }

    pub(super) fn stop() -> Result<(), Error> {
        Err(unsupported())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn options(pairs: &[(&str, Any)]) -> Vec<Any> {
        let map: BTreeMap<String, Any> = pairs.iter().map(|(k, v)| ((*k).to_string(), v.clone())).collect();
        vec![Any::Map(map)]
    }

    fn s(text: &str) -> Any {
        Any::Str(text.into())
    }

    #[test]
    fn a_start_call_is_read_into_options_or_refused_in_its_own_words() {
        let o = options_from(options(&[
            ("channel", s("shift")),
            ("title", s("On shift")),
            ("text", s("Looking for jobs")),
            ("route", s("/board")),
        ]))
        .unwrap();
        assert_eq!(o, Options::new("shift", "On shift").text("Looking for jobs").route("/board"));
        let plain = options_from(options(&[("channel", s("shift")), ("title", s("On"))])).unwrap();
        assert_eq!((plain.text.as_str(), plain.route), ("", None));

        let bad = |pairs: &[(&str, Any)]| options_from(options(pairs)).unwrap_err();
        assert!(bad(&[("title", s("On"))]).message.contains("needs a `channel`"));
        assert!(bad(&[("channel", s("shift"))]).message.contains("needs a `title`"));
        assert!(bad(&[("channel", s("shift")), ("title", s("x")), ("route", s("board"))]).message.contains("a path such as"));
        assert!(bad(&[("channel", s("shift")), ("title", s("x")), ("icon", s("y"))]).message.contains("no option `icon`"));
        assert_eq!(bad(&[("channel", Any::Int(3)), ("title", s("x"))]).kind, "option");
        assert!(options_from(vec![]).unwrap_err().message.contains("takes its options"));
    }

    /// The declaration checks come before anything platform-specific.
    #[test]
    fn a_start_is_held_to_the_declared_service_and_channel() {
        let _table = crate::notifications::TABLE_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::notifications::set_channels(&[("shift", "On shift", "low")]);
        crate::notifications::set_background(&[], false);
        let e = checked(&Options::new("shift", "On shift")).unwrap_err();
        assert_eq!(e.kind, "undeclared");
        assert!(e.message.contains("[background]") && e.message.contains("types"), "{}", e.message);

        crate::notifications::set_background(&["dataSync"], false);
        let e = checked(&Options::new("missing", "x")).unwrap_err();
        assert!(e.message.contains("[channels.missing]"), "{}", e.message);
        crate::notifications::set_channels(&[]);
        crate::notifications::set_background(&[], false);
    }
}
