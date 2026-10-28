//! Tailwind's directives in a `<style>` block, compiled the way Vite does it.
//!
//! A block that uses `@apply`, `@reference` or any other Tailwind directive is
//! handed to the `tailwindcss` CLI, run in the `.rux` file's folder so a
//! relative `@reference "../assets/input.css"` means what it says, and what
//! comes back is the block's CSS. A block with no directive never reaches it,
//! so a project without Tailwind never needs the CLI. Decided by the owner on
//! 2026-10-26; see `docs/06-roadmap.md`.
//!
//! **Cached under the project, by content.** The CLI takes half a second, so
//! each result is kept in `.rux-build/tailwind/<hash>.css`, the hash taken
//! over the block's text and the files it names. The path is read through the
//! [`crate::source`] like any other file, which is what lets a built app carry
//! the results: `rux build` compiles every block (see [`precompile`]) and
//! embeds what it wrote, and the app finds them where it looks on the desktop.
//! A phone or a browser never runs Tailwind.
//!
//! What the hash does not see: a file the referenced one imports in turn, and
//! the CLI's version. Editing either keeps the old result until the block or
//! the referenced file changes; deleting `.rux-build/tailwind` starts over.

use std::path::{Path, PathBuf};

/// The directives that make a block Tailwind's to compile. The same list
/// `rux_style` warns about when nothing compiled one.
const DIRECTIVES: &[&str] = &[
    "apply", "reference", "utility", "variant", "custom-variant", "theme", "source", "plugin", "config",
    "tailwind",
];

/// Where compiled blocks are kept, under the project root.
const CACHE_DIR: &str = ".rux-build/tailwind";

/// Whether `css` uses one of Tailwind's directives.
pub fn uses_directives(css: &str) -> bool {
    first_directive(css).is_some()
}

/// Where in `css` the first of Tailwind's directives is.
pub fn first_directive(css: &str) -> Option<usize> {
    let mut from = 0;
    while let Some(at) = css[from..].find('@') {
        let at = from + at;
        let name: String = css[at + 1..].chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '-').collect();
        if DIRECTIVES.contains(&name.as_str()) {
            return Some(at);
        }
        from = at + 1;
    }
    None
}

/// What a block compiled to, or why it did not.
pub enum Outcome {
    /// The block's CSS, every directive gone.
    Compiled(String),
    /// Nothing here can compile it (a browser, a phone) and no result was
    /// carried in: the block is left as written, and its directives warn.
    Unavailable,
    /// The CLI is missing or refused the block. The message is for the author.
    Failed(String),
}

/// Compile `css`, a `<style>` block of a file in `base`, under the project at
/// `root`: from the cache when it is there, else by running the CLI.
pub fn compile(css: &str, base: &Path, root: &Path) -> Outcome {
    let cached = cache_path(root, &hash(css, base));
    if let Ok(found) = crate::source::read_text(&cached) {
        return Outcome::Compiled(found);
    }
    match run_cli(css, base) {
        Ok(Some(out)) => {
            // Kept for next time. A failure to keep it costs only time.
            if let Some(dir) = cached.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(&cached, &out);
            Outcome::Compiled(out)
        }
        Ok(None) => Outcome::Unavailable,
        Err(why) => Outcome::Failed(why),
    }
}

/// Compile every block in `files` (paths under `root`) that needs it, for
/// `rux build`, and return the cache files written, relative to `root`, for
/// the build to embed beside the documents.
pub fn precompile(root: &Path, files: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    let mut out = Vec::new();
    for file in files.iter().filter(|f| f.extension().is_some_and(|e| e == "rux")) {
        let path = root.join(file);
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        let Ok(sfc) = rux_parser::parse_sfc(&text) else { continue };
        if !uses_directives(&sfc.style) {
            continue;
        }
        let base = path.parent().unwrap_or(root);
        match compile(&sfc.style, base, root) {
            Outcome::Compiled(_) => {
                let relative = Path::new(CACHE_DIR).join(format!("{}.css", hash(&sfc.style, base)));
                if !out.contains(&relative) {
                    out.push(relative);
                }
            }
            Outcome::Unavailable => {}
            Outcome::Failed(why) => return Err(format!("{}: {why}", file.display())),
        }
    }
    Ok(out)
}

