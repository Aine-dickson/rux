//! Formatting for `.rux` files.
//!
//! Two different jobs, deliberately:
//!
//! - **`<template>` and `<script>` are only re-indented.** Leading whitespace is
//!   corrected; nothing on a line is rewritten, wrapped or reordered. A `@tap`
//!   handler is rhai, and rearranging someone's expressions is not this tool's
//!   business.
//! - **`<style>` is genuinely formatted**, by [`css`], one space before `{`,
//!   long rules broken one declaration per line, short ones kept inline. CSS has
//!   a conventional shape worth enforcing.
//!
//! The real `rux fmt`, parse to a tree and pretty-print it through
//! `rux-parser` / `rux-style` / `rux-script`, is still the planned replacement
//! (see `docs/06-roadmap.md`, "Dev tooling"). Until then this is what the
//! playground's Format button and the editor's auto-indent run.
//!
//! The indenter began as a port of `editors/vscode/extension.js`, which VS Code
//! still uses; the CSS formatter has no JS counterpart. When `rux fmt` exists as
//! a CLI the extension should shell out to it and the JS copy should go. **Until
//! then, an indenting change here needs the same change there**, and note the
//! JS still has the `<image>` bug described on `VOID_TAGS` below.
//!
//! Where the indenter differs from the JS: the JS blanks strings and comments
//! with a chain of regexes and then re-scans. This walks each line once as a
//! small state machine, which gets multi-line comments right as a consequence
//! rather than as a separate pass.

pub mod css;

/// What a line does to the indent level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Delta {
    /// Net nesting change across the line.
    pub net: i32,
    /// Closers *leading* the line, which dedent the line itself.
    pub leading_close: usize,
}

/// Whether a comment is still open at the end of a line, and what closes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pending {
    None,
    /// `<!-- …`, closed by `-->`.
    Html,
    /// `/* …`, closed by `*/`.
    Block,
}

/// Tags that never nest, so an opening tag must not increase the indent.
///
/// `image` is the one that matters and the one the JS list misses: it inherited
/// HTML's set, which has `img`, but Rux's element is `<image>`. An `<image
/// src="…">` written without a self-closing slash therefore over-indents
/// everything after it, in VS Code today, and here until this test caught it.
/// The HTML names are kept because they cost nothing and pasted markup is
/// common.
const VOID_TAGS: &[&str] = &[
    "image", "input", //
    "area", "base", "br", "col", "embed", "hr", "img", "link", "meta", "param", "source", "track",
    "wbr",
];

/// The tags in [`VOID_TAGS`], for anything outside the formatter that has to
/// agree with it about what never nests: `rux vocab`, and through it the
/// editor's tag auto-closing, which must not write `</image>` after an
/// `<image src="…">`. This is the same drift the doc comment above describes,
/// caught once in the JS formatter and worth not repeating in the JS editor.
pub fn void_tags() -> &'static [&'static str] {
    VOID_TAGS
}

/// Format `text`, using `unit` for one indent level (`"  "`, `"    "`, `"\t"`…).
///
/// The `<template>` and `<script>` sections are only re-indented, nothing on a
/// line is rewritten. The `<style>` section goes through the CSS pretty-printer
/// in [`css`], which does reflow declarations. That split is deliberate: CSS has
/// a shape worth enforcing, while a `@tap` handler is rhai that only its author
/// should be rearranging.
///
/// Line endings are preserved: CRLF in, CRLF out.
pub fn reindent(text: &str, unit: &str) -> String {
    let eol = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let normalised = text.replace("\r\n", "\n");
    let formatted = match style_span(&normalised) {
        Some((open_end, close_start)) => {
            let head = reindent_lines(&normalised[..open_end], unit);
            let body = css::format(&normalised[open_end..close_start], unit, 1);
            let tail = reindent_lines(&normalised[close_start..], unit);
            format!("{}\n{}{}", head.trim_end(), body, tail.trim_start_matches('\n'))
        }
        None => reindent_lines(&normalised, unit),
    };
    if eol == "\r\n" {
        formatted.replace('\n', "\r\n")
    } else {
        formatted
    }
}

/// Byte range *between* `<style>` and `</style>`, if the document has one.
fn style_span(text: &str) -> Option<(usize, usize)> {
    let open = text.find("<style>")? + "<style>".len();
    let close = text[open..].find("</style>")? + open;
    Some((open, close))
}

