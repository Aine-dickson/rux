//! Formatting for `.rux` files.
//!
//! Two different jobs, deliberately:
//!
//! - **`<template>` and `<script>` are only re-indented.** Leading whitespace is
//!   corrected; nothing on a line is rewritten, wrapped or reordered. A `@tap`
//!   handler is rhai, and rearranging someone's expressions is not this tool's
//!   business. The one exception is [`spellings`]: where the language has
//!   retired a spelling, the old one is rewritten to the new, so `#{ a: 1 }`
//!   loses its `#` and `null` becomes `none`.
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
//! JS still has the `<image>` bug described on [`void_tags`] below.
//!
//! Where the indenter differs from the JS: the JS blanks strings and comments
//! with a chain of regexes and then re-scans. This walks each line once as a
//! small state machine, which gets multi-line comments right as a consequence
//! rather than as a separate pass.

pub mod css;
pub mod spellings;

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
    /// A start tag whose attributes run onto the next line, closed by its
    /// `>`. `counted` is whether the tag was counted as opening a level, which
    /// is undone if it turns out to close itself with `/>`.
    Tag { counted: bool },
}

/// Tags that never nest, so an opening tag must not increase the indent.
///
/// The list lives in `rux-parser`, which is the crate that decides what never
/// nests: a void tag closes itself there, so an `<input type="text">` written
/// without a slash parses. It used to live here, and the copy in the editor's
/// JavaScript inherited HTML's set, which has `img` but not Rux's `<image>` —
/// so an `<image src="...">` over-indented everything after it. One list, read
/// from one place, is the fix for that; re-exported rather than re-declared so
/// `rux vocab` and the editor keep reading it through the formatter.
pub use rux_parser::void_tags;

/// Case-insensitively, because pasted markup is common and `<BR>` should not
/// indent the rest of a file.
fn is_void(tag: &str) -> bool {
    void_tags().iter().any(|v| v.eq_ignore_ascii_case(tag))
}

/// Format `text`, using `unit` for one indent level (`"  "`, `"    "`, `"\t"`…).
///
/// The `<template>` and `<script>` sections are only re-indented, nothing on a
/// line is rewritten except a retired spelling ([`spellings`]). The
/// `<style>` section goes through the CSS pretty-printer
/// in [`css`], which does reflow declarations. That split is deliberate: CSS has
/// a shape worth enforcing, while a `@tap` handler is rhai that only its author
/// should be rearranging.
///
/// Line endings are preserved: CRLF in, CRLF out.
pub fn reindent(text: &str, unit: &str) -> String {
    let eol = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let normalised = spellings::rewrite(&text.replace("\r\n", "\n"));
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
    // One entry per level of indentation, holding how many brackets and tags
    // the line that opened it left open. A line opening two at once, as
    // `signal([` does, is one level: its contents sit one step in, as every
    // other formatter puts them, and the level goes when both are closed.
    let mut levels: Vec<i32> = Vec::new();
    let mut pending = Pending::None;
    let mut in_script = false;
    let mut previous: Option<Previous> = None;

    for raw in text.split('\n') {
        // The attributes of a start tag written over several lines. They keep
        // the author's alignment, as a comment does: lining them up under the
        // first attribute is a choice, and the tag's own line already carries
        // the indent. What follows the tag's `>` on the same line counts as
        // usual.
        if let Pending::Tag { counted } = pending {
            out.push(raw.to_string());
            let trimmed = raw.trim();
            let Some(end) = tag_end(trimmed.as_bytes()) else { continue };
            if counted && trimmed[..end].ends_with('/') {
                close_levels(&mut levels, 1);
            }
            let (delta, next_pending) = scan(&trimmed[end + 1..]);
            apply(&mut levels, delta);
            pending = next_pending;
            continue;
        }

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

        if trimmed.starts_with("<script") {
            in_script = true;
        } else if trimmed.starts_with("</script") {
            in_script = false;
        }

        let (delta, next_pending) = scan(trimmed);
        close_levels(&mut levels, delta.leading_close as i32);
        let indent = levels.len();
        let author = raw.len() - raw.trim_start().len();
        // A wrapped expression's next line, in a script: the line before ended
        // on an operator, so this one carries on its statement. Levelled with
        // the statement it read as a new one; it keeps the author's offset
        // from that line instead (lining up under the right-hand side is a
        // choice), and at least one level. A third line keeps its offset from
        // the second rather than stepping in again.
        let lead = match (in_script, &previous) {
            (true, Some(prev)) if prev.open_ended && unit != "\t" => {
                let offset = author.saturating_sub(prev.author);
                let offset = if prev.continued { offset } else { offset.max(unit.len()) };
                " ".repeat(prev.written + offset)
            }
            (true, Some(prev)) if prev.open_ended => unit.repeat(indent + 1),
            _ => unit.repeat(indent),
        };
        let written = lead.len();
        let continued = previous.as_ref().is_some_and(|p| in_script && p.open_ended);
        out.push(lead + trimmed);
        // A section's own tag is never part of an expression.
        let code = in_script && !trimmed.starts_with('<');
        previous = Some(Previous { author, written, open_ended: code && open_ended(trimmed), continued });
        apply(
            &mut levels,
            Delta { net: delta.net + delta.leading_close as i32, leading_close: 0 },
        );
        pending = next_pending;
    }

    out.join("\n")
}

