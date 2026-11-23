//! Local notifications and what `rux.toml` declared for them and for
//! background work: `[channels.<id>]` and `[background]`.
//!
//! **Declared, never inferred**, like `[permissions]` (see `permissions.rs`):
//! the manifest and the runtime read the same tables, so a post to a channel
//! nobody declared, or a service started in an app that never said what it is
//! for, throws `undeclared` and names the table to add, where the platform
//! would refuse it silently.
//!
//! `notifications.post({ id, channel, title, text, route })` in script and
//! `rux::notifications::post` in the app's Rust are one call: [`post`]. It
//! checks, in this order, that the channel is declared, that
//! `[permissions.notifications]` is declared, and that the person has allowed
//! notifications (`permission`), then hands the notice to the platform.
//!
//! The platforms: Android is built here. The web and Windows are built after
//! it (UD-003 steps 5 and 6); until then they throw `unsupported`.

// The background table is read by the service built on it (UD-003 step 4), and
// the importance's loudness by the web and Windows backends (steps 5 and 6).
#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::Mutex;

use rux_runtime::native::notifications::{set_backend, Backend, Notice};
use rux_runtime::native::{install, Any, Call, Error, Module};

/// How loud a channel is. The words are the ones `rux.toml` takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Importance {
    High,
    Default,
    Low,
    Min,
}

impl Importance {
    fn from_word(word: &str) -> Importance {
        match word {
            "high" => Importance::High,
            "low" => Importance::Low,
            "min" => Importance::Min,
            _ => Importance::Default,
        }
    }

    /// The word `rux.toml` writes.
    fn word(self) -> &'static str {
        match self {
            Importance::High => "high",
            Importance::Default => "default",
            Importance::Low => "low",
            Importance::Min => "min",
        }
    }

    /// Whether a notification of this importance makes a sound.
    pub(crate) fn audible(self) -> bool {
        matches!(self, Importance::High | Importance::Default)
    }
}

/// A declared `[channels.<id>]`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Channel {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) importance: Importance,
}

/// A declared `[background]`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Background {
    pub(crate) types: Vec<String>,
    pub(crate) keep_awake: bool,
}

#[derive(Default)]
struct Declared {
    channels: Vec<Channel>,
    background: Option<Background>,
}

static DECLARED: Mutex<Declared> = Mutex::new(Declared { channels: Vec::new(), background: None });

fn declared() -> std::sync::MutexGuard<'static, Declared> {
    DECLARED.lock().unwrap_or_else(|e| e.into_inner())
}

/// What `rux.toml` declared under `[channels]`, as `(id, name, importance)`
/// with the importance as its word (`"high"`, `"default"`, `"low"`, `"min"`).
/// Set before the first document loads, by the code `rux build` writes and by
/// `rux run`.
pub fn set_channels(channels: &[(&str, &str, &str)]) {
    declared().channels = channels
        .iter()
        .map(|(id, name, importance)| Channel {
            id: (*id).into(),
            name: (*name).into(),
            importance: Importance::from_word(importance),
        })
        .collect();
}

/// What `rux.toml` declared under `[background]`: the Android service types
/// and `keep-awake`. No types means the app declared no service (the manifest
/// refuses a `[background]` without one).
pub fn set_background(types: &[&str], keep_awake: bool) {
    declared().background = (!types.is_empty())
        .then(|| Background { types: types.iter().map(|t| (*t).into()).collect(), keep_awake });
}

/// The declared channel called `id`.
pub(crate) fn channel(id: &str) -> Option<Channel> {
    declared().channels.iter().find(|c| c.id == id).cloned()
}

/// Every declared channel, in the order written.
pub(crate) fn channels() -> Vec<Channel> {
    declared().channels.clone()
}

/// The declared background service, if any.
pub(crate) fn background() -> Option<Background> {
    declared().background.clone()
}

/// The channels as the platform is told them, one `id TAB name TAB importance`
/// per line.
fn channel_spec() -> String {
    channels().iter().map(|c| format!("{}\t{}\t{}", c.id, c.name, c.importance.word())).collect::<Vec<_>>().join("\n")
}

fn option_error(message: String) -> Error {
    Error::new("option", message)
}

