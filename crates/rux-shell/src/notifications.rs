//! What `rux.toml` declared for notifications and background work:
//! `[channels.<id>]` and `[background]`.
//!
//! **Declared, never inferred**, like `[permissions]` (see `permissions.rs`):
//! the manifest and the runtime read the same table, so a post to a channel
//! nobody declared, or a service started in an app that never said what it is
//! for, throws `undeclared` and names the table to add, where the platform
//! would refuse it silently.
//!
//! This is the table and nothing else. The calls that use it (`notifications`
//! and `background` in script, and in the app's Rust) are built on top.

// The table is read by the notification and service calls built on it (UD-003
// steps 3 to 6); until they land, nothing but the tests reads it back.
#![allow(dead_code)]

use std::sync::Mutex;

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

#[cfg(test)]
mod tests {
    use super::*;

    /// One test, because the table is the process's: parallel tests would
    /// overwrite each other's.
    #[test]
    fn what_was_declared_is_what_is_read_back() {
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

        // Declared again, as a second start of the shell would: replaced.
        set_channels(&[]);
        set_background(&[], false);
        assert!(channels().is_empty() && background().is_none());
    }
}
