//! End-to-end tests for `rux fmt`, driving the real binary.
//!
//! Formatting writes to people's files, so the things worth pinning are the ones
//! that would cost them work: that a second run changes nothing, that `--check`
//! never writes, and that a failure leaves stdout empty rather than half a
//! document for an editor to paste over the buffer.

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

const RUX: &str = env!("CARGO_BIN_EXE_rux");

fn fixture(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/fmt-fixtures")
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

fn fmt(args: &[&str]) -> Output {
    Command::new(RUX).arg("fmt").args(args).output().expect("run rux fmt")
}

fn fmt_stdin(args: &[&str], input: &str) -> Output {
    use std::io::Write;
    let mut child = Command::new(RUX)
        .arg("fmt")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn rux fmt");
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(input.as_bytes())
        .expect("write stdin");
    child.wait_with_output().expect("wait for rux fmt")
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

const MESSY: &str = "<template>\n<screen class=\"a\">\n<text>hi</text>\n</screen>\n</template>\n";

#[test]
fn check_reports_without_writing_and_exits_one() {
    let dir = fixture("check_only", &[("app.rux", MESSY)]);
    let file = dir.join("app.rux");

    let out = fmt(&["--check", dir.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    assert!(stdout(&out).contains("app.rux"), "should name the file: {}", stdout(&out));
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        MESSY,
        "--check must not write"
    );
}

#[test]
fn formatting_in_place_is_idempotent() {
    let dir = fixture("idempotent", &[("app.rux", MESSY)]);
    let file = dir.join("app.rux");

    assert!(fmt(&[dir.to_str().unwrap()]).status.success());
    let once = std::fs::read_to_string(&file).unwrap();
    assert_ne!(once, MESSY, "the first run should have changed something");

    assert!(fmt(&[dir.to_str().unwrap()]).status.success());
    let twice = std::fs::read_to_string(&file).unwrap();
    assert_eq!(once, twice, "a second run must change nothing");

    // And the file it just wrote must satisfy its own checker.
    assert!(fmt(&["--check", dir.to_str().unwrap()]).status.success());
}

/// The bug that motivated moving the formatter behind one command. The VS Code
/// copy inherited HTML's void-tag list, which has `img` but not Rux's `<image>`,
/// so everything after an `<image src>` without a self-closing slash was
/// indented one level too deep.
#[test]
fn an_image_without_a_slash_does_not_indent_what_follows() {
    let source = "<template>\n<screen>\n<image src=\"x.png\">\n<text>after</text>\n</screen>\n</template>\n";
    let out = fmt_stdin(&["-"], source);
    let text = stdout(&out);

    let image = text.lines().find(|l| l.contains("<image")).expect("an image line");
    let after = text.lines().find(|l| l.contains("<text>")).expect("a text line");
    let indent = |l: &str| l.len() - l.trim_start().len();
    assert_eq!(
        indent(image),
        indent(after),
        "`<image>` does not nest, so what follows sits at the same depth:\n{text}"
    );
}

#[test]
fn stdin_goes_to_stdout_and_leaves_no_file_behind() {
    let out = fmt_stdin(&["-"], MESSY);
    assert!(out.status.success());
    let text = stdout(&out);
    assert!(text.contains("  <screen"), "should be indented: {text}");
    assert!(text.starts_with("<template>"), "{text}");
}

#[test]
fn the_indent_unit_is_configurable() {
    let tabs = stdout(&fmt_stdin(&["--indent", "tab", "-"], MESSY));
    assert!(tabs.contains("\t<screen"), "expected tabs: {tabs:?}");

    let four = stdout(&fmt_stdin(&["--indent", "4", "-"], MESSY));
    assert!(four.contains("    <screen"), "expected four spaces: {four:?}");

    let bad = fmt_stdin(&["--indent", "0", "-"], MESSY);
    assert_eq!(bad.status.code(), Some(2), "a bad indent is a usage error");
    assert_eq!(stdout(&bad), "", "a usage error must not emit a document");
}

/// Unlike `check`, formatting a component is perfectly sensible: indentation
/// does not depend on where the props come from.
#[test]
fn components_are_formatted_when_walking() {
    let dir = fixture(
        "components",
        &[("components/row.rux", "<template>\n<view>\n<text>x</text>\n</view>\n</template>\n")],
    );
    let out = fmt(&["--check", dir.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1), "the component should be considered");
    assert!(stdout(&out).contains("row.rux"), "{}", stdout(&out));
}

#[test]
fn usage_errors_are_exit_two() {
    assert_eq!(fmt(&["definitely/not/here"]).status.code(), Some(2));
    // `--stdout` has nowhere to put a second document.
    let dir = fixture("two", &[("a.rux", MESSY), ("b.rux", MESSY)]);
    assert_eq!(fmt(&["--stdout", dir.to_str().unwrap()]).status.code(), Some(2));
}

/// CRLF in, CRLF out: rewriting every line ending of a file on a Windows
/// checkout would turn a formatting run into a whole-file diff.
#[test]
fn line_endings_survive() {
    let crlf = MESSY.replace('\n', "\r\n");
    let out = stdout(&fmt_stdin(&["-"], &crlf));
    assert!(out.contains("\r\n"), "CRLF should be preserved");
    assert!(!out.contains("\n\n"), "and not doubled: {out:?}");
}
