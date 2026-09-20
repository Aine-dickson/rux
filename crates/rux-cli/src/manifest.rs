//! `rux.toml`: what a project is called and what it builds into.
//!
//! The manifest deliberately says as little as possible. Whatever it says in
//! v0.8 is a surface that cannot be taken back, and a field the runtime cannot
//! honor is a promise broken on arrival, so a key earns its place by being
//! needed by something that already works.
//!
//! Four keys do:
//!
//! - `name`, what the built thing is called to a person.
//! - `id`, reverse-DNS. Android requires one and refuses to install without it,
//!   and inventing it at packaging time would mean it changed whenever the
//!   packaging did, which for Android is the difference between an update and a
//!   different app.
//! - `version`, the app's own, which is not Rux's.
//! - `entry`, the document to open. It defaults to the same `app.rux` /
//!   `index.rux` that `rux run` already looks for, so a project that wants the
//!   convention writes nothing.
//!
//! **Deliberately absent, and each for the same reason.** `icon` and `splash`
//! name assets nothing yet draws. `[signing]` configures a keystore no code
//! creates. `permissions` would be a list the runtime cannot request, since no
//! Rux API needs one. Every one of them is scheduled, and each lands in the
//! commit that makes it do something rather than ahead of it.
//!
//! The manifest is also what makes a project root explicit. `workspace_root`
//! infers one today by finding `app.rux` while walking up, which is a guess
//! that happens to be right; `rux.toml` is the file that says so.

use std::path::{Path, PathBuf};

/// The file that marks a project root.
pub const MANIFEST: &str = "rux.toml";

/// The entry documents looked for when the manifest names none, in the order
/// `rux run` already prefers them.
const DEFAULT_ENTRIES: [&str; 2] = ["app.rux", "index.rux"];

#[derive(Debug, Clone)]
pub struct Manifest {
    /// The directory the manifest was found in. Every path in the manifest is
    /// relative to this, never to the working directory, so `rux build` from a
    /// subdirectory builds the same thing as from the root.
    pub root: PathBuf,
    pub name: String,
    pub id: String,
    pub version: String,
    /// Relative to `root`.
    pub entry: PathBuf,
    /// The key a release is signed with, when the author has one.
    ///
    /// `None` means a release is signed with the shared debug key, which is
    /// enough to install and not enough to publish.
    pub signing: Option<Signing>,
}

/// Where a release build's signing key lives.
///
/// **No passwords, and that is the whole design.** `rux.toml` is a file people
/// commit, and a keystore password committed beside the keystore it opens is
/// the same as no password at all. The passwords come from the environment, so
/// the manifest can say which key to use without saying how to open it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Signing {
    /// The keystore, relative to the project root unless it is absolute.
    pub keystore: PathBuf,
    /// Which key inside it.
    pub alias: String,
}

/// The environment variable holding the keystore's password.
pub const KEYSTORE_PASSWORD: &str = "RUX_KEYSTORE_PASSWORD";

/// The environment variable holding the key's own password.
///
/// Falls back to [`KEYSTORE_PASSWORD`], because the two are the same in most
/// keystores and making someone set an identical value twice is a way to be
/// asked why.
pub const KEY_PASSWORD: &str = "RUX_KEY_PASSWORD";

impl Manifest {
    /// Find the manifest by walking up from `from`, the way `rux run` finds an
    /// entry point, so the command works from anywhere inside the project.
    pub fn find(from: &Path) -> Result<Self, String> {
        let mut dir = Some(from);
        while let Some(current) = dir {
            let candidate = current.join(MANIFEST);
            if candidate.is_file() {
                return Self::read(&candidate);
            }
            dir = current.parent();
        }
        Err(format!(
            "no {MANIFEST} here or in any parent directory.\n\
             A build needs one: it is what says what the app is called and what its id is.\n\
             Create it beside your entry document:\n\
             \n\
             [app]\n\
             name = \"My App\"\n\
             id = \"dev.example.myapp\"\n\
             version = \"0.1.0\""
        ))
    }

