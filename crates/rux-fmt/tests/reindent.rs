//! Re-indenter behaviour, mostly as invariants.
//!
//! Snapshot tests would lock in today's output; these lock in the properties the
//! playground's Format button actually depends on, that it never loses or
//! rewrites content, and that pressing it twice is the same as pressing it once.

use rux_fmt::{indent_after, indent_of, reindent};

const UNIT: &str = "  ";

/// Outside `<style>`, formatting must only ever move leading whitespace. If a
/// line's trimmed content changes, the button ate someone's code.
#[test]
fn template_and_script_keep_their_lines() {
    let src = "<template>\n<view class=\"a\">\n<text>hi</text>\n</view>\n</template>\n\n<script>\nlet n = signal(0);\n</script>\n";
    let out = reindent(src, UNIT);

    let before: Vec<&str> = src.lines().map(str::trim).collect();
    let after: Vec<&str> = out.lines().map(str::trim).collect();
    assert_eq!(before, after, "content changed, not just indentation");
}

/// The CSS formatter *does* rewrite lines, so line equality no longer holds. The
/// invariant that replaces it: ignoring whitespace and semicolons, the document
/// is unchanged. Semicolons are excluded because the formatter adds the optional
/// one to a block's last declaration; nothing else may appear or vanish.
fn skeleton(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace() && *c != ';').collect()
}

#[test]
fn formatting_never_adds_or_loses_content() {
    let src = "<template>\n<view>x</view>\n</template>\n<style>\n.a{color:red;background:blue;gap:1px;padding:2px}\n.b { color : green }\n</style>\n";
    assert_eq!(skeleton(&reindent(src, UNIT)), skeleton(src));
}

#[test]
fn nests_tags_and_dedents_closers() {
    let src = "<template>\n<view>\n<text>hi</text>\n</view>\n</template>\n";
    let out = reindent(src, UNIT);
    let lines: Vec<&str> = out.lines().collect();

    assert_eq!(lines[0], "<template>");
    assert_eq!(lines[1], "  <view>");
    assert_eq!(lines[2], "    <text>hi</text>", "a tag opened and closed on one line is flat");
    assert_eq!(lines[3], "  </view>");
    assert_eq!(lines[4], "</template>");
}

/// Running it twice must equal running it once, or the button would walk the
/// document sideways every press.
#[test]
fn is_idempotent() {
    let src = "<template>\n  <view>\n<text>hi</text>\n      </view>\n</template>\n\n<style>\n.a { color: red; }\n.b {\ngap: 8px;\n}\n</style>\n";
    let once = reindent(src, UNIT);
    let twice = reindent(&once, UNIT);
    assert_eq!(once, twice);
}

/// `<input />` and friends do not nest.
#[test]
fn void_and_self_closing_tags_do_not_nest() {
    let src = "<view>\n<input r-model=\"a\" />\n<image src=\"x.png\">\n<text>after</text>\n</view>\n";
    let out = reindent(src, UNIT);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines[1], "  <input r-model=\"a\" />");
    assert_eq!(lines[2], "  <image src=\"x.png\">", "a void tag must not open a level");
    assert_eq!(lines[3], "  <text>after</text>", "content after a void tag stays put");
}

/// Braces and angle brackets inside strings are text, not nesting. `level < 20`
/// is the case that bites: a naive scanner reads `< 2` as a tag.
#[test]
fn brackets_inside_strings_are_ignored() {
    let src = "<view>\n<text r-if=\"level < 20\">{ not a block }</text>\n<text @tap='name = \"{{{\"'>x</text>\n</view>\n";
    let out = reindent(src, UNIT);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines[1], "  <text r-if=\"level < 20\">{ not a block }</text>");
    assert_eq!(lines[2], "  <text @tap='name = \"{{{\"'>x</text>");
    assert_eq!(lines[3], "</view>");
}

/// A multi-line comment keeps the author's alignment, and its contents never
/// affect nesting.
#[test]
fn multiline_comments_are_left_alone() {
    let src = "<style>\n/* a comment\n     with { braces } and odd    alignment\n */\n.a { color: red; }\n</style>\n";
    let out = reindent(src, UNIT);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines[2], "     with { braces } and odd    alignment", "comment body was reflowed");
    assert_eq!(lines[4], "  .a { color: red; }", "nesting drifted through the comment");
}

#[test]
fn line_endings_are_preserved() {
    assert!(reindent("<a>\r\n<b>\r\n", UNIT).contains("\r\n"), "CRLF became LF");
    assert!(!reindent("<a>\n<b>\n", UNIT).contains('\r'), "LF gained a CR");
}

#[test]
fn blank_lines_lose_trailing_space() {
    let out = reindent("<view>\n   \n</view>\n", UNIT);
    assert_eq!(out.lines().nth(1), Some(""), "a whitespace-only line should be emptied");
}

/// What auto-indent on Enter uses.
#[test]
fn indent_after_opens_a_level_only_when_the_line_opens_one() {
    assert_eq!(indent_after("<view>", 1), 2, "an open tag indents the next line");
    assert_eq!(indent_after("<text>hi</text>", 1), 1, "a balanced line does not");
    assert_eq!(indent_after("<input />", 1), 1, "self-closing does not");
    assert_eq!(indent_after("</view>", 1), 1, "a closer does not indent further");
    assert_eq!(indent_after(".a {", 0), 1, "a CSS block opens a level");
}

