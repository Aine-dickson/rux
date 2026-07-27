//! The CSS pretty-printer's shape rules.
//!
//! These are the ones with a visible right answer, so unlike the re-indenter's
//! invariants they are written as exact expectations.

use rux_fmt::css::{format, INLINE_MAX};

const UNIT: &str = "  ";

fn fmt(src: &str) -> String {
    format(src, UNIT, 0)
}

/// Selector and brace, however the author aligned them.
#[test]
fn exactly_one_space_before_the_brace() {
    assert_eq!(fmt(".app   { color: red; }"), ".app { color: red; }\n");
    assert_eq!(fmt(".app{ color: red; }"), ".app { color: red; }\n");
    assert_eq!(fmt(".a\n  .b\n{ color: red; }"), ".a .b { color: red; }\n");
}

/// At or under the threshold, a rule stays on one line.
#[test]
fn short_rules_stay_inline() {
    assert_eq!(INLINE_MAX, 3, "the tests below assume the documented threshold");
    assert_eq!(
        fmt(".t { color: red; font-size: 2px; font-weight: 700; }"),
        ".t { color: red; font-size: 2px; font-weight: 700; }\n"
    );
}

/// Over it, one declaration per line with the closer on its own line.
#[test]
fn long_rules_break_one_declaration_per_line() {
    let out = fmt(".a { display: flex; gap: 1px; padding: 2px; color: red; }");
    assert_eq!(
        out,
        ".a {\n  display: flex;\n  gap: 1px;\n  padding: 2px;\n  color: red;\n}\n"
    );
}

/// A rule already spread over lines collapses when it is short enough — the
/// formatter decides the shape, not the input.
#[test]
fn shape_comes_from_the_rule_not_the_input() {
    assert_eq!(fmt(".a {\n  color: red;\n  gap: 1px;\n}"), ".a { color: red; gap: 1px; }\n");
}

#[test]
fn declaration_spacing_is_normalised() {
    assert_eq!(fmt(".a { color:red; }"), ".a { color: red; }\n");
    assert_eq!(fmt(".a { color   :   red; }"), ".a { color: red; }\n");
}

/// A missing final semicolon is supplied.
#[test]
fn trailing_semicolon_is_added() {
    assert_eq!(fmt(".a { color: red }"), ".a { color: red; }\n");
}

/// Colons that are not property separators must survive untouched: a pseudo
/// class, an attribute selector, and a value containing a colon.
#[test]
fn selectors_and_urls_are_not_mangled() {
    assert_eq!(fmt("a:hover { color: red; }"), "a:hover { color: red; }\n");
    assert_eq!(fmt("[role=\"heading\"] { color: red; }"), "[role=\"heading\"] { color: red; }\n");
    assert!(fmt(".a { background: url(http://x/y.png); }").contains("url(http://x/y.png)"));
}

/// A semicolon inside a string or parentheses must not end the declaration.
#[test]
fn semicolons_inside_values_do_not_split() {
    let out = fmt(".a { font-family: \"a;b\", sans-serif; color: red; }");
    assert_eq!(out, ".a { font-family: \"a;b\", sans-serif; color: red; }\n");
}

/// A comment forces the rule to break, because a `/* … */` wedged into a
/// one-liner costs more than the line it saves.
#[test]
fn comments_are_kept_and_force_a_break() {
    let out = fmt(".a { /* why */ color: red; }");
    assert_eq!(out, ".a {\n  /* why */\n  color: red;\n}\n");
}

/// Nesting is generic, so `@media` works even though v0.3 does not honour it.
#[test]
fn at_rules_nest() {
    let out = fmt("@media (max-width: 600px) { .a { color: red; gap: 1px; } }");
    assert_eq!(out, "@media (max-width: 600px) {\n  .a { color: red; gap: 1px; }\n}\n");
}

#[test]
fn is_idempotent() {
    let src = ".a{color:red;gap:1px;padding:2px;margin:3px}.b{color:blue}\n/* note */\n@media (min-width: 1px) { .c { a: 1; b: 2; c: 3; d: 4; } }";
    let once = fmt(src);
    assert_eq!(fmt(&once), once, "formatting twice differs from formatting once");
}

/// Indent level nests with the block depth.
#[test]
fn indent_level_is_respected() {
    let out = format(".a { color: red; }", UNIT, 2);
    assert_eq!(out, "    .a { color: red; }\n");
}
