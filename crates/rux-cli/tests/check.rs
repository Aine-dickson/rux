//! End-to-end tests for `rux check`, driving the real binary.
//!
//! The exit code is the contract: CI and an editor both act on it, and it is not
//! observable from a unit test of the formatting functions. So these run the
//! command the way a user does.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// Cargo builds the binary for us and hands over its path.
const RUX: &str = env!("CARGO_BIN_EXE_rux");

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Write `source` to a fresh directory under the target dir and return it.
/// Named per test so parallel runs cannot collide.
fn fixture(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/check-fixtures")
        .join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create fixture dir");
    for (file, source) in files {
        let path = dir.join(file);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create fixture subdir");
        }
        std::fs::write(path, source).expect("write fixture");
    }
    dir
}

fn check(args: &[&str]) -> Output {
    Command::new(RUX)
        .arg("check")
        .args(args)
        .output()
        .expect("run rux check")
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).replace('\\', "/")
}

const GOOD: &str = r#"<template><screen class="a"><text>hi</text></screen></template>
<style>.a { display: flex; }</style>"#;

/// A closing tag that does not match its opening one: the parser knows exactly
/// where this is, so the diagnostic must carry a line and a column.
const BROKEN: &str = r#"<template>
  <screen class="a">
    <text>hi</view>
  </screen>
</template>"#;

/// `float` is parsed and not honored, which is a warning rather than an error:
/// the document still loads and still renders.
const WARNS: &str = r#"<template><screen class="a"><text>hi</text></screen></template>
<style>.a { display: flex; float: left; }</style>"#;

#[test]
fn a_clean_document_says_so_and_exits_zero() {
    let dir = fixture("clean", &[("app.rux", GOOD)]);
    let out = check(&[dir.to_str().unwrap()]);
    assert!(out.status.success(), "expected exit 0, got {:?}", out.status.code());
    assert_eq!(stdout(&out), "", "a clean run prints no diagnostics");
}