#[test]
fn indent_of_counts_units() {
    assert_eq!(indent_of("    <view>", UNIT), 2);
    assert_eq!(indent_of("<view>", UNIT), 0);
    assert_eq!(indent_of("", UNIT), 0);
}

/// The shipped examples are hand-formatted and already sane; re-indenting must
/// not mangle them. This is the closest thing to a real-world corpus available.
#[test]
fn examples_survive_reindenting() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let mut checked = 0;
    for entry in std::fs::read_dir(&dir).expect("examples dir").flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "rux") {
            continue;
        }
        let src = std::fs::read_to_string(&path).expect("read");
        let out = reindent(&src, UNIT);

        assert_eq!(skeleton(&out), skeleton(&src), "{} lost content", path.display());
        assert_eq!(reindent(&out, UNIT), out, "{} is not idempotent", path.display());
        checked += 1;
    }
    assert!(checked > 5, "expected several examples, checked {checked}");
}

/// An apostrophe in ordinary prose is not the start of a string.
///
/// It used to open one, which ran to the end of the line and swallowed the
/// `</text>` that closed the element, so every line below was indented one
/// level deeper and the next apostrophe added another. One "don't" in a
/// paragraph re-indented the rest of the file, and `rux fmt` writes in place.
#[test]
fn an_apostrophe_in_text_does_not_open_a_string() {
    let src = "<template>\n<screen>\n<view>\n<text>the dog's bowl</text>\n<text>after</text>\n</view>\n</screen>\n</template>\n";
    let out = reindent(src, UNIT);
    let lines: Vec<&str> = out.lines().collect();

    assert_eq!(indent_of(lines[3], UNIT), 3, "the text sits inside screen > view");
    assert_eq!(
        indent_of(lines[4], UNIT),
        indent_of(lines[3], UNIT),
        "and its sibling sits beside it, not below it"
    );
    assert_eq!(indent_of(lines[5], UNIT), 2, "</view> closes back to screen's level");
}

/// A quoted string that *does* close is still skipped whole, which is what
/// stops a brace inside one from counting as nesting.
#[test]
fn a_closed_string_still_hides_what_is_inside_it() {
    let src = "<template>\n<screen>\n<text>a { not a block</text>\n</screen>\n</template>\n";
    let with_braces_in_a_string =
        "<script>\nlet s = \"a { brace\";\nlet t = 1;\n</script>\n";
    let out = reindent(src, UNIT);
    assert_eq!(indent_of(out.lines().nth(2).unwrap(), UNIT), 2, "{out}");

    let out = reindent(with_braces_in_a_string, UNIT);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(
        indent_of(lines[1], UNIT),
        indent_of(lines[2], UNIT),
        "the brace inside the string opened nothing: {out}"
    );
}

/// A start tag whose attributes run onto the next line opens a level like any
/// other, and its attribute lines keep the author's alignment.
///
/// Before this the tag was not counted at all: its children sat level with it
/// and every closer after it dedented one too far, so `examples/components/
/// crew_list.rux` came out of `rux fmt` with a `</view>` at the left margin.
#[test]
fn a_start_tag_over_two_lines_opens_a_level() {
    let src = "<template>\n  <view class=\"rows\">\n    <view class=\"row\" r-for=\"m in crew\"\n          :to=\"path_for(&quot;x&quot;, #{ id: m.id })\">\n      <text>{{ m.name }}</text>\n    </view>\n  </view>\n</template>\n";
    let out = reindent(src, UNIT);
    assert_eq!(out, src, "already formatted, so nothing moves");

    // A self-closing one gives its level back.
    let src = "<template>\n  <view>\n    <input r-model=\"a\"\n           placeholder=\"b\" />\n    <text>after</text>\n  </view>\n</template>\n";
    assert_eq!(reindent(src, UNIT), src);

    // And a `>` inside a quoted value does not end the tag early.
    let src = "<template>\n  <view>\n    <view :show=\"n > 2\"\n          class=\"c\">\n      <text>in</text>\n    </view>\n  </view>\n</template>\n";
    assert_eq!(reindent(src, UNIT), src);
}

/// A wrapped expression keeps its second line's offset from the first, so it
/// does not read as a statement of its own.
#[test]
fn a_wrapped_expression_keeps_its_continuation() {
    let src = "<script>\n  fn measure() {\n    report = \"a \" + w +\n             \" at \" + x;\n    done = true;\n  }\n</script>\n";
    assert_eq!(reindent(src, UNIT), src);
    // Written flush, it gets one level.
    let flush = "<script>\n  fn f() {\n    total = a +\n    b;\n  }\n</script>\n";
    assert_eq!(
        reindent(flush, UNIT),
        "<script>\n  fn f() {\n    total = a +\n      b;\n  }\n</script>\n"
    );
    // And a statement after an ordinary one is not moved.
    let plain = "<script>\n  let a = 1;\n  let b = 2;\n</script>\n";
    assert_eq!(reindent(plain, UNIT), plain);
}

/// Two brackets opened on one line are one level, as `signal([` is written
/// everywhere, and the level goes when both close.
#[test]
fn two_brackets_opened_on_one_line_are_one_level() {
    let src = "<script>\n  let rows = signal([\n    #{ name: \"a\" },\n    #{ name: \"b\" },\n  ]);\n  let after = 1;\n</script>\n";
    assert_eq!(reindent(src, UNIT), src);
    assert_eq!(indent_after("let rows = signal([", 1), 2, "Enter steps in once");
}
