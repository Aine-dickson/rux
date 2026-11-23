//! Where the device is: `location.current`, `location.watch`, `location.stop`
//! and the signals `location.position` and `location.watching`, and the same
//! calls for the app's own Rust (`rux::location`).
//!
//! The checks every platform shares come first: `[permissions.location]` is
//! declared (else `undeclared`, naming the table to add). Whether the person
//! allowed it is the platform's to check, since only it can say.
//!
//! The platforms: Android and the web are built after the interface
//! (UD-002 steps 2 and 3); until then they, and the desktop, throw
//! `unsupported`.

use std::collections::{BTreeMap, HashMap};

use rux_runtime::native::location::{
    set_backend, set_watching, Accuracy, Answer, Backend, Fix, Options, POSITION_SIGNAL, WATCHING_SIGNAL,
};
use rux_runtime::native::{install, Any, Call, Error, IntoRux, Module};

fn option_error(message: String) -> Error {
    Error::new("option", message)
}

/// A call's options map. `call` names it in messages; `allowed` are its keys.
fn options_from(call: &str, args: Vec<Any>, allowed: &[&str]) -> Result<Options, Error> {
    let map: BTreeMap<String, Any> = match args.into_iter().next() {
        Some(Any::Map(m)) => m,
        Some(Any::None) | None => BTreeMap::new(),
        Some(other) => {
            return Err(option_error(format!("location.{call} takes its options as a map, not a {}", other.kind())))
        }
    };
    for key in map.keys() {
        if !allowed.contains(&key.as_str()) {
            return Err(option_error(format!(
                "location.{call} has no option `{key}`. Its options are {}",
                allowed.join(", ")
            )));
        }
    }
    let number = |key: &str| -> Result<Option<f64>, Error> {
        match map.get(key) {
            Some(Any::Int(n)) if *n >= 0 => Ok(Some(*n as f64)),
            Some(Any::Float(f)) if *f >= 0.0 && f.is_finite() => Ok(Some(*f)),
            Some(Any::None) | None => Ok(None),
            Some(Any::Int(_) | Any::Float(_)) => Err(option_error(format!("`{key}` cannot be negative"))),
            Some(other) => Err(option_error(format!("`{key}` takes a number, not a {}", other.kind()))),
        }
    };
    let mut options = Options::new();
    match map.get("accuracy") {
        Some(Any::Str(s)) if s == "high" => options.accuracy = Accuracy::High,
        Some(Any::Str(s)) if s == "balanced" => options.accuracy = Accuracy::Balanced,
        Some(Any::None) | None => {}
        Some(other) => {
            let said = match other {
                Any::Str(s) => format!("\"{s}\""),
                other => format!("a {}", other.kind()),
            };
            return Err(option_error(format!("`accuracy` is \"high\" or \"balanced\", not {said}")));
        }
    }
    if let Some(ms) = number("timeoutMs")? {
        options.timeout_ms = ms as u64;
    }
    if let Some(ms) = number("intervalMs")? {
        options.interval_ms = ms as u64;
    }
    if let Some(m) = number("distanceM")? {
        options.distance_m = m;
    }
    Ok(options)
}

/// The declaration check every platform shares.
fn checked() -> Result<(), Error> {
    crate::permissions::ensure_declared("location")
}

async fn current(options: Options) -> Result<Fix, Error> {
    checked()?;
    platform::current(options).await
}

fn watch(options: Options) -> Result<(), Error> {
    checked()?;
    platform::watch(options)
}

fn stop() -> Result<(), Error> {
    platform::stop()
}

/// Install the calls for every document this process loads, and the same
/// three for the app's own Rust (`rux::location`).
pub(crate) fn install_location() {
    set_backend(Backend { current: |options| -> Answer { Box::pin(current(options)) }, watch, stop });
    let mut calls = HashMap::new();
    calls.insert(
        "current".to_string(),
        Call::future(|args: Vec<Any>| async move {
            let options = options_from("current", args, &["accuracy", "timeoutMs"])?;
            current(options).await?.into_rux()
        }),
    );
    calls.insert(
        "watch".to_string(),
        Call::sync(|args: Vec<Any>| {
            watch(options_from("watch", args, &["accuracy", "intervalMs", "distanceM"])?)?;
            Ok(Any::None)
        }),
    );
    calls.insert("stop".to_string(), Call::sync(|_args: Vec<Any>| stop().map(|()| Any::None)));
    calls.insert("position".to_string(), Call::sync(|_args: Vec<Any>| POSITION_SIGNAL.__read()));
    calls.insert("watching".to_string(), Call::sync(|_args: Vec<Any>| WATCHING_SIGNAL.__read()));
    let _ = install(Module { interface: rux_runtime::builtins::location_interface(), calls });
    // Not watching at start-up. Only the first time: a second start of the
    // shell must not report a running watch as stopped.
    if WATCHING_SIGNAL.get().is_none() {
        set_watching(false);
    }
}

mod platform {
    use super::*;

    fn unsupported() -> Error {
        Error::new("unsupported", "this platform cannot say where it is yet")
    }

    pub(super) async fn current(_options: Options) -> Result<Fix, Error> {
        Err(unsupported())
    }

    pub(super) fn watch(_options: Options) -> Result<(), Error> {
        Err(unsupported())
    }

    pub(super) fn stop() -> Result<(), Error> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(pairs: &[(&str, Any)]) -> Vec<Any> {
        vec![Any::Map(pairs.iter().map(|(k, v)| ((*k).to_string(), v.clone())).collect())]
    }

    #[test]
    fn options_are_read_with_their_defaults_or_refused_in_their_own_words() {
        let watch_keys = ["accuracy", "intervalMs", "distanceM"];
        let o = options_from("watch", map(&[("intervalMs", Any::Int(15_000)), ("distanceM", Any::Float(5.5))]), &watch_keys)
            .unwrap();
        assert_eq!((o.interval_ms, o.distance_m, o.accuracy), (15_000, 5.5, Accuracy::High));
        let current_keys = ["accuracy", "timeoutMs"];
        let o = options_from("current", vec![], &current_keys).unwrap();
        assert_eq!(o, Options::new(), "no options is the defaults");
        let o = options_from(
            "current",
            map(&[("accuracy", Any::Str("balanced".into())), ("timeoutMs", Any::Int(2_000))]),
            &current_keys,
        )
        .unwrap();
        assert_eq!((o.accuracy, o.timeout_ms), (Accuracy::Balanced, 2_000));

        let bad = |pairs: &[(&str, Any)]| options_from("current", map(pairs), &current_keys).unwrap_err();
        assert_eq!(bad(&[("accuracy", Any::Str("exact".into()))]).kind, "option");
        assert!(bad(&[("accuracy", Any::Str("exact".into()))]).message.contains("\"high\" or \"balanced\""));
        assert!(bad(&[("timeoutMs", Any::Int(-1))]).message.contains("cannot be negative"));
        assert!(bad(&[("timeoutMs", Any::Str("x".into()))]).message.contains("takes a number"));
        assert!(bad(&[("intervalMs", Any::Int(1))]).message.contains("no option `intervalMs`"));
        assert!(options_from("current", vec![Any::Int(3)], &[]).unwrap_err().message.contains("as a map"));
    }
}