/// A `notifications.post` call's options as a [`Notice`].
fn notice_from(args: Vec<Any>) -> Result<Notice, Error> {
    let options = match args.into_iter().next() {
        Some(Any::Map(m)) => m,
        Some(other) => {
            return Err(option_error(format!("notifications.post takes its options as a map, not a {}", other.kind())))
        }
        None => {
            return Err(option_error(
                "notifications.post takes its options: notifications.post({ id: \"job-41\", channel: \"jobs\", title: \"New job\" })"
                    .into(),
            ))
        }
    };
    for key in options.keys() {
        if !["id", "channel", "title", "text", "route"].contains(&key.as_str()) {
            return Err(option_error(format!(
                "notifications.post has no option `{key}`. Its options are id, channel, title, text and route"
            )));
        }
    }
    let text = |key: &str, required: bool| -> Result<Option<String>, Error> {
        match options.get(key) {
            Some(Any::Str(s)) if !s.is_empty() => Ok(Some(s.clone())),
            Some(Any::Str(_)) | Some(Any::None) | None if !required => Ok(None),
            Some(Any::Str(_)) | Some(Any::None) | None => {
                Err(option_error(format!("notifications.post needs a `{key}`, the text of one")))
            }
            Some(other) => Err(option_error(format!("`{key}` takes text, not a {}", other.kind()))),
        }
    };
    let route = text("route", false)?;
    if let Some(route) = &route {
        if !route.starts_with('/') {
            return Err(option_error(format!("`route` is a path such as \"/jobs/41\", not \"{route}\"")));
        }
    }
    Ok(Notice {
        id: text("id", true)?.unwrap_or_default(),
        channel: text("channel", true)?.unwrap_or_default(),
        title: text("title", true)?.unwrap_or_default(),
        text: text("text", false)?.unwrap_or_default(),
        route,
    })
}

/// Show `notice`, after the checks every platform shares. Any thread.
pub(crate) fn post(notice: Notice) -> Result<(), Error> {
    let Some(channel) = channel(&notice.channel) else {
        let name = &notice.channel;
        let have = channels();
        let known = if have.is_empty() {
            "rux.toml declares no channels yet".to_string()
        } else {
            format!("it declares {}", have.iter().map(|c| c.id.as_str()).collect::<Vec<_>>().join(", "))
        };
        return Err(Error::new(
            "undeclared",
            format!(
                "`{name}` is not a declared channel ({known}). Add it to rux.toml:\n\n[channels.{name}]\nname = \"What the person sees in settings\"\nimportance = \"default\""
            ),
        ));
    };
    crate::permissions::ensure_declared("notifications")?;
    if notice.route.as_deref().is_some_and(|r| !r.starts_with('/')) {
        return Err(option_error("`route` is a path such as \"/jobs/41\"".into()));
    }
    platform::post(&notice, &channel)
}

fn cancel(id: &str) -> Result<(), Error> {
    platform::cancel(id)
}

fn cancel_all() -> Result<(), Error> {
    platform::cancel_all()
}

/// Install the calls for every document this process loads, and the same
/// three for the app's own Rust (`rux::notifications`).
pub(crate) fn install_notifications() {
    set_backend(Backend { post, cancel, cancel_all });
    let mut calls = HashMap::new();
    calls.insert(
        "post".to_string(),
        Call::sync(|args: Vec<Any>| {
            post(notice_from(args)?)?;
            Ok(Any::None)
        }),
    );
    calls.insert(
        "cancel".to_string(),
        Call::sync(|args: Vec<Any>| match args.into_iter().next() {
            Some(Any::Str(id)) => cancel(&id).map(|()| Any::None),
            _ => Err(Error::new("argument", "`notifications.cancel` takes the id of the notification, a `string`")),
        }),
    );
    calls.insert("cancelAll".to_string(), Call::sync(|_args: Vec<Any>| cancel_all().map(|()| Any::None)));
    let _ = install(Module { interface: rux_runtime::builtins::notifications_interface(), calls });
}

#[cfg(target_os = "android")]
mod platform {
    use super::*;

    pub(super) fn post(notice: &Notice, _channel: &Channel) -> Result<(), Error> {
        // Android 13 made this a permission to ask for. Before it, the same
        // flag says whether the person switched notifications off for the app.
        if !crate::android_permission_flag("granted", "android.permission.POST_NOTIFICATIONS") {
            return Err(Error::new(
                "permission",
                "notifications are not allowed: ask with `permissions.request(\"notifications\")` from a tap first",
            ));
        }
        crate::android_notify(
            &channel_spec(),
            &notice.id,
            &notice.channel,
            &notice.title,
            &notice.text,
            notice.route.as_deref().unwrap_or(""),
        )
        .map_err(|why| Error::new("notification", format!("Android would not show it: {why}")))
    }

