//! `secrets.get`, `secrets.set` and `secrets.remove`, and `rux::secrets` for
//! the app's own Rust (UD-007): a string kept where the platform protects it.
//!
//! - **Windows:** DPAPI (`CryptProtectData`), readable only by the same person
//!   on the same machine, with the app's id as extra entropy so another app of
//!   theirs cannot open it. The sealed bytes are kept, as hex, in
//!   `secrets.json` beside `settings.json`.
//! - **Android:** an AES-GCM key in the `AndroidKeyStore` (it never leaves it
//!   and goes with the app), the ciphertext in a file under the app's
//!   `noBackupFilesDir`, which a backup leaves out. `RuxActivity.ruxSecret`.
//! - **macOS, Linux and the web:** `unsupported`. Nothing is kept rather than
//!   kept where anyone can read it.

use std::collections::HashMap;

use rux_runtime::native::secrets::{self, set_backend, Backend};
use rux_runtime::native::{install, Any, Call, Error, Module};

fn text_arg(name: &str, what: &str, v: Option<&Any>) -> Result<String, Error> {
    match v {
        Some(Any::Str(s)) => Ok(s.clone()),
        Some(other) => Err(Error::new("type", format!("secrets.{name} takes {what}, not a {}", other.kind()))),
        None => Err(Error::new("type", format!("secrets.{name} takes {what}"))),
    }
}

/// Install the calls for every document this process loads, and the same for
/// the app's own Rust (`rux::secrets`).
pub(crate) fn install_secrets() {
    set_backend(Backend { get: platform::get, set: platform::set, remove: platform::remove });
    let mut calls = HashMap::new();
    calls.insert(
        "get".to_string(),
        Call::sync(|args: Vec<Any>| {
            let key = text_arg("get", "a key", args.first())?;
            Ok(secrets::get(&key)?.map_or(Any::None, Any::Str))
        }),
    );
    calls.insert(
        "set".to_string(),
        Call::sync(|args: Vec<Any>| {
            let key = text_arg("set", "a key", args.first())?;
            let value = text_arg("set", "a string to keep", args.get(1))?;
            secrets::set(&key, &value)?;
            Ok(Any::None)
        }),
    );
    calls.insert(
        "remove".to_string(),
        Call::sync(|args: Vec<Any>| {
            secrets::remove(&text_arg("remove", "a key", args.first())?)?;
            Ok(Any::None)
        }),
    );
    let _ = install(Module { interface: rux_runtime::builtins::secrets_interface(), calls });
}

#[cfg(not(any(windows, target_os = "android")))]
mod platform {
    use super::*;

    fn unsupported<T>() -> Result<T, Error> {
        Err(Error::new("unsupported", "secrets are kept on Android and Windows only: here there is no protected store to keep them in"))
    }

    pub(super) fn get(_: &str) -> Result<Option<String>, Error> {
        unsupported()
    }
    pub(super) fn set(_: &str, _: &str) -> Result<(), Error> {
        unsupported()
    }
    pub(super) fn remove(_: &str) -> Result<(), Error> {
        unsupported()
    }
}

#[cfg(windows)]
mod platform {
    use super::*;
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    use windows::Win32::Foundation::{LocalFree, HLOCAL};
    use windows::Win32::Security::Cryptography::{CryptProtectData, CryptUnprotectData, CRYPT_INTEGER_BLOB};

    /// Serialises the read, change, write of the file.
    static LOCK: Mutex<()> = Mutex::new(());

    fn file() -> Result<std::path::PathBuf, Error> {
        crate::settings::data_dir()
            .map(|d| d.join("secrets.json"))
            .ok_or_else(|| Error::new("io", "there is nowhere to keep secrets: no home folder"))
    }

