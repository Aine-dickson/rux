//! Retired spellings become the ones the language has now, so a file has one
//! spelling of each thing:
//!
//! - `#{ a: 1 }` becomes `{ a: 1 }`, below.
//! - `null` becomes `none`, and so does `()` where it is a value (`signal(())`,
//!   `x = ()`), not a call's empty parentheses or an arrow's: step 3 of
//!   `docs/11-next.md`. Both old spellings still read as `none`, with a
//!   warning, so nothing breaks if this pass never runs.
//!
//! On maps:
//!
//! `{` is what a web author writes, and the script engine reads a `{` as a map
//! when it is followed by `name:` or `"name":` (see
//! `crates/rux-rhai/DIVERGENCE.md`). `#{` is rhai's spelling and still parses,
//! so nothing breaks if this pass never runs; it only makes the two agree.
//!
//! Only script is touched: the `<script>` section, the values of bound
//! attributes (`:x`, `@x`, `r-x`), and the inside of `{{ }}`. Prose in a
//! `<text>` that happens to say `#{` is left alone, as are strings and comments
//! in the script.
//!
//! A `#{` is rewritten only where the `{` would read as the same map:
//!
//! - followed by a key and a single `:`, which is a map wherever it stands;
//! - an empty `#{}` only after `=`, `(`, `[`, `,`, `:` or `return`, where it is
//!   plainly a value. At the start of a statement, or as an arrow's body, `{}`
//!   is an empty block, so `#{}` stays there.

/// Rewrite every retired spelling in a whole `.rux` file.
pub fn rewrite(text: &str) -> String {
    let b = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < b.len() {
        let rest = &text[i..];
        if rest.starts_with("<!--") {
            let end = rest.find("-->").map_or(text.len(), |e| i + e + 3);
            out.push_str(&text[i..end]);
            i = end;
        } else if rest.starts_with("<style") {
            let end = rest.find("</style>").map_or(text.len(), |e| i + e);
            out.push_str(&text[i..end]);
            i = end;
        } else if rest.starts_with("<script") && is_tag_name_end(b.get(i + 7)) {
            let open = match rest.find('>') {
                Some(e) => i + e + 1,
                None => text.len(),
            };
            let close = text[open..].find("</script>").map_or(text.len(), |e| open + e);
            out.push_str(&text[i..open]);
            out.push_str(&code(&text[open..close]));
            i = close;
        } else if b[i] == b'<' && b.get(i + 1).is_some_and(|c| c.is_ascii_alphabetic()) {
            i = tag(text, i, &mut out);
        } else if rest.starts_with("{{") {
            let inner = i + 2;
            let end = interpolation_end(&text[inner..]).map_or(text.len(), |e| inner + e);
            out.push_str("{{");
            out.push_str(&code(&text[inner..end]));
            i = end;
        } else {
            let c = rest.chars().next().unwrap();
            out.push(c);
            i += c.len_utf8();
        }
    }
    out
}

fn is_tag_name_end(c: Option<&u8>) -> bool {
    matches!(c, Some(b'>' | b' ' | b'\t' | b'\r' | b'\n' | b'/'))
}

/// Copy the start tag at `start` into `out`, rewriting the values of bound
/// attributes, and return where the tag ends.
fn tag(text: &str, start: usize, out: &mut String) -> usize {
    let b = text.as_bytes();
    let mut i = start + 1;
    while i < b.len() && !matches!(b[i], b'>' | b' ' | b'\t' | b'\r' | b'\n' | b'/') {
        i += 1;
    }
    out.push_str(&text[start..i]);
    while i < b.len() {
        match b[i] {
            b'>' => {
                out.push('>');
                return i + 1;
            }
            c if c.is_ascii_whitespace() || c == b'/' => {
                out.push(c as char);
                i += 1;
            }
            _ => {
                let name_start = i;
                while i < b.len() && !matches!(b[i], b'=' | b'>' | b'/') && !b[i].is_ascii_whitespace() {
                    i += 1;
                }
                let name = &text[name_start..i];
                out.push_str(name);
                if b.get(i) != Some(&b'=') {
                    continue;
                }
                out.push('=');
                i += 1;
                let Some(&quote @ (b'"' | b'\'')) = b.get(i) else { continue };
                let value_start = i + 1;
                let value_end = text[value_start..].find(quote as char).map_or(text.len(), |e| value_start + e);
                let value = &text[value_start..value_end];
                out.push(quote as char);
                if name.starts_with([':', '@']) || name.starts_with("r-") {
                    out.push_str(&code(value));
                } else {
                    out.push_str(value);
                }
                if value_end < text.len() {
                    out.push(quote as char);
                }
                i = (value_end + 1).min(text.len());
            }
        }
    }
    text.len()
}