    pub(super) fn cancel(id: &str) -> Result<(), Error> {
        crate::android_cancel_notification(Some(id));
        Ok(())
    }

    pub(super) fn cancel_all() -> Result<(), Error> {
        crate::android_cancel_notification(None);
        Ok(())
    }
}

#[cfg(not(target_os = "android"))]
mod platform {
    use super::*;

    fn unsupported() -> Error {
        Error::new("unsupported", "this platform cannot show notifications yet: Android can")
    }

    pub(super) fn post(_notice: &Notice, _channel: &Channel) -> Result<(), Error> {
        Err(unsupported())
    }

    pub(super) fn cancel(_id: &str) -> Result<(), Error> {
        Err(unsupported())
    }

    pub(super) fn cancel_all() -> Result<(), Error> {
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
    fn a_post_call_is_read_into_a_notice_or_refused_in_its_own_words() {
        let n = notice_from(options(&[
            ("id", s("job-41")),
            ("channel", s("jobs")),
            ("title", s("New job")),
            ("text", s("Kololo to Ntinda")),
            ("route", s("/jobs/41")),
        ]))
        .unwrap();
        assert_eq!(n, Notice::new("job-41", "jobs", "New job").text("Kololo to Ntinda").route("/jobs/41"));

        let plain =
            notice_from(options(&[("id", s("a")), ("channel", s("jobs")), ("title", s("Hi")), ("text", Any::None)]))
                .unwrap();
        assert_eq!((plain.text.as_str(), plain.route), ("", None), "text and route are optional");

        let base = [("id", s("a")), ("channel", s("jobs")), ("title", s("Hi"))];
        let bad = |extra: (&'static str, Any)| {
            let mut v: Vec<(&str, Any)> = base.to_vec();
            v.retain(|(k, _)| *k != extra.0);
            v.push(extra);
            notice_from(options(&v)).unwrap_err()
        };
        assert_eq!(bad(("title", Any::None)).kind, "option");
        assert!(bad(("title", Any::None)).message.contains("needs a `title`"));
        assert!(bad(("id", s(""))).message.contains("needs a `id`"));
        assert!(bad(("channel", Any::Int(3))).message.contains("takes text, not a int"));
        assert!(bad(("route", s("jobs/41"))).message.contains("a path such as"));
        assert!(bad(("sound", s("ding"))).message.contains("no option `sound`"));
        assert!(notice_from(vec![]).unwrap_err().message.contains("takes its options"));
        assert!(notice_from(vec![s("x")]).unwrap_err().message.contains("as a map"));
    }

    /// One test for the table, because it is the process's: parallel tests
    /// would overwrite each other's.
    #[test]
    fn what_was_declared_is_what_is_read_back_and_what_a_post_is_held_to() {
        set_channels(&[("jobs", "New jobs", "high"), ("shift", "On shift", "low"), ("odd", "Odd", "whatever")]);
        set_background(&["location", "dataSync"], true);

        assert_eq!(channels().len(), 3);
        let jobs = channel("jobs").unwrap();
        assert_eq!((jobs.name.as_str(), jobs.importance), ("New jobs", Importance::High));
        assert!(jobs.importance.audible());
        assert!(!channel("shift").unwrap().importance.audible());
        assert_eq!(channel("odd").unwrap().importance, Importance::Default, "an unknown word is the default");
        assert!(channel("missing").is_none());
        let b = background().unwrap();
        assert_eq!(b.types, ["location", "dataSync"]);
        assert!(b.keep_awake);
        assert_eq!(channel_spec(), "jobs\tNew jobs\thigh\nshift\tOn shift\tlow\nodd\tOdd\tdefault");

        // A channel nobody declared is refused first, saying what to add.
        let e = post(Notice::new("a", "missing", "Hi")).unwrap_err();
        assert_eq!(e.kind, "undeclared");
        assert!(e.message.contains("[channels.missing]") && e.message.contains("jobs, shift, odd"), "{}", e.message);
        // The permission is held to next. (This test does not declare it:
        // another test owns that table.)
        let e = post(Notice::new("a", "jobs", "Hi")).unwrap_err();
        assert_eq!(e.kind, "undeclared");
        assert!(e.message.contains("notifications"), "{}", e.message);

        // Declared again, as a second start of the shell would: replaced.
        set_channels(&[]);
        set_background(&[], false);
        assert!(channels().is_empty() && background().is_none());
        let e = post(Notice::new("a", "jobs", "Hi")).unwrap_err();
        assert!(e.message.contains("declares no channels"), "{}", e.message);
    }
}
