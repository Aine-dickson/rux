//! `rux run` on something that is not there.
//!
//! Bare `rux` defaults to `examples/battery.rux`, a path that only exists
//! inside a checkout of this repository. For everyone who installed from
//! crates.io the first command they typed opened a file watcher on nothing and
//! panicked, so the introduction to Rux was a stack trace.

use std::path::PathBuf;
use std::process::Command;

fn rux() -> PathBuf {
    let mut path = std::env::current_exe().expect("test binary path");
    path.pop();
    if path.ends_with("deps") {
        path.pop();
    }
    path.join(format!("rux{}", std::env::consts::EXE_SUFFIX))
}

fn run_in(dir: &std::path::Path, args: &[&str]) -> (i32, String) {
    let out = Command::new(rux()).args(args).current_dir(dir).output().expect("running rux");
    (out.status.code().unwrap_or(-1), String::from_utf8_lossy(&out.stderr).into_owned())
}

/// A scratch directory with no `examples/` in it, which is what an installed
/// user's working directory looks like.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rux-run-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

#[test]
fn bare_rux_outside_a_checkout_explains_itself() {
    let dir = scratch("bare");
    let (code, stderr) = run_in(&dir, &[]);

    assert_eq!(code, 2, "expected a usage error, not a crash");
    assert!(!stderr.contains("panicked"), "still panicking:\n{stderr}");
    assert!(stderr.contains("does not exist"), "stderr:\n{stderr}");
    assert!(stderr.contains("checkout"), "does not explain why the default is missing");
    assert!(stderr.contains("Usage:"), "the commands are not listed");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_named_file_that_is_missing_is_a_message_not_a_stack_trace() {
    let dir = scratch("missing");
    let (code, stderr) = run_in(&dir, &["run", "nope.rux"]);

    assert_eq!(code, 2);
    assert!(!stderr.contains("panicked"), "still panicking:\n{stderr}");
    assert!(stderr.contains("does not exist"), "stderr:\n{stderr}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_directory_is_not_a_document() {
    let dir = scratch("dir");
    std::fs::create_dir_all(dir.join("somewhere")).unwrap();
    let (code, stderr) = run_in(&dir, &["run", "somewhere"]);

    assert_eq!(code, 2);
    assert!(!stderr.contains("panicked"), "still panicking:\n{stderr}");
    assert!(stderr.contains("is a directory"), "stderr:\n{stderr}");

    let _ = std::fs::remove_dir_all(&dir);
}
