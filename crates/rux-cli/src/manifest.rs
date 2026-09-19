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
}

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

        Ok(Manifest { root, name, id, version, entry })
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
