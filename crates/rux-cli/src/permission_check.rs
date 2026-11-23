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

/// What `rux.toml` declared for notifications, as `rux check` reads it.
pub struct Declared<'a> {
    /// The ids under `[channels]`.
    pub channels: Vec<&'a str>,
    /// Whether a `[background]` table is there.
    pub background: bool,
}

/// The text of the call whose `(` is at `open`, up to its matching `)`.
fn call_args(text: &str, open: usize) -> &str {
    let mut depth = 0usize;
    let mut quote: Option<char> = None;
    for (at, c) in text[open..].char_indices() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'' | '`') => quote = Some(c),
            (None, '(') => depth += 1,
            (None, ')') => {
                depth -= 1;
                if depth == 0 {
                    return &text[open + 1..open + at];
                }
            }
            _ => {}
        }
    }
    &text[open + 1..]
}

/// The literal after `key:` in an options map, `channel: "jobs"`.
fn literal_after_key(args: &str, key: &str) -> Option<String> {
    let mut from = 0;
    while let Some(at) = args[from..].find(key) {
        let start = from + at;
        from = start + key.len();
        let boundary = args[..start].chars().next_back().map_or(true, |c| !(c.is_alphanumeric() || c == '_'));
        let rest = args[from..].trim_start();
        let Some(rest) = rest.strip_prefix(':').filter(|_| boundary) else { continue };
        let rest = rest.trim_start();
        let quote = rest.chars().next().filter(|c| matches!(c, '"' | '\'' | '`'))?;
        let end = rest[1..].find(quote)?;
        return Some(rest[1..1 + end].to_string());
    }
    None
}

/// The nth (from 0) string literal in `args`, for `Notice::new("id", "channel", "title")`.
fn nth_literal(args: &str, n: usize) -> Option<String> {
    let mut found = 0;
    let mut at = 0;
    while at < args.len() {
        let c = args[at..].chars().next()?;
        if matches!(c, '"' | '\'' | '`') {
            let end = args[at + 1..].find(c)?;
            if found == n {
                return Some(args[at + 1..at + 1 + end].to_string());
            }
            found += 1;
            at += end + 2;
        } else {
            at += c.len_utf8();
        }
    }
    None
}

fn undeclared_channel(name: &str, declared: &Declared) -> Option<String> {
    if declared.channels.contains(&name) {
        return None;
    }
    let known = if declared.channels.is_empty() {
        "rux.toml declares no channels yet".to_string()
    } else {
        format!("it declares {}", declared.channels.join(", "))
    };
    Some(format!(
        "`{name}` is not a declared channel ({known}): add `[channels.{name}]` with a `name` and an `importance` to rux.toml"
    ))
}

