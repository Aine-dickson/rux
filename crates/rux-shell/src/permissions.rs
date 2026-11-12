//! `permissions.status`, `permissions.request`, `permissions.why` and
//! `permissions.openSettings`: the run-time half of `rux.toml`'s
//! `[permissions]`.
//!
//! **Only what was declared can be asked for.** The build writes the manifest
//! from `rux.toml` ([`set_permissions`] hands the same table to the runtime),
//! so a name nobody declared would be refused by the platform anyway, silently.
//! Here it throws `undeclared`, saying which table to add.
//!
//! The names are `"location"`, `"location-background"` (Android asks for it as
//! a second step, after the first), `"notifications"`, `"camera"` and
//! `"microphone"`, and any platform name listed under `android = [...]`.
//!
//! The answers: `"granted"`; `"limited"` (an approximate position, where a
//! precise one was asked for); `"unasked"`; `"denied"` (refused, and the system
//! will ask again); `"blocked"` (refused for good, so only the settings page
//! can change it).
//!
//! Desktop has no permission to ask for, so a portable name is `"granted"`
//! there. What a desktop operating system gates (Windows' location privacy
//! switch) belongs to the feature that needs it.

use std::collections::HashMap;
use std::sync::Mutex;

use rux_runtime::native::{install, Any, Call, Error, Module};

/// A declared `[permissions.<name>]`.
#[derive(Clone)]
struct Entry {
    name: String,
    why: String,
    background: bool,
    approximate: bool,
}

#[derive(Default)]
struct Declared {
    entries: Vec<Entry>,
    android: Vec<String>,
}

static DECLARED: Mutex<Declared> = Mutex::new(Declared { entries: Vec::new(), android: Vec::new() });

/// What `rux.toml` declared, as `(name, why, background, approximate)`, and the
/// platform names under `android`. Set before the first document loads, by the
/// code `rux build` writes and by `rux run`.
pub fn set_permissions(entries: &[(&str, &str, bool, bool)], android: &[&str]) {
    let mut d = DECLARED.lock().unwrap_or_else(|e| e.into_inner());
    d.entries = entries
        .iter()
        .map(|(name, why, background, approximate)| Entry {
            name: (*name).into(),
            why: (*why).into(),
            background: *background,
            approximate: *approximate,
        })
        .collect();
    d.android = android.iter().map(|s| (*s).to_string()).collect();
}

/// What a name asks for.
#[derive(Clone, Debug, PartialEq)]
enum Which {
    Location { approximate: bool },
    LocationBackground,
    Notifications,
    Camera,
    Microphone,
    /// A platform name Rux has no word for, from `android = [...]`.
    Raw(String),
}

const FINE: &str = "android.permission.ACCESS_FINE_LOCATION";
const COARSE: &str = "android.permission.ACCESS_COARSE_LOCATION";
const BACKGROUND: &str = "android.permission.ACCESS_BACKGROUND_LOCATION";

impl Which {
    /// The Android names that are asked for together.
    #[cfg_attr(not(target_os = "android"), allow(dead_code))]
    fn android_names(&self) -> Vec<&str> {
        match self {
            Which::Location { approximate: true } => vec![COARSE],
            Which::Location { approximate: false } => vec![FINE, COARSE],
            Which::LocationBackground => vec![BACKGROUND],
            Which::Notifications => vec!["android.permission.POST_NOTIFICATIONS"],
            Which::Camera => vec!["android.permission.CAMERA"],
            Which::Microphone => vec!["android.permission.RECORD_AUDIO"],
            Which::Raw(name) => vec![name.as_str()],
        }
    }
}

