//! Local notifications, from the app's own Rust: the same calls script's
//! `notifications` has, so a service that runs with no page on screen (a poll
//! that finds a new job) can post for itself.
//!
//! A notification belongs to a **channel**, which `rux.toml` declares
//! (`[channels.jobs]`), and posting needs `[permissions.notifications]` and the
//! person's consent (`rux::permissions::request("notifications")`, from a
//! tap). A channel nobody declared and a missing permission declaration are
//! `undeclared` errors; consent not given is a `permission` error. The shell
//! installs the backend at start-up with [`set_backend`], so these calls fail
//! with `unavailable` in a program that never started one (a unit test of the
//! app's Rust).
//!
//! ```ignore
//! use rux::notifications::{self, Notice};
//!
//! notifications::post(Notice::new("job-41", "jobs", "New job").text("Kololo to Ntinda").route("/jobs/41"))?;
//! notifications::cancel("job-41")?;
//! ```
//!
//! **The id is the notification's name.** Posting again with the same id
//! replaces the one on screen instead of adding another, and `cancel` takes it
//! away. A tap opens the app on `route`, as a link to that route would.

use std::sync::OnceLock;

use crate::Error;

/// One notification. Build it with [`Notice::new`] and the setters.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notice {
    /// Names the notification: the same id replaces, `cancel` removes.
    pub id: String,
    /// A channel declared under `[channels.<id>]` in `rux.toml`.
    pub channel: String,
    pub title: String,
    /// The body. Empty for none.
    pub text: String,
    /// Where a tap opens the app: a route such as `/jobs/41`. `None` opens
    /// the app where it was.
    pub route: Option<String>,
}

impl Notice {
    pub fn new(id: impl Into<String>, channel: impl Into<String>, title: impl Into<String>) -> Notice {
        Notice { id: id.into(), channel: channel.into(), title: title.into(), text: String::new(), route: None }
    }

    /// The body text.
    pub fn text(mut self, text: impl Into<String>) -> Notice {
        self.text = text.into();
        self
    }

    /// The route a tap opens.
    pub fn route(mut self, route: impl Into<String>) -> Notice {
        self.route = Some(route.into());
        self
    }
}

/// What the shell provides. Plain function pointers, so installing it needs no
/// allocation and the shell's own state stays its own.
pub struct Backend {
    pub post: fn(Notice) -> Result<(), Error>,
    pub cancel: fn(&str) -> Result<(), Error>,
    pub cancel_all: fn() -> Result<(), Error>,
}

static BACKEND: OnceLock<Backend> = OnceLock::new();

/// Install the backend. The first one wins, as the shell may start twice.
pub fn set_backend(backend: Backend) {
    let _ = BACKEND.set(backend);
}

fn backend() -> Result<&'static Backend, Error> {
    BACKEND.get().ok_or_else(|| {
        Error::new("unavailable", "no notifications backend is installed: the shell has not started")
    })
}

/// Show `notice`, or replace the one with its id. Any thread.
pub fn post(notice: Notice) -> Result<(), Error> {
    (backend()?.post)(notice)
}

/// Take the notification called `id` away, shown or not. Nothing to take away
/// is not an error.
pub fn cancel(id: &str) -> Result<(), Error> {
    (backend()?.cancel)(id)
}

/// Take every notification this app has shown away.
pub fn cancel_all() -> Result<(), Error> {
    (backend()?.cancel_all)()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A program that never started a shell (a unit test of the app's Rust)
    /// gets an error that says so, not a hang or a made-up success.
    #[test]
    fn without_a_shell_every_call_says_there_is_none() {
        let notice = Notice::new("a", "jobs", "New job").text("Kololo").route("/jobs/1");
        assert_eq!(notice.route.as_deref(), Some("/jobs/1"));
        assert_eq!(post(notice).unwrap_err().kind, "unavailable");
        assert_eq!(cancel("a").unwrap_err().kind, "unavailable");
        assert_eq!(cancel_all().unwrap_err().kind, "unavailable");
    }
}
