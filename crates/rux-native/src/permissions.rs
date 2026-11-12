//! Platform permissions, from the app's own Rust: the same answers script's
//! `permissions` gets, so a service that runs with no page on screen (a
//! location watch, a poll that posts notifications) can ask, or check, for
//! itself.
//!
//! What may be asked for is what `rux.toml`'s `[permissions]` declared, and the
//! manifest is written from the same table: a name not declared is an
//! `undeclared` error whichever side asks. The shell installs the backend at
//! start-up with [`set_backend`], so these calls do nothing useful in a program
//! that never started one (a unit test of the app's Rust): they fail with
//! `unavailable`.
//!
//! ```ignore
//! use rux::permissions;
//!
//! if permissions::status("location").await? == "granted" {
//!     // start the watch
//! }
//! ```
//!
//! The answers are `"granted"`, `"limited"`, `"unasked"`, `"denied"` and
//! `"blocked"`. `request` shows the system's dialog, which needs the app on
//! screen: call it from a tap's handler, not from a background task.

use std::future::Future;
use std::pin::Pin;
use std::sync::OnceLock;

use crate::Error;

/// A permission answer arriving later.
pub type Answer = Pin<Box<dyn Future<Output = Result<String, Error>> + Send + 'static>>;

/// What the shell provides. Plain function pointers, so installing it needs no
/// allocation and the shell's own state stays its own.
pub struct Backend {
    pub status: fn(String) -> Answer,
    pub request: fn(String) -> Answer,
    pub why: fn(&str) -> Result<String, Error>,
    pub open_settings: fn() -> Result<(), Error>,
}

static BACKEND: OnceLock<Backend> = OnceLock::new();

/// Install the backend. The first one wins, as the shell may start twice.
pub fn set_backend(backend: Backend) {
    let _ = BACKEND.set(backend);
}

fn backend() -> Result<&'static Backend, Error> {
    BACKEND.get().ok_or_else(|| {
        Error::new("unavailable", "no permissions backend is installed: the shell has not started")
    })
}

/// Whether `name` is allowed, without asking.
pub async fn status(name: &str) -> Result<String, Error> {
    (backend()?.status)(name.to_string()).await
}

/// Ask the person for `name`, and answer with where that left it. Answers at
/// once when the permission is granted or blocked: the system shows no dialog
/// for either.
pub async fn request(name: &str) -> Result<String, Error> {
    (backend()?.request)(name.to_string()).await
}

/// The reason `rux.toml` gave for `name`, for explaining after a refusal.
pub fn why(name: &str) -> Result<String, Error> {
    (backend()?.why)(name)
}

/// Open this app's page in the system settings, where a blocked permission is
/// changed. Throws `unsupported` where there is no such page.
pub fn open_settings() -> Result<(), Error> {
    (backend()?.open_settings)()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A program that never started a shell (a unit test of the app's Rust)
    /// gets an error that says so, not a hang or a made-up answer.
    #[test]
    fn without_a_shell_every_call_says_there_is_none() {
        let e = crate::block_on(status("location")).unwrap_err();
        assert_eq!(e.kind, "unavailable");
        assert_eq!(why("location").unwrap_err().kind, "unavailable");
        assert_eq!(open_settings().unwrap_err().kind, "unavailable");
    }
}