/// The declared permission a name means, or the error that says what to add.
fn resolve(name: &str) -> Result<Which, Error> {
    let d = DECLARED.lock().unwrap_or_else(|e| e.into_inner());
    let add = |table: &str| {
        Error::new(
            "undeclared",
            format!("`{name}` is not declared. Add it to rux.toml:\n\n[permissions.{table}]\nwhy = \"What the person gets from allowing it\""),
        )
    };
    let entry = |n: &str| d.entries.iter().find(|e| e.name == n);
    match name {
        "location" => entry("location").map(|e| Which::Location { approximate: e.approximate }).ok_or_else(|| add("location")),
        "location-background" => match entry("location") {
            Some(e) if e.background => Ok(Which::LocationBackground),
            Some(_) => Err(Error::new(
                "undeclared",
                "`location-background` needs `background = true` under [permissions.location] in rux.toml",
            )),
            None => Err(add("location")),
        },
        "notifications" | "camera" | "microphone" => entry(name)
            .map(|_| match name {
                "notifications" => Which::Notifications,
                "camera" => Which::Camera,
                _ => Which::Microphone,
            })
            .ok_or_else(|| add(name)),
        raw if d.android.iter().any(|a| a == raw) => Ok(Which::Raw(raw.to_string())),
        other => Err(Error::new(
            "undeclared",
            format!(
                "`{other}` is not a permission this app declared. The names are location, \
                 location-background, notifications, camera and microphone, or a platform name \
                 listed under `android = [...]` in [permissions] of rux.toml"
            ),
        )),
    }
}

fn name_arg(call: &str, args: Vec<Any>) -> Result<String, Error> {
    match args.into_iter().next() {
        Some(Any::Str(s)) => Ok(s),
        _ => Err(Error::new("argument", format!("`permissions.{call}` takes the permission's name, a `string`"))),
    }
}

/// Install the calls for every document this process loads.
pub(crate) fn install_permissions() {
    let interface = rux_runtime::builtins::permissions_interface();
    let mut calls = HashMap::new();
    calls.insert(
        "status".to_string(),
        Call::future(|args: Vec<Any>| async move {
            let which = resolve(&name_arg("status", args)?)?;
            Ok(Any::Str(backend::status(&which).await?.to_string()))
        }),
    );
    calls.insert(
        "request".to_string(),
        Call::future(|args: Vec<Any>| async move {
            let which = resolve(&name_arg("request", args)?)?;
            Ok(Any::Str(backend::request(&which).await?.to_string()))
        }),
    );
    calls.insert(
        "why".to_string(),
        Call::sync(|args: Vec<Any>| {
            let name = name_arg("why", args)?;
            resolve(&name)?;
            let d = DECLARED.lock().unwrap_or_else(|e| e.into_inner());
            // A platform name has no reason of its own: nothing was said.
            Ok(Any::Str(d.entries.iter().find(|e| e.name == name).map(|e| e.why.clone()).unwrap_or_default()))
        }),
    );
    calls.insert("openSettings".to_string(), Call::sync(|_args: Vec<Any>| backend::open_settings().map(|()| Any::None)));
    let _ = install(Module { interface, calls });
}

/// The Android answer from what the activity says about each name.
///
/// `granted` and `limited` come from the grants; the rest from whether the
/// app has asked and whether the system would still show its dialog, because
/// "never asked" and "refused for good" are the same to the platform's own
/// check and not to the person.
#[cfg_attr(not(any(target_os = "android", test)), allow(dead_code))]
fn answer(which: &Which, granted: impl Fn(&str) -> bool, asked: bool, rationale: bool) -> &'static str {
    let fine = matches!(which, Which::Location { approximate: false }) && granted(FINE);
    let any = which.android_names().iter().any(|n| granted(n));
    if fine || (any && !matches!(which, Which::Location { approximate: false })) {
        "granted"
    } else if any {
        // Coarse without fine, where fine was asked for.
        "limited"
    } else if !asked {
        "unasked"
    } else if rationale {
        "denied"
    } else {
        "blocked"
    }
}

#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
mod backend {
    use super::*;

    pub(super) async fn status(_which: &Which) -> Result<&'static str, Error> {
        Ok("granted")
    }

    pub(super) async fn request(_which: &Which) -> Result<&'static str, Error> {
        Ok("granted")
    }

    pub(super) fn open_settings() -> Result<(), Error> {
        Err(Error::new("unsupported", "this app has no system settings page to open on the desktop"))
    }
}

#[cfg(target_os = "android")]
mod backend {
    use super::*;
    use crate::files::Slot;

    /// The request waiting on the system's dialog. There is one dialog.
    static PENDING: Mutex<Option<Slot>> = Mutex::new(None);