/// `(line, message)` for each call in `text` that posts to a channel
/// `rux.toml` does not declare, or starts background work in an app with no
/// `[background]`. Reads documents (`notifications.post({ channel: "jobs" })`)
/// and the app's Rust (`Notice::new("id", "jobs", "title")`), by the same plain
/// read as [`undeclared_uses`].
pub fn undeclared_notifications(text: &str, declared: &Declared) -> Vec<(usize, String)> {
    let line_of = |at: usize| text[..at].bytes().filter(|b| *b == b'\n').count() + 1;
    let mut out = Vec::new();
    for (prefix, call) in [
        ("notifications.post", "post"),
        ("notifications::post", "post"),
        ("background.start", "start"),
        ("background::start", "start"),
    ] {
        let mut from = 0;
        while let Some(at) = text[from..].find(prefix) {
            let start = from + at;
            from = start + prefix.len();
            if text[..start].chars().next_back().is_some_and(|c| c.is_alphanumeric() || c == '_') {
                continue;
            }
            let after = &text[from..];
            let trimmed = after.trim_start();
            if !trimmed.starts_with('(') {
                continue;
            }
            let open = from + (after.len() - trimmed.len());
            let args = call_args(text, open);
            let name = prefix.split(['.', ':']).next().unwrap_or_default();
            if name == "background" && !declared.background {
                out.push((
                    line_of(start),
                    "background.start: rux.toml has no `[background]`: add `types = [\"dataSync\"]` (or `location`, ...) to say what the service is for".into(),
                ));
            }
            let channel = literal_after_key(args, "channel")
                .or_else(|| args.find("Notice::new(").and_then(|n| nth_literal(&args[n..], 1)));
            if let Some(why) = channel.and_then(|c| undeclared_channel(&c, declared)) {
                out.push((line_of(start), format!("{name}.{call}: {why}")));
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

/// `(line, message)` for each `location.current` / `location.watch` call (or
/// `location::current` / `location::watch` in the app's Rust) in `text` when
/// `rux.toml` has no `[permissions.location]`; `undeclared` is the message
/// that says what to add, from [`Permissions::undeclared`].
pub fn undeclared_location(text: &str, undeclared: Option<&str>) -> Vec<(usize, String)> {
    let Some(why) = undeclared else { return Vec::new() };
    let mut out = Vec::new();
    for (index, line) in text.lines().enumerate() {
        for prefix in ["location.current", "location.watch", "location::current", "location::watch"] {
            let mut from = 0;
            while let Some(at) = line[from..].find(prefix) {
                let start = from + at;
                from = start + prefix.len();
                // `window.location.watch(...)` and `my_location::current` are not ours.
                let before = line[..start].chars().next_back();
                if before.is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '.' || c == ':') {
                    continue;
                }
                if line[from..].trim_start().starts_with('(') {
                    out.push((index + 1, format!("{prefix}: {why}")));
                }
            }
        }
    }
    out
}

/// `(line, message)` for each `links.open("...")`, `links.dial("...")` or
/// `links.sms("...", ...)` call (or `links::` in the app's Rust) whose first
/// argument is a literal Rux would refuse when the program runs: a scheme that
/// does not open, or a phone number with no digits. A value built at run time
/// is not read, and the call still throws.
pub fn refused_links(text: &str) -> Vec<(usize, String)> {
    use rux_runtime::native::links::{check_url, tel_url};
    let mut out = Vec::new();
    for (index, line) in text.lines().enumerate() {
        for (prefix, call) in [
            ("links.open", "open"),
            ("links::open", "open"),
            ("links.dial", "dial"),
            ("links::dial", "dial"),
            ("links.sms", "sms"),
            ("links::sms", "sms"),
        ] {
            let mut from = 0;
            while let Some(at) = line[from..].find(prefix) {
                let start = from + at;
                from = start + prefix.len();
                if line[..start].chars().next_back().is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '.' || c == ':') {
                    continue;
                }
                let Some(args) = line[from..].trim_start().strip_prefix('(') else { continue };
                let args = args.trim_start();
                let Some(quote) = args.chars().next().filter(|c| matches!(c, '"' | '\'' | '`')) else { continue };
                let Some(end) = args[1..].find(quote) else { continue };
                let literal = &args[1..1 + end];
                if literal.contains("${") {
                    continue;
                }
                let refused = match call {
                    "open" => check_url(literal).err(),
                    _ => tel_url(literal).err(),
                };
                if let Some(e) = refused {
                    out.push((index + 1, format!("{prefix}: {}", e.message)));
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
    #[test]
    fn a_post_to_an_undeclared_channel_or_a_background_with_no_table_is_found() {
        let d = Declared { channels: vec!["jobs"], background: false };
        let text = "notifications.post({ id: \"a\", channel: \"jobs\", title: \"Hi\" });\nnotifications.post({\n  id: \"b\",\n  channel: \"shift\",\n});\nbackground.start({ channel: \"jobs\", title: \"On\" });\n";
        let found = undeclared_notifications(text, &d);
        assert_eq!(found.len(), 2, "{found:?}");
        assert_eq!(found[0].0, 2);
        assert!(found[0].1.contains("[channels.shift]") && found[0].1.contains("it declares jobs"), "{}", found[0].1);
        assert_eq!(found[1].0, 6);
        assert!(found[1].1.contains("no `[background]`"), "{}", found[1].1);

        let rust = "rux::notifications::post(Notice::new(\"a\", \"nope\", \"Hi\").text(\"x\"))?;";
        let found = undeclared_notifications(rust, &Declared { channels: vec![], background: true });
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].1.contains("declares no channels"), "{}", found[0].1);
    }

    #[test]
    fn a_channel_built_or_declared_is_left_alone() {
        let d = Declared { channels: vec!["jobs"], background: true };
        assert!(undeclared_notifications("notifications.post({ channel: name })", &d).is_empty());
        assert!(undeclared_notifications("notifications.post({ channel: \"jobs\" }); background.start({ channel: \"jobs\" });", &d).is_empty());
        assert!(undeclared_notifications("xnotifications.post({ channel: \"x\" })", &d).is_empty());
    }
    #[test]
    fn a_location_call_without_the_permission_declared_is_found() {
        let none = declared("");
        let found = undeclared_location(
            "let f = await location.current({});
location.watch({ intervalMs: 1 });
window.location.watch(x);
location::current(Options::new()).await?;
",
            none.undeclared("location").as_deref(),
        );
        assert_eq!(found.iter().map(|f| f.0).collect::<Vec<_>>(), [1, 2, 4], "{found:?}");
        assert!(found[0].1.contains("[permissions.location]"), "{}", found[0].1);
        let with = declared("
[permissions.location]
why = \"x\"
");
        assert!(undeclared_location("location.current({})", with.undeclared("location").as_deref()).is_empty());
        assert!(undeclared_location("let location = 3; location.position", none.undeclared("location").as_deref()).is_empty());
    }
    #[test]
    fn a_link_literal_rux_would_refuse_is_found() {
        let found = refused_links(
            "links.open(\"https://example.com\");\nlinks.open(\"file:///etc/passwd\");\nlinks.dial(\"call me\");\nlinks.dial(\"+256 700 000 000\");\nlinks.open(url);\nlinks::sms(\"x\", \"hi\");\nxlinks.open(\"javascript:1\");\n",
        );
        assert_eq!(found.iter().map(|f| f.0).collect::<Vec<_>>(), [2, 3, 6], "{found:?}");
        assert!(found[0].1.contains("is not a scheme Rux opens"), "{}", found[0].1);
        assert!(found[1].1.contains("a phone number is digits"), "{}", found[1].1);
    }
}