/// Where the `}}` closing an interpolation starts, counting braces so a map
/// inside it does not end it. The same rule the runtime uses.
fn interpolation_end(after: &str) -> Option<usize> {
    let b = after.as_bytes();
    let mut depth = 0usize;
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            q @ (b'"' | b'\'' | b'`') => i = string_end(b, i, q),
            b'{' => depth += 1,
            b'}' if depth > 0 => depth -= 1,
            b'}' if b.get(i + 1) == Some(&b'}') => return Some(i),
            _ => (),
        }
        i += 1;
    }
    None
}

/// The index of the quote closing the string opened at `start`, or the end.
fn string_end(b: &[u8], start: usize, quote: u8) -> usize {
    let mut i = start + 1;
    while i < b.len() && b[i] != quote {
        i += if b[i] == b'\\' { 2 } else { 1 };
    }
    i.min(b.len())
}

/// Rewrite the retired spellings in a piece of script.
fn code(src: &str) -> String {
    let b = src.as_bytes();
    let mut out = String::with_capacity(src.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            q @ (b'"' | b'\'' | b'`') => {
                let end = (string_end(b, i, q) + 1).min(b.len());
                out.push_str(&src[i..end]);
                i = end;
            }
            b'/' if b.get(i + 1) == Some(&b'/') => {
                let end = src[i..].find('\n').map_or(b.len(), |e| i + e);
                out.push_str(&src[i..end]);
                i = end;
            }
            b'/' if b.get(i + 1) == Some(&b'*') => {
                let end = src[i + 2..].find("*/").map_or(b.len(), |e| i + 2 + e + 2);
                out.push_str(&src[i..end]);
                i = end;
            }
            b'#' if b.get(i + 1) == Some(&b'{') && can_drop_hash(src, i + 2, &out) => {
                out.push('{');
                i += 2;
            }
            c if (c.is_ascii_alphabetic() || c == b'_') => {
                let end = src[i..].find(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).map_or(b.len(), |e| i + e);
                let word = &src[i..end];
                // Not `x.null`, a field that happens to have the name.
                if word == "null" && !out.trim_end().ends_with('.') {
                    out.push_str("none");
                } else {
                    out.push_str(word);
                }
                i = end;
            }
            b'(' if b.get(i + 1) == Some(&b')') && unit_is_a_value(&out, &src[i + 2..]) => {
                out.push_str("none");
                i += 2;
            }
            _ => {
                let c = src[i..].chars().next().unwrap();
                out.push(c);
                i += c.len_utf8();
            }
        }
    }
    out
}

/// Whether a `()` with `before` written ahead of it and `after` following is
/// the empty value, rather than a call's parentheses (`f()`, after a name) or
/// an arrow's (`() => x`).
fn unit_is_a_value(before: &str, after: &str) -> bool {
    if after.trim_start().starts_with("=>") {
        return false;
    }
    let before = before.trim_end();
    before.ends_with(['=', '(', '[', ',', ':', '>', '?', '{', ';'])
        || before.is_empty()
        || before.strip_suffix("return").is_some_and(|r| !r.ends_with(|c: char| c.is_ascii_alphanumeric() || c == '_'))
}