fn cache_path(root: &Path, hash: &str) -> PathBuf {
    root.join(CACHE_DIR).join(format!("{hash}.css"))
}

/// FNV-1a over the block and the contents of every local file it names, so
/// editing the theme a block `@reference`s compiles it again. Paths are not
/// hashed, only what is in them: the build and the phone must agree, and
/// they keep the project in different places.
pub(crate) fn hash(css: &str, base: &Path) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut eat = |bytes: &[u8]| {
        for b in bytes {
            h ^= u64::from(*b);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        // A separator, so moving text between the parts changes the hash.
        h ^= 0xff;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    };
    eat(css.as_bytes());
    for named in local_files(css) {
        if let Ok(text) = crate::source::read_text(&lexical(&base.join(named))) {
            eat(text.as_bytes());
        }
    }
    format!("{h:016x}")
}

/// `components/../assets/a.css` as `assets/a.css`. A disk resolves the `..`
/// itself; a built app's embedded files are keyed by the path as the build
/// walked it, and missing the theme there gave the block another hash, so a
/// built app looked for a CLI it does not have.
fn lexical(path: &Path) -> PathBuf {
    use std::path::Component;
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir if matches!(out.components().next_back(), Some(Component::Normal(_))) => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

/// The files a block names by a relative path: `@reference "../a.css"`,
/// `@import "./b.css"`, `@plugin "./p.js"`, `@config "./t.js"`. A bare name
/// (`"tailwindcss"`) is a package, not a file of the project's.
fn local_files(css: &str) -> Vec<&str> {
    let mut out = Vec::new();
    for line in css.lines() {
        let line = line.trim_start();
        let Some(rest) = ["@reference", "@import", "@plugin", "@config"].iter().find_map(|d| line.strip_prefix(d)) else {
            continue;
        };
        let rest = rest.trim_start();
        let Some(quote) = rest.chars().next().filter(|c| *c == '"' || *c == '\'') else { continue };
        let Some(end) = rest[1..].find(quote) else { continue };
        let path = &rest[1..1 + end];
        if path.starts_with("./") || path.starts_with("../") {
            out.push(path);
        }
    }
    out
}

/// Run the CLI over `css` in `base`. `Ok(None)` where no process can be run.
#[cfg(any(target_arch = "wasm32", target_os = "android"))]
fn run_cli(_css: &str, _base: &Path) -> Result<Option<String>, String> {
    Ok(None)
}

#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
fn run_cli(css: &str, base: &Path) -> Result<Option<String>, String> {
    use std::io::Write;
    use std::process::{Command, Stdio};
    // A document opened by a bare name has an empty folder, which a process
    // cannot be started in.
    let dir = if base.as_os_str().is_empty() { Path::new(".") } else { base };
    let child = Command::new("tailwindcss")
        .args(["--input", "-"])
        .current_dir(dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();
    let mut child = match child {
        Ok(child) => child,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err("this <style> uses Tailwind's directives, which the `tailwindcss` CLI \
                 compiles, and it is not on PATH.\n\nTailwind's standalone CLI needs no Node: \
                 https://github.com/tailwindlabs/tailwindcss/releases (keep it outside the \
                 project, or a release build carries it)"
                .to_string())
        }
        Err(e) => return Err(format!("could not run `tailwindcss`: {e}")),
    };
    // Written from here and read below; the block is small enough that the
    // pipe never fills before the CLI starts reading.
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(css.as_bytes()).map_err(|e| format!("writing to `tailwindcss`: {e}"))?;
    }
    let output = child.wait_with_output().map_err(|e| format!("waiting for `tailwindcss`: {e}"))?;
    if !output.status.success() {
        return Err(cli_error(&String::from_utf8_lossy(&output.stderr)));
    }
    String::from_utf8(output.stdout)
        .map(|css| Some(strip_banner(&css)))
        .map_err(|e| format!("`tailwindcss` wrote something that is not text: {e}"))
}