#[test]
fn a_parse_error_is_located_and_exits_one() {
    let dir = fixture("broken", &[("app.rux", BROKEN)]);
    let out = check(&[dir.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    let text = stdout(&out);
    assert!(text.contains("app.rux:3:"), "expected a line number in: {text}");
    assert!(text.contains(": error: "), "expected an error severity in: {text}");
}

/// Warnings are not failures by default: a document that renders should not
/// break someone's build over a property Rux has not got to yet.
#[test]
fn warnings_alone_do_not_fail_unless_asked() {
    let dir = fixture("warns", &[("app.rux", WARNS)]);
    let path = dir.to_str().unwrap();

    let out = check(&[path]);
    assert!(out.status.success(), "warnings alone should exit 0");
    assert!(stdout(&out).contains(": warning: "), "{}", stdout(&out));

    let denied = check(&["--deny-warnings", path]);
    assert_eq!(denied.status.code(), Some(1), "--deny-warnings should fail");
}

/// A component's props come from its parent, so checking one standalone would
/// report every prop as undefined. Walking a directory must skip them.
///
/// What makes it a component is that something **uses** it. It used to be that
/// its template root was not `<screen>`, which let a layout choice decide
/// whether a file was ever opened.
#[test]
fn components_are_skipped_when_walking_but_not_when_named() {
    let dir = fixture(
        "components",
        &[
            (
                "app.rux",
                r#"<template><screen class="a"><row :label="greeting" /></screen></template>
<style>.a { display: flex; }</style>
<script>
  use components::row;
  let greeting = signal("hi");
</script>"#,
            ),
            (
                "components/row.rux",
                r#"<template><view><text>{{ label }}</text></view></template>"#,
            ),
        ],
    );
    let path = dir.to_str().unwrap();

    let walked = check(&[path]);
    assert!(walked.status.success(), "{}", stdout(&walked));
    assert_eq!(stdout(&walked), "", "walking must not report the component");

    let named = check(&[dir.join("components/row.rux").to_str().unwrap()]);
    assert!(
        stdout(&named).contains("label"),
        "naming a component explicitly should still check it: {}",
        stdout(&named)
    );
}

/// A file nothing uses is a document, and it is checked like one.
///
/// This is the cost of classifying by who imports what, and it is deliberate:
/// there is nobody to supply a name the file never declares, so a name it never
/// declares is wrong. The old root-tag test filed such a file as a component
/// whatever else was true and never looked at it again.
#[test]
fn a_file_nothing_uses_is_checked_like_a_document() {
    let dir = fixture(
        "unused-component",
        &[
            ("app.rux", GOOD),
            (
                "components/orphan.rux",
                r#"<template><view><text>{{ label }}</text></view></template>"#,
            ),
        ],
    );

    let out = check(&[dir.to_str().unwrap()]);
    assert!(
        stdout(&out).contains("orphan.rux") && stdout(&out).contains("label"),
        "a component with no caller is looked at: {}",
        stdout(&out)
    );
}

/// A page is not a component, and the router is what says so.
///
/// It is also the whole of watchlist item 2: `pages/home.rux` opens with a
/// `<view>`, so it was filed as a component and skipped, although `app.rux`
/// names it under `<route view=>`. The project reported "checked 1 file, no
/// problems found" over a page with a mistake in it.
#[test]
fn a_routed_page_is_checked_through_the_document_that_routes_to_it() {
    let dir = fixture(
        "routed-pages",
        &[
            (
                "app.rux",
                r#"<template><screen class="a">
  <router>
    <route path="/" view="home" />
    <route path="/other" view="other" />
  </router>
</screen></template>
<style>.a { display: flex; }</style>
<script>
  use pages::home;
  use pages::other;
  let tally = signal(3);
</script>"#,
            ),
            // Reads the app's signal, which is exactly what makes it
            // uncheckable on its own.
            ("pages/home.rux", r#"<template><view><text>{{ tally }}</text></view></template>"#),
            // Not the route the document opens at, so nothing ever built it.
            (
                "pages/other.rux",
                r#"<template><view><text>{{ never_declared }}</text></view></template>"#,
            ),
        ],
    );

    let walked = check(&[dir.to_str().unwrap()]);
    assert!(
        stdout(&walked).contains("other.rux") && stdout(&walked).contains("never_declared"),
        "the page behind a second route is built and checked: {}",
        stdout(&walked)
    );
    assert!(
        !stdout(&walked).contains("tally"),
        "and the app's own signal is in scope in the page that reads it: {}",
        stdout(&walked)
    );

    // Naming the page asks the same question and must get the same answer,
    // rather than reporting the app's state as undefined in it (watchlist 12).
    let named = check(&[dir.join("pages/home.rux").to_str().unwrap()]);
    assert!(named.status.success(), "{}", stdout(&named));
    assert!(
        !stdout(&named).contains("tally"),
        "a page named on the command line is checked through its app: {}",
        stdout(&named)
    );
}

/// A file that will not parse is never mistaken for a component and skipped:
/// its parse error is the entire reason to run the checker.
#[test]
fn an_unparseable_file_is_still_reported_when_walking() {
    let dir = fixture("unparseable", &[("app.rux", BROKEN)]);
    let out = check(&[dir.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1), "{}", stdout(&out));
}

#[test]
fn json_output_is_parseable_and_keeps_absent_positions_null() {
    let dir = fixture("json", &[("app.rux", WARNS)]);
    let out = check(&["--format", "json", dir.to_str().unwrap()]);
    let text = String::from_utf8_lossy(&out.stdout);

    // Parsed rather than pattern-matched: the point of this format is that a
    // machine can read it, so the test reads it as a machine would.
    let value: serde_json_lite::Value = serde_json_lite::parse(&text)
        .unwrap_or_else(|e| panic!("not valid JSON ({e}): {text}"));
    let items = value.as_array().expect("top level is an array");
    assert!(!items.is_empty(), "expected at least one diagnostic: {text}");
    for item in items {
        assert!(item.get("file").is_some(), "every diagnostic names a file");
        assert!(item.get("severity").is_some(), "every diagnostic has a severity");
        // Present-and-null, not absent: a consumer should not have to tell the
        // difference between "no position" and "field missing".
        assert!(item.get("line").is_some(), "line is present even when unlocated");
    }
}

#[test]
fn a_missing_path_is_a_usage_error_not_a_finding() {
    let out = check(&["definitely/not/here"]);
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn the_shipped_examples_check_clean() {
    let examples = workspace_root().join("examples");
    let out = check(&["--deny-warnings", examples.to_str().unwrap()]);
    assert!(
        out.status.success(),
        "examples/ must stay clean under `rux check --deny-warnings`:\n{}",
        stdout(&out)
    );
}

/// A very small JSON reader, so the test above can parse rather than
/// pattern-match without the CLI gaining a dependency it does not need.
mod serde_json_lite {
    #[derive(Debug, Clone, PartialEq)]
    pub enum Value {
        Null,
        Bool(bool),
        Number(f64),
        String(String),
        Array(Vec<Value>),
        Object(Vec<(String, Value)>),
    }

    impl Value {
        pub fn as_array(&self) -> Option<&Vec<Value>> {
            match self {
                Value::Array(v) => Some(v),
                _ => None,
            }
        }

        pub fn get(&self, key: &str) -> Option<&Value> {
            match self {
                Value::Object(fields) => fields.iter().find(|(k, _)| k == key).map(|(_, v)| v),
                _ => None,
            }
        }
    }

    pub fn parse(input: &str) -> Result<Value, String> {
        let bytes: Vec<char> = input.chars().collect();
        let mut pos = 0;
        let value = parse_value(&bytes, &mut pos)?;
        skip_ws(&bytes, &mut pos);
        if pos != bytes.len() {
            return Err(format!("trailing input at {pos}"));
        }
        Ok(value)
    }

    fn skip_ws(b: &[char], pos: &mut usize) {
        while *pos < b.len() && b[*pos].is_whitespace() {
            *pos += 1;
        }
    }

    fn parse_value(b: &[char], pos: &mut usize) -> Result<Value, String> {
        skip_ws(b, pos);
        match b.get(*pos) {
            Some('[') => parse_array(b, pos),
            Some('{') => parse_object(b, pos),
            Some('"') => Ok(Value::String(parse_string(b, pos)?)),
            Some('t') => lit(b, pos, "true", Value::Bool(true)),
            Some('f') => lit(b, pos, "false", Value::Bool(false)),
            Some('n') => lit(b, pos, "null", Value::Null),
            Some(_) => parse_number(b, pos),
            None => Err("unexpected end".into()),
        }
    }

    fn lit(b: &[char], pos: &mut usize, word: &str, value: Value) -> Result<Value, String> {
        if b[*pos..].starts_with(word.chars().collect::<Vec<_>>().as_slice()) {
            *pos += word.len();
            Ok(value)
        } else {
            Err(format!("expected {word} at {pos}"))
        }
    }

    fn parse_number(b: &[char], pos: &mut usize) -> Result<Value, String> {
        let start = *pos;
        while *pos < b.len() && (b[*pos].is_ascii_digit() || "+-.eE".contains(b[*pos])) {
            *pos += 1;
        }
        b[start..*pos]
            .iter()
            .collect::<String>()
            .parse()
            .map(Value::Number)
            .map_err(|e| format!("bad number at {start}: {e}"))
    }

    fn parse_string(b: &[char], pos: &mut usize) -> Result<String, String> {
        if b.get(*pos) != Some(&'"') {
            return Err(format!("expected a string at {pos}"));
        }
        *pos += 1;
        let mut out = String::new();
        while let Some(&ch) = b.get(*pos) {
            *pos += 1;
            match ch {
                '"' => return Ok(out),
                '\\' => {
                    let esc = b.get(*pos).copied().ok_or("unterminated escape")?;
                    *pos += 1;
                    out.push(match esc {
                        'n' => '\n',
                        'r' => '\r',
                        't' => '\t',
                        'u' => {
                            let hex: String = b[*pos..*pos + 4].iter().collect();
                            *pos += 4;
                            char::from_u32(
                                u32::from_str_radix(&hex, 16).map_err(|e| e.to_string())?,
                            )
                            .ok_or("bad \\u escape")?
                        }
                        other => other,
                    });
                }
                other => out.push(other),
            }
        }
        Err("unterminated string".into())
    }

    fn parse_array(b: &[char], pos: &mut usize) -> Result<Value, String> {
        *pos += 1; // '['
        let mut items = Vec::new();
        loop {
            skip_ws(b, pos);
            if b.get(*pos) == Some(&']') {
                *pos += 1;
                return Ok(Value::Array(items));
            }
            items.push(parse_value(b, pos)?);
            skip_ws(b, pos);
            match b.get(*pos) {
                Some(',') => *pos += 1,
                Some(']') => {}
                _ => return Err(format!("expected , or ] at {pos}")),
            }
        }
    }

    fn parse_object(b: &[char], pos: &mut usize) -> Result<Value, String> {
        *pos += 1; // '{'
        let mut fields = Vec::new();
        loop {
            skip_ws(b, pos);
            if b.get(*pos) == Some(&'}') {
                *pos += 1;
                return Ok(Value::Object(fields));
            }
            let key = parse_string(b, pos)?;
            skip_ws(b, pos);
            if b.get(*pos) != Some(&':') {
                return Err(format!("expected : at {pos}"));
            }
            *pos += 1;
            fields.push((key, parse_value(b, pos)?));
            skip_ws(b, pos);
            match b.get(*pos) {
                Some(',') => *pos += 1,
                Some('}') => {}
                _ => return Err(format!("expected , or }} at {pos}")),
            }
        }
    }
}

/// Three ways an icon is wrong, all of which draw nothing.
///
/// Together in one fixture because the point is that they are told apart: a
/// typo, artwork that does not exist, and no name at all are different
/// mistakes, and an author who sees only a gap in a row cannot tell which they
/// made. `abacus` is outline-only in Tabler, which is what makes it the fair
/// case for the filled message.
const BAD_ICONS: &str = r#"<template>
  <screen>
    <icon name="heart" size="1em" />
    <icon name="hart" size="1em" />
    <icon name="abacus" variant="filled" size="1em" />
    <icon size="1em" />
  </screen>
</template>"#;

#[test]
fn a_misspelled_icon_is_named_and_located() {
    let dir = fixture("icon-misspelled", &[("app.rux", BAD_ICONS)]);
    let out = check(&[dir.join("app.rux").to_str().unwrap()]);
    let text = stdout(&out);
    assert_eq!(out.status.code(), Some(1), "{text}");
    assert!(text.contains("app.rux:4:"), "no position on the bad name: {text}");
    assert!(text.contains("no icon called `hart`"), "{text}");
}

#[test]
fn a_filled_variant_that_does_not_exist_is_refused_rather_than_swapped() {
    // The design call this protects: a silent fallback to the outline would be
    // the usual case rather than the exception, since about four icons in five
    // are outline only, and nobody would learn their icon was never filled.
    let dir = fixture("icon-filled", &[("app.rux", BAD_ICONS)]);
    let out = check(&[dir.join("app.rux").to_str().unwrap()]);
    let text = stdout(&out);
    assert!(text.contains("app.rux:5:"), "no position on the filled variant: {text}");
    assert!(text.contains("`abacus` has no filled artwork"), "{text}");
    assert!(text.contains("does not fall back"), "{text}");
}

#[test]
fn an_icon_with_no_name_says_so() {
    let dir = fixture("icon-nameless", &[("app.rux", BAD_ICONS)]);
    let out = check(&[dir.join("app.rux").to_str().unwrap()]);
    let text = stdout(&out);
    assert!(text.contains("app.rux:6:"), "no position on the nameless icon: {text}");
    assert!(text.contains("needs a `name`"), "{text}");
}

#[test]
fn icons_that_are_right_are_silent() {
    // The half that matters as much: `heart` exists and has filled artwork, so
    // neither form may be reported. A checker that cried about valid icons
    // would be turned off within a day.
    const GOOD_ICONS: &str = r#"<template>
  <screen>
    <icon name="heart" size="1em" />
    <icon name="heart" variant="filled" size="24px" />
    <icon name="circle-check" />
  </screen>
</template>"#;
    let dir = fixture("icon-good", &[("app.rux", GOOD_ICONS)]);
    let out = check(&[dir.join("app.rux").to_str().unwrap()]);
    let text = stdout(&out);
    assert_eq!(out.status.code(), Some(0), "{text}");
    // A clean run prints no diagnostics; the summary goes to stderr.
    assert_eq!(text, "", "valid icons were reported: {text}");
}

#[test]
fn a_bound_name_is_not_reported_because_it_is_not_known_yet() {
    // `:name` is resolved when the app runs, so there is nothing to check here.
    // Reporting it would make the binding unusable; the build says separately
    // that it costs tree-shaking.
    const BOUND: &str = r#"<template>
  <screen>
    <icon :name="chosen" size="1em" />
  </screen>
</template>
<script>
  let chosen = signal("heart");
</script>"#;
    let dir = fixture("icon-bound", &[("app.rux", BOUND)]);
    let out = check(&[dir.join("app.rux").to_str().unwrap()]);
    let text = stdout(&out);
    assert_eq!(out.status.code(), Some(0), "{text}");
}