    pub fn read(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("reading {}: {e}", path.display()))?;
        let root = path.parent().unwrap_or_else(|| Path::new(".")).to_path_buf();
        Self::parse(&text, root).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// Parsed out of a `toml::Value` by hand rather than derived.
    ///
    /// Deriving would pull `serde_derive` and a proc-macro build into a crate
    /// that reads one four-key table, and the compile-time cost of the toolchain
    /// is something this milestone is explicitly judged on. Reading the keys
    /// also lets each one fail in its own words.
    pub fn parse(text: &str, root: PathBuf) -> Result<Self, String> {
        let value: toml::Table = text.parse().map_err(|e| format!("{e}"))?;
        let app = value
            .get("app")
            .ok_or("no [app] table. Every manifest needs one; it is where name, id and version live")?
            .as_table()
            .ok_or("[app] must be a table")?;

        let string = |key: &str| -> Result<String, String> {
            match app.get(key) {
                None => Err(format!("[app] has no `{key}`")),
                Some(toml::Value::String(s)) if !s.trim().is_empty() => Ok(s.clone()),
                Some(toml::Value::String(_)) => Err(format!("[app] `{key}` is empty")),
                Some(_) => Err(format!("[app] `{key}` must be a string")),
            }
        };

        let name = string("name")?;
        let id = string("id")?;
        let version = string("version")?;

        // Reverse-DNS, checked here rather than at packaging time so the message
        // arrives while the author is looking at the manifest. Android rejects
        // an id with no dot in it, and it is the one field that cannot be
        // corrected later without the store treating it as a different app.
        if !id.contains('.') || id.starts_with('.') || id.ends_with('.') {
            return Err(format!(
                "[app] `id` should be reverse-DNS, like `dev.example.myapp`, and `{id}` is not.\n\
                 It is what an operating system files the app under, and changing it later makes \
                 it a different app rather than an update."
            ));
        }

        let entry = match app.get("entry") {
            Some(toml::Value::String(s)) => PathBuf::from(s),
            Some(_) => return Err("[app] `entry` must be a string".into()),
            None => DEFAULT_ENTRIES
                .iter()
                .map(PathBuf::from)
                .find(|candidate| root.join(candidate).is_file())
                .ok_or_else(|| {
                    format!(
                        "[app] names no `entry` and neither {} nor {} is here, \
                         so there is nothing to build",
                        DEFAULT_ENTRIES[0], DEFAULT_ENTRIES[1]
                    )
                })?,
        };

        if !root.join(&entry).is_file() {
            return Err(format!("[app] `entry` names {}, which is not here", entry.display()));
        }

        let signing = match value.get("signing") {
            None => None,
            Some(toml::Value::Table(table)) => {
                let field = |key: &str| -> Result<String, String> {
                    match table.get(key) {
                        Some(toml::Value::String(s)) if !s.trim().is_empty() => Ok(s.clone()),
                        Some(toml::Value::String(_)) => {
                            Err(format!("[signing] `{key}` is empty"))
                        }
                        Some(_) => Err(format!("[signing] `{key}` must be a string")),
                        None => Err(format!(
                            "[signing] has no `{key}`. A signing block needs both `keystore` \
                             and `alias`, or neither"
                        )),
                    }
                };
                // Refused by name, because someone will try it: a password here
                // is a password in version control, beside the keystore it
                // opens. Saying where it goes instead is the whole point of
                // noticing.
                for secret in ["password", "keystore_password", "key_password", "storepass"] {
                    if table.contains_key(secret) {
                        return Err(format!(
                            "[signing] `{secret}` does not belong in {MANIFEST}, which is a file \
                             you commit.\n\nSet {KEYSTORE_PASSWORD} in the environment instead, \
                             and {KEY_PASSWORD} if the key's own password differs."
                        ));
                    }
                }
                Some(Signing { keystore: PathBuf::from(field("keystore")?), alias: field("alias")? })
            }
            Some(_) => return Err("[signing] must be a table".into()),
        };

        Ok(Manifest { root, name, id, version, entry, signing })
    }

    /// The keystore to sign with, and the two passwords, or why not.
    ///
    /// Resolved here rather than at the moment of signing, so a release build
    /// that cannot be signed says so before it spends sixteen minutes
    /// compiling four ABIs.
    pub fn signing_key(&self) -> Result<Option<(PathBuf, String, String, String)>, String> {
        let Some(signing) = &self.signing else { return Ok(None) };
        let keystore = if signing.keystore.is_absolute() {
            signing.keystore.clone()
        } else {
            self.root.join(&signing.keystore)
        };
        if !keystore.is_file() {
            return Err(format!(
                "[signing] names the keystore {}, which is not there.\n\nThe path is relative to \
                 {MANIFEST} unless it is absolute.",
                keystore.display()
            ));
        }
        let store_password = std::env::var(KEYSTORE_PASSWORD).map_err(|_| {
            format!(
                "[signing] names a keystore, and {KEYSTORE_PASSWORD} is not set.\n\nPasswords are \
                 read from the environment rather than {MANIFEST}, because a manifest is a file \
                 you commit."
            )
        })?;
        // The same password unless told otherwise, which is how most keystores
        // are made.
        let key_password = std::env::var(KEY_PASSWORD).unwrap_or_else(|_| store_password.clone());
        Ok(Some((keystore, signing.alias.clone(), store_password, key_password)))
    }

