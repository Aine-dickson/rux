//! `rux check`'s half of permissions: a call that names a permission
//! `rux.toml` does not declare, found before the app runs.
//!
//! A plain read of the text, like the scan for `INTERNET`: a name a script
//! builds from pieces is missed, and one in a comment is counted. Both are
//! cheap, and the runtime still throws `undeclared` for whatever this misses.
//! It reads documents (`permissions.status("camera")`) and the app's own Rust
//! (`permissions::status("camera")`), which is where a background service
//! asks.

use std::path::Path;

use crate::manifest::Permissions;

/// `(line, message)` for each call in `text` that names an undeclared permission.
pub fn undeclared_uses(text: &str, permissions: &Permissions) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    for (index, line) in text.lines().enumerate() {
        for prefix in ["permissions.", "permissions::"] {
            let mut rest = line;
            while let Some(at) = rest.find(prefix) {
                let after = &rest[at + prefix.len()..];
                rest = after;
                let Some(call) = ["status", "request", "why"].iter().find(|c| after.starts_with(**c)) else {
                    continue;
                };
                let args = after[call.len()..].trim_start();
                let Some(args) = args.strip_prefix('(') else { continue };
                let args = args.trim_start();
                let Some(quote) = args.chars().next().filter(|c| matches!(c, '"' | '\'' | '`')) else {
                    continue;
                };
                let Some(end) = args[1..].find(quote) else { continue };
                let name = &args[1..1 + end];
                if let Some(why) = permissions.undeclared(name) {
                    out.push((index + 1, format!("permissions.{call}: {why}")));
                }
            }
        }
    }
    out
}

/// The `.rs` files under `native/src`, for the calls an app's Rust makes.
pub fn native_sources(root: &Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.join(crate::native::DIR).join("src")];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::Manifest;

    fn declared(extra: &str) -> Permissions {
        let text = format!(
            "[app]\nname = \"A\"\nid = \"dev.example.a\"\nversion = \"0.1.0\"\nentry = \"Cargo.toml\"\n{extra}"
        );
        Manifest::parse(&text, std::path::PathBuf::from(".")).unwrap().permissions
    }

    #[test]
    fn a_call_naming_an_undeclared_permission_is_found_with_its_line() {
        let p = declared("\n[permissions.camera]\nwhy = \"Photos\"\n");
        let found = undeclared_uses(
            "async fn a() {\n  let c = await permissions.request(\"camera\");\n  let m = await permissions.status(`microphone`);\n}\n",
            &p,
        );
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].0, 3);
        assert!(found[0].1.contains("[permissions.microphone]"), "{}", found[0].1);
    }

    #[test]
    fn background_needs_its_flag_and_the_rust_spelling_is_read_too() {
        let plain = declared("\n[permissions.location]\nwhy = \"x\"\n");
        let found = undeclared_uses("permissions::request(\"location-background\").await?;", &plain);
        assert_eq!(found.len(), 1);
        assert!(found[0].1.contains("background = true"));
        let with = declared("\n[permissions.location]\nwhy = \"x\"\nbackground = true\n");
        assert!(undeclared_uses("permissions::request(\"location-background\")", &with).is_empty());
    }

    #[test]
    fn a_name_that_is_built_or_not_a_call_is_left_alone() {
        let p = declared("");
        assert!(undeclared_uses("permissions.request(name)", &p).is_empty());
        assert!(undeclared_uses("let permissions = 3; permissions.openSettings();", &p).is_empty());
    }
}