fn reindent_lines(text: &str, unit: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut depth: i32 = 0;
    let mut pending = Pending::None;

    for raw in text.split('\n') {
        // Inside a multi-line comment the author's alignment is theirs to keep,
        // re-indenting ASCII art or a wrapped sentence would be vandalism.
        if pending != Pending::None {
            out.push(raw.to_string());
            pending = close_pending(raw, pending);
            continue;
        }

        let trimmed = raw.trim();
        if trimmed.is_empty() {
            out.push(String::new());
            continue;
        }

        let (delta, next_pending) = scan(trimmed);
        let indent = (depth - delta.leading_close as i32).max(0) as usize;
        out.push(unit.repeat(indent) + trimmed);
        depth = (depth + delta.net).max(0);
        pending = next_pending;
    }

    out.join("\n")
}

/// The indent level a new line should get, given the line it follows and the
/// level that line ended up on. Used for auto-indent when Enter is pressed,
/// where re-running the whole document would be both wasteful and disruptive to
/// the caret.
pub fn indent_after(line: &str, current_indent: usize) -> usize {
    let (delta, _) = scan(line.trim());
    if delta.net > 0 {
        current_indent + delta.net as usize
    } else {
        current_indent
    }
}

/// How many `unit`s of indentation a line already has.
pub fn indent_of(line: &str, unit: &str) -> usize {
    if unit.is_empty() {
        return 0;
    }
    let leading: String = line.chars().take_while(|c| *c == ' ' || *c == '\t').collect();
    leading.len() / unit.len()
}

fn close_pending(line: &str, pending: Pending) -> Pending {
    let closer = match pending {
        Pending::Html => "-->",
        Pending::Block => "*/",
        Pending::None => return Pending::None,
    };
    if line.contains(closer) {
        Pending::None
    } else {
        pending
    }
}

/// Walk a line once, counting nesting outside strings and comments.
fn scan(line: &str) -> (Delta, Pending) {
    let b = line.as_bytes();
    let mut i = 0usize;
    let mut net = 0i32;
    let mut leading_close = 0usize;
    let mut seen_non_close = false;
    let mut pending = Pending::None;

    // Counting a closer only while nothing else has been seen is what makes
    // `</view>` dedent its own line but `<a></a>` not.
    let close = |net: &mut i32, leading_close: &mut usize, seen: &mut bool| {
        *net -= 1;
        if !*seen {
            *leading_close += 1;
        }
    };

    while i < b.len() {
        match b[i] {
            // Strings: skip wholesale, so braces inside them never count.
            b'"' | b'\'' => {
                let quote = b[i];
                i += 1;
                while i < b.len() {
                    if b[i] == b'\\' {
                        i += 2;
                        continue;
                    }
                    if b[i] == quote {
                        i += 1;
                        break;
                    }
                    i += 1;
                }
            }
            b'/' if b.get(i + 1) == Some(&b'/') => break, // line comment: done
            b'/' if b.get(i + 1) == Some(&b'*') => {
                match find(b, i + 2, b"*/") {
                    Some(end) => i = end + 2,
                    None => {
                        pending = Pending::Block;
                        break;
                    }
                }
            }
            b'<' if b[i..].starts_with(b"<!--") => match find(b, i + 4, b"-->") {
                Some(end) => i = end + 3,
                None => {
                    pending = Pending::Html;
                    break;
                }
            },
            b'<' if b.get(i + 1) == Some(&b'/') => {
                // `</name>`: a closing tag.
                match find(b, i, b">") {
                    Some(end) => {
                        close(&mut net, &mut leading_close, &mut seen_non_close);
                        i = end + 1;
                    }
                    None => break,
                }
            }
            b'<' if b.get(i + 1).is_some_and(|c| c.is_ascii_alphabetic()) => {
                let Some(end) = find(b, i, b">") else { break };
                let name_end = i + 1
                    + b[i + 1..end]
                        .iter()
                        .position(|c| !(c.is_ascii_alphanumeric() || *c == b'.' || *c == b'-' || *c == b'_'))
                        .unwrap_or(end - i - 1);
                let name = &line[i + 1..name_end];
                let self_closing = b[..end].ends_with(b"/");
                if !self_closing && !VOID_TAGS.iter().any(|v| v.eq_ignore_ascii_case(name)) {
                    net += 1;
                }
                seen_non_close = true;
                i = end + 1;
            }
            b'{' | b'[' | b'(' => {
                net += 1;
                seen_non_close = true;
                i += 1;
            }
            b'}' | b']' | b')' => {
                close(&mut net, &mut leading_close, &mut seen_non_close);
                i += 1;
            }
            _ => i += 1,
        }
    }

    (Delta { net, leading_close }, pending)
}

/// Byte-substring search from `from`.
pub(crate) fn find(haystack: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    if from > haystack.len() {
        return None;
    }
    haystack[from..]
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|p| p + from)
}
