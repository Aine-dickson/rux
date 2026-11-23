//! Secrets, from the app's own Rust: the same calls script's `secrets` has.
//!
//! A refresh token or an API key belongs here, not in `settings`, which is a
//! plain JSON file. [`set`] keeps a string under a key in what the platform
//! protects: Android's Keystore (the key never leaves it, and goes with the
//! app), Windows' DPAPI (readable only by the same person on the same
//! machine). Values are strings.
//!
//! ```ignore
//! use rux::secrets;
//!
//! secrets::set("refreshToken", &token)?;
//! let token = secrets::get("refreshToken")?; // Option<String>
//! secrets::remove("refreshToken")?;
//! ```
//!
//! A platform with no protected store here (macOS, Linux, the web) fails every
//! call with `unsupported`, rather than keeping a secret where anyone reads it.
//! The shell installs the backend at start-up with [`set_backend`], so these
//! calls fail with `unavailable` in a program that never started one.

use std::sync::OnceLock;

use crate::Error;

/// What a platform does for the three calls. Each gets a checked key.
#[derive(Clone, Copy)]
pub struct Backend {
    pub get: fn(&str) -> Result<Option<String>, Error>,
    pub set: fn(&str, &str) -> Result<(), Error>,
    pub remove: fn(&str) -> Result<(), Error>,
}

static BACKEND: OnceLock<Backend> = OnceLock::new();

/// Install the backend. The first one wins, as the shell may start twice.
pub fn set_backend(backend: Backend) {
    let _ = BACKEND.set(backend);
}

fn backend() -> Result<&'static Backend, Error> {
    BACKEND.get().ok_or_else(|| Error::new("unavailable", "no secrets backend is installed: the shell has not started"))
}

/// A key is a short name, not a path or a sentence: letters, digits and
/// `._-`, so no platform reads it as anything but a name.
pub fn check_key(key: &str) -> Result<&str, Error> {
    let ok = !key.is_empty() && key.len() <= 128 && key.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    if ok {
        Ok(key)
    } else {
        Err(Error::new("option", format!("a secret's key is letters, digits and `._-`, up to 128 of them, not {key:?}")))
    }
}

/// The secret kept under `key`, or `None` when there is none.
pub fn get(key: &str) -> Result<Option<String>, Error> {
    (backend()?.get)(check_key(key)?)
}

/// Keep `value` under `key`, replacing what was there.
pub fn set(key: &str, value: &str) -> Result<(), Error> {
    (backend()?.set)(check_key(key)?, value)
}

/// Forget the secret under `key`. Nothing there is not an error.
pub fn remove(key: &str) -> Result<(), Error> {
    (backend()?.remove)(check_key(key)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_is_a_plain_name() {
        for ok in ["refreshToken", "a.b_c-1"] {
            assert_eq!(check_key(ok).unwrap(), ok);
        }
        for bad in ["", "a b", "../x", "a/b", "é"] {
            assert_eq!(check_key(bad).unwrap_err().kind, "option", "{bad:?}");
        }
        assert_eq!(check_key(&"a".repeat(129)).unwrap_err().kind, "option");
    }

    #[test]
    fn a_call_with_no_backend_is_unavailable() {
        assert_eq!(get("k").unwrap_err().kind, "unavailable");
    }
}
