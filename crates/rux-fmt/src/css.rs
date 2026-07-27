//! A small CSS pretty-printer for the `<style>` section.
//!
//! Unlike the re-indenter in `lib.rs`, this one *does* rewrite lines, it is a
//! formatter, not an aligner. The shape it produces:
//!
//! ```css
//! .title { color: #cdd6f4; font-size: 26px; font-weight: 700; }
//!
//! .card {
//!   display: flex;
//!   flex-direction: column;
//!   gap: 12px;
//!   padding: 16px;
//! }
//! ```
//!
//! - Exactly one space between a selector and its `{`, however the author
//!   aligned them.
//! - A rule with **more than [`INLINE_MAX`] declarations** breaks one per line,
//!   with the closing `}` on its own line. At or under it, the rule stays on one
//!   line, which is what keeps a wall of two-property rules readable.
//! - A rule carrying a comment always breaks, because a `/* … */` shoved into a
//!   one-liner is worse than the line it saved.
//!
//! Declaration *text* is never altered beyond collapsing runs of whitespace:
//! values, colours, `!important` and vendor prefixes pass through untouched.
//! Nesting is handled generically, so `@media` blocks format correctly even
//! though the v0.3 runtime does not honour them yet.

use crate::find;

/// Declarations a rule may hold and still be written on one line.
pub const INLINE_MAX: usize = 3;

#[derive(Debug, PartialEq, Eq)]
enum Item {
    /// `/* … */`, kept verbatim.
    Comment(String),
    /// `color: red`: no trailing semicolon.
    Decl(String),
    /// A selector or at-rule and its block.
    Block { prelude: String, items: Vec<Item> },
    /// `@import …;` and friends: an at-rule with no block.
    AtStatement(String),
}

/// Format a `<style>` body. `level` is the indent depth of the rules themselves.
pub fn format(src: &str, unit: &str, level: usize) -> String {
    let items = parse(src);
    let mut out = String::new();
    emit(&items, &mut out, unit, level);
    out
}

// ── Parsing ──────────────────────────────────────────────────────────────────

fn parse(src: &str) -> Vec<Item> {
    let b = src.as_bytes();
    let mut i = 0usize;
    parse_items(src, b, &mut i)
}

fn parse_items(src: &str, b: &[u8], i: &mut usize) -> Vec<Item> {
    let mut items = Vec::new();
    let mut buf_start = None::<usize>;
    let mut buf_end = 0usize;

    while *i < b.len() {
        match b[*i] {
            b'}' => {
                *i += 1;
                flush(src, &mut items, buf_start.take(), buf_end);
                return items;
            }
            b'/' if b.get(*i + 1) == Some(&b'*') => {
                // A comment between declarations is its own item; one *inside* a
                // declaration's text stays part of it.
                let end = find(b, *i + 2, b"*/").map_or(b.len(), |e| e + 2);
                if buf_start.is_none() {
                    items.push(Item::Comment(src[*i..end].trim().to_string()));
                } else {
                    buf_end = end;
                }
                *i = end;
            }
            b'{' => {
                let prelude = buf_start
                    .take()
                    .map(|s| collapse(&src[s..buf_end]))
                    .unwrap_or_default();
                *i += 1;
                let inner = parse_items(src, b, i);
                items.push(Item::Block { prelude, items: inner });
            }
            b';' => {
                let text = buf_start.take().map(|s| collapse_decl(&src[s..buf_end]));
                *i += 1;
                if let Some(text) = text {
                    if !text.is_empty() {
                        items.push(if text.starts_with('@') {
                            Item::AtStatement(text)
                        } else {
                            Item::Decl(text)
                        });
                    }
                }
            }
            // Skip over strings and parenthesised values so a `;` or `{` inside
            // them cannot end a declaration early.
            b'"' | b'\'' => {
                let quote = b[*i];
                if buf_start.is_none() {
                    buf_start = Some(*i);
                }
                *i += 1;
                while *i < b.len() && b[*i] != quote {
                    *i += if b[*i] == b'\\' { 2 } else { 1 };
                }
                *i = (*i + 1).min(b.len());
                buf_end = *i;
            }
            b'(' => {
                if buf_start.is_none() {
                    buf_start = Some(*i);
                }
                let mut depth = 0usize;
                while *i < b.len() {
                    match b[*i] {
                        b'(' => depth += 1,
                        b')' => {
                            depth -= 1;
                            if depth == 0 {
                                *i += 1;
                                break;
                            }
                        }
                        _ => {}
                    }
                    *i += 1;
                }
                buf_end = *i;
            }
            c if c.is_ascii_whitespace() => *i += 1,
            _ => {
                if buf_start.is_none() {
                    buf_start = Some(*i);
                }
                *i += 1;
                buf_end = *i;
            }
        }
    }

    flush(src, &mut items, buf_start, buf_end);
    items
}