    /// The dialog closed: wake the request waiting on it.
    pub(super) fn done() {
        if let Some(slot) = PENDING.lock().unwrap_or_else(|e| e.into_inner()).take() {
            slot.set(Ok(Any::None));
        }
    }

    pub(super) async fn status(which: &Which) -> Result<&'static str, Error> {
        let names = which.android_names();
        // Whichever name the dialog was asked under: they are asked together.
        let first = names.first().copied().unwrap_or_default();
        Ok(answer(
            which,
            |n| crate::android_permission_flag("granted", n),
            crate::android_permission_flag("asked", first),
            crate::android_permission_flag("rationale", first),
        ))
    }

    pub(super) async fn request(which: &Which) -> Result<&'static str, Error> {
        let before = status(which).await?;
        // Nothing the system would show a dialog for: it is settled, and only
        // the settings page changes it.
        if matches!(before, "granted" | "blocked") {
            return Ok(before);
        }
        if *which == Which::LocationBackground {
            // Coarse is granted whenever any location is.
            if status(&Which::Location { approximate: true }).await? != "granted" {
                return Err(Error::new(
                    "order",
                    "Android asks for background location after the foreground one: request \"location\" first",
                ));
            }
        }
        let slot = {
            let mut pending = PENDING.lock().unwrap_or_else(|e| e.into_inner());
            if pending.is_some() {
                return Err(Error::new("busy", "a permission request is already open"));
            }
            let slot = Slot::default();
            *pending = Some(slot.clone());
            slot
        };
        crate::android_request_permissions(&which.android_names());
        slot.await?;
        status(which).await
    }

    pub(super) fn open_settings() -> Result<(), Error> {
        crate::android_open_app_settings();
        Ok(())
    }
}

/// The system's dialog closed. See [`backend::done`].
#[cfg(target_os = "android")]
pub(crate) fn request_done() {
    backend::done();
}

#[cfg(target_arch = "wasm32")]
mod backend {
    use super::*;

    pub(super) async fn status(_which: &Which) -> Result<&'static str, Error> {
        Err(Error::new("unsupported", "permissions are not asked for on the web yet"))
    }

    pub(super) async fn request(_which: &Which) -> Result<&'static str, Error> {
        Err(Error::new("unsupported", "permissions are not asked for on the web yet"))
    }

    pub(super) fn open_settings() -> Result<(), Error> {
        Err(Error::new("unsupported", "a web page has no system settings page to open"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn declare() {
        set_permissions(
            &[("location", "Shows the rider", true, false), ("camera", "Photos", false, false)],
            &["android.permission.READ_CONTACTS"],
        );
    }

    #[test]
    fn only_a_declared_name_resolves_and_the_rest_say_what_to_add() {
        declare();
        assert_eq!(resolve("location").unwrap(), Which::Location { approximate: false });
        assert_eq!(resolve("location-background").unwrap(), Which::LocationBackground);
        assert_eq!(resolve("camera").unwrap(), Which::Camera);
        assert_eq!(resolve("android.permission.READ_CONTACTS").unwrap(), Which::Raw("android.permission.READ_CONTACTS".into()));
        let e = resolve("microphone").unwrap_err();
        assert_eq!(e.kind, "undeclared");
        assert!(e.message.contains("[permissions.microphone]"), "{}", e.message);
        assert_eq!(resolve("telepathy").unwrap_err().kind, "undeclared");
        set_permissions(&[("location", "x", false, false)], &[]);
        assert!(resolve("location-background").unwrap_err().message.contains("background = true"));
    }

    #[test]
    fn the_answer_tells_never_asked_from_refused_for_good() {
        let location = Which::Location { approximate: false };
        let none = |_: &str| false;
        assert_eq!(answer(&location, none, false, false), "unasked");
        assert_eq!(answer(&location, none, true, true), "denied");
        assert_eq!(answer(&location, none, true, false), "blocked");
        assert_eq!(answer(&location, |n| n == FINE, true, false), "granted");
        assert_eq!(answer(&location, |n| n == COARSE, true, false), "limited");
        // Asked for roughly, and given roughly, is what was wanted.
        assert_eq!(answer(&Which::Location { approximate: true }, |n| n == COARSE, true, false), "granted");
        assert_eq!(answer(&Which::Camera, |_| true, true, false), "granted");
    }
}