    fn read() -> BTreeMap<String, String> {
        file()
            .ok()
            .and_then(|f| std::fs::read_to_string(f).ok())
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    fn write(all: &BTreeMap<String, String>) -> Result<(), Error> {
        let file = file()?;
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir).map_err(Error::from_rust)?;
        }
        let part = file.with_extension("json.part");
        std::fs::write(&part, serde_json::to_string_pretty(all).map_err(Error::from_rust)?).map_err(Error::from_rust)?;
        std::fs::rename(&part, &file).map_err(Error::from_rust)
    }

    fn entropy() -> Vec<u8> {
        format!("rux-secrets:{}", crate::settings::app_id()).into_bytes()
    }

    fn blob(bytes: &[u8]) -> CRYPT_INTEGER_BLOB {
        CRYPT_INTEGER_BLOB { cbData: bytes.len() as u32, pbData: bytes.as_ptr() as *mut u8 }
    }

    /// What a DPAPI call gave, copied out and the system's copy freed.
    fn take(out: CRYPT_INTEGER_BLOB) -> Vec<u8> {
        // Safety: DPAPI allocated `cbData` bytes at `pbData`, which are ours to free.
        unsafe {
            let bytes = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
            let _ = LocalFree(Some(HLOCAL(out.pbData.cast())));
            bytes
        }
    }

    pub(crate) fn seal(plain: &[u8]) -> Result<Vec<u8>, Error> {
        let (input, extra) = (blob(plain), entropy());
        let extra = blob(&extra);
        let mut out = CRYPT_INTEGER_BLOB::default();
        // Safety: the blobs point at live slices for the call.
        unsafe { CryptProtectData(&input, None, Some(&extra), None, None, 0, &mut out) }
            .map_err(|e| Error::new("io", format!("Windows would not protect the secret: {e}")))?;
        Ok(take(out))
    }

    pub(crate) fn open(sealed: &[u8]) -> Result<Vec<u8>, Error> {
        let (input, extra) = (blob(sealed), entropy());
        let extra = blob(&extra);
        let mut out = CRYPT_INTEGER_BLOB::default();
        // Safety: as in `seal`.
        unsafe { CryptUnprotectData(&input, None, Some(&extra), None, None, 0, &mut out) }
            .map_err(|e| Error::new("io", format!("Windows would not open the secret: {e}")))?;
        Ok(take(out))
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    fn unhex(text: &str) -> Option<Vec<u8>> {
        (text.len() % 2 == 0)
            .then(|| (0..text.len()).step_by(2).map(|i| u8::from_str_radix(text.get(i..i + 2)?, 16).ok()).collect())
            .flatten()
    }

    pub(super) fn get(key: &str) -> Result<Option<String>, Error> {
        let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let Some(sealed) = read().get(key).and_then(|h| unhex(h)) else { return Ok(None) };
        let plain = open(&sealed)?;
        String::from_utf8(plain).map(Some).map_err(|_| Error::new("io", "the secret is not text"))
    }

    pub(super) fn set(key: &str, value: &str) -> Result<(), Error> {
        let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let sealed = seal(value.as_bytes())?;
        let mut all = read();
        all.insert(key.to_string(), hex(&sealed));
        write(&all)
    }

    pub(super) fn remove(key: &str) -> Result<(), Error> {
        let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut all = read();
        if all.remove(key).is_some() {
            write(&all)?;
        }
        Ok(())
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn a_sealed_secret_is_not_its_text_and_opens_again() {
            let sealed = seal(b"token-123").unwrap();
            assert!(!sealed.windows(9).any(|w| w == b"token-123"), "the text is not in what is kept");
            assert_eq!(open(&sealed).unwrap(), b"token-123");
            let mut spoiled = sealed.clone();
            *spoiled.last_mut().unwrap() ^= 1;
            assert!(open(&spoiled).is_err(), "a changed blob is refused");
        }

        #[test]
        fn a_secret_is_kept_replaced_and_forgotten_in_a_file_that_does_not_hold_its_text() {
            let dir = std::env::temp_dir().join(format!("rux_secrets_{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            // SAFETY: only this test reads APPDATA in this process while it runs.
            unsafe { std::env::set_var("APPDATA", &dir) };
            assert_eq!(get("refreshToken").unwrap(), None);
            set("refreshToken", "abc-123").unwrap();
            assert_eq!(get("refreshToken").unwrap(), Some("abc-123".into()));
            set("refreshToken", "def-456").unwrap();
            set("other", "x").unwrap();
            assert_eq!(get("refreshToken").unwrap(), Some("def-456".into()));
            let kept = std::fs::read_to_string(file().unwrap()).unwrap();
            assert!(!kept.contains("def-456") && !kept.contains("abc-123"), "{kept}");
            remove("refreshToken").unwrap();
            remove("refreshToken").unwrap();
            assert_eq!(get("refreshToken").unwrap(), None);
            assert_eq!(get("other").unwrap(), Some("x".into()));
            let _ = std::fs::remove_dir_all(&dir);
        }

        #[test]
        fn hex_round_trips() {
            assert_eq!(unhex(&hex(&[0, 255, 16])), Some(vec![0, 255, 16]));
            assert_eq!(unhex("abc"), None);
            assert_eq!(unhex("zz"), None);
        }
    }
}

#[cfg(target_os = "android")]
mod platform {
    use super::*;

    /// `RuxActivity.ruxSecret(op, key, value)`: 0 gets, 1 sets, 2 removes. It
    /// answers the secret, `null` for none, or `"\0" + message` when the
    /// Keystore or the file failed.
    fn call(op: i32, key: &str, value: &str) -> Result<Option<String>, Error> {
        let answer = crate::android_secret(op, key, value)
            .ok_or_else(|| Error::new("unavailable", "the activity could not be asked for a secret"))?;
        match answer {
            Some(text) => match text.strip_prefix('\0') {
                Some(message) => Err(Error::new("io", message.to_string())),
                None => Ok(Some(text)),
            },
            None => Ok(None),
        }
    }

    pub(super) fn get(key: &str) -> Result<Option<String>, Error> {
        call(0, key, "")
    }
    pub(super) fn set(key: &str, value: &str) -> Result<(), Error> {
        call(1, key, value).map(|_| ())
    }
    pub(super) fn remove(key: &str) -> Result<(), Error> {
        call(2, key, "").map(|_| ())
    }
}
