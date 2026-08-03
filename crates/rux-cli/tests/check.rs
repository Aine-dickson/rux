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
#[test]
fn components_are_skipped_when_walking_but_not_when_named() {
    let dir = fixture(
        "components",
        &[
            ("app.rux", GOOD),
            (
                "components/row.rux",
                r#"<template><view><text>{{ label }}</text></view></template>"#,
            ),
        ],
    );
    let path = dir.to_str().unwrap();

    let walked = check(&[path]);
    assert!(walked.status.success());
    assert_eq!(stdout(&walked), "", "walking must not report the component");

    let named = check(&[dir.join("components/row.rux").to_str().unwrap()]);
    assert!(
        stdout(&named).contains("label"),
        "naming a component explicitly should still check it: {}",
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