    /// A file-system-safe stem for the artifact, derived from `name`.
    ///
    /// The name is for a person and may hold spaces and capitals; an executable
    /// should not have to.
    pub fn artifact_stem(&self) -> String {
        let stem: String = self
            .name
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
            .collect();
        // Runs of `-` collapse, and the ends are trimmed, so "My App!" is
        // `my-app` and not `my-app-`.
        let mut out = String::with_capacity(stem.len());
        for part in stem.split('-').filter(|p| !p.is_empty()) {
            if !out.is_empty() {
                out.push('-');
            }
            out.push_str(part);
        }
        if out.is_empty() {
            "app".into()
        } else {
            out
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Result<Manifest, String> {
        Manifest::parse(text, PathBuf::from("."))
    }

    const MINIMAL: &str = r#"
[app]
name = "Task List"
id = "dev.example.tasks"
version = "0.1.0"
entry = "Cargo.toml"
"#;

    #[test]
    fn a_signing_block_reads_and_is_optional() {
        assert_eq!(parse(MINIMAL).unwrap().signing, None);
        let text = format!("{MINIMAL}\n[signing]\nkeystore = \"release.jks\"\nalias = \"upload\"\n");
        let signing = parse(&text).unwrap().signing.expect("a signing block");
        assert_eq!(signing.keystore, PathBuf::from("release.jks"));
        assert_eq!(signing.alias, "upload");
    }

    #[test]
    fn a_password_in_the_manifest_is_refused_by_name() {
        // The point of the whole design. `rux.toml` is committed, and a
        // keystore password committed beside the keystore it opens is the same
        // as no password. Every spelling someone might reach for is caught, and
        // the message says where it goes instead.
        for key in ["password", "keystore_password", "key_password", "storepass"] {
            let text = format!(
                "{MINIMAL}\n[signing]\nkeystore = \"r.jks\"\nalias = \"a\"\n{key} = \"hunter2\"\n"
            );
            let error = parse(&text).expect_err("a password should be refused");
            assert!(error.contains(key), "{error}");
            assert!(error.contains(KEYSTORE_PASSWORD), "should say where it goes: {error}");
        }
    }

    #[test]
    fn half_a_signing_block_is_an_error_naming_the_missing_half() {
        let text = format!("{MINIMAL}\n[signing]\nkeystore = \"release.jks\"\n");
        let error = parse(&text).expect_err("alias is required");
        assert!(error.contains("alias"), "{error}");
    }

    #[test]
    fn a_missing_keystore_is_reported_before_anything_is_built() {
        let text = format!("{MINIMAL}\n[signing]\nkeystore = \"nope.jks\"\nalias = \"a\"\n");
        let error = parse(&text).unwrap().signing_key().expect_err("no such keystore");
        assert!(error.contains("nope.jks"), "{error}");
    }

    #[test]
    fn a_minimal_manifest_reads() {
        let m = parse(MINIMAL).expect("should parse");
        assert_eq!(m.name, "Task List");
        assert_eq!(m.id, "dev.example.tasks");
        assert_eq!(m.version, "0.1.0");
    }

    #[test]
    fn an_id_without_a_dot_is_refused_with_the_reason() {
        let err = parse(&MINIMAL.replace("dev.example.tasks", "tasks")).unwrap_err();
        assert!(err.contains("reverse-DNS"), "{err}");
        assert!(err.contains("different app"), "the why should be in the message: {err}");
    }

    #[test]
    fn a_missing_key_names_the_key() {
        let text = MINIMAL.replace("id = \"dev.example.tasks\"\n", "");
        let err = parse(&text).unwrap_err();
        assert!(err.contains("`id`"), "{err}");
    }

    #[test]
    fn a_missing_app_table_says_so() {
        let err = parse("[other]\nx = 1\n").unwrap_err();
        assert!(err.contains("[app]"), "{err}");
    }

    #[test]
    fn the_artifact_stem_is_safe_and_readable() {
        let m = parse(MINIMAL).unwrap();
        assert_eq!(m.artifact_stem(), "task-list");
        let loud = parse(&MINIMAL.replace("Task List", "My App!!")).unwrap();
        assert_eq!(loud.artifact_stem(), "my-app");
    }
}