/// The line of the CLI's complaint that says what is wrong, without its
/// colours and box drawing: `Cannot apply unknown utility class `bg-gren-500``.
#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
fn cli_error(stderr: &str) -> String {
    let plain = strip_ansi(stderr);
    let said = plain
        .lines()
        .map(|l| l.trim_start_matches(['│', '┌', '└', ' ']).trim())
        .find_map(|l| l.strip_prefix("Error:").map(str::trim).filter(|l| !l.is_empty()))
        .map(str::to_string);
    format!("Tailwind could not compile this <style>: {}", said.unwrap_or_else(|| plain.trim().to_string()))
}

#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            // ESC [ … letter
            for c in chars.by_ref() {
                if c.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Without the `/*! tailwindcss … */` licence line, which is one per block
/// and says nothing a reader of the app needs.
#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
fn strip_banner(css: &str) -> String {
    match css.trim_start().strip_prefix("/*!") {
        Some(rest) if rest.trim_start().starts_with("tailwindcss") => match rest.find("*/") {
            Some(end) => rest[end + 2..].trim_start().to_string(),
            None => css.to_string(),
        },
        _ => css.to_string(),
    }
}

/// The class a complaint names in backticks, so the error can point at the
/// line of the block it is on.
pub fn named_in(message: &str) -> Option<&str> {
    let start = message.find('`')? + 1;
    let end = message[start..].find('`')? + start;
    Some(&message[start..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_directive_is_found_and_ordinary_css_is_not() {
        assert!(uses_directives(".a { @apply px-4; }"));
        assert!(uses_directives("@reference \"../x.css\";"));
        assert!(!uses_directives("@media (width >= 40rem) { .a { color: red; } } @layer base { }"));
        assert!(!uses_directives(".a { color: red; }"));
    }

    #[test]
    fn only_relative_paths_are_the_projects_files() {
        let css = "@reference \"../assets/in.css\";\n@import \"tailwindcss\";\n@plugin './p.js';";
        assert_eq!(local_files(css), vec!["../assets/in.css", "./p.js"]);
    }

    #[test]
    fn the_clis_error_is_its_one_line() {
        let stderr = "\u{1b}[31mError:\u{1b}[39m\n\u{1b}[2m┌\u{1b}[22m\n\u{1b}[2m│\u{1b}[22m Error: Cannot apply unknown utility class `bg-gren-500`\n\u{1b}[2m└\u{1b}[22m\n";
        let said = cli_error(stderr);
        assert!(said.ends_with("Cannot apply unknown utility class `bg-gren-500`"), "{said}");
        assert_eq!(named_in(&said), Some("bg-gren-500"));
    }

    #[test]
    fn a_parent_step_is_resolved_before_the_file_is_read() {
        assert_eq!(lexical(Path::new("components/../assets/in.css")), PathBuf::from("assets/in.css"));
        assert_eq!(lexical(Path::new("./a/./b.css")), PathBuf::from("a/b.css"));
        assert_eq!(lexical(Path::new("../outside.css")), PathBuf::from("../outside.css"));
    }

    /// The build reads the disk and a built app its embedded files, keyed
    /// as the build walked them; a component's block must hash the same in
    /// both, or the app misses what was carried in.
    #[test]
    fn a_component_block_hashes_the_same_built_as_on_disk() {
        let dir = std::env::temp_dir().join(format!("rux_tw_hash_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("components")).unwrap();
        std::fs::create_dir_all(dir.join("assets")).unwrap();
        std::fs::write(dir.join("assets/in.css"), "@theme { --color-x: red; }").unwrap();
        let css = "@reference \"../assets/in.css\";\n.a { @apply text-x; }";
        let on_disk = hash(css, &dir.join("components"));

        let mut files = crate::source::MemorySource::new();
        files.insert("assets/in.css", b"@theme { --color-x: red; }".to_vec());
        let previous = crate::source::set_source(std::rc::Rc::new(files));
        let built = hash(css, Path::new("components"));
        crate::source::set_source(previous);
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(on_disk, built);
    }

    #[test]
    fn the_banner_goes() {
        let css = "/*! tailwindcss v4.3.3 | MIT License | https://tailwindcss.com */\n.a {\n  color: red;\n}\n";
        assert_eq!(strip_banner(css), ".a {\n  color: red;\n}\n");
    }
}