/// Whether the map whose contents start at `at` reads the same without its `#`.
/// `before` is everything already written, to see what position it stands in.
fn can_drop_hash(src: &str, at: usize, before: &str) -> bool {
    let b = src.as_bytes();
    let mut i = at;
    while i < b.len() && b[i].is_ascii_whitespace() {
        i += 1;
    }
    match b.get(i) {
        Some(b'}') => {
            let before = before.trim_end();
            if before.ends_with("=>") {
                return false;
            }
            before.ends_with(['=', '(', '[', ',', ':'])
                || before.strip_suffix("return").is_some_and(|r| {
                    !r.ends_with(|c: char| c.is_ascii_alphanumeric() || c == '_')
                })
        }
        Some(b'"') => {
            let end = string_end(b, i, b'"');
            end < b.len() && colon_follows(b, end + 1)
        }
        Some(c) if c.is_ascii_alphabetic() || *c == b'_' => {
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                i += 1;
            }
            colon_follows(b, i)
        }
        _ => false,
    }
}

/// A single `:` next, not `::`.
fn colon_follows(b: &[u8], mut i: usize) -> bool {
    while i < b.len() && b[i].is_ascii_whitespace() {
        i += 1;
    }
    b.get(i) == Some(&b':') && b.get(i + 1) != Some(&b':')
}

#[cfg(test)]
mod tests {
    use super::rewrite as bare_maps;

    #[test]
    fn null_and_a_unit_value_become_none() {
        let src = "<script>\nlet a: string | null = null;\nlet b = signal(());\nlet c = () => f();\nx.null = g(null, ());\nlet s = \"null\"; // null\n</script>\n<template><view :x=\"a ?? null\" y=\"null\" /></template>";
        assert_eq!(
            bare_maps(src),
            "<script>\nlet a: string | none = none;\nlet b = signal(none);\nlet c = () => f();\nx.null = g(none, none);\nlet s = \"null\"; // null\n</script>\n<template><view :x=\"a ?? none\" y=\"null\" /></template>"
        );
        assert_eq!(bare_maps(&bare_maps(src)), bare_maps(src));
    }

    #[test]
    fn keyed_maps_lose_their_hash() {
        let src = "<script>\nlet m = #{ a: 1, \"b-c\": #{ d: 2 } };\nrows.map(r => #{ id: r.id });\n</script>";
        assert_eq!(
            bare_maps(src),
            "<script>\nlet m = { a: 1, \"b-c\": { d: 2 } };\nrows.map(r => { id: r.id });\n</script>"
        );
    }

    #[test]
    fn a_multi_line_map_loses_its_hash() {
        let src = "<script>\nlet m = [\n  #{\n    id: 1\n  }\n];\n</script>";
        assert_eq!(bare_maps(src), "<script>\nlet m = [\n  {\n    id: 1\n  }\n];\n</script>");
    }

    #[test]
    fn empty_maps_only_where_they_are_values() {
        let src = "<script>\nlet a = #{};\nf(#{ }, #{});\nlet g = () => #{};\n#{};\nfn h() { return #{}; }\n</script>";
        assert_eq!(
            bare_maps(src),
            "<script>\nlet a = {};\nf({ }, {});\nlet g = () => #{};\n#{};\nfn h() { return {}; }\n</script>"
        );
    }

    #[test]
    fn bound_attributes_and_interpolations_are_script() {
        let src = "<template><view class=\"#{ x: 1 }\" :class='#{ here: route == \"/\" }' @tap=\"go(#{ id: 1 })\"><text>Say #{ a: 1 } and {{ #{ a: #{ b: 1 } }.a.b }}</text></view></template>";
        assert_eq!(
            bare_maps(src),
            "<template><view class=\"#{ x: 1 }\" :class='{ here: route == \"/\" }' @tap=\"go({ id: 1 })\"><text>Say #{ a: 1 } and {{ { a: { b: 1 } }.a.b }}</text></view></template>"
        );
    }

    #[test]
    fn strings_comments_and_paths_are_left() {
        let src = "<script>\nlet s = \"#{ a: 1 }\"; // #{ a: 1 }\n/* #{ a: 1 } */\nlet t = #{ host::x };\n</script>\n<style>.a { color: red; }</style>";
        assert_eq!(bare_maps(src), src);
    }

    #[test]
    fn idempotent() {
        let src = "<template><view :class=\"#{ a: b }\" /></template>\n<script>let m = #{ a: 1 };</script>";
        let once = bare_maps(src);
        assert_eq!(bare_maps(&once), once);
    }
}