/// A trailing declaration with no semicolon before `}` or EOF.
fn flush(src: &str, items: &mut Vec<Item>, start: Option<usize>, end: usize) {
    if let Some(s) = start {
        let text = collapse_decl(&src[s..end]);
        if !text.is_empty() {
            items.push(Item::Decl(text));
        }
    }
}

/// Collapse whitespace runs to single spaces. Used for selectors and at-rule
/// preludes, where the text is otherwise left exactly as written.
fn collapse(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// As [`collapse`], plus `prop: value` spacing, so `color:red` and
/// `color  :  red` both land on `color: red`.
///
/// Kept separate from [`collapse`] on purpose: a selector is *not* a
/// declaration, and running this over one would rewrite `a:hover` as
/// `a: hover`: which is exactly what it did until a test caught it.
fn collapse_decl(s: &str) -> String {
    let joined = collapse(s);

    // Only the *first* colon, and only when what precedes it is a bare property
    // name, so `background: url(http://x)` keeps its later colons.
    let Some(i) = joined.find(':') else { return joined };
    let (name, rest) = joined.split_at(i);
    let name = name.trim_end();
    let value = rest[1..].trim();
    let is_property =
        !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');

    if is_property && !value.is_empty() {
        format!("{name}: {value}")
    } else {
        joined
    }
}

// ── Emitting ─────────────────────────────────────────────────────────────────

fn emit(items: &[Item], out: &mut String, unit: &str, level: usize) {
    let pad = unit.repeat(level);
    for (n, item) in items.iter().enumerate() {
        match item {
            Item::Comment(text) => {
                push_line(out, &pad, text);
            }
            Item::Decl(text) => {
                push_line(out, &pad, &format!("{text};"));
            }
            Item::AtStatement(text) => {
                push_line(out, &pad, &format!("{text};"));
            }
            Item::Block { prelude, items: inner } => {
                let decls_only = inner.iter().all(|i| matches!(i, Item::Decl(_)));
                let multi = !inner.is_empty() && !(decls_only && inner.len() <= INLINE_MAX);
                // A block that spans lines gets air on both sides, so it reads as
                // a unit rather than merging into the one-liners around it. The
                // `ends_with` guard is what stops two blocks producing a gap of
                // two blank lines between them.
                if multi && !out.is_empty() && !out.ends_with("\n\n") {
                    out.push('\n');
                }
                if inner.is_empty() {
                    push_line(out, &pad, &format!("{prelude} {{}}"));
                } else if decls_only && inner.len() <= INLINE_MAX {
                    let body: Vec<String> = inner
                        .iter()
                        .map(|i| match i {
                            Item::Decl(t) => format!("{t};"),
                            _ => unreachable!("checked decls_only"),
                        })
                        .collect();
                    push_line(out, &pad, &format!("{prelude} {{ {} }}", body.join(" ")));
                } else {
                    push_line(out, &pad, &format!("{prelude} {{"));
                    emit(inner, out, unit, level + 1);
                    push_line(out, &pad, "}");
                }
                if multi && n + 1 < items.len() {
                    out.push('\n');
                }
            }
        }
    }
}

fn push_line(out: &mut String, pad: &str, text: &str) {
    out.push_str(pad);
    out.push_str(text);
    out.push('\n');
}