/// Close `n` brackets or tags, dropping each level whose last one closes.
fn close_levels(levels: &mut Vec<i32>, mut n: i32) {
    while n > 0 {
        match levels.last_mut() {
            Some(open) if *open > n => {
                *open -= n;
                n = 0;
            }
            Some(open) => {
                n -= *open;
                levels.pop();
            }
            None => break,
        }
    }
}

/// What a line leaves open or closes, as levels: anything it leaves open is
/// one new level, however many brackets that is.
fn apply(levels: &mut Vec<i32>, delta: Delta) {
    close_levels(levels, delta.leading_close as i32);
    match delta.net.cmp(&0) {
        std::cmp::Ordering::Greater => levels.push(delta.net),
        std::cmp::Ordering::Less => close_levels(levels, -delta.net),
        std::cmp::Ordering::Equal => {}
    }
}

/// The last written line, for [`reindent_lines`] to tell a continuation by.
struct Previous {
    /// Its leading whitespace as the author wrote it, in bytes.
    author: usize,
    /// And as it was written out.
    written: usize,
    /// Whether it ended on an operator, so the next line carries it on.
    open_ended: bool,
    /// Whether it was itself a continuation.
    continued: bool,
}

/// Whether a script line ends partway through an expression: on a binary
/// operator, an assignment or an arrow. `x++` and `x--` end a statement, and a
/// comment is not code.
fn open_ended(line: &str) -> bool {
    let code = match line.find("//") {
        Some(at) => line[..at].trim_end(),
        None => line,
    };
    if code.ends_with("++") || code.ends_with("--") || code.ends_with("*/") {
        return false;
    }
    code.ends_with(['+', '-', '*', '/', '%', '=', '&', '|', '?', ':']) || code.ends_with("=>")
}

/// The indent level a new line should get, given the line it follows and the
/// level that line ended up on. Used for auto-indent when Enter is pressed,
/// where re-running the whole document would be both wasteful and disruptive to
/// the caret.
pub fn indent_after(line: &str, current_indent: usize) -> usize {
    let (delta, _) = scan(line.trim());
    if delta.net > 0 {
        current_indent + 1
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
        Pending::None | Pending::Tag { .. } => return Pending::None,
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
            //
            // **Only when the quote closes on this line.** An apostrophe in
            // ordinary prose is not the start of anything: `<text>the dog's
            // bowl</text>` used to open a string that ran to the end of the
            // line, swallowing the `</text>` that closed the element, so every
            // line below it was indented one level deeper, and the next
            // apostrophe added another. A single word like "don't" in a
            // paragraph re-indented the rest of the file.
            //
            // Nothing is lost by requiring the close: neither rhai nor CSS
            // lets a string span a newline, so an unterminated quote on a line
            // is an apostrophe, not a string.
            b'"' | b'\'' => match string_end(b, i) {
                Some(end) => i = end + 1,
                None => i += 1,
            },
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
                let name_end = i + 1
                    + b[i + 1..]
                        .iter()
                        .position(|c| !(c.is_ascii_alphanumeric() || *c == b'.' || *c == b'-' || *c == b'_'))
                        .unwrap_or(b.len() - i - 1);
                let name = &line[i + 1..name_end];
                // A start tag whose attributes go on past this line: it opens a
                // level now, and whatever closes it is on a later line. Before
                // this the tag was not counted at all, so its children lost a
                // level and every closer after it dedented one too far, down to
                // a `</view>` at the left margin.
                let Some(end) = tag_end(&b[i..]).map(|e| e + i) else {
                    let counted = !is_void(name);
                    if counted {
                        net += 1;
                    }
                    pending = Pending::Tag { counted };
                    break;
                };
                let self_closing = b[..end].ends_with(b"/");
                if !self_closing && !is_void(name) {
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

/// The `>` that ends a tag starting in `b`, skipping quoted attribute values,
/// so a `>` inside `:show="n > 2"` does not end the tag.
fn tag_end(b: &[u8]) -> Option<usize> {
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'"' | b'\'' => match string_end(b, i) {
                Some(end) => i = end + 1,
                None => return None,
            },
            b'>' => return Some(i),
            _ => i += 1,
        }
    }
    None
}

/// Where the string opened at `start` closes, or `None` if it does not close on
/// this line.
///
/// Escapes are honoured, so `"a\"b"` ends at the last quote and not the middle
/// one.
fn string_end(b: &[u8], start: usize) -> Option<usize> {
    let quote = b[start];
    let mut i = start + 1;
    while i < b.len() {
        if b[i] == b'\\' {
            i += 2;
            continue;
        }
        if b[i] == quote {
            return Some(i);
        }
        i += 1;
    }
    None
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
